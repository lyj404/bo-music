//! Device identity is global to the installation, not account-scoped.
use crate::atomic;
use std::{io, path::Path};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("device identity I/O failed")]
    Io(#[from] io::Error),
    #[error("stored device identity is invalid")]
    Invalid,
    #[error("could not generate a device identity")]
    Random,
}
fn valid(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn read(path: &Path) -> Result<String, DeviceError> {
    let bytes = atomic::read_limited(path)?;
    let id = std::str::from_utf8(&bytes)
        .map_err(|_| DeviceError::Invalid)?
        .trim();
    if !valid(id) {
        return Err(DeviceError::Invalid);
    }
    Ok(id.to_owned())
}
/// Generates once, or reuses a concurrently-created ID. Invalid existing data
/// is reported and retained rather than silently changing the device identity.
pub fn get_or_create(path: &Path) -> Result<String, DeviceError> {
    match read(path) {
        Ok(id) => return Ok(id),
        Err(DeviceError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| DeviceError::Random)?;
    let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    atomic::create(path, id.as_bytes())?;
    read(path)
}
