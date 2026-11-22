//! Audio playback services. Native handles stay on a dedicated worker thread.
pub mod ambient;
pub mod effects;
pub mod mpv;
pub use mpv::video;
pub mod spectrum;
pub mod spectrum_output;
