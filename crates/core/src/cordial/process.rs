//! Running programs: the seam between the manager and cordial-run,
//! cordial-fetch, pgrep and kill.

use std::fs::File;
use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::CordialError;

/// What a finished program said.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    /// The exit status; -1 when it ended on a signal.
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    pub fn success(&self) -> bool {
        self.status == 0
    }
}

/// The last non-empty line of some output, for an error message.
pub fn last_line(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).trim().lines().last().unwrap_or_default().to_owned()
}

/// A program started to keep running.
pub trait Child: Send {
    /// Its exit status once it has ended (None while it runs), without
    /// waiting. An error means its state could not be read.
    fn exited(&mut self) -> Result<Option<i32>, CordialError>;
}

/// How programs are run. Adapters: [`SystemRunner`], and a recording fake in
/// the self-check.
pub trait Runner: Send + Sync {
    /// Run to completion, with no stdin, capturing its output. A program
    /// still running after `timeout` is killed and is an error.
    fn run(&self, argv: &[String], timeout: Duration) -> Result<Output, CordialError>;
    /// Start a long-running program in a process group of its own, its output
    /// going to `log`, with `env` added to this process's environment.
    fn spawn(
        &self,
        argv: &[String],
        log: File,
        env: &[(String, String)],
    ) -> Result<Box<dyn Child>, CordialError>;
}

/// Real processes.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, argv: &[String], timeout: Duration) -> Result<Output, CordialError> {
        let (program, args) = argv.split_first().ok_or_else(|| process_error("nothing to run"))?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| process_error(&format!("could not run {program}: {e}")))?;
        // Read both pipes while it runs, so a chatty program never blocks on
        // a full pipe.
        let drain = |pipe: Option<Box<dyn Read + Send>>| {
            thread::spawn(move || {
                let mut out = Vec::new();
                if let Some(mut p) = pipe {
                    let _ = p.read_to_end(&mut out);
                }
                out
            })
        };
        let stdout = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let stderr = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(process_error(&format!(
                        "{program} did not finish within {}s",
                        timeout.as_secs()
                    )));
                }
                Err(e) => return Err(process_error(&format!("waiting for {program}: {e}"))),
            }
        };
        Ok(Output {
            status: status.code().unwrap_or(-1),
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        })
    }

    fn spawn(
        &self,
        argv: &[String],
        log: File,
        env: &[(String, String)],
    ) -> Result<Box<dyn Child>, CordialError> {
        use std::os::unix::process::CommandExt;
        let (program, args) = argv.split_first().ok_or_else(|| process_error("nothing to run"))?;
        let err_log = log.try_clone().map_err(|e| CordialError::Io(e.to_string()))?;
        let child = Command::new(program)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(err_log)
            .process_group(0)
            .spawn()
            .map_err(|e| process_error(&format!("could not start {program}: {e}")))?;
        Ok(Box::new(SystemChild(Some(child))))
    }
}

/// A started program. Dropped while it still runs, it is waited for on a
/// thread, so it never lingers as a zombie.
struct SystemChild(Option<std::process::Child>);

impl Child for SystemChild {
    fn exited(&mut self) -> Result<Option<i32>, CordialError> {
        let Some(child) = self.0.as_mut() else { return Ok(None) };
        let status = child
            .try_wait()
            .map_err(|e| process_error(&format!("could not check the client: {e}")))?;
        Ok(status.map(|s| s.code().unwrap_or(-1)))
    }
}

impl Drop for SystemChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            thread::spawn(move || child.wait());
        }
    }
}

fn process_error(msg: &str) -> CordialError {
    CordialError::Process(msg.to_owned())
}

#[cfg(test)]
pub(crate) mod recording {
    //! A runner that answers from a script and remembers every command.

    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;

    /// One `spawn`: its argv and the environment it was given.
    pub type Spawned = (Vec<String>, Vec<(String, String)>);

    #[derive(Default)]
    pub struct Recording {
        /// Answers for `run`, in order; when none is left, success with no output.
        answers: Mutex<VecDeque<Output>>,
        /// What `spawn`ed children report from `exited`.
        pub child_exit: Mutex<Option<i32>>,
        /// When set, `exited` fails with this instead.
        pub child_error: Mutex<Option<String>>,
        /// When set, every `pgrep` answers this (its running clients),
        /// leaving the queued answers for other commands.
        pub pgrep: Mutex<Option<String>>,
        pub ran: Mutex<Vec<Vec<String>>>,
        pub spawned: Mutex<Vec<Spawned>>,
    }

    impl Recording {
        pub fn answer(self, status: i32, stdout: &str, stderr: &str) -> Self {
            self.answers.lock().unwrap().push_back(Output {
                status,
                stdout: stdout.into(),
                stderr: stderr.into(),
            });
            self
        }

        pub fn ran(&self) -> Vec<Vec<String>> {
            self.ran.lock().unwrap().clone()
        }

        pub fn spawned(&self) -> Vec<Spawned> {
            self.spawned.lock().unwrap().clone()
        }
    }

    struct Exits(Result<Option<i32>, String>);

    impl Child for Exits {
        fn exited(&mut self) -> Result<Option<i32>, CordialError> {
            self.0.clone().map_err(CordialError::Process)
        }
    }

    impl Runner for Recording {
        fn run(&self, argv: &[String], _timeout: Duration) -> Result<Output, CordialError> {
            self.ran.lock().unwrap().push(argv.to_vec());
            if argv.first().is_some_and(|p| p == "pgrep") {
                if let Some(clients) = self.pgrep.lock().unwrap().clone() {
                    return Ok(Output {
                        status: 0,
                        stdout: clients.into_bytes(),
                        stderr: Vec::new(),
                    });
                }
            }
            Ok(self.answers.lock().unwrap().pop_front().unwrap_or_default())
        }

        fn spawn(
            &self,
            argv: &[String],
            _log: File,
            env: &[(String, String)],
        ) -> Result<Box<dyn Child>, CordialError> {
            self.spawned.lock().unwrap().push((argv.to_vec(), env.to_vec()));
            let exit = match self.child_error.lock().unwrap().clone() {
                Some(why) => Err(why),
                None => Ok(*self.child_exit.lock().unwrap()),
            };
            Ok(Box::new(Exits(exit)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_program_runs_to_completion_with_its_output() {
        let out = SystemRunner
            .run(&argv(&["sh", "-c", "echo out; echo err >&2; exit 3"]), Duration::from_secs(5))
            .unwrap();
        assert_eq!((out.status, out.stdout, out.stderr), (3, b"out\n".to_vec(), b"err\n".to_vec()));
    }

    #[test]
    fn a_program_that_overruns_is_killed_and_an_error() {
        let err = SystemRunner.run(&argv(&["sleep", "5"]), Duration::from_millis(100)).unwrap_err();
        assert!(err.to_string().contains("did not finish"), "{err}");
    }

    #[test]
    fn a_spawned_program_logs_to_its_file_and_reports_its_exit() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("c.log");
        let env = [("RBX_TEST".to_string(), "hi".to_string())];
        let mut child = SystemRunner
            .spawn(
                &argv(&["sh", "-c", "echo $RBX_TEST; exit 4"]),
                File::create(&log).unwrap(),
                &env,
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(s) = child.exited().unwrap() {
                break s;
            }
            assert!(Instant::now() < deadline, "never exited");
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status, 4);
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "hi\n");
    }

    #[test]
    fn the_last_line_skips_trailing_blank_lines() {
        assert_eq!(last_line(b"a\nb\n\n"), "b");
        assert_eq!(last_line(b""), "");
    }
}
