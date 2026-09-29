//! Macros: emulated input, never your hardware and never the client.
//!
//! A "macro-ready" account's client runs inside its own nested compositor
//! (cage) -- a normal window on your desktop, but with a display of its own,
//! so the game keeps keyboard focus there whatever you are doing elsewhere.
//! A macro types into that display through the Wayland virtual-keyboard and
//! virtual-pointer protocols: input the compositor emulates, delivered like
//! any keyboard's.

pub mod grammar;
pub mod keys;
mod library;
pub mod nested;

pub use grammar::{Macro, ParseError, Row, Step};
pub use library::{MacroLibrary, migrate_legacy};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MacroError {
    /// The name is missing or taken.
    #[error("{0}")]
    Name(String),
    /// Another macro already runs on this hotkey (a GTK accelerator).
    #[error("{hotkey} already runs {by}")]
    HotkeyTaken { hotkey: String, by: String },
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error("could not save the macros: {0}")]
    Io(String),
    #[error("its client is not running -- launch it first")]
    NotRunning,
    #[error("its client is in a normal window -- launch it again with Macro-ready window on")]
    NotNested,
    #[error("its window went away ({0})")]
    WentAway(String),
}
