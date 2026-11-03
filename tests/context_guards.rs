use macroquad::{
    prelude::*,
    ui::{hash, root_ui, widgets, Ui},
};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn rejects_context_access(f: impl FnOnce()) {
    let panic = catch_unwind(AssertUnwindSafe(f)).expect_err("nested engine access must fail");
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap();
    assert!(
        message.contains("already borrowed by with_internal_gl"),
        "{message}"
    );
}

// One window per process, exercising the guards against an actual backend.
#[macroquad::test]
async fn ui_and_gl_access_have_owned_or_scoped_lifetimes() {
    let mut ui = root_ui();
    *ui.get_bool(hash!("guard-state")) = true;
    assert!(catch_unwind(AssertUnwindSafe(root_ui)).is_err());
    assert!(*ui.get_bool(hash!("guard-state")));
    widgets::Window::new(hash!("guard-window"), vec2(10., 10.), vec2(160., 80.)).ui(
        &mut ui,
        |ui| {
            assert!(get_time().is_finite());
            let texture = Texture2D::from_rgba8(1, 1, &[255; 4]);
            draw_texture(&texture, 0., 0., WHITE);
            ui.label(None, "Engine access inside UI");
            with_internal_gl(|mut gl| gl.flush());
        },
    );
    drop(ui);
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let mut ui = root_ui();
        *ui.get_bool(hash!("guard-state")) = false;
        panic!("release UI during unwind");
    }))
    .is_err());
    assert!(!*root_ui().get_bool(hash!("guard-state")));
    root_ui().label(None, "UI available after unwind");
    next_frame().await;

    let texture = Texture2D::from_rgba8(1, 1, &[0, 255, 0, 255]);
    let expected_id = texture.raw_miniquad_id();
    let screen_size = (screen_width(), screen_height());
    let projection = with_internal_gl(|gl| {
        assert_eq!(
            gl.get_viewport(),
            (0, 0, screen_size.0 as i32, screen_size.1 as i32)
        );
        assert_eq!(gl.texture_id(&texture), expected_id);
        gl.get_projection_matrix()
    });
    let expected_projection =
        glam::camera::rh::proj::opengl::orthographic(0., screen_size.0, screen_size.1, 0., -1., 1.);
    assert_eq!(projection, expected_projection);

    with_internal_gl(|mut gl| {
        rejects_context_access(|| with_internal_gl(|_| ()));
        rejects_context_access(|| {
            screen_width();
        });
        rejects_context_access(|| {
            texture.size();
        });
        rejects_context_access(|| draw_rectangle(0., 0., 1., 1., WHITE));
        rejects_context_access(|| {
            root_ui();
        });
        rejects_context_access(|| macroquad::Window::new("nested window", async {}));
        // Failed nested calls must not release the outer scope or damage it.
        assert_eq!(gl.texture_id(&texture), expected_id);
        gl.flush();
        let mut ui = gl.new_ui(64., 64.);
        *ui.get_bool(1) = true;
        assert!(*ui.get_bool(1));
    });
    assert!(catch_unwind(AssertUnwindSafe(|| with_internal_gl(|_| {
        panic!("release GL during unwind");
    })))
    .is_err());
    assert!(get_time().is_finite());
    with_internal_gl(|mut gl| gl.flush());
    drop(Ui::new(64., 64.));

    let target = render_target(64, 64);
    let camera = Camera2D {
        render_target: Some(target.clone()),
        viewport: Some((0, 0, 64, 64)),
        ..Camera2D::from_display_rect(Rect::new(0., 0., 64., 64.))
    };
    set_camera(&camera);
    clear_background(BLACK);
    draw_rectangle(0., 0., 32., 64., Color::from_rgba(255, 0, 0, 255));
    with_internal_gl(|mut gl| {
        assert_eq!(gl.get_projection_matrix(), camera.matrix());
        assert_eq!(gl.get_viewport(), (0, 0, 64, 64));
        gl.flush();
        gl.texture(Some(&texture));
        gl.quad_gl.draw_mode(DrawMode::Triangles);
        gl.quad_gl.geometry(
            &[
                Vertex::new(32., 0., 0., 0., 0., WHITE),
                Vertex::new(64., 0., 0., 1., 0., WHITE),
                Vertex::new(64., 64., 0., 1., 1., WHITE),
                Vertex::new(32., 64., 0., 0., 1., WHITE),
            ],
            &[0, 1, 2, 0, 2, 3],
        );
        gl.flush();
        gl.texture(None);
        // Destruction while the GL scope is active must remain queue-only.
        drop(texture);
    });
    draw_rectangle(0., 0., 8., 64., Color::from_rgba(0, 0, 255, 255));
    set_default_camera();
    let pixels = target.texture.get_texture_data();
    assert_eq!(pixels.get_pixel(4, 32), Color::from_rgba(0, 0, 255, 255));
    assert_eq!(pixels.get_pixel(16, 32), Color::from_rgba(255, 0, 0, 255));
    assert_eq!(pixels.get_pixel(48, 32), Color::from_rgba(0, 255, 0, 255));

    // Exercise migrated particle creation, textured rendering, cache mesh rebuild,
    // projection lookup, and the post-processing viewport path.
    use macroquad_particles::{ColorCurve, Emitter, EmitterConfig, EmittersCache, PostProcessing};
    let particle_texture = Texture2D::from_rgba8(1, 1, &[0, 255, 0, 255]);
    let config = EmitterConfig {
        texture: Some(particle_texture),
        amount: 1,
        size: 8.,
        lifetime: 100.,
        initial_velocity: 0.,
        explosiveness: 1.,
        colors_curve: ColorCurve {
            start: WHITE,
            mid: WHITE,
            end: WHITE,
        },
        ..Default::default()
    };
    set_camera(&camera);
    clear_background(BLACK);
    let mut emitter = Emitter::new(EmitterConfig {
        emitting: false,
        ..config.clone()
    });
    emitter.emit(vec2(16., 16.), 1);
    emitter.draw(Vec2::ZERO);
    let mut cache = EmittersCache::new(config.clone());
    cache.spawn(vec2(48., 48.));
    cache.draw();
    set_default_camera();
    let pixels = target.texture.get_texture_data();
    // OpenGL readback rows run opposite to this camera's world Y axis.
    assert_eq!(pixels.get_pixel(16, 48), Color::from_rgba(0, 255, 0, 255));
    assert_eq!(pixels.get_pixel(48, 16), Color::from_rgba(0, 255, 0, 255));
    set_camera(&camera);
    let mut post = Emitter::new(EmitterConfig {
        post_processing: Some(PostProcessing),
        ..config
    });
    post.draw(vec2(32., 32.));
    set_default_camera();
    unsafe {
        assert_eq!(miniquad::gl::glGetError(), miniquad::gl::GL_NO_ERROR);
    }
    next_frame().await;
}
