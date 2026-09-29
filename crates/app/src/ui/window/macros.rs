//! The macros column, and running macros on accounts.

use adw::prelude::*;
use rbxmgr_core::macros::{self, Player, StopFlag, VirtualInput, nested, random_pick};
use rbxmgr_core::types::{Profile, UserId};

use super::Window;
use crate::ui::macros::card::macro_card;
use crate::ui::widgets::{LabelFluent, clear, hotkey_label, lbl};
use crate::worker;

impl Window {
    pub fn refresh_macros(&self) {
        self.0.cards.borrow_mut().clear();
        let cards = &self.0.ui.cards;
        clear(cards);
        let names: Vec<String> = self.state().macros.names().map(str::to_owned).collect();
        for name in &names {
            cards.append(&macro_card(self, name));
        }
        if names.is_empty() {
            cards.append(
                &lbl("No macros yet. A macro presses keys and clicks for an account on its own -- press New.", "mempty")
                    .wrapped(),
            );
        }
        self.bind_hotkeys();
        self.refresh_launch_state();
    }

    /// In-app hotkeys: each toggles its macro on the selected accounts while
    /// this window has focus. From anywhere, bind a compositor key to
    /// `gapplication action io.github.mujo.RobloxManager run-macro "'NAME'"`.
    fn bind_hotkeys(&self) {
        let shortcuts = &self.0.ui.shortcuts;
        while let Some(s) = shortcuts.item(0).and_downcast::<gtk::Shortcut>() {
            shortcuts.remove_shortcut(&s);
        }
        let keys: Vec<(String, String)> =
            self.state().macros.hotkeys().map(|(n, k)| (n.to_owned(), k.to_owned())).collect();
        for (name, accel) in keys {
            let Some(trigger) = gtk::ShortcutTrigger::parse_string(&accel) else { continue };
            let weak = self.weak();
            let action = gtk::CallbackAction::new(move |_, _| {
                if let Some(w) = weak.upgrade() {
                    w.run_macro_card(&name);
                }
                gtk::glib::Propagation::Stop
            });
            shortcuts.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
        }
    }

    pub fn toggle_macro(&self, name: &str) {
        {
            let mut s = self.state_mut();
            if !s.open_macros.remove(name) {
                s.open_macros.insert(name.to_owned());
            }
        }
        self.refresh_macros();
    }

    /// A switched-off macro cannot run; switching one off stops it.
    pub fn enable_macro(&self, name: &str, on: bool) {
        let saved = self.state_mut().macros.set_enabled(name, on);
        if let Err(e) = saved {
            self.toast(&e.to_string());
        }
        if !on {
            self.stop_macro(name);
        }
        self.refresh_states();
    }

    fn stop_macro(&self, name: &str) {
        for (stop, m) in self.state().macro_runs.values() {
            if m == name {
                stop.set();
            }
        }
    }

    /// Run: the macro on every selected account. Stop: wherever it plays.
    pub fn run_macro_card(&self, name: &str) {
        if self.state().macros_running().contains(name) {
            return self.stop_macro(name);
        }
        if !self.state().macros.enabled(name) {
            return self.toast(&format!("{name} is switched off"));
        }
        let chosen: Vec<UserId> =
            self.state().accounts.selected().iter().map(|a| a.user_id).collect();
        if chosen.is_empty() {
            return self.log("No accounts selected");
        }
        for id in chosen {
            // A macro cannot reach a normal window once you look away.
            self.state_mut().accounts.set_nested(id, true);
            self.start_macro(id, name);
        }
        self.save_accounts();
    }

    pub fn pick_macro(&self, id: UserId, name: Option<String>) {
        self.state_mut().accounts.set_macro(id, name.as_deref());
        self.save_accounts();
        self.refresh_accounts();
    }

    pub fn play_macro_here(&self, id: UserId) {
        let playing = self.state().macro_runs.get(&id).map(|(stop, _)| stop.clone());
        if let Some(stop) = playing {
            return stop.set();
        }
        self.state_mut().accounts.set_nested(id, true);
        self.save_accounts();
        let name = self.state().accounts.get(id).and_then(|a| a.macro_name.clone());
        match name {
            Some(name) => self.start_macro(id, &name),
            None => self.toast("Pick a macro first"),
        }
    }

