//! Favourite games: each account's own, and one strip merged from them all.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::RobloxError;
use super::http::{self, Request, Transport};
use crate::types::{Cookie, PlaceId, UserId};

/// How many games the strip shows.
pub const FAVORITES_SHOWN: usize = 24;

/// A game as the strip shows it, and as accounts.json keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub universe_id: String,
    pub place_id: PlaceId,
    pub name: String,
}

/// The account's favourited games, most recently favourited first. Roblox's
/// page size only takes 10/25/50/100, so a page of 50 is asked for and cut
/// down here. Games with no root place are dropped: the launcher joins a
/// place, so one it cannot open would only ever be a tile that fails.
pub(super) fn of_user(
    t: &dyn Transport,
    cookie: &Cookie,
    user: UserId,
    limit: usize,
) -> Result<Vec<Game>, RobloxError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Favorite {
        id: Option<u64>,
        name: Option<String>,
        root_place: Option<Root>,
    }
    #[derive(Deserialize)]
    struct Root {
        id: Option<u64>,
    }
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        data: Vec<Favorite>,
    }
    let url =
        format!("https://games.roblox.com/v2/users/{user}/favorite/games?limit=50&sortOrder=Desc");
    let page: Page = http::ok(http::send(t, Request::get(url).as_user(cookie))?, true)?.json()?;
    Ok(page
        .data
        .into_iter()
        .filter_map(|f| {
            let root = f.root_place.and_then(|r| r.id)?;
            Some(Game {
                universe_id: f.id.map(|id| id.to_string()).unwrap_or_default(),
                place_id: PlaceId::parse(&root.to_string()).ok()?,
                name: f.name.unwrap_or_else(|| format!("Place {root}")),
            })
        })
        .take(limit)
        .collect())
}

/// One account's side of the merge: its favourites, and how often each
/// place (by id) was launched from here.
pub struct AccountGames<'a> {
    pub favorites: &'a [Game],
    pub plays: &'a BTreeMap<String, u64>,
}

/// Every account's favourites in one strip, best first: by how often the game
/// was launched from here, then by how many accounts favourited it, then by
/// how near the top of a list it sat, then by name.
pub fn merge(accounts: &[AccountGames<'_>], limit: usize) -> Vec<Game> {
    struct Ranked<'a> {
        game: &'a Game,
        accounts: usize,
        best_position: usize,
    }
    let mut plays: HashMap<&str, u64> = HashMap::new();
    for a in accounts {
        for (place, n) in a.plays {
            *plays.entry(place.as_str()).or_default() += n;
        }
    }
    let mut merged: Vec<Ranked<'_>> = Vec::new();
    let mut index: HashMap<&PlaceId, usize> = HashMap::new();
    for a in accounts {
        for (pos, game) in a.favorites.iter().enumerate() {
            match index.get(&game.place_id) {
                Some(&i) => {
                    merged[i].accounts += 1;
                    merged[i].best_position = merged[i].best_position.min(pos);
                }
                None => {
                    index.insert(&game.place_id, merged.len());
                    merged.push(Ranked { game, accounts: 1, best_position: pos });
                }
            }
        }
    }
    let played = |r: &Ranked<'_>| plays.get(r.game.place_id.as_str()).copied().unwrap_or(0);
    merged.sort_by(|a, b| {
        played(b)
            .cmp(&played(a))
            .then(b.accounts.cmp(&a.accounts))
            .then(a.best_position.cmp(&b.best_position))
            .then(a.game.name.cmp(&b.game.name))
    });
    merged.into_iter().take(limit).map(|r| r.game.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roblox::http::canned::Canned;

    fn game(place: &str, name: &str) -> Game {
        Game {
            universe_id: format!("u{place}"),
            place_id: PlaceId::parse(place).unwrap(),
            name: name.into(),
        }
    }

    #[test]
    fn favourites_without_a_root_place_are_dropped_and_the_list_is_cut() {
        let t = Canned::new().answer(
            200,
            r#"{"data": [
                {"id": 11, "name": "A", "rootPlace": {"id": 1}},
                {"id": 12, "name": "No place", "rootPlace": null},
                {"id": 13, "rootPlace": {"id": 3}},
                {"id": 14, "name": "D", "rootPlace": {"id": 4}}
            ]}"#,
        );
        let got = of_user(&t, &Cookie::new("c"), UserId(9), 2).unwrap();
        assert_eq!(
            got,
            vec![
                Game { universe_id: "11".into(), ..game("1", "A") },
                Game { universe_id: "13".into(), ..game("3", "Place 3") }
            ]
        );
        assert_eq!(
            t.asked()[0].url,
            "https://games.roblox.com/v2/users/9/favorite/games?limit=50&sortOrder=Desc"
        );
    }

    #[test]
    fn the_merge_ranks_plays_then_accounts_then_position_then_name() {
        let a = [game("1", "one"), game("2", "two"), game("3", "three")];
        let b = [game("3", "three"), game("4", "four")];
        let c = [game("5", "b-five"), game("6", "a-six")];
        let plays_a: BTreeMap<String, u64> = [("4".to_string(), 2)].into();
        let none = BTreeMap::new();
        let merged = merge(
            &[
                AccountGames { favorites: &a, plays: &plays_a },
                AccountGames { favorites: &b, plays: &none },
                AccountGames { favorites: &c, plays: &none },
            ],
            10,
        );
        let order: Vec<&str> = merged.iter().map(|g| g.place_id.as_str()).collect();
        // 4 was played; 3 is in two lists; then by best position, then name.
        assert_eq!(order, vec!["4", "3", "5", "1", "6", "2"]);
    }

    #[test]
    fn the_merge_keeps_only_the_limit() {
        let a = [game("1", "a"), game("2", "b")];
        let none = BTreeMap::new();
        assert_eq!(merge(&[AccountGames { favorites: &a, plays: &none }], 1).len(), 1);
    }

    #[test]
    fn a_game_reads_and_writes_the_accounts_json_shape() {
        let g: Game = serde_json::from_str(
            r#"{"universe_id": "648454481", "place_id": "1730877806", "name": "GPO"}"#,
        )
        .unwrap();
        assert_eq!(g.place_id.as_str(), "1730877806");
        assert_eq!(
            serde_json::to_string(&g).unwrap(),
            r#"{"universe_id":"648454481","place_id":"1730877806","name":"GPO"}"#
        );
    }
}
