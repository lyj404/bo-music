//! UI-independent application services.

pub mod artwork;
pub mod auth;
pub mod batch_playlist;
pub mod browse;
mod comment_poster;
pub mod comments;
pub mod desktop_service;
pub mod home;
pub mod instance;
pub mod library;
pub mod likes;
#[cfg(any(target_os = "linux", test))]
mod linux_tray;
pub mod lyric_poster;
mod month_poster;
mod month_reports;
#[cfg(any(target_os = "linux", test))]
mod mpris;
mod mv;
pub mod playback;
pub mod playlist_cover;
