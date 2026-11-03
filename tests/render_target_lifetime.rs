use macroquad::prelude::*;

#[macroquad::test]
async fn render_target_texture_outlives_its_target() {
    for sample_count in [1, 4] {
        if sample_count > 1
            && !with_internal_gl(|gl| gl.quad_context.info().features.resolve_attachments)
        {
            continue;
        }
        for depth in [false, true] {
            let texture_count = macroquad::telemetry::textures_count();
            let target = render_target_ex(
                4,
                4,
                RenderTargetParams {
                    sample_count,
                    depth,
                },
            );
            set_camera(&Camera2D {
                render_target: Some(target.clone()),
                ..Camera2D::from_display_rect(Rect::new(0.0, 0.0, 4.0, 4.0))
            });
            clear_background(Color::from_rgba(255, 0, 0, 255));
            draw_rectangle(0.0, 0.0, 4.0, 4.0, Color::from_rgba(255, 0, 0, 255));
            set_default_camera();

            let texture = target.texture.clone();
            let weak = texture.weak_clone();
            let texture_id = texture.raw_miniquad_id();
            let pass = target.render_pass.clone();
            drop(target);
            next_frame().await;
            assert_eq!(
                pass.color_texture.get_texture_data().get_pixel(1, 1),
                Color::from_rgba(255, 0, 0, 255)
            );
            drop(pass);
            next_frame().await;

            assert_eq!(texture.raw_miniquad_id(), texture_id);
            let image = texture.get_texture_data();
            assert_eq!(image.get_pixel(1, 1), Color::from_rgba(255, 0, 0, 255));

            drop(texture);
            next_frame().await;
            assert_ne!(weak.raw_miniquad_id(), texture_id);
            assert_eq!(macroquad::telemetry::textures_count(), texture_count);
            unsafe { assert_eq!(miniquad::gl::glGetError(), miniquad::gl::GL_NO_ERROR) };
        }
    }
}
