//! Persistent application data and operating-system protected credentials.

pub mod atomic;
pub mod credentials;
pub mod device;
pub mod history;
#[cfg(windows)]
pub mod legacy_credentials;
pub mod migration;
pub mod paths;
pub mod queue;
pub mod search_history;
pub mod settings;
pub mod shortcuts;
