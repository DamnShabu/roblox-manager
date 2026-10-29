//! What the window knows: the stores, and everything about this run of the
//! app. Runtime state is keyed by Roblox user id, so relabelling an account
//! strands nothing.

mod views;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rbxmgr_core::accounts::{AccountStore, SessionState};
use rbxmgr_core::cordial::{ClientUsage, MachineLoad, Performance, Window as ClientWindow};
use rbxmgr_core::macros::{MacroLibrary, StopFlag};
use rbxmgr_core::roblox::{Friend, Game};
use rbxmgr_core::types::{PlaceId, ServerId, UserId};
use rbxmgr_core::update::Available;

pub use views::{Need, View};

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

/// What the inspector's Details tab is about, drawn current in the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inspected {
    Account(UserId),
    Group(String),
}

/// How many recent targets the top bar offers.
pub const RECENTS_SHOWN: usize = 4;

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
    /// What the inspector shows, when it is an account or a group.
    pub inspected: Option<Inspected>,
    /// The games picked this run, the latest first.
    pub recent_places: Vec<PlaceId>,
    /// Which accounts the table shows.
    pub view: View,
    /// What each running client takes, from the last reading.
    pub usage: HashMap<UserId, ClientUsage>,
    /// The machine's load, from the last reading.
    pub machine: Option<MachineLoad>,
    /// The level each client launched this run started at: a change to the
    /// account's applies only from its next launch.
    pub launched_as: HashMap<UserId, Performance>,
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
            inspected: None,
            recent_places: Vec::new(),
            view: View::All,
            usage: HashMap::new(),
            machine: None,
            launched_as: HashMap::new(),
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

    /// What closing the window would cut short, said for a confirmation;
    /// None when nothing would be. Macros, launches and updates all run in
    /// this process: closing it stops a macro, leaves a group's remaining
    /// followers unlaunched, and breaks off a download or an unpack.
    pub fn close_warning(&self) -> Option<String> {
        let mut cut = Vec::new();
        match self.macro_runs.len() {
            0 => {}
            1 => cut.push("a macro playing into a client".to_owned()),
            n => cut.push(format!("macros playing into {n} clients")),
        }
        if !self.launches.is_empty() {
            cut.push("a launch still starting accounts".to_owned());
        }
        if self.updating {
            cut.push("an update still installing".to_owned());
        }
        let (last, rest) = cut.split_last()?;
        let what =
            if rest.is_empty() { last.clone() } else { format!("{} and {last}", rest.join(", ")) };
        Some(format!("Closing Roblox Manager stops {what}. Clients already running keep running."))
    }

    /// The top bar's recent targets: the games picked this run, latest
    /// first, then the favourites in their order, up to [`RECENTS_SHOWN`].
    pub fn recents(&self) -> Vec<&Tile> {
        let mut out: Vec<&Tile> = Vec::new();
        let picked = self
            .recent_places
            .iter()
            .filter_map(|p| self.game_list.iter().find(|t| &t.game.place_id == p));
        for t in picked.chain(self.game_list.iter()) {
            if out.len() == RECENTS_SHOWN {
                break;
            }
            if !out.iter().any(|o| o.game.place_id == t.game.place_id) {
                out.push(t);
            }
        }
        out
    }

    /// Remember a game picked, for the recent targets.
    pub fn picked(&mut self, place: &PlaceId) {
        self.recent_places.retain(|p| p != place);
        self.recent_places.insert(0, place.clone());
        self.recent_places.truncate(RECENTS_SHOWN);
    }

    /// Add an activity line; the log keeps the latest [`ACTIVITY_KEPT`].
    pub fn log(&mut self, time: String, line: String) {
        self.activity.insert(0, Activity { time, line });
        self.activity.truncate(ACTIVITY_KEPT);
    }
}

#[cfg(test)]
mod tests;
