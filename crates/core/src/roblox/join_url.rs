//! The link a client is started with.

use crate::types::{PlaceId, ServerId};

/// The engine's own deep link (`roblox://experiences/start`), not the
/// website's `roblox-player:` one: Cordial refuses a roblox-player link that
/// names a server, which is exactly what a follower joining its leader needs.
/// `gameInstanceId` is the engine's name for the server. There is no ticket
/// in it: the engine signs in from the session seeded into the profile.
pub fn join_url(place: &PlaceId, server: Option<&ServerId>) -> String {
    match server {
        Some(server) => {
            format!("roblox://experiences/start?placeId={place}&gameInstanceId={server}")
        }
        None => format!("roblox://experiences/start?placeId={place}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_is_joined_by_place() {
        let place = PlaceId::parse("606849621").unwrap();
        assert_eq!(join_url(&place, None), "roblox://experiences/start?placeId=606849621");
    }

    #[test]
    fn a_server_is_joined_by_game_instance_id() {
        let place = PlaceId::parse("1818").unwrap();
        let server = ServerId::parse("abc-def-123").unwrap();
        assert_eq!(
            join_url(&place, Some(&server)),
            "roblox://experiences/start?placeId=1818&gameInstanceId=abc-def-123"
        );
    }
}
