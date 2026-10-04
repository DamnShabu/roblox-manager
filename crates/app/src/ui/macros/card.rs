//! One macro in the side pane: its switch and Run, and unfolded, its steps.

use adw::prelude::*;
use gtk::Align;
use rbxmgr_core::macros::grammar::{self, loop_label};
use rbxmgr_core::macros::lanes;

use super::editor::MacroDialog;
use crate::state::MacroRun;
use crate::ui::widgets::{
    self, Btn, Fluent, LabelFluent, boxed_list, hotkey_label, icon, lbl, plural, switch,
    toggle_class,
};
use crate::ui::window::Window;

/// How many steps an unfolded card lists; a recording can have hundreds,
/// which its editor shows.
const SHOWN_STEPS: usize = 12;

/// The icon for an editor step type.
pub fn step_icon(kind: &str) -> &'static str {
    match kind {
        "Key" | "Hold" | "Press" | "Release" | "Repeat" => "input-keyboard-symbolic",
        "Type" => "insert-text-symbolic",
        "Click" | "Scroll" | "Path" | "Turn" => "input-mouse-symbolic",
        "Move" => "go-jump-symbolic",
        "Wait" => "appointment-soon-symbolic",
        "Start" => "alarm-symbolic",
        "Stagger" => "view-continuous-symbolic",
        "Note" => "text-x-generic-symbolic",
        "Timeline" | "At" => "document-open-recent-symbolic",
        "When" | "Do" => "image-x-generic-symbolic",
        "Exit" => "media-skip-forward-symbolic",
        _ => "system-run-symbolic",
    }
}

pub fn macro_card(w: &Window, name: &str) -> gtk::ListBox {
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

    let list = boxed_list();
    list.add_css_class("macro");
    let row = adw::ExpanderRow::builder()
        .title(name)
        .subtitle(&about)
        .use_markup(false)
        .expanded(opened)
        .build();
    let toggled = name.to_owned();
    let weak = w.weak();
    row.connect_expanded_notify(move |r| {
        if let Some(w) = weak.upgrade() {
            w.set_macro_open(&toggled, r.is_expanded());
        }
    });

    let run_name = name.to_owned();
    let run = Btn::new("flat circular")
        .icon("media-playback-start-symbolic")
        .build(w.act(move |w| w.run_macro_card(&run_name)));
    run.button.set_valign(Align::Center);
    let toggled = name.to_owned();
    let weak = w.weak();
    let sw = switch(enabled, Some("Switched on: it can run"), move |on| {
        if let Some(w) = weak.upgrade() {
            w.enable_macro(&toggled, on);
        }
    });
    row.add_suffix(&run.button);
    row.add_suffix(&sw);

    // Unfolded: the steps, what is wrong with them, Edit and Run.
    let body = vbox!(4, "macro-steps");
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
            "macro-step",
            icon(step_icon(&r.kind)),
            lbl(&r.kind, "kind").width(64),
            lbl(&shown, "monospace dimmed").ellipsize().hexpand()
        ));
    }
    if listed.len() > SHOWN_STEPS {
        let more = plural(listed.len() - SHOWN_STEPS, "more step", "more steps");
        body.append(&lbl(&format!("…and {more}"), "dimmed caption"));
    }
    if rows.iter().all(|r| r.kind == "Note") {
        body.append(&lbl("No steps yet.", "dimmed"));
    }
    let broken = grammar::parse(&text).err();
    if let Some(e) = &broken {
        body.append(&lbl(&e.to_string(), "error caption").wrapped());
    }
    let edit_name = name.to_owned();
    let edit = Btn::new("")
        .text("Edit…")
        .icon("document-edit-symbolic")
        .build(w.act(move |w| MacroDialog::open(w, Some(&edit_name))));
    let run_name = name.to_owned();
    let run_on =
        Btn::new("suggested-action").text("Run").build(w.act(move |w| w.run_macro_card(&run_name)));
    let buttons = hbox!(8, "", edit.button.hexpand(), run_on.button.clone().hexpand());
    buttons.set_homogeneous(true);
    buttons.set_margin_top(8);
    body.append(&buttons);
    let holder =
        gtk::ListBoxRow::builder().activatable(false).selectable(false).child(&body).build();
    row.add_row(&holder);
    list.append(&row);

    let (name, parses, card) = (name.to_owned(), broken.is_none(), list.clone());
    w.watch_macros(Box::new(move |s| {
        let playing = s.macro_runs.values().filter(|(_, m)| *m == name).count();
        let can = parses && s.macros.enabled(&name);
        toggle_class(&card, "running", playing > 0);
        row.set_subtitle(&if playing > 0 {
            format!("Playing on {}", plural(playing, "client", "clients"))
        } else {
            about.clone()
        });
        // Run starts it on the selected accounts not yet playing it; once
        // every selected one plays it, the same button stops it.
        let action = s.macro_run(&name);
        let (stop, n) = match &action {
            MacroRun::Start(fresh) => (false, fresh.len()),
            MacroRun::Stop => (true, 0),
            MacroRun::Nothing => (false, 0),
        };
        for b in [&run, &run_on] {
            b.set_icon(if stop {
                "media-playback-stop-symbolic"
            } else {
                "media-playback-start-symbolic"
            });
            b.button.set_sensitive(stop || (can && n > 0));
        }
        run_on.set_text(&match (stop, playing) {
            (true, _) => "Stop".to_owned(),
            (false, 0) => format!("Run on {n} Selected"),
            (false, _) => format!("Run on {n} More"),
        });
        toggle_class(&run_on.button, "suggested-action", !stop);
        toggle_class(&run_on.button, "destructive-action", stop);
        let tip = if stop {
            "Stop it everywhere it plays".to_owned()
        } else if !s.macros.enabled(&name) {
            "Switched off: switch it on to run it".to_owned()
        } else if !parses {
            "Fix its steps to run it".to_owned()
        } else if n == 0 {
            "Select the accounts to run it on".to_owned()
        } else if playing > 0 {
            format!(
                "Start it from the top on {}; where it already plays, it plays on",
                plural(n, "more selected account", "more selected accounts")
            )
        } else {
            format!(
                "Play it into {}",
                plural(n, "selected account's client", "selected accounts' clients")
            )
        };
        run.button.set_tooltip_text(Some(&tip));
        widgets::name(&run.button, if stop { "Stop" } else { "Run" });
        run_on.button.set_tooltip_text(Some(&tip));
    }));
    list
}
