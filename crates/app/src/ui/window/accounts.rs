//! The accounts page: drawing it, and everything a row, section or dialog
//! asks of the accounts and groups.

use adw::prelude::*;
use rbxmgr_core::accounts::Account;
use rbxmgr_core::types::{Label, PlaceId, Profile, User, UserId};

use super::Window;
use crate::ui::accounts::group::group_section;
use crate::ui::accounts::group_settings::GroupSettings;
use crate::ui::accounts::leader::{leader_section, placeholder};
use crate::ui::accounts::row::account_row;
use crate::ui::confirm;
use crate::ui::login::AddAccountDialog;
use crate::ui::widgets::{boxed_list, clear, plural, section_header, sentence};

impl Window {
    pub fn refresh_accounts(&self) {
        self.0.rows.borrow_mut().clear();
        let ui = &self.0.ui;
        clear(&ui.accounts_box);
        let (leader, followers, groups, sections) = {
            let s = self.state();
            let store = &s.accounts;
            let group_name = |a: &Account| {
                store
                    .group_of(a)
                    .and_then(|gid| store.groups().iter().find(|g| g.id == gid))
                    .map_or_else(
                        || "Ungrouped".to_owned(),
                        |g| {
                            if g.name.is_empty() { "Untitled group".into() } else { g.name.clone() }
                        },
                    )
            };
            let followers: Vec<(Account, String)> =
                store.followers().into_iter().map(|a| (a.clone(), group_name(a))).collect();
            let visual = store.visual_order();
            let sections: Vec<Vec<Account>> = store
                .groups()
                .iter()
                .map(|g| Some(g.id.as_str()))
                .chain([None])
                .map(|gid| {
                    visual
                        .iter()
                        .filter(|a| store.group_of(a) == gid)
                        .map(|a| (*a).clone())
                        .collect()
                })
                .collect();
            let empty = store.accounts().is_empty();
            (
                if empty { None } else { Some(store.leader().cloned()) },
                followers,
                store.groups().to_vec(),
                sections,
            )
        };
        let Some(leader) = leader else {
            ui.pages.set_visible_child_name("welcome");
            ui.launch_bar.set_visible(false);
            self.refresh_launch_state();
            return;
        };
        ui.pages.set_visible_child_name("accounts");
        ui.launch_bar.set_visible(true);
        if !self.state().filter.is_empty() {
            ui.accounts_box.append(&self.search_results());
            self.refresh_launch_state();
            return;
        }
        ui.accounts_box.append(&leader_section(self, leader.as_ref(), &followers));
        for (i, members) in sections.iter().enumerate() {
            let group = groups.get(i);
            if group.is_none() && members.is_empty() {
                continue;
            }
            ui.accounts_box.append(&group_section(self, group, members));
        }
        self.refresh_launch_state();
    }

    /// Search the accounts: `query` narrows the page to the ones it finds.
    pub fn set_filter(&self, query: &str) {
        let query = query.trim().to_lowercase();
        if self.state().filter == query {
            return;
        }
        self.state_mut().filter = query;
        self.refresh_accounts();
    }

    /// Open the search bar, or focus it when it is open.
    pub fn start_search(&self) {
        self.0.ui.search_bar.set_search_mode(true);
        self.0.ui.search.grab_focus();
    }

    /// The accounts the search finds, as one list.
    fn search_results(&self) -> gtk::Box {
        let found: Vec<Account> = {
            let s = self.state();
            s.matching().into_iter().filter_map(|id| s.accounts.get(id).cloned()).collect()
        };
        let list = boxed_list();
        for a in &found {
            list.append(&account_row(self, a));
        }
        if found.is_empty() {
            list.append(&placeholder("system-search-symbolic", "No account matches that search."));
        }
        vbox!(8, "", section_header(&plural(found.len(), "match", "matches"), None, &[]), list)
    }

    /// Save accounts.json and groups.json; a failure is shown, not lost.
    pub fn save_accounts(&self) {
        if let Err(e) = self.state().accounts.save() {
            self.toast(&sentence(&e.to_string()));
            self.log(&e.to_string());
        }
    }

    /// Layout or selection changed: save, redraw the accounts.
    fn changed(&self) {
        self.save_accounts();
        self.refresh_accounts();
    }

    fn label_of(&self, id: UserId) -> Option<Label> {
        self.state().accounts.get(id).map(|a| a.name.clone())
    }

    // -- layout -----------------------------------------------------------
    pub fn set_leader(&self, id: UserId) {
        self.state_mut().accounts.make_leader(id);
        self.changed();
        if let Some(l) = self.label_of(id) {
            self.log(&format!("Leader set to {l}"));
        }
    }

    pub fn set_follow(&self, id: UserId, on: bool) {
        self.state_mut().accounts.set_follow(id, on);
        self.changed();
    }

