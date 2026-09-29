//! The accounts column: drawing it, and everything a row, card or dialog
//! asks of the accounts and groups.

use adw::prelude::*;
use gtk::Align;
use rbxmgr_core::accounts::Account;
use rbxmgr_core::types::{Label, PlaceId, Profile, User, UserId};

use super::Window;
use crate::ui::accounts::group::group_card;
use crate::ui::accounts::leader::leader_card;
use crate::ui::login::AddAccountDialog;
use crate::ui::widgets::{Fluent, LabelFluent, clear, icon, lbl};

impl Window {
    pub fn refresh_accounts(&self) {
        self.0.chips.borrow_mut().clear();
        let accounts_box = &self.0.ui.accounts_box;
        clear(accounts_box);
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
            accounts_box.append(
                &hbox!(
                    8,
                    "dashedbox",
                    icon("person_add", 16, ""),
                    lbl("No accounts yet -- add one with Add account; it signs in with Roblox Quick Login.", "").wrapped()
                )
                .halign(Align::Fill),
            );
            self.refresh_launch_state();
            return;
        };
        accounts_box.append(&leader_card(self, leader.as_ref(), &followers));
        for (i, members) in sections.iter().enumerate() {
            let group = groups.get(i);
            if group.is_none() && members.is_empty() {
                continue;
            }
            accounts_box.append(&group_card(self, group, members));
        }
        self.refresh_launch_state();
    }

    /// Save accounts.json and groups.json; a failure is shown, not lost.
    pub fn save_accounts(&self) {
        if let Err(e) = self.state().accounts.save() {
            self.toast(&e.to_string());
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
            Err(e) => self.toast(&capitalized(&e.to_string())),
        }
    }

    pub fn drop_on_row(&self, id: UserId, onto: UserId) {
        self.state_mut().accounts.drop_on(id, onto);
        self.changed();
    }

    pub fn toggle_selected(&self, id: UserId) {
        let on = self.state().accounts.get(id).is_some_and(|a| a.selected);
        self.state_mut().accounts.set_selected(&[id], !on);
        self.changed();
    }

    pub fn select_accounts(&self, ids: &[UserId], on: bool) {
        self.state_mut().accounts.set_selected(ids, on);
        self.changed();
    }

    pub fn on_select_all(&self) {
        let (ids, every): (Vec<UserId>, bool) = {
            let s = self.state();
            let all = s.accounts.accounts();
            (all.iter().map(|a| a.user_id).collect(), all.iter().all(|a| a.selected))
        };
        self.select_accounts(&ids, !every);
    }

    pub fn toggle_account(&self, id: UserId) {
        {
            let mut s = self.state_mut();
            if !s.open_accounts.remove(&id) {
                s.open_accounts.insert(id);
            }
        }
        self.refresh_accounts();
    }

    // -- groups -----------------------------------------------------------
    pub fn add_group(&self) {
        {
            let mut s = self.state_mut();
            let gid = s.accounts.add_group(chrono::Utc::now());
            s.edit_group = Some(gid);
        }
        self.changed();
    }

    pub fn delete_group(&self, gid: &str) {
        let gone = {
            let mut s = self.state_mut();
            s.edit_group = None;
            s.accounts.delete_group(gid)
        };
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

    pub fn toggle_group_edit(&self, gid: &str) {
        {
            let mut s = self.state_mut();
            s.edit_group =
                if s.edit_group.as_deref() == Some(gid) { None } else { Some(gid.to_owned()) };
        }
        self.refresh_accounts();
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
        } else {
            self.toast(&format!("Signed {label} in again"));
            self.log(&format!("{label}: new session stored in the keyring"));
        }
    }

    /// Relabel an account, and move its keyring entry with it.
    pub fn rename_account(&self, id: UserId, new: &str) {
        let Some(old) = self.label_of(id) else { return };
        if new == old.as_str() {
            return;
        }
        let new = match Label::parse(new) {
            Ok(l) => l,
            Err(e) => return self.log(&e.to_string()),
        };
        if self.state().accounts.by_label(new.as_str()).is_some() {
            return self.log(&format!("'{new}' already exists"));
        }
        if self.state().busy > 0 {
            self.log("Wait for the current task to finish");
            return self.refresh_accounts();
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
                        Err(e) => w.log(&format!("Could not rename '{old}': {e}")),
                    }
                }
                Err(e) => {
                    w.log(&format!("Could not rename '{old}': {e}"));
                    w.refresh_accounts();
                }
            },
        );
    }

    /// Forget an account. Its client goes too -- once the account is gone
    /// Stop all no longer knows its profile -- and the sessions the manager
    /// gave its profile; the empty profile directory stays.
    pub fn remove_account(&self, id: UserId) {
        if self.state().busy > 0 {
            return self.log("Wait for the current task to finish");
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
            s.open_accounts.remove(&id);
        }
        self.save_accounts();
        self.refresh();
        self.toast(&format!("Removed {shown}"));
        self.log(&format!("Removed {shown}"));
    }
}

fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
