//! Advanced macros: Python scripts kept in a folder, each run as a process
//! of its own for every account it is started on.
//!
//! A script never touches the client either. It asks the manager -- one
//! JSON line at a time, over its stdin and stdout ([`serve`]) -- and the
//! manager presses keys through the same virtual keyboard and pointer a
//! plain macro plays through, and looks through the same copy of the frame
//! a `when` looks at. The `rbxmgr` module the script imports
//! (`rbxmgr.py`, beside this file) is that conversation in Python.
//!
//! Stop ends the process wherever it is, and lets go of whatever it left
//! down.

mod python;
mod serve;

pub use python::Python;
pub use serve::Session;

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use super::MacroError;
use super::player::Input;
use super::sight::{Eyes, Image};
use crate::stop::StopFlag;

/// The `rbxmgr` module a script imports.
pub const HELPER: &str = include_str!("rbxmgr.py");
/// What a new advanced macros folder starts with, to read and copy from.
pub const EXAMPLE: &str = include_str!("example.py");
const EXAMPLE_NAME: &str = "example.py";

/// What a script's run is called where macros are named: its file's name,
/// `.py` and all, so it is never taken for a plain macro of the same name.
pub fn run_name(script: &str) -> String {
    format!("{script}.py")
}

/// One script in the folder: its name (the file's, without `.py`), where
/// it is, and what its first comment says it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub name: String,
    pub path: PathBuf,
    pub about: Option<String>,
}

/// The scripts in `dir`, by name: every `.py` file directly in it, but for
/// one whose name starts with `_` or `.` -- a module the scripts share.
/// A folder that is not there yet has none.
pub fn scripts(dir: &Path) -> io::Result<Vec<Script>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut found = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(stem) = name.strip_suffix(".py") else { continue };
        if stem.is_empty() || stem.starts_with(['_', '.']) || !path.is_file() {
            continue;
        }
        let about = fs::read_to_string(&path).ok().and_then(|text| about(&text));
        found.push(Script { name: stem.to_owned(), path, about });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// What a script says it does: its first comment line, or the first line
/// of its docstring -- whichever comes first, past a `#!` line.
pub fn about(text: &str) -> Option<String> {
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with("#!") || line.starts_with("# -*-") {
            continue;
        }
        let said = match line.strip_prefix('#') {
            Some(comment) => comment,
            None => line
                .strip_prefix("\"\"\"")
                .or_else(|| line.strip_prefix("'''"))?
                .trim_end_matches("\"\"\"")
                .trim_end_matches("'''"),
        };
        let said = said.trim();
        return (!said.is_empty()).then(|| said.to_owned());
    }
    None
}

/// Make the folder if it is not there, with the example in it to start
/// from. One already there is left as it is.
pub fn prepare(dir: &Path) -> io::Result<()> {
    if dir.exists() {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    fs::write(dir.join(EXAMPLE_NAME), EXAMPLE)
}

/// A script started: what it asks on, where the answers go, what it says
/// (its stderr, print() and all), and how to end it.
pub struct Process {
    pub requests: Box<dyn BufRead + Send>,
    pub replies: Box<dyn Write + Send>,
    pub output: Box<dyn BufRead + Send>,
    /// Ask it to end (false), or end it outright (true): it and anything it
    /// started.
    pub end: Box<dyn Fn(bool) + Send + Sync>,
    /// Wait for it to have ended: Ok(None) if it ended cleanly, else how it
    /// did not.
    pub wait: Box<dyn FnOnce() -> io::Result<Option<String>> + Send>,
}

/// What runs a script. Adapters: [`Python`], and a script played from
/// Rust in the tests.
pub trait Interpreter {
    /// Start `script`, with `env` added to its environment.
    fn start(&self, script: &Path, env: &[(&str, String)]) -> io::Result<Process>;
}

/// How long a script asked to end has before it is ended outright.
const GRACE: Duration = Duration::from_secs(1);
/// How many of its last lines a script that fails is told by.
const LAST_SAID: usize = 1;

/// Everything a running script reaches outside itself.
pub struct ScriptRun<'a> {
    /// The client's display link (`nested::display_file`).
    pub display: &'a Path,
    /// Whether the client is still up.
    pub running: &'a dyn Fn() -> bool,
    pub connect: &'a dyn Fn(&Path) -> io::Result<Box<dyn Input>>,
    /// Eyes on the display at the path.
    pub open: &'a dyn Fn(&Path) -> io::Result<Box<dyn Eyes + Send>>,
    /// A picked image by name, and every name there is.
    pub image: &'a dyn Fn(&str) -> Result<Image, String>,
    pub image_names: &'a [String],
    /// Hears every line the script prints.
    pub report: &'a (dyn Fn(String) + Sync),
    pub interpreter: &'a dyn Interpreter,
    /// Its account: the name shown, and the Roblox user id.
    pub account: (&'a str, u64),
}

impl ScriptRun<'_> {
    /// Run `script` until it ends or `stop` is set. Whatever ends it,
    /// nothing it pressed is left down.
    pub fn run(&self, script: &Path, stop: &StopFlag) -> Result<(), MacroError> {
        if !(self.running)() {
            return Err(MacroError::NotRunning);
        }
        let unreachable = |e: io::Error| match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => MacroError::NotNested,
            _ => MacroError::WentAway(e.to_string()),
        };
        let mut input = (self.connect)(self.display).map_err(unreachable)?;
        let env = [
            ("RBXMGR_ACCOUNT", self.account.0.to_owned()),
            ("RBXMGR_USER_ID", self.account.1.to_string()),
        ];
        let process = self
            .interpreter
            .start(script, &env)
            .map_err(|e| MacroError::Script(format!("could not start it: {e}")))?;
        let Process { mut requests, mut replies, output, end, wait } = process;
        let open = || (self.open)(self.display);
        let done = StopFlag::default();
        let (served, said) = thread::scope(|s| {
            let end = &end;
            let done_ref = &done;
            // Stop ends the script wherever it is, a sleep or a loop of
            // its own included: asked first, then outright.
            s.spawn(move || {
                while !done_ref.is_set() {
                    if stop.wait(0.1) {
                        end(false);
                        if !done_ref.wait(GRACE.as_secs_f64()) {
                            end(true);
                        }
                        return;
                    }
                }
            });
            let report = self.report;
            let said = s.spawn(move || {
                let mut last = Vec::new();
                for line in output.lines() {
                    let Ok(line) = line else { break };
                    if line.trim().is_empty() {
                        continue;
                    }
                    report(line.clone());
                    last.push(line);
                    if last.len() > LAST_SAID {
                        last.remove(0);
                    }
                }
                last
            });
            let mut session = Session::new(
                input.as_mut(),
                &open,
                self.image,
                self.image_names,
                self.running,
                stop,
            );
            let served = session.serve(requests.as_mut(), replies.as_mut());
            let let_go = session.release_all();
            // However the conversation ended, the script ends with it: one
            // the display failed under has nothing left to play into.
            drop(replies);
            if served.is_err() || let_go.is_err() || stop.is_set() {
                end(true);
            }
            let ended = wait();
            done.set();
            let said = said.join().unwrap_or_default();
            (served.and(let_go).and(ended), said)
        });
        match served {
            Err(e) => Err(MacroError::WentAway(e.to_string())),
            Ok(_) if stop.is_set() => Ok(()),
            Ok(None) => Ok(()),
            Ok(Some(how)) => Err(MacroError::Script(said.last().cloned().unwrap_or(how))),
        }
    }
}

#[cfg(test)]
mod tests;
