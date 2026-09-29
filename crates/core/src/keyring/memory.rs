//! The keyring's in-memory adapter: what the self-check files secrets in.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::{Attrs, KeyringError, Secrets};

/// Secrets in a map, keyed by their exact attributes. It counts unlocks, and
/// [`MemorySecrets::locked`] makes one whose unlock is always refused and
/// whose every operation fails as a locked keyring's does.
#[derive(Default)]
pub struct MemorySecrets {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    items: BTreeMap<Attrs, (String, String)>,
    unlocks: usize,
    refusal: Option<String>,
}

impl MemorySecrets {
    pub fn locked(reason: &str) -> Self {
        let secrets = MemorySecrets::default();
        secrets.state().refusal = Some(reason.to_owned());
        secrets
    }

    pub fn unlock_count(&self) -> usize {
        self.state().unlocks
    }

    /// Every secret: (attributes, item label, secret).
    pub fn items(&self) -> Vec<(Attrs, String, String)> {
        self.state().items.iter().map(|(a, (l, s))| (a.clone(), l.clone(), s.clone())).collect()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn usable(&self) -> Result<MutexGuard<'_, State>, KeyringError> {
        let state = self.state();
        match &state.refusal {
            Some(reason) => Err(KeyringError::Locked(reason.clone())),
            None => Ok(state),
        }
    }
}

impl Secrets for MemorySecrets {
    fn lookup(&self, attrs: &Attrs) -> Result<Option<String>, KeyringError> {
        Ok(self.usable()?.items.get(attrs).map(|(_, s)| s.clone()))
    }

    fn store(&self, attrs: &Attrs, label: &str, secret: &str) -> Result<(), KeyringError> {
        self.usable()?.items.insert(attrs.clone(), (label.to_owned(), secret.to_owned()));
        Ok(())
    }

    fn clear(&self, attrs: &Attrs) -> Result<(), KeyringError> {
        self.usable()?.items.remove(attrs);
        Ok(())
    }

    fn unlock(&self) -> Result<(), KeyringError> {
        let mut state = self.state();
        state.unlocks += 1;
        match &state.refusal {
            Some(reason) => Err(KeyringError::Locked(reason.clone())),
            None => Ok(()),
        }
    }
}

/// So a test can keep a handle on the adapter it gave the keyring.
impl<S: Secrets> Secrets for Arc<S> {
    fn lookup(&self, attrs: &Attrs) -> Result<Option<String>, KeyringError> {
        (**self).lookup(attrs)
    }

    fn store(&self, attrs: &Attrs, label: &str, secret: &str) -> Result<(), KeyringError> {
        (**self).store(attrs, label, secret)
    }

    fn clear(&self, attrs: &Attrs) -> Result<(), KeyringError> {
        (**self).clear(attrs)
    }

    fn unlock(&self) -> Result<(), KeyringError> {
        (**self).unlock()
    }
}
