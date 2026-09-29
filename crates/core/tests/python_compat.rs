//! Files written by the Python app load, and save back with nothing lost.

#![allow(clippy::unwrap_used)] // a test crate: a failed unwrap is a failed test

use std::fs;
use std::path::Path;

use rbxmgr_core::Paths;
use rbxmgr_core::accounts::{AccountStore, SessionState};
use rbxmgr_core::types::UserId;
use serde_json::Value;

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name))
        .unwrap()
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// A null the Python app wrote for "not yet" is the same as no key.
fn without_nulls(v: Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.into_iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k, without_nulls(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(without_nulls).collect()),
        other => other,
    }
}

/// Keys the Rust side writes only when they differ from the default.
fn without_defaults(v: Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.into_iter()
                .filter(|(k, v)| {
                    !(v == &Value::Bool(false)
                        && ["nested", "low_power", "leader"].contains(&k.as_str()))
                })
                .map(|(k, v)| (k, without_defaults(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(without_defaults).collect()),
        other => other,
    }
}

#[test]
fn python_account_files_load_and_save_back_losslessly() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    fs::create_dir_all(paths.state()).unwrap();
    fs::write(paths.accounts(), fixture("accounts.json")).unwrap();
    fs::write(paths.groups(), fixture("groups.json")).unwrap();

    let store = AccountStore::load(&paths).unwrap();
    assert_eq!(store.accounts().len(), 2);
    assert_eq!(store.leader().map(|a| a.name.as_str()), Some("Main"));
    assert_eq!(store.session(UserId(1002)), SessionState::Expired);
    assert_eq!(store.visual_order().len(), 1);
    assert_eq!(store.groups()[0].place_id.as_ref().map(|p| p.as_str()), Some("1730877806"));

    store.save().unwrap();
    let saved = json(&fs::read_to_string(paths.accounts()).unwrap());
    let original = without_defaults(without_nulls(json(&fixture("accounts.json"))));
    assert_eq!(saved, original);
    assert_eq!(json(&fs::read_to_string(paths.groups()).unwrap()), json(&fixture("groups.json")));
}
