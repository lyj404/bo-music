use crate::artwork::ArtworkFetcher;
use base64::{Engine, engine::general_purpose::STANDARD};
use bowen_domain::account::MonthReport;
use resvg::{tiny_skia, usvg};
use std::{
    fmt::Write as _,
    sync::{Arc, OnceLock},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonthPosterKind {
    Overview,
    Tracks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonthPosterStyle {
    pub kind: MonthPosterKind,
    pub dark: bool,
}

struct Palette {
    background: &'static str,
    foreground: &'static str,
    muted: &'static str,
    panel: &'static str,
    accent: &'static str,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                background: "#151932",
                foreground: "#ffffff",
                muted: "#c2c9e4",
                panel: "#262d52",
                accent: "#aab7ff",
            }
        } else {
            Self {
                background: "#f1f3ff",
                foreground: "#172046",
                muted: "#516084",
                panel: "#ffffff",
                accent: "#405bc4",
            }
        }
    }
}

pub(crate) async fn render(
    report: MonthReport,
    nickname: Option<String>,
    style: MonthPosterStyle,
    fetcher: Option<ArtworkFetcher>,
) -> Result<Vec<u8>, ()> {
    let urls: Vec<_> = match style.kind {
        MonthPosterKind::Overview => report.album_covers.iter().take(6).cloned().collect(),
        MonthPosterKind::Tracks => report
            .top_tracks
            .first()
            .and_then(|track| track.cover_url.clone())
            .into_iter()
            .collect(),
    };
    let mut covers = vec![None; urls.len()];
    if let Some(fetcher) = fetcher {
        let mut tasks = tokio::task::JoinSet::new();
        for (index, url) in urls
            .into_iter()
            .enumerate()
            .filter(|(_, url)| url.len() <= 4096)
        {
            let fetcher = fetcher.clone();
            tasks.spawn(async move { (index, fetcher.fetch(&url).await.ok()) });
        }
        let _ = tokio::time::timeout(Duration::from_secs(9), async {
            while let Some(result) = tasks.join_next().await {
                if let Ok((index, image)) = result {
                    covers[index] = image;
                }
            }
        })
        .await;
    }
    tokio::task::spawn_blocking(move || {
        rasterize(&svg(&report, nickname.as_deref(), style, &covers))
    })
    .await
    .unwrap_or(Err(()))
}

const WIDTH: u32 = 880;
const HEIGHT: u32 = 1200;
const PANEL_WIDTH: u32 = 752;

fn rasterize(svg: &str) -> Result<Vec<u8>, ()> {
    rasterize_png(svg, WIDTH, HEIGHT)
}

pub(crate) fn rasterize_png(svg: &str, width: u32, height: u32) -> Result<Vec<u8>, ()> {
    rasterize_png_with_font(svg, width, height, "")
}

pub(crate) fn rasterize_png_with_font(
    svg: &str,
    width: u32,
    height: u32,
    preferred: &str,
) -> Result<Vec<u8>, ()> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    });
    let font_family = [
        preferred,
        "Microsoft YaHei",
        "Noto Sans CJK SC",
        "Noto Sans SC",
        "WenQuanYi Zen Hei",
        "PingFang SC",
        "Segoe UI",
        "DejaVu Sans",
    ]
    .into_iter()
    .find(|name| {
        fonts.faces().any(|face| {
            face.families
                .iter()
                .any(|(family, _)| family.eq_ignore_ascii_case(name))
        })
    })
    .unwrap_or("sans-serif")
    .to_owned();
    let options = usvg::Options {
        font_family,
        fontdb: Arc::clone(fonts),
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(svg, &options).map_err(|_| ())?;
    let mut pixels = tiny_skia::Pixmap::new(width, height).ok_or(())?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixels.as_mut(),
    );
    pixels.encode_png().map_err(|_| ())
}

fn svg(
    report: &MonthReport,
    nickname: Option<&str>,
    style: MonthPosterStyle,
    covers: &[Option<Vec<u8>>],
) -> String {
    let palette = Palette::new(style.dark);
    let mut svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' xmlns:xlink='http://www.w3.org/1999/xlink' width='{WIDTH}' height='{HEIGHT}' viewBox='0 0 {WIDTH} {HEIGHT}'>"
    );
    rect(&mut svg, (0, 0, WIDTH, HEIGHT), 0, palette.background);
    rect(&mut svg, (64, 56, 54, 6), 3, palette.accent);
    text(
        &mut svg,
        (64, 105),
        24,
        palette.accent,
        "波纹音乐  /  音波回忆",
        44,
    );
    let heading = match style.kind {
        MonthPosterKind::Overview => "月度听歌总览",
        MonthPosterKind::Tracks => "月度歌曲排行",
    };
    text(&mut svg, (64, 186), 62, palette.foreground, heading, 28);
    let period = format!(
        "{} 年 {:02} 月",
        report.period.year(),
        report.period.month()
    );
    text(&mut svg, (64, 238), 29, palette.accent, &period, 30);
    if let Some(name) = nickname.filter(|name| !name.trim().is_empty()) {
        text(&mut svg, (470, 238), 27, palette.muted, name, 23);
    }
    match style.kind {
        MonthPosterKind::Overview => overview(&mut svg, &palette, report, covers),
        MonthPosterKind::Tracks => tracks(
            &mut svg,
            &palette,
            report,
            covers.first().and_then(Option::as_deref),
        ),
    }
    rect(&mut svg, (64, 1112, PANEL_WIDTH, 2), 0, palette.panel);
    text(&mut svg, (64, 1157), 23, palette.muted, "波纹音乐", 20);
    svg.push_str("</svg>");
    svg
}

