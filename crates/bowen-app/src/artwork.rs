//! Bounded, anonymous artwork downloads and thumbnail decoding.
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use std::{io::Cursor, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

const MAX_DOWNLOAD: usize = 4 * 1024 * 1024;
const MAX_DIMENSION: u32 = 4096;
const MAX_DECODED: u64 = 64 * 1024 * 1024;
const THUMBNAIL_SIZE: u32 = 128;

pub type AmbientPalette = [[u8; 3]; 3];

pub struct ArtworkThumbnail {
    pub png: Vec<u8>,
    pub ambient: Option<AmbientPalette>,
}

#[derive(Clone)]
pub struct ArtworkFetcher {
    client: reqwest::Client,
    slots: Arc<Semaphore>,
}
impl ArtworkFetcher {
    pub fn new() -> Result<Self, reqwest::Error> {
        let redirects = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 3 || !valid_url(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(redirects)
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            client,
            slots: Arc::new(Semaphore::new(6)),
        })
    }

    pub async fn fetch(&self, address: &str) -> Result<Vec<u8>, ()> {
        self.fetch_sized(address, THUMBNAIL_SIZE, encode_png).await
    }

    pub(crate) async fn fetch_thumbnail(&self, address: &str) -> Result<ArtworkThumbnail, ()> {
        self.fetch_sized(address, THUMBNAIL_SIZE, |image| {
            let ambient = ambient_palette(&image);
            Ok(ArtworkThumbnail {
                png: encode_png(image)?,
                ambient,
            })
        })
        .await
    }

    pub(crate) async fn fetch_comment_preview(&self, address: &str) -> Result<Vec<u8>, ()> {
        self.fetch_sized(address, 1024, encode_png).await
    }

    async fn fetch_sized<T: Send + 'static>(
        &self,
        address: &str,
        edge: u32,
        process: impl FnOnce(DynamicImage) -> Result<T, ()> + Send + 'static,
    ) -> Result<T, ()> {
        let url = reqwest::Url::parse(address).map_err(|_| ())?;
        if !valid_url(&url) {
            return Err(());
        }
        let _permit = self.slots.acquire().await.map_err(|_| ())?;
        let mut response = self.client.get(url).send().await.map_err(|_| ())?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|size| size > MAX_DOWNLOAD as u64)
        {
            return Err(());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
            if bytes.len().saturating_add(chunk.len()) > MAX_DOWNLOAD {
                return Err(());
            }
            bytes.extend_from_slice(&chunk);
        }
        tokio::task::spawn_blocking(move || process(thumbnail(&bytes, edge)?))
            .await
            .map_err(|_| ())?
    }
}

fn valid_url(url: &reqwest::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}

fn thumbnail(bytes: &[u8], edge: u32) -> Result<DynamicImage, ()> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ())?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
    ) {
        return Err(());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED);
    reader.limits(limits);
    Ok(reader.decode().map_err(|_| ())?.thumbnail(edge, edge))
}

fn encode_png(image: DynamicImage) -> Result<Vec<u8>, ()> {
    let mut output = Cursor::new(Vec::new());
    image
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|_| ())?;
    Ok(output.into_inner())
}

fn ambient_palette(image: &DynamicImage) -> Option<AmbientPalette> {
    let mut buckets = [[0u32; 4]; 512];
    for pixel in image.thumbnail(48, 48).to_rgba8().pixels() {
        let [r, g, b, alpha] = pixel.0;
        if alpha < 128 {
            continue;
        }
        let index = usize::from(r >> 5) * 64 + usize::from(g >> 5) * 8 + usize::from(b >> 5);
        let [count, red, green, blue] = &mut buckets[index];
        *count += 1;
        *red += u32::from(r);
        *green += u32::from(g);
        *blue += u32::from(b);
    }
    buckets.sort_unstable_by_key(|[count, ..]| std::cmp::Reverse(*count));
    let mut seeds: Vec<[u8; 3]> = Vec::with_capacity(3);
    let minimum = (buckets[0][0] / 12).max(1);
    for [count, red, green, blue] in buckets
        .into_iter()
        .take_while(|bucket| bucket[0] >= minimum)
    {
        let candidate = [red, green, blue].map(|sum| (sum / count) as u8);
        if seeds.iter().any(|seed| {
            seed.iter()
                .zip(candidate)
                .map(|(a, b)| u32::from(a.abs_diff(b)).pow(2))
                .sum::<u32>()
                < 48u32.pow(2)
        }) {
            continue;
        }
        seeds.push(candidate);
        if seeds.len() == 3 {
            break;
        }
    }
    let primary = *seeds.first()?;
    Some(std::array::from_fn(|index| {
        let color = seeds.get(index).copied().unwrap_or(primary);
        let peak = f32::from(*color.iter().max().unwrap_or(&0)).max(1.0);
        let brightness = [54.0, 62.0, 58.0][index];
        color.map(|channel| (24.0 + brightness * f32::from(channel) / peak).round() as u8)
    }))
}

pub fn render_ambient(palette: AmbientPalette) -> Option<Vec<u8>> {
    const EDGE: u32 = 64;
    const BLOBS: [(f32, f32, f32, f32); 3] = [
        (0.12, 0.10, 0.66, 0.86),
        (0.92, 0.32, 0.68, 0.90),
        (0.48, 0.84, 0.72, 0.78),
    ];
    let pixels = image::RgbImage::from_fn(EDGE, EDGE, |x, y| {
        let x = (x as f32 + 0.5) / EDGE as f32;
        let y = (y as f32 + 0.5) / EDGE as f32;
        let mut color = [23.0, 28.0, 50.0];
        for ((cx, cy, rx, ry), seed) in BLOBS.into_iter().zip(palette) {
            let radius = ((x - cx) / rx).hypot((y - cy) / ry);
            let opacity = 0.80 * (1.0 - radius * radius).max(0.0).powi(2);
            for (channel, seed) in color.iter_mut().zip(seed) {
                *channel += opacity * (f32::from(seed) - *channel);
            }
        }
        image::Rgb(color.map(|channel| channel.round() as u8))
    });
    encode_png(DynamicImage::ImageRgb8(pixels)).ok()
}

#[cfg(test)]
#[path = "artwork_tests.rs"]
mod tests;
