//! Presence: where users are. The server (gameId) is only filled in where
//! the asking account may join it.

use serde::Deserialize;
use serde_json::json;

use super::http::{self, Request, Transport};
use super::{Presence, RobloxError};
use crate::types::{Cookie, PlaceId, ServerId, UserId};

/// The user ids per request the presence API takes.
const BATCH: usize = 50;

/// One user's entry, as the presence API sends it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Entry {
    #[serde(default)]
    pub user_presence_type: u8,
    pub user_id: Option<u64>,
    pub place_id: Option<u64>,
    pub game_id: Option<String>,
    pub last_location: Option<String>,
}

impl Entry {
    pub const ONLINE: u8 = 1;
    pub const IN_GAME: u8 = 2;
    pub const IN_STUDIO: u8 = 3;

    pub fn place(&self) -> Option<PlaceId> {
        self.place_id.and_then(|p| PlaceId::parse(&p.to_string()).ok())
    }

    pub fn server(&self) -> Option<ServerId> {
        self.game_id.as_deref().and_then(|g| ServerId::parse(g).ok())
    }
}

/// Presence entries for `users`, asked as the cookie's account.
pub(super) fn entries(
    t: &dyn Transport,
    cookie: &Cookie,
    users: &[UserId],
) -> Result<Vec<Entry>, RobloxError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Page {
        #[serde(default)]
        user_presences: Vec<Entry>,
    }
    let mut out = Vec::new();
    for batch in users.chunks(BATCH) {
        let ids: Vec<u64> = batch.iter().map(|id| id.0).collect();
        let req = Request::post(
            "https://presence.roblox.com/v1/presence/users",
            json!({ "userIds": ids }),
        )
        .as_user(cookie);
        let page: Page = http::ok(http::send(t, req)?, true)?.json()?;
        out.extend(page.user_presences);
    }
    Ok(out)
}

pub(super) fn of_user(
    t: &dyn Transport,
    cookie: &Cookie,
    user: UserId,
) -> Result<Presence, RobloxError> {
    let found = entries(t, cookie, &[user])?;
    Ok(found.first().map(|e| Presence { server: e.server(), place: e.place() }).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roblox::http::canned::Canned;

    #[test]
    fn a_users_server_and_place_are_read_from_presence() {
        let t = Canned::new().answer(
            200,
            r#"{"userPresences": [{"userPresenceType": 2, "userId": 5, "placeId": 1818, "gameId": "abc-1"}]}"#,
        );
        let p = of_user(&t, &Cookie::new("c"), UserId(5)).unwrap();
        assert_eq!(p.server.unwrap().as_str(), "abc-1");
        assert_eq!(p.place.unwrap().as_str(), "1818");
        assert_eq!(t.asked()[0].url, "https://presence.roblox.com/v1/presence/users");
    }

    #[test]
    fn nobody_found_is_an_empty_presence() {
        let t = Canned::new().answer(200, r#"{"userPresences": []}"#);
        assert_eq!(of_user(&t, &Cookie::new("c"), UserId(5)).unwrap(), Presence::default());
    }

    #[test]
    fn a_hidden_server_is_no_server() {
        let t = Canned::new().answer(200, r#"{"userPresences": [{"userPresenceType": 2, "userId": 5, "placeId": 1, "gameId": null}]}"#);
        assert_eq!(of_user(&t, &Cookie::new("c"), UserId(5)).unwrap().server, None);
    }

    #[test]
    fn many_users_are_asked_for_in_batches_of_50() {
        let users: Vec<UserId> = (1..=120).map(UserId).collect();
        let t = Canned::new()
            .answer(200, r#"{"userPresences": []}"#)
            .answer(200, r#"{"userPresences": []}"#)
            .answer(200, r#"{"userPresences": []}"#);
        entries(&t, &Cookie::new("c"), &users).unwrap();
        assert_eq!(t.asked().len(), 3);
    }
}
