//! The desktop keyring, over the Secret Service D-Bus interface -- the one
//! libsecret speaks outside a sandbox, and the one Cordial reads the sessions
//! this app gives it from.
//!
//! The "plain" session algorithm: secrets cross the session bus unencrypted,
//! which only this user's processes can reach -- the same trade Cordial makes.

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use super::{Attrs, KeyringError, Secrets};

const BUS: &str = "org.freedesktop.secrets";
const SERVICE_PATH: &str = "/org/freedesktop/secrets";
const SERVICE: &str = "org.freedesktop.Secret.Service";
const COLLECTION: &str = "org.freedesktop.Secret.Collection";
const ITEM: &str = "org.freedesktop.Secret.Item";
const PROMPT: &str = "org.freedesktop.Secret.Prompt";

/// How long an unlock prompt may go unanswered before the operation that
/// needed it gives up.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(180);

/// A secret as the Secret Service passes it: (session, parameters, value,
/// content type).
type Secret = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

/// The Secret Service on the session bus.
pub struct DbusSecrets {
    conn: Connection,
}

impl DbusSecrets {
    pub fn connect() -> Result<Self, KeyringError> {
        let conn = Connection::session().map_err(service)?;
        Ok(DbusSecrets { conn })
    }

    fn proxy<'a>(
        &self,
        path: impl Into<ObjectPath<'a>>,
        iface: &'a str,
    ) -> Result<Proxy<'a>, KeyringError> {
        Proxy::new(&self.conn, BUS, path.into(), iface).map_err(service)
    }

    fn service(&self) -> Result<Proxy<'static>, KeyringError> {
        Proxy::new(&self.conn, BUS, SERVICE_PATH, SERVICE).map_err(service)
    }

    fn default_collection(&self) -> Result<OwnedObjectPath, KeyringError> {
        let path: OwnedObjectPath = self
            .service()?
            .call("ReadAlias", &("default",))
            .map_err(service)?;
        if path.as_str() == "/" {
            return Err(KeyringError::Service(
                "there is no default keyring collection".into(),
            ));
        }
        Ok(path)
    }

    fn session(&self) -> Result<OwnedObjectPath, KeyringError> {
        let (_, session): (OwnedValue, OwnedObjectPath) = self
            .service()?
            .call("OpenSession", &("plain", Value::from("")))
            .map_err(service)?;
        Ok(session)
    }

    /// Unlocked items carrying at least `attrs`: the service matches a
    /// subset, so an entry secret-tool wrote with an extra xdg:schema is
    /// found too.
    fn search(&self, attrs: &Attrs) -> Result<Vec<OwnedObjectPath>, KeyringError> {
        let (unlocked, _locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) = self
            .service()?
            .call("SearchItems", &(as_dict(attrs),))
            .map_err(service)?;
        Ok(unlocked)
    }

    /// Run a prompt the service handed back, and wait for its answer.
    fn prompt(&self, path: &OwnedObjectPath) -> Result<(), KeyringError> {
        let prompt =
            Proxy::new(&self.conn, BUS, path.clone().into_inner(), PROMPT).map_err(service)?;
        // Subscribe before prompting: the answer may come back at once.
        let mut completed = prompt.receive_signal("Completed").map_err(service)?;
        let (tx, rx) = mpsc::channel();
        // The signal iterator has no timeout of its own, so it waits on a
        // thread of its own. An unanswered prompt leaves that thread parked
        // until the prompt is closed, which ends it.
        thread::spawn(move || {
            let dismissed = completed
                .next()
                .and_then(|m| m.body().deserialize::<(bool, OwnedValue)>().ok())
                .map(|(dismissed, _)| dismissed);
            let _ = tx.send(dismissed);
        });
        prompt.call::<_, _, ()>("Prompt", &("",)).map_err(service)?;
        match rx.recv_timeout(PROMPT_TIMEOUT) {
            Ok(Some(false)) => Ok(()),
            Ok(Some(true)) => Err(KeyringError::Locked(
                "the keyring unlock prompt was dismissed".into(),
            )),
            Ok(None) => Err(KeyringError::Locked(
                "the keyring prompt went away unanswered".into(),
            )),
            Err(_) => Err(KeyringError::Locked(format!(
                "the keyring did not unlock within {}s",
                PROMPT_TIMEOUT.as_secs()
            ))),
        }
    }
}

