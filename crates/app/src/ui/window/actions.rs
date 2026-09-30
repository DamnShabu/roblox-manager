//! The window's actions: what the header bar, the menu, the launch bar and
//! the keyboard shortcuts fire.

use adw::prelude::*;
use gtk::glib::variant::StaticVariantType;
use gtk::{gio, glib};
use rbxmgr_core::types::UserId;

use super::Window;
use crate::ui::accounts::group_settings::GroupSettings;
use crate::ui::accounts::settings::AccountSettings;
use crate::ui::login::AddAccountDialog;
use crate::ui::{activity, macros};

/// (action, what it does, its keys) -- the shortcuts list is drawn from this.
pub const SHORTCUTS: &[(&str, &str, &[&str])] = &[
    ("win.add-account", "Add account", &["<Control>n"]),
    ("win.refresh", "Check sessions and reload favourites", &["<Control>r", "F5"]),
    ("win.launch-group", "Launch as group", &["<Control>Return"]),
    ("win.launch-selected", "Launch selected", &["<Control><Shift>Return"]),
    ("win.stop-all", "Stop all", &["<Control><Shift>period"]),
    ("win.new-macro", "New macro", &["<Control><Shift>n"]),
    ("win.toggle-sidebar", "Show or hide macros and activity", &["F9"]),
    ("win.activity-log", "Activity log", &["<Control>l"]),
    ("win.macro-help", "How macros work", &["F1"]),
    ("win.shortcuts", "Keyboard shortcuts", &["<Control>question"]),
    ("window.close", "Close the window", &["<Control>w"]),
    ("app.quit", "Quit", &["<Control>q"]),
];

/// A window action by name, and what it runs.
type Action = (&'static str, fn(&Window));
/// One taking an account's user id.
type AccountAction = (&'static str, fn(&Window, UserId));
/// One taking a group's id.
type GroupAction = (&'static str, fn(&Window, &str));

impl Window {
    pub(super) fn install_actions(&self) {
        let actions: [Action; 14] = [
            ("add-account", Window::on_add),
            ("refresh", Window::refresh_all),
            ("update-roblox", Window::on_update_roblox),
            ("new-group", Window::add_group),
            ("new-macro", |w| macros::editor::MacroDialog::open(w, None)),
            ("launch-selected", Window::launch_selected),
            ("launch-group", Window::launch_chain),
            ("stop-all", Window::on_stop_all),
            ("select-all", Window::on_select_all),
            ("reload-games", Window::reload_games),
            ("activity-log", activity::open_log),
            ("macro-help", macros::help::show),
            ("shortcuts", show_shortcuts),
            ("toggle-sidebar", |w| {
                let split = &w.0.ui.split;
                split.set_show_sidebar(!split.shows_sidebar());
            }),
        ];
        for (name, run) in actions {
            let action = gio::SimpleAction::new(name, None);
            let weak = self.weak();
            action.connect_activate(move |_, _| {
                if let Some(w) = weak.upgrade() {
                    run(&w);
                }
            });
            self.0.win.add_action(&action);
        }
        self.install_item_actions();
    }

    /// Actions on one account or group, which rows' and headers' menus fire
    /// with its id. Each waits for the menu to close before it runs: most
    /// redraw the list, the menu's row with it.
    fn install_item_actions(&self) {
        let accounts: [AccountAction; 6] = [
            ("account-settings", |w, id| AccountSettings::open(w, id)),
            ("make-leader", Window::set_leader),
            ("toggle-follow", |w, id| {
                let on = w.state().accounts.get(id).is_some_and(|a| a.follow.is_none());
                w.set_follow(id, on);
            }),
            ("check-session", |w, id| w.check_sessions(vec![id])),
            ("sign-in-again", |w, id| {
                let acct = w.state().accounts.get(id).cloned();
                if let Some(acct) = acct {
                    AddAccountDialog::open(w, Some(acct));
                }
            }),
            ("remove-account", Window::confirm_remove),
        ];
        for (name, run) in accounts {
            self.add_deferred(name, glib::VariantTy::UINT64, move |w, v| {
                if let Some(id) = v.get::<u64>() {
                    run(w, UserId(id));
                }
            });
        }
        // (user id, group id); "" is Ungrouped.
        let pair = <(u64, String)>::static_variant_type();
        self.add_deferred("move-account", &pair, |w, v| {
            if let Some((id, gid)) = v.get::<(u64, String)>() {
                w.set_group(UserId(id), Some(gid).filter(|g| !g.is_empty()));
            }
        });
        let groups: [GroupAction; 2] = [
            ("group-settings", |w, gid| GroupSettings::open(w, gid)),
            ("delete-group", Window::confirm_delete_group),
        ];
        for (name, run) in groups {
            self.add_deferred(name, glib::VariantTy::STRING, move |w, v| {
                if let Some(gid) = v.get::<String>() {
                    run(w, &gid);
                }
            });
        }
    }

    fn add_deferred(
        &self,
        name: &str,
        param: &glib::VariantTy,
        run: impl Fn(&Window, &glib::Variant) + 'static,
    ) {
        let action = gio::SimpleAction::new(name, Some(param));
        let weak = self.weak();
        let run = std::rc::Rc::new(run);
        action.connect_activate(move |_, value| {
            let (Some(value), weak, run) = (value.cloned(), weak.clone(), run.clone()) else {
                return;
            };
            glib::idle_add_local_once(move || {
                if let Some(w) = weak.upgrade() {
                    run(&w, &value);
                }
            });
        });
        self.0.win.add_action(&action);
    }

    /// Grey out an action, and every button and menu item that fires it.
    pub fn set_action_enabled(&self, name: &str, on: bool) {
        if let Some(a) = self.0.win.lookup_action(name).and_downcast::<gio::SimpleAction>() {
            a.set_enabled(on);
        }
    }
}

/// Set every shortcut in [`SHORTCUTS`] on the application.
pub fn set_accels(app: &adw::Application) {
    for (action, _, keys) in SHORTCUTS {
        app.set_accels_for_action(action, keys);
    }
}

fn show_shortcuts(w: &Window) {
    let dialog = adw::ShortcutsDialog::new();
    let section = adw::ShortcutsSection::new(Some("Roblox Manager"));
    for (action, title, _) in SHORTCUTS {
        section.add(adw::ShortcutsItem::from_action(title, action));
    }
    dialog.add(section);
    let hotkeys: Vec<(String, String)> =
        w.state().macros.hotkeys().map(|(n, k)| (n.to_owned(), k.to_owned())).collect();
    if !hotkeys.is_empty() {
        let section = adw::ShortcutsSection::new(Some("Macros (while this window has focus)"));
        for (name, accel) in hotkeys {
            section.add(adw::ShortcutsItem::new(&format!("Run or stop {name}"), &accel));
        }
        dialog.add(section);
    }
    dialog.present(Some(w.gtk_window()));
}
