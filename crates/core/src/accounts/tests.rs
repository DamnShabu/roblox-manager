use std::fs;

use chrono::TimeZone;
use serde_json::json;

use super::*;

/// A store of accounts (label, selected) with ids 1.., and groups (id, name),
/// saving nowhere that matters.
pub(in crate::accounts) fn store(
    accounts: &[(&str, bool)],
    groups: &[(&str, &str)],
) -> AccountStore {
    AccountStore {
        accounts_file: PathBuf::from("/nonexistent/accounts.json"),
        groups_file: PathBuf::from("/nonexistent/groups.json"),
        accounts: accounts
            .iter()
            .enumerate()
            .map(|(i, (name, selected))| {
                let mut a = Account::new(Label::parse(name).unwrap(), UserId(i as u64 + 1));
                a.selected = *selected;
                a
            })
            .collect(),
        groups: groups
            .iter()
            .map(|(id, name)| serde_json::from_value(json!({"id": id, "name": name})).unwrap())
            .collect(),
        unread_accounts: Vec::new(),
        unread_groups: Vec::new(),
        checking: HashSet::new(),
        set_aside: Vec::new(),
    }
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap()
}

fn user(id: u64, name: &str) -> User {
    User { id: UserId(id), name: name.into(), display_name: Some(format!("{name}!")) }
}

#[test]
fn a_new_user_gets_its_username_as_a_unique_label_and_leads_if_nobody_does() {
    let mut s = store(&[], &[]);
    assert_eq!(s.add_or_refresh(&user(5, "alt"), now()), (Label::parse("alt").unwrap(), true));
    assert_eq!(s.add_or_refresh(&user(6, "alt"), now()).0.as_str(), "alt 2");
    let a = s.get(UserId(5)).unwrap();
    assert!(a.leader && a.selected);
    assert_eq!(
        (a.display.as_deref(), a.session_checked.as_deref()),
        (Some("alt!"), Some("2026-09-29T12:00:00+00:00"))
    );
    assert!(!s.get(UserId(6)).unwrap().leader);
}

#[test]
fn the_same_user_again_refreshes_the_account_already_here() {
    let mut s = store(&[("mine", true)], &[]);
    s.end_check(UserId(1), Some(false), now());
    let (label, new) = s.add_or_refresh(&user(1, "renamed-on-roblox"), now());
    assert_eq!((label.as_str(), new), ("mine", false));
    assert_eq!(s.accounts().len(), 1);
    assert_eq!(s.get(UserId(1)).unwrap().username.as_deref(), Some("renamed-on-roblox"));
    assert_eq!(s.session(UserId(1)), SessionState::Ok { checked: Some(stamp(now())) });
}

#[test]
fn renaming_checks_the_rule_and_uniqueness() {
    let mut s = store(&[("a", true), ("b", true)], &[]);
    assert_eq!(s.rename(UserId(1), "b"), Err(AccountError::LabelTaken("b".into())));
    assert_eq!(s.rename(UserId(1), "x/y"), Err(AccountError::InvalidLabel(InvalidLabel)));
    assert_eq!(s.rename(UserId(1), "c").unwrap().as_str(), "a");
    assert_eq!(s.get(UserId(1)).unwrap().name.as_str(), "c");
}

#[test]
fn removing_closes_up_the_join_order() {
    let mut s = store(&[("a", true), ("b", true), ("c", true)], &[]);
    s.migrate_layout();
    assert_eq!(s.remove(UserId(2)).map(|a| a.user_id), Some(UserId(2)));
    let order: Vec<(u64, Option<u32>)> =
        s.followers().iter().map(|a| (a.user_id.0, a.follow)).collect();
    assert_eq!(order, [(3, Some(1))]);
}

#[test]
fn a_launch_is_recorded_as_a_play_and_a_good_session() {
    let mut s = store(&[("a", true)], &[]);
    s.end_check(UserId(1), Some(false), now());
    let place = PlaceId::parse("77").unwrap();
    s.record_launch(UserId(1), &user(1, "a-user"), Some(&place), now());
    s.record_launch(UserId(1), &user(1, "a-user"), Some(&place), now());
    let a = s.get(UserId(1)).unwrap();
    assert_eq!(a.plays["77"], 2);
    assert_eq!(a.last_launch.as_deref(), Some("2026-09-29T12:00:00+00:00"));
    assert_eq!(a.username.as_deref(), Some("a-user"));
    assert_eq!(s.session(UserId(1)), SessionState::Ok { checked: Some(stamp(now())) });
}

#[test]
fn a_check_says_checking_and_an_offline_one_leaves_the_last_verdict() {
    let mut s = store(&[("a", true)], &[]);
    s.end_check(UserId(1), Some(false), now());
    s.begin_check(UserId(1));
    assert_eq!(s.session(UserId(1)), SessionState::Checking);
    s.end_check(UserId(1), None, now());
    assert_eq!(s.session(UserId(1)), SessionState::Expired);
}

