use macroquad::prelude::{Image, ImageFormat};

#[test]
fn tga_decoding_preserves_rgba_and_top_left_orientation() {
    // Uncompressed 2x2 true-color TGA, 32 bits per pixel, top-left origin,
    // eight alpha bits. TGA stores each pixel in BGRA order.
    let tga = [
        0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2, 0, 32, 0x28, 0, 0, 255, 255, 0, 255, 0, 128,
        255, 0, 0, 64, 255, 255, 255, 0,
    ];

    let image = Image::from_file_with_format(&tga, Some(ImageFormat::Tga)).unwrap();

    assert_eq!((image.width, image.height), (2, 2));
    assert_eq!(
        image.bytes,
        [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 64, 255, 255, 255, 0]
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn png_export_and_reload_preserve_rgba_and_flip_rows() {
    let image = Image {
        width: 2,
        height: 2,
        bytes: vec![
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 64, 255, 255, 255, 0,
        ],
    };
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "macroquad-image-codecs-{}-{unique}.png",
        std::process::id()
    ));

    image.export_png(path.to_str().unwrap());
    let encoded = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();

    // Check both PNG format detection and the explicit-format decoding path.
    for format in [None, Some(ImageFormat::Png)] {
        let decoded = Image::from_file_with_format(&encoded, format).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 2));
        assert_eq!(
            decoded.bytes,
            [0, 0, 255, 64, 255, 255, 255, 0, 255, 0, 0, 255, 0, 255, 0, 128]
        );
    }
}
