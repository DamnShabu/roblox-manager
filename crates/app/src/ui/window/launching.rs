//! Launching and stopping: the game bar and target, the launch buttons,
//! session checks, and updating Roblox.

use std::collections::HashSet;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;
use rbxmgr_core::cordial::roblox_build;
use rbxmgr_core::launch::{LaunchAccount, LaunchRequest, Mode};
use rbxmgr_core::roblox::{FAVORITES_SHOWN, Game, Roblox, RobloxError};
use rbxmgr_core::types::{Label, PlaceId, Profile, ServerId, UserId};

use super::Window;
use crate::state::{Chip, FriendTarget, Tile};
use crate::ui::friends::FriendsDialog;

impl Window {
    // -- the game bar and the target ---------------------------------------
    /// Draw the bar from what is on disk: no network, and no keyring prompt
    /// just for opening the app. Refresh is the deliberate click.
    pub fn show_games(&self) {
        {
            let mut s = self.state_mut();
            let icons = &self.0.services.icons;
            s.game_list = s
                .accounts
                .favorite_strip(FAVORITES_SHOWN)
                .into_iter()
                .map(|game| {
                    let icon = icons.cached(&game.universe_id);
                    Tile { game, icon }
                })
                .collect();
            // A redraw must not move the user's pick out from under them.
            let pick = s.place.clone().or_else(|| s.accounts.last_place().cloned());
            s.place = pick.filter(|p| s.game_list.iter().any(|t| &t.game.place_id == p));
        }
        self.0.ui.games.draw(&self.state());
        self.refresh_launch_state();
    }

    pub fn pick_game(&self, place: Option<PlaceId>) {
        {
            let mut s = self.state_mut();
            s.friend = None;
            s.place = place;
        }
        self.0.ui.games.draw(&self.state());
        self.refresh_launch_state();
    }

    pub fn on_friends(&self) {
        let of = {
            let s = self.state();
            let accounts = &s.accounts;
            accounts
                .leader()
                .or_else(|| accounts.selected().into_iter().next())
                .or_else(|| accounts.accounts().first())
                .map(|a| a.user_id)
        };
        match of {
            Some(id) => FriendsDialog::open(self, id),
            None => self.log("Add an account first -- friends come from your accounts"),
        }
    }

    pub fn join_friend(&self, friend: FriendTarget) {
        let display = friend.display.clone();
        self.state_mut().friend = Some(friend);
        self.0.ui.games.draw(&self.state());
        self.refresh_launch_state();
        self.log(&format!("Target: join {display}"));
    }

    /// Everyone's favourites from Roblox, and their icons.
    pub fn reload_games(&self) {
        let accounts: Vec<(UserId, Label)> =
            self.state().accounts.accounts().iter().map(|a| (a.user_id, a.name.clone())).collect();
        if accounts.is_empty() {
            return self.log("Add an account first -- favourites come from your accounts");
        }
        let total = accounts.len();
        let (keyring, roblox, icons, log) = (
            self.services().keyring.clone(),
            self.services().roblox.clone(),
            self.services().icons.clone(),
            self.logger(),
        );
        self.run_task(
            move || {
                let mut fresh: Vec<(UserId, Vec<Game>)> = Vec::new();
                for (id, label) in accounts {
                    let got = keyring.cookie(&label).map_err(|e| e.to_string()).and_then(|c| {
                        roblox.favorites(&c, id, FAVORITES_SHOWN).map_err(|e| e.to_string())
                    });
                    match got {
                        Ok(games) => fresh.push((id, games)),
                        // One account failing must not blank the bar: its
                        // last-known favourites stay in the merge.
                        Err(e) => log.line(format!("Could not load {label}'s favourites: {e}")),
                    }
                }
                let universes: Vec<String> = fresh
                    .iter()
                    .flat_map(|(_, g)| g.iter().map(|g| g.universe_id.clone()))
                    .collect();
                match roblox.icon_urls(&universes) {
                    Ok(urls) => {
                        let failed = urls
                            .iter()
                            .filter_map(|(u, url)| icons.fetch(roblox.transport(), u, url).err())
                            .last();
                        if let Some(e) = failed {
                            log.line(format!("Some game icons did not load: {e}"));
                        }
                    }
                    Err(e) => log.line(format!("Could not load game icons: {e}")),
                }
                fresh
            },
            move |w, fresh| {
                let ok = fresh.len();
                {
                    let mut s = w.state_mut();
                    for (id, games) in fresh {
                        s.accounts.set_favorites(id, games);
                    }
                }
                w.save_accounts();
                w.show_games();
                w.refresh_accounts();
                let n = w.state().game_list.len();
                w.log(&format!("{n} game(s) from {ok}/{total} account(s)"));
            },
        );
    }

