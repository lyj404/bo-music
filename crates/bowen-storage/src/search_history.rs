//! Account-scoped keyword history using the WinUI string-array format.
use crate::{
    atomic,
    paths::{DataPaths, PathError},
};
use std::{collections::HashSet, io, path::Path};

const CAPACITY: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum SearchHistoryError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub struct SearchHistoryStore(DataPaths);

impl SearchHistoryStore {
    pub fn new(paths: DataPaths) -> Self {
        Self(paths)
    }

    pub fn load(&self, scope: &str) -> Result<Vec<String>, SearchHistoryError> {
        read(&self.0.account_dir(scope)?.join("search-history.json"))
    }

    pub fn record(&self, scope: &str, keyword: String) -> Result<Vec<String>, SearchHistoryError> {
        let path = self.0.account_dir(scope)?.join("search-history.json");
        let words = normalize(std::iter::once(keyword).chain(read(&path)?));
        atomic::replace(&path, &serde_json::to_vec(&words)?)?;
        Ok(words)
    }

    pub fn clear(&self, scope: &str) -> Result<(), SearchHistoryError> {
        let path = self.0.account_dir(scope)?.join("search-history.json");
        atomic::replace(&path, b"[]")?;
        Ok(())
    }
}

fn read(path: &Path) -> Result<Vec<String>, SearchHistoryError> {
    match atomic::read_limited(path) {
        Ok(bytes) => {
            let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
            let words: Vec<String> = serde_json::from_slice(bytes)?;
            Ok(normalize(words))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn normalize(words: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    words
        .into_iter()
        .filter_map(|word| {
            let word = word.trim();
            (!word.is_empty() && seen.insert(word.to_lowercase())).then(|| word.to_owned())
        })
        .take(CAPACITY)
        .collect()
}
