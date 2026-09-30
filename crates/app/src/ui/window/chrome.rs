//! The parts of the window that are built once: the header bar and its
//! menu, the accounts page (or the welcome when there are none), the macros
//! pane beside it, and the launch bar.

use adw::prelude::*;
use gtk::{Align, Label, PolicyType, gio};

use super::WeakWindow;
use crate::ui::games::GameBar;
use crate::ui::widgets::{Btn, Fluent, IconButton, LabelFluent, lbl, name, page_header};

pub struct Chrome {
    pub title: adw::WindowTitle,
    pub spinner: adw::Spinner,
    pub banner: adw::Banner,
    /// "welcome" with no accounts, else "accounts".
    pub pages: gtk::Stack,
    pub accounts_box: gtk::Box,
    pub accounts_meta: Label,
    pub select_all: IconButton,
    pub games: GameBar,
    pub macros_box: gtk::Box,
    pub log_box: gtk::Box,
    pub summary: Label,
    pub target_text: Label,
    pub btn_each: IconButton,
    pub launch_bar: gtk::Box,
    pub toasts: adw::ToastOverlay,
    pub shortcuts: gtk::ShortcutController,
    pub split: adw::OverlaySplitView,
    pub search_bar: gtk::SearchBar,
    pub search: gtk::SearchEntry,
}

