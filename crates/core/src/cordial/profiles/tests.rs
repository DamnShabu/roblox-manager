use super::*;
use crate::cordial::process::recording::Recording;
use crate::keyring::MemorySecrets;
use serde_json::json;

struct World {
    dir: tempfile::TempDir,
    runner: Arc<Recording>,
    secrets: Arc<MemorySecrets>,
    profiles: CordialProfiles,
}

fn world_with(runner: Recording) -> World {
    let dir = tempfile::tempdir().unwrap();
    let runner = Arc::new(runner);
    let secrets = Arc::new(MemorySecrets::default());
    let keyring = Arc::new(Keyring::new(Box::new(Arc::clone(&secrets))));
    let profiles = CordialProfiles::new(
        keyring,
        &Paths::under(dir.path()),
        runner.clone(),
        ProcessView::Own,
        Arc::new(|_| {}),
    );
    World { dir, runner, secrets, profiles }
}

fn world() -> World {
    world_with(Recording::default())
}

fn bob() -> User {
    User { id: UserId(123), name: "bob".into(), display_name: None }
}

fn build() -> Build {
    Build { engine: "/l".into(), apk: "/a.apk".into() }
}

#[test]
fn seeding_creates_the_profile_and_files_identity_then_cookies() {
    let w = world();
    let profile = w.profiles.seed(&bob(), &Cookie::new("sess")).unwrap();
    assert_eq!(profile.as_str(), "rbxmgr-123");
    let dir = w.profiles.path(&profile);
    assert!(dir.is_dir());
    let items = w.secrets.items();
    let kinds: Vec<&str> = items.iter().map(|(a, _, _)| a["store"].as_str()).collect();
    assert_eq!(kinds, ["cookies", "identity"]); // BTreeMap order; both present
    for (attrs, label, secret) in &items {
        assert_eq!(attrs, &secret_attrs_at(&dir, &attrs["store"]));
        assert_eq!(
            label,
            &format!("Cordial: Roblox {} for profile \"rbxmgr-123\"", attrs["store"])
        );
        assert!(secret.starts_with("cordial-secret-hex-v1:"));
    }
    let cookies = items.iter().find(|(a, _, _)| a["store"] == "cookies").unwrap();
    assert_eq!(cookies.2, session::encode(&session::cookie_store(&Cookie::new("sess"))));
}

#[test]
fn clearing_removes_both_sessions_and_keeps_the_directory() {
    let w = world();
    let profile = w.profiles.seed(&bob(), &Cookie::new("sess")).unwrap();
    w.profiles.clear(UserId(123)).unwrap();
    assert!(w.secrets.items().is_empty());
    assert!(w.profiles.path(&profile).is_dir());
}

fn flags(w: &World, p: &Profile) -> PathBuf {
    w.profiles.path(p).join("flags.json")
}

#[test]
fn the_flags_earlier_low_power_switches_wrote_are_taken_out() {
    let w = world();
    let p = Profile::named("p");
    fs::create_dir_all(w.profiles.path(&p)).unwrap();
    let old = r#"{"FIntTaskSchedulerAutoThreadLimit":2,"FFlagX":true}"#;
    fs::write(flags(&w, &p), old).unwrap();
    w.profiles.clear_legacy_low_power_flags(&p).unwrap();
    let written: Value = serde_json::from_slice(&fs::read(flags(&w, &p)).unwrap()).unwrap();
    assert_eq!(written, json!({"FFlagX": true}));
}

#[test]
fn flags_are_left_alone_when_nothing_changes() {
    let w = world();
    let p = Profile::named("p");
    fs::create_dir_all(w.profiles.path(&p)).unwrap();
    let mine = r#"{"FIntTaskSchedulerAutoThreadLimit":4,"DFIntTaskSchedulerTargetFps":144}"#;
    fs::write(flags(&w, &p), mine).unwrap();
    w.profiles.clear_legacy_low_power_flags(&p).unwrap();
    assert_eq!(fs::read_to_string(flags(&w, &p)).unwrap(), mine);
    w.profiles.clear_legacy_low_power_flags(&Profile::named("q")).unwrap();
    assert!(!flags(&w, &Profile::named("q")).exists());
}

