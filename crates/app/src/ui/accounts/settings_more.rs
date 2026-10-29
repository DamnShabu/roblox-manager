//! The lower half of an account's panel: the macro its Run plays and its
//! macro-ready window, and whether Roblox still takes its session.

use adw::prelude::*;
use gtk::{Align, Orientation};
use rbxmgr_core::accounts::{Account, SessionState, relative_time};
use rbxmgr_core::types::Profile;

use crate::ui::ds::{self, Variant};
use crate::ui::login::AddAccountDialog;
use crate::ui::widgets::{Fluent, LabelFluent, lbl, switch, toggle_class};
use crate::ui::window::Window;

/// The macro it plays, Run, where it is; its macro-ready window.
pub fn macro_sec(w: &Window, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let names: Vec<String> = w.state().macros.names().map(str::to_owned).collect();
    let sec = ds::sec("Macro", None);
    let mine = acct.macro_name.clone().filter(|m| names.contains(m));
    let options: Vec<&str> =
        std::iter::once("None").chain(names.iter().map(String::as_str)).collect();
    let at = mine.as_ref().and_then(|m| names.iter().position(|n| n == m)).map_or(0, |i| i + 1);

    let nested = switch(acct.nested, Some("Macro-ready window"), {
        let weak = w.weak();
        move |on| {
            if let Some(w) = weak.upgrade() {
                w.set_nested(id, on);
            }
        }
    });
    let (weak, nested_sw, names) = (w.weak(), nested.clone(), names.clone());
    let pick = ds::select(&options, at, move |i| {
        let name = i.checked_sub(1).and_then(|i| names.get(i)).cloned();
        if let Some(w) = weak.upgrade() {
            let picked = name.is_some();
            w.pick_macro(id, name);
            if picked {
                nested_sw.set_active(true);
            }
        }
    });
    let run = ds::Button::new("Run", Variant::Secondary, false).on(w.act(move |w| {
        w.play_macro_here(id);
        w.refresh_states();
    }));
    let line = gtk::Box::new(Orientation::Horizontal, 8);
    line.append(&pick.hexpand());
    line.append(&run.button);
    let at_now = lbl("", "t-caption muted").ellipsize();
    let dot = gtk::Box::new(Orientation::Horizontal, 0).css("cx-dot run");
    dot.set_valign(Align::Center);
    let playing = gtk::Box::new(Orientation::Horizontal, 8);
    playing.append(&dot);
    playing.append(&at_now);
    let field = ds::sfield("Macro", &line, None);
    field.append(&playing);
    sec.append(&field);

    let text = gtk::Box::new(Orientation::Vertical, 2);
    text.append(&lbl("Macro-ready window", "t-label"));
    text.append(
        &lbl(
            "Runs the client on a display of its own, where a macro can reach it. Picking a \
             macro turns it on.",
            "cx-help",
        )
        .wrapped(),
    );
    text.append(&ds::next_launch());
    let ready = gtk::Box::new(Orientation::Horizontal, 12);
    ready.append(&text.hexpand());
    nested.set_valign(Align::Center);
    ready.append(&nested);
    sec.append(&ready);

    let (run, playing, at_now) = (run.clone(), playing.downgrade(), at_now.downgrade());
    w.watch_while(Box::new(move |s| {
        let (Some(playing), Some(at_now)) = (playing.upgrade(), at_now.upgrade()) else {
            return false;
        };
        let now = s.macro_runs.get(&id).map(|(_, m)| m.clone());
        let chosen = s.accounts.get(id).and_then(|a| a.macro_name.clone());
        run.set_text(if now.is_some() { "Stop" } else { "Run" });
        run.button.set_sensitive(now.is_some() || chosen.is_some_and(|m| s.macros.contains(&m)));
        run.button.set_tooltip_text(Some(&match &now {
            Some(m) => format!("Stop {m} on this account"),
            None => "Play the macro into this account's client".to_owned(),
        }));
        match now {
            Some(m) => {
                let at = s.macro_progress.get(&id).map_or("starting", String::as_str);
                at_now.set_label(&format!("{m}: {at}"));
                playing.set_visible(true);
            }
            None => playing.set_visible(false),
        }
        true
    }));
    sec
}

/// Whether Roblox still takes its stored session, and the fix when not.
pub fn session_sec(w: &Window, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let relogin = acct.clone();
    let sign_in = ds::Button::new("Sign in again", Variant::Ghost, true)
        .on(w.act(move |w| AddAccountDialog::open(w, Some(relogin.clone()))));
    let sec = ds::sec("Session", Some(sign_in.button.upcast_ref()));
    let dot = gtk::Box::new(Orientation::Horizontal, 0).css("cx-dot");
    dot.set_valign(Align::Center);
    let state = lbl("", "t-body-sm").ellipsize();
    let check = ds::Button::new("Check", Variant::Secondary, true)
        .on(w.act(move |w| w.check_sessions(vec![id])));
    let line = gtk::Box::new(Orientation::Horizontal, 8);
    line.append(&dot);
    line.append(&state.clone().hexpand());
    line.append(&check.button);
    sec.append(&line);
    let profile = Profile::of(id).to_string();
    let copy = ds::ib("copy", "Copy the profile's name", true);
    {
        let (weak, profile) = (w.weak(), profile.clone());
        copy.connect_clicked(move |_| {
            if let Some(w) = weak.upgrade() {
                w.gtk_window().clipboard().set_text(&profile);
                w.toast("Profile name copied");
            }
        });
    }
    let prof = gtk::Box::new(Orientation::Horizontal, 8);
    prof.append(&lbl("Cordial profile", "cx-lab"));
    prof.append(&lbl(&profile, "cx-kbd"));
    prof.append(&copy);
    sec.append(&prof);
    sec.append(
        &lbl(
            "Kept in your keyring. Sign in again when launches fail with “session expired”.",
            "cx-help",
        )
        .wrapped(),
    );
    let (dot, state, check) = (dot.downgrade(), state.downgrade(), check.clone());
    w.watch_while(Box::new(move |s| {
        let (Some(dot), Some(state)) = (dot.upgrade(), state.upgrade()) else { return false };
        let session = s.accounts.session(id);
        let (text, kind) = match &session {
            SessionState::Checking => ("Checking…".to_owned(), "start"),
            SessionState::Expired => {
                ("Expired · Roblox no longer takes this session".to_owned(), "bad")
            }
            SessionState::Ok { checked: Some(when) } => (
                format!(
                    "Signed in · Roblox took it {}",
                    relative_time(Some(when), chrono::Utc::now())
                ),
                "run",
            ),
            SessionState::Ok { checked: None } => ("Signed in".to_owned(), "run"),
        };
        state.set_label(&text);
        for k in ["start", "bad", "run"] {
            toggle_class(&dot, k, k == kind);
        }
        check.button.set_sensitive(session != SessionState::Checking);
        true
    }));
    sec
}
