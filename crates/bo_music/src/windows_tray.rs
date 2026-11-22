use super::MusicApp;
use bo_music_app::desktop_service::Command;
use gpui::{Context, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{cell::Cell, io, ptr, rc::Rc};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    UI::{
        Shell::{
            DefSubclassProc, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE,
            NIM_MODIFY, NIM_SETVERSION, NIN_SELECT, NINF_KEY, NOTIFYICON_VERSION_4,
            NOTIFYICONDATAW, RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, DestroyIcon, DestroyMenu,
            GetCursorPos, GetSystemMetrics, HICON, IDI_APPLICATION, IsWindowVisible, LoadIconW,
            MF_SEPARATOR, MF_STRING, PostMessageW, RegisterWindowMessageW, SM_CXSMICON,
            SM_CYSMICON, SW_HIDE, SW_SHOW, SetForegroundWindow, ShowWindowAsync, TPM_NONOTIFY,
            TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CONTEXTMENU, WM_ENDSESSION,
            WM_LBUTTONUP, WM_NCDESTROY, WM_NULL, WM_QUERYENDSESSION, WM_RBUTTONUP,
        },
    },
};

const CALLBACK: u32 = WM_APP + 0x42;
const TRAY_ID: u32 = 1;
const KEY_SELECT: u32 = NIN_SELECT | NINF_KEY;
const MENU_SHOW: usize = 1;
const MENU_TOGGLE: usize = 2;
const MENU_PREVIOUS: usize = 3;
const MENU_NEXT: usize = 4;
const MENU_EXIT: usize = 5;
const ICON_BYTES: &[u8] = include_bytes!("../../../assets/bo_music.ico");

pub(super) struct WindowsTray {
    hwnd: HWND,
    state: Rc<TrayState>,
}

struct TrayState {
    icon: TrayIcon,
    registered: Cell<bool>,
    version4: Cell<bool>,
    attached: Cell<bool>,
    ending_session: Cell<bool>,
    popup_open: Cell<bool>,
    taskbar_created: u32,
    send: Box<dyn Fn(Command)>,
}

impl WindowsTray {
    fn new(window: &Window, send: impl Fn(Command) + 'static) -> io::Result<Self> {
        let hwnd = hwnd(window).ok_or_else(|| io::Error::other("missing Windows handle"))?;
        // The name is a static, NUL-terminated Win32 broadcast message name.
        let taskbar_created = unsafe { RegisterWindowMessageW(windows_sys::w!("TaskbarCreated")) };
        let state = Rc::new(TrayState {
            icon: TrayIcon::load()?,
            registered: Cell::new(false),
            version4: Cell::new(false),
            attached: Cell::new(false),
            ending_session: Cell::new(false),
            popup_open: Cell::new(false),
            taskbar_created,
            send: Box::new(send),
        });
        // The native registration owns a reference until detachment or WM_NCDESTROY.
        let native = Rc::into_raw(Rc::clone(&state));
        if unsafe { SetWindowSubclass(hwnd, Some(tray_message), TRAY_ID as usize, native as usize) }
            == 0
        {
            unsafe { drop(Rc::from_raw(native)) };
            return Err(io::Error::other("could not observe the tray window"));
        }
        state.attached.set(true);
        state.register(hwnd);
        Ok(Self { hwnd, state })
    }
}

impl Drop for WindowsTray {
    fn drop(&mut self) {
        if self.state.attached.get() {
            self.state.unregister(self.hwnd);
            // The callback and its reference data were registered on this same UI thread.
            if unsafe { RemoveWindowSubclass(self.hwnd, Some(tray_message), TRAY_ID as usize) } != 0
                && self.state.attached.replace(false)
            {
                unsafe { Rc::decrement_strong_count(Rc::as_ptr(&self.state)) };
            }
        }
    }
}

