use macroquad::prelude::*;

fn material() -> Material {
    load_material(
        ShaderSource::Glsl {
            vertex: r#"#version 100
                attribute vec3 position;
                uniform mat4 Model;
                uniform mat4 Projection;
                void main() {
                    gl_Position = Projection * Model * vec4(position, 1.0);
                }"#,
            fragment: r#"#version 100
                precision mediump float;
                uniform sampler2D Source;
                void main() {
                    gl_FragColor = texture2D(Source, vec2(0.5));
                }"#,
        },
        MaterialParams {
            textures: vec!["Source".to_owned()],
            ..Default::default()
        },
    )
    .unwrap()
}

#[macroquad::test]
async fn material_texture_lifetimes() {
    material_retains_its_texture_until_replaced_or_dropped().await;
    queued_draws_keep_their_original_material_textures();
}

async fn material_retains_its_texture_until_replaced_or_dropped() {
    let material = material();
    let texture = Texture2D::from_rgba8(1, 1, &[255, 0, 0, 255]);
    let weak = texture.weak_clone();
    let original_id = texture.raw_miniquad_id();
    material.set_texture("Source", texture);
    next_frame().await;
    assert_eq!(weak.raw_miniquad_id(), original_id);

    let replacement = Texture2D::from_rgba8(1, 1, &[0, 0, 255, 255]);
    let replacement_weak = replacement.weak_clone();
    let replacement_id = replacement.raw_miniquad_id();
    material.set_texture("Source", replacement);
    next_frame().await;
    assert_ne!(weak.raw_miniquad_id(), original_id);
    assert_eq!(replacement_weak.raw_miniquad_id(), replacement_id);

    drop(material);
    next_frame().await;
    assert_ne!(replacement_weak.raw_miniquad_id(), replacement_id);
}

fn queued_draws_keep_their_original_material_textures() {
    let target = render_target(4, 2);
    target.texture.set_filter(FilterMode::Nearest);
    set_camera(&Camera2D {
        render_target: Some(target.clone()),
        ..Camera2D::from_display_rect(Rect::new(0.0, 0.0, 4.0, 2.0))
    });
    clear_background(BLACK);
    let material = material();
    gl_use_material(&material);
    material.set_texture("Source", Texture2D::from_rgba8(1, 1, &[255, 0, 0, 255]));
    draw_rectangle(0.0, 0.0, 2.0, 2.0, WHITE);
    material.set_texture("Source", Texture2D::from_rgba8(1, 1, &[0, 0, 255, 255]));
    draw_rectangle(2.0, 0.0, 2.0, 2.0, WHITE);
    gl_use_default_material();
    set_default_camera();

    let image = target.texture.get_texture_data();
    assert_eq!(image.get_pixel(0, 0), Color::from_rgba(255, 0, 0, 255));
    assert_eq!(image.get_pixel(3, 0), Color::from_rgba(0, 0, 255, 255));
}
