//! The accounts page: drawing it for the current view, and everything a
//! row, a group's row, the sidebar or a dialog asks of the accounts and
//! groups.

use std::collections::HashSet;

use adw::prelude::*;
use rbxmgr_core::accounts::{Account, Group};
use rbxmgr_core::types::{Label, PlaceId, Profile, User, UserId};

use super::Window;
use crate::state::{Need, View};
use crate::ui::accounts::group_settings::GroupSettings;
use crate::ui::confirm;
use crate::ui::ds;
use crate::ui::login::AddAccountDialog;
use crate::ui::table::attention::attention_section;
use crate::ui::table::band::group_band;
use crate::ui::table::row::account_row;
use crate::ui::widgets::{Fluent, LabelFluent, clear, lbl, sentence};

/// One group's part of the table: the group (None for Ungrouped), all its
/// members, and the ones the view shows.
type Section = (Option<Group>, Vec<Account>, Vec<Account>);

impl Window {
    pub fn refresh_accounts(&self) {
        self.0.rows.borrow_mut().clear();
        let ui = &self.0.ui;
        clear(&ui.table);
        ui.sidebar.draw_groups(self);
        let (empty, needs, main, sections, move_to) = {
            let s = self.state();
            let store = &s.accounts;
            let all = s.view == View::All;
            let attention: HashSet<UserId> = s.needs().iter().map(|n| n.id).collect();
            let needs: Vec<Need> =
                if matches!(s.view, View::All | View::Attention) { s.needs() } else { Vec::new() };
            // In the All view, what needs attention is drawn there, not twice.
            let pulled = |a: &Account| all && attention.contains(&a.user_id);
            let main = store.leader().filter(|l| all && !pulled(l)).cloned();
            let visual = store.visual_order();
            let sections: Vec<Section> = store
                .groups()
                .iter()
                .map(|g| Some(g.id.as_str()))
                .chain([None])
                .filter_map(|gid| {
                    let members: Vec<Account> = visual
                        .iter()
                        .copied()
                        .filter(|a| store.group_of(a) == gid)
                        .cloned()
                        .collect();
                    let shown: Vec<Account> =
                        members.iter().filter(|a| s.in_view(a) && !pulled(a)).cloned().collect();
                    let group =
                        gid.and_then(|g| store.groups().iter().find(|x| x.id == g)).cloned();
                    let keep = match &s.view {
                        View::All => group.is_some() || !members.is_empty(),
                        View::Group(v) => gid.unwrap_or_default() == v,
                        View::Attention => false,
                        View::Running | View::Launching | View::Idle => !shown.is_empty(),
                    };
                    keep.then_some((group, members, shown))
                })
                .collect();
            let move_to: Vec<(String, String)> = store
                .groups()
                .iter()
                .map(|g| {
                    let name =
                        if g.name.is_empty() { "Untitled group".into() } else { g.name.clone() };
                    (g.id.clone(), name)
                })
                .chain([(String::new(), "Ungrouped".to_owned())])
                .collect();
            (store.accounts().is_empty(), needs, main, sections, move_to)
        };
        if empty {
            ui.pages.set_visible_child_name("welcome");
            self.refresh_launch_state();
            // The one thing to do first: the sign-in is ready beside it.
            if self.0.ui.panel.borrow().is_none() && self.seen() {
                AddAccountDialog::open(self, None);
            }
            return;
        }
        ui.pages.set_visible_child_name("accounts");
        ui.toolbar.set_groups(&move_to, self.state().accounts.selected().len());
        if !self.state().filter.is_empty() {
            self.search_results();
            self.refresh_launch_state();
            return;
        }
        if !needs.is_empty() {
            ui.table.append(&attention_section(self, &needs));
        }
        if let Some(main) = &main {
            ui.table.append(&account_row(self, main));
        }
        for (group, members, shown) in &sections {
            ui.table.append(&group_band(self, group.as_ref(), members, shown));
        }
        if needs.is_empty() && main.is_none() && sections.is_empty() {
            ui.table.append(&self.empty_view());
        }
        self.draw_details();
        self.refresh_launch_state();
    }

    /// Show `view` in the table.
    pub fn set_view(&self, view: View) {
        if self.state().view == view {
            return;
        }
        self.state_mut().view = view;
        self.refresh_accounts();
    }

    /// What a view with no account in it says.
    fn empty_view(&self) -> gtk::Box {
        let text = match self.state().view {
            View::Attention => "Nothing needs attention",
            View::Running => "No account is running",
            View::Launching => "No account is launching",
            View::Idle => "Every account is running",
            View::All | View::Group(_) => "No accounts here",
        };
        let empty = gtk::Box::new(gtk::Orientation::Vertical, 8).css("cx-empty");
        empty.append(&lbl(text, "cx-empty-t").xalign(0.5));
        let all = ds::Button::new("Show all accounts", ds::Variant::Secondary, true)
            .on(self.act(|w| w.set_view(View::All)));
        all.button.set_halign(gtk::Align::Center);
        empty.append(&all.button);
        empty
    }