impl TrayState {
    fn data(&self, hwnd: HWND) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: CALLBACK,
            hIcon: self.icon.handle,
            ..Default::default()
        };
        for (slot, unit) in data.szTip.iter_mut().zip("bo_music".encode_utf16()) {
            *slot = unit;
        }
        data
    }

    fn register(&self, hwnd: HWND) {
        let mut data = self.data(hwnd);
        // The icon handle stays alive while the shell registration is active.
        let added = unsafe {
            Shell_NotifyIconW(NIM_ADD, &data) != 0 || Shell_NotifyIconW(NIM_MODIFY, &data) != 0
        };
        self.registered.set(added);
        self.version4.set(
            added && {
                data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) != 0 }
            },
        );
    }

    fn unregister(&self, hwnd: HWND) {
        if self.registered.replace(false) {
            unsafe { Shell_NotifyIconW(NIM_DELETE, &self.data(hwnd)) };
        }
    }

    fn menu(&self, hwnd: HWND, anchor: Option<POINT>) {
        if self.popup_open.replace(true) {
            return;
        }
        let selected = popup_menu(hwnd, anchor);
        self.popup_open.set(false);
        let command = match selected as usize {
            MENU_SHOW => Command::RaiseMainWindow,
            MENU_TOGGLE => Command::Toggle,
            MENU_PREVIOUS => Command::Previous,
            MENU_NEXT => Command::Next,
            MENU_EXIT => Command::ExitApplication,
            _ => return,
        };
        (self.send)(command);
    }
}

unsafe extern "system" fn tray_message(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass: usize,
    data: usize,
) -> LRESULT {
    // SetWindowSubclass calls this on the owning UI thread. Keep state alive through the popup's
    // nested message loop even if destruction removes the subclass before that loop returns.
    let state = unsafe {
        let pointer = data as *const TrayState;
        Rc::increment_strong_count(pointer);
        Rc::from_raw(pointer)
    };
    if message == CALLBACK {
        match (state.version4.get(), u32::from(lparam as u16)) {
            (true, NIN_SELECT | KEY_SELECT) | (false, WM_LBUTTONUP) => {
                (state.send)(Command::RaiseMainWindow)
            }
            (true, WM_CONTEXTMENU) => state.menu(
                hwnd,
                Some(POINT {
                    x: i32::from(wparam as i16),
                    y: i32::from((wparam >> 16) as i16),
                }),
            ),
            (false, WM_RBUTTONUP) => state.menu(hwnd, None),
            _ => {}
        }
        return 0;
    }
    if state.taskbar_created != 0 && message == state.taskbar_created {
        state.register(hwnd);
        if !state.registered.get() && unsafe { IsWindowVisible(hwnd) } == 0 {
            (state.send)(Command::RaiseMainWindow);
        }
        return 0;
    }
    match message {
        WM_QUERYENDSESSION => state.ending_session.set(true),
        WM_ENDSESSION => state.ending_session.set(wparam != 0),
        WM_NCDESTROY => {
            state.unregister(hwnd);
            unsafe { RemoveWindowSubclass(hwnd, Some(tray_message), subclass) };
            if state.attached.replace(false) {
                unsafe { Rc::decrement_strong_count(data as *const TrayState) };
            }
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

fn popup_menu(hwnd: HWND, anchor: Option<POINT>) -> i32 {
    // Menu strings are static. Win32 owns the menu only until DestroyMenu.
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return 0;
        }
        for (id, label) in [
            (MENU_SHOW, windows_sys::w!("显示主窗口")),
            (MENU_TOGGLE, windows_sys::w!("播放 / 暂停")),
            (MENU_PREVIOUS, windows_sys::w!("上一首")),
            (MENU_NEXT, windows_sys::w!("下一首")),
        ] {
            AppendMenuW(menu, MF_STRING, id, label);
        }
        AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
        AppendMenuW(menu, MF_STRING, MENU_EXIT, windows_sys::w!("退出bo_music"));
        let mut cursor = anchor.unwrap_or_default();
        let selected = if anchor.is_some() || GetCursorPos(&mut cursor) != 0 {
            SetForegroundWindow(hwnd);
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                cursor.x,
                cursor.y,
                0,
                hwnd,
                ptr::null(),
            )
        } else {
            0
        };
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        selected
    }
}

