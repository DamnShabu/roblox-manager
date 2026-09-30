//! Everything roblox-manager does except drawing.
//!
//! Each area is a directory; every seam into the outside world (the keyring,
//! Roblox's web API, processes, the Wayland display) is a trait with a
//! production adapter and the one the tests use.

pub mod accounts;
pub mod cordial;
pub mod desktop;
pub mod json_file;
pub mod keyring;
pub mod launch;
pub mod macros;
pub mod paths;
pub mod roblox;
pub mod stop;
pub mod types;
pub mod window_state;

pub use keyring::{Keyring, KeyringError};
pub use paths::Paths;
pub use types::{Cookie, InvalidId, InvalidLabel, Label, PlaceId, Profile, ServerId, User, UserId};
