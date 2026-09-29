//! An account's settings panel, under its row.

use adw::prelude::*;
use gtk::WrapMode;
use rbxmgr_core::accounts::{Account, SessionState, relative_time};
use rbxmgr_core::types::Profile;

use crate::ui::login::AddAccountDialog;
use crate::ui::widgets::{
    Btn, Fluent, LabelFluent, armed_btn, dot, icon, icon_fill, lbl, setting, switch, wrap,
};
use crate::ui::window::Window;

pub fn panel(w: &Window, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let panel = vbox!(18, "panel");
    let row = |title: &str, content: &gtk::Widget, top: i32| setting(&panel, title, content, top);

    let (groups, leader_name, macros, playing, session) = {
        let s = w.state();
        let groups: Vec<(Option<String>, String)> = std::iter::once((None, "Ungrouped".to_owned()))
            .chain(s.accounts.groups().iter().map(|g| {
                let name =
                    if g.name.is_empty() { "Untitled group".to_owned() } else { g.name.clone() };
                (Some(g.id.clone()), name)
            }))
            .collect();
        let group = s.accounts.group_of(acct).map(str::to_owned);
        let leader = s.accounts.leader().map(|l| l.name.to_string());
        let macros: Vec<String> = s.macros.names().map(str::to_owned).collect();
        let playing = s.macro_runs.contains_key(&id);
        ((groups, group), leader, macros, playing, s.accounts.session(id))
    };

    if acct.leader {
        row(
            "Leader",
            &hbox!(6, "leadnote", icon_fill("star", 18, ""), lbl("This account is the leader", ""))
                .upcast(),
            7,
        );
        row(
            "Group",
            &lbl(
                "The leader can't be in a group. Make another account leader from its settings to hand over the lead.",
                "phint",
            )
            .wrapped()
            .chars(72)
            .upcast(),
            0,
        );
    } else {
        let make = Btn::new("obtn")
            .text("Make leader")
            .icon("star")
            .build(w.act(move |w| w.set_leader(id)));
        row("Leader", &hbox!(0, "", make.button).upcast(), 7);
        let (options, now) = groups;
        let buttons: Vec<gtk::Widget> = options
            .into_iter()
            .map(|(gid, name)| {
                let css = if gid == now { "opt on" } else { "opt" };
                Btn::new(css)
                    .text(&name)
                    .build(w.act(move |w| w.set_group(id, gid.clone())))
                    .button
                    .upcast()
            })
            .collect();
        row("Group", &wrap(6, &buttons).upcast(), 7);
        if let Some(leader) = leader_name {
            let sw = switch(acct.follow.is_some(), None, {
                let weak = w.weak();
                move |on| {
                    if let Some(w) = weak.upgrade() {
                        w.set_follow(id, on);
                    }
                }
            });
            let text = lbl(&format!("Joins {leader}'s server right after it launches."), "phint")
                .wrapped()
                .chars(72);
            row("Auto-join", &hbox!(12, "", sw, text).upcast(), 2);
        }
    }

    let label =
        gtk::Entry::builder().text(acct.name.as_str()).hexpand(true).css_classes(["field"]).build();
    let weak = w.weak();
    label.connect_activate(move |e| {
        if let Some(w) = weak.upgrade() {
            w.rename_account(id, e.text().trim());
        }
    });
    let entry = label.clone();
    let rename = Btn::new("sbtn")
        .text("Rename")
        .build(w.act(move |w| w.rename_account(id, entry.text().trim())));
    row(
        "Label",
        &vbox!(
            6,
            "",
            hbox!(8, "", label, rename.button.centered()),
            lbl("Your name for this account; renaming moves its keyring entry too.", "phint2")
                .wrapped()
        )
        .upcast(),
        10,
    );

    // accounts.json is not encrypted at rest, so the note is for labels, not
    // secrets. It grows with its text: a scroller's minimum height here made
    // GTK measure the panel smaller than its own minimum.
    let note = gtk::TextView::builder()
        .wrap_mode(WrapMode::WordChar)
        .top_margin(9)
        .bottom_margin(9)
        .left_margin(12)
        .right_margin(12)
        .accepts_tab(false)
        .height_request(62)
        .hexpand(true)
        .build();
    note.buffer().set_text(&acct.note);
    let weak = w.weak();
    note.buffer().connect_changed(move |b| {
        if let Some(w) = weak.upgrade() {
            w.set_note(id, &b.text(&b.start_iter(), &b.end_iter(), false));
        }
    });
    let notebox = hbox!(0, "notebox", note);
    notebox.set_overflow(gtk::Overflow::Hidden);
    row(
        "Note",
        &vbox!(
            6,
            "",
            notebox,
            hbox!(
                5,
                "phint2",
                icon("sticky_note_2", 14, ""),
                lbl("Hover the note icon next to the name to read it. Plain text -- not for passwords.", "")
            )
        )
        .upcast(),
        10,
    );

    let mine = acct.macro_name.clone().filter(|m| macros.contains(m));
    let run = Btn::new("sbtn")
        .text(if playing { "Stop here" } else { "Run here" })
        .icon(if playing { "stop" } else { "play_arrow" })
        .fill()
        .gap(4)
        .tip(if playing {
            "Stop the macro on this account"
        } else {
            "Play the macro into this account's client"
        })
        .build(w.act(move |w| w.play_macro_here(id)));
    run.button.set_sensitive(mine.is_some() || playing);
    let mut options: Vec<gtk::Widget> = std::iter::once(None)
        .chain(macros.into_iter().map(Some))
        .map(|m| {
            let css = if m == mine { "opt on" } else { "opt" };
            let text = m.clone().unwrap_or_else(|| "None".to_owned());
            Btn::new(css)
                .text(&text)
                .build(w.act(move |w| w.pick_macro(id, m.clone())))
                .button
                .upcast()
        })
        .collect();
    let divider = gtk::Box::new(gtk::Orientation::Horizontal, 0).css("vdiv").centered();
    divider.set_margin_start(4);
    divider.set_margin_end(4);
    options.push(divider.upcast());
    options.push(run.button.upcast());
    row("Macro", &wrap(6, &options).upcast(), 7);

    let toggle =
        |title: &str, on: bool, text: &str, set: fn(&Window, rbxmgr_core::types::UserId, bool)| {
            let weak = w.weak();
            let sw = switch(on, None, move |on| {
                if let Some(w) = weak.upgrade() {
                    set(&w, id, on);
                }
            });
            let hint = lbl(text, "phint").wrapped().chars(72).hexpand();
            setting(&panel, title, &hbox!(12, "", sw, hint), 2);
        };
    toggle(
        "Macro-ready window",
        acct.nested,
        "Runs the client on a display of its own, so a macro can play into it while you use other windows. \
         Picking a macro turns it on. Applies from the next launch.",
        Window::set_nested,
    );
    toggle(
        "Low-power client",
        acct.low_power,
        "For an account you are not playing on: 20 FPS, fewer CPU threads, lower priority, and slower still \
         when its window is not focused. Applies from the next launch.",
        Window::set_low_power,
    );

    let (text, kind) = match &session {
        SessionState::Checking => ("Checking session…".to_owned(), "checking"),
        SessionState::Expired => ("Session expired".to_owned(), "expired"),
        SessionState::Ok { checked: Some(when) } => {
            (format!("Signed in · checked {}", relative_time(Some(when), chrono::Utc::now())), "ok")
        }
        SessionState::Ok { checked: None } => ("Signed in".to_owned(), "ok"),
    };
    let action = if session == SessionState::Expired {
        let relogin = acct.clone();
        Btn::new("sbtn amber")
            .text("Sign in again")
            .icon("login")
            .size(17)
            .gap(5)
            .build(w.act(move |w| AddAccountDialog::open(w, Some(relogin.clone()))))
    } else {
        let checking = session == SessionState::Checking;
        let b = Btn::new("sbtn")
            .text(if checking { "Checking…" } else { "Check session" })
            .icon("sync")
            .size(17)
            .gap(5)
            .tip("Ask Roblox whether the stored session still works")
            .build(w.act(move |w| w.check_sessions(vec![id])));
        b.button.set_sensitive(!checking);
        b
    };
    let dot_kind = match kind {
        "checking" => "wait",
        "expired" => "expired",
        _ => "running",
    };
    row(
        "Session",
        &vbox!(
            6,
            "",
            hbox!(
                12,
                "",
                hbox!(
                    7,
                    &format!("sess {kind}"),
                    dot(dot_kind, kind == "checking", 7),
                    lbl(&text, "")
                ),
                action.button
            ),
            lbl(
                &format!(
                    "Sign in again when launches fail with HTTP 401. Cordial profile {}.",
                    Profile::of(id)
                ),
                "phint2"
            )
            .wrapped()
            .selectable()
        )
        .upcast(),
        8,
    );
    let remove = armed_btn(
        "Remove account",
        "Click again to remove",
        "person_remove",
        w.act(move |w| w.remove_account(id)),
    );
    row("", &hbox!(0, "prm", remove).upcast(), 0);
    panel
}
