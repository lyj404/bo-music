//! libmpv FFI bindings and ownership.
use super::{LoadSource, MpvError, MpvOptions};
use std::{
    ffi::{CString, c_char, c_int, c_void},
    path::PathBuf,
    ptr,
};

#[repr(C)]
struct MpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}
#[repr(C)]
struct MpvEndFile {
    reason: c_int,
    error: c_int,
    playlist_entry_id: i64,
}

#[repr(C)]
struct MpvStartFile {
    playlist_entry_id: i64,
}

#[repr(C)]
union MpvNodeValue {
    list: *mut MpvNodeList,
    int64: i64,
}

#[repr(C)]
struct MpvNode {
    value: MpvNodeValue,
    format: c_int,
}

#[repr(C)]
struct MpvNodeList {
    num: c_int,
    values: *mut MpvNode,
    keys: *mut *mut c_char,
}

type Create = unsafe extern "C" fn() -> *mut c_void;
type Initialize = unsafe extern "C" fn(*mut c_void) -> c_int;
type SetString = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type CommandFn = unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int;
type CommandRet = unsafe extern "C" fn(*mut c_void, *const *const c_char, *mut MpvNode) -> c_int;
type FreeNode = unsafe extern "C" fn(*mut MpvNode);
type ClientVersion = unsafe extern "C" fn() -> std::ffi::c_ulong;
type GetProperty = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int;
type WaitEvent = unsafe extern "C" fn(*mut c_void, f64) -> *const MpvEvent;
type Destroy = unsafe extern "C" fn(*mut c_void);

pub(super) enum NativeEvent {
    None,
    Start(i64),
    Loaded,
    End { id: i64, reason: i32, error: i32 },
    Overflow,
    Shutdown,
}

pub(super) struct NativeMpv {
    // This field owns the function pointers and must outlive the mpv handle.
    pub(super) _library: libloading::Library,
    pub(super) handle: *mut c_void,
    initialize: Initialize,
    set_option: SetString,
    set_property_fn: SetString,
    command_fn: CommandFn,
    command_ret: CommandRet,
    free_node: FreeNode,
    get_property: GetProperty,
    wait: WaitEvent,
    destroy: Destroy,
}

impl NativeMpv {
    pub(super) fn open(path: PathBuf, options: &MpvOptions) -> Result<Self, MpvError> {
        Self::open_for_output(path, options, false)
    }

    pub(super) fn open_for_output(
        path: PathBuf,
        options: &MpvOptions,
        video: bool,
    ) -> Result<Self, MpvError> {
        // SAFETY: The caller supplies a trusted libmpv library. Symbols match the C API;
        // the library is owned by this struct until after mpv_terminate_destroy.
        unsafe {
            let library = libloading::Library::new(path).map_err(|_| MpvError::Library)?;
            let client_version: ClientVersion = *library
                .get(b"mpv_client_api_version\0")
                .map_err(|_| MpvError::Library)?;
            let version = client_version();
            let major = version >> 16;
            let minor = version & 0xffff;
            if !((major == 1 && minor >= 108) || major == 2) {
                return Err(MpvError::UnsupportedVersion);
            }
            let create: Create = *library
                .get(b"mpv_create\0")
                .map_err(|_| MpvError::Library)?;
            let initialize = *library
                .get(b"mpv_initialize\0")
                .map_err(|_| MpvError::Library)?;
            let set_option = *library
                .get(b"mpv_set_option_string\0")
                .map_err(|_| MpvError::Library)?;
            let set_property_fn = *library
                .get(b"mpv_set_property_string\0")
                .map_err(|_| MpvError::Library)?;
            let command_fn = *library
                .get(b"mpv_command\0")
                .map_err(|_| MpvError::Library)?;
            let command_ret = *library
                .get(b"mpv_command_ret\0")
                .map_err(|_| MpvError::Library)?;
            let free_node = *library
                .get(b"mpv_free_node_contents\0")
                .map_err(|_| MpvError::Library)?;
            let get_property = *library
                .get(b"mpv_get_property\0")
                .map_err(|_| MpvError::Library)?;
            let wait = *library
                .get(b"mpv_wait_event\0")
                .map_err(|_| MpvError::Library)?;
            let destroy = *library
                .get(b"mpv_terminate_destroy\0")
                .map_err(|_| MpvError::Library)?;
            let handle = create();
            if handle.is_null() {
                return Err(MpvError::Create);
            }
            let instance = Self {
                _library: library,
                handle,
                initialize,
                set_option,
                set_property_fn,
                command_fn,
                command_ret,
                free_node,
                get_property,
                wait,
                destroy,
            };
            for (name, value) in [
                ("config", "no"),
                ("terminal", "no"),
                ("vo", if video { "libmpv" } else { "null" }),
                ("vid", if video { "auto" } else { "no" }),
                ("audio-display", "no"),
                ("idle", "yes"),
                ("keep-open", if video { "yes" } else { "no" }),
                ("pause", "yes"),
            ] {
                instance.option(name, value)?;
            }
            if let Some(output) = &options.audio_output {
                instance.option("ao", output)?;
            }
            let result = (instance.initialize)(instance.handle);
            check("initialize", result)?;
            Ok(instance)
        }
    }

