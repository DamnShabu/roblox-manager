//! An account in the inspector: its head (how it is, stop and hide), then
//! its label and note, how it launches, its macro, its session, and
//! removing it. Changes apply as they are made.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType, WrapMode};
use rbxmgr_core::accounts::{Account, relative_time};
use rbxmgr_core::cordial::{Performance, Window as ClientWindow};
use rbxmgr_core::types::UserId;

use super::settings_more::{macro_sec, session_sec};
use crate::state::{Chip, Inspected};
use crate::ui::ds::{self, Tone};
use crate::ui::panel::Panel;
use crate::ui::table::row::confirm_row;
use crate::ui::widgets::{LabelFluent, clear, lbl, toggle_class};
use crate::ui::window::{Window, can_hide};

pub struct AccountSettings;

impl AccountSettings {
    pub fn open(w: &Window, id: UserId) {
        let Some(acct) = w.state().accounts.get(id).cloned() else { return };
        let panel = Panel::new(acct.name.as_str());
        panel.set_child(Some(&content(w, &panel, &acct)));
        panel.present(w);
        w.inspect(Some(Inspected::Account(id)));
    }
}

/// Everything about one account. Rebuilt in place when the account changes
/// shape (made leader), so what shows is what applies.
fn content(w: &Window, panel: &Panel, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let body = gtk::Box::new(Orientation::Vertical, 0);
    body.append(&identity(w, acct));
    body.append(&launching(w, panel, acct));
    body.append(&macro_sec(w, acct));
    body.append(&session_sec(w, acct));
    let remove =
        ds::Button::with_icons("Remove account…", ds::Variant::Ghost, false, Some("trash"), None);
    remove.button.add_css_class("danger-text");
    remove.button.set_halign(Align::Start);
    let confirm = {
        let (weak, p) = (w.weak(), panel.downgrade());
        confirm_row(
            &format!("Remove {}?", acct.name),
            "Its session leaves the keyring and its client is closed. The Roblox account itself \
             is untouched; add it again any time with Quick Login.",
            "Remove",
            move || {
                if let Some(w) = weak.upgrade() {
                    w.remove_account(id);
                }
                if let Some(p) = p.upgrade() {
                    p.close();
                }
            },
        )
    };
    {
        let c = confirm.clone();
        remove.button.connect_clicked(move |_| c.set_reveal_child(true));
    }
    let danger = ds::sec("", None);
    danger.append(&remove.button);
    danger.append(&confirm);
    body.append(&danger);

    let root = gtk::Box::new(Orientation::Vertical, 0);
    root.add_css_class("cx-panel");
    root.append(&head(w, panel, acct));
    root.append(
        &gtk::ScrolledWindow::builder()
            .child(&body)
            .hscrollbar_policy(PolicyType::Never)
            .vexpand(true)
            .build(),
    );
    root
}

/// Its avatar, name and how it is now; its client's stop and hide.
fn head(w: &Window, panel: &Panel, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let picture = w.services().avatars.cached(&id.to_string());
    let title = gtk::Box::new(Orientation::Horizontal, 8);
    title.append(&lbl(acct.name.as_str(), "cx-head-title").ellipsize());
    let badge = gtk::Box::new(Orientation::Horizontal, 0);
    title.append(&badge);
    let user = acct.username.as_deref().unwrap_or(acct.name.as_str());
    let sub = lbl(&format!("@{user} · {id}"), "cx-head-sub").ellipsize();
    let stop = ds::ib("stop", "Close its client", true);
    stop.add_css_class("stop");
    stop.connect_clicked({
        let w = w.weak();
        move |_| {
            if let Some(w) = w.upgrade() {
                w.stop_account(id);
            }
        }
    });
    let hide = ds::ib("eye-off", "Hide its window", true);
    hide.connect_clicked({
        let w = w.weak();
        move |_| {
            if let Some(w) = w.upgrade() {
                let hide = can_hide(&w.state(), id);
                w.set_window_hidden(id, hide);
            }
        }
    });
    let h = ds::head(
        &ds::av(acct.name.as_str(), picture.as_deref(), 36),
        title.upcast_ref(),
        sub.upcast_ref(),
        &[stop.clone().upcast(), hide.clone().upcast(), panel.close_button().upcast()],
    );
    let (badge, stop, hide) = (badge.downgrade(), stop.downgrade(), hide.downgrade());
    w.watch_while(Box::new(move |s| {
        let (Some(badge), Some(stop), Some(hide)) =
            (badge.upgrade(), stop.upgrade(), hide.upgrade())
        else {
            return false;
        };
        clear(&badge);
        match s.chip(id) {
            Chip::Running => badge.append(&ds::badge(Tone::Success, "Running", true, false)),
            Chip::Starting => badge.append(&ds::badge(Tone::Warning, "Starting…", true, true)),
            Chip::Joining => badge.append(&ds::badge(Tone::Warning, "Joining…", true, true)),
            Chip::Expired => badge.append(&ds::badge(Tone::Danger, "Session expired", true, false)),
            Chip::Idle => {}
        }
        let live = s.running.contains(&id) || s.launching.contains(&id);
        stop.set_visible(live);
        let hidden = s.windows.get(&id) == Some(&ClientWindow::Hidden);
        hide.set_visible(hidden || can_hide(s, id));
        ds::set_ib(
            &hide,
            if hidden { "eye-off" } else { "eye" },
            if hidden { "Show its window" } else { "Hide its window; the game keeps running" },
        );
        toggle_class(&hide, "on", hidden);
        true
    }));
    h
}

