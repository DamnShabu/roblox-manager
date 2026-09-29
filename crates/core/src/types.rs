//! The small values every area passes around. Each is built by a validating
//! parse, so a value that exists is one that can be used as it is.

use std::fmt;

use serde::{Deserialize, Serialize};

/// What a label may be, as the user reads it.
pub const LABEL_RULE: &str = "A label cannot be empty, start with '.' or '_', or contain '/' \
                              or control characters";

/// An account's name in this app. Also its keyring key, and once a directory
/// name, so it stays a single ordinary path component; a leading `_` is
/// reserved for the icon cache.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Label(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{LABEL_RULE}")]
pub struct InvalidLabel;

impl Label {
    pub fn parse(s: &str) -> Result<Self, InvalidLabel> {
        let ok = !s.is_empty()
            && !s.contains('/')
            && !s.starts_with(['.', '_'])
            && !s.chars().any(char::is_control);
        if ok { Ok(Label(s.to_owned())) } else { Err(InvalidLabel) }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Label {
    type Error = InvalidLabel;
    fn try_from(s: String) -> Result<Self, InvalidLabel> {
        Label::parse(&s)
    }
}

impl From<Label> for String {
    fn from(l: Label) -> String {
        l.0
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// A Roblox user id.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(pub u64);

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Why a place or server id was refused: anything else in one would add
/// parameters to the join link.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidId {
    #[error("place id {0:?} is not a number")]
    Place(String),
    #[error("server id {0:?} is not a server id")]
    Server(String),
}

/// A Roblox place (one game's start place). Digits only.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PlaceId(String);

impl PlaceId {
    pub fn parse(s: &str) -> Result<Self, InvalidId> {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            Ok(PlaceId(s.to_owned()))
        } else {
            Err(InvalidId::Place(s.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PlaceId {
    type Error = InvalidId;
    fn try_from(s: String) -> Result<Self, InvalidId> {
        PlaceId::parse(&s)
    }
}

impl From<PlaceId> for String {
    fn from(p: PlaceId) -> String {
        p.0
    }
}

impl fmt::Display for PlaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for PlaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PlaceId({})", self.0)
    }
}

/// One running server of a place (Roblox's gameId / gameInstanceId).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ServerId(String);

impl ServerId {
    pub fn parse(s: &str) -> Result<Self, InvalidId> {
        if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            Ok(ServerId(s.to_owned()))
        } else {
            Err(InvalidId::Server(s.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A Cordial profile's name. The manager's own are `rbxmgr-<user id>`
/// ([`Profile::of`]), keyed by the user so a rename never strands one;
/// others (someone playing from Cordial directly) come from `pgrep`.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Profile(String);

impl Profile {
    pub fn of(user: UserId) -> Self {
        Profile(format!("rbxmgr-{user}"))
    }

    /// A profile by the name a running client reports.
    pub fn named(name: impl Into<String>) -> Self {
        Profile(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A `.ROBLOSECURITY` session: a password in all but name. It never prints;
/// [`Cookie::expose`] is the one way to its text, for the places that must
/// send it.
#[derive(Clone, PartialEq, Eq)]
pub struct Cookie(String);

impl Cookie {
    pub fn new(value: impl Into<String>) -> Self {
        Cookie(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Cookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Cookie(***)")
    }
}

/// A Roblox account as Roblox reports it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub name: String,
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
}

impl User {
    /// The display name, falling back to the username.
    pub fn display(&self) -> &str {
        self.display_name.as_deref().filter(|d| !d.is_empty()).unwrap_or(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_that_are_not_one_path_component_are_refused() {
        for bad in ["", "a/b", ".", "..", ".hidden", "_icons", "alt\n1", "tab\there"] {
            assert_eq!(Label::parse(bad), Err(InvalidLabel), "{bad:?}");
        }
    }

    #[test]
    fn ordinary_labels_are_accepted_as_written() {
        for good in ["alt 1", "ñame ✓", "Main"] {
            assert_eq!(Label::parse(good).map(|l| l.to_string()), Ok(good.to_string()));
        }
    }

    #[test]
    fn a_label_deserializes_through_the_same_rule() {
        assert!(serde_json::from_str::<Label>("\"alt\"").is_ok());
        assert!(serde_json::from_str::<Label>("\"a/b\"").is_err());
    }

    #[test]
    fn place_ids_are_digits_only() {
        assert!(PlaceId::parse("606849621").is_ok());
        for bad in ["", "1818&x=1", "12a", "-1"] {
            assert!(PlaceId::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn server_ids_are_alphanumeric_and_dashes() {
        assert!(ServerId::parse("abc-def-123").is_ok());
        for bad in ["", "abc&accessCode=x", "a b", "a/b"] {
            assert!(ServerId::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_managers_profiles_are_named_by_user_id() {
        assert_eq!(Profile::of(UserId(123)).as_str(), "rbxmgr-123");
    }

    #[test]
    fn a_cookie_never_prints_its_value() {
        let c = Cookie::new("_|WARNING:-DO-NOT-SHARE-THIS.--secret");
        let shown = format!("{c:?}");
        assert!(!shown.contains("secret"), "{shown}");
        assert_eq!(c.expose(), "_|WARNING:-DO-NOT-SHARE-THIS.--secret");
    }

    #[test]
    fn a_user_reads_robloxs_json_and_falls_back_to_the_username() {
        let u: User = serde_json::from_str(r#"{"id": 7, "name": "bob", "displayName": ""}"#)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((u.id, u.display()), (UserId(7), "bob"));
    }
}
