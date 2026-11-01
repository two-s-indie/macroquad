//! Paired with miniquad/examples/dpi_diagnostic.rs; see examples/dpi_diagnostic.md.
use macroquad::prelude::*;
use macroquad::window::miniquad;

async fn diagnostic() {
    let mut previous = None;
    let mut frames = 0u64;
    let mut fullscreen = false;
    let mut grabbed = false;
    println!("source={}", env!("CARGO_MANIFEST_DIR"));

    loop {
        frames += 1;
        let scale = screen_dpi_scale();
        let framebuffer = miniquad::window::screen_size();
        let logical = (screen_width(), screen_height());
        let mouse = mouse_position();
        let snapshot = (scale, framebuffer, logical, mouse);
        if previous != Some(snapshot) || is_key_pressed(KeyCode::P) {
            println!(
                "t={:.3} frame={} dpi={:.4} framebuffer={:?} logical={:?} mouse_logical={:?}",
                get_time(),
                frames,
                scale,
                framebuffer,
                logical,
                mouse
            );
            previous = Some(snapshot);
        }

        if is_key_pressed(KeyCode::Escape) {
            break;
        }
        if is_key_pressed(KeyCode::R) {
            println!("request logical=800x600");
            request_new_screen_size(800.0, 600.0);
        }
        if is_key_pressed(KeyCode::O) {
            println!("request logical=801x601");
            request_new_screen_size(801.0, 601.0);
        }
        if is_key_pressed(KeyCode::F) {
            fullscreen = !fullscreen;
            set_fullscreen(fullscreen);
        }
        if is_key_pressed(KeyCode::C) {
            grabbed = !grabbed;
            set_cursor_grab(grabbed);
        }

        clear_background(Color::from_rgba(15, 20, 26, 255));
        let rect = Rect::new(
            (logical.0 - 200.0) / 2.0,
            (logical.1 - 120.0) / 2.0,
            200.0,
            120.0,
        );
        let hovered = rect.contains(vec2(mouse.0, mouse.1));
        draw_rectangle(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if hovered { GOLD } else { BLUE },
        );
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, WHITE);
        draw_line(mouse.0 - 10.0, mouse.1, mouse.0 + 10.0, mouse.1, 2.0, WHITE);
        draw_line(mouse.0, mouse.1 - 10.0, mouse.0, mouse.1 + 10.0, 2.0, WHITE);

        let lines = [
            format!("DPI scale: {scale:.4}    frame: {frames}"),
            format!("Framebuffer: {:.0} x {:.0}", framebuffer.0, framebuffer.1),
            format!("Logical size: {:.3} x {:.3}", logical.0, logical.1),
            format!(
                "Mouse logical: {:.2}, {:.2}   inside rectangle: {hovered}",
                mouse.0, mouse.1
            ),
            format!(
                "High DPI: {}    blocking: {}",
                miniquad::window::high_dpi(),
                miniquad::window::blocking_event_loop()
            ),
            "R: 800x600   O: 801x601   F: fullscreen   C: grab   P: log   Esc: quit".into(),
        ];
        for (index, line) in lines.iter().enumerate() {
            draw_text(line, 12.0, 28.0 + index as f32 * 26.0, 22.0, WHITE);
        }
        // No polling timer, DPI correction, or unconditional schedule_update:
        // a scale-only transition must wake the backend itself.
        next_frame().await;
    }
    set_cursor_grab(false);
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args
        .iter()
        .any(|arg| !["--low-dpi", "--blocking", "--odd-size"].contains(&arg.as_str()))
    {
        eprintln!("Usage: dpi_diagnostic [--low-dpi] [--blocking] [--odd-size]");
        return;
    }
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    let odd = has("--odd-size");
    let conf = macroquad::conf::Conf {
        miniquad_conf: miniquad::conf::Conf {
            window_title: "Macroquad DPI diagnostic".into(),
            window_width: if odd { 801 } else { 800 },
            window_height: if odd { 601 } else { 600 },
            high_dpi: !has("--low-dpi"),
            platform: miniquad::conf::Platform {
                blocking_event_loop: has("--blocking"),
                ..Default::default()
            },
            ..Default::default()
        },
        update_on: Some(macroquad::conf::UpdateTrigger {
            key_down: true,
            mouse_down: true,
            mouse_up: true,
            mouse_motion: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    println!(
        "requested={}x{} high_dpi={} blocking={}",
        conf.miniquad_conf.window_width,
        conf.miniquad_conf.window_height,
        conf.miniquad_conf.high_dpi,
        conf.miniquad_conf.platform.blocking_event_loop
    );
    macroquad::Window::from_config(conf, diagnostic());
}
