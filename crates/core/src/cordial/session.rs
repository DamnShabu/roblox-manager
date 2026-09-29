//! The session the manager gives a Cordial profile, in the formats Cordial
//! reads (crates/cordial-shell/src/secrets.rs, cookies.rs and identity.rs at
//! the pinned build). An upstream bump that changes them needs these changed
//! with it.

use serde::Serialize;

use crate::types::{Cookie, User};

/// Cordial's saved identity (schema 1). Only userId and username are
/// required; the client rewrites the rest from Roblox once it is up.
pub fn identity_json(user: &User) -> String {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Identity<'a> {
        schema: u8,
        user_id: u64,
        username: &'a str,
        display_name: &'a str,
        membership_type: u8,
        is_under13: bool,
        has_roblox_subscription: bool,
        country_code: &'a str,
    }
    let identity = Identity {
        schema: 1,
        user_id: user.id.0,
        username: &user.name,
        display_name: user.display(),
        membership_type: 0,
        is_under13: false,
        has_roblox_subscription: false,
        country_code: "",
    };
    // A struct of strings, numbers and bools always serializes.
    serde_json::to_string(&identity).unwrap_or_default() + "\n"
}

/// Cordial's cookie-store body holding one session. Both hosts are the ones
/// Cordial seeds itself; its routing check requires every copy of
/// .ROBLOSECURITY in the store to agree, which these do.
pub fn cookie_store(cookie: &Cookie) -> String {
    let jar = escape_jar(&format!(".ROBLOSECURITY={}", cookie.expose()));
    let mut store = String::from(
        "# cordial cookie store v1 -- a live Roblox session. Treat it as a password.\n",
    );
    for host in [".roblox.com", "roblox.com"] {
        store.push_str(&format!("{host}\t{jar}\n"));
    }
    store
}

fn escape_jar(jar: &str) -> String {
    jar.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r")
}

/// How Cordial stores a body in the keyring: hex behind a version prefix,
/// because some services mangle tabs and newlines in a text secret.
pub fn encode(body: &str) -> String {
    let hex: String = body.bytes().map(|b| format!("{b:02x}")).collect();
    format!("cordial-secret-hex-v1:{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::UserId;

    const COOKIE: &str = "_|WARNING:-DO-NOT-SHARE-THIS.--Sharing|_ABC.def-123";

    #[test]
    fn the_jar_carries_exactly_the_session_on_both_hosts() {
        let store = cookie_store(&Cookie::new(COOKIE));
        assert_eq!(
            store,
            format!(
                "# cordial cookie store v1 -- a live Roblox session. Treat it as a password.\n\
                 .roblox.com\t.ROBLOSECURITY={COOKIE}\nroblox.com\t.ROBLOSECURITY={COOKIE}\n"
            )
        );
    }

    #[test]
    fn tabs_newlines_and_backslashes_in_a_session_are_escaped() {
        let store = cookie_store(&Cookie::new("a\tb\\c\nd\re"));
        assert!(store.contains(".ROBLOSECURITY=a\\tb\\\\c\\nd\\re\n"), "{store:?}");
    }

    #[test]
    fn the_identity_is_schema_1_with_what_routing_needs() {
        let user = User { id: UserId(123), name: "bob".into(), display_name: None };
        let id: serde_json::Value = serde_json::from_str(&identity_json(&user)).unwrap();
        assert_eq!(
            id,
            serde_json::json!({
                "schema": 1, "userId": 123, "username": "bob", "displayName": "bob",
                "membershipType": 0, "isUnder13": false, "hasRobloxSubscription": false,
                "countryCode": ""
            })
        );
        assert!(identity_json(&user).ends_with("}\n"));
    }

    #[test]
    fn bodies_are_hex_behind_cordials_prefix() {
        assert_eq!(encode("a\tb\n"), "cordial-secret-hex-v1:6109620a");
    }
}
