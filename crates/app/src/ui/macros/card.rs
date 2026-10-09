//! One macro in the Macros tab: unfold, its switch, Run and Edit, and
//! unfolded, its steps.

use adw::prelude::*;
use rbxmgr_core::macros::grammar::{self, loop_label};
use rbxmgr_core::macros::lanes;

use super::editor::MacroDialog;
use crate::state::MacroRun;
use crate::ui::ds::{self, set_ib};
use crate::ui::widgets::{
    self, Fluent, LabelFluent, hotkey_label, lbl, plural, switch, toggle_class,
};
use crate::ui::window::Window;

/// How many steps an unfolded card lists; a recording can have hundreds,
/// which its editor shows.
const SHOWN_STEPS: usize = 12;

/// The icon for an editor step type.
pub fn step_icon(kind: &str) -> &'static str {
    match kind {
        "Key" | "Press" | "Release" => "keyboard",
        "Hold" => "hold",
        "Repeat" => "repeat",
        "Type" => "type",
        "Click" | "Scroll" | "Path" | "Turn" => "mouse",
        "Move" => "move",
        "Wait" => "clock",
        "Start" => "start",
        "Stagger" => "users",
        "Note" => "note",
        "Timeline" | "At" => "timeline",
        "When" => "image",
        "Do" => "do-arrow",
        "Exit" => "exit",
        _ => "info",
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
    let steps =
        rows.iter().filter(|r| !matches!(r.kind.as_str(), "Note" | "Timeline" | "Stagger")).count();
    let mut about = Vec::new();
    if hotkey.is_some() {
        about.push(hotkey_label(hotkey.as_deref()));
    }
    about.push(plural(steps, "step", "steps"));
    about.push(loop_label(loops).to_lowercase());
    if let Some(r) = rows.iter().rfind(|r| r.kind == "Stagger" && !r.value.is_empty()) {
        about.push(format!("{} s apart", r.value));
    }
    let about = about.join(" · ");

    // -- the row: unfold, switch, name, Run, Edit --------------------------------
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8).css("mc-row");
    let fold = ds::ib("chev-right", if opened { "Fold" } else { "Show its steps" }, true);
    row.append(&fold);
    let toggled = name.to_owned();
    let weak = w.weak();
    let sw = switch(enabled, Some("Switched on: it can run"), move |on| {
        if let Some(w) = weak.upgrade() {
            w.enable_macro(&toggled, on);
        }
    });
    row.append(&sw);
    let sub = lbl(&about, "cx-row-s").ellipsize();
    let text_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text_box.append(&lbl(name, "cx-row-t").ellipsize());
    text_box.append(&sub);
    row.append(&text_box.hexpand().centered());
    let run_name = name.to_owned();
    let run = ds::ib("play-filled", "Run", true);
    run.add_css_class("go");
    run.connect_clicked({
        let w = w.weak();
        move |_| {
            if let Some(w) = w.upgrade() {
                w.run_macro_card(&run_name);
            }
        }
    });
    row.append(&run);
    let edit_name = name.to_owned();
    let edit = ds::ib("pencil", &format!("Edit {name}"), true);
    edit.connect_clicked({
        let w = w.weak();
        move |_| {
            if let Some(w) = w.upgrade() {
                MacroDialog::open(&w, Some(&edit_name));
            }
        }
    });
    row.append(&edit);

    // -- unfolded: its steps and what is wrong with them ---------------------------
    let body = gtk::Box::new(gtk::Orientation::Vertical, 4).css("mc-steps");
    let units = lanes::units(&rows);
    let listed: Vec<(usize, usize)> =
        units.iter().copied().filter(|(i, _)| rows[*i].kind != "Note").collect();
    for &(i, len) in listed.iter().take(SHOWN_STEPS) {
        let r = &rows[i];
        let shown = if r.kind == "Timeline" {
            let (drawn, n) = lanes::of_rows(&rows[i..i + len]);
            format!("{:.1} s · {}", drawn.secs, plural(n, "step", "steps"))
        } else if matches!(r.kind.as_str(), "Wait" | "Start" | "Stagger") && !r.value.is_empty() {
            format!("{} s", r.value)
        } else if r.value.is_empty() {
            "—".to_owned()
        } else {
            r.value.clone()
        };
        body.append(&hbox!(
            10,
            "mc-step",
            ds::icon(step_icon(&r.kind)),
            lbl(&r.kind, "t-label-sm").width(56),
            lbl(&shown, "t-code-sm muted").ellipsize().hexpand()
        ));
    }
    if listed.len() > SHOWN_STEPS {
        let more = plural(listed.len() - SHOWN_STEPS, "more step", "more steps");
        body.append(&lbl(&format!("…and {more}"), "t-caption muted"));
    }
    if rows.iter().all(|r| r.kind == "Note") {
        body.append(&lbl("No steps yet.", "t-caption muted"));
    }
    let broken = grammar::parse(&text).err();
    if let Some(e) = &broken {
        body.append(&lbl(&e.to_string(), "t-caption danger-text").wrapped());
    }
    let unfold = gtk::Revealer::builder().child(&body).reveal_child(opened).build();
    {
        let (unfold, toggled, weak) = (unfold.clone(), name.to_owned(), w.weak());
        fold.connect_clicked(move |b| {
            let open = !unfold.reveals_child();
            unfold.set_reveal_child(open);
            set_ib(
                b,
                if open { "chev-down" } else { "chev-right" },
                if open { "Fold" } else { "Show its steps" },
            );
            if let Some(w) = weak.upgrade() {
                w.set_macro_open(&toggled, open);
            }
        });
    }
    if opened {
        set_ib(&fold, "chev-down", "Fold");
    }
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.append(&row);
    card.append(&unfold);

    let (name, parses, shown) = (name.to_owned(), broken.is_none(), row.clone());
    w.watch_macros(Box::new(move |s| {
        let playing: Vec<&str> = s
            .macro_runs
            .iter()
            .filter(|(_, (_, m))| *m == name)
            .filter_map(|(id, _)| s.accounts.get(*id).map(|a| a.name.as_str()))
            .collect();
        let can = parses && s.macros.enabled(&name);
        toggle_class(&shown, "playing", !playing.is_empty());
        toggle_class(&shown, "off", !s.macros.enabled(&name));
        sub.set_label(&if playing.is_empty() {
            about.clone()
        } else {
            format!("Playing on {}", playing.join(", "))
        });
        // Run starts it on the selected accounts not yet playing it; once
        // every selected one plays it, the same button stops it.
        let action = s.macro_run(&name);
        let (stop, n) = match &action {
            MacroRun::Start(fresh) => (false, fresh.len()),
            MacroRun::Stop => (true, 0),
            MacroRun::Nothing => (false, 0),
        };
        let tip = if stop {
            "Stop it everywhere it plays".to_owned()
        } else if !s.macros.enabled(&name) {
            "Switched off: switch it on to run it".to_owned()
        } else if !parses {
            "Fix its steps to run it".to_owned()
        } else if n == 0 {
            "Select the accounts to run it on".to_owned()
        } else {
            format!(
                "Play it into {}",
                plural(n, "selected account's client", "selected accounts' clients")
            )
        };
        set_ib(&run, if stop { "stop" } else { "play-filled" }, &tip);
        run.set_sensitive(stop || (can && n > 0));
        widgets::name(&run, if stop { "Stop" } else { "Run" });
    }));
    card
}
