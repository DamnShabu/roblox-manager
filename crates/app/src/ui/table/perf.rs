//! An account's performance level where its row is: a quiet button with the
//! level and its frame cap, which opens the four levels. A client still
//! running at the level it launched with, after the account's has changed,
//! shows a relaunch mark: a change applies from the next launch.

use adw::prelude::*;
use gtk::{Align, Orientation, gio};
use rbxmgr_core::cordial::Performance;
use rbxmgr_core::types::UserId;

use crate::state::AppState;
use crate::ui::ds;
use crate::ui::widgets::{Fluent, lbl, name};

/// The frame cap a level runs at, as words: "10 FPS", or the monitor's.
pub fn fps(level: Performance, hz: Option<u32>) -> String {
    match level.fps_cap(hz) {
        Some(n) => format!("{n} FPS"),
        None => "Full rate".to_owned(),
    }
}

/// The four levels for one account, each a `win.set-performance` item.
pub fn menu(id: UserId, hz: Option<u32>) -> gio::Menu {
    let menu = gio::Menu::new();
    for level in Performance::ALL {
        let label = format!("{} · {}", level.label(), fps(level, hz));
        let item = gio::MenuItem::new(Some(&label), None);
        item.set_action_and_target_value(
            Some("win.set-performance"),
            Some(&(id.0, level.label().to_owned()).to_variant()),
        );
        menu.append_item(&item);
    }
    menu
}

pub struct PerfCell {
    pub root: gtk::MenuButton,
    mark: gtk::Image,
    level: gtk::Label,
    fps: gtk::Label,
}

impl PerfCell {
    pub fn build(id: UserId, label: &str, hz: Option<u32>) -> Self {
        let mark = ds::icon("relaunch").css("mn-pend s16").visible(false);
        let level = lbl("", "mn-perf-l");
        let fps = lbl("", "mn-dim");
        let inner = gtk::Box::new(Orientation::Horizontal, 6);
        inner.append(&mark);
        inner.append(&level);
        inner.append(&fps);
        inner.append(&ds::icon("chev-down").css("mn-dim s16"));
        let root = gtk::MenuButton::builder()
            .child(&inner)
            .menu_model(&menu(id, hz))
            .halign(Align::Start)
            .valign(Align::Center)
            .css_classes(["mn-perf"])
            .build();
        name(&root, &format!("Performance of {label}"));
        PerfCell { root, mark, level, fps }
    }

    /// The account's level, and the relaunch mark when its client runs at
    /// another.
    pub fn draw(&self, s: &AppState, id: UserId, hz: Option<u32>) {
        let Some(level) = s.accounts.get(id).map(|a| a.performance()) else { return };
        self.level.set_label(level.label());
        self.fps.set_label(&fps(level, hz));
        match s.stale_performance(id) {
            Some(running) => {
                self.mark.set_visible(true);
                self.root.set_tooltip_text(Some(&format!(
                    "Running at {}; launch it again to switch to {}",
                    running.label(),
                    level.label()
                )));
            }
            None => {
                self.mark.set_visible(false);
                self.root.set_tooltip_text(Some("How much of the machine its client may use"));
            }
        }
    }
}
