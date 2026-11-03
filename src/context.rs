//! Checked, scoped access to one engine session on its owner thread.

use std::cell::RefCell;

pub(crate) struct Slot<T>(RefCell<Option<T>>);

impl<T> Slot<T> {
    pub(crate) const fn new() -> Self {
        Self(RefCell::new(None))
    }

    pub(crate) fn install(&self, value: T) {
        let mut slot = self.0.borrow_mut();
        assert!(slot.is_none(), "engine context is already initialized");
        *slot = Some(value);
    }

    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut slot = self.0.try_borrow_mut().expect(
            "engine context is already borrowed; use the supplied access inside with_internal_gl",
        );
        f(slot
            .as_mut()
            .expect("engine context is not initialized or has shut down"))
    }

    pub(crate) fn take(&self) -> Option<T> {
        self.0.borrow_mut().take()
    }

    #[cfg(test)]
    fn is_initialized(&self) -> bool {
        self.0.borrow().is_some()
    }
}

thread_local! {
    static CONTEXT: Slot<crate::Context> = const { Slot::new() };
}

pub(crate) fn with<R>(f: impl FnOnce(&mut crate::Context) -> R) -> R {
    crate::thread_assert::same_thread();
    CONTEXT.with(|slot| slot.with(f))
}

pub(crate) fn install(context: crate::Context) {
    crate::thread_assert::same_thread();
    CONTEXT.with(|slot| slot.install(context));
}

pub(crate) fn take() -> Option<crate::Context> {
    crate::thread_assert::same_thread();
    CONTEXT.with(Slot::take)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        panic::{catch_unwind, AssertUnwindSafe},
    };

    #[test]
    fn access_requires_installation_and_rejects_nested_borrows() {
        let slot = Slot::new();
        assert!(catch_unwind(AssertUnwindSafe(|| slot.with(|_: &mut usize| ()))).is_err());
        slot.install(1);
        slot.with(|value| {
            assert!(catch_unwind(AssertUnwindSafe(|| slot.with(|_| ()))).is_err());
            *value = 2;
        });
        assert_eq!(slot.take(), Some(2));
        assert!(catch_unwind(AssertUnwindSafe(|| slot.with(|_| ()))).is_err());
    }

    #[test]
    fn unwind_and_sequential_sessions_do_not_retain_borrows() {
        let slot = Slot::new();
        for value in 1..4 {
            slot.install(value);
            assert!(catch_unwind(AssertUnwindSafe(|| slot.with(|value| {
                *value += 1;
                panic!("unwind session");
            })))
            .is_err());
            assert_eq!(slot.take(), Some(value + 1));
        }
    }

    #[test]
    fn detached_values_drop_outside_the_slot_borrow() {
        use std::rc::{Rc, Weak};
        struct Value(Weak<Slot<Value>>, Rc<Cell<usize>>);
        impl Drop for Value {
            fn drop(&mut self) {
                assert!(!self.0.upgrade().unwrap().is_initialized());
                self.1.set(self.1.get() + 1);
            }
        }
        let count = Rc::new(Cell::new(0));
        let slot = Rc::new(Slot::new());
        slot.install(Value(Rc::downgrade(&slot), count.clone()));
        drop(slot.take());
        assert_eq!(count.get(), 1);
    }
}
