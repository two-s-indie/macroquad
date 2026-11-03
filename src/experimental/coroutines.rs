//! The way to emulate multitasking with macroquad's `.await`.
//! Useful for state machines, animation cutscenes and work spread across frames.

use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use crate::exec::resume;

mod generational_storage;
#[cfg(test)]
mod tests;
use generational_storage::{GenerationalId, GenerationalStorage};

struct CoroutineInternal {
    future: Pin<Box<dyn Future<Output = Box<dyn Any>>>>,
    manual_poll: bool,
    manual_time: Option<f64>,
    has_value: bool,
}

enum CoroutineState {
    Running(CoroutineInternal),
    // Reserve the identity while the future is owned by the poll guard.
    Polling {
        manual_poll: bool,
        manual_time: Option<f64>,
    },
    Value(Box<dyn Any>),
}

struct Scheduler {
    coroutines: GenerationalStorage<CoroutineState>,
    active_delta: Option<f64>,
    updating: bool,
    closed: bool,
}

#[derive(Clone)]
pub(crate) struct CoroutinesContext(Rc<RefCell<Scheduler>>);

fn context() -> CoroutinesContext {
    crate::with_context(|context| context.coroutines_context.clone())
}

impl CoroutinesContext {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(Scheduler {
            coroutines: GenerationalStorage::new(),
            active_delta: None,
            updating: false,
            closed: false,
        })))
    }

    fn start<T: 'static + Any>(&self, future: impl Future<Output = T> + 'static) -> Coroutine<T> {
        if self.0.borrow().closed {
            // Drop captured user values after releasing the scheduler borrow.
            drop(future);
            return Coroutine {
                id: GenerationalId::cancelled(),
                _phantom: PhantomData,
            };
        }
        let coroutine = CoroutineState::Running(CoroutineInternal {
            future: Box::pin(async { Box::new(future.await) as _ }),
            has_value: TypeId::of::<()>() != TypeId::of::<T>(),
            manual_poll: false,
            manual_time: None,
        });
        let id = self.0.borrow_mut().coroutines.push(coroutine);
        Coroutine {
            id,
            _phantom: PhantomData,
        }
    }

    pub fn update(&self) {
        struct Updating {
            scheduler: CoroutinesContext,
        }
        impl Drop for Updating {
            fn drop(&mut self) {
                self.scheduler.0.borrow_mut().updating = false;
            }
        }
        let ids = {
            let mut scheduler = self.0.borrow_mut();
            assert!(!scheduler.updating, "recursive coroutine update");
            scheduler.updating = true;
            scheduler.coroutines.ids()
        };
        let _updating = Updating {
            scheduler: self.clone(),
        };
        for id in ids {
            self.poll(id, None);
        }
    }

    fn is_done(&self, id: GenerationalId) -> bool {
        matches!(
            self.0.borrow().coroutines.get(id),
            None | Some(CoroutineState::Value(_))
        )
    }

    fn retrieve<T: 'static + Any>(&self, id: GenerationalId) -> Option<T> {
        if TypeId::of::<T>() == TypeId::of::<()>() && self.is_done(id) {
            return Some(*Box::<dyn Any>::from(Box::new(())).downcast::<T>().unwrap());
        }
        let removed = {
            let mut scheduler = self.0.borrow_mut();
            if matches!(scheduler.coroutines.get(id), Some(CoroutineState::Value(_))) {
                scheduler.coroutines.take(id)
            } else {
                None
            }
        };
        match removed {
            Some(CoroutineState::Value(value)) => Some(*value.downcast().unwrap()),
            _ => None,
        }
    }

    fn set_manual_poll(&self, id: GenerationalId) {
        let mut scheduler = self.0.borrow_mut();
        match scheduler.coroutines.get_mut(id) {
            Some(CoroutineState::Running(f)) => {
                f.manual_poll = true;
                f.manual_time = Some(0.);
            }
            Some(CoroutineState::Polling {
                manual_poll,
                manual_time,
            }) => {
                *manual_poll = true;
                *manual_time = Some(0.);
            }
            _ => {}
        }
    }

    fn poll(&self, id: GenerationalId, delta: Option<f64>) {
        let (future, previous_delta) = {
            let mut scheduler = self.0.borrow_mut();
            let Some(slot) = scheduler.coroutines.get_mut(id) else {
                return;
            };
            match slot {
                CoroutineState::Value(_) => return,
                CoroutineState::Polling { .. } => {
                    assert!(delta.is_none(), "coroutine is already being polled");
                    return;
                }
                CoroutineState::Running(f) => {
                    if delta.is_none() && f.manual_poll {
                        return;
                    }
                    if delta.is_some() {
                        assert!(
                            f.manual_poll,
                            "coroutine requires set_manual_poll before poll"
                        );
                    }
                }
            }
            let CoroutineState::Running(f) = slot else {
                unreachable!()
            };
            let manual_poll = f.manual_poll;
            let manual_time = f.manual_time.map(|time| time + delta.unwrap_or(0.));
            let CoroutineState::Running(future) = std::mem::replace(
                slot,
                CoroutineState::Polling {
                    manual_poll,
                    manual_time,
                },
            ) else {
                unreachable!()
            };
            let previous_delta = std::mem::replace(&mut scheduler.active_delta, delta);
            (future, previous_delta)
        };
        let mut guard = PollGuard {
            context: self.clone(),
            id,
            previous_delta,
            future: Some(future),
            active: true,
        };
        let result = resume(&mut guard.future.as_mut().unwrap().future);
        guard.finish(result);
    }

    fn stop(&self, id: GenerationalId) {
        let removed = self.0.borrow_mut().coroutines.take(id);
        drop(removed);
    }

    pub(crate) fn stop_all(&self) {
        let removed = self.0.borrow_mut().coroutines.take_all();
        drop(removed);
    }

    pub(crate) fn shutdown(&self) {
        let removed = {
            let mut scheduler = self.0.borrow_mut();
            scheduler.closed = true;
            scheduler.coroutines.take_all()
        };
        drop(removed);
    }

    fn active_delta(&self) -> Option<f64> {
        self.0.borrow().active_delta
    }

    pub(crate) fn allocated_memory(&self) -> usize {
        self.0.borrow().coroutines.allocated_memory()
    }
    pub(crate) fn active_coroutines_count(&self) -> usize {
        self.0.borrow().coroutines.count()
    }
}

