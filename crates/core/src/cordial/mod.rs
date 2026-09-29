//! Cordial, the Roblox runtime: this repo's fork, as two programs on PATH.
//! `cordial-run` is one account's game client and `cordial-fetch` installs
//! the Roblox build it runs. One Cordial profile per account; the manager
//! gives each its session and starts it with the game's own deep link.

pub mod build;
pub mod clients;
pub mod engine;
pub mod process;
pub mod session;

pub use build::{Build, roblox_build};
pub use process::{Child, Output, Runner, SystemRunner};

use crate::keyring::KeyringError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CordialError {
    #[error("{0}")]
    Io(String),
    #[error(transparent)]
    Keyring(#[from] KeyringError),
    #[error("{0}")]
    Process(String),
    #[error("its client exited at once ({reason}); log: {log}")]
    ExitedAtOnce { reason: String, log: String },
    #[error("could not install Roblox: {0}")]
    Build(String),
}
