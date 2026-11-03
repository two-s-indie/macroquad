//! Each native session runs in a child process; Miniquad cannot reopen its
//! process-global native display. Headless tests cover repeated context slots.
use macroquad::{
    audio,
    experimental::{coroutines::start_coroutine, scene},
    prelude::*,
    ui::root_ui,
    Window,
};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

type Events = Arc<Mutex<Vec<&'static str>>>;
struct Probe {
    name: &'static str,
    events: Events,
    engine_available: bool,
    spawn: bool,
    panic: bool,
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.events.lock().unwrap().push(self.name);
        if self.engine_available {
            assert!(get_time().is_finite());
            let texture = Texture2D::from_rgba8(1, 1, &[11, 22, 33, 255]);
            assert_eq!(
                texture.get_texture_data().get_pixel(0, 0),
                Color::from_rgba(11, 22, 33, 255)
            );
            draw_text("teardown", 0., 20., 14., WHITE);
            if self.spawn {
                let token = Probe {
                    name: "spawned-drop",
                    events: self.events.clone(),
                    engine_available: true,
                    spawn: false,
                    panic: false,
                };
                start_coroutine(async move {
                    let _token = token;
                    std::future::pending::<()>().await;
                });
            }
        }
        if self.panic {
            panic!("intentional UI teardown panic");
        }
    }
}

static UNIFORM_DROPPED: AtomicBool = AtomicBool::new(false);
#[repr(transparent)]
struct ReenterUniform(f32);
impl Drop for ReenterUniform {
    fn drop(&mut self) {
        assert!(get_time().is_finite());
        UNIFORM_DROPPED.store(true, Ordering::SeqCst);
    }
}

#[test]
fn context_teardown_releases_user_state_and_rejects_late_access() {
    const SCENARIO: &str = "MACROQUAD_CONTEXT_LIFECYCLE";
    let Ok(scenario) = std::env::var(SCENARIO) else {
        for scenario in ["completion", "quit", "unwind", "shutdown_panic"] {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "context_teardown_releases_user_state_and_rejects_late_access",
                    "--nocapture",
                ])
                .env(SCENARIO, scenario)
                .status()
                .unwrap();
            assert!(status.success(), "{scenario} failed");
        }
        return;
    };
    let events = Events::default();
    let late = Arc::new(Mutex::new(None));
    let saved = late.clone();
    let log = events.clone();
    let normal = scenario != "unwind";
    let panics = scenario == "unwind" || scenario == "shutdown_panic";
    assert!(catch_unwind(get_time).is_err());
    let result = catch_unwind(AssertUnwindSafe(|| {
        Window::from_config(
            Conf {
                window_title: "Context teardown test".into(),
                window_width: 320,
                window_height: 240,
                window_resizable: false,
                ..Default::default()
            },
            async move {
                let _main = Probe {
                    name: "main-drop",
                    events: log.clone(),
                    engine_available: normal,
                    spawn: false,
                    panic: false,
                };
                *root_ui().get_any::<Option<Probe>>(1) = Some(Probe {
                    name: "ui-drop",
                    events: log.clone(),
                    engine_available: normal,
                    spawn: normal,
                    panic: scenario == "shutdown_panic",
                });
                if normal {
                    let token = Probe {
                        name: "recovery-drop",
                        events: log.clone(),
                        engine_available: true,
                        spawn: false,
                        panic: false,
                    };
                    set_panic_handler(move |_, _| {
                        let _ = &token;
                        async {}
                    });
                }
                assert!(std::thread::spawn(|| catch_unwind(get_time).is_err())
                    .join()
                    .unwrap());
                assert!(std::thread::spawn(
                    || catch_unwind(|| Window::new("overlap", async {})).is_err()
                )
                .join()
                .unwrap());
                assert!(get_time().is_finite());

                const VERTEX: &str = "#version 100\nattribute vec3 position; void main() { gl_Position = vec4(position, 1.0); }";
                const FRAGMENT: &str = "#version 100\nprecision mediump float; uniform float Test; void main() { gl_FragColor = vec4(Test, 0.0, 0.0, 1.0); }";
                let material = load_material(
                    ShaderSource::Glsl {
                        vertex: VERTEX,
                        fragment: FRAGMENT,
                    },
                    MaterialParams {
                        uniforms: vec![UniformDesc::new("Test", UniformType::Float1)],
                        ..Default::default()
                    },
                )
                .unwrap();
                material.set_uniform("Test", ReenterUniform(1.));
                assert!(UNIFORM_DROPPED.load(Ordering::SeqCst));
                let sound = audio::load_sound_from_bytes(include_bytes!("../examples/sound.wav"))
                    .await
                    .unwrap();
                audio::set_sound_volume(&sound, 0.);
                audio::stop_sound(&sound);
                let another = audio::load_sound_from_bytes(include_bytes!("../examples/sound.wav"))
                    .await
                    .unwrap();
                with_internal_gl(|_| drop(another));
                *saved.lock().unwrap() = Some((
                    sound,
                    Texture2D::from_rgba8(1, 1, &[255; 4]),
                    render_target(1, 1),
                ));
                next_frame().await;
                if scenario == "unwind" {
                    panic!("intentional main panic");
                }
                if scenario == "quit" {
                    miniquad::window::request_quit();
                    loop {
                        next_frame().await;
                    }
                }
            },
        )
    }));
    assert_eq!(result.is_err(), panics);
    assert!(catch_unwind(get_time).is_err());
    assert!(catch_unwind(|| with_internal_gl(|_| ())).is_err());
    assert!(catch_unwind(root_ui).is_err());
    assert_eq!(scene::all_nodes().count(), 0);
    assert!(!macroquad::telemetry::is_enabled());
    // Resource release after native teardown, including on another thread,
    // must not need the context/backend. The sound is never played.
    let resources = late.lock().unwrap().take().unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| resources
        .2
        .render_pass
        .raw_miniquad_id()))
    .is_err());
    std::thread::spawn(move || drop(resources)).join().unwrap();
    let events = events.lock().unwrap();
    assert_eq!(
        events.iter().filter(|&&event| event == "main-drop").count(),
        1
    );
    assert_eq!(
        events.iter().filter(|&&event| event == "ui-drop").count(),
        1
    );
    if normal {
        assert_eq!(
            events
                .iter()
                .filter(|&&event| event == "recovery-drop")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|&&event| event == "spawned-drop")
                .count(),
            1
        );
        assert!(
            events.iter().position(|&e| e == "ui-drop")
                < events.iter().position(|&e| e == "spawned-drop")
        );
    }
}
