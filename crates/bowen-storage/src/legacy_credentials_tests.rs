use super::*;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use windows_sys::Win32::Security::Cryptography::CryptProtectData;

#[derive(Default)]
struct Memory {
    primary: Option<String>,
    named: HashMap<String, String>,
    fail_write: bool,
}
#[derive(Clone, Default)]
struct Backend(Arc<Mutex<Memory>>);
impl SecretBackend for Backend {
    fn read(&self) -> Result<Option<String>, CredentialError> {
        Ok(self.0.lock().unwrap().primary.clone())
    }
    fn write(&self, value: &str) -> Result<(), CredentialError> {
        let mut memory = self.0.lock().unwrap();
        if memory.fail_write {
            return Err(CredentialError::Unavailable);
        }
        memory.primary = Some(value.into());
        Ok(())
    }
    fn delete(&self) -> Result<(), CredentialError> {
        self.0.lock().unwrap().primary = None;
        Ok(())
    }
    fn read_named(&self, key: &str) -> Result<Option<String>, CredentialError> {
        Ok(self.0.lock().unwrap().named.get(key).cloned())
    }
    fn write_named(&self, key: &str, value: &str) -> Result<(), CredentialError> {
        self.0
            .lock()
            .unwrap()
            .named
            .insert(key.into(), value.into());
        Ok(())
    }
    fn delete_named(&self, key: &str) -> Result<(), CredentialError> {
        self.0.lock().unwrap().named.remove(key);
        Ok(())
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: std::path::PathBuf,
    source: std::path::PathBuf,
    target: DataPaths,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "bowen-dpapi-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let source = root.join("old");
        let target = DataPaths::new(root.join("new"));
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(target.root()).unwrap();
        Self {
            root,
            source,
            target,
        }
    }
    fn put(&self, name: &str, json: &str, entropy: &[u8]) -> Vec<u8> {
        let bytes = protect(json.as_bytes(), entropy);
        fs::write(self.source.join(name), &bytes).unwrap();
        bytes
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn protect(data: &[u8], entropy: &[u8]) -> Vec<u8> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
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
    // SAFETY: valid buffers for the duration of the call; the guard releases the output.
    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            &salt,
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output.0,
        )
    };
    assert_ne!(ok, 0);
    // SAFETY: DPAPI returned cbData initialized bytes.
    unsafe { std::slice::from_raw_parts(output.0.pbData, output.0.cbData as usize) }.to_vec()
}
const ACTIVE: &str = r#"{"Uid":"101","Token":"synthetic-active","Nickname":"Alice"}"#;
const ACCOUNTS: &str = r#"{"Accounts":[{"Credential":{"Uid":"202","Token":"synthetic-second","Nickname":"Bob"},"LastUsedAt":"2025-01-01T00:00:00Z"},{"Credential":{"Uid":"101","Token":"synthetic-old","Nickname":"Alice"},"LastUsedAt":"2024-01-01T00:00:00Z"}]}"#;

#[test]
fn imports_dpapi_files_and_preserves_ciphertext_and_active_token() {
    let fixture = Fixture::new();
    let original = fixture.put("session.dat", ACTIVE, b"Bodian.Session.v1");
    fixture.put("remembered-accounts.dat", ACCOUNTS, b"Bodian.Accounts.v1");
    let store = CredentialStore::new(Backend::default());
    assert!(import_once(&fixture.source, &fixture.target, &store).unwrap());
    let active = store.load().unwrap().unwrap();
    assert_eq!(active.uid, "101");
    assert_eq!(active.token, "synthetic-active");
    let accounts = store.remembered().unwrap();
    assert_eq!(
        accounts.iter().map(|x| x.uid.as_str()).collect::<Vec<_>>(),
        ["202", "101"]
    );
    assert_eq!(
        fs::read(fixture.source.join("session.dat")).unwrap(),
        original
    );
    assert_eq!(
        fs::read(
            fixture
                .target
                .root()
                .join("legacy-backup/credentials/session.dat")
        )
        .unwrap(),
        original
    );
    assert!(!import_once(&fixture.source, &fixture.target, &store).unwrap());
}

#[test]
fn signed_out_and_forgotten_states_are_preserved() {
    let fixture = Fixture::new();
    fixture.put("remembered-accounts.dat", ACCOUNTS, b"Bodian.Accounts.v1");
    let store = CredentialStore::new(Backend::default());
    assert!(import_once(&fixture.source, &fixture.target, &store).unwrap());
    assert!(store.load().unwrap().is_none());
    assert_eq!(store.remembered().unwrap().len(), 2);
    let other = Fixture::new();
    other.put("session.dat", ACTIVE, b"Bodian.Session.v1");
    let store = CredentialStore::new(Backend::default());
    assert!(import_once(&other.source, &other.target, &store).unwrap());
    assert!(store.load().unwrap().is_some());
    assert!(store.remembered().unwrap().is_empty());
}

#[test]
fn failure_retries_and_existing_vault_is_never_overwritten() {
    let fixture = Fixture::new();
    fixture.put("session.dat", ACTIVE, b"wrong entropy");
    let backend = Backend::default();
    let store = CredentialStore::new(backend.clone());
    assert!(import_once(&fixture.source, &fixture.target, &store).is_err());
    assert!(!fixture.target.root().join(MARKER).exists());
    assert!(store.load().unwrap().is_none());
    let other = Fixture::new();
    other.put("session.dat", ACTIVE, b"Bodian.Session.v1");
    backend.0.lock().unwrap().fail_write = true;
    assert!(import_once(&other.source, &other.target, &store).is_err());
    assert!(!other.target.root().join(MARKER).exists());
    assert!(backend.0.lock().unwrap().named.is_empty());
    backend.0.lock().unwrap().fail_write = false;
    assert!(import_once(&other.source, &other.target, &store).unwrap());
    store.clear().unwrap();
    let third = Fixture::new();
    third.put("session.dat", ACTIVE, b"Bodian.Session.v1");
    assert!(!import_once(&third.source, &third.target, &store).unwrap());
    assert!(store.load().unwrap().is_none());
}
