//! What the window knows: the stores, and everything about this run of the
//! app. Runtime state is keyed by Roblox user id, so relabelling an account
//! strands nothing.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rbxmgr_core::accounts::{AccountStore, SessionState};
use rbxmgr_core::cordial::Window as ClientWindow;
use rbxmgr_core::macros::{MacroLibrary, StopFlag};
use rbxmgr_core::roblox::{Friend, Game};
use rbxmgr_core::types::{PlaceId, ServerId, UserId};
use rbxmgr_core::update::Available;

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

/// One line of the activity log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    /// Local time, "14:05".
    pub time: String,
    pub line: String,
}

/// How many activity lines are kept for the log window.
pub const ACTIVITY_KEPT: usize = 500;

pub struct AppState {
    pub accounts: AccountStore,
    pub macros: MacroLibrary,
    /// Accounts with a live client, from the poll.
    pub running: HashSet<UserId>,
    /// The windows of running clients whose engine says how they are, from
    /// the poll; a client from before Stacked could hide one is not here.
    pub windows: HashMap<UserId, ClientWindow>,
    /// Accounts a launch is starting now...
    pub launching: HashSet<UserId>,
    /// ...and of those, the ones following a leader.
    pub joining: HashSet<UserId>,
    /// Each account's playing macro: what stops it, and its name.
    pub macro_runs: HashMap<UserId, (StopFlag, String)>,
    /// Where each account's playing macro is: the last thing it reported
    /// ("round 3, step 4/9: pressing e"), shown on its row. None said yet is absent.
    pub macro_progress: HashMap<UserId, String>,
    /// The account a recording hears, while one is armed or under way.
    pub recording: Option<UserId>,
    /// Launches under way: what stops each, and the accounts it starts.
    pub launches: Vec<(StopFlag, Vec<UserId>)>,
    /// The macros shown unfolded.
    pub open_macros: HashSet<String>,
    /// What the account search holds, lower-cased; empty shows the layout.
    pub filter: String,
    pub ungrouped_open: bool,
    /// A friend's server as the launch target, instead of the picked game.
    pub friend: Option<FriendTarget>,
    /// The picked game; None is Roblox's own games browser.
    pub place: Option<PlaceId>,
    pub game_list: Vec<Tile>,
    /// Newest first, the last [`ACTIVITY_KEPT`].
    pub activity: Vec<Activity>,
    /// The place each account's client was last launched into.
    pub playing: HashMap<UserId, PlaceId>,
    /// Why each account's last launch failed, until one succeeds.
    pub failures: HashMap<UserId, String>,
    /// An Update is being installed.
    pub updating: bool,
    /// What has a newer release, from the last check.
    pub available: Available,
    /// The app version an Update installed, which a restart starts.
    pub restart_to: Option<String>,
    /// How many tasks are running; the window spins while any are.
    pub busy: u32,
}

/// What a macro's Run does, given what plays where.
#[derive(Debug, PartialEq, Eq)]
pub enum MacroRun {
    /// Start it on these accounts, in the order listed.
    Start(Vec<UserId>),
    /// Stop it everywhere it plays.
    Stop,
    /// No account is selected, and it plays nowhere.
    Nothing,
}

