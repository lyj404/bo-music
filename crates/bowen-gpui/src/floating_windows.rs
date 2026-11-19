use super::MusicApp;
use bowen_app::desktop_service::{Command, FloatingBounds, FloatingWindows};
use gpui::{App, Bounds, Context, DisplayId, Pixels, Size, Window, WindowBounds, point, px, size};
use std::{cell::Cell, rc::Rc};

pub(super) type FloatingPreferences = Rc<Cell<Option<FloatingWindows>>>;

pub(super) fn observe_main_placement(window: &mut Window, cx: &Context<MusicApp>) {
    let last = Cell::new(None);
    cx.observe_window_bounds(window, move |view, window, _| {
        let WindowBounds::Windowed(bounds) = window.window_bounds() else {
            return;
        };
        let bounds = FloatingBounds {
            x: bounds.origin.x.into(),
            y: bounds.origin.y.into(),
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
        };
        if bounds.width > 0. && bounds.height > 0. && last.get() != Some(bounds) {
            last.set(Some(bounds));
            let _ = view.service.commands.send(Command::SaveMainWindow(bounds));
        }
    })
    .detach();
}

pub(super) fn observe_placement<View: 'static>(
    source: &MusicApp,
    window: &mut Window,
    cx: &Context<View>,
    minimum_width: Pixels,
    slot: impl Fn(&mut FloatingWindows) -> &mut Option<FloatingBounds> + 'static,
) {
    let preferences = source.floating_windows.clone();
    let commands = source.service.commands.clone();
    cx.observe_window_bounds(window, move |_, window, _| {
        let WindowBounds::Windowed(bounds) = window.window_bounds() else {
            return;
        };
        let Some(mut settings) = preferences.get() else {
            return;
        };
        let new = Some(FloatingBounds {
            x: bounds.origin.x.into(),
            y: bounds.origin.y.into(),
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
        });
        if new != *slot(&mut settings)
            && bounds.size.width >= minimum_width
            && bounds.size.height > px(0.)
        {
            *slot(&mut settings) = new;
            preferences.set(Some(settings));
            let _ = commands.send(Command::SaveFloatingWindows(settings));
        }
    })
    .detach();
}

pub(super) fn restore_placement(
    saved: Option<FloatingBounds>,
    fallback: Bounds<Pixels>,
    display: Option<DisplayId>,
    minimum: Size<Pixels>,
    cx: &App,
) -> (Bounds<Pixels>, Option<DisplayId>) {
    if let Some(saved) = saved {
        let bounds = Bounds::new(
            point(px(saved.x), px(saved.y)),
            size(px(saved.width), px(saved.height)),
        );
        let displays = cx.displays();
        if let Some(monitor) = displays
            .iter()
            .find(|screen| screen.bounds().contains(&bounds.center()))
            .or_else(|| {
                displays
                    .iter()
                    .find(|screen| screen.bounds().intersects(&bounds))
            })
        {
            return (
                constrain(bounds, monitor.bounds(), minimum),
                Some(monitor.id()),
            );
        }
    }
    match display
        .and_then(|id| cx.find_display(id))
        .or_else(|| cx.primary_display())
    {
        Some(monitor) => (
            constrain(fallback, monitor.bounds(), minimum),
            Some(monitor.id()),
        ),
        None => (fallback, None),
    }
}

#[cfg(windows)]
pub(super) fn restore_legacy_placement(
    saved: Option<bowen_app::desktop_service::PhysicalWindowBounds>,
    minimum: Size<Pixels>,
    cx: &App,
) -> Option<(Bounds<Pixels>, Option<DisplayId>)> {
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromRect},
        UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
    };
    let saved = saved?;
    let rect = RECT {
        left: saved.x,
        top: saved.y,
        right: saved.x.checked_add(saved.width)?,
        bottom: saved.y.checked_add(saved.height)?,
    };
    let mut monitor_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let (mut dpi, mut dpi_y) = (0, 0);
    // Win32 reads the legacy rectangle and writes monitor info and DPI to local storage.
    unsafe {
        let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONULL);
        if monitor.is_null()
            || GetMonitorInfoW(monitor, &mut monitor_info) == 0
            || GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y) < 0
            || dpi == 0
        {
            return None;
        }
    }
    let scale = dpi as f32 / 96.;
    let logical = |area: RECT| {
        Bounds::new(
            point(px(area.left as f32 / scale), px(area.top as f32 / scale)),
            size(
                px((area.right - area.left) as f32 / scale),
                px((area.bottom - area.top) as f32 / scale),
            ),
        )
    };
    let screen = logical(monitor_info.rcMonitor);
    let display = cx.displays().into_iter().find(|display| {
        let bounds = display.bounds();
        (bounds.origin.x - screen.origin.x).abs() < px(1.)
            && (bounds.origin.y - screen.origin.y).abs() < px(1.)
            && (bounds.size.width - screen.size.width).abs() < px(1.)
            && (bounds.size.height - screen.size.height).abs() < px(1.)
    })?;
    Some((
        constrain(logical(rect), logical(monitor_info.rcWork), minimum),
        Some(display.id()),
    ))
}

fn constrain(
    bounds: Bounds<Pixels>,
    screen: Bounds<Pixels>,
    minimum: Size<Pixels>,
) -> Bounds<Pixels> {
    let width = bounds.size.width.max(minimum.width).min(screen.size.width);
    let height = bounds
        .size
        .height
        .max(minimum.height)
        .min(screen.size.height);
    Bounds::new(
        point(
            bounds
                .origin
                .x
                .max(screen.origin.x)
                .min(screen.origin.x + screen.size.width - width),
            bounds
                .origin
                .y
                .max(screen.origin.y)
                .min(screen.origin.y + screen.size.height - height),
        ),
        size(width, height),
    )
}

#[cfg(test)]
#[path = "floating_windows_tests.rs"]
mod tests;
