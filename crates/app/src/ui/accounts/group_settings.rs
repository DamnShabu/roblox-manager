//! A group in the inspector: its name (edited in its head), stopping or
//! selecting it, the game its Launch opens, its members, and deleting it.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType};
use rbxmgr_core::types::{PlaceId, UserId};

use crate::state::{Chip, Inspected, Tile};
use crate::ui::accounts::settings::AccountSettings;
use crate::ui::ds::{self, Tone, Variant};
use crate::ui::panel::Panel;
use crate::ui::table::row::confirm_row;
use crate::ui::widgets::{Fluent, LabelFluent, clear, lbl, plural, toggle_class};
use crate::ui::window::Window;

pub struct GroupSettings;

/// A game a group can launch into, as the list shows it.
struct Choice {
    /// None is no game.
    game: Option<(PlaceId, String)>,
    label: String,
    icon: Option<std::path::PathBuf>,
}

impl GroupSettings {
    pub fn open(w: &Window, gid: &str) {
        let Some(group) = w.state().accounts.groups().iter().find(|g| g.id == gid).cloned() else {
            return;
        };
        let games: Vec<Tile> = w.state().game_list.clone();
        let members: Vec<(UserId, String, Option<u32>)> = w
            .state()
            .accounts
            .group_members(gid)
            .iter()
            .map(|a| (a.user_id, a.name.to_string(), a.follow))
            .collect();
        let ids: Vec<UserId> = members.iter().map(|m| m.0).collect();
        let panel = Panel::new("Group");

        // -- head: art, the name to edit, members and how many run ----------------
        let art = match group
            .place_id
            .as_ref()
            .and_then(|p| games.iter().find(|t| &t.game.place_id == p))
        {
            Some(t) => ds::art(&t.game.name, t.icon.as_deref(), 36),
            None => ds::art_icon("gamepad", 36),
        };
        let name = ds::input(&group.name);
        name.set_placeholder_text(Some("Group name"));
        let (weak, id) = (w.weak(), gid.to_owned());
        name.connect_changed(move |e| {
            if let Some(w) = weak.upgrade() {
                w.rename_group(&id, e.text().trim());
            }
        });
        let sub = gtk::Box::new(Orientation::Horizontal, 4);
        let head =
            ds::head(&art, name.upcast_ref(), sub.upcast_ref(), &[panel.close_button().upcast()]);

        // -- stop it, select its accounts -----------------------------------------------
        let stop = {
            let id = gid.to_owned();
            ds::Button::new("Stop group", Variant::Secondary, true)
                .on(w.act(move |w| w.stop_group(&id)))
        };
        let select = {
            let id = gid.to_owned();
            ds::Button::new("Select its accounts", Variant::Ghost, true)
                .on(w.act(move |w| w.select_group(&id)))
        };
        let acts = gtk::Box::new(Orientation::Horizontal, 8).css("cx-sec");
        acts.add_css_class("first");
        acts.append(&stop.button);
        acts.append(&select.button);

        // -- its game ---------------------------------------------------------------------
        let reload = ds::ib("refresh", "Reload everyone's favourites", true);
        reload.set_action_name(Some("win.reload-games"));
        let game = ds::sec("Game", Some(reload.upcast_ref()));
        game.append(
            &lbl(
                "Launch group, here or on the group's band, sends every account here into this game.",
                "cx-help",
            )
            .wrapped(),
        );
        let filter = gtk::SearchEntry::builder()
            .placeholder_text("Filter favourites")
            .css_classes(["search-field"])
            .build();
        game.append(&filter);
        let mut options = vec![Choice { game: None, label: "No game".to_owned(), icon: None }];
        options.extend(games.iter().map(|t| Choice {
            game: Some((t.game.place_id.clone(), t.game.name.clone())),
            label: t.game.name.clone(),
            icon: t.icon.clone(),
        }));
        // A game no longer among the favourites stays on offer while chosen.
        if let Some(place) =
            group.place_id.as_ref().filter(|p| !games.iter().any(|t| &t.game.place_id == *p))
        {
            let shown = group.game.clone().unwrap_or_else(|| format!("Place {place}"));
            options.push(Choice {
                game: Some((place.clone(), shown.clone())),
                label: shown,
                icon: None,
            });
        }
        let list = gtk::Box::new(Orientation::Vertical, 0).css("cx-list");
        let mut first: Option<gtk::CheckButton> = None;
        let mut rows: Vec<(gtk::Box, String)> = Vec::new();
        for Choice { game: choice, label, icon } in options {
            let chosen = choice.as_ref().map(|(p, _)| p) == group.place_id.as_ref();
            let radio = gtk::CheckButton::builder().active(chosen).valign(Align::Center).build();
            match &first {
                Some(f) => radio.set_group(Some(f)),
                None => first = Some(radio.clone()),
            }
            let row = gtk::Box::new(Orientation::Horizontal, 12).css("cx-row");
            toggle_class(&row, "current", chosen);
            row.append(&radio);
            row.append(&match &choice {
                Some(_) => ds::art(&label, icon.as_deref(), 24),
                None => ds::art_icon("x", 24),
            });
            row.append(&lbl(&label, "cx-row-t").ellipsize().hexpand());
            let tag = lbl("Chosen", "badge accent").visible(chosen);
            row.append(&tag);
            let click = gtk::GestureClick::new();
            {
                let radio = radio.clone();
                click.connect_released(move |_, _, _, _| radio.set_active(true));
            }
            row.add_controller(click);
            let (weak, id, r, t) = (w.weak(), gid.to_owned(), row.downgrade(), tag.downgrade());
            radio.connect_toggled(move |b| {
                if let Some(r) = r.upgrade() {
                    toggle_class(&r, "current", b.is_active());
                }
                if let Some(t) = t.upgrade() {
                    t.set_visible(b.is_active());
                }
                if let (true, Some(w)) = (b.is_active(), weak.upgrade()) {
                    w.set_group_game(&id, choice.clone());
                }
            });
            rows.push((row.clone(), label.to_lowercase()));
            list.append(&row);
        }
        let rows = Rc::new(rows);
        filter.connect_search_changed(move |e| {
            let q = e.text().trim().to_lowercase();
            for (row, label) in rows.iter() {
                row.set_visible(q.is_empty() || label.contains(&q));
            }
        });
        game.append(&list);

        // -- its members ----------------------------------------------------------------
        let count = lbl(&members.len().to_string(), "cx-count");
        let mem = ds::sec("Members", Some(count.upcast_ref()));
        let mlist = gtk::Box::new(Orientation::Vertical, 0).css("cx-list");
        let mut states = Vec::new();
        for (id, label, follow) in &members {
            let id = *id;
            let row = gtk::Box::new(Orientation::Horizontal, 8).css("cx-row");
            let pic = w.services().avatars.cached(&id.to_string());
            row.append(&ds::av(label, pic.as_deref(), 24));
            row.append(&lbl(label, "cx-row-t").ellipsize().hexpand());
            let dot = gtk::Box::new(Orientation::Horizontal, 0).css("cx-dot");
            dot.set_valign(Align::Center);
            let state = lbl("", "t-body-sm");
            row.append(&dot);
            row.append(&state);
            let role = follow.map_or_else(|| "Solo".to_owned(), |n| format!("Auto-join #{n}"));
            row.append(&lbl(&role, "t-caption muted"));
            let open = ds::ib("arrow-right", &format!("Open {label}"), true);
            open.connect_clicked({
                let w = w.weak();
                move |_| {
                    if let Some(w) = w.upgrade() {
                        AccountSettings::open(&w, id);
                    }
                }
            });
            row.append(&open);
            mlist.append(&row);
            states.push((id, dot.downgrade(), state.downgrade()));
        }
        if !members.is_empty() {
            mem.append(&mlist);
        }
        let hint = gtk::Box::new(Orientation::Horizontal, 6);
        hint.append(&ds::icon("grip").css("muted s14"));
        hint.append(&lbl("Drag accounts onto the group band to add them.", "t-caption muted"));
        mem.append(&hint);

        // -- deleting it -------------------------------------------------------------------
        let delete =
            ds::Button::with_icons("Delete group…", Variant::Ghost, false, Some("trash"), None);
        delete.button.set_halign(Align::Start);
        let n = members.len();
        let body = match n {
            0 => "It has no accounts.".to_owned(),
            1 => "Its account moves to Ungrouped; it is not removed.".to_owned(),
            n => format!("Its {n} accounts move to Ungrouped; none is removed."),
        };
        let shown_name =
            if group.name.is_empty() { "Untitled group".to_owned() } else { group.name.clone() };
        let confirm = {
            let (weak, id, p) = (w.weak(), gid.to_owned(), panel.downgrade());
            confirm_row(&format!("Delete {shown_name}?"), &body, "Delete", move || {
                if let Some(p) = p.upgrade() {
                    p.close();
                }
                if let Some(w) = weak.upgrade() {
                    w.delete_group(&id);
                }
            })
        };
        {
            let c = confirm.clone();
            delete.button.connect_clicked(move |_| c.set_reveal_child(true));
        }
        let danger = ds::sec("", None);
        danger.append(&delete.button);
        danger.append(&confirm);

        let content = gtk::Box::new(Orientation::Vertical, 0);
        content.append(&acts);
        content.append(&game);
        content.append(&mem);
        content.append(&danger);
        let root = gtk::Box::new(Orientation::Vertical, 0);
        root.add_css_class("cx-panel");
        root.append(&head);
        root.append(
            &gtk::ScrolledWindow::builder()
                .child(&content)
                .hscrollbar_policy(PolicyType::Never)
                .vexpand(true)
                .build(),
        );
        panel.set_child(Some(&root));

        let (sub_w, stop_w) = (sub.downgrade(), stop.button.downgrade());
        w.watch_while(Box::new(move |s| {
            let (Some(sub), Some(stop)) = (sub_w.upgrade(), stop_w.upgrade()) else { return false };
            clear(&sub);
            sub.append(&lbl(&plural(ids.len(), "account", "accounts"), "cx-head-sub"));
            let up = ids.iter().filter(|id| s.running.contains(id)).count();
            if up > 0 {
                sub.append(&lbl("·", "cx-head-sub"));
                sub.append(&ds::badge(Tone::Success, &format!("{up} running"), true, false));
            }
            stop.set_visible(s.any_live(&ids));
            for (id, dot, state) in &states {
                let (Some(dot), Some(state)) = (dot.upgrade(), state.upgrade()) else { continue };
                let (text, kind) = match s.chip(*id) {
                    Chip::Running => ("Running", "run"),
                    Chip::Starting | Chip::Joining => ("Starting", "start"),
                    Chip::Expired => ("Expired", "bad"),
                    Chip::Idle => ("Idle", ""),
                };
                state.set_label(text);
                for k in ["run", "start", "bad"] {
                    toggle_class(&dot, k, k == kind);
                }
            }
            true
        }));

        // The band shows the name: drawn again once it is settled.
        let weak = w.weak();
        panel.connect_closed(move |_| {
            if let Some(w) = weak.upgrade() {
                w.refresh_accounts();
                w.show_games();
            }
        });
        panel.present(w);
        w.inspect(Some(Inspected::Group(gid.to_owned())));
        if group.name == "New group" {
            name.grab_focus();
        }
    }
}
