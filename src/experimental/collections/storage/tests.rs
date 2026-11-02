use super::{get, get_mut, store, try_get, try_get_mut};
use std::{
    cell::Cell,
    panic::{catch_unwind, AssertUnwindSafe},
    rc::Rc,
};

fn assert_panics(f: impl FnOnce()) {
    assert!(catch_unwind(AssertUnwindSafe(f)).is_err());
}

#[test]
fn missing_values_return_none_or_panic() {
    struct Missing;
    assert!(try_get::<Missing>().is_none());
    assert!(try_get_mut::<Missing>().is_none());
    assert_panics(|| drop(get::<Missing>()));
    assert_panics(|| drop(get_mut::<Missing>()));
}

#[test]
fn shared_and_exclusive_borrows_are_checked_and_released() {
    struct Value(i32);
    store(Value(1));

    let first = get::<Value>();
    let second = get::<Value>();
    assert_eq!(first.0, second.0);
    assert_panics(|| drop(try_get_mut::<Value>()));
    drop(first);
    assert_panics(|| drop(try_get_mut::<Value>()));
    drop(second);

    let mut value = get_mut::<Value>();
    value.0 = 2;
    assert_panics(|| drop(try_get::<Value>()));
    assert_panics(|| drop(try_get_mut::<Value>()));
    drop(value);
    assert_eq!(get::<Value>().0, 2);

    assert_panics(|| {
        let mut value = get_mut::<Value>();
        value.0 = 3;
        panic!("unwind while holding a mutable guard");
    });
    assert_eq!(get::<Value>().0, 3);
}

#[test]
fn unrelated_entries_remain_accessible_during_a_mutable_borrow() {
    struct First(i32);
    struct Second(i32);
    store(First(1));
    let mut first = get_mut::<First>();
    store(Second(2));
    get_mut::<Second>().0 += 1;
    first.0 += get::<Second>().0;
    drop(first);
    assert_eq!(get::<First>().0, 4);
}

struct Tracked {
    value: i32,
    drops: Rc<Cell<usize>>,
}

impl Drop for Tracked {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

#[test]
fn shared_guards_keep_replaced_values_alive() {
    let drops = Rc::new(Cell::new(0));
    store(Tracked {
        value: 1,
        drops: drops.clone(),
    });
    let first = get::<Tracked>();
    let second = get::<Tracked>();
    store(Tracked {
        value: 2,
        drops: drops.clone(),
    });

    assert_eq!(first.value, 1);
    assert_eq!(get::<Tracked>().value, 2);
    assert_eq!(drops.get(), 0);
    drop(first);
    assert_eq!(drops.get(), 0);
    assert_eq!(second.value, 1);
    drop(second);
    assert_eq!(drops.get(), 1);
}

#[test]
fn mutable_guards_do_not_restore_replaced_entries() {
    let drops = Rc::new(Cell::new(0));
    store(Tracked {
        value: 1,
        drops: drops.clone(),
    });
    let mut old = get_mut::<Tracked>();
    store(Tracked {
        value: 2,
        drops: drops.clone(),
    });

    old.value = 10;
    let mut current = get_mut::<Tracked>();
    current.value = 20;
    assert_eq!(old.value, 10);
    assert_eq!(drops.get(), 0);
    drop(old);
    assert_eq!(drops.get(), 1);
    assert_eq!(current.value, 20);
    drop(current);
    assert_eq!(get::<Tracked>().value, 20);
}

#[test]
fn replaced_values_can_access_storage_in_their_destructors() {
    struct DropCount(usize);
    struct ReentrantDrop(bool);
    impl Drop for ReentrantDrop {
        fn drop(&mut self) {
            if self.0 {
                assert!(!get::<ReentrantDrop>().0);
                get_mut::<DropCount>().0 += 1;
            }
        }
    }

    store(DropCount(0));
    store(ReentrantDrop(true));
    store(ReentrantDrop(false));
    assert_eq!(get::<DropCount>().0, 1);

    store(ReentrantDrop(true));
    let shared = get::<ReentrantDrop>();
    store(ReentrantDrop(false));
    assert_eq!(get::<DropCount>().0, 1);
    drop(shared);
    assert_eq!(get::<DropCount>().0, 2);

    store(ReentrantDrop(true));
    let exclusive = get_mut::<ReentrantDrop>();
    store(ReentrantDrop(false));
    assert_eq!(get::<DropCount>().0, 2);
    drop(exclusive);
    assert_eq!(get::<DropCount>().0, 3);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn threads_have_independent_storage_and_drop_values_on_exit() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct Value(i32);
    struct DropOnExit(Arc<AtomicUsize>);
    impl Drop for DropOnExit {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    store(Value(1));
    let drops = Arc::new(AtomicUsize::new(0));
    let child_drops = drops.clone();
    std::thread::spawn(move || {
        assert!(try_get::<Value>().is_none());
        store(Value(2));
        store(DropOnExit(child_drops));
        assert_eq!(get::<Value>().0, 2);
    })
    .join()
    .unwrap();

    assert_eq!(get::<Value>().0, 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
