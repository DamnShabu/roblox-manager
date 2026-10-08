//! An account's settings: its label and note, where it launches from, its
//! macro, its session, and removing it. Changes apply as they are made.

use adw::prelude::*;
use gtk::{Align, WrapMode};
use rbxmgr_core::accounts::{Account, SessionState, relative_time};
use rbxmgr_core::cordial::Performance;
use rbxmgr_core::types::{Profile, UserId};

use crate::ui::login::AddAccountDialog;
use crate::ui::widgets::{Btn, Fluent, LabelFluent, avatar, icon, lbl, toggle_class};
use crate::ui::window::Window;

pub struct AccountSettings;

impl AccountSettings {
    pub fn open(w: &Window, id: UserId) {
        let Some(acct) = w.state().accounts.get(id).cloned() else { return };
        let dialog = adw::PreferencesDialog::builder()
            .title("Account Settings")
            .content_width(560)
            .content_height(720)
            .build();
        dialog.add(&page(w, &dialog, &acct));
        dialog.present(Some(w.gtk_window()));
    }
}

/// Everything about one account. Rebuilt in place when the account changes
/// shape (made leader), so its rows are always the ones that apply.
fn page(w: &Window, dialog: &adw::PreferencesDialog, acct: &Account) -> adw::PreferencesPage {
    let id = acct.user_id;
    let page = adw::PreferencesPage::new();
    page.add(&identity(w, acct));
    page.add(&label_and_note(w, acct));
    page.add(&launching(w, dialog, &page, acct));
    page.add(&macro_group(w, acct));
    page.add(&session_group(w, acct));

    let remove = adw::ButtonRow::builder().title("Remove Account…").build();
    remove.add_css_class("destructive-action");
    // Weak: the dialog owns this row, and so this handler.
    let (weak, d) = (w.weak(), dialog.downgrade());
    remove.connect_activated(move |_| {
        if let Some(w) = weak.upgrade() {
            let d = d.clone();
            w.confirm_remove_then(id, move || {
                if let Some(d) = d.upgrade() {
                    d.close();
                }
            });
        }
    });
    let danger = adw::PreferencesGroup::new();
    danger.add(&remove);
    page.add(&danger);
    page
}

/// The account at a glance: its picture, label and Roblox user.
fn identity(w: &Window, acct: &Account) -> adw::PreferencesGroup {
    let picture = w.services().avatars.cached(&acct.user_id.to_string());
    let who = match (&acct.username, &acct.display) {
        (Some(user), Some(display)) if display != user => format!("{display} · @{user}"),
        (Some(user), _) => format!("@{user}"),
        _ => format!("Roblox user {}", acct.user_id),
    };
    let group = adw::PreferencesGroup::new();
    let head = hbox!(
        16,
        "",
        avatar(acct.name.as_str(), picture.as_deref(), 64),
        vbox!(
            2,
            "",
            lbl(acct.name.as_str(), "title-2").ellipsize(),
            lbl(&who, "dimmed").ellipsize()
        )
        .centered()
        .hexpand()
    );
    head.set_margin_bottom(6);
    group.add(&head);
    group
}

fn label_and_note(w: &Window, acct: &Account) -> adw::PreferencesGroup {
    let id = acct.user_id;
    let group = adw::PreferencesGroup::new();
    let label = adw::EntryRow::builder()
        .title("Label")
        .text(acct.name.as_str())
        .show_apply_button(true)
        .build();
    let weak = w.weak();
    label.connect_apply(move |e| {
        if let Some(w) = weak.upgrade() {
            w.rename_account(id, e.text().trim());
        }
    });
    group.add(&label);
    group.add(
        &lbl("Your name for this account. Renaming moves its keyring entry too.", "caption dimmed")
            .wrapped()
            .top(6),
    );

    // accounts.json is not encrypted at rest, so the note is for labels, not
    // secrets.
    let note = gtk::TextView::builder()
        .wrap_mode(WrapMode::WordChar)
        .top_margin(10)
        .bottom_margin(10)
        .left_margin(12)
        .right_margin(12)
        .accepts_tab(false)
        .height_request(72)
        .build();
    note.add_css_class("inline");
    note.buffer().set_text(&acct.note);
    let weak = w.weak();
    note.buffer().connect_changed(move |b| {
        if let Some(w) = weak.upgrade() {
            w.set_note(id, &b.text(&b.start_iter(), &b.end_iter(), false));
        }
    });
    let frame = gtk::Frame::builder().child(&note).build();
    frame.add_css_class("view");
    frame.set_margin_top(12);
    let note_group = vbox!(
        6,
        "",
        lbl("Note", "heading"),
        frame,
        lbl(
            "Shown on the note icon beside its name. Plain text — not for passwords.",
            "caption dimmed"
        )
        .wrapped()
    );
    note_group.set_margin_top(18);
    group.add(&note_group);
    group
}