struct PollGuard {
    context: CoroutinesContext,
    id: GenerationalId,
    previous_delta: Option<f64>,
    future: Option<CoroutineInternal>,
    active: bool,
}
impl PollGuard {
    fn finish(mut self, result: Option<Box<dyn Any>>) {
        let completed = result.is_some();
        let has_value = self.future.as_ref().unwrap().has_value;
        let mut next = if completed {
            result.map(CoroutineState::Value)
        } else {
            self.future.take().map(CoroutineState::Running)
        };
        {
            let mut scheduler = self.context.0.borrow_mut();
            scheduler.active_delta = self.previous_delta;
            if let Some(CoroutineState::Polling {
                manual_poll,
                manual_time,
            }) = scheduler.coroutines.get_mut(self.id)
            {
                if let Some(CoroutineState::Running(f)) = &mut next {
                    f.manual_poll = *manual_poll;
                    f.manual_time = *manual_time;
                }
                if completed && !has_value {
                    // This is only a placeholder; it owns no user data.
                    scheduler.coroutines.take(self.id);
                } else {
                    *scheduler.coroutines.get_mut(self.id).unwrap() = next.take().unwrap();
                }
            }
        }
        self.active = false;
        // A cancelled poll's result and remaining future storage are dropped
        // after restoring timing and releasing the scheduler borrow.
        drop(next);
    }
}
impl Drop for PollGuard {
    fn drop(&mut self) {
        if self.active {
            let removed = {
                let mut scheduler = self.context.0.borrow_mut();
                scheduler.active_delta = self.previous_delta;
                scheduler.coroutines.take(self.id)
            };
            drop(removed);
        }
        // self.future is dropped after this method, outside the scheduler borrow.
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Coroutine<T = ()> {
    id: GenerationalId,
    _phantom: PhantomData<T>,
}
impl<T: 'static + Any> Coroutine<T> {
    /// Returns true if the coroutine finished, panicked, or was stopped.
    pub fn is_done(&self) -> bool {
        context().is_done(self.id)
    }
    pub fn retrieve(&self) -> Option<T> {
        context().retrieve(self.id)
    }

    /// By default coroutines are being polled each frame, inside the "next_frame()"
    ///
    /// ```skip
    /// start_coroutine(async move {
    ///    println!("a");
    ///    next_frame().await;
    ///    println!("b");
    /// }); // <- coroutine is created, but not yet polled
    /// println!("c"); // <- first print, "c"
    /// next_frame().await; // coroutine will be polled for the first time
    ///                     // will print "a"
    /// println!("d");      // "d"
    /// next_frame().await; // coroutine will be polled second time, pass next_frame().await and will print "b"
    /// ```
    /// will print "cadb" (there is a test for it, "tests/coroutine.rs:coroutine_execution_order" )
    ///
    /// But, sometimes, automatic polling is not nice
    /// good example - game pause. Imagine a player that have some "update" function
    /// and some coroutines runned. During the pause "update" just early quit, but
    /// what with the coroutines?
    ///
    /// "set_manual_poll" allows to control how coroutine is beng polled
    /// after set_manual_poll() coroutine will never be polled automatically
    /// so player will need to poll all its coroutines inside "update" function
    pub fn set_manual_poll(&mut self) {
        context().set_manual_poll(self.id);
    }

    /// Poll once and advance the local timeline by `delta_time`.
    /// Panics unless manual polling was enabled, or if this coroutine is already
    /// being polled. A panic from its future cancels that coroutine.
    pub fn poll(&mut self, delta_time: f64) {
        context().poll(self.id, Some(delta_time));
    }
}

/// Schedule a coroutine. Coroutines started during an automatic update become
/// eligible on the next update; manual polling may run them immediately.
pub fn start_coroutine<T: 'static + Any>(
    future: impl Future<Output = T> + 'static + Send,
) -> Coroutine<T> {
    context().start(future)
}

