//! Per-data-directory application ownership and activation requests.
use bowen_storage::paths::DataPaths;
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::{Path, PathBuf},
};

pub struct PrimaryInstance {
    // Keep the OS lock alive until the application's event loop returns. The lock file is never
    // unlinked, so every contender always locks the same file even across shutdown and restart.
    _lock: File,
    activation: ActivationInbox,
}

#[derive(Clone)]
pub struct ActivationInbox(PathBuf);

impl PrimaryInstance {
    /// `None` means another process owns the data directory and has been asked to activate.
    pub fn start() -> io::Result<Option<Self>> {
        let paths = DataPaths::platform_default().map_err(io::Error::other)?;
        Self::start_in(paths.root())
    }

    fn start_in(root: &Path) -> io::Result<Option<Self>> {
        fs::create_dir_all(root)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("instance.lock"))?;
        let activation = ActivationInbox(root.join("instance.activate"));
        match lock.try_lock() {
            Ok(()) => Ok(Some(Self {
                _lock: lock,
                activation,
            })),
            Err(TryLockError::WouldBlock) => {
                activation.request()?;
                Ok(None)
            }
            Err(TryLockError::Error(error)) => Err(error),
        }
    }

    pub fn activation_inbox(&self) -> ActivationInbox {
        self.activation.clone()
    }
}

impl ActivationInbox {
    fn request(&self) -> io::Result<()> {
        match File::create_new(&self.0) {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Consume a pending request. Several launches before the next poll coalesce into one.
    pub fn take_request(&self) -> io::Result<bool> {
        match fs::remove_file(&self.0) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
#[path = "instance_tests.rs"]
mod tests;
