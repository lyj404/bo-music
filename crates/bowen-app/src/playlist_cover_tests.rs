use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn preview_and_output_share_the_original_and_crop_geometry() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("bowen-cover-{}-{unique}", std::process::id()));
    let image = DynamicImage::ImageRgb8(image::ImageBuffer::from_fn(800, 400, |x, _| {
        if x < 400 {
            image::Rgb([255, 0, 0])
        } else {
            image::Rgb([0, 255, 0])
        }
    }));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Bmp).unwrap();
    std::fs::write(&path, bytes.into_inner()).unwrap();
    let loaded = Arc::new(load(&path).unwrap());
    assert_eq!((loaded.width, loaded.height), (800, 400));
    assert!(image::load_from_memory(&loaded.preview_png).is_ok());
    std::fs::write(&path, b"changed after preview").unwrap();
    let crop = CoverCrop {
        horizontal: 1.0,
        vertical: 0.5,
        zoom: 1.0,
    };
    assert_eq!(crop.source_rect(loaded.width, loaded.height), (400, 0, 400));
    let output = render(&CoverEdit {
        image: loaded,
        crop,
    })
    .unwrap();
    let output = image::load_from_memory_with_format(&output, ImageFormat::Png).unwrap();
    assert_eq!((output.width(), output.height()), (512, 512));
    assert_eq!(output.to_rgb8().get_pixel(0, 0).0, [0, 255, 0]);
    assert!(load(&path).is_err());
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(MAX_FILE + 1)
        .unwrap();
    assert!(load(&path).is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn crop_pan_and_zoom_stay_inside_landscape_and_portrait_images() {
    let mut crop = CoverCrop::default();
    assert_eq!(crop.source_rect(800, 400), (200, 0, 400));
    crop.pan(800, 400, 10000.0, 10000.0, 200.0);
    assert_eq!(crop.source_rect(800, 400), (0, 0, 400));
    crop.change_zoom(99.0);
    crop.pan(400, 800, -10000.0, -10000.0, 200.0);
    assert_eq!(crop.source_rect(400, 800), (300, 700, 100));
    assert_eq!(
        CoverCrop {
            horizontal: f32::NAN,
            vertical: f32::INFINITY,
            zoom: f32::NAN
        }
        .source_rect(800, 400),
        (200, 0, 400)
    );
}

#[test]
fn jpeg_exif_orientation_is_applied_before_preview_and_crop() {
    use image::ImageEncoder;
    let mut encoded = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut encoded);
    encoder
        .set_exif_metadata(vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0,
            0,
        ])
        .unwrap();
    encoder
        .write_image(&[0; 4 * 2 * 3], 4, 2, image::ExtendedColorType::Rgb8)
        .unwrap();
    let image = decode(&encoded).unwrap();
    assert_eq!((image.width(), image.height()), (2, 4));
}