/// Cancel the current snapshot. Destructors run outside scheduler/context
/// borrows; coroutines they create remain scheduled. A currently polling future
/// finishes its current poll before its storage is destroyed.
pub fn stop_all_coroutines() {
    context().stop_all();
}

pub fn stop_coroutine<T: 'static + Any>(coroutine: Coroutine<T>) {
    context().stop(coroutine.id);
}

pub struct TimerDelayFuture {
    pub(crate) remaining_time: f32,
}
impl Future for TimerDelayFuture {
    type Output = Option<()>;
    fn poll(mut self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        let delta = context()
            .active_delta()
            .unwrap_or_else(|| crate::time::get_frame_time() as _);
        self.remaining_time -= delta as f32;
        if self.remaining_time <= 0.0 {
            Poll::Ready(Some(()))
        } else {
            Poll::Pending
        }
    }
}
pub const fn wait_seconds(time: f32) -> TimerDelayFuture {
    TimerDelayFuture {
        remaining_time: time,
    }
}

/// Special built-in coroutines for modifying values over time.
pub mod tweens {
    use crate::experimental::scene::{Handle, Lens, Node};
    use std::future::Future;
    use std::pin::Pin;
    use std::{
        ops::{Add, Mul, Sub},
        task::{Context, Poll},
    };

    /// A tween retaining its typed projection until completion. The node and
    /// projection types are inferred by `linear`; no node guard crosses a poll.
    pub struct LinearTweenFuture<T, T1: 'static, F>
    where
        T: Copy + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
    {
        from: T,
        to: T,
        lens: Lens<T1, T, F>,
        start_time: f64,
        time: f32,
    }
    impl<T, T1, F> Unpin for LinearTweenFuture<T, T1, F> where
        T: Copy + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>
    {
    }

    impl<T, T1, F> Future for LinearTweenFuture<T, T1, F>
    where
        T: Copy + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
        F: for<'r> FnMut(&'r mut T1) -> &'r mut T,
    {
        type Output = ();

        fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
            let t = (miniquad::date::now() - self.start_time) / self.time as f64;
            let this = self.get_mut();
            this.lens
                .with(|var| {
                    if t <= 1. {
                        *var = this.from + (this.to - this.from) * t as f32;
                        Poll::Pending
                    } else {
                        *var = this.to;
                        Poll::Ready(())
                    }
                })
                .unwrap_or(Poll::Ready(()))
        }
    }

    pub fn linear<T, T1, F>(
        handle: Handle<T1>,
        lens: F,
        from: T,
        to: T,
        time: f32,
    ) -> LinearTweenFuture<T, T1, F>
    where
        T: Copy + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
        T1: Node,
        F: for<'r> FnMut(&'r mut T1) -> &'r mut T,
    {
        LinearTweenFuture {
            to,
            from,
            lens: handle.lens(lens),
            time,
            start_time: miniquad::date::now(),
        }
    }

    pub async fn follow_path<T, T1, F>(handle: Handle<T1>, mut lens: F, path: Vec<T>, time: f32)
    where
        T: Copy + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
        T1: Node,
        F: for<'r> FnMut(&'r mut T1) -> &'r mut T,
    {
        for point in path.windows(2) {
            linear(
                handle,
                &mut lens,
                point[0],
                point[1],
                time / path.len() as f32,
            )
            .await
        }
    }
}
