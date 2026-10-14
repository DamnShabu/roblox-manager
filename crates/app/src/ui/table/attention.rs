//! What needs the user, at the top of the table: each account whose session
//! expired or whose launch failed, why, and its fix in place.

use adw::prelude::*;
use gtk::{Align, Orientation};
use rbxmgr_core::accounts::Account;

use super::perf::PerfCell;
use super::row::{context_menu, inspect_on_click, menu};
use super::{Col, table_row};
use crate::state::{Inspected, Need};
use crate::ui::activity;
use crate::ui::ds::{self, Variant};
use crate::ui::login::AddAccountDialog;
use crate::ui::widgets::{Fluent, LabelFluent, lbl, name, sentence, toggle_class};
use crate::ui::window::{Window, monitor_hz};

/// The section: its head, then a row for each need.
pub fn attention_section(w: &Window, needs: &[Need]) -> gtk::Box {
    let section = gtk::Box::new(Orientation::Vertical, 0);
    let head = gtk::Box::new(Orientation::Horizontal, 8).css("mn-sec");
    head.append(&ds::icon("error").css("bad s16"));
    head.append(&lbl("Needs attention", "mn-sec-t"));
    head.append(&lbl(&needs.len().to_string(), "mn-dim"));
    section.append(&head);
    for need in needs {
        let acct = w.state().accounts.get(need.id).cloned();
        if let Some(acct) = acct {
            section.append(&attention_row(w, &acct, need));
        }
    }
    section
}

fn attention_row(w: &Window, acct: &Account, need: &Need) -> gtk::Box {
    let id = acct.user_id;
    let cols = w.columns();
    let hz = monitor_hz();
    let row = table_row("mn-row attn");
    let check = ds::check(acct.selected, &format!("Select {}", acct.name));
    check.connect_toggled({
        let weak = w.weak();
        move |c| {
            if let Some(w) = weak.upgrade() {
                w.select_accounts(&[id], c.is_active());
            }
        }
    });
    row.append(&cols.cell(Col::Check, &check));

    let group = {
        let s = w.state();
        s.accounts
            .group_of(acct)
            .and_then(|g| s.accounts.groups().iter().find(|x| x.id == g))
            .map(|g| if g.name.is_empty() { "Untitled group".to_owned() } else { g.name.clone() })
            .unwrap_or_else(|| "Ungrouped".to_owned())
    };
    let picture = w.services().avatars.cached(&id.to_string());
    let who = gtk::Box::new(Orientation::Horizontal, 8);
    who.append(&ds::av(acct.name.as_str(), picture.as_deref(), 26));
    who.append(&lbl(acct.name.as_str(), "mn-name").ellipsize());
    who.append(&lbl(if acct.leader { "Main account" } else { &group }, "mn-user").ellipsize());
    row.append(&cols.cell(Col::Name, &who));

    let reason = gtk::Box::new(Orientation::Horizontal, 10);
    let (what, why) = if need.expired {
        ("Signed out", "Roblox no longer takes its session".to_owned())
    } else {
        ("Failed to start", sentence(&need.why))
    };
    reason.append(&lbl(what, "mn-state bad"));
    reason.append(&lbl(&why, "mn-dim").ellipsize().hexpand().tip(&why));
    if need.expired {
        let again = acct.clone();
        let b =
            ds::Button::with_icons("Sign in again", Variant::Secondary, true, Some("login"), None)
                .on(w.act(move |w| AddAccountDialog::open(w, Some(again.clone()))));
        reason.append(&b.button.centered());
    } else {
        let b =
            ds::Button::with_icons("Launch again", Variant::Secondary, true, Some("replay"), None)
                .on(w.act(move |w| w.play_or_stop(id)));
        reason.append(&b.button.centered());
        let log = ds::Button::with_icons("Log", Variant::Ghost, true, Some("log"), None)
            .on(w.act(activity::open_log));
        reason.append(&log.button.centered());
    }
    row.append(&cols.span(&[Col::Status, Col::Uptime, Col::Cpu, Col::Memory, Col::Macro], &reason));

    let perf = PerfCell::build(id, acct.name.as_str(), hz);
    row.append(&cols.cell(Col::Perf, &perf.root));
    let model = menu(w, acct);
    let more = gtk::MenuButton::builder()
        .child(&ds::icon("more"))
        .menu_model(&model)
        .valign(Align::Center)
        .halign(Align::End)
        .tooltip_text(format!("More for {}", acct.name))
        .css_classes(["ib", "sm", "mn-act"])
        .build();
    name(&more, &format!("More for {}", acct.name));
    row.append(&cols.cell(Col::Actions, &more));
    context_menu(&row, &model);
    inspect_on_click(w, &row, id);
    let shown = row.clone();
    w.watch_accounts(Box::new(move |s| {
        perf.draw(s, id, hz);
        toggle_class(&shown, "current", s.inspected == Some(Inspected::Account(id)));
    }));
    row
}
