//! Latest spectrum from the system's playback output. No PCM is retained by the UI.
use crate::spectrum::SpectrumLevels;

#[cfg(any(target_os = "linux", test))]
mod pcm;
#[cfg(windows)]
#[path = "spectrum_output/windows.rs"]
mod platform;
#[cfg(target_os = "linux")]
#[path = "spectrum_output/linux.rs"]
mod platform;
#[cfg(any(windows, target_os = "linux"))]
mod worker;

#[derive(Clone, Copy)]
pub enum OutputSpectrumFrame {
    Unsupported,
    Waiting,
    Unavailable,
    Levels(SpectrumLevels),
}

#[derive(Default)]
pub struct OutputSpectrum {
    #[cfg(any(windows, target_os = "linux"))]
    worker: Option<worker::Worker>,
}

impl OutputSpectrum {
    pub fn frame(&mut self) -> OutputSpectrumFrame {
        #[cfg(any(windows, target_os = "linux"))]
        {
            if self.worker.is_none() {
                self.worker = worker::Worker::spawn().ok();
            }
            self.worker
                .as_mut()
                .map_or(OutputSpectrumFrame::Unavailable, |worker| worker.frame())
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        OutputSpectrumFrame::Unsupported
    }

    pub fn stop(&mut self) {
        #[cfg(any(windows, target_os = "linux"))]
        if let Some(worker) = self.worker.as_mut() {
            worker.stop();
        }
    }
}
