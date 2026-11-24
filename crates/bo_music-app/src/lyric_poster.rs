use crate::month_poster::{rasterize_png_with_font, rect, text};
use bo_music_storage::settings::{LyricPosterStyle, PosterTheme};
use std::path::Path;

pub const MAX_LINES: usize = 8;

pub struct Lyrics {
    pub title: String,
    pub artist: String,
    pub lines: Vec<String>,
    pub style: LyricPosterStyle,
    pub cover_rgb: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
#[error("could not render the lyric poster")]
pub struct RenderError;

pub fn render(lyrics: &Lyrics) -> Result<Vec<u8>, RenderError> {
    let lines: Vec<_> = lyrics
        .lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .take(MAX_LINES)
        .collect();
    if lines.is_empty() {
        return Err(RenderError);
    }
    let background = lyrics
        .style
        .background_rgb
        .unwrap_or_else(|| match lyrics.style.theme {
            PosterTheme::Cover => lyrics.cover_rgb.unwrap_or(0x24324a),
            PosterTheme::Dark => 0x171d33,
            PosterTheme::Light => 0xf7f9ff,
            PosterTheme::Warm => 0xf4f1ea,
        });
    let foreground = lyrics
        .style
        .text_rgb
        .unwrap_or_else(|| contrasting_text(background));
    let background = format!("#{:06x}", background & 0xffffff);
    let foreground = format!("#{:06x}", foreground & 0xffffff);
    let width = 760;
    let height = 230 + lines.len() as u32 * 56;
    let mut svg =
        format!("<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='{height}'>");
    rect(&mut svg, (0, 0, width, height), 0, &background);
    rect(&mut svg, (52, 45, 44, 5), 2, &foreground);
    text(&mut svg, (52, 102), 42, &foreground, &lyrics.title, 36);
    text(&mut svg, (52, 143), 28, &foreground, &lyrics.artist, 46);
    for (index, line) in lines.into_iter().enumerate() {
        text(
            &mut svg,
            (52, 214 + index as u32 * 56),
            34,
            &foreground,
            line,
            38,
        );
    }
    text(&mut svg, (52, height - 33), 22, &foreground, "bo-music", 12);
    svg.push_str("</svg>");
    rasterize_png_with_font(&svg, width, height, &lyrics.style.font_family).map_err(|_| RenderError)
}

fn contrasting_text(rgb: u32) -> u32 {
    let linear = |shift: u32| {
        let channel = f64::from((rgb >> shift) & 0xff) / 255.;
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(16) + 0.7152 * linear(8) + 0.0722 * linear(0);
    if luminance > 0.179 {
        0x000000
    } else {
        0xffffff
    }
}

pub fn save(path: &Path, png: &[u8]) -> std::io::Result<()> {
    bo_music_storage::atomic::replace(path, png)
}

#[cfg(test)]
#[path = "lyric_poster_tests.rs"]
mod tests;
