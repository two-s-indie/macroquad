//! Characterization tests for the legacy scene, not a proof of aliasing safety.
//! Ignored tests specify known defects to fix during the ownership migration.

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

// All tests touching SCENE use this fixture. Restore the previous global even
// on unwind. Legacy Scene has no node-dropping destructor, so explicitly drop
// remaining test values before freeing the arena. Test destructors only log.
fn with_scene(test: impl FnOnce()) {
    let _lock = SCENE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    struct Restore(Option<Scene>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let scene = unsafe { std::ptr::replace(&raw mut SCENE, self.0.take()) };
            if let Some(mut scene) = scene {
                for cell in scene.nodes.iter_mut().filter_map(Option::take) {
                    assert!(!unsafe { *cell.used }, "test leaked a guard");
                    unsafe { (*cell.virtual_drop)(cell.data) };
                }
            }
        }
    }
    let _restore = Restore(unsafe { std::ptr::replace(&raw mut SCENE, Some(Scene::new_at(0.0))) });
    events();
    DELETE_TARGET.with(|target| *target.borrow_mut() = None);
    test();
}

fn tick(now: f64) {
    unsafe { get_scene() }.update_with(|| now, |_| {}, || {});
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
        unsafe { get_scene() }.update_with(
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
#[ignore = "step 2: clear must preflight live guards before removing any entries"]
fn rejected_clear_leaves_all_nodes_registered() {
    with_scene(|| {
        let a = add_node(Value(1));
        let b = add_node(Value(2));
        let held = get_node(b);
        assert!(
            std::panic::catch_unwind(AssertUnwindSafe(|| unsafe { get_scene() }.clear())).is_err()
        );
        drop(held); // Never dereference a node after failed clear.
        assert!(try_get_node(a).is_some());
        assert!(try_get_node(b).is_some());
    });
}

#[test]
#[ignore = "step 2: stale deletion must validate before taking the current entry"]
fn stale_delete_does_not_remove_reused_slot() {
    with_scene(|| {
        let old = add_node(Value(1));
        get_node(old).delete();
        tick(0.0);
        let new = add_node(Value(2));
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            unsafe { get_scene() }.delete(old.id.unwrap())
        }));
        assert!(try_get_node(new).is_some());
    });
}

#[test]
#[ignore = "step 2: typed lookup must check the stored node type"]
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
#[ignore = "steps 2/3: handles need scene identity as well as slot generation"]
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
#[ignore = "steps 2/3: iteration must validate the dense entry generation"]
fn immediate_slot_reuse_does_not_duplicate_iteration() {
    with_scene(|| {
        let old = add_node(Value(1));
        get_node(old).delete();
        let new = add_node(Value(2));
        let ids: Vec<_> = all_nodes().map(|node| node.handle::<Value>().id).collect();
        assert_eq!(ids, [new.id]);
    });
}
