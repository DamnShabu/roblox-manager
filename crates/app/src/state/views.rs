//! What the table shows: the sidebar's views of the accounts, what
//! needs the user, and the header's line that sums them up.

use rbxmgr_core::accounts::{Account, SessionState};
use rbxmgr_core::cordial::Performance;
use rbxmgr_core::types::UserId;

use super::AppState;

/// Which accounts the table shows: the sidebar's filters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    All,
    /// Expired sessions and failed launches.
    Attention,
    Running,
    Launching,
    Idle,
    /// One group's accounts; "" is Ungrouped.
    Group(String),
}

/// How many accounts each status filter holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub all: usize,
    pub attention: usize,
    pub running: usize,
    pub launching: usize,
    pub idle: usize,
}

/// Something that needs the user: which account, and what -- its session
/// expired, or (`why`) its last launch failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Need {
    pub id: UserId,
    pub name: String,
    pub expired: bool,
    pub why: String,
}

impl AppState {
    /// Whether an account needs the user: its session expired, or its last
    /// launch failed and it is not running.
    pub fn needs_attention(&self, id: UserId) -> bool {
        self.accounts.session(id) == SessionState::Expired
            || (self.failures.contains_key(&id) && !self.running.contains(&id))
    }

    /// Everything that needs the user now: expired sessions, then failed
    /// launches.
    pub fn needs(&self) -> Vec<Need> {
        let accounts = self.accounts.accounts();
        let expired =
            accounts.iter().filter(|a| self.accounts.session(a.user_id) == SessionState::Expired);
        let failed = accounts.iter().filter_map(|a| {
            let why =
                self.failures.get(&a.user_id).filter(|_| !self.running.contains(&a.user_id))?;
            Some((a, why))
        });
        expired
            .map(|a| Need {
                id: a.user_id,
                name: a.name.to_string(),
                expired: true,
                why: String::new(),
            })
            .chain(failed.map(|(a, why)| Need {
                id: a.user_id,
                name: a.name.to_string(),
                expired: false,
                why: why.clone(),
            }))
            .collect()
    }

    /// Whether the table's view shows `a`. The main account is no group's.
    pub fn in_view(&self, a: &Account) -> bool {
        let id = a.user_id;
        let live = self.running.contains(&id);
        match &self.view {
            View::All => true,
            View::Attention => self.needs_attention(id),
            View::Running => live,
            View::Launching => !live && self.launching.contains(&id),
            View::Idle => !live && !self.launching.contains(&id),
            View::Group(g) => !a.leader && self.accounts.group_of(a).unwrap_or_default() == g,
        }
    }

    /// How many accounts each status filter holds.
    pub fn counts(&self) -> Counts {
        let ids: Vec<UserId> = self.accounts.accounts().iter().map(|a| a.user_id).collect();
        let n = |f: &dyn Fn(UserId) -> bool| ids.iter().filter(|id| f(**id)).count();
        let live = |id: UserId| self.running.contains(&id);
        Counts {
            all: ids.len(),
            attention: n(&|id| self.needs_attention(id)),
            running: n(&live),
            launching: n(&|id| !live(id) && self.launching.contains(&id)),
            idle: n(&|id| !live(id) && !self.launching.contains(&id)),
        }
    }

    /// What the table's view is called.
    pub fn view_title(&self) -> String {
        match &self.view {
            View::All => "All accounts".to_owned(),
            View::Attention => "Needs attention".to_owned(),
            View::Running => "Running".to_owned(),
            View::Launching => "Launching".to_owned(),
            View::Idle => "Idle".to_owned(),
            View::Group(g) if g.is_empty() => "Ungrouped".to_owned(),
            View::Group(g) => self.accounts.groups().iter().find(|x| &x.id == g).map_or_else(
                || "All accounts".to_owned(),
                |x| if x.name.is_empty() { "Untitled group".to_owned() } else { x.name.clone() },
            ),
        }
    }

    /// The level a running client still plays at when its account's has
    /// changed since it launched: a relaunch applies the account's.
    pub fn stale_performance(&self, id: UserId) -> Option<Performance> {
        let started = *self.launched_as.get(&id)?;
        let now = self.accounts.get(id)?.performance();
        (self.running.contains(&id) && started != now).then_some(started)
    }

    /// The header's line: "3 running · 1 starting · 2 need attention", or
    /// "All idle".
    pub fn status_line(&self) -> String {
        let c = self.counts();
        let mut parts = Vec::new();
        if c.running > 0 {
            parts.push(format!("{} running", c.running));
        }
        if c.launching > 0 {
            parts.push(format!("{} starting", c.launching));
        }
        match c.attention {
            0 => {}
            1 => parts.push("1 needs attention".to_owned()),
            n => parts.push(format!("{n} need attention")),
        }
        if parts.is_empty() { "All idle".to_owned() } else { parts.join(" · ") }
    }
}
