//! The Roblox build the clients run, installed by cordial-fetch.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde::Deserialize;

use super::CordialError;
use super::process::{Runner, last_line};

/// An installed Roblox build: the engine's library directory and its APK.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    pub engine: PathBuf,
    pub apk: PathBuf,
}

/// One at a time: the startup of two launches must not both download.
static FETCHING: Mutex<()> = Mutex::new(());

/// The installed build, installing one first when there is none -- or, with
/// `newest`, whenever a newer one exists. Installing takes a copy already on
/// this machine when there is one and downloads otherwise; either way
/// cordial-fetch only installs what Roblox signed.
pub fn roblox_build(
    runner: &dyn Runner,
    log: &dyn Fn(String),
    newest: bool,
) -> Result<Build, CordialError> {
    let _one_at_a_time = FETCHING.lock().unwrap_or_else(PoisonError::into_inner);
    let fetch = |args: &[&str], timeout: Duration| {
        let argv: Vec<String> = std::iter::once("cordial-fetch")
            .chain(args.iter().copied())
            .map(String::from)
            .collect();
        runner.run(&argv, timeout)
    };
    if !newest {
        let status = fetch(&["--status"], Duration::from_secs(30))?;
        if let Some(build) = status.success().then(|| parse_result(&status.stdout)).flatten() {
            return Ok(build);
        }
    }
    log("Getting the Roblox build (first run or update; this can take a few minutes)...".into());
    let args: &[&str] = if newest { &["--newest"] } else { &[] };
    let out = fetch(args, Duration::from_secs(3600))?;
    match out.success().then(|| parse_result(&out.stdout)).flatten() {
        Some(build) => Ok(build),
        None => {
            let why = last_line(&out.stderr);
            Err(CordialError::Build(if why.is_empty() {
                format!("exit status {}", out.status)
            } else {
                why
            }))
        }
    }
}

/// The build from cordial-fetch's closing JSON line.
fn parse_result(stdout: &[u8]) -> Option<Build> {
    #[derive(Deserialize)]
    struct Fetched {
        engine: Option<PathBuf>,
        apk: Option<PathBuf>,
    }
    let got: Fetched = serde_json::from_str(&last_line(stdout)).ok()?;
    Some(Build { engine: got.engine?, apk: got.apk? })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cordial::process::recording::Recording;
    use std::cell::RefCell;

    const FETCHED: &str = r#"{"apk": "/c/build/x86_64/base.apk", "engine": "/c/lib/x86_64", "version": "2.734.0.917"}
"#;

    fn build() -> Build {
        Build { engine: "/c/lib/x86_64".into(), apk: "/c/build/x86_64/base.apk".into() }
    }

    #[test]
    fn an_installed_build_is_used_as_it_is() {
        let r = Recording::default().answer(0, FETCHED, "");
        let logs = RefCell::new(Vec::new());
        assert_eq!(roblox_build(&r, &|l| logs.borrow_mut().push(l), false).unwrap(), build());
        assert_eq!(r.ran(), vec![vec!["cordial-fetch".to_string(), "--status".into()]]);
        assert!(logs.borrow().is_empty());
    }

    #[test]
    fn no_build_yet_installs_one_and_says_so() {
        let r = Recording::default().answer(1, "", "").answer(0, FETCHED, "");
        let logs = RefCell::new(Vec::new());
        assert_eq!(roblox_build(&r, &|l| logs.borrow_mut().push(l), false).unwrap(), build());
        assert_eq!(r.ran()[1], vec!["cordial-fetch".to_string()]);
        assert_eq!(logs.borrow().len(), 1);
    }

    #[test]
    fn updating_asks_for_the_newest_and_skips_the_status_check() {
        let r = Recording::default().answer(0, FETCHED, "");
        roblox_build(&r, &|_| {}, true).unwrap();
        assert_eq!(r.ran(), vec![vec!["cordial-fetch".to_string(), "--newest".into()]]);
    }

    #[test]
    fn a_failed_install_says_why() {
        let r = Recording::default().answer(1, "", "").answer(
            1,
            "",
            "asking local\ncordial-fetch: no source has a build\n",
        );
        let err = roblox_build(&r, &|_| {}, false).unwrap_err();
        assert_eq!(err, CordialError::Build("cordial-fetch: no source has a build".into()));
    }
}