impl Chrome {
    pub fn build(win: &adw::ApplicationWindow, w: &WeakWindow, sidebar: bool) -> Self {
        // -- header bar -----------------------------------------------------
        let title = adw::WindowTitle::new("Roblox Manager", "All idle");
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&title));
        header.pack_start(&action_button(
            "list-add-symbolic",
            "Add Account (Ctrl+N)",
            "win.add-account",
        ));
        header.pack_start(&action_button(
            "view-refresh-symbolic",
            "Check Sessions and Reload Favourites (Ctrl+R)",
            "win.refresh",
        ));
        let spinner = adw::Spinner::new();
        spinner.set_visible(false);
        spinner.set_tooltip_text(Some("Working…"));
        header.pack_start(&spinner);
        let menu = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&main_menu())
            .primary(true)
            .tooltip_text("Main Menu")
            .build();
        name(&menu, "Main Menu");
        header.pack_end(&menu);
        let sidebar_toggle = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-right-symbolic")
            .tooltip_text("Macros and Activity (F9)")
            .active(sidebar)
            .build();
        name(&sidebar_toggle, "Macros and Activity");
        header.pack_end(&sidebar_toggle);
        let search_toggle = gtk::ToggleButton::builder()
            .icon_name("system-search-symbolic")
            .tooltip_text("Search Accounts (Ctrl+F)")
            .build();
        name(&search_toggle, "Search Accounts");
        header.pack_end(&search_toggle);
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search accounts by name, Roblox user or note")
            .hexpand(true)
            .build();
        let search_bar = gtk::SearchBar::builder()
            .child(&adw::Clamp::builder().maximum_size(520).child(&search).build())
            .show_close_button(false)
            .build();
        search_bar.connect_entry(&search);
        search_toggle
            .bind_property("active", &search_bar, "search-mode-enabled")
            .bidirectional()
            .build();
        {
            let w = w.clone();
            search.connect_search_changed(move |e| {
                if let Some(w) = w.upgrade() {
                    w.set_filter(&e.text());
                }
            });
        }
        {
            // Closed, the search is forgotten: the entry's words with it, so
            // opening it again never shows words that filter nothing.
            let entry = search.clone();
            search_bar.connect_search_mode_enabled_notify(move |bar| {
                if !bar.is_search_mode() {
                    entry.set_text("");
                }
            });
        }

        let banner =
            adw::Banner::new("Installing the newest Roblox build — this can take a few minutes");

        // -- accounts page ----------------------------------------------------
        let games = GameBar::new(w.clone());
        let reload_games = action_button(
            "view-refresh-symbolic",
            "Reload Everyone's Favourites",
            "win.reload-games",
        )
        .css("flat circular");
        let games_section = vbox!(
            8,
            "",
            page_header(
                "Launch Into",
                Some("A favourite game, Roblox's own games browser, or a friend's server"),
                &[reload_games.centered().upcast()]
            ),
            games.root.clone()
        );

        let accounts_meta = lbl("", "caption dimmed account-count");
        let select_all = Btn::new("flat")
            .text("Select All")
            .icon("edit-select-all-symbolic")
            .build(w.act(|w| w.on_select_all()));
        let new_group = Btn::new("flat")
            .text("New Group")
            .icon("folder-new-symbolic")
            .tip("A named set of accounts with a game of its own")
            .build(w.act(|w| w.add_group()));
        let accounts_head = hbox!(
            8,
            "section-header",
            vbox!(2, "", lbl("Accounts", "title-4"), accounts_meta.clone()).hexpand().centered(),
            select_all.button.clone().centered(),
            new_group.button.centered()
        );
        let accounts_box = vbox!(22, "");
        let page = vbox!(32, "", games_section, vbox!(18, "", accounts_head, accounts_box.clone()))
            .margins(24);
        page.set_margin_top(18);
        let clamp =
            adw::Clamp::builder().maximum_size(1080).tightening_threshold(760).child(&page).build();
        let accounts_page = gtk::ScrolledWindow::builder()
            .child(&clamp)
            .hscrollbar_policy(PolicyType::Never)
            .vexpand(true)
            .build();

        let welcome = adw::StatusPage::builder()
            .icon_name("io.github.mujo.RobloxManager")
            .title("Add Your First Account")
            .description(
                "Accounts sign in with Roblox Quick Login: you approve the sign-in on a device \
                 where you are already signed in, and no password is typed here. Sessions are \
                 kept in your keyring.",
            )
            .build();
        welcome.set_child(Some(
            &gtk::Button::builder()
                .label("_Add Account")
                .use_underline(true)
                .halign(Align::Center)
                .css_classes(["pill", "suggested-action"])
                .action_name("win.add-account")
                .build(),
        ));
        let pages =
            gtk::Stack::builder().transition_type(gtk::StackTransitionType::Crossfade).build();
        pages.add_named(&welcome, Some("welcome"));
        pages.add_named(&accounts_page, Some("accounts"));

        // -- macros and activity pane -------------------------------------------
        let macros_box = vbox!(10, "");
        let log_box = vbox!(2, "");
        let macros_head = page_header(
            "Macros",
            Some("Keys and clicks played into macro-ready clients"),
            &[
                action_button("help-about-symbolic", "How Macros Work (F1)", "win.macro-help")
                    .css("flat circular")
                    .centered()
                    .upcast(),
                action_button("list-add-symbolic", "New Macro", "win.new-macro")
                    .css("flat circular")
                    .centered()
                    .upcast(),
            ],
        );
        let log_head = page_header(
            "Activity",
            None,
            &[gtk::Button::builder()
                .label("Show All")
                .css_classes(["flat"])
                .action_name("win.activity-log")
                .tooltip_text("The Whole Log (Ctrl+L)")
                .valign(Align::Center)
                .build()
                .upcast()],
        );
        let pane = vbox!(
            26,
            "pane",
            vbox!(12, "", macros_head, macros_box.clone()),
            vbox!(8, "", log_head, log_box.clone())
        );
        let pane_scroller = gtk::ScrolledWindow::builder()
            .child(&pane)
            .hscrollbar_policy(PolicyType::Never)
            .vexpand(true)
            .build();

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&pages));
        let split = adw::OverlaySplitView::builder()
            .content(&toasts)
            .sidebar(&pane_scroller)
            .sidebar_position(gtk::PackType::End)
            .min_sidebar_width(300.0)
            .max_sidebar_width(380.0)
            .sidebar_width_fraction(0.3)
            .show_sidebar(sidebar)
            .build();
        sidebar_toggle.bind_property("active", &split, "show-sidebar").bidirectional().build();

        // -- launch bar -----------------------------------------------------------
        let summary = lbl("", "summary").ellipsize();
        let target_text = lbl("", "caption dimmed").ellipsize();
        let stop_all = Btn::new("")
            .text("Stop All")
            .tip("Close every account's client and stop every macro")
            .build(|| {});
        stop_all.button.set_action_name(Some("win.stop-all"));
        let btn_each = Btn::new("")
            .text("Launch Selected")
            .tip("Each selected account into the target (Ctrl+Shift+Enter)")
            .build(|| {});
        btn_each.button.set_action_name(Some("win.launch-selected"));
        let btn_group = Btn::new("suggested-action")
            .text("Launch as Group")
            .icon("media-playback-start-symbolic")
            .tip("The leader first, then the auto-join list into its server (Ctrl+Enter)")
            .build(|| {});
        btn_group.button.set_action_name(Some("win.launch-group"));
        let summary_box = vbox!(2, "", summary.clone(), target_text.clone()).hexpand().centered();
        let launch_bar = hbox!(
            8,
            "launch-bar",
            summary_box.clone(),
            stop_all.button.clone().centered(),
            btn_each.button.clone().centered(),
            btn_group.button.clone().centered()
        );

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(&search_bar);
        toolbar.add_top_bar(&banner);
        toolbar.set_content(Some(&split));
        toolbar.add_bottom_bar(&launch_bar);
        toolbar.set_bottom_bar_style(adw::ToolbarStyle::RaisedBorder);
        win.set_content(Some(&toolbar));

        // Narrow: the macros pane slides over the accounts instead of beside
        // them, and the launch bar keeps only its buttons.
        if let Ok(cond) = adw::BreakpointCondition::parse("max-width: 880sp") {
            let narrow = adw::Breakpoint::new(cond);
            narrow.add_setter(&split, "collapsed", Some(&true.to_value()));
            win.add_breakpoint(narrow);
        }
        if let Ok(cond) = adw::BreakpointCondition::parse("max-width: 560sp") {
            let tiny = adw::Breakpoint::new(cond);
            tiny.add_setter(&split, "collapsed", Some(&true.to_value()));
            tiny.add_setter(&summary_box, "visible", Some(&false.to_value()));
            tiny.add_setter(&stop_all.button, "visible", Some(&false.to_value()));
            win.add_breakpoint(tiny);
        }

        let shortcuts = gtk::ShortcutController::new();
        win.add_controller(shortcuts.clone());

        Chrome {
            title,
            spinner,
            banner,
            pages,
            accounts_box,
            accounts_meta,
            select_all,
            games,
            macros_box,
            log_box,
            summary,
            target_text,
            btn_each,
            launch_bar,
            toasts,
            shortcuts,
            split,
            search_bar,
            search,
        }
    }
}

