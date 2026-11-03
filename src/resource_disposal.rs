//! CPU-only disposal requests. Only the owning renderer may drain a queue.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, ThreadId};

struct State<T> {
    owner: ThreadId,
    pending: Mutex<Option<Vec<T>>>,
    has_pending: AtomicBool,
    closed: AtomicBool,
}

pub(crate) struct DisposalQueue<T>(Arc<State<T>>);
pub(crate) struct DisposalSender<T>(Weak<State<T>>);

impl<T> Clone for DisposalSender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> PartialEq for DisposalSender<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
}
impl<T> std::fmt::Debug for DisposalSender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DisposalSender")
            .field(&self.0.as_ptr())
            .finish()
    }
}

impl<T> DisposalQueue<T> {
    pub(crate) fn new() -> Self {
        Self(Arc::new(State {
            owner: thread::current().id(),
            pending: Mutex::new(Some(Vec::new())),
            has_pending: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        }))
    }

    pub(crate) fn sender(&self) -> DisposalSender<T> {
        DisposalSender(Arc::downgrade(&self.0))
    }

    pub(crate) fn owns(&self, sender: &DisposalSender<T>) -> bool {
        std::ptr::eq(Arc::as_ptr(&self.0), sender.0.as_ptr())
    }

    pub(crate) fn take(&self) -> Vec<T> {
        // Geometry checks material retirement on every draw. Empty queues
        // require neither a mutex acquisition nor an allocation.
        if !self.0.has_pending.load(Ordering::Acquire) {
            return Vec::new();
        }
        assert_eq!(
            self.0.owner,
            thread::current().id(),
            "resource queue used off its owner thread"
        );
        let mut pending = self.0.pending.lock().unwrap();
        self.0.has_pending.store(false, Ordering::Release);
        pending.as_mut().map(std::mem::take).unwrap_or_default()
    }

    // No GPU calls. Closing and dropping the owner make all late sends no-ops.
    pub(crate) fn close(&self) {
        self.0.closed.store(true, Ordering::Release);
        let pending = self.0.pending.lock().unwrap().take();
        self.0.has_pending.store(false, Ordering::Release);
        drop(pending); // Never run destructors with the queue locked.
    }
}

impl<T> Drop for DisposalQueue<T> {
    fn drop(&mut self) {
        self.close();
    }
}

impl<T> DisposalSender<T> {
    pub(crate) fn assert_active(&self) {
        let state = self.0.upgrade().expect("resource session has ended");
        assert_eq!(
            state.owner,
            thread::current().id(),
            "resource used off its owner thread"
        );
        assert!(
            !state.closed.load(Ordering::Acquire),
            "resource session has ended"
        );
    }

    pub(crate) fn send(&self, value: T) {
        let Some(state) = self.0.upgrade() else {
            return;
        };
        let mut pending = state.pending.lock().unwrap();
        if pending.is_none() {
            return;
        }
        // Keep live-resource destruction on the engine thread, as before.
        // Release the lock before panicking so a misuse cannot poison the queue.
        if state.owner != thread::current().id() {
            drop(pending);
            panic!("resource dropped off its owner thread");
        }
        pending.as_mut().unwrap().push(value);
        state.has_pending.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_use_rejects_closed_expired_and_foreign_threads() {
        let queue = DisposalQueue::<()>::new();
        let sender = queue.sender();
        sender.assert_active();
        let foreign = sender.clone();
        assert!(thread::spawn(move || foreign.assert_active())
            .join()
            .is_err());
        sender.assert_active();
        queue.close();
        assert!(std::panic::catch_unwind(|| sender.assert_active()).is_err());
        drop(queue);
        assert!(std::panic::catch_unwind(|| sender.assert_active()).is_err());
    }

    #[test]
    fn old_senders_cannot_reach_a_new_session() {
        let old = DisposalQueue::new();
        let sender = old.sender();
        sender.send(1);
        assert_eq!(old.take(), vec![1]);
        assert!(old.take().is_empty());
        old.close();
        let new = DisposalQueue::new();
        assert!(!new.owns(&sender));
        sender.send(2);
        assert!(old.take().is_empty());
        drop(old);
        sender.send(3);
        new.sender().send(4);
        assert_eq!(new.take(), vec![4]);
    }

    #[test]
    fn live_cross_thread_send_is_rejected_without_poisoning_queue() {
        let queue = DisposalQueue::new();
        let sender = queue.sender();
        assert!(thread::spawn(move || sender.send(1)).join().is_err());
        queue.sender().send(2);
        assert_eq!(queue.take(), vec![2]);
        queue.close();
        let sender = queue.sender();
        thread::spawn(move || sender.send(3)).join().unwrap();
        assert!(queue.take().is_empty());
    }

    struct Nested {
        sender: DisposalSender<Nested>,
        enqueue_child: bool,
        drops: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl Drop for Nested {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::Relaxed);
            if self.enqueue_child {
                self.sender.send(Nested {
                    sender: self.sender.clone(),
                    enqueue_child: false,
                    drops: self.drops.clone(),
                });
            }
        }
    }

    #[test]
    fn drain_and_close_release_locks_before_nested_destruction() {
        let queue = DisposalQueue::new();
        let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for close in [false, true] {
            queue.sender().send(Nested {
                sender: queue.sender(),
                enqueue_child: true,
                drops: drops.clone(),
            });
            if close {
                queue.close();
            } else {
                drop(queue.take());
                assert_eq!(drops.load(Ordering::Relaxed), 1);
                drop(queue.take());
            }
            assert!(queue.take().is_empty());
        }
        assert_eq!(drops.load(Ordering::Relaxed), 4);
    }
}