struct TrayIcon {
    handle: HICON,
    owned: bool,
}

impl TrayIcon {
    fn load() -> io::Result<Self> {
        // Screen small-icon dimensions are used to choose the best entry in the embedded app ICO.
        let (width, height) = unsafe {
            (
                GetSystemMetrics(SM_CXSMICON).max(16),
                GetSystemMetrics(SM_CYSMICON).max(16),
            )
        };
        if let Some(payload) = icon_payload(ICON_BYTES, width, height) {
            // `CreateIconFromResourceEx` reads a DWORD-aligned resource, which include_bytes does not guarantee.
            let mut aligned = vec![0u32; payload.len().div_ceil(4)];
            let handle = unsafe {
                ptr::copy_nonoverlapping(
                    payload.as_ptr(),
                    aligned.as_mut_ptr().cast(),
                    payload.len(),
                );
                CreateIconFromResourceEx(
                    aligned.as_ptr().cast(),
                    payload.len() as u32,
                    1,
                    0x0003_0000,
                    width,
                    height,
                    0,
                )
            };
            if !handle.is_null() {
                return Ok(Self {
                    handle,
                    owned: true,
                });
            }
        }
        let handle = unsafe { LoadIconW(ptr::null_mut(), IDI_APPLICATION) };
        if handle.is_null() {
            Err(io::Error::other("could not load the tray icon"))
        } else {
            Ok(Self {
                handle,
                owned: false,
            })
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        if self.owned {
            unsafe { DestroyIcon(self.handle) };
        }
    }
}

fn icon_payload(bytes: &[u8], width: i32, height: i32) -> Option<&[u8]> {
    if bytes.get(..4)? != [0, 0, 1, 0] {
        return None;
    }
    let count = usize::from(u16::from_le_bytes(bytes.get(4..6)?.try_into().ok()?));
    let entries_end = 6 + count * 16;
    let entries = bytes.get(6..entries_end)?;
    entries
        .chunks_exact(16)
        .filter_map(|entry| {
            let dimension = |value: u8| if value == 0 { 256 } else { i32::from(value) };
            let distance =
                dimension(entry[0]).abs_diff(width) + dimension(entry[1]).abs_diff(height);
            let length = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
            let start = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
            if length == 0 || start < entries_end {
                return None;
            }
            Some((distance, bytes.get(start..start.checked_add(length)?)?))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, payload)| payload)
}

fn hwnd(window: &Window) -> Option<HWND> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as _),
        _ => None,
    }
}

pub(super) fn restore_hidden_window(window: &Window) {
    if let Some(hwnd) = hwnd(window) {
        // GPUI restores minimized windows. Explicitly show a window hidden in the tray first.
        unsafe {
            if IsWindowVisible(hwnd) == 0 {
                ShowWindowAsync(hwnd, SW_SHOW);
            }
        }
    }
}

impl MusicApp {
    pub(super) fn init_windows_tray(&mut self, window: &Window, cx: &mut Context<Self>) {
        let commands = self.service.commands.clone();
        match WindowsTray::new(window, move |command| {
            let _ = commands.send(command);
        }) {
            Ok(tray) => {
                let state = Rc::downgrade(&tray.state);
                window.on_window_should_close(cx, move |window, _| {
                    let can_hide = state
                        .upgrade()
                        .is_some_and(|state| state.registered.get() && !state.ending_session.get());
                    if can_hide && let Some(hwnd) = hwnd(window) {
                        // If hiding fails, allow the ordinary window close.
                        return unsafe { ShowWindowAsync(hwnd, SW_HIDE) } == 0;
                    }
                    true
                });
                self.windows_tray = Some(tray);
            }
            Err(error) => eprintln!("Windows 托盘初始化失败：{error}"),
        }
    }
}

#[cfg(test)]
#[path = "windows_tray_tests.rs"]
mod tests;