    pub fn move_follower(&self, id: UserId, delta: isize) {
        self.state_mut().accounts.move_follower(id, delta);
        self.changed();
    }

    pub fn set_group(&self, id: UserId, group: Option<String>) {
        let refused = self.state_mut().accounts.set_group(id, group.as_deref());
        match refused {
            Ok(()) => self.changed(),
            Err(e) => self.toast(&sentence(&e.to_string())),
        }
    }

    pub fn drop_on_row(&self, id: UserId, onto: UserId) {
        self.state_mut().accounts.drop_on(id, onto);
        self.changed();
    }

    /// Selection is drawn by the rows' redraws: nothing is rebuilt, so
    /// a click keeps its place and its focus.
    pub fn select_accounts(&self, ids: &[UserId], on: bool) {
        let changed = {
            let s = self.state();
            ids.iter().any(|id| s.accounts.get(*id).is_some_and(|a| a.selected != on))
        };
        if !changed {
            return;
        }
        self.state_mut().accounts.set_selected(ids, on);
        self.save_accounts();
        self.refresh_launch_state();
    }

    pub fn on_select_all(&self) {
        let (ids, every): (Vec<UserId>, bool) = {
            let s = self.state();
            let all = s.accounts.accounts();
            (all.iter().map(|a| a.user_id).collect(), all.iter().all(|a| a.selected))
        };
        self.select_accounts(&ids, !every);
    }

    // -- groups -----------------------------------------------------------
    /// A new group, and its settings open to name it.
    pub fn add_group(&self) {
        let gid = self.state_mut().accounts.add_group(chrono::Utc::now());
        self.changed();
        GroupSettings::open(self, &gid);
    }

    pub fn confirm_delete_group(&self, gid: &str) {
        self.confirm_delete_group_then(gid, || {});
    }

    /// Ask, then delete the group and run `after`.
    pub fn confirm_delete_group_then(&self, gid: &str, after: impl Fn() + 'static) {
        let Some((name, n)) = ({
            let s = self.state();
            let a = &s.accounts;
            a.groups().iter().find(|g| g.id == gid).map(|g| {
                let n = a.accounts().iter().filter(|x| a.group_of(x) == Some(gid)).count();
                (if g.name.is_empty() { "Untitled group".to_owned() } else { g.name.clone() }, n)
            })
        }) else {
            return;
        };
        let body = match n {
            0 => "It has no accounts.".to_owned(),
            1 => "Its account moves to Ungrouped; it is not removed.".to_owned(),
            n => format!("Its {n} accounts move to Ungrouped; none is removed."),
        };
        let gid = gid.to_owned();
        confirm::ask(self, &format!("Delete {name}?"), &body, "_Delete", move |w| {
            w.delete_group(&gid);
            after();
        });
    }

    pub fn delete_group(&self, gid: &str) {
        let gone = self.state_mut().accounts.delete_group(gid);
        self.changed();
        self.show_games();
        let name = gone
            .map(|g| g.name)
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Untitled group".into());
        self.log(&format!("Deleted group {name}"));
    }

    pub fn toggle_group_open(&self, gid: Option<String>) {
        {
            let mut s = self.state_mut();
            match gid {
                None => s.ungrouped_open = !s.ungrouped_open,
                Some(gid) => {
                    let open =
                        s.accounts.groups().iter().find(|g| g.id == gid).is_some_and(|g| g.open);
                    s.accounts.set_group_open(&gid, !open);
                }
            }
        }
        self.changed();
    }

    pub fn rename_group(&self, gid: &str, name: &str) {
        self.state_mut().accounts.rename_group(gid, name);
        self.save_accounts();
    }

    pub fn set_group_game(&self, gid: &str, game: Option<(PlaceId, String)>) {
        self.state_mut().accounts.set_group_game(gid, game);
        self.changed();
        self.show_games();
    }

    // -- one account --------------------------------------------------------
    pub fn set_note(&self, id: UserId, note: &str) {
        self.state_mut().accounts.set_note(id, note);
        self.save_accounts();
    }

    pub fn set_nested(&self, id: UserId, on: bool) {
        self.state_mut().accounts.set_nested(id, on);
        self.save_accounts();
    }

    pub fn set_low_power(&self, id: UserId, on: bool) {
        self.state_mut().accounts.set_low_power(id, on);
        self.changed();
    }

    pub fn on_add(&self) {
        AddAccountDialog::open(self, None);
    }

    /// An approved Quick Login whose cookie is now in the keyring under
    /// `label`: saved, drawn, and said.
    pub fn after_sign_in(&self, label: &Label, user: &User, new: bool) {
        self.save_accounts();
        self.refresh();
        if new {
            self.toast(&format!("Added {label}"));
            self.log(&format!("Added {label} (@{})", user.name));
            // Its favourites join the strip, and its picture its row.
            self.reload_games();
        } else {
            self.toast(&format!("Signed {label} in again"));
            self.log(&format!("{label}: new session stored in the keyring"));
        }
    }

