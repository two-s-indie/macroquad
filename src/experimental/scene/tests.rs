//! Ownership regressions and callback characterization. The remaining global
//! and engine lifecycle still require the step 3 migration.

use super::*;
use std::{cell::RefCell, panic::AssertUnwindSafe, sync::Mutex};

static SCENE_TEST_LOCK: Mutex<()> = Mutex::new(());

thread_local! {
    static EVENTS: RefCell<Vec<(u8, &'static str)>> = const { RefCell::new(Vec::new()) };
    static DELETE_TARGET: RefCell<Option<Handle<Value>>> = const { RefCell::new(None) };
}

fn record(id: u8, event: &'static str) {
    EVENTS.with(|events| events.borrow_mut().push((id, event)));
}

fn events() -> Vec<(u8, &'static str)> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

// All tests touching SCENE use this fixture. Detach the old registry before
// dropping it, so test destructors may safely reenter the restored scene.
fn with_scene(test: impl FnOnce()) {
    let _lock = SCENE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    struct Restore(Option<Scene>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let scene = unsafe { std::ptr::replace(&raw mut SCENE, self.0.take()) };
            drop(scene);
        }
    }
    let _restore = Restore(unsafe { std::ptr::replace(&raw mut SCENE, Some(Scene::new_at(0.0))) });
    events();
    DELETE_TARGET.with(|target| *target.borrow_mut() = None);
    test();
}

fn tick(now: f64) {
    update_with(|| now, |_| {}, || {});
}

struct Value(u8);
impl Node for Value {}
impl Drop for Value {
    fn drop(&mut self) {
        record(self.0, "drop");
    }
}

struct Other(u8);
impl Node for Other {}

#[test]
fn node_borrows_conflict_only_for_the_same_node() {
    with_scene(|| {
        let a = add_node(Value(1));
        let b = add_node(Value(2));
        let mut first = get_node(a);
        assert!(try_get_node(a).is_none());
        assert!(get_untyped_node(a.untyped()).is_none());
        let mut second = get_node(b);
        first.0 += 10;
        second.0 += 20;
        drop((first, second));
        assert_eq!(get_node(a).0, 11);
        let untyped = get_untyped_node(b.untyped()).unwrap();
        assert!(untyped.is::<Value>());
        assert_eq!(untyped.to_typed::<Value>().0, 22);
        assert!(try_get_node(Handle::<Value>::null()).is_none());
    });
}

#[test]
fn deleted_handles_stay_stale_after_slot_reuse() {
    with_scene(|| {
        let old = add_node(Value(1));
        get_node(old).delete();
        assert!(try_get_node(old).is_none());
        assert_eq!(events(), [(1, "drop")]);
        tick(0.0); // Reconcile the old dense entry before reusing its slot.
        let new = add_node(Value(2));
        assert_eq!(old.id.unwrap().id, new.id.unwrap().id);
        assert_ne!(old.id, new.id);
        assert!(try_get_node(old).is_none());
        assert_eq!(get_node(new).0, 2);
        get_untyped_node(new.untyped()).unwrap().delete();
        assert_eq!(events(), [(2, "drop")]);
    });
}

#[test]
fn clear_preserves_persistent_nodes_and_resets_reused_capabilities() {
    with_scene(|| {
        let persistent = add_node(Value(1));
        let temporary = add_node(Value(2));
        get_node(persistent).persist();
        get_node(temporary).provides(42u8);
        // A borrowed persistent node is unaffected by clear.
        let held = get_node(persistent);
        unsafe { get_scene() }.clear();
        assert_eq!(held.0, 1);
        drop(held);
        assert!(try_get_node(temporary).is_none());
        assert_eq!(events(), [(2, "drop")]);
        let reused = add_node(Value(3));
        assert_eq!(reused.id.unwrap().id, temporary.id.unwrap().id);
        assert_eq!(find_nodes_with::<u8>().count(), 0);
        unsafe { get_scene() }.clear();
        assert_eq!(events(), [(3, "drop")]);
        assert_eq!(get_node(persistent).0, 1);
    });
}

#[test]
fn type_and_capability_queries_skip_borrowed_nodes() {
    with_scene(|| {
        let a = add_node(Value(1));
        let b = add_node(Other(2));
        get_node(a).provides(7u8);
        get_node(a).provides(8u8);
        get_node(b).provides(9u8);
        assert_eq!(find_nodes_by_type::<Value>().count(), 1);
        assert_eq!(find_node_by_type::<Other>().unwrap().0, 2);
        let capabilities: Vec<_> = find_nodes_with::<u8>().map(|n| n.capability).collect();
        assert_eq!(capabilities, [7, 9]); // First capability of the requested type.
        let held = get_node(a);
        assert!(find_node_by_type::<Value>().is_none());
        assert_eq!(find_nodes_with::<u8>().count(), 1);
        drop(held);
    });
}

#[test]
fn iterator_snapshots_length_and_skips_borrowed_or_deleted_nodes() {
    with_scene(|| {
        let a = add_node(Value(1));
        let b = add_node(Value(2));
        let mut iter = all_nodes();
        add_node(Value(3));
        let held = get_node(a);
        assert_eq!(iter.next().unwrap().handle::<Value>().id, b.id);
        assert!(iter.next().is_none());
        drop(held);
        let iter = all_nodes();
        get_node(b).delete();
        assert_eq!(
            iter.map(|n| n.to_typed::<Value>().0).collect::<Vec<_>>(),
            [1, 3]
        );
    });
}

struct Trace(u8);
impl Node for Trace {
    fn ready(node: RefMut<Self>) {
        record(node.0, "ready");
    }
    fn update(node: RefMut<Self>) {
        assert!(!in_fixed_update());
        record(node.0, "update");
    }
    fn fixed_update(node: RefMut<Self>) {
        assert!(in_fixed_update());
        record(node.0, "fixed");
    }
    fn draw(node: RefMut<Self>) {
        assert!(!in_fixed_update());
        record(node.0, "draw");
    }
}

#[test]
fn callback_order_clock_sampling_and_camera_passes() {
    with_scene(|| {
        add_node(Trace(1));
        add_node(Trace(2));
        set_camera(1, Some(Camera2D::default()));
        update_with(
            || {
                record(0, "clock");
                CONST_FPS
            },
            |_| record(0, "push"),
            || record(0, "pop"),
        );
        assert_eq!(
            events(),
            [
                (1, "ready"),
                (2, "ready"),
                (0, "clock"),
                (1, "update"),
                (2, "update"),
                (0, "push"),
                (1, "draw"),
                (2, "draw"),
                (0, "pop"),
                (0, "push"),
                (1, "draw"),
                (2, "draw"),
                (0, "pop"),
            ]
        ); // Exactly one fixed interval does not run a fixed callback (strict >).
        set_camera(1, None);
        tick(2.0 * CONST_FPS + 0.001);
        assert_eq!(
            events(),
            [
                (1, "update"),
                (2, "update"),
                (1, "fixed"),
                (2, "fixed"),
                (1, "fixed"),
                (2, "fixed"),
                (1, "draw"),
                (2, "draw"),
            ]
        );
    });
}

#[test]
fn nearby_frame_durations_snap_to_common_refresh_intervals() {
    for interval in [1.0 / 120.0, 1.0 / 60.0, 1.0 / 30.0] {
        with_scene(|| {
            tick(interval + 0.0001);
            let scene = unsafe { get_scene() };
            let expected = if interval > CONST_FPS {
                CONST_FPS
            } else {
                interval
            };
            assert!((scene.acc - expected).abs() < 1e-12);
            assert_eq!(scene.current_time, interval + 0.0001);
        });
    }
}

struct Spawner(bool);
impl Node for Spawner {
    fn update(mut node: RefMut<Self>) {
        if !node.0 {
            add_node(Trace(2));
            node.0 = true;
        }
    }
}

#[test]
fn legacy_addition_during_update_can_draw_before_ready() {
    with_scene(|| {
        add_node(Spawner(false));
        tick(0.0);
        assert_eq!(events(), [(2, "draw")]);
        tick(0.0);
        assert_eq!(events(), [(2, "ready"), (2, "update"), (2, "draw")]);
    });
}

struct Killer;
impl Node for Killer {
    fn update(node: RefMut<Self>) {
        let target = DELETE_TARGET.with(|target| target.borrow_mut().take().unwrap());
        get_node(target).delete();
        node.delete();
    }
}

#[test]
fn callback_can_delete_another_node_and_itself() {
    with_scene(|| {
        let killer = add_node(Killer);
        let victim = add_node(Value(2));
        DELETE_TARGET.with(|target| *target.borrow_mut() = Some(victim));
        tick(0.0);
        assert!(try_get_node(killer).is_none());
        assert!(try_get_node(victim).is_none());
        assert_eq!(all_nodes().count(), 0);
        assert_eq!(events(), [(2, "drop")]);
    });
}

#[test]
fn rejected_clear_leaves_all_nodes_registered() {
    with_scene(|| {
        let a = add_node(Value(1));
        let b = add_node(Value(2));
        let held = get_node(b);
        assert!(
            std::panic::catch_unwind(AssertUnwindSafe(|| unsafe { get_scene() }.clear())).is_err()
        );
        assert_eq!(held.0, 2);
        assert!(events().is_empty());
        drop(held);
        assert!(try_get_node(a).is_some());
        assert!(try_get_node(b).is_some());
    });
}

#[test]
fn stale_delete_does_not_remove_reused_slot() {
    with_scene(|| {
        let old = add_node(Value(1));
        get_node(old).delete();
        tick(0.0);
        let new = add_node(Value(2));
        assert!(unsafe { get_scene() }.delete(old.id.unwrap()).is_none());
        assert!(try_get_node(new).is_some());
    });
}

#[test]
fn forged_typed_handle_is_rejected() {
    with_scene(|| {
        let original = add_node(Value(1));
        let wrong = get_untyped_node(original.untyped())
            .unwrap()
            .handle::<Other>();
        // Only inspect Option; never dereference the incorrectly typed guard.
        assert!(try_get_node(wrong).is_none());
    });
}

#[test]
fn foreign_scene_handle_is_rejected() {
    with_scene(|| {
        let foreign = add_node(Value(1));
        let mut other_scene = Scene::new_at(0.0);
        other_scene.add_node(Value(2));
        let rejected = other_scene.get(foreign).is_none();
        other_scene.clear();
        assert!(rejected);
    });
}

#[test]
fn immediate_slot_reuse_does_not_duplicate_iteration() {
    with_scene(|| {
        let old = add_node(Value(1));
        get_node(old).delete();
        let new = add_node(Value(2));
        let ids: Vec<_> = all_nodes().map(|node| node.handle::<Value>().id).collect();
        assert_eq!(ids, [new.id]);
    });
}

#[test]
fn guards_and_capabilities_survive_registry_growth() {
    with_scene(|| {
        let a = add_node(Value(1));
        let b = add_node(Value(2));
        let mut typed = get_node(a);
        let mut untyped = get_untyped_node(b.untyped()).unwrap();
        let address = &*typed as *const Value;
        for _ in 0..2048 {
            add_node(Other(0));
        }
        typed.0 = 3;
        typed.provides(42u8);
        typed.persist();
        untyped.downcast_mut::<Value>().unwrap().0 = 4;
        let mut converted = untyped.to_typed::<Value>();
        converted.provides(43u8);
        assert_eq!(&*typed as *const Value, address);
        drop((typed, converted));
        let caps: Vec<_> = find_nodes_with::<u8>()
            .map(|node| node.capability)
            .collect();
        assert_eq!(caps, [42, 43]);
        clear_nodes();
        assert_eq!(get_node(a).0, 3);
        assert!(try_get_node(b).is_none());
        assert_eq!(events(), [(4, "drop")]);
    });
}

#[test]
fn guards_outlive_their_scene_and_cannot_modify_a_new_session() {
    with_scene(|| {
        let mut old_scene = Scene::new_at(0.0);
        let a = old_scene.add_node(Value(1));
        let b = old_scene.add_node(Value(2));
        let mut typed = old_scene.get(a).unwrap();
        let mut untyped = old_scene.get_any(b.untyped()).unwrap();
        drop(old_scene);
        assert!(events().is_empty());
        let fresh = add_node(Value(3));
        assert_eq!(fresh.id.unwrap().id, a.id.unwrap().id);
        assert!(try_get_node(a).is_none());
        typed.0 = 4;
        typed.provides(9u8);
        typed.persist(); // Only affects the detached entry.
        untyped.downcast_mut::<Value>().unwrap().0 = 5;
        typed.delete(); // Cannot delete the current scene's same-numbered slot.
        drop(untyped);
        assert_eq!(events(), [(4, "drop"), (5, "drop")]);
        assert_eq!(get_node(fresh).0, 3);
        assert_eq!(find_nodes_with::<u8>().count(), 0);
        clear_nodes();
        assert_eq!(events(), [(3, "drop")]);
    });
}

#[test]
fn wrong_conversion_and_panics_release_the_node_borrow() {
    with_scene(|| {
        let handle = add_node(Value(1));
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            get_untyped_node(handle.untyped())
                .unwrap()
                .to_typed::<Other>();
        }));
        assert!(result.is_err());
        assert_eq!(get_node(handle).0, 1);
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut node = get_node(handle);
            node.0 = 2;
            panic!("user code");
        }));
        assert!(result.is_err());
        assert_eq!(get_node(handle).0, 2);
    });
}

