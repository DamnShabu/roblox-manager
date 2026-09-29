//! Friends and where they are now. Three public web APIs: the friend list
//! (ids only -- Roblox blanks the names there), their presence, then names.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use super::http::{self, Request, Transport};
use super::presence::{self, Entry};
use super::{RobloxError, users};
use crate::types::{Cookie, PlaceId, ServerId, UserId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FriendState {
    /// In a game whose place shows.
    Game,
    /// Online, in Studio, or in a game whose place is hidden.
    Online,
    Offline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Friend {
    pub id: UserId,
    pub name: String,
    pub display: String,
    pub state: FriendState,
    pub place: Option<PlaceId>,
    /// Their server, when their privacy lets the asking account join it.
    pub server: Option<ServerId>,
    /// The game's name as presence reports it.
    pub game: Option<String>,
}

pub(super) fn of_user(
    t: &dyn Transport,
    cookie: &Cookie,
    user: UserId,
) -> Result<Vec<Friend>, RobloxError> {
    #[derive(Deserialize)]
    struct Listed {
        id: Option<u64>,
    }
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        data: Vec<Listed>,
    }
    let req =
        Request::get(format!("https://friends.roblox.com/v1/users/{user}/friends")).as_user(cookie);
    let page: Page = http::ok(http::send(t, req)?, true)?.json()?;
    let ids: Vec<UserId> = page.data.iter().filter_map(|f| f.id).map(UserId).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let entries = presence::entries(t, cookie, &ids)?;
    let playing: HashMap<UserId, &Entry> = entries
        .iter()
        .filter(|e| e.user_presence_type == Entry::IN_GAME && e.place_id.is_some())
        .filter_map(|e| Some((UserId(e.user_id?), e)))
        .collect();
    let online: HashSet<UserId> = entries
        .iter()
        .filter(|e| {
            matches!(e.user_presence_type, Entry::ONLINE | Entry::IN_GAME | Entry::IN_STUDIO)
        })
        .filter_map(|e| e.user_id.map(UserId))
        .collect();
    let names = users::names(t, &ids)?;
    let mut friends: Vec<Friend> = ids
        .into_iter()
        .map(|id| {
            let game = playing.get(&id);
            let state = match (game, online.contains(&id)) {
                (Some(_), _) => FriendState::Game,
                (None, true) => FriendState::Online,
                (None, false) => FriendState::Offline,
            };
            let name = names.get(&id).map_or_else(|| format!("user {id}"), |u| u.name.clone());
            let display = names.get(&id).map_or_else(|| name.clone(), |u| u.display().to_owned());
            Friend {
                id,
                name,
                display,
                state,
                place: game.and_then(|e| e.place()),
                server: game.and_then(|e| e.server()),
                game: game.map(|e| {
                    e.last_location
                        .clone()
                        .unwrap_or_else(|| format!("Place {}", e.place_id.unwrap_or_default()))
                }),
            }
        })
        .collect();
    friends.sort_by_cached_key(|f| (f.state, f.display.to_lowercase()));
    Ok(friends)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roblox::http::canned::Canned;

    #[test]
    fn friends_are_ranked_game_then_online_then_offline_then_by_name() {
        let t = Canned::new()
            .answer(200, r#"{"data": [{"id": 1}, {"id": 2}, {"id": 3}, {"id": 4}, {"id": 5}]}"#)
            .answer(
                200,
                r#"{"userPresences": [
                    {"userPresenceType": 2, "userId": 2, "placeId": 99, "gameId": "s-1", "lastLocation": "Obby"},
                    {"userPresenceType": 3, "userId": 3},
                    {"userPresenceType": 1, "userId": 4},
                    {"userPresenceType": 2, "userId": 5}
                ]}"#,
            )
            .answer(
                200,
                r#"{"data": [
                    {"id": 1, "name": "zed", "displayName": "Zed"},
                    {"id": 2, "name": "amy", "displayName": "Amy"},
                    {"id": 3, "name": "bo", "displayName": "bo"},
                    {"id": 4, "name": "al", "displayName": "Al"}
                ]}"#,
            );
        let got = of_user(&t, &Cookie::new("c"), UserId(9)).unwrap();
        let order: Vec<(u64, FriendState)> = got.iter().map(|f| (f.id.0, f.state)).collect();
        assert_eq!(
            order,
            vec![
                (2, FriendState::Game),
                (4, FriendState::Online),
                (3, FriendState::Online),
                (5, FriendState::Online),
                (1, FriendState::Offline),
            ]
        );
        assert_eq!(got[0].server.as_ref().unwrap().as_str(), "s-1");
        assert_eq!(got[0].game.as_deref(), Some("Obby"));
        assert_eq!(got[3].name, "user 5");
        assert_eq!(t.asked()[0].url, "https://friends.roblox.com/v1/users/9/friends");
    }

    #[test]
    fn no_friends_asks_nothing_more() {
        let t = Canned::new().answer(200, r#"{"data": []}"#);
        assert!(of_user(&t, &Cookie::new("c"), UserId(9)).unwrap().is_empty());
        assert_eq!(t.asked().len(), 1);
    }
}