#[test]
fn flags_that_do_not_parse_are_not_overwritten() {
    let w = world();
    let p = Profile::named("p");
    fs::create_dir_all(w.profiles.path(&p)).unwrap();
    fs::write(flags(&w, &p), "{broken").unwrap();
    w.profiles.clear_legacy_low_power_flags(&p).unwrap();
    assert_eq!(fs::read_to_string(flags(&w, &p)).unwrap(), "{broken");
}

#[test]
fn a_client_that_stays_up_is_launched_with_its_log_rotated() {
    let w = world();
    let p = Profile::named("rbxmgr-7");
    let logs = w.dir.path().join("cache/rbxmgr/logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("rbxmgr-7.log"), "old run").unwrap();
    w.profiles.launch(&p, Some("roblox://x"), &build(), ClientOpts::default()).unwrap();
    let (argv, env) = &w.runner.spawned()[0];
    assert_eq!(argv, &engine::client_argv("cordial-run", &p, Some("roblox://x"), &build()));
    assert!(env.contains(&("CORDIAL_SECRET_STORE".into(), "keyring".into())));
    assert_eq!(fs::read_to_string(logs.join("rbxmgr-7.log.1")).unwrap(), "old run");
    assert!(!flags(&w, &p).exists(), "a normal client writes no flags");
}

#[test]
fn a_low_power_client_is_niced_throttled_and_capped() {
    let w = world();
    let p = Profile::named("rbxmgr-7");
    w.profiles.launch(&p, None, &build(), ClientOpts { low_power: true, nested: false }).unwrap();
    let (argv, env) = &w.runner.spawned()[0];
    assert_eq!(&argv[..4], ["nice", "-n", "10", "cordial-run"]);
    for (k, v) in engine::LOW_POWER_ENV.into_iter().filter(|(k, _)| *k != "CORDIAL_FPS_CAP") {
        assert!(env.contains(&(k.into(), v.into())), "{k}");
    }
    let cap = ("CORDIAL_FPS_CAP".into(), engine::UNREACHED_LOW_POWER_FPS_CAP.into());
    assert!(env.contains(&cap), "no macro reaches it, so it runs slower still");
    assert!(!flags(&w, &p).exists(), "low power writes no flags any more");
}

#[test]
fn a_low_power_client_plays_at_the_lowest_graphics_quality_until_it_is_not() {
    let w = world();
    let p = Profile::named("rbxmgr-7");
    let prefs = w.profiles.path(&p).join("data/files/appData/GlobalBasicSettings_13.xml");
    fs::create_dir_all(prefs.parent().unwrap()).unwrap();
    let mine = r#"<Properties><token name="SavedQualityLevel">7</token></Properties>"#;
    fs::write(&prefs, mine).unwrap();
    w.profiles.launch(&p, None, &build(), ClientOpts { low_power: true, nested: true }).unwrap();
    assert!(fs::read_to_string(&prefs).unwrap().contains(r#""SavedQualityLevel">1<"#));
    w.profiles.launch(&p, None, &build(), ClientOpts::default()).unwrap();
    assert_eq!(fs::read_to_string(&prefs).unwrap(), mine, "the player's own level back");
}

#[test]
fn a_macro_ready_client_runs_in_a_cage_linked_where_macros_look() {
    let w = world();
    let p = Profile::named("rbxmgr-7");
    w.profiles.launch(&p, None, &build(), ClientOpts { nested: true, low_power: false }).unwrap();
    let (argv, _) = &w.runner.spawned()[0];
    assert_eq!(&argv[..2], ["cage", "--"]);
    let link = nested::display_file(&w.dir.path().join("run"), &p);
    assert_eq!(argv[5], link.display().to_string());
    assert_eq!(argv[6], "cordial-run");
}

#[test]
fn a_macro_ready_client_keeps_playing_out_of_sight_low_power_or_not() {
    let w = world();
    let p = Profile::named("rbxmgr-7");
    w.profiles.launch(&p, None, &build(), ClientOpts { nested: true, low_power: true }).unwrap();
    let (_, env) = &w.runner.spawned()[0];
    let present: Vec<&str> =
        env.iter().filter(|(k, _)| k == "CORDIAL_PRESENT_MODE").map(|(_, v)| v.as_str()).collect();
    assert_eq!(present, ["mailbox"], "never FIFO, which waits on a cage nobody is looking at");
    assert!(env.contains(&("CORDIAL_FPS_CAP".into(), "20".into())), "and still capped");
}

#[test]
fn a_macro_ready_launch_keeps_cages_off_the_display_they_open_on() {
    use rustix::fs::{FlockOperation, flock};
    let dir = tempfile::tempdir().unwrap();
    let run = dir.path().to_str().unwrap().to_owned();
    let paths = Paths::from_vars(
        |k| match k {
            "HOME" | "XDG_RUNTIME_DIR" => Some(run.clone()),
            "WAYLAND_DISPLAY" => Some("wayland-1".into()),
            _ => None,
        },
        1000,
    );
    let profiles = CordialProfiles::new(
        Arc::new(Keyring::new(Box::new(MemorySecrets::default()))),
        &paths,
        Arc::new(Recording::default()),
        ProcessView::Own,
        Arc::new(|_| {}),
    );
    let p = Profile::named("rbxmgr-7");
    profiles.launch(&p, None, &build(), ClientOpts { nested: true, low_power: false }).unwrap();
    // What a cage does to pick its own display name.
    let lock = File::open(dir.path().join("wayland-1.lock")).unwrap();
    assert!(flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err());
}

#[test]
fn a_macro_ready_client_with_a_relay_runs_behind_it_inside_its_cage() {
    let w = world();
    let profiles = w.profiles.with_relay(Some(PathBuf::from("/app/libexec/roblox-manager")));
    let p = Profile::named("rbxmgr-7");
    profiles.launch(&p, None, &build(), ClientOpts { nested: true, low_power: false }).unwrap();
    let (argv, _) = &w.runner.spawned()[0];
    let link = nested::display_file(&w.dir.path().join("run"), &p).display().to_string();
    let relay = "/app/libexec/roblox-manager";
    assert_eq!(argv[..6], [relay, "--window-relay", &link, "--", "cage", "--"], "cage behind one");
    assert_eq!(argv[9], link, "the display link, as the script's $0");
    assert_eq!(argv[10..15], [relay, "--relay", &link, "--", "cordial-run"]);
}

#[test]
fn a_client_in_a_normal_window_never_runs_behind_a_relay() {
    let w = world();
    let profiles = w.profiles.with_relay(Some(PathBuf::from("/app/libexec/roblox-manager")));
    let p = Profile::named("rbxmgr-7");
    profiles.launch(&p, None, &build(), ClientOpts::default()).unwrap();
    assert_eq!(w.runner.spawned()[0].0[0], "cordial-run");
}

#[test]
fn a_client_that_dies_at_once_is_a_failure_that_says_why() {
    let runner = Recording::default();
    *runner.child_exit.lock().unwrap() = Some(1);
    let w = world_with(runner);
    let p = Profile::named("rbxmgr-7");
    let logs = w.dir.path().join("cache/rbxmgr/logs");
    fs::create_dir_all(&logs).unwrap();
    // The recorder does not write the log; a real client would have.
    let err = w.profiles.launch(&p, None, &build(), ClientOpts::default()).unwrap_err();
    match err {
        CordialError::ExitedAtOnce { reason, log } => {
            assert_eq!(reason, "exit status 1");
            assert!(log.ends_with("rbxmgr-7.log"), "{log}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_status_check_that_fails_is_a_failed_launch_not_a_running_client() {
    let runner = Recording::default();
    *runner.child_error.lock().unwrap() = Some("wait failed".into());
    let w = world_with(runner);
    let err = w.profiles.launch(&Profile::named("p"), None, &build(), ClientOpts::default());
    assert!(
        matches!(&err, Err(CordialError::Process(why)) if why.contains("wait failed")),
        "{err:?}"
    );
}

#[test]
fn a_pgrep_that_fails_is_an_error_not_nothing_running() {
    let w = world_with(Recording::default().answer(2, "", "pgrep: invalid option\n"));
    let err = w.profiles.running().unwrap_err();
    assert!(err.to_string().contains("pgrep: invalid option"), "{err}");
}

#[test]
fn stopping_signals_only_the_given_profiles_clients() {
    let pgrep = "1 cordial-run --profile main\n2 cordial-run --profile mine-from-cordial\n3 cordial-run --profile main\n";
    let w = world_with(Recording::default().answer(0, pgrep, ""));
    let n = w.profiles.stop(&HashSet::from([Profile::named("main")])).unwrap();
    assert_eq!(n, 2);
    assert_eq!(w.runner.ran()[1], ["kill", "1", "3"]);
}

#[test]
fn from_a_flatpak_clients_are_found_and_stopped_on_the_host() {
    // The host's pgrep also lists the flatpak-spawn asking it, which is no client.
    let pgrep =
        "7 /app/bin/cordial-run --profile main\n8 flatpak-spawn --host pgrep -a -f cordial-run\n";
    let dir = tempfile::tempdir().unwrap();
    let runner = Arc::new(Recording::default().answer(0, pgrep, ""));
    let keyring = Arc::new(Keyring::new(Box::new(MemorySecrets::default())));
    let profiles = CordialProfiles::new(
        keyring,
        &Paths::under(dir.path()),
        runner.clone(),
        ProcessView::Host,
        Arc::new(|_| {}),
    );
    assert_eq!(profiles.stop(&HashSet::from([Profile::named("main")])).unwrap(), 1);
    let uid = rustix::process::getuid().as_raw().to_string();
    assert_eq!(
        runner.ran(),
        [
            vec!["flatpak-spawn", "--host", "pgrep", "-u", &uid, "-a", "-f", "cordial-run"],
            vec!["flatpak-spawn", "--host", "kill", "7"],
        ]
    );
}

#[test]
fn a_kill_that_fails_is_an_error_but_a_client_already_gone_is_not() {
    let pgrep = "1 cordial-run --profile main\n";
    let refused = Recording::default().answer(0, pgrep, "").answer(
        1,
        "",
        "kill: (1) - Operation not permitted\n",
    );
    let w = world_with(refused);
    assert!(w.profiles.stop(&HashSet::from([Profile::named("main")])).is_err());
    let gone =
        Recording::default().answer(0, pgrep, "").answer(1, "", "kill: (1) - No such process\n");
    let w = world_with(gone);
    assert_eq!(w.profiles.stop(&HashSet::from([Profile::named("main")])).unwrap(), 1);
}

#[test]
fn stopping_nothing_runs_no_kill() {
    let w = world_with(Recording::default().answer(0, "2 cordial-run --profile other\n", ""));
    assert_eq!(w.profiles.stop(&HashSet::from([Profile::named("main")])).unwrap(), 0);
    assert_eq!(w.runner.ran().len(), 1);
}

#[test]
fn no_clients_running_is_an_empty_set_not_an_error() {
    let w = world_with(Recording::default().answer(1, "", ""));
    assert!(w.profiles.running().unwrap().is_empty());
}

fn say_window(w: &World, profile: &str, state: &str) {
    let dir = w.profiles.path(&Profile::named(profile));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("window-state"), state).unwrap();
}

#[test]
fn a_window_is_as_its_engine_last_said() {
    let w = world();
    let main = Profile::named("main");
    assert_eq!(w.profiles.window(&main).unwrap(), None, "no file: an engine that says nothing");
    say_window(&w, "main", "hidden\n");
    assert_eq!(w.profiles.window(&main).unwrap(), Some(Window::Hidden));
    say_window(&w, "main", "shown\n");
    assert_eq!(w.profiles.window(&main).unwrap(), Some(Window::Shown));
}

#[test]
fn hiding_signals_only_clients_whose_engine_can_hide() {
    // `old` has no window state: SIGUSR1 would kill it, so it is left alone.
    let pgrep = "1 cordial-run --profile main\n2 cordial-run --profile old\n3 cordial-run --profile other\n";
    let runner = Recording::default();
    *runner.pgrep.lock().unwrap() = Some(pgrep.into());
    let w = world_with(runner);
    say_window(&w, "main", "shown\n");
    say_window(&w, "other", "shown\n");
    let which = HashSet::from([Profile::named("main"), Profile::named("old")]);
    assert_eq!(w.profiles.set_hidden(&which, true).unwrap(), 1);
    assert_eq!(w.profiles.set_hidden(&which, false).unwrap(), 1);
    let ran = w.runner.ran();
    assert_eq!(ran[1], ["kill", "-USR1", "1"]);
    assert_eq!(ran[3], ["kill", "-USR2", "1"]);
}

#[test]
fn hiding_a_client_that_cannot_hide_runs_no_kill() {
    let w = world_with(Recording::default().answer(0, "2 cordial-run --profile old\n", ""));
    assert_eq!(w.profiles.set_hidden(&HashSet::from([Profile::named("old")]), true).unwrap(), 0);
    assert_eq!(w.runner.ran().len(), 1);
}

/// A window relay for `profile`'s macro-ready client, answering `answer` to
/// every line, and the lines it was sent.
fn window_relay(w: &World, profile: &str, answer: &'static str) -> Arc<Mutex<Vec<String>>> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    let display = nested::display_file(&w.dir.path().join("run"), &Profile::named(profile));
    fs::create_dir_all(display.parent().unwrap()).unwrap();
    let control = UnixListener::bind(display.with_extension("window")).unwrap();
    let said = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&said);
    std::thread::spawn(move || {
        for conn in control.incoming() {
            let conn = conn.unwrap();
            let mut line = String::new();
            BufReader::new(&conn).read_line(&mut line).unwrap();
            heard.lock().unwrap().push(line.trim().to_owned());
            (&conn).write_all(answer.as_bytes()).unwrap();
        }
    });
    said
}

/// `profile` up in a macro-ready window: its cage's display, linked.
fn in_cage(w: &World, profile: &str) -> std::os::unix::net::UnixListener {
    let display = nested::display_file(&w.dir.path().join("run"), &Profile::named(profile));
    fs::create_dir_all(display.parent().unwrap()).unwrap();
    std::os::unix::net::UnixListener::bind(display).unwrap()
}

#[test]
fn a_macro_ready_client_s_window_is_as_its_window_relay_says() {
    let w = world();
    let _cage = in_cage(&w, "rbxmgr-7");
    say_window(&w, "rbxmgr-7", "shown\n");
    window_relay(&w, "rbxmgr-7", "hidden\n");
    assert_eq!(w.profiles.window(&Profile::named("rbxmgr-7")).unwrap(), Some(Window::Hidden));
}

#[test]
fn hiding_a_macro_ready_client_asks_its_window_relay_and_never_signals_the_engine() {
    let runner = Recording::default();
    *runner.pgrep.lock().unwrap() = Some("1 cordial-run --profile rbxmgr-7\n".into());
    let w = world_with(runner);
    let _cage = in_cage(&w, "rbxmgr-7");
    say_window(&w, "rbxmgr-7", "shown\n");
    let said = window_relay(&w, "rbxmgr-7", "hidden\n");
    let which = HashSet::from([Profile::named("rbxmgr-7")]);
    assert_eq!(w.profiles.set_hidden(&which, true).unwrap(), 1);
    assert_eq!(w.profiles.set_hidden(&which, false).unwrap(), 1);
    assert_eq!(*said.lock().unwrap(), ["hide", "show"]);
    assert!(w.runner.ran().iter().all(|argv| argv[0] != "kill"), "{:?}", w.runner.ran());
}

#[test]
fn a_macro_ready_client_from_before_window_relays_cannot_be_hidden() {
    let runner = Recording::default();
    *runner.pgrep.lock().unwrap() = Some("1 cordial-run --profile rbxmgr-7\n".into());
    let w = world_with(runner);
    let _cage = in_cage(&w, "rbxmgr-7");
    // Its engine can hide its own window -- inside cage, where macros need it.
    say_window(&w, "rbxmgr-7", "shown\n");
    let p = Profile::named("rbxmgr-7");
    assert_eq!(w.profiles.window(&p).unwrap(), None);
    assert_eq!(w.profiles.set_hidden(&HashSet::from([p]), true).unwrap(), 0);
    assert!(w.runner.ran().iter().all(|argv| argv[0] != "kill"));
}