#[test]
fn rejected_delete_preserves_a_borrowed_node() {
    with_scene(|| {
        let handle = add_node(Value(1));
        let held = get_node(handle);
        assert!(std::panic::catch_unwind(AssertUnwindSafe(|| {
            unsafe { get_scene() }.delete(handle.id.unwrap());
        }))
        .is_err());
        assert_eq!(held.0, 1);
        assert!(events().is_empty());
        drop(held);
        get_node(handle).delete();
        assert_eq!(events(), [(1, "drop")]);
    });
}

#[repr(align(256))]
struct Aligned(u8);
impl Node for Aligned {}

struct Empty;
impl Node for Empty {}
impl Drop for Empty {
    fn drop(&mut self) {
        record(0, "empty-drop");
    }
}

#[test]
fn typed_allocations_support_alignment_zero_size_and_reused_slots() {
    with_scene(|| {
        let old = add_node(Value(1));
        get_node(old).delete();
        let aligned = add_node(Aligned(2));
        assert_eq!(aligned.id.unwrap().id, old.id.unwrap().id);
        let mut node = get_node(aligned);
        assert_eq!((&*node as *const Aligned as usize) % 256, 0);
        node.0 += 1;
        assert_eq!(node.0, 3);
        node.delete();
        let empty = add_node(Empty);
        get_node(empty).delete();
        assert_eq!(events(), [(1, "drop"), (0, "empty-drop")]);
    });
}

