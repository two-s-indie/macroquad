use macroquad::{
    prelude::*,
    telemetry,
    ui::{root_ui, widgets},
};

// One window per process: Miniquad's native display is initialized only once.
#[macroquad::test]
async fn rendering_uses_explicit_backend_and_texture_lookup() {
    let red = Color::from_rgba(255, 0, 0, 255);
    let blue = Color::from_rgba(0, 0, 255, 255);
    let green = Color::from_rgba(0, 255, 0, 255);
    set_default_filter_mode(FilterMode::Nearest);
    gl_set_drawcall_buffer_capacity(128, 192);

    let texture = Texture2D::from_rgba8(2, 2, &[255; 16]);
    texture.update(&Image::gen_image_color(2, 2, red));
    texture.update_from_bytes(2, 2, &[0, 0, 255, 255].repeat(4));
    texture.update_part(&Image::gen_image_color(1, 2, red), 0, 0, 1, 2);
    assert_eq!(texture.size(), vec2(2., 2.));
    assert_eq!((texture.width(), texture.height()), (2., 2.));
    let pixels = texture.get_texture_data();
    assert_eq!(pixels.get_pixel(0, 0), red);
    assert_eq!(pixels.get_pixel(1, 0), blue);
    build_textures_atlas();

    let target = render_target(64, 64);
    set_camera(&Camera2D {
        render_target: Some(target.clone()),
        ..Camera2D::from_display_rect(Rect::new(0., 0., 64., 64.))
    });
    clear_background(BLACK);
    draw_texture_ex(
        &texture,
        0.,
        0.,
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(64., 64.)),
            ..Default::default()
        },
    );
    set_default_camera();
    let pixels = target.texture.get_texture_data();
    assert_eq!(pixels.get_pixel(8, 32), red);
    assert_eq!(pixels.get_pixel(56, 32), blue);

    // Upload newly cached glyphs and flush text through the same backend.
    let font = load_ttf_font_from_bytes(include_bytes!("../src/ProggyClean.ttf")).unwrap();
    set_camera(&Camera2D {
        render_target: Some(target.clone()),
        ..Camera2D::from_display_rect(Rect::new(0., 0., 64., 64.))
    });
    clear_background(BLACK);
    draw_text_ex(
        "Atlas",
        2.,
        30.,
        TextParams {
            font: Some(&font),
            font_size: 24,
            color: WHITE,
            ..Default::default()
        },
    );
    // Its atlas must survive until this queued text is rendered.
    drop(font);
    set_default_camera();
    assert!(target
        .texture
        .get_texture_data()
        .bytes
        .chunks_exact(4)
        .any(|p| p[0] > 0));

    // Cube faces must use the renderer already borrowed by the primitive.
    set_camera(&Camera2D {
        render_target: Some(target.clone()),
        ..Camera2D::from_display_rect(Rect::new(0., 0., 64., 64.))
    });
    clear_background(BLACK);
    draw_cube(vec3(32., 32., 0.), vec3(32., 32., 1.), None, red);
    set_default_camera();
    let pixels = target.texture.get_texture_data();
    assert_eq!(pixels.get_pixel(32, 32), red);
    assert_eq!(pixels.get_pixel(0, 0), BLACK);

    // This frame initializes the UI material and uploads its font atlas.
    root_ui().label(Some(vec2(4., 4.)), "First UI frame");
    telemetry::capture_frame();
    next_frame().await;
    clear_background(BLACK);
    draw_rectangle(0., 0., 8., 8., red);
    root_ui().label(Some(vec2(4., 40.)), "Captured UI glyphs");
    widgets::Texture::new(texture.clone())
        .position(vec2(4., 64.))
        .size(32., 32.)
        .ui(&mut root_ui());
    next_frame().await;

    let captures = telemetry::drawcalls();
    assert!(
        captures.len() >= 3,
        "expected shape, UI text, and UI texture captures"
    );
    assert!(captures.iter().all(|call| call.indices_count > 0));
    for call in captures {
        assert_eq!(
            Texture2D::from_miniquad_texture(call.texture).size(),
            vec2(128., 128.)
        );
    }

    clear_background(green);
    let screen = get_screen_data();
    assert_eq!(screen.get_pixel(0, 0), green);
}
