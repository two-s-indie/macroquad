#![cfg(target_os = "windows")]

//! Run with the sibling Miniquad patch documented in examples/dpi_diagnostic.md.
//! Real HWND/GPU integration; injected DPI messages are distinct from monitor moves.
use macroquad::{prelude::*, window::miniquad};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use winapi::{
    shared::{
        minwindef::BOOL,
        windef::{
            DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, HDC, HMONITOR, HWND,
            POINT, RECT,
        },
    },
    um::{
        imm::*,
        libloaderapi::{GetModuleHandleA, GetProcAddress},
        winuser::*,
    },
};

// winapi 0.3 exposes only a subset of IMM declarations.
#[repr(C)]
#[allow(non_snake_case)]
struct CANDIDATEFORM {
    dwIndex: u32,
    dwStyle: u32,
    ptCurrentPos: POINT,
    rcArea: RECT,
}
#[link(name = "imm32")]
extern "system" {
    fn ImmGetCompositionWindow(context: HIMC, form: *mut COMPOSITIONFORM) -> BOOL;
    fn ImmGetCandidateWindow(context: HIMC, index: u32, form: *mut CANDIDATEFORM) -> BOOL;
}

unsafe fn symbol<T: Copy>(name: &[u8]) -> T {
    let module = GetModuleHandleA(b"user32.dll\0".as_ptr().cast());
    let address = GetProcAddress(module, name.as_ptr().cast());
    assert!(
        !address.is_null(),
        "Windows 10 1703+ is required for this integration test"
    );
    std::mem::transmute_copy(&address)
}

struct CursorGuard {
    position: POINT,
    clip: RECT,
}
impl CursorGuard {
    unsafe fn save() -> Self {
        let mut state: Self = std::mem::zeroed();
        assert_ne!(GetCursorPos(&mut state.position), 0);
        assert_ne!(GetClipCursor(&mut state.clip), 0);
        state
    }
}
impl Drop for CursorGuard {
    fn drop(&mut self) {
        unsafe {
            ClipCursor(std::ptr::null());
            SetCursorPos(self.position.x, self.position.y);
            ClipCursor(&self.clip);
        }
    }
}

unsafe extern "system" fn monitor_rect(_: HMONITOR, _: HDC, rect: *mut RECT, data: isize) -> BOOL {
    (*(data as *mut Vec<RECT>)).push(*rect);
    1
}

unsafe fn outer(hwnd: HWND) -> RECT {
    let mut rect = std::mem::zeroed();
    assert_ne!(GetWindowRect(hwnd, &mut rect), 0);
    rect
}

unsafe fn client_on_screen(hwnd: HWND) -> RECT {
    let mut rect: RECT = std::mem::zeroed();
    assert_ne!(GetClientRect(hwnd, &mut rect), 0);
    let mut top = POINT {
        x: rect.left,
        y: rect.top,
    };
    let mut bottom = POINT {
        x: rect.right,
        y: rect.bottom,
    };
    assert_ne!(ClientToScreen(hwnd, &mut top), 0);
    assert_ne!(ClientToScreen(hwnd, &mut bottom), 0);
    RECT {
        left: top.x,
        top: top.y,
        right: bottom.x,
        bottom: bottom.y,
    }
}

fn rect_tuple(r: RECT) -> (i32, i32, i32, i32) {
    (r.left, r.top, r.right, r.bottom)
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.01, "{a} != {b}");
}
fn physical(logical: f32) -> i32 {
    (logical * screen_dpi_scale()).round() as i32
}

unsafe fn inject_dpi(hwnd: HWND, dpi: u32) {
    let rect = outer(hwnd);
    SendMessageW(
        hwnd,
        WM_DPICHANGED,
        (dpi | dpi << 16) as _,
        &rect as *const _ as _,
    );
}

fn hit_rect() -> Rect {
    Rect::new(
        (screen_width() - 200.0) / 2.0,
        (screen_height() - 120.0) / 2.0,
        200.0,
        120.0,
    )
}

