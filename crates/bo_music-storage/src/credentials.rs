//! Account credentials are stored only in the operating system's secret store.
//!
//! Calls may block while the user unlocks a keyring. Run them on a dedicated
//! blocking thread, never on a UI thread or a Tokio runtime worker.
use serde::{Deserialize, Serialize};
use thiserror::Error;

const VERSION: u32 = 1;
const VAULT_VERSION: u32 = 2;
const MAX_ACCOUNTS: usize = 5;
// Windows generic credentials have a 2560-byte limit; leave room for encoding.
const MAX_ENCODED_BYTES: usize = 2400;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CredentialError {
    #[error("system credential store is unavailable or locked")]
    Unavailable,
    #[error("stored credential is invalid or unsupported")]
    Invalid,
}

/// Contains a secret: deliberately does not implement Debug or Display.
#[derive(Clone, Serialize, Deserialize)]
pub struct Credential {
    pub uid: String,
    pub token: String,
    pub nickname: Option<String>,
}

impl Credential {
    fn valid(&self) -> bool {
        self.uid.parse::<i64>().is_ok_and(|uid| uid > 0)
            && !self.token.is_empty()
            && !self.token.chars().any(char::is_control)
    }
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    credential: Credential,
}

/// Public account metadata; it never contains the token or keyring entry name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RememberedAccount {
    pub uid: String,
    pub nickname: Option<String>,
    pub stale: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Slot {
    uid: String,
    nickname: Option<String>,
    key: String,
    #[serde(default)]
    stale: bool,
}

#[derive(Serialize, Deserialize)]
struct Vault {
    version: u32,
    active: Option<Slot>,
    accounts: Vec<Slot>,
    #[serde(default)]
    pending_deletions: Vec<String>,
}

impl Default for Vault {
    fn default() -> Self {
        Self {
            version: VAULT_VERSION,
            active: None,
            accounts: Vec::new(),
            pending_deletions: Vec::new(),
        }
    }
}

enum Stored {
    Empty,
    Legacy(Credential),
    Vault(Vault),
}

/// Implementations must protect the value at rest and must not log it.
pub trait SecretBackend {
    fn read(&self) -> Result<Option<String>, CredentialError>;
    fn write(&self, secret: &str) -> Result<(), CredentialError>;
    fn delete(&self) -> Result<(), CredentialError>;
    fn read_named(&self, _name: &str) -> Result<Option<String>, CredentialError> {
        Err(CredentialError::Unavailable)
    }
    fn write_named(&self, _name: &str, _secret: &str) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }
    fn delete_named(&self, _name: &str) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }
}

impl<T: SecretBackend + ?Sized> SecretBackend for std::sync::Arc<T> {
    fn read(&self) -> Result<Option<String>, CredentialError> {
        (**self).read()
    }
    fn write(&self, value: &str) -> Result<(), CredentialError> {
        (**self).write(value)
    }
    fn delete(&self) -> Result<(), CredentialError> {
        (**self).delete()
    }
    fn read_named(&self, name: &str) -> Result<Option<String>, CredentialError> {
        (**self).read_named(name)
    }
    fn write_named(&self, name: &str, value: &str) -> Result<(), CredentialError> {
        (**self).write_named(name, value)
    }
    fn delete_named(&self, name: &str) -> Result<(), CredentialError> {
        (**self).delete_named(name)
    }
}

pub struct CredentialStore<B> {
    backend: B,
}

