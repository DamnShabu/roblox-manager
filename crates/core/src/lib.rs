//! Everything roblox-manager does except drawing.
//!
//! Each area is a directory; every seam into the outside world (the keyring,
//! Roblox's web API, processes, the Wayland display) is a trait with a
//! production adapter and the one the tests use.

pub mod json_file;
pub mod paths;
pub mod types;

pub use paths::Paths;
pub use types::{Cookie, InvalidId, InvalidLabel, Label, PlaceId, Profile, ServerId, User, UserId};
