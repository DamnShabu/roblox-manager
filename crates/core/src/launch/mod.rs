//! Launching accounts: each into the target, or as a group behind a leader.
//! Per account the stored session is checked with Roblox, the account's
//! Cordial profile is given that session, and its client is started --
//! skipping one already running, and spacing sign-ins, since several
//! accounts signing in from one IP in the same second is what gets Roblox to
//! start refusing.

mod each;
mod group;

use std::cell::RefCell;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::cordial::{Build, ClientOpts, CordialError, CordialProfiles};
use crate::keyring::Keyring;
use crate::roblox::{Presence, Roblox, RobloxError, join_url};
use crate::stop::StopFlag;
use crate::types::{Label, PlaceId, Profile, ServerId, User, UserId};

/// How launches are spaced and how long a leader's server is waited for.
#[derive(Clone)]
pub struct Pacing {
    /// Between two sign-ins.
    pub stagger: Duration,
    /// How long to wait for the leader's server to show in presence.
    pub leader_timeout: Duration,
    /// How often to ask.
    pub poll: Duration,
    pub sleep: Arc<dyn Fn(Duration) + Send + Sync>,
}

impl Default for Pacing {
    fn default() -> Self {
        Pacing {
            stagger: Duration::from_secs(8),
            leader_timeout: Duration::from_secs(90),
            poll: Duration::from_secs(3),
            sleep: Arc::new(thread::sleep),
        }
    }
}

/// One account to launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchAccount {
    pub id: UserId,
    pub label: Label,
    pub opts: ClientOpts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Every account into the target: the place's own servers, or one server
    /// when the request names it.
    Each,
    /// The first account leads: it launches, its server is waited for, and
    /// the rest join it there.
    Group,
}

#[derive(Clone, Debug)]
pub struct LaunchRequest {
    /// In launch order. For a group, the first is the leader.
    pub accounts: Vec<LaunchAccount>,
    pub mode: Mode,
    /// None is Roblox's own home screen.
    pub place: Option<PlaceId>,
    /// A server to send everyone to (a friend's), in each mode.
    pub server: Option<ServerId>,
    /// Set from another thread to end the launch before its next sign-in,
    /// or while it waits for a leader's server. Clients already started
    /// keep running: stopping those is the profiles' `stop`.
    pub stop: StopFlag,
}

/// What happened, per account.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchReport {
    /// Started, with the user Roblox says the session is.
    pub launched: Vec<(UserId, User)>,
    /// Roblox refused the stored session: Sign in again is the fix.
    pub expired: Vec<UserId>,
    /// Anything else, with why.
    pub failed: Vec<(UserId, String)>,
    /// Never started: the launch was stopped before their turn.
    pub cancelled: Vec<UserId>,
    /// The server a group joined, when its leader reported one.
    pub server: Option<ServerId>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchError {
    /// No Roblox build could be installed, so nothing could start.
    #[error(transparent)]
    Build(#[from] CordialError),
}

/// Installs or finds the Roblox build, reporting progress through the log.
pub type BuildFn = Arc<dyn Fn(&dyn Fn(String)) -> Result<Build, CordialError> + Send + Sync>;

/// Worker threads only: everything here waits on the keyring, Roblox and
/// processes.
pub struct Launcher {
    keyring: Arc<Keyring>,
    roblox: Arc<dyn Roblox>,
    profiles: Arc<CordialProfiles>,
    build: BuildFn,
    pacing: Pacing,
}

impl Launcher {
    pub fn new(
        keyring: Arc<Keyring>,
        roblox: Arc<dyn Roblox>,
        profiles: Arc<CordialProfiles>,
        build: BuildFn,
        pacing: Pacing,
    ) -> Self {
        Launcher { keyring, roblox, profiles, build, pacing }
    }

    pub fn launch(
        &self,
        req: LaunchRequest,
        log: &dyn Fn(String),
    ) -> Result<LaunchReport, LaunchError> {
        let build = (self.build)(log)?;
        let run =
            Run { launcher: self, build, log, stop: req.stop.clone(), report: RefCell::default() };
        match (req.mode, req.accounts.split_first()) {
            (_, None) => {}
            (Mode::Each, Some(_)) => {
                each::launch(&run, &req.accounts, req.place.as_ref(), req.server.as_ref());
            }
            (Mode::Group, Some((leader, followers))) => {
                let server = group::launch(&run, leader, followers, req.place.as_ref());
                run.report.borrow_mut().server = server;
            }
        }
        Ok(run.report.into_inner())
    }
}

/// One launch in progress: what `each` and `group` drive, and where the
/// outcome of every account is recorded.
struct Run<'a> {
    launcher: &'a Launcher,
    build: Build,
    log: &'a dyn Fn(String),
    stop: StopFlag,
    report: RefCell<LaunchReport>,
}

