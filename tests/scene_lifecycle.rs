//! Real-backend coverage for scene callbacks, cancellation and normal exit paths.
use macroquad::{
    experimental::{
        coroutines::{start_coroutine, stop_all_coroutines},
        scene::{self, Node, RefMut},
    },
    prelude::*,
    Window,
};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

type Events = Arc<Mutex<Vec<&'static str>>>;

struct Probe(Events);
impl Node for Probe {
    fn ready(node: RefMut<Self>) {
        node.0.lock().unwrap().push("ready");
        scene::add_node(Child {
            events: node.0.clone(),
            ready: false,
        });
    }
    fn update(node: RefMut<Self>) {
        assert!(get_time().is_finite());
        assert!(get_frame_time().is_finite());
        node.0.lock().unwrap().push("update");
    }
    fn fixed_update(_node: RefMut<Self>) {
        assert!((get_frame_time() - 1.0 / 60.0).abs() < 1e-6);
    }
    fn draw(node: RefMut<Self>) {
        assert_eq!(scene::camera_pos(), vec2(2., 2.));
        draw_rectangle(0., 0., 4., 4., Color::from_rgba(255, 0, 0, 255));
        node.0.lock().unwrap().push("draw");
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        // Teardown must run while graphics and coroutine APIs are still usable.
        stop_all_coroutines();
        let texture = Texture2D::from_rgba8(1, 1, &[1, 2, 3, 255]);
        assert_eq!(
            texture.get_texture_data().get_pixel(0, 0),
            Color::from_rgba(1, 2, 3, 255)
        );
        self.0.lock().unwrap().push("node-drop");
    }
}

struct Child {
    events: Events,
    ready: bool,
}
impl Node for Child {
    fn ready(mut node: RefMut<Self>) {
        node.ready = true;
        node.events.lock().unwrap().push("child-ready");
    }
    fn update(node: RefMut<Self>) {
        assert!(node.ready);
    }
    fn draw(node: RefMut<Self>) {
        assert!(node.ready);
    }
}

struct DropToken(Events);
impl Drop for DropToken {
    fn drop(&mut self) {
        self.0.lock().unwrap().push("spawn-drop");
    }
}
struct CancelProbe {
    node: scene::Handle<Probe>,
    events: Events,
}
impl Future for CancelProbe {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        Poll::Pending
    }
}
impl Drop for CancelProbe {
    fn drop(&mut self) {
        assert!(scene::try_get_node(self.node).is_some());
        self.events.lock().unwrap().push("cancel");
        // Cancellation detaches a snapshot; futures created by destructors remain
        // scheduled. No scheduler or scene borrow may surround this destructor.
        let token = DropToken(self.events.clone());
        start_coroutine(async move {
            let _token = token;
            std::future::pending::<()>().await;
        });
    }
}

fn config() -> Conf {
    Conf {
        window_title: "Scene lifecycle test".into(),
        window_width: 64,
        window_height: 64,
        window_resizable: false,
        ..Default::default()
    }
}

fn install(events: Events) -> (scene::Handle<Probe>, RenderTarget) {
    let node = scene::add_node(Probe(events));
    scene::get_node(node).persist();
    let target = render_target(4, 4);
    scene::set_camera(
        0,
        Some(Camera2D {
            render_target: Some(target.clone()),
            ..Camera2D::from_display_rect(Rect::new(0., 0., 4., 4.))
        }),
    );
    (node, target)
}

#[test]
fn rendering_cancellation_completion_and_accepted_quit() {
    const SCENARIO: &str = "MACROQUAD_SCENE_TEST_EXIT";
    // Miniquad's native display is process-global and cannot be initialized twice.
    // Isolate each real-backend scenario; headless tests cover session replacement.
    let Ok(scenario) = std::env::var(SCENARIO) else {
        for scenario in ["completion", "quit"] {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "rendering_cancellation_completion_and_accepted_quit",
                    "--nocapture",
                ])
                .env(SCENARIO, scenario)
                .status()
                .unwrap();
            assert!(status.success(), "{scenario} scenario failed");
        }
        return;
    };
    assert!(scenario == "completion" || scenario == "quit");
    let events = Events::default();
    let saved = Arc::new(Mutex::new(None));
    let (log, handles) = (events.clone(), saved.clone());
    Window::from_config(config(), async move {
        let (node, target) = install(log.clone());
        *handles.lock().unwrap() = Some(node);
        let coroutine = start_coroutine(CancelProbe {
            node,
            events: log.clone(),
        });
        next_frame().await;
        assert!(!log.lock().unwrap().contains(&"child-ready"));
        next_frame().await;
        assert!(log.lock().unwrap().contains(&"child-ready"));
        assert_eq!(
            target.texture.get_texture_data().get_pixel(1, 1),
            Color::from_rgba(255, 0, 0, 255)
        );
        scene::clear();
        assert!(coroutine.is_done());
        assert!(scene::try_get_node(node).is_some());
        assert!(!log.lock().unwrap().contains(&"spawn-drop"));
        if scenario == "quit" {
            miniquad::window::request_quit();
            // Exercise accepted quit while the main future remains pending.
            loop {
                next_frame().await;
            }
        }
        // Completion or accepted quit cancels the new future before node teardown.
    });
    let old = saved.lock().unwrap().unwrap();
    assert!(scene::try_get_node(old).is_none());
    let log = events.lock().unwrap();
    assert!(
        log.iter().position(|e| *e == "spawn-drop").unwrap()
            < log.iter().position(|e| *e == "node-drop").unwrap()
    );
    assert_eq!(log.iter().filter(|event| **event == "node-drop").count(), 1);
    assert_eq!(scene::all_nodes().count(), 0);
}
