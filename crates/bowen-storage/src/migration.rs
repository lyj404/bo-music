//! Explicit, repeatable import of legacy non-credential data.
use crate::{atomic, paths::DataPaths};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use thiserror::Error;

pub(crate) const STANDALONE_SETTINGS: &[(&str, &str)] = &[
    ("settings.json", "appearance"),
    ("font-settings.json", "fonts"),
    ("shell-background.json", "shellBackground"),
    ("playback.json", "playback"),
    ("volume.json", "volume"),
    ("audio-quality.json", "audioQuality"),
    ("audio-effects.json", "audioEffects"),
    ("lyrics.json", "lyrics"),
    ("desktop-lyrics.json", "desktopLyrics"),
    ("lyrics-poster.json", "lyricPoster"),
    ("view-mode.json", "viewMode"),
    ("shortcuts.json", "shortcuts"),
    ("input-method.json", "inputMethod"),
];
pub(crate) const STANDALONE_WINDOWS: &[(&str, &str)] = &[
    ("window.json", "main"),
    ("desktop-lyrics-window.json", "desktopLyrics"),
    ("mini-player-window.json", "miniPlayer"),
];

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("migration I/O failed")]
    Io(#[from] io::Error),
    #[error("legacy and new directories must not overlap")]
    OverlappingRoots,
    #[error("legacy data contains a symbolic link or a non-file")]
    UnsafeFile,
}

#[derive(Default, Debug, Eq, PartialEq)]
pub struct MigrationReport {
    pub imported: usize,
    pub preserved: usize,
}

pub(crate) fn regular_file(path: &Path) -> Result<bool, MigrationError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => Ok(true),
        Ok(_) => Err(MigrationError::UnsafeFile),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Import only the allowlisted non-credential files. The entire selected source
/// set is backed up before any live destination file is published.
pub fn import_legacy(
    source_root: &Path,
    target: &DataPaths,
) -> Result<MigrationReport, MigrationError> {
    let source = fs::canonicalize(source_root)?;
    let destination = fs::canonicalize(target.root())?;
    if source.starts_with(&destination) || destination.starts_with(&source) {
        return Err(MigrationError::OverlappingRoots);
    }
    back_up_standalone_settings(&source, &destination)?;
    let mut files: Vec<PathBuf> = vec![
        "config.json".into(),
        "config.json.bak".into(),
        "devid.txt".into(),
    ];
    let accounts = source.join("accounts");
    match fs::symlink_metadata(&accounts) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            for entry in fs::read_dir(&accounts)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let Some(scope) = name.to_str() else {
                    continue;
                };
                if target.account_dir(scope).is_err() {
                    continue;
                }
                for item in ["queue.json", "history.json", "search-history.json"] {
                    files.push(Path::new("accounts").join(scope).join(item));
                }
            }
        }
        Ok(_) => return Err(MigrationError::UnsafeFile),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    files.sort();
    let mut prepared = Vec::new();
    for relative in files {
        let original = source.join(&relative);
        if !regular_file(&original)? {
            continue;
        }
        let backup_relative = Path::new("legacy-backup").join(&relative);
        ensure_safe_parents(&destination, &backup_relative)?;
        let backup = destination.join(backup_relative);
        if !regular_file(&backup)? {
            let data = atomic::read_limited(&original)?;
            atomic::create(&backup, &data)?;
        }
        // A prior interrupted run keeps its first backup and resumes from it.
        let data = atomic::read_limited(&backup)?;
        prepared.push((relative, data));
    }
    let mut report = MigrationReport::default();
    for (relative, data) in prepared {
        ensure_safe_parents(&destination, &relative)?;
        if atomic::create(&destination.join(relative), &data)? {
            report.imported += 1;
        } else {
            report.preserved += 1;
        }
    }
    Ok(report)
}

/// Backfill the settings backups for profiles whose other legacy data was already imported.
pub fn import_standalone_settings(
    source_root: &Path,
    target: &DataPaths,
) -> Result<(), MigrationError> {
    let source = fs::canonicalize(source_root)?;
    let destination = fs::canonicalize(target.root())?;
    if source.starts_with(&destination) || destination.starts_with(&source) {
        return Err(MigrationError::OverlappingRoots);
    }
    back_up_standalone_settings(&source, &destination)
}

fn back_up_standalone_settings(source: &Path, destination: &Path) -> Result<(), MigrationError> {
    for (filename, _) in STANDALONE_SETTINGS.iter().chain(STANDALONE_WINDOWS) {
        let original = source.join(filename);
        if !regular_file(&original)? {
            continue;
        }
        let relative = Path::new("legacy-backup").join(filename);
        ensure_safe_parents(destination, &relative)?;
        let backup = destination.join(relative);
        if !regular_file(&backup)? {
            atomic::create(&backup, &atomic::read_limited(&original)?)?;
        }
    }
    Ok(())
}

pub(crate) fn ensure_safe_parents(root: &Path, relative: &Path) -> Result<(), MigrationError> {
    let mut current = root.to_path_buf();
    if let Some(parent) = relative.parent() {
        for part in parent.components() {
            let std::path::Component::Normal(name) = part else {
                return Err(MigrationError::UnsafeFile);
            };
            current.push(name);
            match fs::create_dir(&current) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            if !fs::symlink_metadata(&current)?.file_type().is_dir() {
                return Err(MigrationError::UnsafeFile);
            }
        }
    }
    Ok(())
}
