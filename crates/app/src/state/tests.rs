use super::*;
use rbxmgr_core::Paths;
use rbxmgr_core::types::User;

fn state() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    let mut accounts = AccountStore::load(&paths).unwrap();
    let now = chrono::Utc::now();
    for (id, name) in [(1, "a"), (2, "b")] {
        accounts
            .add_or_refresh(&User { id: UserId(id), name: name.into(), display_name: None }, now);
    }
    let macros = MacroLibrary::load(&paths.macros());
    (dir, AppState::new(accounts, macros))
}

#[test]
fn a_chip_shows_the_liveliest_state() {
    let (_d, mut s) = state();
    assert_eq!(s.chip(UserId(1)), Chip::Idle);
    s.accounts.end_check(UserId(1), Some(false), chrono::Utc::now());
    assert_eq!(s.chip(UserId(1)), Chip::Expired);
    s.launching.insert(UserId(1));
    assert_eq!(s.chip(UserId(1)), Chip::Starting);
    s.joining.insert(UserId(1));
    assert_eq!(s.chip(UserId(1)), Chip::Joining);
    s.running.insert(UserId(1));
    assert_eq!(s.chip(UserId(1)), Chip::Running);
}

#[test]
fn the_views_split_the_accounts_by_state_and_count_them() {
    let (_d, mut s) = state();
    s.running.insert(UserId(1));
    s.failures.insert(UserId(2), "no build".into());
    let shown = |s: &AppState| -> Vec<u64> {
        s.accounts.accounts().iter().filter(|a| s.in_view(a)).map(|a| a.user_id.0).collect()
    };
    assert_eq!(shown(&s), [1, 2]);
    s.view = View::Running;
    assert_eq!(shown(&s), [1]);
    s.view = View::Attention;
    assert_eq!(shown(&s), [2]);
    s.view = View::Idle;
    assert_eq!(shown(&s), [2]);
    s.launching.insert(UserId(2));
    s.view = View::Launching;
    assert_eq!(shown(&s), [2]);
    let c = s.counts();
    assert_eq!((c.all, c.running, c.launching, c.idle, c.attention), (2, 1, 1, 0, 1));
    assert_eq!(
        s.needs(),
        [Need { id: UserId(2), name: "b".into(), expired: false, why: "no build".into() }]
    );
}

#[test]
fn a_group_view_shows_its_members_and_never_the_main_account() {
    let (_d, mut s) = state();
    let g = s.accounts.add_group(chrono::Utc::now());
    s.accounts.set_group(UserId(2), Some(&g)).unwrap();
    s.accounts.make_leader(UserId(1));
    s.view = View::Group(g);
    let shown: Vec<u64> =
        s.accounts.accounts().iter().filter(|a| s.in_view(a)).map(|a| a.user_id.0).collect();
    assert_eq!(shown, [2]);
    s.view = View::Group(String::new());
    assert!(
        s.accounts.accounts().iter().all(|a| !s.in_view(a)),
        "the main account is pinned, not ungrouped"
    );
}

#[test]
fn a_running_clients_old_level_shows_until_it_is_relaunched() {
    let (_d, mut s) = state();
    s.launched_as.insert(UserId(1), Performance::Medium);
    s.accounts.set_performance(UserId(1), Performance::Low);
    assert_eq!(s.stale_performance(UserId(1)), None, "not running");
    s.running.insert(UserId(1));
    assert_eq!(s.stale_performance(UserId(1)), Some(Performance::Medium));
    s.accounts.set_performance(UserId(1), Performance::Medium);
    assert_eq!(s.stale_performance(UserId(1)), None);
}

#[test]
fn a_group_is_live_while_any_member_runs_or_starts() {
    let (_d, mut s) = state();
    let group = [UserId(1), UserId(2)];
    assert!(!s.any_live(&group));
    s.launching.insert(UserId(2));
    assert!(s.any_live(&group));
    s.launching.clear();
    s.running.insert(UserId(1));
    assert!(s.any_live(&group));
    assert!(!s.any_live(&[UserId(2)]));
    assert!(!s.any_live(&[]));
}

#[test]
fn the_search_matches_every_word_in_any_name_or_the_note() {
    let (_d, mut s) = state();
    s.accounts.set_note(UserId(2), "Has the Buddha fruit");
    s.filter = "b".into();
    assert_eq!(s.matching(), [UserId(2)], "label b");
    s.filter = "buddha b".into();
    assert_eq!(s.matching(), [UserId(2)], "both words, one in the note");
    s.filter = "buddha x".into();
    assert!(s.matching().is_empty());
    s.filter = String::new();
    assert_eq!(s.matching(), [UserId(1), UserId(2)], "no words: everyone, leader first");
}

#[test]
fn stopping_launches_stops_those_that_start_the_accounts_asked_for() {
    let (_d, mut s) = state();
    let (a, b) = (StopFlag::default(), StopFlag::default());
    s.launches = vec![(a.clone(), vec![UserId(1)]), (b.clone(), vec![UserId(2)])];
    assert_eq!(s.stop_launches(Some(&[UserId(2), UserId(9)])), 1);
    assert!(!a.is_set() && b.is_set());
    assert_eq!(s.stop_launches(None), 1, "the one already stopped is not counted");
    assert!(a.is_set());
}