    /// The launch buttons, summary, target and select-all, as the state is.
    pub fn refresh_launch_state(&self) {
        let (n, total, leader, followers, target) = {
            let s = self.state();
            let a = &s.accounts;
            (
                a.selected().len(),
                a.accounts().len(),
                a.leader().map(|l| l.name.to_string()),
                a.followers().len(),
                s.target_label(),
            )
        };
        let ui = &self.0.ui;
        ui.btn_each.button.set_sensitive(n >= 1);
        ui.btn_each.set_text(&format!("Launch selected ({n})"));
        ui.btn_group.button.set_sensitive(leader.is_some());
        ui.summary.set_label(&leader.map_or_else(
            || "No leader set".to_owned(),
            |l| format!("Leader {l} · {followers} auto-join"),
        ));
        ui.target_text.set_label(&target);
        let every = total > 0 && n == total;
        ui.select_all.set_text(if every { "Deselect all" } else { "Select all" });
        ui.select_all.set_icon(if every { "remove_done" } else { "done_all" });
        self.refresh_states();
    }

    // -- launching ----------------------------------------------------------
    pub fn launch_selected(&self) {
        let (ids, target) = {
            let s = self.state();
            (s.accounts.selected().iter().map(|a| a.user_id).collect::<Vec<_>>(), s.target())
        };
        if ids.is_empty() {
            return self.log("No accounts selected");
        }
        self.launch(ids, Mode::Each, Some(target));
    }

    /// Launch as group: the leader, then its auto-join list into its server.
    /// With a friend as the target everyone goes to the friend's server.
    pub fn launch_chain(&self) {
        let (ids, friend, target) = {
            let s = self.state();
            let Some(leader) = s.accounts.leader() else {
                drop(s);
                return self.log("No leader set -- make an account the leader first");
            };
            let ids: Vec<UserId> = std::iter::once(leader.user_id)
                .chain(s.accounts.followers().iter().map(|a| a.user_id))
                .collect();
            (ids, s.friend.is_some(), s.target())
        };
        if friend {
            self.launch(ids, Mode::Each, Some(target));
        } else {
            self.launch(ids, Mode::Group, None);
        }
    }

    pub fn launch_group(&self, gid: &str) {
        let found = {
            let s = self.state();
            let a = &s.accounts;
            a.groups().iter().find(|g| g.id == gid).and_then(|g| {
                let place = g.place_id.clone()?;
                let ids: Vec<UserId> = a
                    .visual_order()
                    .iter()
                    .filter(|x| a.group_of(x) == Some(gid))
                    .map(|x| x.user_id)
                    .collect();
                let name = if g.name.is_empty() { "group".to_owned() } else { g.name.clone() };
                let game = g.game.clone().unwrap_or_else(|| place.to_string());
                Some((ids, place, name, game))
            })
        };
        match found {
            Some((ids, place, name, game)) if !ids.is_empty() => {
                self.log(&format!("Launching {name} · {game}"));
                self.launch(ids, Mode::Each, Some((Some(place), None)));
            }
            Some((_, _, name, _)) => self.log(&format!("{name} has no accounts to launch")),
            // No game: the group's settings are where one is picked.
            None => self.log("Pick the group's game in its settings first"),
        }
    }

    /// A row's play: the leader starts its chain; an auto-join account joins
    /// the leader when that is already running; anyone else goes to the
    /// target on their own. A running account's play is stop.
    pub fn play_or_stop(&self, id: UserId) {
        if self.state().chip(id) == Chip::Running {
            return self.stop_account(id);
        }
        let (is_leader, join, target) = {
            let s = self.state();
            let leader = s.accounts.leader();
            let follows = s.accounts.get(id).is_some_and(|a| a.follow.is_some());
            let join = leader
                .filter(|l| follows && s.running.contains(&l.user_id) && s.friend.is_none())
                .map(|l| l.user_id);
            (leader.is_some_and(|l| l.user_id == id), join, s.target())
        };
        match (is_leader, join) {
            (true, _) => self.launch_chain(),
            (false, Some(leader)) => self.launch(vec![leader, id], Mode::Group, None),
            (false, None) => self.launch(vec![id], Mode::Each, Some(target)),
        }
    }

