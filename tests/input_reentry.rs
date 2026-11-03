#![cfg(target_os = "windows")]
use macroquad::{
    input::utils::*,
    prelude::*,
    ui::{root_ui, widgets},
    Window,
};
use miniquad::EventHandler;
use winapi::{shared::windef::HWND, um::winuser::*};

fn motion(hwnd: HWND, x: u16, y: u16) {
    unsafe {
        SendMessageW(
            hwnd,
            WM_MOUSEMOVE,
            0,
            ((y as u32) << 16 | x as u32) as isize,
        );
    }
}

struct Replay {
    hwnd: HWND,
    subscriber: usize,
    seen: Vec<f32>,
    inject: bool,
    fail: bool,
}
impl EventHandler for Replay {
    fn update(&mut self) {}
    fn draw(&mut self) {}
    fn mouse_motion_event(&mut self, x: f32, _: f32) {
        self.seen.push(x);
        assert!(get_frame_time().is_finite());
        clear_background(BLACK);
        // Force the subscriber vector to grow while replay is active.
        for _ in 0..100 {
            register_input_subscriber();
        }
        if self.inject {
            self.inject = false;
            repeat_all_miniquad_input(self, self.subscriber);
            motion(self.hwnd, 30, 10);
        }
        if self.fail {
            self.fail = false;
            panic!("intentional input callback panic");
        }
    }
}

#[test]
fn input_snapshots_and_replay_allow_engine_reentry() {
    Window::from_config(
        Conf {
            window_title: "Input snapshot reentry".into(),
            window_width: 320,
            window_height: 240,
            ..Default::default()
        },
        async {
            let title: Vec<_> = "Input snapshot reentry\0".encode_utf16().collect();
            let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
            assert!(!hwnd.is_null());
            let subscriber = register_input_subscriber();
            motion(hwnd, 10, 10);
            motion(hwnd, 20, 10);
            // Miniquad defers messages received while a draw callback is active.
            next_frame().await;
            let mut replay = Replay {
                hwnd,
                subscriber,
                seen: vec![],
                inject: true,
                fail: true,
            };
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                repeat_all_miniquad_input(&mut replay, subscriber)
            }))
            .is_err());
            repeat_all_miniquad_input(&mut replay, subscriber);
            assert_eq!(replay.seen, vec![10., 20.]);
            next_frame().await;
            repeat_all_miniquad_input(&mut replay, subscriber);
            assert_eq!(replay.seen, vec![10., 20., 30.]);
            repeat_all_miniquad_input(&mut replay, subscriber);
            assert_eq!(replay.seen.len(), 3);

            // Deliver an actual mouse click to the UI snapshot on the next frame.
            next_frame().await;
            motion(hwnd, 10, 10);
            unsafe {
                SendMessageW(hwnd, WM_LBUTTONDOWN, 1, (10 << 16) | 10);
                SendMessageW(hwnd, WM_LBUTTONUP, 0, (10 << 16) | 10);
            }
            next_frame().await;
            assert!(widgets::Button::new("click")
                .position(vec2(0., 0.))
                .size(vec2(30., 30.))
                .ui(&mut root_ui()));
        },
    );
}
