//! What the window knows: the stores, and everything about this run of the
//! app. Runtime state is keyed by Roblox user id, so relabelling an account
//! strands nothing.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rbxmgr_core::accounts::{AccountStore, SessionState};
use rbxmgr_core::macros::{MacroLibrary, StopFlag};
use rbxmgr_core::roblox::{Friend, Game};
use rbxmgr_core::types::{PlaceId, ServerId, UserId};

/// A game as the bar shows it: its icon, when one is cached.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub game: Game,
    pub icon: Option<PathBuf>,
}

/// A friend's server, when that is the launch target.
#[derive(Clone, Debug, PartialEq)]
pub struct FriendTarget {
    pub user: UserId,
    pub display: String,
    pub game: String,
    pub place: PlaceId,
    pub server: ServerId,
}

impl FriendTarget {
    /// A friend who can be joined: in a game, with the server visible.
    pub fn of(f: &Friend) -> Option<Self> {
        Some(FriendTarget {
            user: f.id,
            display: f.display.clone(),
            game: f.game.clone().unwrap_or_default(),
            place: f.place.clone()?,
            server: f.server.clone()?,
        })
    }
}

/// An account's status chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chip {
    Running,
    Joining,
    Starting,
    Expired,
    Idle,
}

impl Chip {
    /// (CSS class, text, whether its dot pulses)
    pub fn look(self) -> (&'static str, &'static str, bool) {
        match self {
            Chip::Running => ("running", "Running", true),
            Chip::Joining => ("joining", "Joining…", true),
            Chip::Starting => ("starting", "Starting…", true),
            Chip::Expired => ("expired", "Expired", false),
            Chip::Idle => ("idle", "Idle", false),
        }
    }
}

pub struct AppState {
    pub accounts: AccountStore,
    pub macros: MacroLibrary,
    /// Accounts with a live client, from the poll.
    pub running: HashSet<UserId>,
    /// Accounts a launch is starting now...
    pub launching: HashSet<UserId>,
    /// ...and of those, the ones following a leader.
    pub joining: HashSet<UserId>,
    /// Each account's playing macro: what stops it, and its name.
    pub macro_runs: HashMap<UserId, (StopFlag, String)>,
    pub open_accounts: HashSet<UserId>,
    pub open_macros: HashSet<String>,
    /// The group showing its settings.
    pub edit_group: Option<String>,
    pub ungrouped_open: bool,
    /// A friend's server as the launch target, instead of the picked game.
    pub friend: Option<FriendTarget>,
    /// The picked game; None is Roblox's own games browser.
    pub place: Option<PlaceId>,
    pub game_list: Vec<Tile>,
    /// (time, line), newest first, the last four.
    pub activity: Vec<(String, String)>,
    /// How many tasks are running; the window spins while any are.
    pub busy: u32,
}

impl AppState {
    pub fn new(accounts: AccountStore, macros: MacroLibrary) -> Self {
        AppState {
            accounts,
            macros,
            running: HashSet::new(),
            launching: HashSet::new(),
            joining: HashSet::new(),
            macro_runs: HashMap::new(),
            open_accounts: HashSet::new(),
            open_macros: HashSet::new(),
            edit_group: None,
            ungrouped_open: true,
            friend: None,
            place: None,
            game_list: Vec::new(),
            activity: Vec::new(),
            busy: 0,
        }
    }

    pub fn chip(&self, id: UserId) -> Chip {
        if self.running.contains(&id) {
            Chip::Running
        } else if self.joining.contains(&id) {
            Chip::Joining
        } else if self.launching.contains(&id) {
            Chip::Starting
        } else if self.accounts.session(id) == SessionState::Expired {
            Chip::Expired
        } else {
            Chip::Idle
        }
    }

    /// Whether any of `ids` has a client up or on its way: a group's header
    /// then offers Shut down instead of Launch.
    pub fn any_live(&self, ids: &[UserId]) -> bool {
        ids.iter().any(|id| self.running.contains(id) || self.launching.contains(id))
    }

    /// Where a launch sends accounts: a friend's server, or the picked game.
    pub fn target(&self) -> (Option<PlaceId>, Option<ServerId>) {
        match &self.friend {
            Some(f) => (Some(f.place.clone()), Some(f.server.clone())),
            None => (self.place.clone(), None),
        }
    }

    pub fn target_label(&self) -> String {
        if let Some(f) = &self.friend {
            return format!("Join {} · {}", f.display, f.game);
        }
        self.place
            .as_ref()
            .and_then(|p| self.game_list.iter().find(|t| &t.game.place_id == p))
            .map_or_else(|| "Games browser".to_owned(), |t| t.game.name.clone())
    }

    /// The title bar pill: "3 running" or "All idle".
    pub fn pill(&self) -> String {
        match self.running.len() {
            0 => "All idle".to_owned(),
            n => format!("{n} running"),
        }
    }

    /// The macros playing anywhere.
    pub fn macros_running(&self) -> HashSet<&str> {
        self.macro_runs.values().map(|(_, m)| m.as_str()).collect()
    }

    /// Add an activity line; the log keeps the latest four.
    pub fn log(&mut self, time: String, line: String) {
        self.activity.insert(0, (time, line));
        self.activity.truncate(4);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbxmgr_core::Paths;
    use rbxmgr_core::types::User;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        let mut accounts = AccountStore::load(&paths).unwrap();
        let now = chrono::Utc::now();
        for (id, name) in [(1, "a"), (2, "b")] {
            accounts.add_or_refresh(
                &User { id: UserId(id), name: name.into(), display_name: None },
                now,
            );
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
    fn the_pill_counts_running_clients() {
        let (_d, mut s) = state();
        assert_eq!(s.pill(), "All idle");
        s.running.extend([UserId(1), UserId(2)]);
        assert_eq!(s.pill(), "2 running");
    }

    #[test]
    fn the_activity_log_keeps_the_latest_four() {
        let (_d, mut s) = state();
        for n in 0..6 {
            s.log("12:00".into(), format!("line {n}"));
        }
        let lines: Vec<&str> = s.activity.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(lines, ["line 5", "line 4", "line 3", "line 2"]);
    }
}
