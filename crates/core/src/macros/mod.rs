//! Macros: emulated input, never your hardware and never the client.
//!
//! A "macro-ready" account's client runs inside its own nested compositor
//! (cage) -- a normal window on your desktop, but with a display of its own,
//! so the game keeps keyboard focus there whatever you are doing elsewhere.
//! A macro types into that display through the Wayland virtual-keyboard and
//! virtual-pointer protocols: input the compositor emulates, delivered like
//! any keyboard's.

pub mod nested;
