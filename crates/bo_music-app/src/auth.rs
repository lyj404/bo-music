//! UI-independent account persistence. Execute these blocking operations off
//! the UI and async runtime threads; system keyrings can require user interaction.
use bo_music_api::{
    login::ValidatedCredential,
    session::{Session, StoredIdentity},
};
use bo_music_storage::credentials::{Credential, CredentialError, CredentialStore, SecretBackend};

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("account changed while the operation was in progress")]
    SessionChanged,
    #[error(transparent)]
    Storage(#[from] CredentialError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Account {
    pub uid: String,
    pub nickname: Option<String>,
}

/// Store the validated login before exposing it as the current account.
pub fn install<B: SecretBackend>(
    credential: ValidatedCredential,
    store: &CredentialStore<B>,
) -> Result<Account, AuthError> {
    let account = Account {
        uid: credential.uid().to_string(),
        nickname: credential.nickname.clone(),
    };
    credential
        .adopt_with(|uid, token, nickname| {
            store.save(Credential {
                uid: uid.into(),
                token: token.into(),
                nickname: nickname.map(str::to_owned),
            })
        })
        .map_err(|_| AuthError::SessionChanged)??;
    Ok(account)
}

/// Restore only if no intervening sign-in or sign-out changed the session.
pub fn restore<B: SecretBackend>(
    session: &Session,
    store: &CredentialStore<B>,
) -> Result<Option<Account>, AuthError> {
    let snapshot = session.snapshot();
    if snapshot.is_authenticated() {
        return Err(AuthError::SessionChanged);
    }
    let revision = snapshot.revision;
    let Some(credential) = store.load()? else {
        return Ok(None);
    };
    let account = Account {
        uid: credential.uid.clone(),
        nickname: credential.nickname.clone(),
    };
    if !session.restore_if_revision(revision, credential.uid, credential.token) {
        return Err(AuthError::SessionChanged);
    }
    Ok(Some(account))
}

/// Remove the persisted active credential before clearing the in-memory session.
pub fn sign_out<B: SecretBackend>(
    session: &Session,
    store: &CredentialStore<B>,
) -> Result<(), AuthError> {
    let revision = session.snapshot().revision;
    session
        .clear_after(revision, || store.clear())
        .ok_or(AuthError::SessionChanged)??;
    Ok(())
}

pub fn switch<B: SecretBackend>(
    session: &Session,
    store: &CredentialStore<B>,
    uid: &str,
) -> Result<Account, AuthError> {
    let snapshot = session.snapshot();
    let mut account = None;
    session
        .switch_after(snapshot.revision, || {
            let credential = store.switch(uid)?;
            account = Some(Account {
                uid: credential.uid.clone(),
                nickname: credential.nickname.clone(),
            });
            StoredIdentity::new(credential.uid, credential.token).ok_or(CredentialError::Invalid)
        })
        .ok_or(AuthError::SessionChanged)??;
    account.ok_or(AuthError::Storage(CredentialError::Invalid))
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
