//! What the window asks Roblox, beyond launching: whether sessions still
//! work, and everyone's favourites and pictures.

use rbxmgr_core::roblox::{FAVORITES_SHOWN, Game, Roblox, RobloxError};
use rbxmgr_core::types::{Label, UserId};

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
                match roblox.headshot_urls(&users) {
                    Ok(urls) => {
                        let failed = urls
                            .iter()
                            .filter_map(|(id, url)| {
                                avatars.refresh(roblox.transport(), &id.to_string(), url).err()
                            })
                            .last();
                        if let Some(e) = failed {
                            log.line(format!("Some account pictures did not load: {e}"));
                        }
                    }
                    Err(e) => log.line(format!("Could not load account pictures: {e}")),
                }
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
        if ids.is_empty() {
            return self.toast("Add an account first");
        }
        self.check_sessions(ids);
        self.reload_games();
    }
}