impl Secrets for DbusSecrets {
    fn lookup(&self, attrs: &Attrs) -> Result<Option<String>, KeyringError> {
        let Some(item) = self.search(attrs)?.into_iter().next() else {
            return Ok(None);
        };
        let secrets: HashMap<OwnedObjectPath, Secret> = self
            .service()?
            .call("GetSecrets", &(vec![item.clone()], self.session()?))
            .map_err(service)?;
        Ok(secrets
            .get(&item)
            .map(|(_, _, value, _)| String::from_utf8_lossy(value).into_owned()))
    }

    /// Existing matches are deleted first rather than left to CreateItem's
    /// replace: that only replaces an item with the *same* attribute set, and
    /// an entry secret-tool wrote carries an extra xdg:schema, so it would
    /// survive beside the new one as a stale second answer to every lookup.
    fn store(&self, attrs: &Attrs, label: &str, secret: &str) -> Result<(), KeyringError> {
        self.clear(attrs)?;
        let collection = self.default_collection()?;
        let props: HashMap<&str, Value> = HashMap::from([
            ("org.freedesktop.Secret.Item.Label", Value::from(label)),
            (
                "org.freedesktop.Secret.Item.Attributes",
                Value::from(as_dict(attrs)),
            ),
        ]);
        let value: Secret = (
            self.session()?,
            Vec::new(),
            secret.as_bytes().to_vec(),
            "text/plain; charset=utf8".to_owned(),
        );
        let (_item, prompt): (OwnedObjectPath, OwnedObjectPath) = self
            .proxy(collection.as_ref(), COLLECTION)?
            .call("CreateItem", &(props, value, true))
            .map_err(service)?;
        if prompt.as_str() != "/" {
            return Err(KeyringError::Locked(
                "the keyring is locked -- unlock it and try again".into(),
            ));
        }
        Ok(())
    }

    fn clear(&self, attrs: &Attrs) -> Result<(), KeyringError> {
        for item in self.search(attrs)? {
            let _prompt: OwnedObjectPath = self
                .proxy(item.as_ref(), ITEM)?
                .call("Delete", &())
                .map_err(service)?;
        }
        Ok(())
    }

    /// The login keyring is not necessarily unlocked: on a machine that
    /// autologins, PAM never sees a password, so every write fails until the
    /// user unlocks it. libsecret does not prompt on its own, so the unlock
    /// is asked for explicitly, which routes it through the desktop's prompter.
    fn unlock(&self) -> Result<(), KeyringError> {
        let collection = self.default_collection()?;
        let locked: bool = self
            .proxy(collection.as_ref(), COLLECTION)?
            .get_property("Locked")
            .map_err(service)?;
        if !locked {
            return Ok(());
        }
        let (unlocked, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) = self
            .service()?
            .call("Unlock", &(vec![collection],))
            .map_err(service)?;
        if !unlocked.is_empty() {
            return Ok(());
        }
        if prompt.as_str() == "/" {
            return Err(KeyringError::Locked(
                "the keyring stayed locked and offered no prompt".into(),
            ));
        }
        self.prompt(&prompt)
    }
}

fn as_dict(attrs: &Attrs) -> HashMap<&str, &str> {
    attrs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}

fn service(e: zbus::Error) -> KeyringError {
    KeyringError::Service(e.to_string())
}

#[cfg(test)]
mod live {
    use super::*;

    /// Read-only check against the real session keyring: unlock, then look
    /// up the account named by RBXMGR_LIVE_LABEL. Prints only whether a
    /// cookie was found. Run by hand:
    /// `RBXMGR_LIVE_LABEL=<label> cargo test -p rbxmgr-core live -- --ignored`
    #[test]
    #[ignore = "needs a session bus and a real keyring"]
    fn reads_an_accounts_cookie_from_the_desktop_keyring() {
        let label = std::env::var("RBXMGR_LIVE_LABEL").expect("set RBXMGR_LIVE_LABEL");
        let secrets = DbusSecrets::connect().unwrap();
        secrets.unlock().unwrap();
        let attrs: Attrs = [("app", "rbxmgr"), ("account", label.as_str())]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .into();
        let found = secrets.lookup(&attrs).unwrap();
        println!(
            "cookie found: {}",
            found.is_some_and(|c| !c.trim().is_empty())
        );
        assert!(
            secrets
                .lookup(&[("app".into(), "rbxmgr-no-such".into())].into())
                .unwrap()
                .is_none()
        );
    }
}
