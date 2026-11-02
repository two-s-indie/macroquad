use super::*;
use std::panic::catch_unwind;

fn fresh_profiler() {
    PROFILER.with_borrow_mut(|profiler| *profiler = Profiler::new());
}

fn finish_frame(frame_time: f32) -> Option<bool> {
    PROFILER.with_borrow_mut(|profiler| profiler.reset(frame_time))
}

fn recorded_drawcall(indices_count: usize) -> DrawCallTelemetry {
    // A diagnostic record only; no rendering operation uses this handle.
    DrawCallTelemetry {
        indices_count,
        texture: miniquad::TextureId::from_raw_id(miniquad::RawId::OpenGl(0)),
    }
}

#[test]
fn nested_zones_survive_frame_reset_and_snapshots_are_owned() {
    fresh_profiler();
    PROFILER.with_borrow_mut(|profiler| {
        profiler.begin_zone("root", 1.0);
        // Enough siblings to grow the children vector while nesting zones.
        for index in 0..16 {
            let now = 2.0 + index as f64 * 2.0;
            profiler.begin_zone("child", now);
            profiler.begin_zone("grandchild", now + 0.25);
            profiler.end_zone(now + 0.75);
            profiler.end_zone(now + 1.0);
        }
        profiler.end_zone(35.0);
    });

    assert_eq!(finish_frame(0.25), None);
    let snapshot = frame();
    assert_eq!(snapshot.full_frame_time, 0.25);
    assert_eq!(snapshot.zones.len(), 1);
    let root = &snapshot.zones[0];
    assert_eq!(root.name, "root");
    assert_eq!(root.duration, 34.0);
    assert_eq!(root.children.len(), 16);
    for child in &root.children {
        assert_eq!(child.duration, 1.0);
        assert_eq!(child.children[0].duration, 0.5);
    }
    PROFILER.with_borrow(|profiler| {
        assert!(profiler.frame.zones.is_empty());
        assert!(profiler.frame.active_zone.is_null());
    });

    finish_frame(0.5);
    assert!(frame().zones.is_empty());
    assert_eq!(frame().full_frame_time, 0.5);
    assert_eq!(snapshot.zones[0].children.len(), 16);
}

#[test]
fn unbalanced_zone_panics_release_the_profiler_borrow() {
    fresh_profiler();
    assert!(catch_unwind(|| PROFILER.with_borrow_mut(|p| p.end_zone(0.0))).is_err());
    PROFILER.with_borrow_mut(|p| p.begin_zone("unfinished", 1.0));
    assert!(catch_unwind(|| finish_frame(0.1)).is_err());
    PROFILER.with_borrow_mut(|p| p.end_zone(2.0));
    finish_frame(0.1);
    assert_eq!(frame().zones[0].duration, 1.0);
}

#[test]
fn enable_disable_requests_take_effect_at_frame_boundaries() {
    fresh_profiler();
    // Disabled zones must remain usable without a graphics context.
    drop(ZoneGuard::new("disabled"));
    enable();
    assert!(!is_enabled());
    finish_frame(0.1);
    assert!(is_enabled());
    disable();
    assert!(is_enabled());
    finish_frame(0.1);
    assert!(!is_enabled());
    drop(ZoneGuard::new("disabled again"));

    enable();
    disable();
    finish_frame(0.1);
    assert!(!is_enabled());
}

#[test]
fn capture_requests_start_next_frame_and_preserve_completed_results() {
    fresh_profiler();
    assert_eq!(finish_frame(0.1), None);
    PROFILER.with_borrow_mut(|p| p.drawcalls.push(recorded_drawcall(3)));
    capture_frame();
    assert_eq!(drawcalls().len(), 1);
    assert_eq!(finish_frame(0.1), Some(true));
    assert!(drawcalls().is_empty());
    PROFILER.with_borrow(|p| {
        assert!(p.capture);
        assert!(!p.capture_request);
    });

    PROFILER.with_borrow_mut(|p| p.drawcalls.push(recorded_drawcall(6)));
    let snapshot = drawcalls();
    assert_eq!(finish_frame(0.1), Some(false));
    assert_eq!(drawcalls()[0].indices_count, 6);
    assert_eq!(finish_frame(0.1), None);
    assert_eq!(drawcalls()[0].indices_count, 6);

    capture_frame();
    assert_eq!(finish_frame(0.1), Some(true));
    PROFILER.with_borrow_mut(|p| p.drawcalls.push(recorded_drawcall(9)));
    // Back-to-back captures stay enabled but discard the preceding results.
    capture_frame();
    assert_eq!(finish_frame(0.1), Some(true));
    assert!(drawcalls().is_empty());
    assert_eq!(snapshot[0].indices_count, 6);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn telemetry_state_is_local_to_each_thread() {
    fresh_profiler();
    enable();
    finish_frame(0.1);
    log_string("parent");
    let snapshot = strings();
    log_string("later");
    assert_eq!(snapshot, ["parent"]);

    std::thread::spawn(|| {
        assert!(!is_enabled());
        assert!(strings().is_empty());
        assert!(frame().zones.is_empty());
        log_string("child");
        capture_frame();
        assert_eq!(strings(), ["child"]);
    })
    .join()
    .unwrap();

    assert!(is_enabled());
    assert_eq!(strings(), ["parent", "later"]);
    assert_eq!(finish_frame(0.1), None);
}