/// Its label and its note.
fn identity(w: &Window, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let who = acct
        .display
        .as_ref()
        .map(|d| lbl(&format!("{d} on Roblox"), "t-caption muted").upcast::<gtk::Widget>());
    let sec = ds::sec("Identity", who.as_ref());
    sec.add_css_class("first");
    let label = ds::input(acct.name.as_str());
    let apply = {
        let weak = w.weak();
        move |e: &gtk::Entry| {
            if let Some(w) = weak.upgrade() {
                w.rename_account(id, e.text().trim());
            }
        }
    };
    label.connect_activate(apply.clone());
    let focus = gtk::EventControllerFocus::new();
    {
        let label = label.clone();
        focus.connect_leave(move |_| apply(&label));
    }
    label.add_controller(focus);
    sec.append(&ds::sfield("Label", &label, Some("Renaming moves its keyring entry too.")));
    // accounts.json is not encrypted at rest, so the note is for labels, not
    // secrets.
    let note = gtk::TextView::builder()
        .wrap_mode(WrapMode::WordChar)
        .accepts_tab(false)
        .height_request(48)
        .css_classes(["cx-input"])
        .build();
    note.buffer().set_text(&acct.note);
    let weak = w.weak();
    note.buffer().connect_changed(move |b| {
        if let Some(w) = weak.upgrade() {
            w.set_note(id, &b.text(&b.start_iter(), &b.end_iter(), false));
        }
    });
    sec.append(&ds::sfield(
        "Note",
        &note,
        Some("Shown on the note icon beside its name. Plain text — not for passwords."),
    ));
    sec
}

