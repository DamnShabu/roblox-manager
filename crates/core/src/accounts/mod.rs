//! The accounts and their layout: accounts.json and groups.json, and the
//! only place either is changed. Keyed by Roblox user id -- the label is a
//! name the user can change.

mod labels;
mod layout;
mod model;
mod time;

use std::collections::HashSet;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::json_file;
use crate::paths::Paths;
use crate::roblox::{AccountGames, Game, merge_favorites};
use crate::types::{InvalidLabel, Label, PlaceId, User, UserId};

pub use labels::unique_label;
pub use model::{Account, Group, StoredSession};
pub use time::{relative_time, stamp};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AccountError {
    #[error(transparent)]
    InvalidLabel(#[from] InvalidLabel),
    #[error("'{0}' already exists")]
    LabelTaken(String),
    #[error("no such account")]
    NoSuchAccount,
    #[error("the leader can't be in a group")]
    LeaderInGroup,
    #[error("could not save: {0}")]
    Io(String),
    #[error("could not read the accounts: {0}")]
    Unreadable(String),
}

/// What is known about an account's stored session right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// A check is under way.
    Checking,
    /// Roblox refused it: Sign in again is the fix.
    Expired,
    /// Nothing against it; `checked` is when Roblox last accepted it.
    Ok { checked: Option<String> },
}

/// Every account and group, as loaded, changed only through here.
#[derive(Debug)]
pub struct AccountStore {
    accounts_file: PathBuf,
    groups_file: PathBuf,
    accounts: Vec<Account>,
    groups: Vec<Group>,
    /// Entries that did not read as an account or group, written back as found.
    unread_accounts: Vec<Value>,
    unread_groups: Vec<Value>,
    /// Files that did not parse, moved aside so no save overwrites them.
    set_aside: Vec<PathBuf>,
    /// Sessions being checked. Never saved: a check that dies with the app
    /// must not leave a row stuck on "Checking".
    checking: HashSet<UserId>,
}