fn scene(hwnd: HWND, name: &str, artifacts: Option<&PathBuf>) {
    let dpi = screen_dpi_scale();
    let framebuffer = miniquad::window::screen_size();
    let mut client: RECT = unsafe { std::mem::zeroed() };
    assert_ne!(unsafe { GetClientRect(hwnd, &mut client) }, 0);
    assert_eq!(framebuffer, (client.right as f32, client.bottom as f32));
    close(screen_width() * dpi, framebuffer.0);
    close(screen_height() * dpi, framebuffer.1);
    clear_background(BLACK);
    let rect = hit_rect();
    let mouse = mouse_position();
    let hovered = rect.contains(vec2(mouse.0, mouse.1));
    draw_rectangle(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        if hovered { GREEN } else { BLUE },
    );
    let measured = measure_text("MMMM", None, 32, 1.0);
    let drawn = draw_text("MMMM", 30.0, 60.0, 32.0, WHITE);
    close(measured.width, drawn.width);
    close(measured.height, drawn.height);
    assert!(measured.width > 30.0 && measured.width < 150.0);
    assert!(measured.height > 10.0 && measured.height < 40.0);
    let image = get_screen_data(); // flushes actual GPU drawing, not just font metrics
    assert_eq!((image.width as f32, image.height as f32), framebuffer);
    let mut ink = 0;
    for y in 0..physical(70.0) as usize {
        for x in 0..physical(200.0) as usize {
            let pixel =
                image.get_image_data()[(image.height as usize - 1 - y) * image.width as usize + x];
            if pixel[0] > 100 && pixel[1] > 100 && pixel[2] > 100 {
                ink += 1;
            }
        }
    }
    assert!(ink > 100, "text did not render at DPI {dpi}: ink={ink}");
    let center = image.get_pixel(image.width as u32 / 2, image.height as u32 / 2);
    let expected = if hovered { GREEN } else { BLUE };
    close(center.r, expected.r);
    close(center.g, expected.g);
    close(center.b, expected.b);
    if let Some(directory) = artifacts {
        std::fs::create_dir_all(directory).unwrap();
        image.export_png(directory.join(format!("{name}.png")).to_str().unwrap());
    }
    println!("{name}: dpi={dpi} framebuffer={framebuffer:?} logical=({}, {}) mouse={mouse:?} text={measured:?} ink={ink}", screen_width(), screen_height());
}

async fn pump_request() {
    // This wakes explicit test commands, not DPI transitions. DPI awaits below
    // deliberately use no polling timer, key event or unconditional schedule.
    miniquad::window::schedule_update();
    next_frame().await;
}

unsafe fn check_ime(hwnd: HWND, expected: (i32, i32)) -> bool {
    let context = ImmGetContext(hwnd);
    if context.is_null() {
        return false;
    }
    let mut composition: COMPOSITIONFORM = std::mem::zeroed();
    let mut candidate: CANDIDATEFORM = std::mem::zeroed();
    let available = ImmGetCompositionWindow(context, &mut composition) != 0
        && ImmGetCandidateWindow(context, 0, &mut candidate) != 0;
    ImmReleaseContext(hwnd, context);
    if available {
        assert_eq!(
            (composition.ptCurrentPos.x, composition.ptCurrentPos.y),
            expected,
            "composition position changed units"
        );
        assert_eq!(
            (candidate.ptCurrentPos.x, candidate.ptCurrentPos.y),
            expected,
            "candidate position changed units"
        );
    }
    available
}

