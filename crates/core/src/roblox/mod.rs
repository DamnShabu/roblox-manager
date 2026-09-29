//! Roblox's own web APIs: who a session belongs to, where users are, friends,
//! favourites, icons, and the Quick Login flow. A handful of requests, not a
//! client library.

mod friends;
pub mod http;
mod presence;
mod users;

use std::sync::Arc;

use crate::types::{Cookie, PlaceId, ServerId, User, UserId};

pub use friends::{Friend, FriendState};
pub use http::{Transport, UreqTransport};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RobloxError {
    #[error(
        "the stored session has expired or was signed out -- use 'Sign in again' on the account"
    )]
    Expired,
    #[error("{detail}")]
    Http { status: u16, detail: String },
    #[error("could not reach Roblox: {0}")]
    Offline(String),
    #[error("Roblox sent something unexpected: {0}")]
    BadResponse(String),
}

/// Where a user is, as far as the asking account may see.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Presence {
    /// Their server, when they are in one the asker may join.
    pub server: Option<ServerId>,
    pub place: Option<PlaceId>,
}

/// What the rest of the manager asks Roblox. Adapters: [`HttpRoblox`], and
/// fakes in the launch tests.
pub trait Roblox: Send + Sync {
    /// Whose session this is. [`RobloxError::Expired`] when Roblox refuses it.
    fn whoami(&self, cookie: &Cookie) -> Result<User, RobloxError>;
    /// Where `user` is, asked as the cookie's account.
    fn presence(&self, cookie: &Cookie, user: UserId) -> Result<Presence, RobloxError>;
    /// Every friend of `user` and where they are now: in a game first, then
    /// online, then offline, each by display name.
    fn friends(&self, cookie: &Cookie, user: UserId) -> Result<Vec<Friend>, RobloxError>;
}

/// Roblox over HTTPS.
pub struct HttpRoblox {
    transport: Arc<dyn Transport>,
}

impl HttpRoblox {
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        HttpRoblox { transport }
    }
}

impl Roblox for HttpRoblox {
    fn whoami(&self, cookie: &Cookie) -> Result<User, RobloxError> {
        users::whoami(&*self.transport, cookie)
    }

    fn presence(&self, cookie: &Cookie, user: UserId) -> Result<Presence, RobloxError> {
        presence::of_user(&*self.transport, cookie, user)
    }

    fn friends(&self, cookie: &Cookie, user: UserId) -> Result<Vec<Friend>, RobloxError> {
        friends::of_user(&*self.transport, cookie, user)
    }
}
