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
        root.map(|p| Self::new(p.join("BowenRust")))
            .ok_or(PathError::NoDataDirectory)
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
