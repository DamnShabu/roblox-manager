//! One macro: its switch, hotkey and steps -- and Run on the selected accounts.

use adw::prelude::*;
use rbxmgr_core::macros::grammar::{self, loop_label};

use super::editor::MacroDialog;
use crate::ui::widgets::{
    Btn, Fluent, LabelFluent, dot, hotkey_label, icon, icon_tile, lbl, switch,
};
use crate::ui::window::Window;

/// The icon for an editor step type.
pub fn step_icon(kind: &str) -> &'static str {
    match kind {
        "Key" => "keyboard",
        "Hold" => "keyboard_keys",
        "Type" => "text_fields",
        "Click" => "mouse",
        "Move" => "open_with",
        "Wait" => "timer",
        "Start" => "hourglass_top",
        "Note" => "notes",
        _ => "radio_button_checked",
    }
}

pub fn macro_card(w: &Window, name: &str) -> gtk::Box {
    let (text, enabled, hotkey, opened) = {
        let s = w.state();
        (
            s.macros.text(name).unwrap_or_default().to_owned(),
            s.macros.enabled(name),
            s.macros.hotkey(name).map(str::to_owned),
            s.open_macros.contains(name),
        )
    };
    let (rows, loops) = grammar::rows(&text);
    let steps = rows.iter().filter(|r| r.kind != "Note").count();
    let card = vbox!(0, "mcard");
    card.set_overflow(gtk::Overflow::Hidden);

    let toggled = name.to_owned();
    let weak = w.weak();
    let sw = switch(enabled, Some("Enable macro"), move |on| {
        if let Some(w) = weak.upgrade() {
            w.enable_macro(&toggled, on);
        }
    });
    let running = hbox!(6, "mrun", dot("running", true, 7), lbl("Running", ""));
    let meta = hbox!(
        8,
        "mmeta",
        hbox!(5, "hk", icon("keyboard", 14, ""), lbl(&hotkey_label(hotkey.as_deref()), "mono")),
        lbl(&format!("{steps} step{}", if steps == 1 { "" } else { "s" }), ""),
        gtk::Box::new(gtk::Orientation::Horizontal, 0).css("bullet").centered(),
        hbox!(4, "", icon("repeat", 14, ""), lbl(&loop_label(loops), "")),
        gtk::Box::new(gtk::Orientation::Horizontal, 0).hexpand(),
        icon(if opened { "expand_less" } else { "expand_more" }, 20, "chevic")
    );
    let head = vbox!(
        10,
        "mhead",
        hbox!(10, "", lbl(name, "mname").hexpand().ellipsize(), running.clone(), sw.clone()),
        meta
    );
    head.set_cursor_from_name(Some("pointer"));
    let click = gtk::GestureClick::new();
    let (weak, toggled, switch_widget, header) =
        (w.weak(), name.to_owned(), sw.clone(), head.clone());
    click.connect_released(move |_, _, x, y| {
        // The switch is in the header too; a click on it only switches.
        let on_switch = header.pick(x, y, gtk::PickFlags::DEFAULT).is_some_and(|p| {
            p == switch_widget.clone().upcast::<gtk::Widget>() || p.is_ancestor(&switch_widget)
        });
        if let (false, Some(w)) = (on_switch, weak.upgrade()) {
            w.toggle_macro(&toggled);
        }
    });
    head.add_controller(click);
    card.append(&head);

    let mut run_button = None;
    let mut meta_label = None;
    if opened {
        let body = vbox!(12, "mbody");
        let listing = vbox!(6, "msteps");
        for r in rows.iter().filter(|r| r.kind != "Note") {
            let shown = if matches!(r.kind.as_str(), "Wait" | "Start") && !r.value.is_empty() {
                format!("{} s", r.value)
            } else if r.value.is_empty() {
                "—".to_owned()
            } else {
                r.value.clone()
            };
            listing.append(&hbox!(
                10,
                "mstep",
                icon_tile("mstepic", step_icon(&r.kind), 17),
                lbl(&r.kind, "msteptype").hexpand(),
                lbl(&shown, "mstepval mono").ellipsize()
            ));
        }
        body.append(&listing);
        let broken = grammar::parse(&text).err();
        if let Some(e) = &broken {
            body.append(&lbl(&e.to_string(), "merr").wrapped());
        }
        let ml = lbl("", "");
        body.append(&hbox!(6, "mmeta", icon("group", 15, ""), ml.clone()));
        let edit_name = name.to_owned();
        let edit = Btn::new("medit")
            .text("Edit steps")
            .icon("edit")
            .size(17)
            .build(w.act(move |w| MacroDialog::open(w, Some(&edit_name))));
        let run_name = name.to_owned();
        let run = Btn::new("mrunb")
            .text("Run")
            .icon("play_arrow")
            .fill()
            .build(w.act(move |w| w.run_macro_card(&run_name)));
        run.button.set_sensitive(broken.is_none());
        let buttons = hbox!(8, "", edit.button.hexpand(), run.button.clone().hexpand());
        buttons.set_homogeneous(true);
        body.append(&buttons);
        card.append(&body);
        run_button = Some((run, broken.is_none()));
        meta_label = Some(ml);
    }

    let (name, root) = (name.to_owned(), card.clone());
    w.add_card(Box::new(move |s| {
        let is_running = s.macros_running().contains(name.as_str());
        if is_running {
            card.add_css_class("running");
        } else {
            card.remove_css_class("running");
        }
        running.set_visible(is_running);
        if let Some((run, parses)) = &run_button {
            run.set_icon(if is_running { "stop" } else { "play_arrow" });
            run.set_text(if is_running { "Stop" } else { "Run" });
            if is_running {
                run.button.add_css_class("stop");
            } else {
                run.button.remove_css_class("stop");
            }
            run.button.set_sensitive(is_running || (*parses && s.macros.enabled(&name)));
        }
        if let Some(ml) = &meta_label {
            let n = s.accounts.selected().len();
            ml.set_label(&format!("Runs on {n} selected client{}", if n == 1 { "" } else { "s" }));
        }
    }));
    root
}
