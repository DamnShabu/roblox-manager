//! Starting an engine the way Cordial's own window does (spawn() in
//! crates/cordial-shell/src/launch.rs): its settings as environment, its
//! arguments, and what a low-power client changes.

use std::path::Path;

use serde_json::{Map, Value};

use super::build::Build;
use crate::types::Profile;

/// Cordial's settings from shell.json. Missing or unreadable is none set.
pub fn load_settings(path: &Path) -> Map<String, Value> {
    crate::json_file::read(path)
}

/// The settings as the environment Cordial's window gives an engine. An
/// absent key keeps the engine's own default. The session store is always
/// the keyring, so no session is ever written to a plaintext file.
pub fn env(settings: &Map<String, Value>) -> Vec<(String, String)> {
    let text = |k: &str| settings.get(k).and_then(Value::as_str);
    let flag = |k: &str| settings.get(k).and_then(Value::as_bool);
    let mut env = vec![("CORDIAL_SECRET_STORE", "keyring".to_owned())];
    if flag("gamemode") == Some(false) {
        env.push(("CORDIAL_GAMEMODE", "0".into()));
    }
    if let Some(bar @ ("compact" | "hidden")) = text("title_bar") {
        env.push(("CORDIAL_TITLE_BAR", bar.into()));
    }
    if let Some(t @ ("visible" | "unfocused" | "off")) = text("throttle") {
        env.push(("CORDIAL_THROTTLE", t.into()));
    }
    match text("pointer_acceleration") {
        Some("unlockedcursor") => env.push(("CORDIAL_POINTER_ACCEL", "unlocked".into())),
        Some("always") => env.push(("CORDIAL_POINTER_ACCEL", "always".into())),
        _ => {}
    }
    match settings.get("graphics") {
        None | Some(Value::Null) => {}
        Some(Value::String(g)) if g.is_empty() || g == "automatic" => {}
        Some(Value::String(g)) => env.push(("CORDIAL_GRAPHICS", g.clone())),
        Some(other) => env.push(("CORDIAL_GRAPHICS", other.to_string())),
    }
    if let Some(m @ ("fifo" | "mailbox" | "immediate")) = text("present_mode") {
        env.push(("CORDIAL_PRESENT_MODE", m.into()));
    }
    if flag("gamepad") == Some(false) {
        env.push(("CORDIAL_GAMEPAD", "0".into()));
    }
    if flag("close_on_leave") == Some(true) {
        env.push(("CORDIAL_CLOSE_ON_LEAVE", "1".into()));
    }
    match text("graphics_optimization_mode") {
        Some("roblox-app") => env.push(("CORDIAL_DEVICE_PROFILE", "roblox-app".into())),
        Some("mobile-tier") => env.push(("CORDIAL_DEVICE_PROFILE", "android-tablet".into())),
        Some("more-cores") => env.push(("CORDIAL_PERFORMANCE", "throughput".into())),
        Some("fewer-cores") => env.push(("CORDIAL_PERFORMANCE", "latency".into())),
        _ => {}
    }
    if let Some(sink) = text("audio_output").map(str::trim).filter(|s| !s.is_empty()) {
        env.push(("CORDIAL_AUDIO_SINK", sink.into()));
    }
    // Stacked's own settings. The client confines the cursor and draws its
    // own theme unless told otherwise, so only the other choice is sent.
    if flag("fullscreen_confine") == Some(false) {
        env.push(("CORDIAL_NO_FULLSCREEN_CONFINE", "1".into()));
    }
    if text("theme") == Some("system") {
        env.push(("CORDIAL_THEME", "system".into()));
    }
    if let Some(cap) = settings.get("fps_cap").and_then(Value::as_u64).filter(valid_fps_cap) {
        env.push((FPS_CAP, cap.to_string()));
    }
    env.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// The frame-rate target Stacked turns into a flag layer beneath the
/// profile's own flags.json, so a target set there still wins.
const FPS_CAP: &str = "CORDIAL_FPS_CAP";

/// The range the client accepts; anything else it refuses by name.
fn valid_fps_cap(cap: &u64) -> bool {
    (1..=1000).contains(cap)
}

/// A low-power client, for an account along for the ride: throttled when
/// unfocused, FIFO-paced, no GameMode boost, 20 frames a second, a longer
/// back-off in an idle poll loop (and niced by the launcher, with the game's
/// own graphics quality at its lowest, see `quality.rs`). These replace
/// whatever the settings chose. 20 rather than 10 for a macro-ready client:
/// a macro's presses only reach the game a frame at a time, and at 10 a press
/// lands up to a tenth of a second off -- enough for clients playing the
/// same macro to drift apart. One no macro can reach gets
/// [`UNREACHED_LOW_POWER_FPS_CAP`].
pub const LOW_POWER_ENV: [(&str, &str); 5] = [
    ("CORDIAL_THROTTLE", "unfocused"),
    ("CORDIAL_PRESENT_MODE", "fifo"),
    ("CORDIAL_GAMEMODE", "0"),
    (FPS_CAP, "20"),
    ("CORDIAL_POLL_COALESCE_US", "2000"),
];

/// The frame cap for a low-power client outside a macro-ready window, where
/// no macro's timing depends on it. At 10 frames against 20 with the default
/// 250 us back-off, on Stacked 0.21.6's landing page, a client went from
/// 4.3% to 2.8% of a core.
pub const UNREACHED_LOW_POWER_FPS_CAP: &str = "10";

/// What earlier versions wrote into the profile's flags.json, taken back out.
/// The frame-rate target there would outrank one set anywhere else; the
/// worker-thread cap was measured doing nothing (same 66 threads, same CPU),
/// as Stacked's own docs/NEXT.md had already found.
const LEGACY_LOW_POWER_FLAGS: [(&str, i64); 2] =
    [("DFIntTaskSchedulerTargetFps", 20), ("FIntTaskSchedulerAutoThreadLimit", 2)];

/// `flags` without what earlier versions of the low-power switch wrote. A
/// value you set to something else yourself is never removed.
pub fn without_legacy_low_power_flags(flags: &Map<String, Value>) -> Map<String, Value> {
    let mut flags = flags.clone();
    for (k, v) in LEGACY_LOW_POWER_FLAGS {
        if flags.get(k) == Some(&Value::from(v)) {
            flags.remove(k);
        }
    }
    flags
}

/// `env` with the low-power values in place of any the settings gave, for a
/// client in a macro-ready window or not.
pub fn with_low_power(env: Vec<(String, String)>, nested: bool) -> Vec<(String, String)> {
    let env = replaced(env, &LOW_POWER_ENV);
    if nested { env } else { replaced(env, &[(FPS_CAP, UNREACHED_LOW_POWER_FPS_CAP)]) }
}

/// A macro-ready client's presents never wait on its display. Its cage is a
/// window like any other, and draws only when the desktop asks it to: niri
/// asks about once a second for a window on another workspace or scrolled
/// out of view. A FIFO-paced engine waits for each of those frames before it
/// runs on, so the game -- and the macro playing into it -- all but stopped
/// whenever its window was out of sight. MAILBOX hands each frame over and
/// carries on. A low-power client stays as light: its frame cap, which the
/// engine keeps itself, is what holds it to 20 a second.
pub const NESTED_ENV: [(&str, &str); 1] = [("CORDIAL_PRESENT_MODE", "mailbox")];

/// `env` for a client in a macro-ready window, after any low-power values.
pub fn with_nested(env: Vec<(String, String)>) -> Vec<(String, String)> {
    replaced(env, &NESTED_ENV)
}

/// `env` with `values` in place of any it already had for their keys.
fn replaced(mut env: Vec<(String, String)>, values: &[(&str, &str)]) -> Vec<(String, String)> {
    env.retain(|(k, _)| values.iter().all(|(v, _)| v != k));
    env.extend(values.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())));
    env
}

