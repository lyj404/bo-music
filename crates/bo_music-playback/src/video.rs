//! Dedicated libmpv video session with bounded, CPU-rendered BGRA frames.
use super::{
    LoadSource, MpvError, MpvOptions, PlaybackState,
    native::{NativeEvent, NativeMpv, check},
    validate_source,
};
use std::{
    ffi::{c_int, c_void},
    path::PathBuf,
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use thiserror::Error;

const POLL: Duration = Duration::from_millis(8);
const STATUS_INTERVAL: Duration = Duration::from_millis(100);
const FRAME_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PIXELS: u32 = 1280 * 720;
// Values from mpv/render.h.
const API_TYPE: c_int = 1;
const SW_SIZE: c_int = 17;
const SW_FORMAT: c_int = 18;
const SW_STRIDE: c_int = 19;
const SW_POINTER: c_int = 20;
const UPDATE_FRAME: u64 = 1;

#[derive(Debug, Error)]
pub enum VideoError {
    #[error(transparent)]
    Mpv(#[from] MpvError),
    #[error("this libmpv build does not support software video output")]
    RendererUnavailable,
    #[error("video render surface exceeds its allowed dimensions")]
    InvalidDimensions,
    #[error("video playback command queue is full")]
    Busy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoSize {
    width: u32,
    height: u32,
}

impl VideoSize {
    pub fn new(width: u32, height: u32) -> Result<Self, VideoError> {
        if width == 0 || height == 0 || width > 1280 || height > 1280 || width * height > MAX_PIXELS
        {
            return Err(VideoError::InvalidDimensions);
        }
        Ok(Self { width, height })
    }
}

#[derive(Debug)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    /// Owned tightly packed pixels, four bytes per pixel in BGRA order.
    pub bgra: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct VideoSnapshot {
    pub state: PlaybackState,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub frame: Option<Arc<VideoFrame>>,
    pub failure: Option<VideoFailure>,
    pub revision: u64,
}

#[derive(Clone)]
pub struct VideoReader(Arc<Mutex<VideoSnapshot>>);

impl VideoReader {
    pub fn snapshot(&self) -> VideoSnapshot {
        self.0.lock().unwrap().clone()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoFailure {
    Playback(i32),
    NoVideoFrame,
    Renderer,
}

enum Request {
    Pause(bool),
    Seek(f64),
    Volume(f64),
}

pub struct VideoPlayer {
    commands: mpsc::SyncSender<Request>,
    latest: Arc<Mutex<VideoSnapshot>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl VideoPlayer {
    /// Initializes libmpv on a worker. Call outside the UI thread because startup is synchronous.
    pub fn open(
        library: PathBuf,
        source: LoadSource,
        options: MpvOptions,
        size: VideoSize,
    ) -> Result<Self, VideoError> {
        validate_source(&source)?;
        let (commands, receiver) = mpsc::sync_channel(16);
        let (started, ready) = mpsc::sync_channel(1);
        let latest = Arc::new(Mutex::new(VideoSnapshot {
            state: PlaybackState::Loading,
            ..Default::default()
        }));
        let output = Arc::clone(&latest);
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("bo_music-video".into())
            .spawn(move || {
                let result = Session::open(library, source, options, size);
                match result {
                    Ok(mut session) => {
                        if started.send(Ok(())).is_ok() {
                            session.run(receiver, &output, &cancel);
                        }
                    }
                    Err(error) => {
                        let _ = started.send(Err(error));
                    }
                }
            })
            .map_err(|_| MpvError::Worker)?;
        let player = Self {
            commands,
            latest,
            stop,
            worker: Some(worker),
        };
        ready.recv().map_err(|_| MpvError::Worker)??;
        Ok(player)
    }

    pub fn snapshot(&self) -> VideoSnapshot {
        self.latest.lock().unwrap().clone()
    }

    pub fn reader(&self) -> VideoReader {
        VideoReader(Arc::clone(&self.latest))
    }
    pub fn pause(&self, paused: bool) -> Result<(), VideoError> {
        self.send(Request::Pause(paused))
    }
    pub fn seek(&self, seconds: f64) -> Result<(), VideoError> {
        if !seconds.is_finite() || seconds < 0. {
            return Err(MpvError::InvalidArgument.into());
        }
        self.send(Request::Seek(seconds))
    }
    pub fn volume(&self, percent: f64) -> Result<(), VideoError> {
        if !percent.is_finite() {
            return Err(MpvError::InvalidArgument.into());
        }
        self.send(Request::Volume(percent.clamp(0., 100.)))
    }

    /// Close the session and wait for native teardown. Use this on a background worker when
    /// another player must wait for video audio output to be released.
    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    fn send(&self, request: Request) -> Result<(), VideoError> {
        self.commands
            .try_send(request)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => VideoError::Busy,
                mpsc::TrySendError::Disconnected(_) => MpvError::Closed.into(),
            })
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Native teardown can wait for I/O. If still running, the worker will finish independently.
        if let Some(worker) = self.worker.take()
            && worker.is_finished()
        {
            let _ = worker.join();
        }
    }
}

struct Session {
    // Field order joins the render thread and frees its context before destroying the core.
    renderer: RenderWorker,
    mpv: NativeMpv,
    entry: i64,
}

impl Session {
    fn open(
        library: PathBuf,
        source: LoadSource,
        options: MpvOptions,
        size: VideoSize,
    ) -> Result<Self, VideoError> {
        let mpv = NativeMpv::open_for_output(library, &options, true)?;
        let renderer = RenderWorker::open(&mpv, size)?;
        let entry = mpv.load(source)?;
        Ok(Self {
            renderer,
            mpv,
            entry,
        })
    }

    fn run(
        &mut self,
        commands: mpsc::Receiver<Request>,
        output: &Mutex<VideoSnapshot>,
        stop: &AtomicBool,
    ) {
        let mut started = false;
        let mut loaded = false;
        let mut next_status = Instant::now();
        let mut frame_deadline = Instant::now() + FRAME_TIMEOUT;
        while !stop.load(Ordering::Acquire) {
            match commands.recv_timeout(POLL) {
                Ok(request) => self.apply(request),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            for _ in 0..32 {
                let failure = match self.mpv.wait_event(0.) {
                    NativeEvent::Start(id) if id == self.entry => {
                        started = true;
                        None
                    }
                    NativeEvent::Loaded if started && !loaded => {
                        loaded = true;
                        frame_deadline = Instant::now() + FRAME_TIMEOUT;
                        None
                    }
                    NativeEvent::End { id, reason, error } if id == self.entry => {
                        if reason == 0 && output.lock().unwrap().frame.is_some() {
                            let mut view = output.lock().unwrap();
                            view.state = PlaybackState::Stopped;
                            view.revision = view.revision.wrapping_add(1);
                            continue;
                        }
                        Some(if error < 0 {
                            VideoFailure::Playback(error)
                        } else {
                            VideoFailure::NoVideoFrame
                        })
                    }
                    NativeEvent::Overflow | NativeEvent::Shutdown => Some(VideoFailure::Renderer),
                    NativeEvent::None => break,
                    _ => None,
                };
                if let Some(failure) = failure {
                    fail(output, failure);
                    return;
                }
            }
            if self.renderer.failed.load(Ordering::Acquire) {
                fail(output, VideoFailure::Renderer);
                return;
            }
            if loaded
                && self.renderer.has_frame()
                && self
                    .mpv
                    .number("video-params/w")
                    .is_some_and(|width| width > 0.)
                && let Some(frame) = self.renderer.take_frame()
            {
                let mut view = output.lock().unwrap();
                view.frame = Some(Arc::new(frame));
                view.revision = view.revision.wrapping_add(1);
            }
            if Instant::now() >= frame_deadline && output.lock().unwrap().frame.is_none() {
                fail(output, VideoFailure::NoVideoFrame);
                return;
            }
            if loaded && Instant::now() >= next_status {
                self.publish_status(output);
                next_status = Instant::now() + STATUS_INTERVAL;
            }
        }
    }

    fn apply(&self, request: Request) {
        let _ = match request {
            Request::Pause(paused) => {
                if !paused && self.mpv.flag("eof-reached") == Some(true) {
                    let _ = self.mpv.command(&["seek", "0", "absolute+exact"]);
                }
                self.mpv
                    .set_property("pause", if paused { "yes" } else { "no" })
            }
            Request::Seek(seconds) => {
                self.mpv
                    .command(&["seek", &seconds.to_string(), "absolute+exact"])
            }
            Request::Volume(percent) => self.mpv.set_property("volume", &percent.to_string()),
        };
    }

    fn publish_status(&self, output: &Mutex<VideoSnapshot>) {
        let position = self
            .mpv
            .number("time-pos")
            .filter(|p| p.is_finite())
            .unwrap_or(0.)
            .max(0.);
        let duration = self
            .mpv
            .number("duration")
            .filter(|p| p.is_finite() && *p > 0.);
        let state = if self.mpv.flag("eof-reached") == Some(true) {
            PlaybackState::Stopped
        } else if self.mpv.flag("pause") == Some(true) {
            PlaybackState::Paused
        } else {
            PlaybackState::Playing
        };
        let mut view = output.lock().unwrap();
        if (view.position_seconds, view.duration_seconds, view.state) != (position, duration, state)
        {
            view.position_seconds = position;
            view.duration_seconds = duration;
            view.state = state;
            view.revision = view.revision.wrapping_add(1);
        }
    }
}

fn fail(output: &Mutex<VideoSnapshot>, reason: VideoFailure) {
    let mut view = output.lock().unwrap();
    view.failure = Some(reason);
    view.state = PlaybackState::Failed;
    view.revision = view.revision.wrapping_add(1);
}

#[repr(C)]
struct Parameter {
    kind: c_int,
    data: *mut c_void,
}

impl Parameter {
    const END: Self = Self {
        kind: 0,
        data: ptr::null_mut(),
    };
    fn new<T>(kind: c_int, data: *const T) -> Self {
        Self {
            kind,
            data: data.cast_mut().cast(),
        }
    }
}

type Create = unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *mut Parameter) -> c_int;
type SetCallback =
    unsafe extern "C" fn(*mut c_void, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void);
type Update = unsafe extern "C" fn(*mut c_void) -> u64;
type Render = unsafe extern "C" fn(*mut c_void, *mut Parameter) -> c_int;
type Free = unsafe extern "C" fn(*mut c_void);

struct RenderWorker {
    worker: Option<JoinHandle<()>>,
    frame: Arc<Mutex<Option<VideoFrame>>>,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
}

impl RenderWorker {
    fn open(mpv: &NativeMpv, size: VideoSize) -> Result<Self, VideoError> {
        let native = RenderInit::from_mpv(mpv)?;
        let frame = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (rendered, stopping, errored) = (frame.clone(), stop.clone(), failed.clone());
        let (started, ready) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("bo_music-video-render".into())
            .spawn(move || {
                let mut renderer = match SoftwareRenderer::new(native, size) {
                    Ok(renderer) => renderer,
                    Err(error) => {
                        let _ = started.send(Err(error));
                        return;
                    }
                };
                if started.send(Ok(())).is_err() {
                    return;
                }
                while !stopping.load(Ordering::Acquire) {
                    match renderer.frame() {
                        Ok(Some(frame)) => *rendered.lock().unwrap() = Some(frame),
                        Ok(None) => thread::sleep(POLL),
                        Err(_) => {
                            errored.store(true, Ordering::Release);
                            break;
                        }
                    }
                }
            })
            .map_err(|_| MpvError::Worker)?;
        let renderer = Self {
            worker: Some(worker),
            frame,
            stop,
            failed,
        };
        ready.recv().map_err(|_| MpvError::Worker)??;
        Ok(renderer)
    }

    fn has_frame(&self) -> bool {
        self.frame.lock().unwrap().is_some()
    }
    fn take_frame(&self) -> Option<VideoFrame> {
        self.frame.lock().unwrap().take()
    }
}

impl Drop for RenderWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct RenderInit {
    core: *mut c_void,
    create: Create,
    update: Update,
    render: Render,
    free: Free,
    callback: SetCallback,
}

// SAFETY: libmpv permits render-context initialization on a separate thread. The command worker
// owns the core and dynamic library, and joins this renderer before releasing either one.
unsafe impl Send for RenderInit {}

impl RenderInit {
    fn from_mpv(mpv: &NativeMpv) -> Result<Self, VideoError> {
        // The exported function signatures match mpv/render.h.
        unsafe {
            let library = &mpv._library;
            let unsupported = |_| VideoError::RendererUnavailable;
            Ok(Self {
                core: mpv.handle,
                create: *library
                    .get(b"mpv_render_context_create\0")
                    .map_err(unsupported)?,
                update: *library
                    .get(b"mpv_render_context_update\0")
                    .map_err(unsupported)?,
                render: *library
                    .get(b"mpv_render_context_render\0")
                    .map_err(unsupported)?,
                free: *library
                    .get(b"mpv_render_context_free\0")
                    .map_err(unsupported)?,
                callback: *library
                    .get(b"mpv_render_context_set_update_callback\0")
                    .map_err(unsupported)?,
            })
        }
    }
}

struct SoftwareRenderer {
    context: *mut c_void,
    update: Update,
    render: Render,
    free: Free,
    callback: SetCallback,
    pending: Box<AtomicBool>,
    size: VideoSize,
    stride: usize,
    surface: Vec<AlignedBlock>,
}

#[derive(Clone)]
#[repr(C, align(64))]
struct AlignedBlock([u8; 64]);

impl SoftwareRenderer {
    fn new(native: RenderInit, size: VideoSize) -> Result<Self, VideoError> {
        // Symbols follow mpv/render.h. This renderer is dropped before the owning mpv instance.
        unsafe {
            let mut context = ptr::null_mut();
            let mut args = [Parameter::new(API_TYPE, c"sw".as_ptr()), Parameter::END];
            if (native.create)(&mut context, native.core, args.as_mut_ptr()) < 0
                || context.is_null()
            {
                return Err(VideoError::RendererUnavailable);
            }
            let stride = (size.width as usize * 4).next_multiple_of(64);
            let mut result = Self {
                context,
                update: native.update,
                render: native.render,
                free: native.free,
                callback: native.callback,
                pending: Box::new(AtomicBool::new(false)),
                size,
                stride,
                surface: vec![AlignedBlock([0; 64]); stride * size.height as usize / 64],
            };
            (native.callback)(
                context,
                Some(mark_pending),
                (&mut *result.pending as *mut AtomicBool).cast(),
            );
            Ok(result)
        }
    }

    fn frame(&mut self) -> Result<Option<VideoFrame>, MpvError> {
        // The callback signals from libmpv threads. This worker is the only render caller.
        if !self.pending.swap(false, Ordering::Acquire)
            || unsafe { (self.update)(self.context) } & UPDATE_FRAME == 0
        {
            return Ok(None);
        }
        let dimensions = [self.size.width as c_int, self.size.height as c_int];
        let mut args = [
            Parameter::new(SW_SIZE, dimensions.as_ptr()),
            Parameter::new(SW_FORMAT, c"bgr0".as_ptr()),
            Parameter::new(SW_STRIDE, &self.stride),
            Parameter::new(SW_POINTER, self.surface.as_mut_ptr().cast::<u8>()),
            Parameter::END,
        ];
        // The aligned surface owns stride * height writable bytes until render returns.
        check("render video", unsafe {
            (self.render)(self.context, args.as_mut_ptr())
        })?;
        let bytes = unsafe {
            std::slice::from_raw_parts(self.surface.as_ptr().cast::<u8>(), self.surface.len() * 64)
        };
        Ok(Some(VideoFrame {
            width: self.size.width,
            height: self.size.height,
            bgra: to_bgra(bytes, self.stride, self.size.width as usize),
        }))
    }
}

impl Drop for SoftwareRenderer {
    fn drop(&mut self) {
        // Disable callbacks and free the native context before dropping its callback and surface.
        unsafe {
            (self.callback)(self.context, None, ptr::null_mut());
            (self.free)(self.context);
        }
    }
}

unsafe extern "C" fn mark_pending(data: *mut c_void) {
    // Only an atomic flag is touched; the renderer owns it until the callback is disabled.
    unsafe { (*(data.cast::<AtomicBool>())).store(true, Ordering::Release) };
}

fn to_bgra(bytes: &[u8], stride: usize, width: usize) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(bytes.len() / stride * width * 4);
    for row in bytes.chunks_exact(stride) {
        pixels.extend_from_slice(&row[..width * 4]);
    }
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    pixels
}

#[cfg(test)]
#[path = "video_tests.rs"]
mod tests;
