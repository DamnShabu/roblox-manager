//! The macros pane, and running macros on accounts.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::glib;
use rbxmgr_core::macros::{self, Player, Seed, StopFlag, VirtualInput, nested};
use rbxmgr_core::types::{Profile, UserId};

use super::Window;
use crate::state::MacroRun;
use crate::ui::accounts::leader::placeholder;

/// How far ahead a macro run on several accounts at once is due to start:
/// time for the slowest of their clients to be reached first.
const TOGETHER: Duration = Duration::from_secs(1);

/// A client up in a macro-ready window: whose it is, and where its display
/// is linked.
pub struct ReadyClient {
    pub id: UserId,
    pub label: String,
    pub display: PathBuf,
}
use crate::ui::macros::card::macro_card;
use crate::ui::widgets::{boxed_list, clear, hotkey_label};
use crate::worker;

impl Window {
    pub fn refresh_macros(&self) {
        self.0.cards.borrow_mut().clear();
        let cards = &self.0.ui.macros_box;
        clear(cards);
        let names: Vec<String> = self.state().macros.names().map(str::to_owned).collect();
        for name in &names {
            cards.append(&macro_card(self, name));
        }
        if names.is_empty() {
            let list = boxed_list();
            list.append(&placeholder(
                "input-keyboard-symbolic",
                "No macros yet. A macro presses keys and clicks for an account on its own.",
            ));
            let add = adw::ButtonRow::builder()
                .title("New Macro…")
                .start_icon_name("list-add-symbolic")
                .action_name("win.new-macro")
                .build();
            list.append(&add);
            cards.append(&list);
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

    /// Remember a macro folded or unfolded, for the next redraw.
    pub fn set_macro_open(&self, name: &str, open: bool) {
        let mut s = self.state_mut();
        if open {
            s.open_macros.insert(name.to_owned());
        } else {
            s.open_macros.remove(name);
        }
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

    /// Run: the macro on every selected account not already playing it,
    /// each from its first step; those already playing it play on. Stop,
    /// once every selected one plays it: wherever it plays.
    pub fn run_macro_card(&self, name: &str) {
        let chosen = match self.state().macro_run(name) {
            MacroRun::Start(chosen) => chosen,
            MacroRun::Stop => return self.stop_macro(name),
            MacroRun::Nothing => return self.toast("Select the accounts to run it on"),
        };
        if !self.state().macros.enabled(name) {
            return self.toast(&format!("{name} is switched off"));
        }
        // One moment for all of them, so they play in step -- or, staggered,
        // a turn each from it, in the order listed -- and one seed, so they
        // pick the same random moments. One that does not parse is told of
        // by start_macro.
        let together = Instant::now() + TOGETHER;
        let seed = Seed::fresh();
        let m = self.state().macros.text(name).and_then(|t| macros::grammar::parse(t).ok());
        for (nth, id) in chosen.into_iter().enumerate() {
            // A macro cannot reach a normal window once you look away.
            self.state_mut().accounts.set_nested(id, true);
            let start = m.as_ref().map_or(together, |m| m.start_of(together, nth));
            self.start_macro(id, name, start, seed);
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
            Some(name) => self.start_macro(id, &name, Instant::now(), Seed::fresh()),
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

    /// The clients up in a macro-ready window: the ones a macro's point can
    /// be picked in, or steps recorded from.
    pub fn macro_ready_clients(&self) -> Vec<ReadyClient> {
        let s = self.state();
        let runtime = self.services().paths.runtime_dir();
        s.accounts
            .accounts()
            .iter()
            .filter(|a| s.running.contains(&a.user_id))
            .map(|a| ReadyClient {
                id: a.user_id,
                label: a.name.to_string(),
                display: nested::display_file(runtime, &Profile::of(a.user_id)),
            })
            .filter(|c| c.display.exists())
            .collect()
    }

    /// The account a recording hears, while one is armed or under way: no
    /// macro plays into it meanwhile, or the recording would hear that too.
    pub fn set_recording(&self, id: Option<UserId>) {
        self.state_mut().recording = id;
    }

    /// Play `name` into the account's macro-ready client, on a thread.
    /// Its first step is due at `start`, and its random moments are drawn
    /// from `seed`.
    pub fn start_macro(&self, id: UserId, name: &str, start: Instant, seed: Seed) {
        let (label, text) = {
            let s = self.state();
            let Some(label) = s.accounts.get(id).map(|a| a.name.clone()) else { return };
            if !s.macros.enabled(name) {
                drop(s);
                return self.toast(&format!("{name} is switched off"));
            }
            if s.recording == Some(id) {
                drop(s);
                return self.toast(&format!("{label} is being recorded: stop that first"));
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
        let previous = {
            let mut s = self.state_mut();
            s.macro_progress.remove(&id);
            s.macro_runs.insert(id, (stop.clone(), name.to_owned()))
        };
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
        let progress = self.show_progress(id, &stop);
        let weak = self.weak();
        worker::run(
            move || {
                let running = || profiles.running().is_ok_and(|up| up.contains(&profile));
                let connect = |p: &std::path::Path| -> std::io::Result<Box<dyn macros::Input>> {
                    Ok(Box::new(VirtualInput::connect(p)?))
                };
                let report = |text: String| {
                    log.line(format!("{label}: {name} -- {text}"));
                    // The receiver only goes away with the main loop.
                    let _ = progress.send_blocking(text);
                };
                let now = chrono::Local::now;
                let pick = seed.picker();
                let player = Player {
                    display: &display,
                    running: &running,
                    connect: &connect,
                    report: &report,
                    pick: &pick,
                    now: &now,
                    start,
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
                    s.macro_progress.remove(&id);
                }
                drop(s);
                w.refresh_states();
            },
        );
    }

    /// Where the run `stop` stops on account `id` is, from what it reports:
    /// sent here from its thread, shown on its row. A burst is shown as its
    /// last, and nothing from a run another has since replaced is shown.
    fn show_progress(&self, id: UserId, stop: &StopFlag) -> async_channel::Sender<String> {
        let (tx, rx) = async_channel::unbounded::<String>();
        let (weak, run) = (self.weak(), stop.clone());
        glib::spawn_future_local(async move {
            while let Ok(mut latest) = rx.recv().await {
                while let Ok(newer) = rx.try_recv() {
                    latest = newer;
                }
                let Some(w) = weak.upgrade() else { return };
                let mut s = w.state_mut();
                if !s.macro_runs.get(&id).is_some_and(|(flag, _)| flag.same_as(&run)) {
                    return;
                }
                s.macro_progress.insert(id, latest);
                drop(s);
                w.refresh_states();
            }
        });
        tx
    }
}
