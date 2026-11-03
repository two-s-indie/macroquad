#![cfg(one_screenshot)]

use macroquad::{prelude::*, Window};

#[test]
fn screenshot_diagnostic_exports_third_frame() {
    const CHILD: &str = "MACROQUAD_SCREENSHOT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        Window::from_config(
            Conf {
                window_title: "Screenshot diagnostic".into(),
                window_width: 320,
                window_height: 240,
                ..Default::default()
            },
            async {
                for frame in 0..10 {
                    clear_background(match frame {
                        0 => BLUE,
                        1 => GREEN,
                        2 => Color::from_rgba(255, 0, 0, 255),
                        _ => WHITE,
                    });
                    next_frame().await;
                }
            },
        );
        return;
    }

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::current_dir()
        .unwrap()
        .join("target/test-artifacts")
        .join(format!("screenshot-{nonce}"));
    std::fs::create_dir_all(&directory).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "screenshot_diagnostic_exports_third_frame",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .current_dir(&directory)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("screenshot diagnostic did not exit");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        !output.status.success(),
        "diagnostic should stop via its intentional panic"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("screenshot successfully saved"));
    let image = image::open(directory.join("screenshot.png"))
        .unwrap()
        .into_rgba8();
    assert_eq!(image.dimensions(), (320, 240));
    assert_eq!(image.get_pixel(160, 120).0, [255, 0, 0, 255]);
}