    /// Start a launch at once, whatever else is running. Only an account
    /// another launch is still starting is left out: starting it twice would
    /// have its second client refused by Cordial's profile lock -- and a group
    /// whose leader is left out does not launch, or its first follower would
    /// lead. With no target, the game picked in the bar. A launch into the
    /// bar's pick remembers it for next time.
    pub fn launch(
        &self,
        ids: Vec<UserId>,
        mode: Mode,
        target: Option<(Option<PlaceId>, Option<ServerId>)>,
    ) {
        let (accounts, joining, place, server) = {
            let mut s = self.state_mut();
            let mut accounts = Vec::new();
            for &id in &ids {
                let Some(a) = s.accounts.get(id) else { continue };
                if s.launching.contains(&id) {
                    let line = format!("{}: already launching -- skipped", a.name);
                    self.log(&line);
                    continue;
                }
                let opts =
                    rbxmgr_core::cordial::ClientOpts { nested: a.nested, low_power: a.low_power };
                accounts.push(LaunchAccount { id, label: a.name.clone(), opts });
            }
            let leader_skipped = mode == Mode::Group
                && ids.first().is_some_and(|l| accounts.first().is_none_or(|a| a.id != *l));
            if leader_skipped {
                drop(s);
                return self.log("Its leader is still launching -- try again once it is up");
            }
            let joining: HashSet<UserId> = if mode == Mode::Group {
                accounts.iter().skip(1).map(|a| a.id).collect()
            } else {
                HashSet::new()
            };
            let (place, server) = target.unwrap_or_else(|| (s.place.clone(), None));
            if let Some(p) =
                place.as_ref().filter(|p| server.is_none() && s.place.as_ref() == Some(p))
            {
                s.accounts.remember_place(p);
            }
            s.launching.extend(accounts.iter().map(|a| a.id));
            s.joining.extend(joining.iter().copied());
            (accounts, joining, place, server)
        };
        if accounts.is_empty() {
            return;
        }
        self.refresh_states();
        let ids: Vec<UserId> = accounts.iter().map(|a| a.id).collect();
        let request = LaunchRequest { accounts, mode, place: place.clone(), server };
        let (launcher, log) = (self.services().launcher.clone(), self.logger());
        self.run_task(
            move || launcher.launch(request, &|l| log.line(l)),
            move |w, result| {
                {
                    let mut s = w.state_mut();
                    for id in &ids {
                        s.launching.remove(id);
                    }
                    s.joining.retain(|id| !joining.contains(id));
                    if let Ok(report) = &result {
                        let now = chrono::Utc::now();
                        for id in &report.expired {
                            s.accounts.end_check(*id, Some(false), now);
                        }
                        for (id, user) in &report.launched {
                            s.accounts.record_launch(*id, user, place.as_ref(), now);
                        }
                    }
                }
                if let Err(e) = result {
                    w.log(&format!("Launch failed: {e}"));
                }
                w.save_accounts();
                w.refresh_accounts();
            },
        );
    }

    // -- stopping -----------------------------------------------------------
    pub fn stop_account(&self, id: UserId) {
        let Some(label) = self.state().accounts.get(id).map(|a| a.name.clone()) else { return };
        self.stop_profiles(label.to_string(), [Profile::of(id)].into_iter().collect());
    }

    /// A group header's Shut down: every member's client at once.
    pub fn stop_group(&self, gid: &str) {
        let found = {
            let s = self.state();
            let a = &s.accounts;
            a.groups().iter().find(|g| g.id == gid).map(|g| {
                let name = if g.name.is_empty() { "group".to_owned() } else { g.name.clone() };
                let members: HashSet<Profile> = a
                    .accounts()
                    .iter()
                    .filter(|x| a.group_of(x) == Some(gid))
                    .map(|x| Profile::of(x.user_id))
                    .collect();
                (name, members)
            })
        };
        if let Some((name, members)) = found.filter(|f| !f.1.is_empty()) {
            self.stop_profiles(name, members);
        }
    }

    fn stop_profiles(&self, label: String, which: HashSet<Profile>) {
        let profiles = self.services().profiles.clone();
        let log = self.logger();
        crate::worker::run(
            move || match profiles.stop(&which) {
                Ok(0) => log.line(format!("{label}: was not running")),
                Ok(_) => log.line(format!("Shut down {label}")),
                Err(e) => log.line(format!("{label}: could not stop -- {e}")),
            },
            |()| {},
        );
    }