impl AppState {
    pub fn new(accounts: AccountStore, macros: MacroLibrary) -> Self {
        AppState {
            accounts,
            macros,
            running: HashSet::new(),
            windows: HashMap::new(),
            launching: HashSet::new(),
            joining: HashSet::new(),
            macro_runs: HashMap::new(),
            macro_progress: HashMap::new(),
            recording: None,
            launches: Vec::new(),
            open_macros: HashSet::new(),
            filter: String::new(),
            ungrouped_open: true,
            friend: None,
            place: None,
            game_list: Vec::new(),
            activity: Vec::new(),
            playing: HashMap::new(),
            failures: HashMap::new(),
            updating: false,
            available: Available::default(),
            restart_to: None,
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

    /// The accounts the search finds, in drawn order with the leader first:
    /// its words against the label, the Roblox user and display names, and
    /// the note.
    pub fn matching(&self) -> Vec<UserId> {
        let words: Vec<&str> = self.filter.split_whitespace().collect();
        let a = &self.accounts;
        a.leader()
            .into_iter()
            .chain(a.visual_order())
            .filter(|acct| {
                let hay = [
                    acct.name.as_str(),
                    acct.username.as_deref().unwrap_or_default(),
                    acct.display.as_deref().unwrap_or_default(),
                    &acct.note,
                ]
                .join("\n")
                .to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .map(|acct| acct.user_id)
            .collect()
    }

    /// Stop every launch under way that starts any of `ids` (every one,
    /// for None) before its next sign-in. Returns how many were stopped.
    pub fn stop_launches(&self, ids: Option<&[UserId]>) -> usize {
        let mut n = 0;
        for (stop, of) in &self.launches {
            if ids.is_none_or(|ids| of.iter().any(|id| ids.contains(id))) && !stop.is_set() {
                stop.set();
                n += 1;
            }
        }
        n
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

    /// A place's game name, when the bar has it.
    pub fn game_name(&self, place: &PlaceId) -> Option<&str> {
        self.game_list.iter().find(|t| &t.game.place_id == place).map(|t| t.game.name.as_str())
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

    /// The window's subtitle: "3 running · 1 macro playing", or "All idle".
    pub fn status_line(&self) -> String {
        let mut parts = Vec::new();
        match (self.running.len(), self.launching.len()) {
            (0, 0) => {}
            (0, n) => parts.push(format!("{n} starting")),
            (n, 0) => parts.push(format!("{n} running")),
            (n, m) => parts.push(format!("{n} running, {m} starting")),
        }
        match self.macro_runs.len() {
            0 => {}
            1 => parts.push("1 macro playing".to_owned()),
            n => parts.push(format!("{n} macros playing")),
        }
        if parts.is_empty() { "All idle".to_owned() } else { parts.join(" · ") }
    }

    /// What a macro's Run does now: start it on the selected accounts not
    /// already playing it -- each from its first step, the others playing
    /// on as they are -- or, once every selected one plays it, stop it
    /// everywhere.
    pub fn macro_run(&self, name: &str) -> MacroRun {
        let fresh: Vec<UserId> = self
            .accounts
            .selected()
            .iter()
            .map(|a| a.user_id)
            .filter(|id| !self.macro_runs.get(id).is_some_and(|(_, m)| m == name))
            .collect();
        if !fresh.is_empty() {
            MacroRun::Start(fresh)
        } else if self.macro_runs.values().any(|(_, m)| m == name) {
            MacroRun::Stop
        } else {
            MacroRun::Nothing
        }
    }

    /// Add an activity line; the log keeps the latest [`ACTIVITY_KEPT`].
    pub fn log(&mut self, time: String, line: String) {
        self.activity.insert(0, Activity { time, line });
        self.activity.truncate(ACTIVITY_KEPT);
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
    fn the_status_line_counts_clients_and_macros() {
        let (_d, mut s) = state();
        assert_eq!(s.status_line(), "All idle");
        s.launching.insert(UserId(2));
        assert_eq!(s.status_line(), "1 starting");
        s.running.extend([UserId(1), UserId(2)]);
        assert_eq!(s.status_line(), "2 running, 1 starting");
        s.launching.clear();
        s.macro_runs.insert(UserId(1), (StopFlag::default(), "m".into()));
        assert_eq!(s.status_line(), "2 running · 1 macro playing");
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
    fn the_activity_log_keeps_the_latest_lines_newest_first() {
        let (_d, mut s) = state();
        for n in 0..ACTIVITY_KEPT + 3 {
            s.log("12:00".into(), format!("line {n}"));
        }
        assert_eq!(s.activity.len(), ACTIVITY_KEPT);
        assert_eq!(s.activity[0].line, format!("line {}", ACTIVITY_KEPT + 2));
    }
}
