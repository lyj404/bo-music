//! Bounded reads and same-directory staged writes.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub fn read_limited(path: &Path) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take((MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "document too large",
        ));
    }
    Ok(bytes)
}

struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn stage(path: &Path, bytes: &[u8]) -> io::Result<TempFile> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "document too large",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    fs::create_dir_all(parent)?;
    loop {
        let number = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".bo_music-{}-{number}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temp) {
            Ok(mut file) => {
                let guard = TempFile(temp);
                file.write_all(bytes)?;
                file.sync_all()?;
                return Ok(guard);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Replaces a file using a fully written, synced temporary sibling.
pub fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let staged = stage(path, bytes)?;
    fs::rename(&staged.0, path)?;
    sync_directory(path.parent().expect("stage checked the parent"))
}

/// Publish without overwriting an existing destination, including across processes.
pub fn create(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    let staged = stage(path, bytes)?;
    match fs::hard_link(&staged.0, path) {
        Ok(()) => {
            sync_directory(path.parent().expect("stage checked the parent"))?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}