struct ReentrantDrop {
    anchor: Handle<Value>,
}
impl Node for ReentrantDrop {
    fn update(node: RefMut<Self>) {
        node.delete();
    }
}
impl Drop for ReentrantDrop {
    fn drop(&mut self) {
        get_node(self.anchor).0 += 1;
        record(0, "reenter");
        add_node(Other(7));
    }
}

#[test]
fn deletion_and_clear_allow_destructors_to_reenter_the_scene() {
    with_scene(|| {
        let anchor = add_node(Value(1));
        get_node(anchor).persist();
        let node = add_node(ReentrantDrop { anchor });
        get_node(node).delete();
        assert_eq!(get_node(anchor).0, 2);
        assert_eq!(find_node_by_type::<Other>().unwrap().0, 7);
        add_node(ReentrantDrop { anchor });
        clear_nodes();
        assert_eq!(get_node(anchor).0, 3);
        assert_eq!(find_nodes_by_type::<Other>().count(), 1);
        add_node(ReentrantDrop { anchor });
        tick(0.0); // Self-deletion from a callback also releases registry access.
        assert_eq!(get_node(anchor).0, 4);
        assert_eq!(events(), [(0, "reenter"), (0, "reenter"), (0, "reenter")]);
    });
}

struct PanicOnDrop;
impl Node for PanicOnDrop {}
impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        record(0, "panic-drop");
        panic!("node destructor");
    }
}