    pub fn on_stop_all(&self) {
        let profiles: HashSet<Profile> = {
            let s = self.state();
            for (stop, _) in s.macro_runs.values() {
                stop.set();
            }
            s.accounts.accounts().iter().map(|a| Profile::of(a.user_id)).collect()
        };
        let (cordial, log) = (self.services().profiles.clone(), self.logger());
        crate::worker::run(
            move || match cordial.stop(&profiles) {
                Ok(n) => log.line(format!("Stopped {n} client(s)")),
                Err(e) => log.line(format!("Could not stop the clients: {e}")),
            },
            |()| {},
        );
    }

    // -- sessions -----------------------------------------------------------
    /// Ask Roblox whether each stored session still works. Only asks: a
    /// refused one says so on its row, and Sign in again is the fix.
    pub fn check_sessions(&self, ids: Vec<UserId>) {
        let accounts: Vec<(UserId, Label)> = {
            let mut s = self.state_mut();
            let found: Vec<_> = ids
                .iter()
                .filter_map(|id| s.accounts.get(*id).map(|a| (*id, a.name.clone())))
                .collect();
            for (id, _) in &found {
                s.accounts.begin_check(*id);
            }
            found
        };
        self.refresh_accounts();
        let (keyring, roblox, log) =
            (self.services().keyring.clone(), self.services().roblox.clone(), self.logger());
        self.run_task(
            move || {
                accounts
                    .into_iter()
                    .map(|(id, label)| {
                        let verdict = match keyring
                            .cookie(&label)
                            .map_err(|e| e.to_string())
                            .map(|c| roblox.whoami(&c))
                        {
                            Ok(Ok(_)) => Some(true),
                            Ok(Err(RobloxError::Expired)) => {
                                log.line(format!("{label}: session expired -- sign in again"));
                                Some(false)
                            }
                            // Offline is not expired: the last verdict stands.
                            Ok(Err(e)) => {
                                log.line(format!("{label}: could not check the session: {e}"));
                                None
                            }
                            Err(e) => {
                                log.line(format!("{label}: could not check the session: {e}"));
                                None
                            }
                        };
                        (id, verdict)
                    })
                    .collect::<Vec<_>>()
            },
            |w, verdicts| {
                let now = chrono::Utc::now();
                for (id, verdict) in verdicts {
                    w.state_mut().accounts.end_check(id, verdict, now);
                }
                w.save_accounts();
                w.refresh_accounts();
            },
        );
    }

    pub fn refresh_all(&self) {
        let ids: Vec<UserId> = self.state().accounts.accounts().iter().map(|a| a.user_id).collect();
        if !ids.is_empty() {
            self.check_sessions(ids);
        }
        self.reload_games();
    }

    // -- updating Roblox ------------------------------------------------------
    /// Install the newest Roblox build any source has. Launches install one
    /// when there is none, but only this moves to a newer one: Roblox turns
    /// old clients away, so this is the button for "the game says update".
    pub fn on_update_roblox(&self) {
        self.set_update("busy");
        let (runner, log) = (self.services().runner.clone(), self.logger());
        self.run_task(
            move || roblox_build(&*runner, &|l| log.line(l), true),
            |w, got| match got {
                Ok(_) => {
                    w.log("Roblox is up to date");
                    w.set_update("done");
                    let weak = w.weak();
                    glib::timeout_add_local_once(Duration::from_millis(2600), move || {
                        if let Some(w) = weak.upgrade() {
                            w.set_update("idle");
                        }
                    });
                }
                Err(e) => {
                    w.log(&format!("Could not update Roblox: {e}"));
                    w.set_update("idle");
                }
            },
        );
    }

    fn set_update(&self, state: &str) {
        let (ic, text) = match state {
            "busy" => ("sync", "Updating…"),
            "done" => ("check_circle", "Up to date"),
            _ => ("download", "Update Roblox"),
        };
        let upd = &self.0.ui.upd;
        upd.set_icon(ic);
        upd.set_text(text);
        for s in ["busy", "done"] {
            if s == state {
                upd.button.add_css_class(s);
            } else {
                upd.button.remove_css_class(s);
            }
        }
        upd.button.set_sensitive(state != "busy");
    }
}
