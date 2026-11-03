use super::*;
use std::{
    cell::Cell,
    panic::{catch_unwind, AssertUnwindSafe},
};

struct OnDrop(Option<Box<dyn FnOnce()>>);
impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.take().unwrap()();
    }
}

#[test]
fn shutdown_cancels_destructor_insertions_outside_scheduler_borrows() {
    let scheduler = CoroutinesContext::new();
    let drops = Rc::new(Cell::new(0));
    let (ctx, count) = (scheduler.clone(), drops.clone());
    let token = OnDrop(Some(Box::new(move || {
        count.set(count.get() + 1);
        let inner = OnDrop(Some(Box::new(move || count.set(count.get() + 1))));
        let cancelled = ctx.start(async move {
            let _inner = inner;
        });
        assert!(ctx.is_done(cancelled.id));
    })));
    let old = scheduler.start(async move {
        let _token = token;
        std::future::pending::<()>().await;
    });
    scheduler.shutdown();
    assert_eq!(drops.get(), 2);
    assert!(scheduler.is_done(old.id));
    assert_eq!(scheduler.active_coroutines_count(), 0);
    scheduler.shutdown();
}

#[test]
fn snapshot_defers_insertions_and_rejects_reused_slots() {
    let scheduler = CoroutinesContext::new();
    let victim = Rc::new(Cell::new(None::<GenerationalId>));
    let log = Rc::new(RefCell::new(Vec::new()));
    let (ctx, victim_id, events) = (scheduler.clone(), victim.clone(), log.clone());
    scheduler.start(async move {
        events.borrow_mut().push("parent");
        ctx.stop(victim_id.get().unwrap());
        for _ in 0..1200 {
            let events = events.clone();
            ctx.start(async move {
                events.borrow_mut().push("child");
            });
        }
    });
    let events = log.clone();
    victim.set(Some(
        scheduler
            .start(async move {
                events.borrow_mut().push("cancelled");
            })
            .id,
    ));
    scheduler.update();
    assert_eq!(&*log.borrow(), &["parent"]);
    scheduler.update();
    assert_eq!(log.borrow().len(), 1201);
    assert_eq!(scheduler.active_coroutines_count(), 0);
}

#[test]
fn self_cancellation_cannot_restore_over_a_replacement() {
    let scheduler = CoroutinesContext::new();
    let id = Rc::new(Cell::new(None));
    let replacement = Rc::new(Cell::new(None));
    let dropped = Rc::new(Cell::new(0));
    let (ctx, this, next, count) = (
        scheduler.clone(),
        id.clone(),
        replacement.clone(),
        dropped.clone(),
    );
    let token = OnDrop(Some(Box::new(move || count.set(count.get() + 1))));
    let task = scheduler.start(async move {
        let _token = token;
        let id = this.get().unwrap();
        assert!(!ctx.is_done(id));
        ctx.stop(id);
        assert!(ctx.is_done(id));
        next.set(Some(ctx.start(async { 7u32 }).id));
        std::future::pending::<()>().await;
    });
    id.set(Some(task.id));
    scheduler.update();
    assert_eq!(dropped.get(), 1);
    assert!(scheduler.is_done(task.id));
    scheduler.update();
    assert_eq!(
        scheduler.retrieve::<u32>(replacement.get().unwrap()),
        Some(7)
    );
}

#[test]
fn stop_all_during_poll_drops_outside_borrows_and_preserves_drop_insertions() {
    let scheduler = CoroutinesContext::new();
    let dropped = Rc::new(Cell::new(0));
    let (ctx, count) = (scheduler.clone(), dropped.clone());
    let token = OnDrop(Some(Box::new(move || {
        assert!(ctx.0.try_borrow_mut().is_ok());
        count.set(count.get() + 1);
        ctx.start(async {});
    })));
    let ctx = scheduler.clone();
    let task = scheduler.start(async move {
        let _token = token;
        ctx.stop_all();
        std::future::pending::<()>().await;
    });
    scheduler.start(std::future::pending::<()>());
    scheduler.update();
    assert!(scheduler.is_done(task.id));
    assert_eq!(dropped.get(), 1);
    assert_eq!(scheduler.active_coroutines_count(), 1);
    scheduler.update();
    assert_eq!(scheduler.active_coroutines_count(), 0);
}

