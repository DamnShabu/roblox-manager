//! What the window asks Roblox, beyond launching: whether sessions still
//! work, and everyone's favourites and pictures.

use rbxmgr_core::roblox::{FAVORITES_SHOWN, Game, Roblox, RobloxError};
use rbxmgr_core::types::{Label, PlaceId, UserId};

use super::Window;

impl Window {
    // -- favourites and pictures ------------------------------------------------
    /// Everyone's favourites from Roblox, their icons, and the accounts'
    /// headshots.
    pub fn reload_games(&self) {
        let accounts: Vec<(UserId, Label)> =
            self.state().accounts.accounts().iter().map(|a| (a.user_id, a.name.clone())).collect();
        if accounts.is_empty() {
            return self.toast("Add an account first: favourites come from your accounts");
        }
        let total = accounts.len();
        let (keyring, roblox, icons, avatars, log) = (
            self.services().keyring.clone(),
            self.services().roblox.clone(),
            self.services().icons.clone(),
            self.services().avatars.clone(),
            self.logger(),
        );
        self.run_task(
            move || {
                let users: Vec<UserId> = accounts.iter().map(|(id, _)| *id).collect();
                fetch_headshots(&roblox, &avatars, &users, &|l| log.line(l));
                let mut fresh: Vec<(UserId, Vec<Game>)> = Vec::new();
                let cookies = keyring.cookies(accounts.iter().map(|(_, label)| label));
                for ((id, label), cookie) in accounts.iter().zip(cookies) {
                    let got = cookie.map_err(|e| e.to_string()).and_then(|c| {
                        roblox.favorites(&c, *id, FAVORITES_SHOWN).map_err(|e| e.to_string())
                    });
                    match got {
                        Ok(games) => fresh.push((*id, games)),
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
                let cookies = keyring.cookies(accounts.iter().map(|(_, label)| label));
                accounts
                    .iter()
                    .zip(cookies)
                    .map(|(&(id, ref label), cookie)| {
                        let verdict = match cookie.map(|c| roblox.whoami(&c)) {
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
        if ids.is_empty() {
            return self.toast("Add an account first");
        }
        self.check_sessions(ids);
        self.reload_games();
    }
}

impl Window {
    /// The accounts' Roblox headshots that are not on disk yet: public, so
    /// no keyring is asked, and fetched once, when the window opens or an
    /// account is added. Refresh fetches them all again.
    pub fn load_missing_headshots(&self) {
        let missing: Vec<UserId> = {
            let s = self.state();
            let avatars = &self.services().avatars;
            s.accounts
                .accounts()
                .iter()
                .map(|a| a.user_id)
                .filter(|id| avatars.cached(&id.to_string()).is_none())
                .collect()
        };
        if missing.is_empty() {
            return;
        }
        let (roblox, avatars, log) =
            (self.services().roblox.clone(), self.services().avatars.clone(), self.logger());
        self.run_task(
            move || fetch_headshots(&roblox, &avatars, &missing, &|l| log.line(l)),
            |w, ()| w.refresh_accounts(),
        );
    }
}

/// Fetch `users`' headshots into the avatar cache; what fails is said once.
fn fetch_headshots(
    roblox: &rbxmgr_core::roblox::HttpRoblox,
    avatars: &rbxmgr_core::roblox::IconCache,
    users: &[UserId],
    log: &dyn Fn(String),
) {
    match roblox.headshot_urls(users) {
        Ok(urls) => {
            let failed = urls
                .iter()
                .filter_map(|(id, url)| {
                    avatars.refresh(roblox.transport(), &id.to_string(), url).err()
                })
                .last();
            if let Some(e) = failed {
                log(format!("Some account pictures did not load: {e}"));
            }
        }
        Err(e) => log(format!("Could not load account pictures: {e}")),
    }
}

impl Window {
    /// The icons of the games the window shows that are not on disk yet: the
    /// favourites', and each group's game, whose universe is looked up from
    /// its place once and kept on the group. Public, so no keyring is asked.
    pub fn load_missing_game_icons(&self) {
        let (universes, places): (Vec<String>, Vec<(String, PlaceId)>) = {
            let s = self.state();
            let icons = &self.services().icons;
            let mut universes: Vec<String> = s
                .game_list
                .iter()
                .filter(|t| t.icon.is_none())
                .map(|t| t.game.universe_id.clone())
                .collect();
            let mut places = Vec::new();
            for g in s.accounts.groups() {
                let Some(place) = g.place_id.clone() else { continue };
                if s.game_list.iter().any(|t| t.game.place_id == place) {
                    continue;
                }
                match &g.universe_id {
                    Some(u) if icons.cached(u).is_some() => {}
                    Some(u) => universes.push(u.clone()),
                    None => places.push((g.id.clone(), place)),
                }
            }
            universes.sort();
            universes.dedup();
            (universes, places)
        };
        if universes.is_empty() && places.is_empty() {
            return;
        }
        let (roblox, icons, log) =
            (self.services().roblox.clone(), self.services().icons.clone(), self.logger());
        self.run_task(
            move || {
                let mut found = Vec::new();
                let mut universes = universes;
                for (gid, place) in places {
                    match roblox.place_details(&place) {
                        Ok(d) => {
                            universes.push(d.universe_id.clone());
                            found.push((gid, place, d.universe_id));
                        }
                        Err(e) => log.line(format!("Could not look up place {place}: {e}")),
                    }
                }
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
                found
            },
            |w, found: Vec<(String, PlaceId, String)>| {
                for (gid, place, universe) in &found {
                    w.state_mut().accounts.set_group_universe(gid, place, universe);
                }
                if !found.is_empty() {
                    w.save_accounts();
                }
                w.show_games();
                w.refresh_accounts();
            },
        );
    }

    /// A game's icon on disk: its favourite tile's, else its universe's.
    pub fn game_icon(&self, place: &PlaceId, universe: Option<&str>) -> Option<std::path::PathBuf> {
        let s = self.state();
        s.game_list
            .iter()
            .find(|t| &t.game.place_id == place)
            .and_then(|t| t.icon.clone())
            .or_else(|| universe.and_then(|u| self.services().icons.cached(u)))
    }
}