impl<B: SecretBackend> CredentialStore<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Includes an explicitly signed-out vault, which must not be overwritten.
    pub fn has_stored_state(&self) -> Result<bool, CredentialError> {
        Ok(!matches!(self.read_state()?, Stored::Empty))
    }

    pub fn load(&self) -> Result<Option<Credential>, CredentialError> {
        match self.read_state()? {
            Stored::Empty => Ok(None),
            Stored::Legacy(credential) => Ok(Some(credential)),
            Stored::Vault(vault) => vault
                .active
                .as_ref()
                .map(|slot| self.load_slot(slot))
                .transpose(),
        }
    }

    pub fn save(&self, credential: Credential) -> Result<(), CredentialError> {
        let value = encode_credential(&credential)?;
        let state = self.read_state()?;
        let mut staged = Vec::new();
        let mut vault = match state {
            Stored::Empty => Vault::default(),
            Stored::Vault(vault) => vault,
            Stored::Legacy(old) if old.uid == credential.uid => Vault::default(),
            Stored::Legacy(old) => {
                let slot = self.stage_credential(&old, &encode_credential(&old)?)?;
                staged.push(slot.key.clone());
                Vault {
                    active: Some(slot.clone()),
                    accounts: vec![slot],
                    ..Default::default()
                }
            }
        };
        let before = live_keys(&vault);
        let slot = match self.stage_credential(&credential, &value) {
            Ok(slot) => slot,
            Err(error) => {
                self.discard_staged(&staged);
                return Err(error);
            }
        };
        staged.push(slot.key.clone());
        vault.accounts.retain(|entry| entry.uid != slot.uid);
        vault.accounts.insert(0, slot.clone());
        vault.accounts.truncate(MAX_ACCOUNTS);
        vault.active = Some(slot);
        mark_retired(&mut vault, before);
        if let Err(error) = self.write_vault(&vault) {
            self.discard_staged(&staged);
            return Err(error);
        }
        self.cleanup(&mut vault);
        Ok(())
    }

    /// Import a legacy snapshot only if no Rust credential state has ever been written.
    /// The active credential and remembered list are published in one index transaction.
    pub fn import_if_empty(
        &self,
        active: Option<Credential>,
        remembered: Vec<Credential>,
    ) -> Result<bool, CredentialError> {
        if !matches!(self.read_state()?, Stored::Empty) {
            return Ok(false);
        }
        if remembered.len() > MAX_ACCOUNTS {
            return Err(CredentialError::Invalid);
        }
        if let Some(credential) = &active {
            encode_credential(credential)?;
        }
        let mut ids = std::collections::HashSet::new();
        for credential in &remembered {
            encode_credential(credential)?;
            if !ids.insert(&credential.uid) {
                return Err(CredentialError::Invalid);
            }
        }
        if active.is_none() && remembered.is_empty() {
            return Ok(false);
        }
        let mut staged = Vec::new();
        let result = (|| {
            let active_slot = if let Some(credential) = &active {
                let slot = self.stage_credential(credential, &encode_credential(credential)?)?;
                staged.push(slot.key.clone());
                Some(slot)
            } else {
                None
            };
            let mut accounts = Vec::new();
            for credential in &remembered {
                let slot = if active
                    .as_ref()
                    .is_some_and(|current| current.uid == credential.uid)
                {
                    active_slot
                        .as_ref()
                        .expect("active credential is present")
                        .clone()
                } else {
                    let slot =
                        self.stage_credential(credential, &encode_credential(credential)?)?;
                    staged.push(slot.key.clone());
                    slot
                };
                accounts.push(slot);
            }
            if !matches!(self.read_state()?, Stored::Empty) {
                return Ok(false);
            }
            self.write_vault(&Vault {
                active: active_slot,
                accounts,
                ..Default::default()
            })?;
            Ok(true)
        })();
        if !matches!(result, Ok(true)) {
            self.discard_staged(&staged);
        }
        result
    }

    /// Sign out without forgetting remembered accounts.
    pub fn clear(&self) -> Result<(), CredentialError> {
        let mut staged = Vec::new();
        let mut vault = match self.read_state()? {
            Stored::Empty => return self.backend.delete(),
            Stored::Vault(vault) => vault,
            Stored::Legacy(old) => {
                let slot = self.stage_credential(&old, &encode_credential(&old)?)?;
                staged.push(slot.key.clone());
                Vault {
                    active: Some(slot.clone()),
                    accounts: vec![slot],
                    ..Default::default()
                }
            }
        };
        let before = live_keys(&vault);
        vault.active = None;
        mark_retired(&mut vault, before);
        if let Err(error) = self.write_vault(&vault) {
            self.discard_staged(&staged);
            return Err(error);
        }
        self.cleanup(&mut vault);
        Ok(())
    }

    /// A server-rejected session is not restored automatically or offered for switching.
    pub fn expire_active(&self) -> Result<(), CredentialError> {
        let mut staged = Vec::new();
        let mut vault = match self.read_state()? {
            Stored::Empty => return Ok(()),
            Stored::Vault(vault) => vault,
            Stored::Legacy(old) => {
                let slot = self.stage_credential(&old, &encode_credential(&old)?)?;
                staged.push(slot.key.clone());
                Vault {
                    active: Some(slot.clone()),
                    accounts: vec![slot],
                    ..Default::default()
                }
            }
        };
        let before = live_keys(&vault);
        if let Some(active) = vault.active.take()
            && let Some(slot) = vault
                .accounts
                .iter_mut()
                .find(|slot| slot.uid == active.uid)
        {
            slot.stale = true;
        }
        mark_retired(&mut vault, before);
        if let Err(error) = self.write_vault(&vault) {
            self.discard_staged(&staged);
            return Err(error);
        }
        self.cleanup(&mut vault);
        Ok(())
    }

    pub fn remembered(&self) -> Result<Vec<RememberedAccount>, CredentialError> {
        let slots = match self.read_state()? {
            Stored::Empty => Vec::new(),
            Stored::Legacy(credential) => {
                return Ok(vec![RememberedAccount {
                    uid: credential.uid,
                    nickname: credential.nickname,
                    stale: false,
                }]);
            }
            Stored::Vault(vault) => vault.accounts,
        };
        Ok(slots
            .into_iter()
            .map(|slot| RememberedAccount {
                uid: slot.uid,
                nickname: slot.nickname,
                stale: slot.stale,
            })
            .collect())
    }

    pub fn has_pending_cleanup(&self) -> Result<bool, CredentialError> {
        Ok(
            matches!(self.read_state()?, Stored::Vault(vault) if !vault.pending_deletions.is_empty()),
        )
    }

    /// Retry previously committed removals whose system-keyring deletion failed.
    pub fn retry_cleanup(&self) -> Result<(), CredentialError> {
        if let Stored::Vault(mut vault) = self.read_state()?
            && !self.cleanup(&mut vault)
        {
            return Err(CredentialError::Unavailable);
        }
        Ok(())
    }

    pub fn switch(&self, uid: &str) -> Result<Credential, CredentialError> {
        match self.read_state()? {
            Stored::Legacy(credential) if credential.uid == uid => Ok(credential),
            Stored::Vault(mut vault) => {
                let index = vault
                    .accounts
                    .iter()
                    .position(|slot| slot.uid == uid && !slot.stale)
                    .ok_or(CredentialError::Invalid)?;
                let slot = vault.accounts.remove(index);
                let credential = self.load_slot(&slot)?;
                let before = live_keys(&vault);
                vault.accounts.insert(0, slot.clone());
                vault.active = Some(slot);
                mark_retired(&mut vault, before);
                self.write_vault(&vault)?;
                self.cleanup(&mut vault);
                Ok(credential)
            }
            _ => Err(CredentialError::Invalid),
        }
    }

    /// Forget a remembered account while leaving an active session intact.
    pub fn forget(&self, uid: &str) -> Result<(), CredentialError> {
        let mut vault = match self.read_state()? {
            Stored::Empty => return Ok(()),
            Stored::Legacy(credential) => {
                if credential.uid != uid {
                    return Ok(());
                }
                let slot = self.stage_credential(&credential, &encode_credential(&credential)?)?;
                let vault = Vault {
                    active: Some(slot.clone()),
                    accounts: Vec::new(),
                    ..Default::default()
                };
                if let Err(error) = self.write_vault(&vault) {
                    self.discard_staged(&[slot.key]);
                    return Err(error);
                }
                return Ok(());
            }
            Stored::Vault(vault) => vault,
        };
        let before = live_keys(&vault);
        vault.accounts.retain(|slot| slot.uid != uid);
        mark_retired(&mut vault, before);
        self.write_vault(&vault)?;
        if self.cleanup(&mut vault) {
            Ok(())
        } else {
            Err(CredentialError::Unavailable)
        }
    }

    fn read_state(&self) -> Result<Stored, CredentialError> {
        let Some(value) = self.backend.read()? else {
            return Ok(Stored::Empty);
        };
        if value.len() > MAX_ENCODED_BYTES {
            return Err(CredentialError::Invalid);
        }
        let json: serde_json::Value =
            serde_json::from_str(&value).map_err(|_| CredentialError::Invalid)?;
        match json.get("version").and_then(serde_json::Value::as_u64) {
            Some(1) => Ok(Stored::Legacy(decode_credential(&value)?)),
            Some(2) => {
                let vault: Vault =
                    serde_json::from_value(json).map_err(|_| CredentialError::Invalid)?;
                validate_vault(&vault)?;
                Ok(Stored::Vault(vault))
            }
            _ => Err(CredentialError::Invalid),
        }
    }

    fn load_slot(&self, slot: &Slot) -> Result<Credential, CredentialError> {
        let value = self
            .backend
            .read_named(&slot.key)?
            .ok_or(CredentialError::Invalid)?;
        let credential = decode_credential(&value)?;
        if credential.uid != slot.uid {
            return Err(CredentialError::Invalid);
        }
        Ok(credential)
    }

    fn stage_credential(
        &self,
        credential: &Credential,
        value: &str,
    ) -> Result<Slot, CredentialError> {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| CredentialError::Unavailable)?;
        let key = format!(
            "account-{}-{}",
            credential.uid,
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        self.backend.write_named(&key, value)?;
        Ok(Slot {
            uid: credential.uid.clone(),
            nickname: credential
                .nickname
                .as_deref()
                .map(|name| name.chars().take(64).collect()),
            key,
            stale: false,
        })
    }

    fn discard_staged(&self, keys: &[String]) {
        for key in keys {
            let _ = self.backend.delete_named(key);
        }
    }
    fn write_vault(&self, vault: &Vault) -> Result<(), CredentialError> {
        validate_vault(vault)?;
        let value = serde_json::to_string(vault).map_err(|_| CredentialError::Invalid)?;
        if value.len() > MAX_ENCODED_BYTES {
            return Err(CredentialError::Invalid);
        }
        self.backend.write(&value)
    }
    fn cleanup(&self, vault: &mut Vault) -> bool {
        let before = vault.pending_deletions.len();
        vault
            .pending_deletions
            .retain(|key| self.backend.delete_named(key).is_err());
        if vault.pending_deletions.len() != before && self.write_vault(vault).is_err() {
            return false;
        }
        vault.pending_deletions.is_empty()
    }
}

