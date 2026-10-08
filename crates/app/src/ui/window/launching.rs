//! Launching and stopping: the game strip and the target, the launch bar,
//! and each way accounts are started and stopped.

use std::collections::HashSet;

use rbxmgr_core::launch::{LaunchAccount, LaunchReport, LaunchRequest, Mode};
use rbxmgr_core::roblox::FAVORITES_SHOWN;
use rbxmgr_core::stop::StopFlag;
use rbxmgr_core::types::{PlaceId, Profile, ServerId, UserId};

use super::Window;
use crate::state::{Chip, FriendTarget, Tile};
use crate::ui::activity;
use crate::ui::friends::FriendsDialog;
use crate::ui::widgets::plural;

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
            None => self.toast("Add an account first: friends come from your accounts"),
        }
    }

    pub fn join_friend(&self, friend: FriendTarget) {
        let display = friend.display.clone();
        self.state_mut().friend = Some(friend);
        self.0.ui.games.draw(&self.state());
        self.refresh_launch_state();
        self.log(&format!("Target: join {display}"));
    }

    /// The launch bar, the accounts' heading and select-all, as the state is.
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
        self.set_action_enabled("refresh", total > 0);
        self.set_action_enabled("reload-games", total > 0);
        self.set_action_enabled("launch-selected", n >= 1);
        self.set_action_enabled("launch-group", leader.is_some());
        ui.btn_each.set_text(&if n == 0 {
            "Launch Selected".to_owned()
        } else {
            format!("Launch {n} Selected")
        });
        ui.summary.set_label(&leader.map_or_else(
            || "No leader yet".to_owned(),
            |l| match followers {
                0 => format!("{l} leads"),
                n => format!("{l} leads · {} follow", plural(n, "account", "accounts")),
            },
        ));
        ui.target_text.set_label(&format!("Into {target}"));
        ui.accounts_meta
            .set_label(&format!("{} · {n} selected", plural(total, "account", "accounts")));
        let every = total > 0 && n == total;
        ui.select_all.set_text(if every { "Select None" } else { "Select All" });
        ui.select_all.set_icon(if every {
            "edit-clear-all-symbolic"
        } else {
            "edit-select-all-symbolic"
        });
        self.refresh_states();
    }

    // -- launching ----------------------------------------------------------
    pub fn launch_selected(&self) {
        let (ids, target) = {
            let s = self.state();
            (s.accounts.selected().iter().map(|a| a.user_id).collect::<Vec<_>>(), s.target())
        };
        if ids.is_empty() {
            return self.toast("Select the accounts to launch");
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
                return self.toast("Make an account the leader first");
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
                let ids: Vec<UserId> = a.group_members(gid).iter().map(|x| x.user_id).collect();
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
            Some((_, _, name, _)) => self.toast(&format!("{name} has no accounts to launch")),
            // No game: the group's settings are where one is picked.
            None => self.toast("Choose the group's game in its settings first"),
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
        let display_hz = monitor_hz();
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
                let opts = rbxmgr_core::cordial::ClientOpts {
                    nested: a.nested,
                    performance: a.performance(),
                    display_hz,
                };
                accounts.push(LaunchAccount { id, label: a.name.clone(), opts });
            }
            let leader_skipped = mode == Mode::Group
                && ids.first().is_some_and(|l| accounts.first().is_none_or(|a| a.id != *l));
            if leader_skipped {
                drop(s);
                self.surface();
                return self.toast("Its leader is still starting: try again once it is up");
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
        let ids: Vec<UserId> = accounts.iter().map(|a| a.id).collect();
        let stop = StopFlag::default();
        self.state_mut().launches.push((stop.clone(), ids.clone()));
        self.refresh_states();
        let request =
            LaunchRequest { accounts, mode, place: place.clone(), server, stop: stop.clone() };
        let (launcher, log) = (self.services().launcher.clone(), self.logger());
        self.run_task(
            crate::worker::catching(move || launcher.launch(request, &|l| log.line(l))),
            move |w, result| {
                {
                    let mut s = w.state_mut();
                    s.launches.retain(|(flag, _)| !flag.same_as(&stop));
                    for id in &ids {
                        s.launching.remove(id);
                    }
                    s.joining.retain(|id| !joining.contains(id));
                    if let Ok(Ok(report)) = &result {
                        let now = chrono::Utc::now();
                        for id in &report.expired {
                            s.accounts.end_check(*id, Some(false), now);
                            s.failures.remove(id);
                        }
                        for (id, user) in &report.launched {
                            s.accounts.record_launch(*id, user, place.as_ref(), now);
                            s.failures.remove(id);
                            match &place {
                                Some(p) => s.playing.insert(*id, p.clone()),
                                None => s.playing.remove(id),
                            };
                        }
                        for (id, why) in &report.failed {
                            s.failures.insert(*id, why.clone());
                        }
                    }
                }
                match result {
                    Ok(Ok(report)) => w.tell_report(&report),
                    Err(crashed) => {
                        w.surface();
                        w.log(&format!("Launch failed: {crashed}"));
                        w.toast_with("The launch stopped short", "Details", activity::open_log);
                    }
                    Ok(Err(e)) => {
                        w.surface();
                        w.log(&format!("Launch failed: {e}"));
                        w.toast_with(
                            "Nothing launched: Roblox could not be installed",
                            "Details",
                            activity::open_log,
                        );
                    }
                }
                w.save_accounts();
                w.refresh_accounts();
            },
        );
    }

    /// A toast for a launch that did not all go through; one that did is
    /// told by its rows turning to Running.
    fn tell_report(&self, report: &LaunchReport) {
        let (ok, expired, failed, stopped) = (
            report.launched.len(),
            report.expired.len(),
            report.failed.len(),
            report.cancelled.len(),
        );
        if expired + failed + stopped == 0 {
            if ok > 0 {
                self.notify("Launched", &format!("{} up", plural(ok, "client", "clients")));
            }
            return;
        }
        self.surface();
        if expired + failed == 0 {
            return self.toast(&format!("Launch stopped · {stopped} not started"));
        }
        let mut parts = Vec::new();
        if ok > 0 {
            parts.push(format!("{ok} launched"));
        }
        if expired > 0 {
            parts.push(format!("{expired} expired"));
        }
        if failed > 0 {
            parts.push(format!("{failed} failed"));
        }
        if stopped > 0 {
            parts.push(format!("{stopped} not started"));
        }
        self.toast_with(&parts.join(" · "), "Details", activity::open_log);
        self.notify("Not every account launched", &parts.join(" · "));
    }

    // -- stopping -----------------------------------------------------------
    pub fn stop_account(&self, id: UserId) {
        let Some(label) = self.state().accounts.get(id).map(|a| a.name.clone()) else { return };
        self.stop_profiles(label.to_string(), [Profile::of(id)].into_iter().collect());
    }

    /// A group header's Shut down: every member's client at once.
    pub fn stop_group(&self, gid: &str) {
        // Its members as drawn: the leader keeps its group but is not one of
        // them, and stopping the group must not stop the leader.
        let (found, ids) = {
            let s = self.state();
            let a = &s.accounts;
            let ids: Vec<UserId> = a.group_members(gid).iter().map(|x| x.user_id).collect();
            let found = a.groups().iter().find(|g| g.id == gid).map(|g| {
                let name = if g.name.is_empty() { "group".to_owned() } else { g.name.clone() };
                let members: HashSet<Profile> = ids.iter().map(|id| Profile::of(*id)).collect();
                (name, members)
            });
            (found, ids)
        };
        if self.state().stop_launches(Some(&ids)) > 0 {
            self.log("Stopping the launch under way");
        }
        if let Some((name, members)) = found.filter(|f| !f.1.is_empty()) {
            self.stop_profiles(name, members);
        }
    }

    fn stop_profiles(&self, label: String, which: HashSet<Profile>) {
        let profiles = self.services().profiles.clone();
        let weak = self.weak();
        crate::worker::run(
            move || profiles.stop(&which),
            move |stopped| {
                let Some(w) = weak.upgrade() else { return };
                match stopped {
                    Ok(0) => w.log(&format!("{label}: was not running")),
                    Ok(_) => w.log(&format!("Shut down {label}")),
                    Err(e) => {
                        w.log(&format!("{label}: could not stop -- {e}"));
                        w.toast(&format!("Could not stop {label}"));
                    }
                }
                w.poll_running();
            },
        );
    }

    pub fn on_stop_all(&self) {
        if self.state().stop_launches(None) > 0 {
            self.log("Stopping the launches under way");
        }
        let profiles: HashSet<Profile> = {
            let s = self.state();
            for (stop, _) in s.macro_runs.values() {
                stop.set();
            }
            s.accounts.accounts().iter().map(|a| Profile::of(a.user_id)).collect()
        };
        let (cordial, weak) = (self.services().profiles.clone(), self.weak());
        crate::worker::run(
            move || cordial.stop(&profiles),
            move |stopped| {
                let Some(w) = weak.upgrade() else { return };
                match stopped {
                    Ok(n) => w.log(&format!("Stopped {n} client(s)")),
                    Err(e) => {
                        w.log(&format!("Could not stop the clients: {e}"));
                        w.toast("Could not stop the clients");
                    }
                }
                w.poll_running();
            },
        );
    }
}

/// The fastest refresh rate among the desktop's monitors, which High and Max
/// clients run at. Read here rather than by the client: a macro-ready one
/// sees only its cage's output, never the monitor's.
fn monitor_hz() -> Option<u32> {
    use gtk::prelude::*;
    let monitors = gtk::gdk::Display::default()?.monitors();
    (0..monitors.n_items())
        .filter_map(|i| monitors.item(i)?.downcast::<gtk::gdk::Monitor>().ok())
        .filter_map(|m| u32::try_from(m.refresh_rate()).ok())
        .map(|millihertz| (millihertz + 500) / 1000)
        .filter(|hz| *hz > 0)
        .max()
}
