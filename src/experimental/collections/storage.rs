//! Persistent storage local to the current thread.
//!
//! Shared and mutable access is checked at runtime. Returned guards keep their
//! values alive even if a later `store` replaces the entry. Values are not shared
//! between threads. When thread-local storage is destroyed, its values are
//! dropped; their destructors must not access this storage during thread teardown.
//!
//! ```
//! use macroquad::experimental::collections::storage;
//!
//! struct WorldBoundaries(i32);
//!
//! fn draw_player() {
//!   let boundaries: i32 = storage::get::<WorldBoundaries>().0;
//!   assert_eq!(boundaries, 23);
//! }
//!
//! storage::store(WorldBoundaries(23));
//! draw_player();
//! ```

use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
    ops::{Deref, DerefMut},
    rc::Rc,
};

// Shared guards own clones of the value's Rc. A mutable guard temporarily takes
// the sole Rc out of the slot, leaving None to reject further borrows.
type Slot<T> = Rc<RefCell<Option<Rc<T>>>>;

thread_local! {
    static STORAGE: RefCell<HashMap<TypeId, Box<dyn Any>>> = RefCell::new(HashMap::new());
}

struct StorageMut<T> {
    value: Rc<T>,
    slot: Slot<T>,
}

impl<T> Deref for StorageMut<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for StorageMut<T> {
    fn deref_mut(&mut self) -> &mut T {
        Rc::get_mut(&mut self.value).expect("mutable storage guard must own its value exclusively")
    }
}

impl<T> Drop for StorageMut<T> {
    fn drop(&mut self) {
        // Restore only this slot, never the map entry: store() may have replaced
        // it while this guard was alive. No user destructor runs under this borrow.
        *self.slot.borrow_mut() = Some(Rc::clone(&self.value));
    }
}

fn slot<T: Any>() -> Option<Slot<T>> {
    STORAGE.with_borrow(|storage| {
        storage
            .get(&TypeId::of::<T>())
            .and_then(|data| data.downcast_ref::<Slot<T>>())
            .cloned()
    })
}

/// Store data in the current thread's storage, replacing an old value if any.
/// Outstanding guards keep the old value alive and continue to access it.
pub fn store<T: Any>(data: T) {
    let value: Slot<T> = Rc::new(RefCell::new(Some(Rc::new(data))));
    let previous =
        STORAGE.with_borrow_mut(|storage| storage.insert(TypeId::of::<T>(), Box::new(value)));
    // A replaced value's destructor may itself access storage.
    drop(previous);
}

/// Get a shared guard to data from the current thread's storage.
/// Panics if the type is missing or its current value is mutably borrowed.
pub fn get<T: Any>() -> impl Deref<Target = T> {
    try_get::<T>().unwrap()
}

/// Get a shared guard to data from the current thread's storage.
/// Returns None if the type is missing; panics if its value is mutably borrowed.
pub fn try_get<T: Any>() -> Option<impl Deref<Target = T>> {
    let slot = slot::<T>()?;
    let value = Rc::clone(
        slot.borrow()
            .as_ref()
            .expect("storage value is already mutably borrowed"),
    );
    Some(value)
}

/// Get a mutable guard to data from the current thread's storage.
/// Returns None if the type is missing; panics if its value is already borrowed.
pub fn try_get_mut<T: Any>() -> Option<impl DerefMut<Target = T>> {
    let slot = slot::<T>()?;
    let value = {
        let mut value = slot.borrow_mut();
        // get_mut checks uniqueness without exposing or cloning the Rc. Failure
        // leaves the value in the slot, so a conflicting borrow cannot lose it.
        Rc::get_mut(
            value
                .as_mut()
                .expect("storage value is already mutably borrowed"),
        )
        .expect("storage value is already borrowed");
        value.take().unwrap()
    };
    Some(StorageMut { value, slot })
}

/// Get a mutable guard to data from the current thread's storage.
/// Panics if the type is missing or its current value is already borrowed.
pub fn get_mut<T: Any>() -> impl DerefMut<Target = T> {
    try_get_mut::<T>().unwrap()
}

#[cfg(test)]
mod tests;
