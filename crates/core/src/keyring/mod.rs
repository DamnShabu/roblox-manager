//! Every secret the manager keeps: each account's session cookie, and the
//! sessions it gives Cordial profiles. They live in the Secret Service
//! (gnome-keyring), never in a file.

mod dbus;
mod memory;

use std::collections::BTreeMap;

use crate::types::{Cookie, Label};

pub use dbus::DbusSecrets;
pub use memory::MemorySecrets;

/// A secret's attributes: what it is filed, and found, under.
pub type Attrs = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyringError {
    #[error("the keyring stayed locked: {0}")]
    Locked(String),
    #[error("no cookie in the keyring for '{0}' -- use 'Sign in again' on the account")]
    NoCookie(Label),
    #[error("the keyring failed: {0}")]
    Service(String),
}

/// Where secrets are kept. Adapters: [`DbusSecrets`] (the desktop keyring)
/// and [`MemorySecrets`] (a map, for tests).
pub trait Secrets: Send + Sync {
    /// The secret filed under `attrs`, if any.
    fn lookup(&self, attrs: &Attrs) -> Result<Option<String>, KeyringError>;
    /// File `secret` under exactly `attrs`, replacing whatever matched them.
    fn store(&self, attrs: &Attrs, label: &str, secret: &str) -> Result<(), KeyringError>;
    /// Remove everything filed under `attrs`.
    fn clear(&self, attrs: &Attrs) -> Result<(), KeyringError>;
    /// Make the keyring usable, prompting the user if it is locked. Blocks the
    /// calling thread until the prompt is answered.
    fn unlock(&self) -> Result<(), KeyringError>;
}

/// The keyring as the manager uses it. Every read and write unlocks first,
/// which may put up the desktop's password prompt and wait for it -- so call
/// it from a worker thread, never the UI's.
pub struct Keyring {
    secrets: Box<dyn Secrets>,
}

impl Keyring {
    pub fn new(secrets: Box<dyn Secrets>) -> Self {
        Keyring { secrets }
    }

    pub fn get(&self, attrs: &Attrs) -> Result<Option<String>, KeyringError> {
        self.secrets.unlock()?;
        self.secrets.lookup(attrs)
    }

    pub fn put(&self, attrs: &Attrs, label: &str, secret: &str) -> Result<(), KeyringError> {
        self.secrets.unlock()?;
        self.secrets.store(attrs, label, secret)
    }

    pub fn delete(&self, attrs: &Attrs) -> Result<(), KeyringError> {
        self.secrets.unlock()?;
        self.secrets.clear(attrs)
    }

    /// [`Keyring::delete`] as a courtesy: it never prompts, and a locked
    /// keyring simply keeps the entry. For tidying up nobody asked for.
    pub fn forget(&self, attrs: &Attrs) {
        // Deliberately unchecked: the entry is stale either way, and nobody
        // is waiting on this to succeed.
        let _ = self.secrets.clear(attrs);
    }

    /// The account's session. A missing one is an error that names the fix.
    pub fn cookie(&self, label: &Label) -> Result<Cookie, KeyringError> {
        let found = self.get(&account_attrs(label))?.unwrap_or_default();
        let value = found.trim();
        if value.is_empty() {
            return Err(KeyringError::NoCookie(label.clone()));
        }
        Ok(Cookie::new(value))
    }

    pub fn set_cookie(&self, label: &Label, cookie: &Cookie) -> Result<(), KeyringError> {
        self.put(&account_attrs(label), &format!("rbxmgr {label}"), cookie.expose())
    }

    pub fn drop_cookie(&self, label: &Label) -> Result<(), KeyringError> {
        self.delete(&account_attrs(label))
    }

    /// Re-file an account's cookie under its new label.
    pub fn move_cookie(&self, old: &Label, new: &Label) -> Result<(), KeyringError> {
        self.set_cookie(new, &self.cookie(old)?)?;
        self.drop_cookie(old)
    }
}

/// Where an account's own cookie is filed.
fn account_attrs(label: &Label) -> Attrs {
    [("app", "rbxmgr"), ("account", label.as_str())]
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn label(s: &str) -> Label {
        Label::parse(s).unwrap()
    }

    fn keyring() -> (Keyring, Arc<MemorySecrets>) {
        let mem = Arc::new(MemorySecrets::default());
        (Keyring::new(Box::new(Arc::clone(&mem))), mem)
    }

    #[test]
    fn a_cookie_round_trips_stripped() {
        let (k, _) = keyring();
        k.set_cookie(&label("x"), &Cookie::new("  cookie-value \n")).unwrap();
        assert_eq!(k.cookie(&label("x")).unwrap().expose(), "cookie-value");
    }

    #[test]
    fn an_accounts_cookie_is_filed_under_app_and_account() {
        let (k, mem) = keyring();
        k.set_cookie(&label("alt 1"), &Cookie::new("c")).unwrap();
        let attrs: Attrs = [("app", "rbxmgr"), ("account", "alt 1")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .into();
        assert_eq!(mem.items(), vec![(attrs, "rbxmgr alt 1".to_string(), "c".to_string())]);
    }

    #[test]
    fn every_read_and_write_unlocks_first() {
        let (k, mem) = keyring();
        let l = label("x");
        k.set_cookie(&l, &Cookie::new("c")).unwrap();
        k.cookie(&l).unwrap();
        k.put(&Attrs::new(), "l", "s").unwrap();
        k.get(&Attrs::new()).unwrap();
        k.delete(&Attrs::new()).unwrap();
        k.drop_cookie(&l).unwrap();
        assert_eq!(mem.unlock_count(), 6);
    }

    #[test]
    fn a_missing_cookie_names_the_fix() {
        let (k, _) = keyring();
        let err = k.cookie(&label("x")).unwrap_err();
        assert_eq!(err, KeyringError::NoCookie(label("x")));
        assert!(err.to_string().contains("Sign in again"), "{err}");
    }

    #[test]
    fn a_refused_unlock_is_an_error_everywhere_but_forget() {
        let mem = Arc::new(MemorySecrets::locked("the prompt was dismissed"));
        let k = Keyring::new(Box::new(Arc::clone(&mem)));
        let l = label("x");
        let refused = KeyringError::Locked("the prompt was dismissed".into());
        assert_eq!(k.cookie(&l).unwrap_err(), refused);
        assert_eq!(k.set_cookie(&l, &Cookie::new("c")).unwrap_err(), refused);
        assert_eq!(k.drop_cookie(&l).unwrap_err(), refused);
        assert_eq!(k.put(&Attrs::new(), "l", "s").unwrap_err(), refused);
        assert_eq!(k.delete(&Attrs::new()).unwrap_err(), refused);
        let before = mem.unlock_count();
        k.forget(&Attrs::new());
        assert_eq!(mem.unlock_count(), before, "forget must never prompt");
    }

    #[test]
    fn moving_a_cookie_leaves_only_the_new_label() {
        let (k, mem) = keyring();
        k.set_cookie(&label("old"), &Cookie::new("c")).unwrap();
        k.move_cookie(&label("old"), &label("new ✓")).unwrap();
        assert_eq!(k.cookie(&label("new ✓")).unwrap().expose(), "c");
        assert!(matches!(k.cookie(&label("old")), Err(KeyringError::NoCookie(_))));
        assert_eq!(mem.items().len(), 1);
    }
}
