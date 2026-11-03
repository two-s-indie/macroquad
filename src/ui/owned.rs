//! Exclusive guards that own their value instead of borrowing a registry.

use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    rc::Rc,
};

pub(crate) struct Owned<T>(Rc<RefCell<Option<Box<T>>>>);

impl<T> Clone for Owned<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T> Owned<T> {
    pub(super) fn new(value: T) -> Self {
        Self(Rc::new(RefCell::new(Some(Box::new(value)))))
    }

    pub(super) fn borrow_mut(&self) -> Guard<T> {
        let value = self
            .0
            .borrow_mut()
            .take()
            .expect("root UI is already borrowed");
        Guard {
            value: Some(value),
            owner: self.clone(),
        }
    }
}

pub(super) struct Guard<T> {
    value: Option<Box<T>>,
    owner: Owned<T>,
}

impl<T> Deref for Guard<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value.as_ref().unwrap()
    }
}

impl<T> DerefMut for Guard<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value.as_mut().unwrap()
    }
}

impl<T> Drop for Guard<T> {
    fn drop(&mut self) {
        // Restore this owner only, even if the engine has replaced its UI.
        // Release the slot borrow before any user-owned values can be dropped.
        let previous = self.owner.0.replace(self.value.take());
        debug_assert!(previous.is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        panic::{catch_unwind, AssertUnwindSafe},
    };

    #[test]
    fn conflict_and_unwind_restore_exclusive_access() {
        let owner = Owned::new(1);
        let mut guard = owner.borrow_mut();
        assert!(catch_unwind(AssertUnwindSafe(|| owner.borrow_mut())).is_err());
        *guard = 2;
        drop(guard);
        assert!(catch_unwind(AssertUnwindSafe(|| {
            let mut guard = owner.borrow_mut();
            *guard = 3;
            panic!("unwind guard");
        }))
        .is_err());
        assert_eq!(*owner.borrow_mut(), 3);
    }

    #[test]
    fn guard_survives_owner_replacement_and_drops_once() {
        struct Value(Rc<Cell<usize>>);
        impl Drop for Value {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let drops = Rc::new(Cell::new(0));
        let mut owner = Owned::new(Value(drops.clone()));
        let guard = owner.borrow_mut();
        owner = Owned::new(Value(drops.clone()));
        drop(owner);
        assert_eq!(drops.get(), 1);
        assert_eq!(guard.0.get(), 1);
        drop(guard);
        assert_eq!(drops.get(), 2);
    }

    #[test]
    fn detached_value_destructor_can_borrow_another_owner() {
        struct Value(Owned<usize>);
        impl Drop for Value {
            fn drop(&mut self) {
                *self.0.borrow_mut() += 1;
            }
        }
        let other = Owned::new(0);
        let owner = Owned::new(Value(other.clone()));
        let guard = owner.borrow_mut();
        drop(owner);
        drop(guard);
        assert_eq!(*other.borrow_mut(), 1);
    }
}
