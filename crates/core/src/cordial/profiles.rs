//! Every account's Cordial profile, and the client that plays in it: where
//! the profile lives, the session it is given, its low-power flags, starting
//! its client, and which clients are up.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value};

use super::build::Build;
use super::process::{ProcessView, Runner, last_line};
use super::{CordialError, clients, engine, session};
use crate::keyring::{Attrs, Keyring};
use crate::macros::{nested, relay};
use crate::paths::Paths;
use crate::types::{Cookie, Profile, User, UserId};

/// A client still up this long after starting got past the checks that end
/// one at once (a profile already in use, a missing engine, a bad link).
pub const STARTUP_CHECK: Duration = Duration::from_secs(5);

/// The two sessions the manager gives a profile, filed in the keyring.
pub const SECRET_KINDS: [&str; 2] = ["identity", "cookies"];

/// How one client is started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClientOpts {
    /// In a cage of its own, where macros can reach it.
    pub nested: bool,
    /// Throttled, FIFO-paced, niced, with frame and thread caps.
    pub low_power: bool,
}

/// Worker threads only: seeding and clearing go through the keyring, and
/// finding clients runs pgrep.
pub struct CordialProfiles {
    keyring: Arc<Keyring>,
    paths: Paths,
    runner: Arc<dyn Runner>,
    /// Where pgrep and kill see the clients.
    view: ProcessView,
    sleep: Arc<dyn Fn(Duration) + Send + Sync>,
    /// The program a macro-ready client runs behind, so it can be recorded.
    relay: Option<PathBuf>,
    /// The lock that keeps cages off the display they open on, held from the
    /// first macro-ready launch on. See [`nested::hold_parent_display`].
    parent_display: Mutex<Option<File>>,
}

impl CordialProfiles {
    pub fn new(
        keyring: Arc<Keyring>,
        paths: &Paths,
        runner: Arc<dyn Runner>,
        view: ProcessView,
        sleep: Arc<dyn Fn(Duration) + Send + Sync>,
    ) -> Self {
        CordialProfiles {
            keyring,
            paths: paths.clone(),
            runner,
            view,
            sleep,
            relay: None,
            parent_display: Mutex::default(),
        }
    }

    /// Run macro-ready clients behind `relay` (the app itself), where a
    /// recording can hear their windows. None runs them straight in cage.
    pub fn with_relay(mut self, relay: Option<PathBuf>) -> Self {
        self.relay = relay;
        self
    }

    pub fn path(&self, profile: &Profile) -> PathBuf {
        self.paths.cordial_profiles().join(profile.as_str())
    }

    /// The keyring attributes Cordial files a profile's `kind` under, keyed
    /// by the profile's full path.
    pub fn secret_attrs(&self, profile: &Profile, kind: &str) -> Attrs {
        secret_attrs_at(&self.path(profile), kind)
    }

    /// Make the account's profile routable. Returns its name. Rewritten on
    /// every launch rather than once: the cookie has just been checked with
    /// Roblox, so it is known good. The keyring is the only place the session
    /// goes -- never a file in the profile.
    pub fn seed(&self, user: &User, cookie: &Cookie) -> Result<Profile, CordialError> {
        let profile = Profile::of(user.id);
        fs::create_dir_all(self.path(&profile))
            .map_err(|e| io(&format!("could not create Cordial profile {profile}"), e))?;
        let bodies = [session::identity_json(user), session::cookie_store(cookie)];
        for (kind, body) in SECRET_KINDS.into_iter().zip(bodies) {
            self.keyring.put(
                &self.secret_attrs(&profile, kind),
                &format!("Cordial: Roblox {kind} for profile \"{profile}\""),
                &session::encode(&body),
            )?;
        }
        Ok(profile)
    }

    /// Drop the session and identity the manager gave an account's profile.
    /// The profile directory stays; it holds no credential.
    pub fn clear(&self, user: UserId) -> Result<(), CordialError> {
        let profile = Profile::of(user);
        for kind in SECRET_KINDS {
            self.keyring.delete(&self.secret_attrs(&profile, kind))?;
        }
        Ok(())
    }