impl Run<'_> {
    fn log(&self, line: String) {
        (self.log)(line);
    }

    fn sleep(&self, d: Duration) {
        (self.launcher.pacing.sleep)(d);
    }

    fn stopped(&self) -> bool {
        self.stop.is_set()
    }

    /// Wait `d` unless the launch has been stopped; false when it has, by
    /// the start or the end of the wait.
    fn pause(&self, d: Duration) -> bool {
        if self.stopped() {
            return false;
        }
        self.sleep(d);
        !self.stopped()
    }

    /// The launch was stopped: `rest` never start.
    fn give_up(&self, rest: &[LaunchAccount]) {
        if rest.is_empty() {
            return;
        }
        let names: Vec<&str> = rest.iter().map(|a| a.label.as_str()).collect();
        self.log(format!("Launch stopped -- {} not started", names.join(", ")));
        self.report.borrow_mut().cancelled.extend(rest.iter().map(|a| a.id));
    }

    fn pacing(&self) -> &Pacing {
        &self.launcher.pacing
    }

    /// Whether the account's client is up. Not knowing is a failure of that
    /// account, recorded here; the caller logs it.
    fn running(&self, a: &LaunchAccount) -> Result<bool, String> {
        match self.launcher.profiles.running() {
            Ok(up) => Ok(up.contains(&Profile::of(a.id))),
            Err(e) => {
                self.fail(a, e.to_string());
                Err(e.to_string())
            }
        }
    }

    /// Sign the account in and start its client, recording the outcome. The
    /// caller logs a failure, whose reason is returned.
    fn start(&self, a: &LaunchAccount, url: Option<String>) -> Result<(), String> {
        match self.sign_in_and_start(a, url.as_deref()) {
            Ok(user) => {
                self.report.borrow_mut().launched.push((a.id, user));
                Ok(())
            }
            Err(SignIn::Expired(why)) => {
                self.report.borrow_mut().expired.push(a.id);
                Err(why)
            }
            Err(SignIn::Failed(why)) => {
                self.fail(a, why.clone());
                Err(why)
            }
        }
    }

    fn sign_in_and_start(&self, a: &LaunchAccount, url: Option<&str>) -> Result<User, SignIn> {
        let l = self.launcher;
        let failed = |e: &dyn std::fmt::Display| SignIn::Failed(e.to_string());
        let cookie = l.keyring.cookie(&a.label).map_err(|e| failed(&e))?;
        let user = l.roblox.whoami(&cookie).map_err(|e| match e {
            RobloxError::Expired => SignIn::Expired(e.to_string()),
            other => failed(&other),
        })?;
        // The profile is named by the user the session really is. A cookie
        // filed under this account that belongs to someone else would start
        // that someone's profile, under this account's row.
        if user.id != a.id {
            return Err(SignIn::Failed(format!(
                "its stored session belongs to {} (user {}), not this account -- sign in again",
                user.name, user.id
            )));
        }
        let profile = l.profiles.seed(&user, &cookie).map_err(|e| failed(&e))?;
        l.profiles.launch(&profile, url, &self.build, a.opts).map_err(|e| failed(&e))?;
        Ok(user)
    }

    /// Where the account is now, asked as itself.
    fn presence(&self, a: &LaunchAccount) -> Result<Presence, String> {
        let cookie = self.launcher.keyring.cookie(&a.label).map_err(|e| e.to_string())?;
        self.launcher.roblox.presence(&cookie, a.id).map_err(|e| e.to_string())
    }

    fn fail(&self, a: &LaunchAccount, why: String) {
        self.report.borrow_mut().failed.push((a.id, why));
    }
}

/// Why an account did not start.
enum SignIn {
    /// Roblox refused its session.
    Expired(String),
    Failed(String),
}

fn url(place: Option<&PlaceId>, server: Option<&ServerId>) -> Option<String> {
    place.map(|p| join_url(p, server))
}

#[cfg(test)]
mod tests;