    /// The editor's Save. Returns what is wrong, or None once saved.
    pub fn save_macro(
        &self,
        old: Option<&str>,
        new: &str,
        text: &str,
        hotkey: Option<&str>,
    ) -> Option<String> {
        let saved = self.state_mut().macros.save(old, new, text, hotkey);
        if let Err(e) = saved {
            return Some(match e {
                macros::MacroError::HotkeyTaken { hotkey, by } => {
                    format!("{} already runs {by}", hotkey_label(Some(&hotkey)))
                }
                other => other.to_string(),
            });
        }
        let new = new.trim();
        if let Some(old) = old.filter(|o| *o != new) {
            let mut s = self.state_mut();
            s.accounts.rename_macro(old, new);
            for (_, m) in s.macro_runs.values_mut() {
                if m == old {
                    new.clone_into(m);
                }
            }
            s.open_macros.remove(old);
        }
        if old.is_none() {
            self.state_mut().open_macros = [new.to_owned()].into();
        }
        self.save_accounts();
        self.refresh_macros();
        self.refresh_accounts();
        self.log(&format!("Saved {new}"));
        None
    }

    pub fn delete_macro(&self, name: &str) {
        self.stop_macro(name);
        let deleted = {
            let mut s = self.state_mut();
            s.accounts.drop_macro(name);
            s.macros.delete(name)
        };
        if let Err(e) = deleted {
            self.toast(&e.to_string());
        }
        self.save_accounts();
        self.refresh_macros();
        self.refresh_accounts();
        self.log(&format!("Deleted {name}"));
    }

    /// Play `name` into the account's macro-ready client, on a thread.
    pub fn start_macro(&self, id: UserId, name: &str) {
        let (label, text) = {
            let s = self.state();
            let Some(label) = s.accounts.get(id).map(|a| a.name.clone()) else { return };
            if !s.macros.enabled(name) {
                drop(s);
                return self.toast(&format!("{name} is switched off"));
            }
            match s.macros.text(name) {
                Some(t) => (label, t.to_owned()),
                None => {
                    drop(s);
                    return self.toast(&format!("{label}: pick a macro first"));
                }
            }
        };
        let m = match macros::grammar::parse(&text) {
            Ok(m) => m,
            Err(e) => return self.toast(&format!("{name}: {e}")),
        };
        // One macro per client: two typing into one display would interleave.
        let stop = StopFlag::default();
        let previous = self.state_mut().macro_runs.insert(id, (stop.clone(), name.to_owned()));
        if let Some((old, _)) = previous {
            old.set();
        }
        self.refresh_states();
        self.log(&format!("{label}: playing {name}"));
        let profile = Profile::of(id);
        let display = nested::display_file(self.services().paths.runtime_dir(), &profile);
        let (profiles, log, name) =
            (self.services().profiles.clone(), self.logger(), name.to_owned());
        let mine = stop.clone();
        let weak = self.weak();
        worker::run(
            move || {
                let running = || profiles.running().is_ok_and(|up| up.contains(&profile));
                let connect = |p: &std::path::Path| -> std::io::Result<Box<dyn macros::Input>> {
                    Ok(Box::new(VirtualInput::connect(p)?))
                };
                let report = |text: String| log.line(format!("{label}: {name} -- {text}"));
                let now = chrono::Local::now;
                let player = Player {
                    display: &display,
                    running: &running,
                    connect: &connect,
                    report: &report,
                    pick: &random_pick,
                    now: &now,
                };
                match player.play(&m, &stop) {
                    Ok(()) => log.line(format!(
                        "{label}: {name} {}",
                        if stop.is_set() { "stopped" } else { "finished" }
                    )),
                    Err(e) => log.line(format!("{label}: {name} stopped -- {e}")),
                }
            },
            move |()| {
                let Some(w) = weak.upgrade() else { return };
                let mut s = w.state_mut();
                // Only this run's entry: a newer run may have replaced it.
                if s.macro_runs.get(&id).is_some_and(|(flag, _)| flag.same_as(&mine)) {
                    s.macro_runs.remove(&id);
                }
                drop(s);
                w.refresh_states();
                w.refresh_accounts();
            },
        );
    }
}