impl AccountStore {
    /// The accounts and groups on disk. Before the first groups.json, the old
    /// layout (the first selected account led, the other selected followed) is
    /// carried over and both files are written.
    pub fn load(paths: &Paths) -> Result<Self, AccountError> {
        // An unreadable file is an error, not an empty list: the first save
        // would otherwise replace every account with nothing.
        let unreadable = |e: std::io::Error| AccountError::Unreadable(e.to_string());
        let accounts =
            json_file::read_owned::<Vec<Value>>(&paths.accounts()).map_err(unreadable)?;
        let groups = json_file::read_owned::<Vec<Value>>(&paths.groups()).map_err(unreadable)?;
        let set_aside: Vec<PathBuf> =
            accounts.set_aside.iter().chain(&groups.set_aside).cloned().collect();
        // A groups file that was set aside existed: no first-run migration.
        let first_run = groups.value.is_none() && groups.set_aside.is_none();
        let accounts = accounts.value.unwrap_or_default();
        let groups = groups.value;
        let (accounts, unread_accounts) = model::split_entries(accounts);
        let (groups, unread_groups): (Vec<Group>, _) =
            model::split_entries(groups.unwrap_or_default());
        let (groups, empty): (Vec<Group>, Vec<Group>) =
            groups.into_iter().partition(|g| !g.id.is_empty());
        let empty = empty
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AccountError::Unreadable(e.to_string()))?;
        let mut store = AccountStore {
            accounts_file: paths.accounts(),
            groups_file: paths.groups(),
            accounts,
            groups,
            unread_accounts,
            unread_groups: unread_groups.into_iter().chain(empty).collect(),
            checking: HashSet::new(),
            set_aside,
        };
        if first_run {
            store.migrate_layout();
            store.save()?;
        }
        Ok(store)
    }

    /// Files that did not parse on load and were moved aside, for the UI to
    /// tell the user about.
    pub fn set_aside(&self) -> &[PathBuf] {
        &self.set_aside
    }

    pub fn save(&self) -> Result<(), AccountError> {
        // An entry that does not serialize fails the save rather than
        // quietly going missing from the file.
        fn entries<T: serde::Serialize>(
            items: &[T],
            unread: &[Value],
        ) -> Result<Vec<Value>, AccountError> {
            let mut out = items
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AccountError::Io(e.to_string()))?;
            out.extend(unread.iter().cloned());
            Ok(out)
        }
        let io = |e: std::io::Error| AccountError::Io(e.to_string());
        json_file::write(&self.accounts_file, &entries(&self.accounts, &self.unread_accounts)?)
            .map_err(io)?;
        json_file::write(&self.groups_file, &entries(&self.groups, &self.unread_groups)?)
            .map_err(io)
    }

    /// In list order.
    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    pub fn get(&self, id: UserId) -> Option<&Account> {
        self.accounts.iter().find(|a| a.user_id == id)
    }

    pub fn by_label(&self, label: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.name.as_str() == label)
    }

    fn get_mut(&mut self, id: UserId) -> Result<&mut Account, AccountError> {
        self.accounts.iter_mut().find(|a| a.user_id == id).ok_or(AccountError::NoSuchAccount)
    }

    /// A newly approved account, or one already here refreshed from Roblox --
    /// the same user twice would share one Cordial profile. A new one gets
    /// its username as its label, numbered if taken, and leads if nobody
    /// does. Returns its label and whether it is new.
    pub fn add_or_refresh(&mut self, user: &User, now: DateTime<Utc>) -> (Label, bool) {
        self.checking.remove(&user.id);
        if let Ok(a) = self.get_mut(user.id) {
            a.username = Some(user.name.clone());
            a.display = Some(user.display().to_owned());
            a.session = None;
            a.session_checked = Some(stamp(now));
            return (a.name.clone(), false);
        }
        let taken: Vec<&str> = self.accounts.iter().map(|a| a.name.as_str()).collect();
        let label = unique_label(&user.name, &taken);
        let mut a = Account::new(label.clone(), user.id);
        a.username = Some(user.name.clone());
        a.display = Some(user.display().to_owned());
        a.session_checked = Some(stamp(now));
        self.accounts.push(a);
        if self.leader().is_none() {
            self.make_leader(user.id);
        }
        (label, true)
    }

    /// Relabel an account. Returns the old label; the caller moves its
    /// cookie in the keyring.
    pub fn rename(&mut self, id: UserId, new: &str) -> Result<Label, AccountError> {
        let new = Label::parse(new)?;
        if self.accounts.iter().any(|a| a.name == new && a.user_id != id) {
            return Err(AccountError::LabelTaken(new.to_string()));
        }
        let a = self.get_mut(id)?;
        Ok(std::mem::replace(&mut a.name, new))
    }

    /// Forget an account; the rest of the auto-join order closes up.
    pub fn remove(&mut self, id: UserId) -> Option<Account> {
        let at = self.accounts.iter().position(|a| a.user_id == id)?;
        let gone = self.accounts.remove(at);
        self.checking.remove(&id);
        self.set_follow(id, false);
        Some(gone)
    }

    /// A launch went through: its time, a play of `place`, and the session
    /// known good.
    pub fn record_launch(
        &mut self,
        id: UserId,
        user: &User,
        place: Option<&PlaceId>,
        now: DateTime<Utc>,
    ) {
        self.checking.remove(&id);
        let Ok(a) = self.get_mut(id) else { return };
        a.last_launch = Some(stamp(now));
        a.session_checked = a.last_launch.clone();
        a.session = None;
        a.username = Some(user.name.clone());
        if let Some(place) = place {
            *a.plays.entry(place.to_string()).or_default() += 1;
        }
    }

    pub fn begin_check(&mut self, id: UserId) {
        self.checking.insert(id);
    }

    /// A check ended: `Some(true)` Roblox took the session, `Some(false)` it
    /// refused it, `None` nothing was learned (offline) and the last verdict
    /// stands.
    pub fn end_check(&mut self, id: UserId, accepted: Option<bool>, now: DateTime<Utc>) {
        self.checking.remove(&id);
        let Ok(a) = self.get_mut(id) else { return };
        match accepted {
            Some(true) => {
                a.session = None;
                a.session_checked = Some(stamp(now));
            }
            Some(false) => a.session = Some(StoredSession::Expired),
            None => {}
        }
    }

    pub fn session(&self, id: UserId) -> SessionState {
        if self.checking.contains(&id) {
            return SessionState::Checking;
        }
        match self.get(id) {
            Some(a) if a.session == Some(StoredSession::Expired) => SessionState::Expired,
            Some(a) => SessionState::Ok { checked: a.session_checked.clone() },
            None => SessionState::Ok { checked: None },
        }
    }

    pub fn set_selected(&mut self, ids: &[UserId], on: bool) {
        for a in self.accounts.iter_mut().filter(|a| ids.contains(&a.user_id)) {
            a.selected = on;
        }
    }

    pub fn set_note(&mut self, id: UserId, note: &str) {
        if let Ok(a) = self.get_mut(id) {
            note.clone_into(&mut a.note);
        }
    }

    pub fn set_nested(&mut self, id: UserId, on: bool) {
        if let Ok(a) = self.get_mut(id) {
            a.nested = on;
        }
    }

    pub fn set_low_power(&mut self, id: UserId, on: bool) {
        if let Ok(a) = self.get_mut(id) {
            a.low_power = on;
        }
    }

    /// The macro an account's Run plays. Picking one turns the macro-ready
    /// window on: a macro cannot reach a normal window once you look away.
    pub fn set_macro(&mut self, id: UserId, name: Option<&str>) {
        if let Ok(a) = self.get_mut(id) {
            a.macro_name = name.map(str::to_owned);
            a.nested |= name.is_some();
        }
    }

    pub fn rename_macro(&mut self, old: &str, new: &str) {
        for a in self.accounts.iter_mut().filter(|a| a.macro_name.as_deref() == Some(old)) {
            a.macro_name = Some(new.to_owned());
        }
    }

    pub fn drop_macro(&mut self, name: &str) {
        for a in self.accounts.iter_mut().filter(|a| a.macro_name.as_deref() == Some(name)) {
            a.macro_name = None;
        }
    }

    pub fn set_favorites(&mut self, id: UserId, games: Vec<Game>) {
        if let Ok(a) = self.get_mut(id) {
            a.favorites = games;
        }
    }

    /// The place picked in the game bar, remembered for the next start.
    pub fn remember_place(&mut self, place: &PlaceId) {
        for a in &mut self.accounts {
            a.last_place = Some(place.clone());
        }
    }

    pub fn last_place(&self) -> Option<&PlaceId> {
        self.accounts.iter().find_map(|a| a.last_place.as_ref())
    }

    /// The accounts a join link from the browser starts with, in list order.
    pub fn link_accounts(&self) -> Vec<UserId> {
        self.accounts.iter().filter(|a| a.join_links).map(|a| a.user_id).collect()
    }

    /// Remember `ids`, and only those, for the next join link.
    pub fn set_link_accounts(&mut self, ids: &[UserId]) {
        for a in &mut self.accounts {
            a.join_links = ids.contains(&a.user_id);
        }
    }

    /// Every account's favourites merged into one strip, best first.
    pub fn favorite_strip(&self, limit: usize) -> Vec<Game> {
        let lists: Vec<AccountGames<'_>> = self
            .accounts
            .iter()
            .map(|a| AccountGames { favorites: &a.favorites, plays: &a.plays })
            .collect();
        merge_favorites(&lists, limit)
    }
}

#[cfg(test)]
mod tests;