#[test]
fn checking_never_reaches_the_disk_but_expired_does() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    let mut s = AccountStore::load(&paths).unwrap();
    s.add_or_refresh(&user(1, "a"), now());
    s.add_or_refresh(&user(2, "b"), now());
    s.begin_check(UserId(1));
    s.end_check(UserId(2), Some(false), now());
    s.save().unwrap();
    let raw = fs::read_to_string(paths.accounts()).unwrap();
    assert!(!raw.contains("checking"), "{raw}");
    let again = AccountStore::load(&paths).unwrap();
    assert_eq!(again.session(UserId(1)), SessionState::Ok { checked: Some(stamp(now())) });
    assert_eq!(again.session(UserId(2)), SessionState::Expired);
}

#[test]
fn picking_a_macro_turns_on_the_macro_ready_window() {
    let mut s = store(&[("a", true), ("b", true)], &[]);
    s.set_macro(UserId(1), Some("m"));
    s.set_macro(UserId(2), Some("m"));
    assert!(s.get(UserId(1)).unwrap().nested);
    s.rename_macro("m", "n");
    assert_eq!(s.get(UserId(2)).unwrap().macro_name.as_deref(), Some("n"));
    s.drop_macro("n");
    assert!(s.accounts().iter().all(|a| a.macro_name.is_none()));
    assert!(s.get(UserId(1)).unwrap().nested, "dropping the macro keeps the window");
}

#[test]
fn a_first_load_carries_the_old_layout_over_and_writes_both_files() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    json_file::write(
        &paths.accounts(),
        &json!([{"name": "a", "user_id": 1}, {"name": "b", "user_id": 2, "selected": false}, {"name": "c", "user_id": 3}]),
    )
    .unwrap();
    let s = AccountStore::load(&paths).unwrap();
    assert_eq!(s.leader().map(|a| a.user_id), Some(UserId(1)));
    assert_eq!(s.followers().iter().map(|a| a.user_id.0).collect::<Vec<_>>(), [3]);
    assert!(paths.groups().exists());
    let again = AccountStore::load(&paths).unwrap();
    assert_eq!(again.followers().len(), 1);
}

#[test]
fn entries_that_are_not_accounts_survive_a_save() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    json_file::write(
        &paths.accounts(),
        &json!([{"name": "a", "user_id": 1}, {"name": "../x", "user_id": 2}]),
    )
    .unwrap();
    json_file::write(&paths.groups(), &json!([])).unwrap();
    let s = AccountStore::load(&paths).unwrap();
    assert_eq!(s.accounts().len(), 1);
    s.save().unwrap();
    let raw: Value = serde_json::from_str(&fs::read_to_string(paths.accounts()).unwrap()).unwrap();
    assert_eq!(raw[1], json!({"name": "../x", "user_id": 2}));
}

#[test]
fn the_index_never_holds_a_secret() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    let mut s = AccountStore::load(&paths).unwrap();
    s.add_or_refresh(&user(1, "a"), now());
    s.save().unwrap();
    let raw = fs::read_to_string(paths.accounts()).unwrap();
    for word in ["ROBLOSECURITY", "cookie", "privateKey", "password"] {
        assert!(!raw.to_lowercase().contains(&word.to_lowercase()), "{raw}");
    }
}

#[test]
fn the_last_place_picked_is_remembered() {
    let mut s = store(&[("a", true), ("b", true)], &[]);
    s.remember_place(&PlaceId::parse("5").unwrap());
    assert_eq!(s.last_place().map(PlaceId::as_str), Some("5"));
}

#[test]
fn a_hand_broken_accounts_file_is_set_aside_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    fs::create_dir_all(paths.state()).unwrap();
    let broken = r#"[{"name": "a", "user_id": 1},]"#;
    fs::write(paths.accounts(), broken).unwrap();
    let mut s = AccountStore::load(&paths).unwrap();
    assert!(s.accounts().is_empty());
    let aside = s.set_aside().to_vec();
    assert_eq!(aside.len(), 1, "{aside:?}");
    s.add_or_refresh(&user(2, "b"), now());
    s.save().unwrap();
    assert_eq!(fs::read_to_string(&aside[0]).unwrap(), broken, "the user's text survives the save");
}

#[test]
fn the_accounts_for_join_links_are_remembered_and_replaced() {
    let mut s = store(&[("a", true), ("b", true), ("c", true)], &[]);
    assert!(s.link_accounts().is_empty());
    s.set_link_accounts(&[UserId(3), UserId(1)]);
    assert_eq!(s.link_accounts(), [UserId(1), UserId(3)], "in list order");
    s.set_link_accounts(&[UserId(2)]);
    assert_eq!(s.link_accounts(), [UserId(2)]);
    let saved = serde_json::to_value(s.get(UserId(2)).unwrap()).unwrap();
    assert_eq!(saved["join_links"], json!(true));
    let unpicked = serde_json::to_value(s.get(UserId(1)).unwrap()).unwrap();
    assert!(unpicked.get("join_links").is_none(), "false is not written");
}