    /// Search the accounts: `query` narrows the table to the ones it finds.
    pub fn set_filter(&self, query: &str) {
        let query = query.trim().to_lowercase();
        if self.state().filter == query {
            return;
        }
        self.state_mut().filter = query;
        self.refresh_accounts();
    }

    /// Focus the search.
    pub fn start_search(&self) {
        self.0.ui.top.search.grab_focus();
    }

    /// The accounts the search finds, as plain rows.
    fn search_results(&self) {
        let found: Vec<Account> = {
            let s = self.state();
            s.matching().into_iter().filter_map(|id| s.accounts.get(id).cloned()).collect()
        };
        let table = &self.0.ui.table;
        for a in &found {
            table.append(&account_row(self, a));
        }
        if found.is_empty() {
            let empty = gtk::Box::new(gtk::Orientation::Vertical, 8).css("cx-empty");
            empty.append(&lbl("No account matches that search", "cx-empty-t").xalign(0.5));
            empty.append(
                &lbl("Search looks at each account's name, Roblox user and note.", "").xalign(0.5),
            );
            let clear_it = ds::Button::new("Clear search", ds::Variant::Secondary, true)
                .on(self.act(|w| {
                    w.0.ui.top.search.set_text("");
                }));
            clear_it.button.set_halign(gtk::Align::Center);
            empty.append(&clear_it.button);
            table.append(&empty);
        }
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
    /// Pin `id` as the main account: drawn first, out of every group.
    pub fn set_main(&self, id: UserId) {
        self.state_mut().accounts.make_leader(id);
        self.changed();
        if let Some(l) = self.label_of(id) {
            self.log(&format!("Main account set to {l}"));
        }
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

    /// Select every account the view shows, or none of them.
    pub fn select_every(&self, on: bool) {
        let ids: Vec<UserId> = {
            let s = self.state();
            s.accounts.accounts().iter().filter(|a| s.in_view(a)).map(|a| a.user_id).collect()
        };
        self.select_accounts(&ids, on);
    }

    /// Select every account in a group.
    pub fn select_group(&self, gid: &str) {
        let ids: Vec<UserId> =
            self.state().accounts.group_members(gid).iter().map(|a| a.user_id).collect();
        self.select_accounts(&ids, true);
    }

    /// Move every selected account to a group ("" is Ungrouped).
    pub fn move_selected(&self, gid: &str) {
        let ids: Vec<UserId> = self.state().accounts.selected().iter().map(|a| a.user_id).collect();
        for id in ids {
            let refused =
                self.state_mut().accounts.set_group(id, Some(gid).filter(|g| !g.is_empty()));
            if let Err(e) = refused {
                self.toast(&sentence(&e.to_string()));
            }
        }
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

    // -- groups -----------------------------------------------------------
    /// A new group, and its settings open to name it.
    pub fn add_group(&self) {
        let gid = self.state_mut().accounts.add_group(chrono::Utc::now());
        self.changed();
        GroupSettings::open(self, &gid);
    }

    /// Delete group…: the confirmation under its band, or a dialog when
    /// the band is not drawn.
    pub fn confirm_delete_group(&self, gid: &str) {
        if !self.open_group_confirm(gid) {
            self.confirm_delete_group_then(gid, || {});
        }
    }

    /// Ask, then delete the group and run `after`.
    pub fn confirm_delete_group_then(&self, gid: &str, after: impl Fn() + 'static) {
        let Some((name, n)) = ({
            let s = self.state();
            let a = &s.accounts;
            a.groups().iter().find(|g| g.id == gid).map(|g| {
                let n = a.group_members(gid).len();
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
        self.load_missing_game_icons();
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

    /// Its rows and the details redraw it: nothing is rebuilt.
    pub fn set_performance(&self, id: UserId, level: rbxmgr_core::cordial::Performance) {
        self.state_mut().accounts.set_performance(id, level);
        self.save_accounts();
        self.refresh_states();
    }

    /// One level for every selected account.
    pub fn set_performance_selected(&self, level: rbxmgr_core::cordial::Performance) {
        let ids: Vec<UserId> = self.state().accounts.selected().iter().map(|a| a.user_id).collect();
        for id in &ids {
            self.state_mut().accounts.set_performance(*id, level);
        }
        self.save_accounts();
        self.refresh_states();
        self.toast(&format!(
            "{} for {}; it applies when each next launches",
            level.label(),
            crate::ui::widgets::plural(ids.len(), "account", "accounts")
        ));
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

    /// Remove…: the confirmation under its row, or a dialog when the row
    /// is not drawn.
    pub fn confirm_remove(&self, id: UserId) {
        if !self.open_confirm(id) {
            self.confirm_remove_then(id, || {});
        }
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
            move |w, errors: Vec<String>| {
                for e in &errors {
                    w.log(&format!("{label}: could not finish removing it: {e}"));
                }
                // Removing promised its session leaves the keyring: a part
                // that did not happen is said where it is seen, not only
                // in the log.
                if !errors.is_empty() {
                    w.toast_with(
                        &format!("{label} was removed, but not everything of it went"),
                        "Details",
                        crate::ui::activity::open_log,
                    );
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
