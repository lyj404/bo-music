use std::sync::{Arc, Mutex};

pub const ANONYMOUS_UID: &str = "-1";

/// An atomic identity snapshot used for both query and headers of a request.
#[derive(Clone)]
pub struct SessionSnapshot {
    pub uid: String,
    pub token: String,
    pub revision: u64,
}

impl std::fmt::Debug for SessionSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionSnapshot")
            .field("revision", &self.revision)
            .field("credentials", &"[redacted]")
            .finish()
    }
}

impl SessionSnapshot {
    pub fn is_authenticated(&self) -> bool {
        !self.uid.is_empty() && self.uid != ANONYMOUS_UID
    }
}

#[derive(Clone)]
pub struct Session(Arc<Mutex<SessionSnapshot>>);

/// A credential already validated by the secure account store.
pub struct StoredIdentity {
    uid: String,
    token: String,
}

impl StoredIdentity {
    pub fn new(uid: String, token: String) -> Option<Self> {
        (uid.parse::<i64>().is_ok_and(|value| value > 0)
            && !token.is_empty()
            && !token.chars().any(char::is_control))
        .then_some(Self { uid, token })
    }
}

impl Default for Session {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(SessionSnapshot {
            uid: ANONYMOUS_UID.into(),
            token: String::new(),
            revision: 0,
        })))
    }
}

impl Session {
    pub(crate) fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        self.0.lock().expect("session lock poisoned").clone()
    }

    /// Runs a short synchronous action while the identity revision remains locked.
    /// The action must not call back into this session or perform network I/O.
    pub fn with_revision<T>(&self, revision: u64, action: impl FnOnce() -> T) -> Option<T> {
        let state = self.0.lock().expect("session lock poisoned");
        if state.revision != revision {
            return None;
        }
        Some(action())
    }

    /// Serialize a blocking credential-store operation with identity changes.
    /// Call only on a dedicated blocking worker; the action must not use this session.
    pub fn with_revision_for_store<T>(
        &self,
        revision: u64,
        action: impl FnOnce() -> T,
    ) -> Option<T> {
        let state = self.0.lock().expect("session lock poisoned");
        if state.revision != revision {
            return None;
        }
        Some(action())
    }

    pub fn set(&self, uid: String, token: String) {
        let mut state = self.0.lock().expect("session lock poisoned");
        state.uid = uid;
        state.token = token;
        state.revision += 1;
    }

    /// Installs a login result only while the account observed at login is current.
    pub(crate) fn set_if_revision(&self, revision: u64, uid: String, token: String) -> bool {
        let mut state = self.0.lock().expect("session lock poisoned");
        if state.revision != revision {
            return false;
        }
        state.uid = uid;
        state.token = token;
        state.revision += 1;
        true
    }

    /// Restore a previously validated, securely stored credential only if the
    /// account has not changed while it was being loaded.
    pub fn restore_if_revision(&self, revision: u64, uid: String, token: String) -> bool {
        if !uid.parse::<i64>().is_ok_and(|value| value > 0) || token.is_empty() {
            return false;
        }
        self.set_if_revision(revision, uid, token)
    }

    /// Persist the selected stored account and install it under one revision guard.
    /// The callback may access the keyring and must run on a blocking thread.
    pub fn switch_after<E>(
        &self,
        revision: u64,
        action: impl FnOnce() -> Result<StoredIdentity, E>,
    ) -> Option<Result<(), E>> {
        let mut state = self.0.lock().expect("session lock poisoned");
        if state.revision != revision {
            return None;
        }
        Some(action().map(|identity| {
            state.uid = identity.uid;
            state.token = identity.token;
            state.revision = state.revision.wrapping_add(1);
        }))
    }

    /// Persist and install a validated credential as one revision-guarded transaction.
    /// The callback must not re-enter this session. It may block on a system keyring,
    /// so callers must execute it on a dedicated blocking thread.
    pub(crate) fn set_if_revision_after<E>(
        &self,
        revision: u64,
        uid: String,
        token: String,
        persist: impl FnOnce(&str, &str) -> Result<(), E>,
    ) -> Option<Result<(), E>> {
        let mut state = self.0.lock().expect("session lock poisoned");
        if state.revision != revision {
            return None;
        }
        Some(persist(&uid, &token).map(|()| {
            state.uid = uid;
            state.token = token;
            state.revision += 1;
        }))
    }

    pub fn clear(&self) {
        self.clear_if_revision(None);
    }

    /// Delete the persistent active credential before clearing this session.
    /// Run on a dedicated blocking thread; `delete` must not re-enter the session.
    pub fn clear_after<E>(
        &self,
        revision: u64,
        delete: impl FnOnce() -> Result<(), E>,
    ) -> Option<Result<(), E>> {
        let mut state = self.0.lock().expect("session lock poisoned");
        if state.revision != revision {
            return None;
        }
        Some(delete().map(|()| {
            state.uid = ANONYMOUS_UID.into();
            state.token.clear();
            // An explicit sign-out must also invalidate a restore already in flight.
            state.revision += 1;
        }))
    }

    /// An old unauthorized response must not sign out a newly selected account.
    pub fn clear_if_revision(&self, revision: Option<u64>) -> bool {
        let mut state = self.0.lock().expect("session lock poisoned");
        if revision.is_some_and(|value| value != state.revision)
            || (state.uid == ANONYMOUS_UID && state.token.is_empty())
        {
            return false;
        }
        state.uid = ANONYMOUS_UID.into();
        state.token.clear();
        state.revision += 1;
        true
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
