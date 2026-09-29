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
fn low_power_writes_its_flags_into_the_profile() {
    let w = world();
    let p = Profile::named("p");
    w.profiles.set_low_power(&p, true).unwrap();
    let written: Value = serde_json::from_slice(&fs::read(flags(&w, &p)).unwrap()).unwrap();
    assert_eq!(
        written,
        json!({"DFIntTaskSchedulerTargetFps": 20, "FIntTaskSchedulerAutoThreadLimit": 2})
    );
}

#[test]
fn flags_are_left_alone_when_nothing_changes() {
    let w = world();
    let p = Profile::named("p");
    fs::create_dir_all(w.profiles.path(&p)).unwrap();
    let mine = r#"{"FIntTaskSchedulerAutoThreadLimit":2,"DFIntTaskSchedulerTargetFps":20}"#;
    fs::write(flags(&w, &p), mine).unwrap();
    w.profiles.set_low_power(&p, true).unwrap();
    assert_eq!(fs::read_to_string(flags(&w, &p)).unwrap(), mine);
    w.profiles.set_low_power(&Profile::named("q"), false).unwrap();
    assert!(!flags(&w, &Profile::named("q")).exists());
}

#[test]
fn flags_that_do_not_parse_are_not_overwritten() {
    let w = world();
    let p = Profile::named("p");
    fs::create_dir_all(w.profiles.path(&p)).unwrap();
    fs::write(flags(&w, &p), "{broken").unwrap();
    w.profiles.set_low_power(&p, true).unwrap();
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
    assert_eq!(argv, &engine::client_argv(&p, Some("roblox://x"), &build()));
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
    for (k, v) in engine::LOW_POWER_ENV {
        assert!(env.contains(&(k.into(), v.into())), "{k}");
    }
    assert!(flags(&w, &p).exists());
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
    assert_eq!(
        runner.ran(),
        [
            vec!["flatpak-spawn", "--host", "pgrep", "-a", "-f", "cordial-run"],
            vec!["flatpak-spawn", "--host", "kill", "7"],
        ]
    );
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
