use super::MiniPlayer;
use gpui::{Context, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{mem::size_of, ptr, time::Duration};
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow},
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
        WindowsAndMessaging::{
            GetCursorPos, GetWindowRect, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
        },
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    Left,
    Right,
}

impl Edge {
    fn near(rect: RECT, work: RECT, distance: i32) -> Option<Self> {
        if rect.left.abs_diff(work.left) <= distance as u32 {
            Some(Self::Left)
        } else if rect.right.abs_diff(work.right) <= distance as u32 {
            Some(Self::Right)
        } else {
            None
        }
    }

    fn x(self, work: RECT, width: i32) -> i32 {
        match self {
            Self::Left => work.left,
            Self::Right => work.right - width,
        }
    }
}

#[derive(Default)]
pub(super) struct Dock {
    folded: Option<(Edge, i32)>,
}

impl Dock {
    pub(super) fn arrow(&self) -> Option<&'static str> {
        self.folded.map(|(edge, _)| match edge {
            Edge::Left => "›",
            Edge::Right => "‹",
        })
    }

    fn tick(&mut self, window: &Window, show_queue: bool) -> bool {
        let Some((hwnd, rect, work)) = geometry(window) else {
            return false;
        };
        let mut cursor = POINT::default();
        // Both input APIs are read-only and cursor points to writable stack storage.
        if unsafe { GetCursorPos(&mut cursor) == 0 || GetAsyncKeyState(i32::from(VK_LBUTTON)) < 0 }
        {
            return false;
        }
        let over_window = cursor.x >= rect.left
            && cursor.x < rect.right
            && cursor.y >= rect.top
            && cursor.y < rect.bottom;
        if self.folded.is_some() {
            return over_window && self.expand(window);
        }
        if over_window || show_queue {
            return false;
        }
        let scale = window.scale_factor();
        let Some(edge) = Edge::near(rect, work, (12. * scale).round() as i32) else {
            return false;
        };
        let ribbon = (32. * scale).round() as i32;
        let width = rect.right - rect.left;
        if width <= ribbon || !move_window(hwnd, rect, work, edge, ribbon) {
            return false;
        }
        self.folded = Some((edge, width));
        true
    }

    pub(super) fn expand(&mut self, window: &Window) -> bool {
        let Some((edge, width)) = self.folded else {
            return false;
        };
        let Some((hwnd, rect, work)) = geometry(window) else {
            return false;
        };
        if !move_window(hwnd, rect, work, edge, width.min(work.right - work.left)) {
            return false;
        }
        self.folded = None;
        true
    }
}

pub(super) fn watch(window: &Window, cx: &Context<MiniPlayer>) {
    cx.spawn_in(window, async move |entity, cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(130))
                .await;
            if entity
                .update_in(cx, |player, window, cx| {
                    if player.dock.tick(window, player.show_queue) {
                        cx.notify();
                    }
                })
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
}

fn geometry(window: &Window) -> Option<(HWND, RECT, RECT)> {
    let RawWindowHandle::Win32(handle) = HasWindowHandle::window_handle(window).ok()?.as_raw()
    else {
        return None;
    };
    let hwnd = handle.hwnd.get() as HWND;
    let mut rect = RECT::default();
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // The borrowed GPUI window owns hwnd. The output structures have the Win32 ABI layout.
    unsafe {
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.is_null() || GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
    }
    Some((hwnd, rect, info.rcWork))
}

fn move_window(hwnd: HWND, rect: RECT, work: RECT, edge: Edge, width: i32) -> bool {
    let height = (rect.bottom - rect.top).min(work.bottom - work.top);
    let y = rect.top.clamp(work.top, work.bottom - height);
    // hwnd was obtained from the live window on the UI thread. Z-order and focus stay in place.
    unsafe {
        SetWindowPos(
            hwnd,
            ptr::null_mut(),
            edge.x(work, width),
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        ) != 0
    }
}

#[cfg(test)]
#[path = "mini_player_dock_tests.rs"]
mod tests;