#[test]
fn nested_manual_polls_restore_outer_timing() {
    let scheduler = CoroutinesContext::new();
    let ctx = scheduler.clone();
    let inner = scheduler.start(std::future::poll_fn(move |_| {
        assert_eq!(ctx.active_delta(), Some(0.25));
        Poll::<()>::Ready(())
    }));
    scheduler.set_manual_poll(inner.id);
    let ctx = scheduler.clone();
    let outer = scheduler.start(std::future::poll_fn(move |_| {
        assert_eq!(ctx.active_delta(), Some(0.5));
        ctx.poll(inner.id, Some(0.25));
        assert_eq!(ctx.active_delta(), Some(0.5));
        Poll::<()>::Ready(())
    }));
    scheduler.set_manual_poll(outer.id);
    scheduler.update();
    assert!(!scheduler.is_done(outer.id));
    scheduler.poll(outer.id, Some(0.5));
    assert_eq!(scheduler.active_delta(), None);
    assert_eq!(scheduler.retrieve::<()>(outer.id), Some(()));
    assert_eq!(scheduler.retrieve::<()>(outer.id), Some(()));
}

#[test]
fn panic_cancels_the_future_and_restores_poll_and_update_state() {
    for manual in [false, true] {
        let scheduler = CoroutinesContext::new();
        let ctx = scheduler.clone();
        let token = OnDrop(Some(Box::new(move || {
            // Async locals may unwind inside the poll itself. They still see
            // its timing, but must be able to reenter the scheduler.
            assert_eq!(ctx.active_delta(), manual.then_some(0.1));
            ctx.start(async {});
        })));
        let task = scheduler.start(std::future::poll_fn(move |_| {
            let _keep_alive = &token;
            panic!("poll failure");
            #[allow(unreachable_code)]
            Poll::<()>::Pending
        }));
        if manual {
            scheduler.set_manual_poll(task.id);
        }
        assert!(catch_unwind(AssertUnwindSafe(|| {
            if manual {
                scheduler.poll(task.id, Some(0.1));
            } else {
                scheduler.update();
            }
        }))
        .is_err());
        assert!(scheduler.is_done(task.id));
        assert_eq!(scheduler.active_delta(), None);
        assert!(!scheduler.0.borrow().updating);
        scheduler.update();
        assert_eq!(scheduler.active_coroutines_count(), 0);
    }
}

#[test]
fn recursive_poll_is_rejected_without_losing_outer_state() {
    let scheduler = CoroutinesContext::new();
    let id = Rc::new(Cell::new(None));
    let (ctx, this) = (scheduler.clone(), id.clone());
    let task = scheduler.start(std::future::poll_fn(move |_| {
        assert!(
            catch_unwind(AssertUnwindSafe(|| ctx.poll(this.get().unwrap(), Some(9.)))).is_err()
        );
        assert_eq!(ctx.active_delta(), Some(0.1));
        Poll::Ready(5u32)
    }));
    id.set(Some(task.id));
    scheduler.set_manual_poll(task.id);
    scheduler.poll(task.id, Some(0.1));
    assert_eq!(scheduler.retrieve::<u32>(task.id), Some(5));
    assert_eq!(scheduler.retrieve::<u32>(task.id), None);
}

#[test]
fn returned_values_and_completed_future_destructors_can_reenter() {
    let scheduler = CoroutinesContext::new();
    let ctx = scheduler.clone();
    let value = OnDrop(Some(Box::new(move || {
        ctx.start(async {});
    })));
    let task = scheduler.start(async move { value });
    scheduler.update();
    scheduler.stop(task.id);
    assert_eq!(scheduler.active_coroutines_count(), 1);
    scheduler.update();
    let ctx = scheduler.clone();
    let token = OnDrop(Some(Box::new(move || {
        assert_eq!(ctx.active_delta(), Some(0.2));
        ctx.start(async {});
    })));
    let task = scheduler.start(std::future::poll_fn(move |_| {
        let _keep_alive = &token;
        Poll::Ready(())
    }));
    scheduler.set_manual_poll(task.id);
    scheduler.poll(task.id, Some(0.2));
    scheduler.update();
    assert_eq!(scheduler.active_coroutines_count(), 0);
}

#[test]
fn switching_to_manual_poll_during_poll_is_preserved() {
    let scheduler = CoroutinesContext::new();
    let id = Rc::new(Cell::new(None));
    let polls = Rc::new(Cell::new(0));
    let (ctx, this, count) = (scheduler.clone(), id.clone(), polls.clone());
    let task = scheduler.start(std::future::poll_fn(move |_| {
        count.set(count.get() + 1);
        ctx.set_manual_poll(this.get().unwrap());
        Poll::<()>::Pending
    }));
    id.set(Some(task.id));
    scheduler.update();
    scheduler.update();
    assert_eq!(polls.get(), 1);
    scheduler.poll(task.id, Some(0.1));
    assert_eq!(polls.get(), 2);
    scheduler.stop_all();
}
