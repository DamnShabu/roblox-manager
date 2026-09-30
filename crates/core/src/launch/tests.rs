use std::collections::VecDeque;
use std::sync::Mutex;

use super::*;
use crate::cordial::process::recording::Recording;
use crate::keyring::MemorySecrets;
use crate::paths::Paths;
use crate::roblox::{Friend, Game, QuickLoginCode, QuickLoginStatus};
use crate::stop::StopFlag;
use crate::types::Cookie;

/// Roblox as the launch sees it: a cookie `u<id>` is user `id`, `expired` is
/// refused, and presence answers from a queue (then "no server").
#[derive(Default)]
struct FakeRoblox {
    presences: Mutex<VecDeque<Result<Presence, RobloxError>>>,
    presence_asks: Mutex<usize>,
}

impl Roblox for FakeRoblox {
    fn whoami(&self, cookie: &Cookie) -> Result<User, RobloxError> {
        match cookie.expose().strip_prefix('u').and_then(|id| id.parse().ok()) {
            Some(id) => Ok(User { id: UserId(id), name: format!("user{id}"), display_name: None }),
            None if cookie.expose() == "expired" => Err(RobloxError::Expired),
            None => Err(RobloxError::Offline("no network".into())),
        }
    }
    fn presence(&self, _c: &Cookie, _u: UserId) -> Result<Presence, RobloxError> {
        *self.presence_asks.lock().unwrap() += 1;
        self.presences.lock().unwrap().pop_front().unwrap_or(Ok(Presence::default()))
    }
    fn friends(&self, _: &Cookie, _: UserId) -> Result<Vec<Friend>, RobloxError> {
        unimplemented!()
    }
    fn favorites(&self, _: &Cookie, _: UserId, _: usize) -> Result<Vec<Game>, RobloxError> {
        unimplemented!()
    }
    fn icon_urls(
        &self,
        _: &[String],
    ) -> Result<std::collections::HashMap<String, String>, RobloxError> {
        unimplemented!()
    }
    fn headshot_urls(
        &self,
        _: &[UserId],
    ) -> Result<std::collections::HashMap<UserId, String>, RobloxError> {
        unimplemented!()
    }
    fn quick_login_create(&self) -> Result<QuickLoginCode, RobloxError> {
        unimplemented!()
    }
    fn quick_login_status(&self, _: &QuickLoginCode) -> Result<QuickLoginStatus, RobloxError> {
        unimplemented!()
    }
    fn quick_login_redeem(&self, _: &QuickLoginCode) -> Result<Cookie, RobloxError> {
        unimplemented!()
    }
}

struct World {
    _dir: tempfile::TempDir,
    roblox: Arc<FakeRoblox>,
    runner: Arc<Recording>,
    slept: Arc<Mutex<Vec<Duration>>>,
    /// Set during the nth wait (from 1), as a Stop pressed then would be.
    stop_at_wait: Arc<Mutex<Option<(usize, StopFlag)>>>,
    logs: Mutex<Vec<String>>,
    launcher: Launcher,
}

/// Accounts 1..=n, labelled a1..an, whose cookies are `u<id>` -- except the
/// labels listed in `sessions`, which get the cookie given there.
fn world(n: u64, sessions: &[(&str, &str)], running: &[u64]) -> World {
    let dir = tempfile::tempdir().unwrap();
    let keyring = Arc::new(Keyring::new(Box::new(MemorySecrets::default())));
    for id in 1..=n {
        let label = Label::parse(&format!("a{id}")).unwrap();
        let cookie = sessions
            .iter()
            .find(|(l, _)| *l == label.as_str())
            .map_or_else(|| format!("u{id}"), |(_, c)| c.to_string());
        keyring.set_cookie(&label, &Cookie::new(cookie)).unwrap();
    }
    let runner = Arc::new(Recording::default());
    let pgrep: String =
        running.iter().map(|id| format!("{id}0 cordial-run --profile rbxmgr-{id}\n")).collect();
    *runner.pgrep.lock().unwrap() = Some(pgrep);
    let profiles = Arc::new(CordialProfiles::new(
        Arc::clone(&keyring),
        &Paths::under(dir.path()),
        runner.clone(),
        crate::cordial::ProcessView::Own,
        Arc::new(|_| {}),
    ));
    let roblox = Arc::new(FakeRoblox::default());
    let slept = Arc::new(Mutex::new(Vec::new()));
    let stop_at_wait: Arc<Mutex<Option<(usize, StopFlag)>>> = Arc::default();
    let (slept2, stop2) = (Arc::clone(&slept), Arc::clone(&stop_at_wait));
    let pacing = Pacing {
        stagger: Duration::from_secs(8),
        leader_timeout: Duration::from_secs(9),
        poll: Duration::from_secs(3),
        sleep: Arc::new(move |d| {
            let mut slept = slept2.lock().unwrap();
            slept.push(d);
            if let Some((n, flag)) = &*stop2.lock().unwrap() {
                if slept.len() == *n {
                    flag.set();
                }
            }
        }),
    };
    let build: BuildFn = Arc::new(|_log| Ok(Build { engine: "/l".into(), apk: "/a".into() }));
    let launcher = Launcher::new(keyring, roblox.clone(), profiles, build, pacing);
    World { _dir: dir, roblox, runner, slept, stop_at_wait, logs: Mutex::new(Vec::new()), launcher }
}

