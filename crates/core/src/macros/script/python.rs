//! Running a script with the system's Python.

use std::fs;
use std::io::{self, BufReader};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

use rustix::process::{Pid, Signal, kill_process_group};

use super::{HELPER, Interpreter, Process};

/// Run before the script: the helper imported first, so the conversation
/// is on its own descriptors before anything the script does can print,
/// then the script as `__main__`, from its own folder, as `python3 it.py`
/// would run it.
const BOOT: &str = "import os, runpy, sys
script, helper = sys.argv[1], sys.argv[2]
sys.path.insert(0, helper)
import rbxmgr
sys.argv = [script]
sys.path.insert(0, os.path.dirname(script))
runpy.run_path(script, run_name='__main__')
";

/// The system's `python3`, with the `rbxmgr` module kept in `helper_dir`.
pub struct Python {
    pub program: PathBuf,
    pub helper_dir: PathBuf,
}

impl Python {
    pub fn new(helper_dir: PathBuf) -> Self {
        Python { program: PathBuf::from("python3"), helper_dir }
    }

    /// The helper as this version has it: written again whenever what is
    /// there differs, so an update reaches scripts at their next run.
    fn write_helper(&self) -> io::Result<()> {
        let file = self.helper_dir.join("rbxmgr.py");
        if fs::read_to_string(&file).is_ok_and(|there| there == HELPER) {
            return Ok(());
        }
        fs::create_dir_all(&self.helper_dir)?;
        fs::write(file, HELPER)
    }
}

impl Interpreter for Python {
    fn start(&self, script: &Path, env: &[(&str, String)]) -> io::Result<Process> {
        self.write_helper()?;
        let mut command = Command::new(&self.program);
        command
            .arg("-u")
            .arg("-c")
            .arg(BOOT)
            .arg(script)
            .arg(&self.helper_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // A group of its own, so ending it ends whatever it started too.
            .process_group(0)
            .envs(env.iter().map(|(k, v)| (k, v)));
        if let Some(dir) = script.parent() {
            command.current_dir(dir);
        }
        let mut child = command.spawn().map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => {
                io::Error::new(e.kind(), "python3 is not installed, or not on PATH")
            }
            _ => e,
        })?;
        let piped = || io::Error::other("its pipes were not made");
        let requests = BufReader::new(child.stdout.take().ok_or_else(piped)?);
        let replies = child.stdin.take().ok_or_else(piped)?;
        let output = BufReader::new(child.stderr.take().ok_or_else(piped)?);
        let group = Pid::from_child(&child);
        // Once reaped its group may be another's: ending it then sends nothing.
        let reaped = Arc::new(Mutex::new(false));
        let gone = reaped.clone();
        let end = move |outright: bool| {
            let reaped = gone.lock().unwrap_or_else(PoisonError::into_inner);
            if !*reaped {
                // One already gone has nothing to end.
                let _ =
                    kill_process_group(group, if outright { Signal::KILL } else { Signal::TERM });
            }
        };
        let wait = move || {
            let status = child.wait()?;
            *reaped.lock().unwrap_or_else(PoisonError::into_inner) = true;
            Ok((!status.success()).then(|| status.to_string()))
        };
        Ok(Process {
            requests: Box::new(requests),
            replies: Box::new(replies),
            output: Box::new(output),
            end: Box::new(end),
            wait: Box::new(wait),
        })
    }
}
