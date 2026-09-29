//! The parts of the window that are built once: the title bar, the two
//! columns' frames, the action bar.

use adw::prelude::*;
use gtk::{Align, Label, Orientation, PolicyType};

use super::WeakWindow;
use crate::ui::games::GameBar;
use crate::ui::macros::editor::MacroDialog;
use crate::ui::widgets::{Btn, Fluent, IconButton, LabelFluent, icon, lbl, section};

pub struct Chrome {
    pub pill: gtk::Box,
    pub pill_dot: gtk::Box,
    pub pill_label: Label,
    pub upd: IconButton,
    pub select_all: IconButton,
    pub accounts_box: gtk::Box,
    pub games: GameBar,
    pub cards: gtk::Box,
    pub log_box: gtk::Box,
    pub summary: Label,
    pub target_text: Label,
    pub btn_each: IconButton,
    pub btn_group: IconButton,
    pub toasts: adw::ToastOverlay,
    pub shortcuts: gtk::ShortcutController,
}

impl Chrome {
    pub fn build(win: &adw::ApplicationWindow, w: &WeakWindow) -> Self {
        // -- title bar ------------------------------------------------------
        let pill_dot = hbox!(0, "").centered();
        let pill_label = Label::new(Some("All idle"));
        let pill = hbox!(7, "pill", pill_dot.clone(), pill_label.clone()).centered();
        let upd = Btn::new("tbtn upd")
            .text("Update Roblox")
            .icon("download")
            .tip("Pull the latest Roblox client")
            .build(w.act(|w| w.on_update_roblox()));
        upd.button.set_valign(Align::Center);
        let add = Btn::new("tbtn")
            .text("Add account")
            .icon("person_add")
            .tip("Add an account with Roblox Quick Login")
            .build(w.act(|w| w.on_add()));
        let reload = Btn::new("ibtn reload")
            .icon("refresh")
            .size(20)
            .tip("Check every session and reload favourites")
            .build(w.act(|w| w.refresh_all()));
        let close = Btn::new("ibtn close")
            .icon("close")
            .size(20)
            .tip("Close")
            .build(w.act(|w| w.gtk_window().close()));
        let bar = hbox!(
            12,
            "titlebar",
            gtk::Image::builder().icon_name("roblox-manager-mark").pixel_size(28).build(),
            lbl("Roblox Manager", "apptitle"),
            pill.clone(),
            gtk::Box::new(Orientation::Horizontal, 0).hexpand(),
            upd.button.clone(),
            add.button.centered(),
            gtk::Box::new(Orientation::Horizontal, 0).css("vdiv").centered(),
            reload.button.centered(),
            close.button.centered()
        );

        // -- left: game and accounts ----------------------------------------
        let games = GameBar::new(w.clone());
        let select_all = Btn::new("textbtn")
            .text("Select all")
            .icon("done_all")
            .build(w.act(|w| w.on_select_all()));
        let new_group = Btn::new("tbtn plain")
            .text("New group")
            .icon("create_new_folder")
            .build(w.act(|w| w.add_group()));
        let reload_games = Btn::new("ibtn reload")
            .icon("refresh")
            .size(20)
            .tip("Refresh favourites")
            .build(w.act(|w| w.reload_games()));
        let accounts_box = vbox!(12, "");
        let left = vbox!(
            32,
            "left",
            vbox!(
                18,
                "",
                section(
                    "videogame_asset",
                    "Game",
                    "Leader's favourites, or open the games browser",
                    &[reload_games.button.centered().upcast()]
                ),
                games.root.clone()
            ),
            vbox!(
                18,
                "",
                section(
                    "group",
                    "Accounts",
                    "Make one the leader in its settings · drag accounts onto it to auto-join after it",
                    &[hbox!(6, "", select_all.button.clone(), new_group.button.clone()).centered().upcast()]
                ),
                accounts_box.clone()
            )
        )
        .hexpand();

        // -- right: macros and activity -------------------------------------
        let cards = vbox!(16, "");
        let log_box = vbox!(8, "");
        let new_macro = Btn::new("hbtn")
            .text("New")
            .icon("add")
            .gap(4)
            .build(w.act(|w| MacroDialog::open(w, None)));
        let right = vbox!(
            16,
            "right",
            section(
                "keyboard",
                "Macros",
                "Hotkey-triggered input sequences",
                &[new_macro.button.centered().upcast()]
            ),
            cards.clone(),
            gtk::Box::new(Orientation::Vertical, 0).vexpand(),
            vbox!(
                10,
                "activity",
                hbox!(6, "acthead", icon("history", 16, ""), lbl("Activity", "")),
                log_box.clone()
            )
        )
        .width(380);
        // A vertical scroller measures the column at no fixed height and
        // reports its minimum, so the column holds the design's width.
        let right = gtk::ScrolledWindow::builder()
            .child(&right)
            .propagate_natural_height(true)
            .hscrollbar_policy(PolicyType::Never)
            .build();
        // Set, not left unset: an unset hexpand takes its children's, and a
        // card's expanding title would widen the column past the design's.
        right.set_hexpand(false);
        let body = hbox!(0, "", left, right);

        // -- action bar -----------------------------------------------------
        let summary = lbl("", "summary").ellipsize();
        let target_text = lbl("", "").ellipsize();
        let btn_each = Btn::new("big second")
            .text("Launch selected")
            .icon("play_arrow")
            .size(20)
            .fill()
            .gap(7)
            .tip("Each selected account into the target")
            .build(w.act(|w| w.launch_selected()));
        let btn_group = Btn::new("big primary")
            .text("Launch as group")
            .icon("groups")
            .size(20)
            .fill()
            .gap(8)
            .tip("The leader first; auto-join accounts follow into its server")
            .build(w.act(|w| w.launch_chain()));
        let stop_all = Btn::new("big stopall")
            .text("Stop all")
            .icon("stop_circle")
            .size(20)
            .gap(7)
            .tip("Close every account's client and stop every macro")
            .build(w.act(|w| w.on_stop_all()));
        let actions = hbox!(
            12,
            "actionbar",
            vbox!(
                1,
                "",
                summary.clone(),
                hbox!(4, "target", icon("arrow_forward", 14, ""), target_text.clone())
            )
            .hexpand()
            .centered(),
            hbox!(8, "", stop_all.button, btn_each.button.clone(), btn_group.button.clone())
        );

        let scroller = gtk::ScrolledWindow::builder()
            .child(&body)
            .vexpand(true)
            .hscrollbar_policy(PolicyType::Never)
            .build();
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&scroller));
        let toolbar = adw::ToolbarView::new();
        toolbar.set_content(Some(&toasts));
        toolbar.add_top_bar(&gtk::WindowHandle::builder().child(&bar).build());
        toolbar.add_bottom_bar(&actions);
        win.set_content(Some(&toolbar));

        // Narrow window: the macros column wraps below the accounts.
        if let Ok(cond) = adw::BreakpointCondition::parse("max-width: 960sp") {
            let narrow = adw::Breakpoint::new(cond);
            narrow.add_setter(&body, "orientation", Some(&Orientation::Vertical.to_value()));
            win.add_breakpoint(narrow);
        }

        let shortcuts = gtk::ShortcutController::new();
        win.add_controller(shortcuts.clone());

        Chrome {
            pill,
            pill_dot,
            pill_label,
            upd,
            select_all,
            accounts_box,
            games,
            cards,
            log_box,
            summary,
            target_text,
            btn_each,
            btn_group,
            toasts,
            shortcuts,
        }
    }
}
