//! What accounts.json and groups.json hold, read leniently: a field that is
//! missing or of the wrong type takes its default, and keys this version does
//! not know are kept, so nothing another version wrote is lost on save.

use std::collections::BTreeMap;

use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::roblox::Game;
use crate::types::{Label, PlaceId, UserId};

/// One account: an index entry, never a secret -- its session lives in the
/// keyring under its label.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Account {
    /// The label this app shows and files the account's cookie under.
    pub name: Label,
    pub user_id: UserId,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub note: String,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub last_launch: Option<String>,
    /// When Roblox last accepted the stored session.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub session_checked: Option<String>,
    /// Set when Roblox refused the stored session.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub session: Option<StoredSession>,
    #[serde(default = "yes", deserialize_with = "lenient_selected")]
    pub selected: bool,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub last_place: Option<PlaceId>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Vec::is_empty")]
    pub favorites: Vec<Game>,
    /// Launches from here, by place id.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "BTreeMap::is_empty")]
    pub plays: BTreeMap<String, u64>,
    /// The client runs in a cage of its own, where macros can reach it.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "is_false")]
    pub nested: bool,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "is_false")]
    pub low_power: bool,
    /// The macro its Run plays.
    #[serde(
        rename = "macro",
        default,
        deserialize_with = "lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub macro_name: Option<String>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "is_false")]
    pub leader: bool,
    /// Its place in the auto-join order behind the leader, from 1.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub follow: Option<u32>,
    /// The group it is drawn in.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Picked up front when a join link comes from the browser.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "is_false")]
    pub join_links: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Account {
    pub fn new(name: Label, user_id: UserId) -> Self {
        Account {
            name,
            user_id,
            username: None,
            display: None,
            note: String::new(),
            last_launch: None,
            session_checked: None,
            session: None,
            selected: true,
            last_place: None,
            favorites: Vec::new(),
            plays: BTreeMap::new(),
            nested: false,
            low_power: false,
            macro_name: None,
            leader: false,
            follow: None,
            group: None,
            join_links: false,
            extra: Map::new(),
        }
    }
}

/// What is remembered about a session between runs: only a refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StoredSession {
    Expired,
}

/// A named group of accounts, drawn together and launched into its game.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    #[serde(default, deserialize_with = "lenient")]
    pub name: String,
    #[serde(default, deserialize_with = "lenient")]
    pub place_id: Option<PlaceId>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    #[serde(default = "yes", deserialize_with = "lenient_open")]
    pub open: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A value of the wrong type takes the default instead of failing the file.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(T::deserialize(v).unwrap_or_default())
}

fn lenient_selected<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    Ok(Value::deserialize(d)?.as_bool().unwrap_or(true))
}

fn lenient_open<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    lenient_selected(d)
}

fn yes() -> bool {
    true
}

fn is_false(b: &bool) -> bool {
    !b
}

/// Entries from a JSON list: the ones that read as `T`, and the ones that do
/// not, kept as they were so they are written back untouched.
pub(super) fn split_entries<T: DeserializeOwned>(list: Vec<Value>) -> (Vec<T>, Vec<Value>) {
    let mut good = Vec::new();
    let mut kept = Vec::new();
    for entry in list {
        match T::deserialize(&entry) {
            Ok(t) => good.push(t),
            Err(_) => kept.push(entry),
        }
    }
    (good, kept)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_account_written_by_the_python_app_reads_back_whole() {
        let raw = json!({
            "name": "Main", "user_id": 7, "display": "D", "note": "farm acct",
            "last_launch": "2026-09-22T05:00:00+00:00", "selected": true, "last_place": "1730877806",
            "favorites": [{"universe_id": "648454481", "place_id": "1730877806", "name": "GPO"}],
            "plays": {"1730877806": 3}, "nested": false, "leader": true, "macro": "Macro 1",
            "follow": 2, "group": "g1", "someday": {"new": "field"}
        });
        let a: Account = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(a.name.as_str(), "Main");
        assert_eq!(a.macro_name.as_deref(), Some("Macro 1"));
        assert_eq!(a.plays["1730877806"], 3);
        let back = serde_json::to_value(&a).unwrap();
        assert_eq!(back["someday"], json!({"new": "field"}), "unknown keys survive");
        let mut expected = raw.as_object().unwrap().clone();
        expected.remove("nested"); // false is the default and is not written
        assert_eq!(back, Value::Object(expected));
    }

    #[test]
    fn a_field_of_the_wrong_type_takes_its_default() {
        let a: Account = serde_json::from_value(json!({
            "name": "x", "user_id": 1, "selected": "yes", "plays": [1, 2], "follow": "first",
            "session": "checking"
        }))
        .unwrap();
        assert!(a.selected);
        assert!(a.plays.is_empty());
        assert_eq!((a.follow, a.session), (None, None));
    }

    #[test]
    fn an_entry_that_is_no_account_is_kept_aside_unread() {
        let (good, kept) = split_entries::<Account>(vec![
            json!({"name": "ok", "user_id": 1}),
            json!({"name": "a/b", "user_id": 2}),
            json!("junk"),
        ]);
        assert_eq!(good.len(), 1);
        assert_eq!(kept, vec![json!({"name": "a/b", "user_id": 2}), json!("junk")]);
    }

    #[test]
    fn a_group_defaults_to_open_with_no_game() {
        let g: Group = serde_json::from_value(json!({"id": "g1"})).unwrap();
        assert!(g.open);
        assert_eq!((g.place_id, g.game), (None, None));
    }
}