fn encode_credential(credential: &Credential) -> Result<String, CredentialError> {
    if !credential.valid() {
        return Err(CredentialError::Invalid);
    }
    let value = serde_json::to_string(&Record {
        version: VERSION,
        credential: credential.clone(),
    })
    .map_err(|_| CredentialError::Invalid)?;
    if value.len() > MAX_ENCODED_BYTES {
        return Err(CredentialError::Invalid);
    }
    Ok(value)
}
fn decode_credential(value: &str) -> Result<Credential, CredentialError> {
    if value.len() > MAX_ENCODED_BYTES {
        return Err(CredentialError::Invalid);
    }
    let record: Record = serde_json::from_str(value).map_err(|_| CredentialError::Invalid)?;
    if record.version != VERSION || !record.credential.valid() {
        return Err(CredentialError::Invalid);
    }
    Ok(record.credential)
}
fn valid_key(key: &str) -> bool {
    let Some((uid, random)) = key
        .strip_prefix("account-")
        .and_then(|rest| rest.split_once('-'))
    else {
        return false;
    };
    uid.parse::<i64>().is_ok_and(|id| id > 0)
        && random.len() == 32
        && random
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
fn validate_vault(vault: &Vault) -> Result<(), CredentialError> {
    if vault.version != VAULT_VERSION
        || vault.accounts.len() > MAX_ACCOUNTS
        || vault.pending_deletions.len() > 16
    {
        return Err(CredentialError::Invalid);
    }
    let valid_slot = |slot: &Slot| {
        slot.uid.parse::<i64>().is_ok_and(|id| id > 0)
            && valid_key(&slot.key)
            && slot.key.starts_with(&format!("account-{}-", slot.uid))
            && slot
                .nickname
                .as_ref()
                .is_none_or(|name| name.chars().count() <= 64)
    };
    if vault.active.as_ref().is_some_and(|slot| !valid_slot(slot))
        || vault.accounts.iter().any(|slot| !valid_slot(slot))
        || vault.pending_deletions.iter().any(|key| !valid_key(key))
    {
        return Err(CredentialError::Invalid);
    }
    let mut ids = std::collections::HashSet::new();
    if vault.accounts.iter().any(|slot| !ids.insert(&slot.uid)) {
        return Err(CredentialError::Invalid);
    }
    let live = live_keys(vault);
    if vault.pending_deletions.iter().any(|key| live.contains(key)) {
        return Err(CredentialError::Invalid);
    }
    Ok(())
}
fn live_keys(vault: &Vault) -> std::collections::HashSet<String> {
    vault
        .accounts
        .iter()
        .chain(vault.active.iter())
        .map(|slot| slot.key.clone())
        .collect()
}
fn mark_retired(vault: &mut Vault, before: std::collections::HashSet<String>) {
    let after = live_keys(vault);
    for key in before.difference(&after) {
        if !vault.pending_deletions.contains(key) {
            vault.pending_deletions.push(key.clone());
        }
    }
}

/// Windows Credential Manager or Linux Secret Service; no plaintext fallback.
pub struct OsSecretBackend;

#[cfg(any(windows, target_os = "linux"))]
impl OsSecretBackend {
    fn entry(service: &str, name: &str) -> Result<keyring::Entry, CredentialError> {
        keyring::Entry::new(service, name).map_err(|_| CredentialError::Unavailable)
    }

    fn read_entry(name: &str) -> Result<Option<String>, CredentialError> {
        let entry = Self::entry("bo_music", name)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => {
                // Migrate each secret independently so existing vault slots remain readable.
                let previous = Self::entry("bowen-music-rust", name)?;
                match previous.get_password() {
                    Ok(value) => {
                        entry
                            .set_password(&value)
                            .map_err(|_| CredentialError::Unavailable)?;
                        let _ = previous.delete_credential();
                        Ok(Some(value))
                    }
                    Err(keyring::Error::NoEntry) => Ok(None),
                    Err(_) => Err(CredentialError::Unavailable),
                }
            }
            Err(_) => Err(CredentialError::Unavailable),
        }
    }

    fn delete_entry(name: &str) -> Result<(), CredentialError> {
        // Delete both namespaces before allowing a removed credential to be forgotten.
        for service in ["bowen-music-rust", "bo_music"] {
            match Self::entry(service, name)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(_) => return Err(CredentialError::Unavailable),
            }
        }
        Ok(())
    }
}