    /// A rename that failed after its session moved: the session goes back
    /// under the label the account still has, or every later read of it
    /// would find nothing and the account would look signed out.
    fn move_cookie_back(&self, from: Label, to: Label) {
        let keyring = self.services().keyring.clone();
        let weak = self.weak();
        crate::worker::run(
            move || keyring.move_cookie(&from, &to).map(|()| to),
            move |back| {
                let Some(w) = weak.upgrade() else { return };
                if let Err(e) = back {
                    w.log(&format!("Could not move the session back after a failed rename: {e}"));
                    w.toast("The account's session could not be moved back: sign in again");
                }
            },
        );
    }

    /// Relabel an account, and move its keyring entry with it.
    pub fn rename_account(&self, id: UserId, new: &str) {
        let Some(old) = self.label_of(id) else { return };
        if new == old.as_str() {
            return;
        }
        let new = match Label::parse(new) {
            Ok(l) => l,
            Err(e) => return self.toast(&e.to_string()),
        };
        if self.state().accounts.by_label(new.as_str()).is_some() {
            return self.toast(&format!("An account is already called {new}"));
        }
        if self.state().busy > 0 {
            return self.toast("Wait for the current task to finish, then rename it");
        }
        let keyring = self.services().keyring.clone();
        let (from, to) = (old.clone(), new.clone());
        self.run_task(
            move || keyring.move_cookie(&from, &to),
            move |w, moved| match moved {
                Ok(()) => {
                    let renamed = w.state_mut().accounts.rename(id, new.as_str());
                    match renamed {
                        Ok(_) => {
                            w.save_accounts();
                            w.refresh();
                            w.toast(&format!("Renamed {old} → {new}"));
                            w.log(&format!("Renamed {old} → {new}"));
                        }
                        Err(e) => {
                            w.toast(&format!("Could not rename {old}"));
                            w.log(&format!("Could not rename '{old}': {e}"));
                            w.move_cookie_back(new, old);
                        }
                    }
                }
                Err(e) => {
                    w.toast(&format!("Could not rename {old}"));
                    w.log(&format!("Could not rename '{old}': {e}"));
                    w.refresh_accounts();
                }
            },
        );
    }

    pub fn confirm_remove(&self, id: UserId) {
        self.confirm_remove_then(id, || {});
    }

    /// Ask, then remove the account and run `after`.
    pub fn confirm_remove_then(&self, id: UserId, after: impl Fn() + 'static) {
        let Some(label) = self.label_of(id) else { return };
        confirm::ask(
            self,
            &format!("Remove {label}?"),
            "Its session leaves the keyring and its client is closed. The Roblox account itself \
             is untouched; add it again any time with Quick Login.",
            "_Remove",
            move |w| {
                w.remove_account(id);
                after();
            },
        );
    }

    /// Why the account's last launch failed, in full.
    pub fn show_failure(&self, id: UserId) {
        let (label, why) = {
            let s = self.state();
            (s.accounts.get(id).map(|a| a.name.to_string()), s.failures.get(&id).cloned())
        };
        if let (Some(label), Some(why)) = (label, why) {
            confirm::tell(self, &format!("{label} Did Not Launch"), &sentence(&why));
        }
    }

    /// Forget an account. Its client goes too -- once the account is gone
    /// Stop all no longer knows its profile -- and the sessions the manager
    /// gave its profile; the empty profile directory stays.
    pub fn remove_account(&self, id: UserId) {
        if self.state().busy > 0 {
            return self.toast("Wait for the current task to finish, then remove it");
        }
        let Some(label) = self.label_of(id) else { return };
        let playing = self.state_mut().macro_runs.remove(&id);
        if let Some((stop, _)) = playing {
            stop.set();
        }
        let (profiles, keyring) =
            (self.services().profiles.clone(), self.services().keyring.clone());
        let (gone, shown) = (label.clone(), label.clone());
        self.run_task(
            move || {
                let profile = [Profile::of(id)].into_iter().collect();
                let steps = [
                    profiles.stop(&profile).map(|_| ()).map_err(|e| e.to_string()),
                    profiles.clear(id).map_err(|e| e.to_string()),
                    keyring.drop_cookie(&gone).map_err(|e| e.to_string()),
                ];
                steps.into_iter().filter_map(Result::err).collect::<Vec<_>>()
            },
            move |w, errors| {
                for e in errors {
                    w.log(&format!("{label}: could not finish removing it: {e}"));
                }
            },
        );
        {
            let mut s = self.state_mut();
            s.accounts.remove(id);
            s.failures.remove(&id);
        }
        self.save_accounts();
        self.refresh();
        self.toast(&format!("Removed {shown}"));
        self.log(&format!("Removed {shown}"));
    }
}