fn overview(svg: &mut String, p: &Palette, report: &MonthReport, covers: &[Option<Vec<u8>>]) {
    let values = [
        ("听歌时长 · 分钟", report.listen_total_minutes.clone()),
        (
            "累计播放 · 次",
            report.play_count.map(|value| value.to_string()),
        ),
        (
            "听过歌曲 · 首",
            report.song_count.map(|value| value.to_string()),
        ),
        (
            "听歌时长排名",
            report.time_rank.map(|value| value.to_string()),
        ),
    ];
    for (index, (label, value)) in values.into_iter().enumerate() {
        let x = 64 + (index as u32 % 2) * 384;
        let y = 287 + (index as u32 / 2) * 127;
        rect(svg, (x, y, 368, 108), 20, p.panel);
        text(svg, (x + 21, y + 36), 23, p.muted, label, 28);
        text(
            svg,
            (x + 21, y + 83),
            36,
            p.foreground,
            value.as_deref().unwrap_or("—"),
            19,
        );
    }
    let notes = [
        report
            .artist_name
            .as_ref()
            .map(|name| format!("常听歌手  {name}")),
        report
            .listen_chain
            .as_ref()
            .map(|chain| format!("连续听歌  {chain}")),
        report
            .listen_top_track_title
            .as_ref()
            .map(|track| format!("常听歌曲  {track}")),
    ];
    for (index, note) in notes.into_iter().flatten().enumerate() {
        text(
            svg,
            (64, 581 + index as u32 * 42),
            30,
            p.foreground,
            &note,
            48,
        );
    }
    if !covers.is_empty() {
        text(svg, (64, 737), 30, p.accent, "常听专辑", 18);
        for (index, cover) in covers.iter().enumerate() {
            let x = 64 + index as u32 % 3 * 126;
            let y = 766 + index as u32 / 3 * 126;
            image(svg, (x, y, 112), cover.as_deref(), p.panel);
        }
    }
}

fn tracks(svg: &mut String, p: &Palette, report: &MonthReport, cover: Option<&[u8]>) {
    if let Some(first) = report.top_tracks.first() {
        rect(svg, (64, 275, PANEL_WIDTH, 126), 20, p.panel);
        image(svg, (78, 288, 100), cover, p.background);
        text(svg, (204, 321), 22, p.accent, "本月最常听", 22);
        text(svg, (204, 362), 34, p.foreground, &first.title, 32);
    } else {
        text(svg, (64, 330), 32, p.muted, "本月暂无排行", 30);
    }
    for (index, track) in report.top_tracks.iter().take(10).enumerate() {
        let y = 420 + index as u32 * 64;
        rect(svg, (64, y, PANEL_WIDTH, 54), 10, p.panel);
        text(
            svg,
            (80, y + 34),
            24,
            p.accent,
            &format!("{:02}", index + 1),
            4,
        );
        text(svg, (137, y + 24), 24, p.foreground, &track.title, 41);
        text(svg, (137, y + 46), 19, p.muted, &track.artist, 43);
        if let Some(count) = track.play_count {
            text(svg, (706, y + 34), 22, p.accent, &format!("{count} 次"), 8);
        }
    }
}

pub(crate) fn rect(svg: &mut String, (x, y, w, h): (u32, u32, u32, u32), radius: u32, fill: &str) {
    let _ = write!(
        svg,
        "<rect x='{x}' y='{y}' width='{w}' height='{h}' rx='{radius}' fill='{fill}'/>"
    );
}

pub(crate) fn image(
    svg: &mut String,
    (x, y, edge): (u32, u32, u32),
    bytes: Option<&[u8]>,
    placeholder: &str,
) {
    rect(svg, (x, y, edge, edge), 10, placeholder);
    if let Some(bytes) = bytes {
        let data = STANDARD.encode(bytes);
        let _ = write!(
            svg,
            "<svg x='{x}' y='{y}' width='{edge}' height='{edge}' overflow='hidden'><image width='{edge}' height='{edge}' preserveAspectRatio='xMidYMid slice' xlink:href='data:image/png;base64,{data}'/></svg>"
        );
    }
}

pub(crate) fn text(
    svg: &mut String,
    (x, y): (u32, u32),
    size: u32,
    fill: &str,
    value: &str,
    budget: usize,
) {
    let _ = write!(
        svg,
        "<text x='{x}' y='{y}' font-size='{size}' fill='{fill}'>"
    );
    let mut width = 0;
    for ch in value.chars() {
        if ch.is_control() || matches!(ch, '\u{fffe}' | '\u{ffff}') {
            continue;
        }
        width += if ch.is_ascii() { 1 } else { 2 };
        if width > budget {
            svg.push('…');
            break;
        }
        match ch {
            '&' => svg.push_str("&amp;"),
            '<' => svg.push_str("&lt;"),
            '>' => svg.push_str("&gt;"),
            _ => svg.push(ch),
        }
    }
    svg.push_str("</text>");
}

#[cfg(test)]
#[path = "month_poster_tests.rs"]
mod tests;