#[cfg(any(windows, target_os = "linux"))]
impl SecretBackend for OsSecretBackend {
    fn read(&self) -> Result<Option<String>, CredentialError> {
        Self::read_entry("active-account")
    }

    fn write(&self, secret: &str) -> Result<(), CredentialError> {
        Self::entry("bo_music", "active-account")?
            .set_password(secret)
            .map_err(|_| CredentialError::Unavailable)
    }

    fn delete(&self) -> Result<(), CredentialError> {
        Self::delete_entry("active-account")
    }

    fn read_named(&self, name: &str) -> Result<Option<String>, CredentialError> {
        if !valid_key(name) {
            return Err(CredentialError::Invalid);
        }
        Self::read_entry(name)
    }

    fn write_named(&self, name: &str, secret: &str) -> Result<(), CredentialError> {
        if !valid_key(name) {
            return Err(CredentialError::Invalid);
        }
        Self::entry("bo_music", name)?
            .set_password(secret)
            .map_err(|_| CredentialError::Unavailable)
    }

    fn delete_named(&self, name: &str) -> Result<(), CredentialError> {
        if !valid_key(name) {
            return Err(CredentialError::Invalid);
        }
        Self::delete_entry(name)
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
impl SecretBackend for OsSecretBackend {
    fn read(&self) -> Result<Option<String>, CredentialError> {
        Err(CredentialError::Unavailable)
    }
    fn write(&self, _: &str) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }
    fn delete(&self) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod tests;