/// Leader, group, auto-join, and how its client runs.
fn launching(
    w: &Window,
    dialog: &adw::PreferencesDialog,
    page: &adw::PreferencesPage,
    acct: &Account,
) -> adw::PreferencesGroup {
    let id = acct.user_id;
    let (groups, current, leader) = {
        let s = w.state();
        let groups: Vec<(Option<String>, String)> = std::iter::once((None, "Ungrouped".into()))
            .chain(s.accounts.groups().iter().map(|g| {
                let name = if g.name.is_empty() { "Untitled group".into() } else { g.name.clone() };
                (Some(g.id.clone()), name)
            }))
            .collect();
        let current = s.accounts.group_of(acct).map(str::to_owned);
        (groups, current, s.accounts.leader().map(|l| l.name.to_string()))
    };
    let group = adw::PreferencesGroup::builder().title("Launching").build();

    let lead = adw::ActionRow::builder().title("Leader").build();
    if acct.leader {
        lead.set_subtitle("This account launches first; its auto-join accounts follow it");
        lead.add_prefix(&icon("starred-symbolic").css("accent"));
    } else {
        lead.set_subtitle("Launch as Group starts the leader, then the auto-join list");
        let (weak, dialog, page) = (w.weak(), dialog.downgrade(), page.downgrade());
        let make = Btn::new("").text("Make Leader").build(move || {
            let (Some(w), Some(dialog), Some(page)) =
                (weak.upgrade(), dialog.upgrade(), page.upgrade())
            else {
                return;
            };
            w.set_leader(id);
            // Its group and auto-join rows no longer apply: draw them again.
            if let Some(acct) = w.state().accounts.get(id).cloned() {
                dialog.remove(&page);
                dialog.add(&self::page(&w, &dialog, &acct));
            }
        });
        make.button.set_valign(Align::Center);
        lead.add_suffix(&make.button);
    }
    group.add(&lead);

    if !acct.leader {
        let names: Vec<&str> = groups.iter().map(|(_, n)| n.as_str()).collect();
        let combo = adw::ComboRow::builder()
            .title("Group")
            .model(&gtk::StringList::new(&names))
            .selected(groups.iter().position(|(g, _)| *g == current).unwrap_or(0) as u32)
            .build();
        let weak = w.weak();
        combo.connect_selected_notify(move |c| {
            let gid = groups.get(c.selected() as usize).and_then(|(g, _)| g.clone());
            if let Some(w) = weak.upgrade() {
                w.set_group(id, gid);
            }
        });
        group.add(&combo);
        if let Some(leader) = leader {
            let follow = adw::SwitchRow::builder()
                .title("Auto-join the Leader")
                .subtitle(format!("Joins {leader}'s server right after it launches"))
                .active(acct.follow.is_some())
                .build();
            let weak = w.weak();
            follow.connect_active_notify(move |r| {
                if let Some(w) = weak.upgrade() {
                    w.set_follow(id, r.is_active());
                }
            });
            group.add(&follow);
        }
    }

    let levels: Vec<&str> = Performance::ALL.iter().map(|p| p.label()).collect();
    let performance =
        adw::ComboRow::builder()
            .title("Performance")
            .subtitle(performance_detail(acct.performance()))
            .model(&gtk::StringList::new(&levels))
            .selected(
                Performance::ALL.iter().position(|p| *p == acct.performance()).unwrap_or(2) as u32
            )
            .build();
    let weak = w.weak();
    performance.connect_selected_notify(move |c| {
        let Some(&level) = Performance::ALL.get(c.selected() as usize) else { return };
        c.set_subtitle(performance_detail(level));
        if let Some(w) = weak.upgrade() {
            w.set_performance(id, level);
        }
    });
    group.add(&performance);
    group
}

/// What a performance level does, under its row.
fn performance_detail(level: Performance) -> &'static str {
    match level {
        Performance::Low => {
            "10 FPS, the lowest graphics, lower priority, and slower still out of focus. For an \
             account along for the ride. From the next launch."
        }
        Performance::Medium => {
            "60 FPS, reduced graphics, slightly lower priority, and slower out of focus. From the \
             next launch."
        }
        Performance::High => {
            "Your monitor's refresh rate and the game's own graphics. From the next launch."
        }
        Performance::Max => {
            "Your monitor's refresh rate and the game's top graphics. From the next launch."
        }
    }
}

