use crate::{
    artwork::ArtworkFetcher,
    month_poster::{image, rasterize_png, rect, text},
};
use bowen_domain::{comments::SongComment, track::Track};
use std::time::Duration;

pub(crate) async fn render(
    comment: SongComment,
    track: Track,
    link: Option<String>,
    fetcher: Option<ArtworkFetcher>,
) -> Result<Vec<u8>, ()> {
    let load = |url: Option<&str>| {
        let url = url.filter(|url| url.len() <= 4096).map(str::to_owned);
        let fetcher = fetcher.clone();
        async move {
            tokio::time::timeout(
                Duration::from_secs(8),
                fetcher?.fetch_comment_preview(&url?),
            )
            .await
            .ok()?
            .ok()
        }
    };
    let (avatar, attachment, cover) = tokio::join!(
        load(comment.avatar_url.as_deref()),
        load(comment.image_url.as_deref()),
        load(track.cover_url.as_deref()),
    );
    tokio::task::spawn_blocking(move || {
        make_poster(
            &comment,
            &track,
            link.as_deref(),
            avatar.as_deref(),
            attachment.as_deref(),
            cover.as_deref(),
        )
    })
    .await
    .unwrap_or(Err(()))
}

fn make_poster(
    comment: &SongComment,
    track: &Track,
    link: Option<&str>,
    avatar: Option<&[u8]>,
    attachment: Option<&[u8]>,
    cover: Option<&[u8]>,
) -> Result<Vec<u8>, ()> {
    let lines = wrap(&comment.content);
    let footer = 280 + lines.len() as u32 * 37 + if attachment.is_some() { 320 } else { 0 };
    let height = footer + 176;
    let mut svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' xmlns:xlink='http://www.w3.org/1999/xlink' width='760' height='{height}'>"
    );
    rect(&mut svg, (0, 0, 760, height), 0, "#151932");
    text(&mut svg, (48, 79), 32, "#aab7ff", "波纹音乐 / 歌曲评论", 42);
    image(&mut svg, (48, 109, 64), avatar, "#262d52");
    text(&mut svg, (130, 136), 28, "#ffffff", &comment.nickname, 36);
    let meta = format!(
        "{}  {}  ·  {} 赞",
        comment.publish_time, comment.location, comment.like_count
    );
    text(&mut svg, (130, 164), 20, "#c2c9e4", &meta, 54);
    if !comment.reply_nickname.is_empty() {
        text(
            &mut svg,
            (48, 204),
            22,
            "#aab7ff",
            &format!("回复 {}", comment.reply_nickname),
            54,
        );
    }
    for (index, line) in lines.iter().enumerate() {
        text(
            &mut svg,
            (48, 246 + index as u32 * 37),
            27,
            "#ffffff",
            line,
            46,
        );
    }
    if let Some(bytes) = attachment {
        let y = footer - 320;
        image(&mut svg, (48, y, 290), Some(bytes), "#262d52");
    }
    rect(&mut svg, (40, footer, 680, 130), 16, "#262d52");
    image(&mut svg, (56, footer + 20, 90), cover, "#151932");
    text(
        &mut svg,
        (164, footer + 55),
        28,
        "#ffffff",
        &track.title,
        28,
    );
    text(
        &mut svg,
        (164, footer + 89),
        23,
        "#c2c9e4",
        &track.artist_text,
        34,
    );
    if let Some(code) = link.and_then(|link| qrcode::QrCode::new(link.as_bytes()).ok()) {
        let scale = 100 / (code.width() as u32 + 8);
        if scale > 0 {
            let extent = (code.width() as u32 + 8) * scale;
            let origin = (700 - extent, footer + (130 - extent) / 2);
            rect(&mut svg, (origin.0, origin.1, extent, extent), 0, "#ffffff");
            for (index, color) in code.to_colors().into_iter().enumerate() {
                if color == qrcode::Color::Dark {
                    let x = origin.0 + (index % code.width()) as u32 * scale + 4 * scale;
                    let y = origin.1 + (index / code.width()) as u32 * scale + 4 * scale;
                    rect(&mut svg, (x, y, scale, scale), 0, "#151932");
                }
            }
        }
    }
    svg.push_str("</svg>");
    rasterize_png(&svg, 760, height)
}

fn wrap(content: &str) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut width = 0;
    for ch in content.chars() {
        let step = if ch.is_ascii() { 1 } else { 2 };
        if ch == '\n' || width + step > 44 {
            if lines.len() == 18 {
                lines.last_mut().expect("line exists").push('…');
                break;
            }
            lines.push(String::new());
            width = 0;
        }
        if !ch.is_control() {
            lines.last_mut().expect("line exists").push(ch);
            width += step;
        }
    }
    lines
}

#[cfg(test)]
#[path = "comment_poster_tests.rs"]
mod tests;
