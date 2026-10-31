use macroquad::prelude::*;

fn material(color: &str) -> Material {
    load_material(
        ShaderSource::Glsl {
            vertex: r#"#version 100
                attribute vec3 position;
                uniform mat4 Model;
                uniform mat4 Projection;
                void main() {
                    gl_Position = Projection * Model * vec4(position, 1.0);
                }"#,
            fragment: &format!(
                "#version 100\nprecision mediump float;\nvoid main() {{ gl_FragColor = {color}; }}"
            ),
        },
        MaterialParams::default(),
    )
    .unwrap()
}

#[macroquad::test]
async fn dropped_materials_finish_queued_draws() {
    let target = render_target(6, 2);
    set_camera(&Camera2D {
        render_target: Some(target.clone()),
        ..Camera2D::from_display_rect(Rect::new(0.0, 0.0, 6.0, 2.0))
    });
    clear_background(BLACK);

    let red = material("vec4(1.0, 0.0, 0.0, 1.0)");
    gl_use_material(&red);
    draw_rectangle(0.0, 0.0, 2.0, 2.0, WHITE);
    drop(red);

    // Dropping the active material restores the default for subsequent draws.
    draw_rectangle(2.0, 0.0, 2.0, 2.0, Color::from_rgba(0, 255, 0, 255));

    // A newly loaded material must not replace the pipeline of a queued draw.
    let blue = material("vec4(0.0, 0.0, 1.0, 1.0)");
    gl_use_material(&blue);
    draw_rectangle(4.0, 0.0, 2.0, 2.0, WHITE);
    drop(blue);
    set_default_camera();

    let image = target.texture.get_texture_data();
    assert_eq!(image.get_pixel(0, 0), Color::from_rgba(255, 0, 0, 255));
    assert_eq!(image.get_pixel(2, 0), Color::from_rgba(0, 255, 0, 255));
    assert_eq!(image.get_pixel(4, 0), Color::from_rgba(0, 0, 255, 255));

    let owned = material("vec4(1.0)");
    gl_use_material(&owned);
    draw_rectangle(0.0, 0.0, 1.0, 1.0, WHITE);
    unsafe { get_internal_gl().flush() };
    // OpenGL keeps the currently bound program alive until another is bound,
    // so its delete status directly verifies that the material released it.
    unsafe {
        const GL_CURRENT_PROGRAM: u32 = 0x8B8D;
        const GL_DELETE_STATUS: u32 = 0x8B80;
        let mut program = 0;
        miniquad::gl::glGetIntegerv(GL_CURRENT_PROGRAM, &mut program);
        assert_ne!(program, 0);
        drop(owned);
        get_internal_gl().flush();
        let mut deleted = 0;
        miniquad::gl::glGetProgramiv(program as u32, GL_DELETE_STATUS, &mut deleted);
        assert_eq!(miniquad::gl::glGetError(), miniquad::gl::GL_NO_ERROR);
        assert_eq!(deleted, 1);
    }
    draw_rectangle(0.0, 0.0, 1.0, 1.0, WHITE);
    unsafe { get_internal_gl().flush() };

    // Materials without pending draws can reuse storage within the same frame.
    for _ in 0..100 {
        drop(material("vec4(1.0)"));
    }
    next_frame().await;
}