#[test]
fn panicking_destructor_does_not_leave_membership_or_borrow_state_stuck() {
    with_scene(|| {
        let handle = add_node(PanicOnDrop);
        assert!(std::panic::catch_unwind(AssertUnwindSafe(|| get_node(handle).delete())).is_err());
        assert!(try_get_node(handle).is_none());
        let fresh = add_node(Value(1));
        assert_eq!(get_node(fresh).0, 1);
        assert_eq!(events(), [(0, "panic-drop")]);
        let handle = add_node(PanicOnDrop);
        assert!(std::panic::catch_unwind(AssertUnwindSafe(clear_nodes)).is_err());
        assert!(try_get_node(handle).is_none());
        assert_eq!(all_nodes().count(), 0);
        assert_eq!(events(), [(1, "drop"), (0, "panic-drop")]);
        assert_eq!(get_node(add_node(Value(2))).0, 2);
    });
}

#[test]
fn iteration_survives_clear_and_skips_reused_snapshot_entries() {
    with_scene(|| {
        let old = add_node(Value(1));
        let mut snapshot = all_nodes();
        get_node(old).delete();
        let fresh = add_node(Value(2));
        assert!(snapshot.next().is_none());
        let mut snapshot = all_nodes();
        clear_nodes();
        add_node(Value(3));
        assert!(snapshot.next().is_none());
        assert!(try_get_node(fresh).is_none());
    });
}

struct Fields {
    values: Vec<f32>,
    selected: usize,
    // Nodes need not be Send for their handles or tween futures to be Send.
    owner: Rc<()>,
}
impl Node for Fields {}

fn fields() -> Fields {
    Fields {
        values: vec![1., 2.],
        selected: 1,
        owner: Rc::new(()),
    }
}

