//! Bounded image loading and square crop geometry for playlist covers.
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

const MAX_FILE: u64 = 12 * 1024 * 1024;
const MAX_DIMENSION: u32 = 8192;
const MAX_DECODED: u64 = 128 * 1024 * 1024;
const COVER_SIZE: u32 = 512;
const PREVIEW_SIZE: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoverCrop {
    pub horizontal: f32,
    pub vertical: f32,
    pub zoom: f32,
}
impl Default for CoverCrop {
    fn default() -> Self {
        Self {
            horizontal: 0.5,
            vertical: 0.5,
            zoom: 1.0,
        }
    }
}
impl CoverCrop {
    /// Return the square crop in the original image's pixels.
    pub fn source_rect(self, width: u32, height: u32) -> (u32, u32, u32) {
        if width == 0 || height == 0 {
            return (0, 0, 0);
        }
        let finite = |v: f32, default: f32| if v.is_finite() { v } else { default };
        let zoom = finite(self.zoom, 1.0).clamp(1.0, 4.0);
        let x = finite(self.horizontal, 0.5).clamp(0.0, 1.0);
        let y = finite(self.vertical, 0.5).clamp(0.0, 1.0);
        let side = ((width.min(height) as f32 / zoom).round() as u32).clamp(1, width.min(height));
        let left = ((width - side) as f32 * x).round() as u32;
        let top = ((height - side) as f32 * y).round() as u32;
        (left, top, side)
    }

    /// Drag the image by viewport pixels; a zero-sized viewport is ignored.
    pub fn pan(&mut self, width: u32, height: u32, dx: f32, dy: f32, viewport: f32) {
        if !viewport.is_finite() || viewport <= 0.0 || !dx.is_finite() || !dy.is_finite() {
            return;
        }
        let (_, _, side) = self.source_rect(width, height);
        if side == 0 {
            return;
        }
        let x_range = width - side;
        let y_range = height - side;
        if x_range > 0 {
            self.horizontal =
                (self.horizontal - dx / viewport * side as f32 / x_range as f32).clamp(0.0, 1.0);
        }
        if y_range > 0 {
            self.vertical =
                (self.vertical - dy / viewport * side as f32 / y_range as f32).clamp(0.0, 1.0);
        }
    }

    pub fn change_zoom(&mut self, delta: f32) {
        if delta.is_finite() {
            self.zoom = (self.zoom + delta).clamp(1.0, 4.0);
        }
    }
}

pub struct CoverImage {
    pub width: u32,
    pub height: u32,
    pub preview_png: Vec<u8>,
    original: Arc<[u8]>,
}

#[derive(Clone)]
pub struct CoverEdit {
    pub image: Arc<CoverImage>,
    pub crop: CoverCrop,
}

fn decode(bytes: &[u8]) -> Result<DynamicImage, ()> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ())?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Bmp | ImageFormat::WebP)
    ) {
        return Err(());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| ())?;
    if decoder.total_bytes() > MAX_DECODED {
        return Err(());
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(|_| ())?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn png(image: &DynamicImage) -> Result<Vec<u8>, ()> {
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|_| ())?;
    Ok(output.into_inner())
}

pub(crate) fn load(path: &Path) -> Result<CoverImage, ()> {
    let file = File::open(path).map_err(|_| ())?;
    let metadata = file.metadata().map_err(|_| ())?;
    if !metadata.is_file() || metadata.len() > MAX_FILE {
        return Err(());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_FILE {
        return Err(());
    }
    let image = decode(&bytes)?;
    if image.width() == 0 || image.height() == 0 {
        return Err(());
    }
    let preview_png = png(&image.thumbnail(PREVIEW_SIZE, PREVIEW_SIZE))?;
    Ok(CoverImage {
        width: image.width(),
        height: image.height(),
        preview_png,
        original: bytes.into(),
    })
}

pub(crate) fn render(edit: &CoverEdit) -> Result<Vec<u8>, ()> {
    let image = decode(&edit.image.original)?;
    let (left, top, side) = edit.crop.source_rect(image.width(), image.height());
    if side == 0 {
        return Err(());
    }
    let cropped = image.crop_imm(left, top, side, side).resize_exact(
        COVER_SIZE,
        COVER_SIZE,
        image::imageops::FilterType::Lanczos3,
    );
    png(&cropped)
}

#[cfg(test)]
#[path = "playlist_cover_tests.rs"]
mod tests;