async fn exercise(title: Vec<u16>, low: bool, artifacts: Option<PathBuf>) {
    let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    assert!(!hwnd.is_null());
    let _cursor = unsafe { CursorGuard::save() };
    let native_dpi: unsafe extern "system" fn(HWND) -> u32 =
        unsafe { symbol(b"GetDpiForWindow\0") };
    let initial = unsafe { native_dpi(hwnd) };
    assert_eq!(
        screen_dpi_scale(),
        if low { 1.0 } else { initial as f32 / 96.0 }
    );
    let mut monitors: Vec<RECT> = vec![];
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(monitor_rect),
            &mut monitors as *mut _ as _,
        );
    }
    println!("physical monitors={} startup_dpi={initial}", monitors.len());
    for index in (0..monitors.len()).chain((0..monitors.len()).rev()) {
        let monitor = &monitors[index];
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                monitor.left + 40,
                monitor.top + 40,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        pump_request().await;
        let actual = unsafe { native_dpi(hwnd) };
        assert_eq!(
            screen_dpi_scale(),
            if low { 1.0 } else { actual as f32 / 96.0 }
        );
        assert_eq!(
            miniquad::window::screen_size(),
            (physical(801.0) as f32, physical(601.0) as f32)
        );
        println!(
            "real monitor {index}: bounds={:?} native_dpi={actual}",
            rect_tuple(*monitor)
        );
        scene(hwnd, &format!("real-monitor-{index}"), artifacts.as_ref());
    }

    let mut ime_verified = 0;
    for dpi in [96, 120, 144, 168, 192, 168, 96] {
        let mut before: POINT = unsafe { std::mem::zeroed() };
        unsafe {
            GetCursorPos(&mut before);
            inject_dpi(hwnd, dpi);
        }
        next_frame().await; // backend alone must wake blocking mode
        assert_eq!(
            screen_dpi_scale(),
            if low { 1.0 } else { dpi as f32 / 96.0 }
        );
        let mut after: POINT = unsafe { std::mem::zeroed() };
        unsafe {
            GetCursorPos(&mut after);
        }
        if (before.x, before.y) == (after.x, after.y) {
            unsafe {
                ScreenToClient(hwnd, &mut after);
            }
            close(mouse_position().0 * screen_dpi_scale(), after.x as f32);
            close(mouse_position().1 * screen_dpi_scale(), after.y as f32);
        }
        // Request both sizes so even low-DPI mode changes its framebuffer and
        // the await relies on backend resize delivery, not a test wake-up.
        for size in [(800.0, 600.0), (801.0, 601.0)] {
            request_new_screen_size(size.0, size.1);
            next_frame().await;
            assert_eq!(
                miniquad::window::screen_size(),
                (physical(size.0) as f32, physical(size.1) as f32),
                "runtime request truncated or scaled twice"
            );
        }
        let point = (
            physical(screen_width() / 2.0),
            physical(screen_height() / 2.0),
        );
        let packed = ((point.0 as u32 & 0xffff) | ((point.1 as u32 & 0xffff) << 16)) as isize;
        // A click must use its own client coordinates, even if no mouse-motion
        // message was delivered after a DPI/layout change.
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, MK_LBUTTON as _, packed);
        }
        pump_request().await;
        assert!(is_mouse_button_pressed(MouseButton::Left));
        close(mouse_position().0 * screen_dpi_scale(), point.0 as f32);
        close(mouse_position().1 * screen_dpi_scale(), point.1 as f32);
        assert!(hit_rect().contains(vec2(mouse_position().0, mouse_position().1)));
        scene(hwnd, &format!("dpi-{dpi}-hit"), artifacts.as_ref());
        let released = (-13i32, -7i32);
        let release_packed =
            ((released.0 as u32 & 0xffff) | ((released.1 as u32 & 0xffff) << 16)) as isize;
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONUP, 0, release_packed);
        }
        pump_request().await;
        assert!(is_mouse_button_released(MouseButton::Left));
        close(mouse_position().0 * screen_dpi_scale(), released.0 as f32);
        close(mouse_position().1 * screen_dpi_scale(), released.1 as f32);

        let ime = (physical(80.0), physical(140.0));
        miniquad::window::set_ime_position(ime.0, ime.1);
        pump_request().await;
        assert!(
            unsafe { check_ime(hwnd, ime) },
            "IMM forms unavailable at DPI {dpi}"
        );
        unsafe {
            SendMessageW(hwnd, WM_IME_STARTCOMPOSITION, 0, 0);
        }
        pump_request().await;
        assert!(unsafe { check_ime(hwnd, ime) });
        ime_verified += 1;
    }

    set_cursor_grab(true);
    pump_request().await;
    for dpi in [120, 192, 168] {
        unsafe {
            inject_dpi(hwnd, dpi);
        }
        next_frame().await;
        let mut clipped: RECT = unsafe { std::mem::zeroed() };
        assert_ne!(unsafe { GetClipCursor(&mut clipped) }, 0);
        assert_eq!(
            rect_tuple(clipped),
            rect_tuple(unsafe { client_on_screen(hwnd) }),
            "clip rectangle is not the physical client area"
        );
    }
    set_cursor_grab(false);
    pump_request().await;
    let mut pointer: POINT = unsafe { std::mem::zeroed() };
    unsafe {
        GetCursorPos(&mut pointer);
        ScreenToClient(hwnd, &mut pointer);
    }
    close(mouse_position().0 * screen_dpi_scale(), pointer.x as f32);
    close(mouse_position().1 * screen_dpi_scale(), pointer.y as f32);
    println!("IME coordinate queries verified={ime_verified}/7; candidate-popup appearance still requires an interactive IME test");
}

#[test]
#[ignore = "requires the sibling Miniquad DPI fork, an interactive Windows desktop, and IMM support"]
fn dpi_integration() {
    unsafe {
        let set_context: unsafe extern "system" fn(DPI_AWARENESS_CONTEXT) -> BOOL =
            symbol(b"SetProcessDpiAwarenessContext\0");
        assert_ne!(set_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2), 0);
    }
    let _cursor = unsafe { CursorGuard::save() };
    let mode = match std::env::var("MACROQUAD_DPI_CASE") {
        Ok(mode) => mode,
        Err(_) => {
            for case in ["high", "high-blocking", "low", "low-blocking"] {
                let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "dpi_integration",
                        "--ignored",
                        "--nocapture",
                        "--test-threads=1",
                    ])
                    .env("MACROQUAD_DPI_CASE", case)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(45);
                while child.try_wait().unwrap().is_none() {
                    if Instant::now() >= deadline {
                        child.kill().unwrap();
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                let output = child.wait_with_output().unwrap();
                assert!(
                    output.status.success(),
                    "{case}:\n{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                print!("{case}: {}", String::from_utf8_lossy(&output.stdout));
            }
            return;
        }
    };
    let low = mode.starts_with("low");
    let title = format!("Macroquad DPI integration {}", std::process::id());
    let encoded: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let artifacts =
        std::env::var_os("MACROQUAD_DPI_ARTIFACTS").map(|p| PathBuf::from(p).join(&mode));
    let conf = macroquad::conf::Conf {
        miniquad_conf: miniquad::conf::Conf {
            window_title: title,
            window_width: 801,
            window_height: 601,
            high_dpi: !low,
            platform: miniquad::conf::Platform {
                blocking_event_loop: mode.contains("blocking"),
                ..Default::default()
            },
            ..Default::default()
        },
        update_on: Some(macroquad::conf::UpdateTrigger::default()),
        ..Default::default()
    };
    macroquad::Window::from_config(conf, exercise(encoded, low, artifacts));
}
