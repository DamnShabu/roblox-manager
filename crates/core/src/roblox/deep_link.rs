//! The links a browser hands the app: the website's Play button
//! (`roblox-player:`), Roblox's deep links (`roblox://`), and a game's page on
//! roblox.com. Only what the launcher can do is read out of one -- a place,
//! and a server in it; a link to anything else says so.

use crate::types::{InvalidId, PlaceId, ServerId};

/// A game, or one server of it, that a link asks to join.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinLink {
    pub place: PlaceId,
    /// Set when the link names a server (a "join this server" link).
    pub server: Option<ServerId>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    #[error("this is not a Roblox game link")]
    NotRoblox,
    #[error("the link does not name a game")]
    NoPlace,
    #[error("private server links cannot be joined from here yet")]
    PrivateServer,
    #[error("links that follow a user cannot be joined from here yet")]
    FollowUser,
    #[error(transparent)]
    Invalid(#[from] InvalidId),
}

impl JoinLink {
    pub fn parse(link: &str) -> Result<Self, LinkError> {
        let link = link.trim();
        let (scheme, rest) = link.split_once(':').ok_or(LinkError::NotRoblox)?;
        match scheme.to_ascii_lowercase().as_str() {
            "roblox-player" => from_query(&player_query(rest)),
            "roblox" => {
                let rest = rest.trim_start_matches('/');
                let query = rest.split_once('?').map_or(rest, |(_, q)| q);
                from_query(&pairs(query))
            }
            "https" | "http" => from_web(rest),
            _ => Err(LinkError::NotRoblox),
        }
    }
}

/// The query of the place launcher URL a `roblox-player:` link carries:
/// `roblox-player:1+launchmode:play+...+placelauncherurl:<percent-encoded>+...`.
fn player_query(rest: &str) -> Vec<(String, String)> {
    rest.split('+')
        .filter_map(|part| part.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("placelauncherurl"))
        .map(|(_, url)| {
            let url = percent_decode(url);
            let query = url.split_once('?').map_or("", |(_, q)| q).to_owned();
            pairs(&query)
        })
        .unwrap_or_default()
}

/// `roblox.com/games/<place>/<name>`, or `roblox.com/games/start?placeId=...`.
fn from_web(rest: &str) -> Result<JoinLink, LinkError> {
    let rest = rest.trim_start_matches('/');
    let (host, path) = rest.split_once('/').ok_or(LinkError::NotRoblox)?;
    let host = host.to_ascii_lowercase();
    if host != "roblox.com" && !host.ends_with(".roblox.com") {
        return Err(LinkError::NotRoblox);
    }
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let query = pairs(query.split('#').next().unwrap_or_default());
    let mut segments = path.split('/').filter(|s| !s.is_empty());
    // Some links carry a locale first: /de/games/...
    let games = segments.by_ref().take(2).position(|s| s.eq_ignore_ascii_case("games"));
    if games.is_none() {
        return Err(LinkError::NotRoblox);
    }
    match segments.next() {
        Some(s) if s.eq_ignore_ascii_case("start") => from_query(&query),
        Some(place) => {
            if get(&query, &["privateServerLinkCode"]).is_some() {
                return Err(LinkError::PrivateServer);
            }
            Ok(JoinLink { place: PlaceId::parse(place)?, server: None })
        }
        None => Err(LinkError::NoPlace),
    }
}

/// A link's parameters, whichever spelling it uses for them.
fn from_query(query: &[(String, String)]) -> Result<JoinLink, LinkError> {
    if get(query, &["linkCode", "privateServerLinkCode", "accessCode"]).is_some() {
        return Err(LinkError::PrivateServer);
    }
    let place = get(query, &["placeId"]);
    if place.is_none() && get(query, &["userId"]).is_some() {
        return Err(LinkError::FollowUser);
    }
    let place = PlaceId::parse(place.ok_or(LinkError::NoPlace)?)?;
    let server =
        get(query, &["gameInstanceId", "gameId", "jobId"]).map(ServerId::parse).transpose()?;
    Ok(JoinLink { place, server })
}

/// The first non-empty value of any of `keys`, ignoring case: Roblox writes
/// both `placeId` and `placeID`.
fn get<'a>(query: &'a [(String, String)], keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        query
            .iter()
            .find(|(k, v)| k.eq_ignore_ascii_case(key) && !v.is_empty())
            .map(|(_, v)| v.as_str())
    })
}