#[test]
fn the_target_is_a_friends_server_else_the_picked_game() {
    let (_d, mut s) = state();
    assert_eq!(s.target_label(), "Games browser");
    let place = PlaceId::parse("77").unwrap();
    s.game_list = vec![Tile {
        game: Game { universe_id: "1".into(), place_id: place.clone(), name: "Obby".into() },
        icon: None,
    }];
    s.place = Some(place.clone());
    assert_eq!((s.target(), s.target_label()), ((Some(place.clone()), None), "Obby".into()));
    s.friend = Some(FriendTarget {
        user: UserId(9),
        display: "Pal".into(),
        game: "Tag".into(),
        place: PlaceId::parse("5").unwrap(),
        server: ServerId::parse("s-1").unwrap(),
    });
    assert_eq!(s.target_label(), "Join Pal · Tag");
    assert_eq!(s.target().1.map(|x| x.to_string()), Some("s-1".into()));
}

#[test]
fn the_status_line_counts_clients_and_what_needs_attention() {
    let (_d, mut s) = state();
    assert_eq!(s.status_line(), "All idle");
    s.launching.insert(UserId(2));
    assert_eq!(s.status_line(), "1 starting");
    s.running.extend([UserId(1), UserId(2)]);
    assert_eq!(s.status_line(), "2 running", "a running client is no longer starting");
    s.running.remove(&UserId(2));
    s.launching.clear();
    s.failures.insert(UserId(2), "no build".into());
    assert_eq!(s.status_line(), "1 running · 1 needs attention");
    s.accounts.end_check(UserId(1), Some(false), chrono::Utc::now());
    assert_eq!(s.status_line(), "1 running · 2 need attention");
}

#[test]
fn run_starts_a_macro_on_the_selected_accounts_not_yet_playing_it() {
    let (_d, mut s) = state();
    s.accounts.set_selected(&[UserId(1), UserId(2)], false);
    assert_eq!(s.macro_run("m"), MacroRun::Nothing);
    s.accounts.set_selected(&[UserId(1)], true);
    assert_eq!(s.macro_run("m"), MacroRun::Start(vec![UserId(1)]));
    s.macro_runs.insert(UserId(1), (StopFlag::default(), "m".into()));
    assert_eq!(s.macro_run("m"), MacroRun::Stop, "every selected one plays it");
    s.accounts.set_selected(&[UserId(2)], true);
    assert_eq!(
        s.macro_run("m"),
        MacroRun::Start(vec![UserId(2)]),
        "the one playing it plays on; only the other starts"
    );
    s.macro_runs.insert(UserId(2), (StopFlag::default(), "other".into()));
    assert_eq!(s.macro_run("m"), MacroRun::Start(vec![UserId(2)]), "playing another macro");
    s.accounts.set_selected(&[UserId(1), UserId(2)], false);
    assert_eq!(s.macro_run("m"), MacroRun::Stop, "none selected: it can still be stopped");
}

#[test]
fn recent_targets_put_the_latest_pick_first_then_the_favourites() {
    let (_d, mut s) = state();
    let tile = |id: &str| Tile {
        game: Game {
            universe_id: id.into(),
            place_id: PlaceId::parse(id).unwrap(),
            name: id.into(),
        },
        icon: None,
    };
    s.game_list = ["1", "2", "3", "4", "5"].map(tile).to_vec();
    let ids = |s: &AppState| s.recents().iter().map(|t| t.game.name.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&s), ["1", "2", "3", "4"]);
    s.picked(&PlaceId::parse("5").unwrap());
    s.picked(&PlaceId::parse("3").unwrap());
    assert_eq!(ids(&s), ["3", "5", "1", "2"]);
}

#[test]
fn the_activity_log_keeps_the_latest_lines_newest_first() {
    let (_d, mut s) = state();
    for n in 0..ACTIVITY_KEPT + 3 {
        s.log("12:00".into(), format!("line {n}"));
    }
    assert_eq!(s.activity.len(), ACTIVITY_KEPT);
    assert_eq!(s.activity[0].line, format!("line {}", ACTIVITY_KEPT + 2));
}

#[test]
fn closing_warns_of_what_it_would_cut_short() {
    let (_d, mut s) = state();
    assert_eq!(s.close_warning(), None);
    s.launches.push((StopFlag::default(), vec![UserId(1)]));
    let one = s.close_warning().unwrap();
    assert!(one.contains("stops a launch still starting accounts."), "{one}");
    s.macro_runs.insert(UserId(1), (StopFlag::default(), "m".into()));
    s.macro_runs.insert(UserId(2), (StopFlag::default(), "m".into()));
    s.updating = true;
    let all = s.close_warning().unwrap();
    assert!(
        all.contains("macros playing into 2 clients, a launch still starting accounts and an update still installing"),
        "{all}"
    );
}