    fn option(&self, name: &str, value: &str) -> Result<(), MpvError> {
        let name = CString::new(name).map_err(|_| MpvError::InvalidArgument)?;
        let value = CString::new(value).map_err(|_| MpvError::InvalidArgument)?;
        // SAFETY: handle is live, and both strings are NUL terminated for the duration of the call.
        check("set option", unsafe {
            (self.set_option)(self.handle, name.as_ptr(), value.as_ptr())
        })
    }
    pub(super) fn set_property(&self, name: &str, value: &str) -> Result<(), MpvError> {
        let name = CString::new(name).map_err(|_| MpvError::InvalidArgument)?;
        let value = CString::new(value).map_err(|_| MpvError::InvalidArgument)?;
        // SAFETY: handle is live, and both strings are NUL terminated for the duration of the call.
        check("set property", unsafe {
            (self.set_property_fn)(self.handle, name.as_ptr(), value.as_ptr())
        })
    }
    pub(super) fn command(&self, args: &[&str]) -> Result<(), MpvError> {
        let strings: Vec<_> = args
            .iter()
            .map(|arg| CString::new(*arg).map_err(|_| MpvError::InvalidArgument))
            .collect::<Result<_, _>>()?;
        let mut pointers: Vec<_> = strings.iter().map(|arg| arg.as_ptr()).collect();
        pointers.push(ptr::null());
        // SAFETY: mpv receives a null-terminated array of live C strings, owned for this call.
        check("command", unsafe {
            (self.command_fn)(self.handle, pointers.as_ptr())
        })
    }
    pub(super) fn load(&self, source: LoadSource) -> Result<i64, MpvError> {
        let mut options = format!(
            "pause={}",
            if source.initially_paused { "yes" } else { "no" }
        );
        if let Some(start) = source.start_seconds.filter(|n| *n > 0.0) {
            options.push_str(&format!(",start={start}"));
        }
        if let Some(end) = source.end_seconds {
            options.push_str(&format!(",end={end}"));
        }
        self.set_property("pause", if source.initially_paused { "yes" } else { "no" })?;
        let args = ["loadfile", &source.address, "replace", "0", &options];
        let strings: Vec<_> = args
            .iter()
            .map(|arg| CString::new(*arg).map_err(|_| MpvError::InvalidArgument))
            .collect::<Result<_, _>>()?;
        let mut pointers: Vec<_> = strings.iter().map(|arg| arg.as_ptr()).collect();
        pointers.push(ptr::null());
        let mut result = MpvNode {
            value: MpvNodeValue { int64: 0 },
            format: 0,
        };
        // SAFETY: arguments are live C strings and result is a correctly laid out mpv_node.
        let code = unsafe { (self.command_ret)(self.handle, pointers.as_ptr(), &mut result) };
        check("load", code)?;
        // SAFETY: libmpv owns the returned node until free_node_contents. Its map format
        // guarantees valid keys and values for each entry; no pointer escapes this block.
        let id = unsafe {
            let id = if result.format == 8 {
                result.value.list.as_ref().and_then(|list| {
                    (0..list.num.max(0) as usize).find_map(|index| {
                        let key = *list.keys.add(index);
                        let value = &*list.values.add(index);
                        if !key.is_null()
                            && std::ffi::CStr::from_ptr(key).to_bytes() == b"playlist_entry_id"
                            && value.format == 4
                        {
                            Some(value.value.int64)
                        } else {
                            None
                        }
                    })
                })
            } else {
                None
            };
            (self.free_node)(&mut result);
            id
        };
        if id.is_none() {
            let _ = self.command(&["stop"]);
        }
        id.ok_or(MpvError::MissingEntryId)
    }
    pub(super) fn number(&self, name: &str) -> Option<f64> {
        let name = CString::new(name).ok()?;
        let mut value = 0_f64;
        // SAFETY: MPV_FORMAT_DOUBLE writes an f64 to the supplied valid pointer.
        let result = unsafe {
            (self.get_property)(
                self.handle,
                name.as_ptr(),
                5,
                (&mut value as *mut f64).cast(),
            )
        };
        (result >= 0).then_some(value)
    }
    pub(super) fn flag(&self, name: &str) -> Option<bool> {
        let name = CString::new(name).ok()?;
        let mut value: c_int = 0;
        // SAFETY: MPV_FORMAT_FLAG writes an int to the supplied valid pointer.
        let result = unsafe {
            (self.get_property)(
                self.handle,
                name.as_ptr(),
                3,
                (&mut value as *mut c_int).cast(),
            )
        };
        (result >= 0).then_some(value != 0)
    }
    pub(super) fn wait_event(&self, seconds: f64) -> NativeEvent {
        // SAFETY: only this worker calls mpv_wait_event; pointers remain valid until the next call.
        unsafe {
            let event = (self.wait)(self.handle, seconds);
            let Some(event) = event.as_ref() else {
                return NativeEvent::Shutdown;
            };
            match event.event_id {
                1 => NativeEvent::Shutdown,
                6 => match (event.data as *const MpvStartFile).as_ref() {
                    Some(start) => NativeEvent::Start(start.playlist_entry_id),
                    None => NativeEvent::Overflow,
                },
                8 => NativeEvent::Loaded,
                7 => match (event.data as *const MpvEndFile).as_ref() {
                    Some(end) => NativeEvent::End {
                        id: end.playlist_entry_id,
                        reason: end.reason,
                        error: end.error,
                    },
                    None => NativeEvent::Overflow,
                },
                24 => NativeEvent::Overflow,
                _ => NativeEvent::None,
            }
        }
    }
}
impl Drop for NativeMpv {
    fn drop(&mut self) {
        // SAFETY: the handle was created once, remains on this thread, and is destroyed once before the library unloads.
        unsafe { (self.destroy)(self.handle) };
    }
}
pub(super) fn check(operation: &'static str, code: i32) -> Result<(), MpvError> {
    if code < 0 {
        Err(MpvError::Native { operation, code })
    } else {
        Ok(())
    }
}