fn pairs(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

/// `%XX` escapes decoded; one that is not two hex digits is kept as written.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (
            bytes[i],
            bytes.get(i + 1).copied().and_then(hex),
            bytes.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(place: &str, server: Option<&str>) -> JoinLink {
        JoinLink {
            place: PlaceId::parse(place).unwrap(),
            server: server.map(|s| ServerId::parse(s).unwrap()),
        }
    }

    #[test]
    fn the_websites_play_button_joins_its_place() {
        let url = "roblox-player:1+launchmode:play+gameinfo:SECRET+launchtime:1727000000000\
                   +placelauncherurl:https%3A%2F%2Fassetgame.roblox.com%2Fgame%2FPlaceLauncher.ashx\
                   %3Frequest%3DRequestGame%26browserTrackerId%3D1%26placeId%3D1730877806\
                   %26isPlayTogetherGame%3Dfalse+browsertrackerid:1+robloxLocale:en_us\
                   +gameLocale:en_us+channel:+LaunchExp:InApp";
        assert_eq!(JoinLink::parse(url), Ok(link("1730877806", None)));
    }

    #[test]
    fn a_server_from_the_website_is_its_game_id() {
        let url = "roblox-player:1+launchmode:play+placelauncherurl:https%3A%2F%2Fassetgame.roblox.com\
                   %2Fgame%2FPlaceLauncher.ashx%3Frequest%3DRequestGameJob%26placeId%3D1818\
                   %26gameId%3Dabc-def-123";
        assert_eq!(JoinLink::parse(url), Ok(link("1818", Some("abc-def-123"))));
    }

    #[test]
    fn deep_links_join_a_place_and_maybe_a_server() {
        assert_eq!(JoinLink::parse("roblox://placeId=606849621"), Ok(link("606849621", None)));
        assert_eq!(
            JoinLink::parse("roblox://placeID=1818&gameInstanceId=abc-1"),
            Ok(link("1818", Some("abc-1")))
        );
        assert_eq!(
            JoinLink::parse("roblox://experiences/start?placeId=1818&gameInstanceId=abc-1"),
            Ok(link("1818", Some("abc-1")))
        );
    }

    #[test]
    fn a_game_page_joins_its_place() {
        assert_eq!(
            JoinLink::parse("https://www.roblox.com/games/1730877806/Grand-Piece-Online"),
            Ok(link("1730877806", None))
        );
        assert_eq!(JoinLink::parse("https://www.roblox.com/de/games/1818"), Ok(link("1818", None)));
        assert_eq!(
            JoinLink::parse("https://www.roblox.com/games/start?placeId=1818&gameInstanceId=a-1"),
            Ok(link("1818", Some("a-1")))
        );
    }

    #[test]
    fn links_the_launcher_cannot_follow_say_why() {
        assert_eq!(
            JoinLink::parse("roblox://placeId=1&linkCode=123"),
            Err(LinkError::PrivateServer)
        );
        assert_eq!(
            JoinLink::parse("https://www.roblox.com/games/1/x?privateServerLinkCode=9"),
            Err(LinkError::PrivateServer)
        );
        assert_eq!(JoinLink::parse("roblox://userId=77"), Err(LinkError::FollowUser));
        assert_eq!(JoinLink::parse("roblox-player:1+launchmode:app"), Err(LinkError::NoPlace));
        assert_eq!(JoinLink::parse("https://example.com/games/1"), Err(LinkError::NotRoblox));
        assert_eq!(JoinLink::parse("file:///tmp/x"), Err(LinkError::NotRoblox));
        assert_eq!(JoinLink::parse("nothing"), Err(LinkError::NotRoblox));
    }

    #[test]
    fn nothing_can_be_smuggled_into_the_join_link() {
        assert!(matches!(
            JoinLink::parse("roblox://placeId=1%26x%3D2"),
            Err(LinkError::Invalid(InvalidId::Place(_)))
        ));
        assert!(matches!(
            JoinLink::parse("roblox://placeId=1&gameInstanceId=a%26accessCode%3Db"),
            Err(LinkError::Invalid(InvalidId::Server(_)))
        ));
    }

    #[test]
    fn a_broken_escape_is_kept_as_written() {
        assert_eq!(percent_decode("a%2Gb%4"), "a%2Gb%4");
        assert_eq!(percent_decode("%41%62"), "Ab");
    }
}
