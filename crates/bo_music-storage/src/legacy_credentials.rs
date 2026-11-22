//! One-time Windows DPAPI import. Only ciphertext is ever written to disk.
use crate::{
    atomic,
    credentials::{Credential, CredentialError, CredentialStore, SecretBackend},
    migration::{self, MigrationError},
    paths::DataPaths,
};
use serde::Deserialize;
use std::{fs, io, path::Path, ptr};
use thiserror::Error;
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData},
};
use zeroize::{Zeroize, Zeroizing};

const MAX_SECRET_BYTES: usize = 1024 * 1024;
const MARKER: &str = "legacy-credentials-import-complete";

#[derive(Debug, Error)]
pub enum LegacyCredentialError {
    #[error("legacy credential file is invalid or cannot be decrypted by this Windows user")]
    Invalid,
    #[error("legacy credential migration failed")]
    Migration(#[from] MigrationError),
    #[error("legacy credential I/O failed")]
    Io(#[from] io::Error),
    #[error("system credential store is unavailable or invalid")]
    Store(#[from] CredentialError),
}

#[derive(Deserialize)]
struct LegacyCredential {
    #[serde(rename = "Uid")]
    uid: String,
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "Nickname")]
    nickname: Option<String>,
}

impl Drop for LegacyCredential {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

impl LegacyCredential {
    fn credential(&self) -> Credential {
        Credential {
            uid: self.uid.clone(),
            token: self.token.clone(),
            nickname: self.nickname.clone(),
        }
    }
}

#[derive(Deserialize)]
struct LegacyAccounts {
    #[serde(rename = "Accounts")]
    accounts: Option<Vec<Option<LegacyAccount>>>,
}

#[derive(Deserialize)]
struct LegacyAccount {
    #[serde(rename = "Credential")]
    credential: LegacyCredential,
}

/// Must run on a blocking worker. Existing Rust vaults always take precedence.
pub fn import_once<B: SecretBackend>(
    source: &Path,
    target: &DataPaths,
    store: &CredentialStore<B>,
) -> Result<bool, LegacyCredentialError> {
    let destination = fs::canonicalize(target.root())?;
    let marker = destination.join(MARKER);
    if migration::regular_file(&marker)? {
        return Ok(false);
    }
    if store.has_stored_state()? {
        atomic::create(&marker, b"1")?;
        return Ok(false);
    }
    let source = match fs::symlink_metadata(source) {
        Ok(metadata) if metadata.file_type().is_dir() => fs::canonicalize(source)?,
        Ok(_) => return Err(MigrationError::UnsafeFile.into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            atomic::create(&marker, b"1")?;
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    };
    if source.starts_with(&destination) || destination.starts_with(&source) {
        return Err(MigrationError::OverlappingRoots.into());
    }
    let mut encrypted = Vec::new();
    for name in ["session.dat", "remembered-accounts.dat"] {
        let original = source.join(name);
        if !migration::regular_file(&original)? {
            encrypted.push(None);
            continue;
        }
        let relative = Path::new("legacy-backup").join("credentials").join(name);
        migration::ensure_safe_parents(&destination, &relative)?;
        let backup = destination.join(relative);
        if !migration::regular_file(&backup)? {
            let data = atomic::read_limited(&original)?;
            if data.len() > MAX_SECRET_BYTES {
                return Err(LegacyCredentialError::Invalid);
            }
            atomic::create(&backup, &data)?;
        }
        let data = atomic::read_limited(&backup)?;
        if data.len() > MAX_SECRET_BYTES {
            return Err(LegacyCredentialError::Invalid);
        }
        encrypted.push(Some(data));
    }
    let active = encrypted[0]
        .as_ref()
        .map(|data| {
            let plaintext = unprotect(data, b"Bodian.Session.v1")?;
            let parsed: LegacyCredential =
                serde_json::from_slice(&plaintext).map_err(|_| LegacyCredentialError::Invalid)?;
            Ok::<_, LegacyCredentialError>(parsed.credential())
        })
        .transpose()?;
    let accounts = encrypted[1]
        .as_ref()
        .map(|data| {
            let plaintext = unprotect(data, b"Bodian.Accounts.v1")?;
            let parsed: LegacyAccounts =
                serde_json::from_slice(&plaintext).map_err(|_| LegacyCredentialError::Invalid)?;
            Ok::<_, LegacyCredentialError>(
                parsed
                    .accounts
                    .unwrap_or_default()
                    .iter()
                    .flatten()
                    .map(|account| account.credential.credential())
                    .collect(),
            )
        })
        .transpose()?
        .unwrap_or_default();
    let imported = store.import_if_empty(active, accounts)?;
    atomic::create(&marker, b"1")?;
    Ok(imported)
}

struct DpapiOutput(CRYPT_INTEGER_BLOB);
impl Drop for DpapiOutput {
    fn drop(&mut self) {
        if !self.0.pbData.is_null() {
            // SAFETY: DPAPI owns this allocation, with cbData initialized bytes.
            unsafe {
                std::slice::from_raw_parts_mut(self.0.pbData, self.0.cbData as usize).zeroize();
                LocalFree(self.0.pbData.cast());
            }
        }
    }
}

fn unprotect(data: &[u8], entropy: &[u8]) -> Result<Zeroizing<Vec<u8>>, LegacyCredentialError> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: data
            .len()
            .try_into()
            .map_err(|_| LegacyCredentialError::Invalid)?,
        pbData: data.as_ptr().cast_mut(),
    };
    let salt = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr().cast_mut(),
    };
    let mut output = DpapiOutput(CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    });
    // SAFETY: pointers are valid for the call; output is freed and wiped by DpapiOutput.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            &salt,
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output.0,
        )
    };
    if ok == 0 || output.0.pbData.is_null() || output.0.cbData as usize > MAX_SECRET_BYTES {
        return Err(LegacyCredentialError::Invalid);
    }
    // SAFETY: a successful DPAPI call returns cbData initialized bytes.
    Ok(Zeroizing::new(
        unsafe { std::slice::from_raw_parts(output.0.pbData, output.0.cbData as usize) }.to_vec(),
    ))
}

#[cfg(test)]
#[path = "legacy_credentials_tests.rs"]
mod tests;
