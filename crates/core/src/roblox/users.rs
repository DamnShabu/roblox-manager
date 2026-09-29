//! Users: whose session a cookie is, and names for user ids.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::json;

use super::RobloxError;
use super::http::{self, Request, Transport};
use crate::types::{Cookie, User, UserId};

/// The user ids per request Roblox takes for name lookups.
const NAMES_BATCH: usize = 100;

pub(super) fn whoami(t: &dyn Transport, cookie: &Cookie) -> Result<User, RobloxError> {
    let req = Request::get("https://users.roblox.com/v1/users/authenticated").as_user(cookie);
    http::ok(http::send(t, req)?, true)?.json()
}

/// Username and display name for each id Roblox knows. No cookie: public data.
pub(super) fn names(
    t: &dyn Transport,
    ids: &[UserId],
) -> Result<HashMap<UserId, User>, RobloxError> {
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        data: Vec<User>,
    }
    let mut found = HashMap::new();
    for batch in ids.chunks(NAMES_BATCH) {
        let ids: Vec<u64> = batch.iter().map(|id| id.0).collect();
        let req = Request::post("https://users.roblox.com/v1/users", json!({ "userIds": ids }));
        let page: Page = http::ok(http::send(t, req)?, false)?.json()?;
        found.extend(page.data.into_iter().map(|u| (u.id, u)));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roblox::http::canned::Canned;

    #[test]
    fn whoami_reads_the_authenticated_user_as_that_user() {
        let t = Canned::new().answer(200, r#"{"id": 7, "name": "bob", "displayName": "Bobby"}"#);
        let u = whoami(&t, &Cookie::new("c")).unwrap();
        assert_eq!((u.id, u.display()), (UserId(7), "Bobby"));
        let asked = &t.asked()[0];
        assert_eq!(asked.url, "https://users.roblox.com/v1/users/authenticated");
        assert_eq!(asked.cookie.as_ref().map(Cookie::expose), Some("c"));
    }

    #[test]
    fn a_refused_session_is_expired() {
        let t = Canned::new()
            .answer(401, r#"{"errors":[{"code":0,"message":"Authorization has been denied"}]}"#);
        assert_eq!(whoami(&t, &Cookie::new("c")).unwrap_err(), RobloxError::Expired);
    }

    #[test]
    fn names_are_asked_for_in_batches_of_100() {
        let ids: Vec<UserId> = (1..=150).map(UserId).collect();
        let t = Canned::new()
            .answer(200, r#"{"data": [{"id": 1, "name": "a", "displayName": "A"}]}"#)
            .answer(200, r#"{"data": []}"#);
        let found = names(&t, &ids).unwrap();
        assert_eq!(found[&UserId(1)].display(), "A");
        assert_eq!(t.asked().len(), 2);
        assert_eq!(t.asked()[1].json.as_ref().unwrap()["userIds"].as_array().unwrap().len(), 50);
    }
}