/// An icon button that fires a window action.
fn action_button(icon: &str, tip: &str, action: &str) -> gtk::Button {
    let b = gtk::Button::builder().icon_name(icon).tooltip_text(tip).action_name(action).build();
    name(&b, tip);
    b
}

fn main_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let add = gio::Menu::new();
    add.append(Some("_Add Account…"), Some("win.add-account"));
    add.append(Some("New _Group"), Some("win.new-group"));
    add.append(Some("New _Macro…"), Some("win.new-macro"));
    menu.append_section(None, &add);
    let roblox = gio::Menu::new();
    roblox.append(Some("_Refresh Sessions and Favourites"), Some("win.refresh"));
    roblox.append(Some("_Update Roblox"), Some("win.update-roblox"));
    roblox.append(Some("Update _Stacked"), Some("win.update-stacked"));
    roblox.append(Some("Open Roblox _Links Here"), Some("win.open-links-here"));
    menu.append_section(None, &roblox);
    let style = gio::Menu::new();
    for (label, name) in [("Follow System", "system"), ("Light", "light"), ("Dark", "dark")] {
        let item = gio::MenuItem::new(Some(label), None);
        item.set_action_and_target_value(Some("win.style"), Some(&name.to_variant()));
        style.append_item(&item);
    }
    menu.append_section(Some("Style"), &style);
    let help = gio::Menu::new();
    help.append(Some("Activity _Log"), Some("win.activity-log"));
    help.append(Some("How _Macros Work"), Some("win.macro-help"));
    help.append(Some("_Keyboard Shortcuts"), Some("win.shortcuts"));
    help.append(Some("_About Roblox Manager"), Some("app.about"));
    menu.append_section(None, &help);
    menu
}
