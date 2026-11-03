use std::{
    marker::PhantomData,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        RwLock,
    },
    thread::{self, ThreadId},
};

static THREAD_ID: RwLock<Option<ThreadId>> = RwLock::new(None);
static WINDOW_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Reserve before entering the backend's startup code. The actual owner may be
/// a different thread on platforms whose event loop calls the factory elsewhere.
pub(crate) struct WindowReservation;
impl WindowReservation {
    pub(crate) fn new() -> Self {
        assert!(
            !WINDOW_ACTIVE.swap(true, Ordering::AcqRel),
            "a Macroquad window is already active"
        );
        Self
    }
}
impl Drop for WindowReservation {
    fn drop(&mut self) {
        WINDOW_ACTIVE.store(false, Ordering::Release);
    }
}

pub(crate) struct Registration(PhantomData<Rc<()>>);
pub(crate) fn set_thread_id() -> Registration {
    let previous = {
        let mut owner = THREAD_ID
            .write()
            .expect("Macroquad thread ID lock poisoned");
        let previous = *owner;
        if previous.is_none() {
            *owner = Some(thread::current().id());
        }
        previous
    };
    assert!(previous.is_none(), "Macroquad already has an owner thread");
    Registration(PhantomData)
}
impl Drop for Registration {
    fn drop(&mut self) {
        let mut owner = THREAD_ID
            .write()
            .expect("Macroquad thread ID lock poisoned");
        *owner = None;
    }
}

pub(crate) fn same_thread() {
    thread_local! { static CURRENT_THREAD_ID: ThreadId = thread::current().id(); }
    let owner = *THREAD_ID.read().expect("Macroquad thread ID lock poisoned");
    assert_eq!(
        owner.expect("Macroquad thread is not initialized"),
        CURRENT_THREAD_ID.with(|id| *id),
        "Macroquad accessed from a different thread"
    );
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::panic::catch_unwind;

    #[test]
    fn sessions_reject_overlap_and_allow_new_owner_after_release() {
        assert!(catch_unwind(same_thread).is_err());
        let reservation = WindowReservation::new();
        let owner = set_thread_id();
        same_thread();
        assert!(catch_unwind(WindowReservation::new).is_err());
        assert!(
            thread::spawn(|| catch_unwind(WindowReservation::new).is_err())
                .join()
                .unwrap()
        );
        assert!(thread::spawn(same_thread).join().is_err());
        assert!(thread::spawn(|| catch_unwind(set_thread_id).is_err())
            .join()
            .unwrap());
        same_thread();
        drop(owner);
        assert!(catch_unwind(same_thread).is_err());
        drop(reservation);
        thread::spawn(|| {
            let _reservation = WindowReservation::new();
            let _owner = set_thread_id();
            same_thread();
        })
        .join()
        .unwrap();
        assert!(catch_unwind(same_thread).is_err());
    }
}
