//! The core, wired up once for the window.

use std::sync::Arc;

use rbxmgr_core::Paths;
use rbxmgr_core::cordial::{CordialProfiles, ProcessView, Runner, SystemRunner, roblox_build};
use rbxmgr_core::keyring::{Attrs, DbusSecrets, Keyring, KeyringError, Secrets};
use rbxmgr_core::launch::{BuildFn, Launcher, Pacing};
use rbxmgr_core::roblox::{HttpRoblox, IconCache, UreqTransport};

pub struct Services {
    pub paths: Paths,
    pub keyring: Arc<Keyring>,
    pub roblox: Arc<HttpRoblox>,
    pub runner: Arc<dyn Runner>,
    pub profiles: Arc<CordialProfiles>,
    pub launcher: Arc<Launcher>,
    pub icons: Arc<IconCache>,
    /// Accounts' headshots.
    pub avatars: Arc<IconCache>,
}

impl Services {
    pub fn new() -> Self {
        let paths = Paths::from_env();
        let secrets: Box<dyn Secrets> = match DbusSecrets::connect() {
            Ok(dbus) => Box::new(dbus),
            Err(e) => Box::new(Unavailable(e)),
        };
        let keyring = Arc::new(Keyring::new(secrets));
        let roblox = Arc::new(HttpRoblox::new(Arc::new(UreqTransport::default())));
        let runner: Arc<dyn Runner> = Arc::new(SystemRunner);
        let profiles = Arc::new(CordialProfiles::new(
            Arc::clone(&keyring),
            &paths,
            Arc::clone(&runner),
            // Inside a Flatpak, clients an earlier launch started are the host's.
            ProcessView::detect(),
            Arc::new(std::thread::sleep),
        ));
        let build_runner = Arc::clone(&runner);
        let build: BuildFn = Arc::new(move |log| roblox_build(&*build_runner, log, false));
        let launcher = Arc::new(Launcher::new(
            Arc::clone(&keyring),
            roblox.clone(),
            Arc::clone(&profiles),
            build,
            Pacing::default(),
        ));
        let icons = Arc::new(IconCache::new(paths.icons()));
        let avatars = Arc::new(IconCache::new(paths.avatars()));
        Services { paths, keyring, roblox, runner, profiles, launcher, icons, avatars }
    }
}

/// The keyring when the session bus could not be reached: every use says so.
struct Unavailable(KeyringError);

impl Secrets for Unavailable {
    fn lookup(&self, _: &Attrs) -> Result<Option<String>, KeyringError> {
        Err(self.error())
    }
    fn store(&self, _: &Attrs, _: &str, _: &str) -> Result<(), KeyringError> {
        Err(self.error())
    }
    fn clear(&self, _: &Attrs) -> Result<(), KeyringError> {
        Err(self.error())
    }
    fn unlock(&self) -> Result<(), KeyringError> {
        Err(self.error())
    }
}

impl Unavailable {
    fn error(&self) -> KeyringError {
        self.0.clone()
    }
}
