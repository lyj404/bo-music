use std::{
    env,
    path::{Path, PathBuf},
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PathError {
    #[error("no application data directory is available")]
    NoDataDirectory,
    #[error("invalid account scope")]
    InvalidScope,
    #[error("cannot migrate the previous application data directory")]
    Migration(#[from] std::io::Error),
}

#[derive(Clone, Debug)]
pub struct DataPaths {
    root: PathBuf,
}

impl DataPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn platform_default() -> Result<Self, PathError> {
        #[cfg(windows)]
        let root = env::var_os("LOCALAPPDATA")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        #[cfg(not(windows))]
        let root = env::var_os("XDG_DATA_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                env::var_os("HOME")
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .map(|p| p.join(".local/share"))
            });
        Self::from_data_home(root.ok_or(PathError::NoDataDirectory)?)
    }

    fn from_data_home(root: PathBuf) -> Result<Self, PathError> {
        let current = root.join("bo_music");
        // Preserve the entire Rust profile, including backups and downloaded assets.
        let previous = root.join("BowenRust");
        if !current.try_exists()? && previous.try_exists()? {
            std::fs::rename(&previous, &current)?;
        }
        Ok(Self::new(current))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn config(&self) -> PathBuf {
        self.root.join("config.json")
    }
    pub fn device_id(&self) -> PathBuf {
        self.root.join("devid.txt")
    }
    pub fn account_dir(&self, scope: &str) -> Result<PathBuf, PathError> {
        // Reject bad input rather than silently merging an account into anonymous data.
        if scope != "anonymous" && (scope.is_empty() || !scope.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(PathError::InvalidScope);
        }
        Ok(self.root.join("accounts").join(scope))
    }
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
