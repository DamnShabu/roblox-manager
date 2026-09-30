//! What a place is: its game's name, who made it and how many play it now.
//! Two public APIs, no cookie: the place's universe, then the universe.

use serde::Deserialize;

use super::RobloxError;
use super::http::{self, Request, Transport};
use crate::types::PlaceId;

/// A place's game, as a link's popup shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceDetails {
    /// What its icon is filed under.
    pub universe_id: String,
    pub name: String,
    /// The user or group that made it.
    pub creator: String,
    /// Players in it right now.
    pub playing: u64,
}

pub(super) fn details(t: &dyn Transport, place: &PlaceId) -> Result<PlaceDetails, RobloxError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Universe {
        universe_id: Option<u64>,
    }
    #[derive(Deserialize)]
    struct Creator {
        name: Option<String>,
    }
    #[derive(Deserialize)]
    struct Game {
        name: Option<String>,
        creator: Option<Creator>,
        playing: Option<u64>,
    }
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        data: Vec<Game>,
    }
    let url = format!("https://apis.roblox.com/universes/v1/places/{place}/universe");
    let universe: Universe = http::ok(http::send(t, Request::get(url))?, false)?.json()?;
    let universe = universe.universe_id.ok_or_else(|| {
        RobloxError::BadResponse(format!("place {place} belongs to no game Roblox shows"))
    })?;
    let url = format!("https://games.roblox.com/v1/games?universeIds={universe}");
    let page: Page = http::ok(http::send(t, Request::get(url))?, false)?.json()?;
    let game = page.data.into_iter().next().ok_or_else(|| {
        RobloxError::BadResponse(format!("no game {universe} in Roblox's answer"))
    })?;
    Ok(PlaceDetails {
        universe_id: universe.to_string(),
        name: game.name.unwrap_or_else(|| format!("Place {place}")),
        creator: game.creator.and_then(|c| c.name).unwrap_or_default(),
        playing: game.playing.unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roblox::http::canned::Canned;

    #[test]
    fn a_place_is_looked_up_through_its_universe() {
        let t = Canned::new().answer(200, r#"{"universeId": 648454481}"#).answer(
            200,
            r#"{"data": [{"id": 648454481, "rootPlaceId": 1730877806, "name": "Grand Piece Online",
                "creator": {"id": 1, "name": "Grand Quest Games", "type": "Group"}, "playing": 18204}]}"#,
        );
        let place = PlaceId::parse("1730877806").unwrap();
        assert_eq!(
            details(&t, &place).unwrap(),
            PlaceDetails {
                universe_id: "648454481".into(),
                name: "Grand Piece Online".into(),
                creator: "Grand Quest Games".into(),
                playing: 18204,
            }
        );
        let asked: Vec<String> = t.asked().into_iter().map(|r| r.url).collect();
        assert_eq!(
            asked,
            [
                "https://apis.roblox.com/universes/v1/places/1730877806/universe",
                "https://games.roblox.com/v1/games?universeIds=648454481"
            ]
        );
        assert!(t.asked().iter().all(|r| r.cookie.is_none()), "public: no cookie is sent");
    }

    #[test]
    fn a_place_with_no_game_is_an_error() {
        let t = Canned::new().answer(200, r#"{"universeId": null}"#);
        let place = PlaceId::parse("1").unwrap();
        assert!(matches!(details(&t, &place), Err(RobloxError::BadResponse(_))));
    }
}