/// Its macro-ready window, the macro its Run plays, and Run itself.
fn macro_group(w: &Window, acct: &Account) -> adw::PreferencesGroup {
    let id = acct.user_id;
    let names: Vec<String> = w.state().macros.names().map(str::to_owned).collect();
    let group = adw::PreferencesGroup::builder()
        .title("Macro")
        .description("A macro plays into a macro-ready window, even while you use other windows.")
        .build();

    let nested = adw::SwitchRow::builder()
        .title("Macro-Ready Window")
        .subtitle(
            "Runs the client on a display of its own, where a macro can reach it. \
             Picking a macro turns it on. From the next launch.",
        )
        .active(acct.nested)
        .build();
    let weak = w.weak();
    nested.connect_active_notify(move |r| {
        if let Some(w) = weak.upgrade() {
            w.set_nested(id, r.is_active());
        }
    });

    let mine = acct.macro_name.clone().filter(|m| names.contains(m));
    let options: Vec<&str> =
        std::iter::once("None").chain(names.iter().map(String::as_str)).collect();
    let combo = adw::ComboRow::builder()
        .title("Macro")
        .model(&gtk::StringList::new(&options))
        .selected(
            mine.as_ref().and_then(|m| names.iter().position(|n| n == m)).map_or(0, |i| i + 1)
                as u32,
        )
        .build();
    let (weak, nested_row) = (w.weak(), nested.clone());
    combo.connect_selected_notify(move |c| {
        let name = (c.selected() as usize).checked_sub(1).and_then(|i| names.get(i)).cloned();
        if let Some(w) = weak.upgrade() {
            let picked = name.is_some();
            w.pick_macro(id, name);
            if picked {
                nested_row.set_active(true);
            }
        }
    });

    let run =
        Btn::new("").text("Run").icon("media-playback-start-symbolic").build(w.act(move |w| {
            w.play_macro_here(id);
            w.refresh_states();
        }));
    run.button.set_valign(Align::Center);
    combo.add_suffix(&run.button);
    group.add(&combo);
    group.add(&nested);

    let run = run.downgrade();
    w.watch_while(Box::new(move |s| {
        let Some(run) = run.upgrade() else { return false };
        let b = &run.button;
        let playing = s.macro_runs.get(&id).map(|(_, m)| m.clone());
        let chosen = s.accounts.get(id).and_then(|a| a.macro_name.clone());
        run.set_text(if playing.is_some() { "Stop" } else { "Run" });
        run.set_icon(if playing.is_some() {
            "media-playback-stop-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
        toggle_class(b, "destructive-action", playing.is_some());
        b.set_sensitive(playing.is_some() || chosen.is_some_and(|m| s.macros.contains(&m)));
        b.set_tooltip_text(Some(&match playing {
            Some(m) => format!("Stop {m} on this account"),
            None => "Play the macro into this account's client".to_owned(),
        }));
        true
    }));
    group
}

/// Whether Roblox still takes its stored session, and the fix when not.
fn session_group(w: &Window, acct: &Account) -> adw::PreferencesGroup {
    let id = acct.user_id;
    let group = adw::PreferencesGroup::builder()
        .title("Session")
        .description(format!(
            "Kept in your keyring. Sign in again when launches fail with “session expired”. \
             Its Cordial profile is {}.",
            Profile::of(id)
        ))
        .build();
    let row = adw::ActionRow::new();
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let status = hbox!(0, "status", dot.clone());
    dot.add_css_class("dot");
    dot.set_valign(Align::Center);
    status.set_valign(Align::Center);
    row.add_prefix(&status);
    let check = Btn::new("").text("Check").build(w.act(move |w| w.check_sessions(vec![id])));
    check.button.set_valign(Align::Center);
    let relogin = acct.clone();
    let sign_in = Btn::new("suggested-action")
        .text("Sign In Again…")
        .build(w.act(move |w| AddAccountDialog::open(w, Some(relogin.clone()))));
    sign_in.button.set_valign(Align::Center);
    row.add_suffix(&check.button);
    row.add_suffix(&sign_in.button);
    group.add(&row);

    // Weak, all of them: the redraw must end with the dialog.
    let (shown, status, check, sign_in) =
        (row.downgrade(), status.downgrade(), check.downgrade(), sign_in.downgrade());
    w.watch_while(Box::new(move |s| {
        let (Some(row), Some(status), Some(check), Some(sign_in)) =
            (shown.upgrade(), status.upgrade(), check.upgrade(), sign_in.upgrade())
        else {
            return false;
        };
        let session = s.accounts.session(id);
        let (title, sub, kind) = match &session {
            SessionState::Checking => ("Checking…", String::new(), "starting"),
            SessionState::Expired => {
                ("Expired", "Roblox no longer takes this session".to_owned(), "expired")
            }
            SessionState::Ok { checked: Some(when) } => (
                "Signed in",
                format!("Roblox took it {}", relative_time(Some(when), chrono::Utc::now())),
                "running",
            ),
            SessionState::Ok { checked: None } => ("Signed in", String::new(), "running"),
        };
        row.set_title(title);
        row.set_subtitle(&sub);
        for k in ["starting", "expired", "running"] {
            toggle_class(&status, k, k == kind);
        }
        check.button.set_sensitive(session != SessionState::Checking);
        check.button.set_visible(session != SessionState::Expired);
        sign_in.button.set_visible(session == SessionState::Expired);
        true
    }));
    group
}