fn account(id: u64) -> LaunchAccount {
    LaunchAccount {
        id: UserId(id),
        label: Label::parse(&format!("a{id}")).unwrap(),
        opts: ClientOpts::default(),
    }
}

fn request(ids: &[u64], mode: Mode, place: Option<&str>, server: Option<&str>) -> LaunchRequest {
    LaunchRequest {
        accounts: ids.iter().copied().map(account).collect(),
        mode,
        place: place.map(|p| PlaceId::parse(p).unwrap()),
        server: server.map(|s| ServerId::parse(s).unwrap()),
        stop: StopFlag::default(),
    }
}

impl World {
    fn launch(&self, req: LaunchRequest) -> LaunchReport {
        self.launcher.launch(req, &|l| self.logs.lock().unwrap().push(l)).unwrap()
    }

    /// The join link each started client was given, by profile.
    fn started(&self) -> Vec<(String, Option<String>)> {
        self.runner
            .spawned()
            .into_iter()
            .map(|(argv, _)| {
                let at =
                    |flag: &str| argv.iter().position(|a| a == flag).map(|i| argv[i + 1].clone());
                (at("--profile").unwrap_or_default(), at("--join-url"))
            })
            .collect()
    }

    /// The request, stopped during its `n`th wait.
    fn stop_at_wait(&self, n: usize, req: &LaunchRequest) {
        *self.stop_at_wait.lock().unwrap() = Some((n, req.stop.clone()));
    }

    fn staggers(&self) -> usize {
        self.slept.lock().unwrap().iter().filter(|d| **d == Duration::from_secs(8)).count()
    }

    fn logged(&self, needle: &str) -> bool {
        self.logs.lock().unwrap().iter().any(|l| l.contains(needle))
    }
}

fn launched(r: &LaunchReport) -> Vec<u64> {
    r.launched.iter().map(|(id, _)| id.0).collect()
}

fn link(place: &str, server: Option<&str>) -> Option<String> {
    Some(join_url(
        &PlaceId::parse(place).unwrap(),
        server.map(|s| ServerId::parse(s).unwrap()).as_ref(),
    ))
}

#[test]
fn each_mode_launches_every_account_into_the_place() {
    let w = world(3, &[], &[]);
    let r = w.launch(request(&[1, 2, 3], Mode::Each, Some("77"), None));
    assert_eq!(launched(&r), [1, 2, 3]);
    assert_eq!(w.started()[2], ("rbxmgr-3".into(), link("77", None)));
    assert_eq!(w.staggers(), 2, "no wait before the first sign-in");
}

#[test]
fn each_mode_sends_everyone_to_a_named_server() {
    let w = world(2, &[], &[]);
    w.launch(request(&[1, 2], Mode::Each, Some("77"), Some("s-9")));
    assert!(w.started().iter().all(|(_, url)| *url == link("77", Some("s-9"))));
}

#[test]
fn no_place_is_robloxs_home_screen() {
    let w = world(1, &[], &[]);
    w.launch(request(&[1], Mode::Each, None, Some("s-9")));
    assert_eq!(w.started(), [("rbxmgr-1".to_string(), None)]);
}

#[test]
fn an_account_already_running_is_skipped_and_costs_no_wait() {
    let w = world(3, &[], &[2]);
    let r = w.launch(request(&[1, 2, 3], Mode::Each, Some("77"), None));
    assert_eq!(launched(&r), [1, 3]);
    assert_eq!(w.staggers(), 1);
    assert!(w.logged("a2: already running -- skipping launch"));
}

#[test]
fn expired_and_failed_accounts_are_reported_and_the_rest_still_launch() {
    let w = world(3, &[("a1", "expired"), ("a2", "offline")], &[]);
    let r = w.launch(request(&[1, 2, 3], Mode::Each, Some("77"), None));
    assert_eq!(launched(&r), [3]);
    assert_eq!(r.expired, [UserId(1)]);
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].1.contains("no network"), "{:?}", r.failed);
    assert!(w.logged("a2: FAILED -- "));
}

#[test]
fn a_group_waits_for_the_leaders_server_and_the_rest_join_it() {
    let w = world(3, &[], &[]);
    w.roblox.presences.lock().unwrap().extend([
        Ok(Presence::default()),
        Ok(Presence {
            server: Some(ServerId::parse("s-1").unwrap()),
            place: Some(PlaceId::parse("88").unwrap()),
        }),
    ]);
    let r = w.launch(request(&[1, 2, 3], Mode::Group, Some("77"), None));
    assert_eq!(launched(&r), [1, 2, 3]);
    assert_eq!(r.server.as_ref().map(ServerId::as_str), Some("s-1"));
    let started = w.started();
    assert_eq!(started[0].1, link("77", None));
    assert_eq!(started[1].1, link("88", Some("s-1")), "followers go to the leader's place");
    assert_eq!(started[2].1, link("88", Some("s-1")));
}