/// One account's engine, `program` (a `cordial-run`), with the arguments
/// upstream's window starts it with.
pub fn client_argv(
    program: &str,
    profile: &Profile,
    url: Option<&str>,
    build: &Build,
) -> Vec<String> {
    let mut argv = vec![program.to_owned(), "--lib-dir".into()];
    argv.push(build.engine.display().to_string());
    argv.push("--apk".into());
    argv.push(build.apk.display().to_string());
    argv.extend(["--host-libc", "--game-activity", "--run", "0", "--profile"].map(String::from));
    argv.push(profile.as_str().to_owned());
    if let Some(url) = url {
        argv.push("--join-url".into());
        argv.push(url.to_owned());
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn pairs(p: &[(&str, &str)]) -> Vec<(String, String)> {
        p.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn settings_reach_the_engine_as_its_window_passes_them() {
        let s = map(json!({
            "roblox": {"apk": null, "lib_dir": null}, "gamemode": false,
            "throttle": "visible", "pointer_acceleration": "unlockedcursor",
            "graphics": "automatic", "present_mode": "mailbox", "gamepad": true,
            "close_on_leave": false, "graphics_optimization_mode": "more-cores",
            "audio_output": "", "title_bar": "default"
        }));
        assert_eq!(
            env(&s),
            pairs(&[
                ("CORDIAL_SECRET_STORE", "keyring"),
                ("CORDIAL_GAMEMODE", "0"),
                ("CORDIAL_THROTTLE", "visible"),
                ("CORDIAL_POINTER_ACCEL", "unlocked"),
                ("CORDIAL_PRESENT_MODE", "mailbox"),
                ("CORDIAL_PERFORMANCE", "throughput"),
            ])
        );
    }

    #[test]
    fn every_other_setting_maps_too() {
        let s = map(json!({
            "title_bar": "hidden", "pointer_acceleration": "always", "graphics": "vulkan",
            "gamepad": false, "close_on_leave": true, "graphics_optimization_mode": "mobile-tier",
            "audio_output": " sink-1 "
        }));
        assert_eq!(
            env(&s),
            pairs(&[
                ("CORDIAL_SECRET_STORE", "keyring"),
                ("CORDIAL_TITLE_BAR", "hidden"),
                ("CORDIAL_POINTER_ACCEL", "always"),
                ("CORDIAL_GRAPHICS", "vulkan"),
                ("CORDIAL_GAMEPAD", "0"),
                ("CORDIAL_CLOSE_ON_LEAVE", "1"),
                ("CORDIAL_DEVICE_PROFILE", "android-tablet"),
                ("CORDIAL_AUDIO_SINK", "sink-1"),
            ])
        );
    }

    #[test]
    fn stackeds_own_settings_are_sent_only_when_they_differ_from_its_default() {
        let s = map(json!({"fullscreen_confine": false, "theme": "system", "fps_cap": 144}));
        assert_eq!(
            env(&s),
            pairs(&[
                ("CORDIAL_SECRET_STORE", "keyring"),
                ("CORDIAL_NO_FULLSCREEN_CONFINE", "1"),
                ("CORDIAL_THEME", "system"),
                ("CORDIAL_FPS_CAP", "144"),
            ])
        );
        let defaults = map(json!({"fullscreen_confine": true, "theme": "stacked", "fps_cap": 0}));
        assert_eq!(env(&defaults), pairs(&[("CORDIAL_SECRET_STORE", "keyring")]));
    }

    #[test]
    fn low_power_replaces_what_the_settings_chose() {
        let chosen = env(&map(json!({"throttle": "visible", "fps_cap": 144})));
        assert_eq!(
            with_low_power(chosen.clone(), true),
            pairs(&[
                ("CORDIAL_SECRET_STORE", "keyring"),
                ("CORDIAL_THROTTLE", "unfocused"),
                ("CORDIAL_PRESENT_MODE", "fifo"),
                ("CORDIAL_GAMEMODE", "0"),
                ("CORDIAL_FPS_CAP", "20"),
                ("CORDIAL_POLL_COALESCE_US", "2000"),
            ])
        );
        let caps: Vec<String> = with_low_power(chosen, false)
            .into_iter()
            .filter(|(k, _)| k == "CORDIAL_FPS_CAP")
            .map(|(_, v)| v)
            .collect();
        assert_eq!(caps, ["10"], "no macro reaches it, so no macro's timing holds it at 20");
    }

    #[test]
    fn a_macro_ready_client_never_waits_on_its_display_even_low_power() {
        let fifo = env(&map(json!({"present_mode": "fifo"})));
        assert_eq!(
            with_nested(fifo),
            pairs(&[("CORDIAL_SECRET_STORE", "keyring"), ("CORDIAL_PRESENT_MODE", "mailbox")])
        );
        let low = with_nested(with_low_power(env(&map(json!({}))), true));
        let present: Vec<&str> = low
            .iter()
            .filter(|(k, _)| k == "CORDIAL_PRESENT_MODE")
            .map(|(_, v)| v.as_str())
            .collect();
        assert_eq!(present, ["mailbox"]);
        assert!(low.contains(&("CORDIAL_FPS_CAP".to_owned(), "20".to_owned())), "still capped");
    }

    #[test]
    fn no_settings_is_only_the_keyring_store() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("shell.json");
        std::fs::write(&bad, "{nope").unwrap();
        assert_eq!(env(&load_settings(&bad)), pairs(&[("CORDIAL_SECRET_STORE", "keyring")]));
        assert!(load_settings(&dir.path().join("missing.json")).is_empty());
    }

    #[test]
    fn the_flags_earlier_versions_wrote_are_taken_out_and_yours_kept() {
        let ours = map(json!({
            "DFIntTaskSchedulerTargetFps": 20, "FIntTaskSchedulerAutoThreadLimit": 2, "FFlagX": true
        }));
        assert_eq!(Value::Object(without_legacy_low_power_flags(&ours)), json!({"FFlagX": true}));
        let yours =
            map(json!({"DFIntTaskSchedulerTargetFps": 144, "FIntTaskSchedulerAutoThreadLimit": 4}));
        assert_eq!(without_legacy_low_power_flags(&yours), yours);
    }

    #[test]
    fn the_engine_is_cordial_run_for_the_accounts_profile() {
        let build = Build { engine: PathBuf::from("/l"), apk: PathBuf::from("/a.apk") };
        let argv =
            client_argv("cordial-run", &Profile::named("rbxmgr-7"), Some("roblox://x"), &build);
        assert_eq!(
            argv,
            [
                "cordial-run",
                "--lib-dir",
                "/l",
                "--apk",
                "/a.apk",
                "--host-libc",
                "--game-activity",
                "--run",
                "0",
                "--profile",
                "rbxmgr-7",
                "--join-url",
                "roblox://x"
            ]
        );
        assert!(
            !client_argv("cordial-run", &Profile::named("p"), None, &build)
                .contains(&"--join-url".to_string())
        );
    }
}