/// Its group, its role in the launch order, and how its client runs.
fn launching(w: &Window, panel: &Panel, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let (groups, current, leader, place, followers) = {
        let s = w.state();
        let groups: Vec<(Option<String>, String)> = std::iter::once((None, "Ungrouped".into()))
            .chain(s.accounts.groups().iter().map(|g| {
                let name = if g.name.is_empty() { "Untitled group".into() } else { g.name.clone() };
                (Some(g.id.clone()), name)
            }))
            .collect();
        let current = s.accounts.group_of(acct).map(str::to_owned);
        (
            groups,
            current,
            s.accounts.leader().map(|l| l.name.to_string()),
            acct.follow,
            s.accounts.followers().len(),
        )
    };
    let launched = match acct.last_launch.as_deref() {
        None => "Never launched".to_owned(),
        when => format!("Launched {}", relative_time(when, chrono::Utc::now())),
    };
    let sec = ds::sec("Launch", Some(lbl(&launched, "t-caption muted").upcast_ref()));

    let names: Vec<&str> = groups.iter().map(|(_, n)| n.as_str()).collect();
    let at = groups.iter().position(|(g, _)| *g == current).unwrap_or(0);
    let (weak, groups) = (w.weak(), groups.clone());
    let group = ds::select(&names, at, move |i| {
        let gid = groups.get(i).and_then(|(g, _)| g.clone());
        if let Some(w) = weak.upgrade() {
            w.set_group(id, gid);
        }
    });
    sec.append(&ds::sfield("Group", &group, None));

    // Solo, Auto-join or Leader. Making it the leader redraws the panel:
    // its auto-join place no longer applies.
    let role_now = if acct.leader {
        "leader"
    } else if place.is_some() {
        "join"
    } else {
        "solo"
    };
    let (weak, p) = (w.weak(), panel.downgrade());
    let busy = Rc::new(Cell::new(false));
    let role = ds::seg(
        &[("solo", "Solo"), ("join", "Auto-join"), ("leader", "Leader")],
        role_now,
        move |key| {
            let (Some(w), Some(p)) = (weak.upgrade(), p.upgrade()) else { return };
            if busy.replace(true) {
                return;
            }
            match key {
                "leader" => w.set_leader(id),
                "join" => w.set_follow(id, true),
                _ => w.set_follow(id, false),
            }
            if let Some(acct) = w.state().accounts.get(id).cloned() {
                p.set_child(Some(&content(&w, &p, &acct)));
            }
            busy.set(false);
        },
    );
    if acct.leader || leader.is_none() {
        // The leader stays one until another is made leader; with none,
        // there is nothing to auto-join.
        let mut child = role.first_child();
        while let Some(b) = child {
            if acct.leader {
                b.set_sensitive(false);
            }
            child = b.next_sibling();
        }
        if !acct.leader
            && let Some(join) = role.first_child().and_then(|c| c.next_sibling())
        {
            join.set_sensitive(false);
        }
    }
    let role_box = gtk::Box::new(Orientation::Vertical, 6);
    role_box.append(&role);
    let help = match (acct.leader, place, &leader) {
        (true, _, _) => {
            "Launch as group starts it first; its auto-join accounts follow it".to_owned()
        }
        (false, Some(n), Some(l)) => {
            let order = gtk::Box::new(Orientation::Horizontal, 4);
            order.append(&lbl(&format!("#{n} of {followers}"), "cx-chip"));
            let up = ds::ib("chev-up", "Join earlier", true);
            up.set_sensitive(n > 1);
            let down = ds::ib("chev-down", "Join later", true);
            down.set_sensitive((n as usize) < followers);
            for (b, by) in [(&up, -1), (&down, 1)] {
                let (weak, p) = (w.weak(), panel.downgrade());
                b.connect_clicked(move |_| {
                    let (Some(w), Some(p)) = (weak.upgrade(), p.upgrade()) else { return };
                    w.move_follower(id, by);
                    if let Some(acct) = w.state().accounts.get(id).cloned() {
                        p.set_child(Some(&content(&w, &p, &acct)));
                    }
                });
                order.append(b);
            }
            role_box.append(&order);
            format!("Joins {l}'s server right after it launches")
        }
        (false, _, Some(l)) => format!("Launches on its own; Auto-join sends it into {l}'s server"),
        (false, _, None) => {
            "No leader yet: make one the leader to launch them as a group".to_owned()
        }
    };
    role_box.append(&lbl(&help, "cx-help").wrapped());
    sec.append(&ds::sfield("Role", &role_box, None));

    let detail = lbl(performance_detail(acct.performance()), "cx-help").wrapped();
    let opts: Vec<(&str, &str)> = Performance::ALL.iter().map(|p| (p.label(), p.label())).collect();
    let (weak, d) = (w.weak(), detail.clone());
    let perf = ds::seg(&opts, acct.performance().label(), move |key| {
        let Some(level) = Performance::ALL.into_iter().find(|p| p.label() == key) else { return };
        d.set_label(performance_detail(level));
        if let Some(w) = weak.upgrade() {
            w.set_performance(id, level);
        }
    });
    let perf_box = gtk::Box::new(Orientation::Vertical, 6);
    perf_box.append(&perf);
    perf_box.append(&detail);
    perf_box.append(&ds::next_launch());
    sec.append(&ds::sfield("Performance", &perf_box, None));
    sec
}

/// What a performance level does, under its control.
fn performance_detail(level: Performance) -> &'static str {
    match level {
        Performance::Low => {
            "10 FPS, the lowest graphics, lower priority, and slower still out of focus. For an \
             account along for the ride."
        }
        Performance::Medium => {
            "60 FPS, reduced graphics, slightly lower priority, and slower out of focus."
        }
        Performance::High => "Your monitor's refresh rate and the game's own graphics.",
        Performance::Max => "Your monitor's refresh rate and the game's top graphics.",
    }
}