    /// Bring the profile's flags.json in line with its low-power switch,
    /// leaving the file untouched when nothing changes, and alone when it
    /// does not parse (Cordial reports that itself).
    pub fn set_low_power(&self, profile: &Profile, on: bool) -> Result<(), CordialError> {
        let path = self.path(profile).join("flags.json");
        let flags: Map<String, Value> = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(flags) => flags,
                Err(_) => return Ok(()),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
            Err(e) => return Err(io(&format!("could not read {}", path.display()), e)),
        };
        let wanted = engine::low_power_flags(&flags, on);
        if wanted != flags {
            crate::json_file::write(&path, &wanted)
                .map_err(|e| io(&format!("could not write {}", path.display()), e))?;
        }
        Ok(())
    }

    /// Start one account's client and check it survives its first seconds.
    /// Its output goes to `<cache>/logs/<profile>.log` -- the only account of
    /// why a client ended -- with the previous launch's kept as `.log.1`.
    pub fn launch(
        &self,
        profile: &Profile,
        url: Option<&str>,
        build: &Build,
        opts: ClientOpts,
    ) -> Result<(), CordialError> {
        self.set_low_power(profile, opts.low_power)?;
        let mut env = engine::env(&engine::load_settings(&self.paths.cordial_shell_json()));
        if opts.low_power {
            env = engine::with_low_power(env);
        }
        let log_path = self.rotate_log(profile)?;
        let log = File::create(&log_path)
            .map_err(|e| io(&format!("could not open {}", log_path.display()), e))?;
        let program = super::stacked::engine_program(&self.paths);
        let mut argv = engine::client_argv(&program, profile, url, build);
        if opts.low_power {
            argv.splice(0..0, ["nice", "-n", "10"].map(String::from));
        }
        if opts.nested {
            self.hold_parent_display()?;
            let display = nested::display_file(self.paths.runtime_dir(), profile);
            if let Some(relay) = &self.relay {
                argv = relay::argv(relay, &display, &argv);
            }
            argv = nested::cage_argv(&display, &argv);
        }
        let mut child = self.runner.spawn(&argv, log, &env)?;
        (self.sleep)(STARTUP_CHECK);
        match child.exited()? {
            None => Ok(()),
            Some(status) => {
                let tail = read_tail(&log_path);
                Err(CordialError::ExitedAtOnce {
                    reason: if tail.is_empty() { format!("exit status {status}") } else { tail },
                    log: log_path.display().to_string(),
                })
            }
        }
    }

    /// Lock the display cages open on before one starts, unless it already
    /// is.
    fn hold_parent_display(&self) -> Result<(), CordialError> {
        let mut held = self
            .parent_display
            .lock()
            .map_err(|_| CordialError::Process("the display lock was poisoned".into()))?;
        if held.is_none() {
            let (dir, display) = (self.paths.runtime_dir(), self.paths.wayland_display());
            *held = nested::hold_parent_display(dir, display)
                .map_err(|e| io("could not lock the Wayland display", e))?;
        }
        Ok(())
    }

    /// The profile's log path, the previous launch's moved to `.log.1`.
    fn rotate_log(&self, profile: &Profile) -> Result<PathBuf, CordialError> {
        let logs = self.paths.logs();
        fs::create_dir_all(&logs)
            .map_err(|e| io(&format!("could not create {}", logs.display()), e))?;
        let path = logs.join(format!("{profile}.log"));
        if path.exists() {
            fs::rename(&path, logs.join(format!("{profile}.log.1")))
                .map_err(|e| io("could not keep the previous log", e))?;
        }
        Ok(path)
    }

    /// Every running Cordial client, as {pid: profile}, somebody playing
    /// from Cordial directly included. pgrep exits 1 when nothing matches,
    /// which is an answer, not an error.
    pub fn clients(&self) -> Result<BTreeMap<u32, Profile>, CordialError> {
        let argv = self.view.argv(&["pgrep", "-a", "-f", "cordial-run"]);
        let out = self.runner.run(&argv, Duration::from_secs(10))?;
        // 1 is "nothing matched"; anything past it is pgrep failing.
        if out.status > 1 || out.status < 0 {
            return Err(CordialError::Process(format!(
                "could not list the running clients: {}",
                last_line(&out.stderr)
            )));
        }
        Ok(clients::parse(&String::from_utf8_lossy(&out.stdout)))
    }

    /// The profiles a client has open.
    pub fn running(&self) -> Result<HashSet<Profile>, CordialError> {
        Ok(self.clients()?.into_values().collect())
    }

    /// SIGTERM every client running one of `which`; cordial-run ends its
    /// session cleanly on it. Returns how many were signalled. Clients of
    /// profiles no account owns are left alone.
    pub fn stop(&self, which: &HashSet<Profile>) -> Result<usize, CordialError> {
        let pids: Vec<String> = self
            .clients()?
            .into_iter()
            .filter(|(_, profile)| which.contains(profile))
            .map(|(pid, _)| pid.to_string())
            .collect();
        if !pids.is_empty() {
            let kill: Vec<&str> =
                std::iter::once("kill").chain(pids.iter().map(String::as_str)).collect();
            let argv = self.view.argv(&kill);
            self.runner.run(&argv, Duration::from_secs(10))?;
        }
        Ok(pids.len())
    }

    pub(super) fn keyring(&self) -> &Keyring {
        &self.keyring
    }

    pub(super) fn paths(&self) -> &Paths {
        &self.paths
    }
}

pub(super) fn secret_attrs_at(profile_dir: &Path, kind: &str) -> Attrs {
    [
        ("xdg:schema", "org.cordial.Session"),
        ("application", "cordial"),
        ("profile", &profile_dir.display().to_string()),
        ("store", kind),
    ]
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .into()
}

/// The last line of up to the final 4 KiB of a log.
fn read_tail(path: &Path) -> String {
    let mut bytes = Vec::new();
    if File::open(path).and_then(|mut f| f.read_to_end(&mut bytes)).is_err() {
        return String::new();
    }
    last_line(&bytes[bytes.len().saturating_sub(4096)..])
}

pub(super) fn io(context: &str, e: std::io::Error) -> CordialError {
    CordialError::Io(format!("{context}: {e}"))
}

#[cfg(test)]
mod tests;