#[test]
fn a_leader_already_running_is_looked_up_at_once() {
    let w = world(2, &[], &[1]);
    w.roblox
        .presences
        .lock()
        .unwrap()
        .push_back(Ok(Presence { server: Some(ServerId::parse("s-1").unwrap()), place: None }));
    w.launch(request(&[1, 2], Mode::Group, Some("77"), None));
    let polls = w.slept.lock().unwrap().iter().filter(|d| **d == Duration::from_secs(3)).count();
    assert_eq!(polls, 0);
    assert_eq!(w.started(), [("rbxmgr-2".to_string(), link("77", Some("s-1")))]);
}

#[test]
fn with_no_server_in_time_the_followers_get_their_own() {
    let w = world(2, &[], &[]);
    let r = w.launch(request(&[1, 2], Mode::Group, Some("77"), None));
    assert_eq!(r.server, None);
    assert_eq!(*w.roblox.presence_asks.lock().unwrap(), 3, "9s timeout / 3s polls");
    assert_eq!(w.started()[1].1, link("77", None));
    assert!(w.logged("followers get their own"));
}

#[test]
fn a_failed_leader_launches_nobody_else() {
    let w = world(2, &[("a1", "expired")], &[]);
    let r = w.launch(request(&[1, 2], Mode::Group, Some("77"), None));
    assert!(r.launched.is_empty());
    assert_eq!(r.expired, [UserId(1)]);
    assert!(w.logged("nobody has a server to join"));
}

#[test]
fn a_leader_alone_never_polls_presence() {
    let w = world(1, &[], &[]);
    w.launch(request(&[1], Mode::Group, Some("77"), None));
    assert_eq!(*w.roblox.presence_asks.lock().unwrap(), 0);
}

#[test]
fn a_failed_presence_poll_is_not_a_failed_launch() {
    let w = world(2, &[], &[]);
    w.roblox.presences.lock().unwrap().push_back(Err(RobloxError::Offline("blip".into())));
    let r = w.launch(request(&[1, 2], Mode::Group, Some("77"), None));
    assert_eq!(launched(&r), [1, 2]);
    assert!(w.logged("presence poll failed"));
}

#[test]
fn no_build_is_an_error_and_nothing_starts() {
    let mut w = world(1, &[], &[]);
    w.launcher.build = Arc::new(|_| Err(CordialError::Build("no source has a build".into())));
    let err = w.launcher.launch(request(&[1], Mode::Each, Some("77"), None), &|_| {}).unwrap_err();
    assert_eq!(err, LaunchError::Build(CordialError::Build("no source has a build".into())));
    assert!(w.started().is_empty());
}

#[test]
fn a_started_client_gets_its_accounts_options() {
    let w = world(1, &[], &[]);
    let mut req = request(&[1], Mode::Each, Some("77"), None);
    req.accounts[0].opts = ClientOpts { nested: true, low_power: true };
    w.launch(req);
    let argv = &w.runner.spawned()[0].0;
    assert_eq!(&argv[..2], ["cage", "--"]);
    assert!(argv.contains(&"nice".to_string()));
}

#[test]
fn a_session_that_is_another_user_starts_nobodys_client() {
    let w = world(2, &[("a1", "u2")], &[]);
    let r = w.launch(request(&[1], Mode::Each, Some("77"), None));
    assert!(r.launched.is_empty());
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].1.contains("belongs to user2"), "{:?}", r.failed);
    assert!(w.started().is_empty());
}

#[test]
fn a_stopped_launch_starts_nobody_after_the_stop() {
    let w = world(3, &[], &[]);
    let req = request(&[1, 2, 3], Mode::Each, Some("77"), None);
    w.stop_at_wait(1, &req);
    let r = w.launch(req);
    assert_eq!(launched(&r), [1], "stopped during the wait before the second");
    assert_eq!(r.cancelled, [UserId(2), UserId(3)]);
    assert_eq!(w.started().len(), 1);
    assert!(w.logged("Launch stopped -- a2, a3 not started"));
}

#[test]
fn a_launch_stopped_before_it_began_starts_nobody() {
    for mode in [Mode::Each, Mode::Group] {
        let w = world(2, &[], &[]);
        let req = request(&[1, 2], mode, Some("77"), None);
        req.stop.set();
        let r = w.launch(req);
        assert!(r.launched.is_empty(), "{mode:?}");
        assert_eq!(r.cancelled, [UserId(1), UserId(2)], "{mode:?}");
        assert!(w.started().is_empty(), "{mode:?}");
    }
}

#[test]
fn a_group_stopped_while_it_waits_for_the_leaders_server_sends_nobody_after_it() {
    let w = world(3, &[], &[]);
    let req = request(&[1, 2, 3], Mode::Group, Some("77"), None);
    w.stop_at_wait(1, &req);
    let r = w.launch(req);
    assert_eq!(launched(&r), [1], "the leader was already up");
    assert_eq!(r.cancelled, [UserId(2), UserId(3)]);
    assert_eq!(*w.roblox.presence_asks.lock().unwrap(), 0, "no poll after the stop");
    assert!(!w.logged("followers get their own"));
}
