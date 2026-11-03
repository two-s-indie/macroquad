use macroquad::{experimental::coroutines::*, prelude::*, ui::root_ui, Window};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
};

struct PanicFuture(Arc<AtomicUsize>);
impl Future for PanicFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        panic!("intentional coroutine panic");
    }
}
impl Drop for PanicFuture {
    fn drop(&mut self) {
        // Reenter both the scheduler and engine during unwinding.
        start_coroutine(async {});
        let texture = Texture2D::from_rgba8(1, 1, &[1, 2, 3, 255]);
        assert_eq!(texture.size(), vec2(1., 1.));
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn callbacks_and_panic_recovery_can_reenter_engine() {
    let old_hook = std::panic::take_hook();
    let dropped = Arc::new(AtomicUsize::new(0));
    let recovered = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(Mutex::new(None::<Coroutine>));
    let (drops, recovery, handle) = (dropped.clone(), recovered.clone(), failed.clone());
    Window::from_config(
        Conf {
            window_title: "Coroutine reentry test".into(),
            window_width: 64,
            window_height: 64,
            ..Default::default()
        },
        async move {
            set_panic_handler(move |message, _| {
                assert!(message.contains("coroutine_reentry.rs"));
                assert_eq!(drops.load(Ordering::SeqCst), 1);
                assert!(handle.lock().unwrap().unwrap().is_done());
                // The factory itself runs after unwind, without a context borrow.
                stop_all_coroutines();
                assert!(get_time().is_finite());
                clear_background(BLACK);
                let recovery = recovery.clone();
                async move {
                    let delayed = start_coroutine(async {
                        wait_seconds(60.).await;
                    });
                    next_frame().await;
                    // The failed manual poll used a huge delta. It must not leak
                    // into the next automatic coroutine's wait_seconds.
                    assert!(!delayed.is_done());
                    stop_coroutine(delayed);
                    recovery.store(true, Ordering::SeqCst);
                }
            });

            let target = render_target(4, 4);
            let destination = target.clone();
            let child_done = Arc::new(AtomicBool::new(false));
            let done = child_done.clone();
            start_coroutine(async move {
                let mut child = start_coroutine(async move {
                    set_camera(&Camera2D {
                        render_target: Some(destination),
                        ..Camera2D::from_display_rect(Rect::new(0., 0., 4., 4.))
                    });
                    clear_background(Color::from_rgba(255, 0, 0, 255));
                    draw_rectangle(0., 0., 4., 4., Color::from_rgba(255, 0, 0, 255));
                    set_default_camera();
                    let _ = (
                        mouse_position(),
                        mouse_delta_position(),
                        is_key_down(KeyCode::A),
                        get_frame_time(),
                    );
                    root_ui().label(None, "Coroutine UI");
                    done.store(true, Ordering::SeqCst);
                });
                child.set_manual_poll();
                child.poll(0.1);
                assert!(child.is_done());
            });
            next_frame().await;
            assert!(child_done.load(Ordering::SeqCst));
            assert_eq!(
                target.texture.get_texture_data().get_pixel(1, 1),
                Color::from_rgba(255, 0, 0, 255)
            );

            let mut bomb = start_coroutine(PanicFuture(dropped));
            bomb.set_manual_poll();
            *failed.lock().unwrap() = Some(bomb);
            bomb.poll(1_000_000.);
            panic!("panic did not propagate");
        },
    );
    std::panic::set_hook(old_hook);
    assert!(recovered.load(Ordering::SeqCst));
}
