use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
};

fn image_bytes() -> Vec<u8> {
    let image = image::DynamicImage::new_rgb8(800, 400);
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).unwrap();
    output.into_inner()
}

#[test]
fn rejects_images_over_the_dimension_limit() {
    let image = image::DynamicImage::new_rgb8(MAX_DIMENSION + 1, 1);
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).unwrap();
    assert!(thumbnail(&output.into_inner(), THUMBNAIL_SIZE).is_err());
}

#[test]
fn comment_preview_preserves_more_detail_than_cover_thumbnails() {
    let image = image::DynamicImage::new_rgb8(1600, 800);
    let mut original = Cursor::new(Vec::new());
    image.write_to(&mut original, ImageFormat::Png).unwrap();
    let output = thumbnail(&original.into_inner(), 1024).unwrap();
    assert_eq!((output.width(), output.height()), (1024, 512));
}

#[test]
fn ambient_uses_visible_cover_colors_and_darkens_them_for_lyrics() {
    let cover = image::RgbaImage::from_fn(48, 48, |x, _| {
        image::Rgba(match x {
            0..16 => [255, 0, 0, 255],
            16..28 => [0, 255, 0, 255],
            _ => [0, 0, 255, 0],
        })
    });
    let palette = ambient_palette(&DynamicImage::ImageRgba8(cover)).unwrap();
    assert!(palette[0][0] > palette[0][1]);
    assert!(palette[1][1] > palette[1][0]);
    assert!(palette.iter().all(|color| color[2] == 24));
    assert_eq!(ambient_palette(&DynamicImage::new_rgba8(48, 48)), None);

    let png = render_ambient(palette).unwrap();
    let background = image::load_from_memory_with_format(&png, ImageFormat::Png)
        .unwrap()
        .into_rgb8();
    assert_eq!(background.dimensions(), (64, 64));
    let left = background.get_pixel(8, 6);
    let right = background.get_pixel(58, 20);
    assert!(left[0] > left[1] && right[1] > right[0]);
    assert!(background.as_raw().iter().all(|&value| value <= 86));
}

#[tokio::test]
async fn downloads_without_credentials_and_returns_a_bounded_thumbnail() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut buffer = [0; 4096];
        let count = socket.read(&mut buffer).unwrap();
        let request = String::from_utf8_lossy(&buffer[..count]).into_owned();
        let bytes = image_bytes();
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )
        .unwrap();
        socket.write_all(&bytes).unwrap();
        request
    });
    let thumbnail = ArtworkFetcher::new()
        .unwrap()
        .fetch_thumbnail(&format!("http://{address}/cover.jpg"))
        .await
        .unwrap();
    let image = image::load_from_memory_with_format(&thumbnail.png, ImageFormat::Png).unwrap();
    assert_eq!((image.width(), image.height()), (128, 64));
    assert!(thumbnail.ambient.is_some());
    let request = server.join().unwrap().to_ascii_lowercase();
    assert!(
        !request.contains("authorization:")
            && !request.contains("cookie:")
            && !request.contains("token:")
    );
}

#[tokio::test]
async fn rejects_invalid_urls_oversized_responses_and_non_images() {
    let fetcher = ArtworkFetcher::new().unwrap();
    assert!(fetcher.fetch("file:///tmp/secret").await.is_err());
    assert!(fetcher.fetch("http://user@127.0.0.1/x").await.is_err());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for response in [
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_DOWNLOAD + 1
            ),
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope".to_owned(),
        ] {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = [0; 1024];
            let _ = socket.read(&mut buffer);
            let _ = socket.write_all(response.as_bytes());
        }
    });
    assert!(
        fetcher
            .fetch(&format!("http://{address}/large"))
            .await
            .is_err()
    );
    assert!(
        fetcher
            .fetch(&format!("http://{address}/invalid"))
            .await
            .is_err()
    );
    server.join().unwrap();
}