#[test]
fn projection_reads_initialized_data_and_holds_the_parent_borrow() {
    with_scene(|| {
        let handle = add_node(fields());
        let mut calls = 0;
        let mut lens = handle.lens(|node| {
            calls += 1;
            assert_eq!(Rc::strong_count(&node.owner), 1);
            &mut node.values[node.selected]
        });
        assert_eq!(
            lens.with(|field| {
                assert!(try_get_node(handle).is_none());
                assert!(std::panic::catch_unwind(AssertUnwindSafe(clear_nodes)).is_err());
                for _ in 0..128 {
                    add_node(Other(0));
                }
                *field += 10.;
                *field
            }),
            Some(12.)
        );
        assert_eq!(get_node(handle).values, [1., 12.]);
        get_node(handle).selected = 0;
        lens.with(|field| *field = 3.);
        assert_eq!(get_node(handle).values, [3., 12.]);
        drop(lens);
        assert_eq!(calls, 2); // Projection is retained and evaluated per access.
    });
}

#[test]
fn projection_unwinding_releases_guard_and_deleted_handles_stay_invalid() {
    with_scene(|| {
        let handle = add_node(fields());
        let mut lens = handle.lens(|node| &mut node.values[0]);
        assert!(std::panic::catch_unwind(AssertUnwindSafe(|| {
            lens.with(|_| panic!("field operation"));
        }))
        .is_err());
        assert_eq!(get_node(handle).values[0], 1.);
        get_node(handle).delete();
        add_node(fields());
        assert_eq!(lens.with(|field| *field), None);
        let handle = add_node(fields());
        let mut lens = handle.lens(|node| &mut node.values[99]);
        assert!(std::panic::catch_unwind(AssertUnwindSafe(|| lens.with(|_| ()))).is_err());
        assert!(try_get_node(handle).is_some());
    });
}

#[test]
fn tween_polling_releases_guards_and_supports_borrowed_projections() {
    use crate::experimental::coroutines::tweens::{follow_path, linear};
    use std::{
        future::Future,
        pin::{pin, Pin},
        task::{Context, Poll, Waker},
    };
    fn assert_send<T: Send>(_: &T) {}

    with_scene(|| {
        let handle = add_node(fields());
        assert_send(&handle);
        let mut cx = Context::from_waker(Waker::noop());
        let mut tween = linear(handle, |node| &mut node.values[0], 5., 10., f32::INFINITY);
        assert_send(&tween);
        assert_eq!(Pin::new(&mut tween).poll(&mut cx), Poll::Pending);
        assert_eq!(get_node(handle).values[0], 5.);
        get_node(handle).delete();
        let fresh = add_node(fields());
        assert_eq!(Pin::new(&mut tween).poll(&mut cx), Poll::Ready(()));
        assert_eq!(get_node(fresh).values[0], 1.);

        let mut calls = 0;
        {
            let mut projection = |node: &mut Fields| {
                calls += 1;
                node.selected = 0;
            };
            let mut tween = linear(
                fresh,
                |node| {
                    projection(node);
                    &mut node.values[node.selected]
                },
                2.,
                8.,
                0.,
            );
            assert_eq!(Pin::new(&mut tween).poll(&mut cx), Poll::Ready(()));
        }
        assert_eq!(calls, 1);
        assert_eq!(get_node(fresh).values[0], 8.);
        {
            let path = follow_path(
                fresh,
                |node| {
                    calls += 1;
                    &mut node.values[0]
                },
                vec![8., 9., 10.],
                0.,
            );
            assert_eq!(pin!(path).poll(&mut cx), Poll::Ready(()));
        }
        assert_eq!(calls, 3);
        assert_eq!(get_node(fresh).values[0], 10.);
    });
}

#[test]
fn exhausted_generations_retire_slots() {
    let mut scene = Scene::new_at(0.0);
    let old = scene.add_node(Other(1));
    drop(scene.delete(old.id.unwrap()));
    scene.free_slots[0].generation = u64::MAX;
    let new = scene.add_node(Other(2));
    assert_ne!(new.id.unwrap().id, old.id.unwrap().id);
    assert!(scene.get(old).is_none());
}

#[test]
fn sendable_handles_do_not_resolve_in_another_threads_scene() {
    let mut scene = Scene::new_at(0.0);
    let handle = scene.add_node(fields());
    std::thread::spawn(move || {
        // Use an independent registry; global concurrent access awaits step 3.
        let mut other = Scene::new_at(0.0);
        let local = other.add_node(fields());
        assert_eq!(local.id.unwrap().id, handle.id.unwrap().id);
        assert!(other.get(handle).is_none());
    })
    .join()
    .unwrap();
    assert!(scene.get(handle).is_some());
}
