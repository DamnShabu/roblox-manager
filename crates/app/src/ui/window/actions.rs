//! The window's actions: what the header bar, the menu, the launch bar and
//! the keyboard shortcuts fire.

use adw::prelude::*;
use gtk::glib::variant::StaticVariantType;
use gtk::{gio, glib};
use rbxmgr_core::cordial::Performance;
use rbxmgr_core::types::UserId;
use rbxmgr_core::window_state::Style;

use super::Window;
use crate::ui::accounts::group_settings::GroupSettings;
use crate::ui::accounts::settings::AccountSettings;
use crate::ui::login::AddAccountDialog;
use crate::ui::{activity, macros};

/// (action, what it does, its keys) -- the shortcuts list is drawn from this.
pub const SHORTCUTS: &[(&str, &str, &[&str])] = &[
    ("win.add-account", "Add account", &["<Control>n"]),
    ("win.search", "Search accounts", &["<Control>f"]),
    ("win.refresh", "Check sessions and reload favourites", &["<Control>r", "F5"]),
    ("win.launch-selected", "Launch selected", &["<Control><Shift>Return"]),
    ("win.stop-all", "Stop all", &["<Control><Shift>period"]),
    ("win.pick-target", "Pick where launches go", &["<Control>t"]),
    ("win.new-macro", "New macro", &["<Control><Shift>n"]),
    ("win.toggle-sidebar", "Show or hide the sidebar", &["F9"]),
    ("win.toggle-details", "Show or fold the details", &["F7"]),
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
        let actions: [Action; 25] = [
            ("add-account", Window::on_add),
            ("refresh", Window::refresh_all),
            ("update", Window::on_update),
            ("restart", Window::on_restart),
            ("new-group", Window::add_group),
            ("new-macro", |w| macros::editor::MacroDialog::open(w, None)),
            ("open-scripts", Window::open_scripts_folder),
            ("reload-scripts", Window::draw_scripts),
            ("launch-selected", Window::launch_selected),
            ("stop-all", Window::on_stop_all),
            ("stop-selected", Window::stop_selected),
            ("hide-selected", Window::hide_selected),
            ("pick-target", Window::pick_target),
            ("hide-all", Window::on_hide_all),
            ("show-all", Window::on_show_all),
            ("select-all", Window::on_select_all),
            ("search", Window::start_search),
            ("reload-games", Window::reload_games),
            ("join-friend", Window::on_friends),
            ("open-links-here", Window::open_links_here),
            ("activity-log", activity::open_log),
            ("macro-help", macros::help::show),
            ("shortcuts", show_shortcuts),
            ("toggle-sidebar", Window::toggle_sidebar),
            ("toggle-details", |w| {
                let fold = &w.0.ui.details.fold;
                fold.set_active(!fold.is_active());
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

    /// The saved style, kept for the window-state file; the app is dark only.
    pub(super) fn install_style(&self, style: Style) {
        apply_style(style);
        let action = gio::SimpleAction::new_stateful(
            "style",
            Some(glib::VariantTy::STRING),
            &style.name().to_variant(),
        );
        action.connect_change_state(|a, value| {
            let Some(style) = value.and_then(|v| v.get::<String>()).and_then(|n| Style::parse(&n))
            else {
                return;
            };
            a.set_state(&style.name().to_variant());
            apply_style(style);
        });
        self.0.win.add_action(&action);
    }

    /// The style the menu shows as chosen.
    pub(super) fn style(&self) -> Style {
        self.0
            .win
            .lookup_action("style")
            .and_then(|a| a.state())
            .and_then(|v| v.get::<String>())
            .and_then(|n| Style::parse(&n))
            .unwrap_or_default()
    }

    /// Actions on one account or group, which rows' and headers' menus fire
    /// with its id. Each waits for the menu to close before it runs: most
    /// redraw the list, the menu's row with it.
    fn install_item_actions(&self) {
        let accounts: [AccountAction; 5] = [
            ("account-settings", |w, id| AccountSettings::open(w, id)),
            ("make-main", Window::set_main),
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
        // (user id, level), from a row's performance menu.
        self.add_deferred("set-performance", &pair, |w, v| {
            if let Some((id, level)) = v.get::<(u64, String)>()
                && let Some(level) = Performance::ALL.into_iter().find(|p| p.label() == level)
            {
                w.set_performance(UserId(id), level);
            }
        });
        // (user id, macro), from a row's Add macro.
        self.add_deferred("set-macro", &pair, |w, v| {
            if let Some((id, name)) = v.get::<(u64, String)>() {
                w.pick_macro(UserId(id), Some(name));
            }
        });
        // A level, for every selected account.
        self.add_deferred("perf-selected", glib::VariantTy::STRING, |w, v| {
            if let Some(level) = v.get::<String>()
                && let Some(level) = Performance::ALL.into_iter().find(|p| p.label() == level)
            {
                w.set_performance_selected(level);
            }
        });
        let macros: [GroupAction; 4] = [
            ("edit-macro", |w, name| macros::editor::MacroDialog::open(w, Some(name))),
            ("run-script", Window::run_script),
            ("edit-script", Window::edit_script),
            ("toggle-macro", |w, name| {
                let on = w.state().macros.enabled(name);
                w.enable_macro(name, !on);
            }),
        ];
        for (name, run) in macros {
            self.add_deferred(name, glib::VariantTy::STRING, move |w, v| {
                if let Some(m) = v.get::<String>() {
                    run(w, &m);
                }
            });
        }
        let groups: [GroupAction; 4] = [
            ("group-settings", |w, gid| GroupSettings::open(w, gid)),
            ("delete-group", Window::confirm_delete_group),
            ("select-group", Window::select_group),
            ("move-selected", Window::move_selected),
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

/// The app is dark whatever the saved style says; the style is still kept,
/// so the file it is saved in reads the same to earlier versions.
fn apply_style(_style: Style) {
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
}

/// Set every shortcut in [`SHORTCUTS`] on the application.
pub fn set_accels(app: &adw::Application) {
    for (action, _, keys) in SHORTCUTS {
        app.set_accels_for_action(action, keys);
    }
}

fn show_shortcuts(w: &Window) {
    crate::ui::help::show(w, false);
}
