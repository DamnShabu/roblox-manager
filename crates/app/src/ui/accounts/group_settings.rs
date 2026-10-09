//! A group's settings: its name, the game its Launch opens, deleting it.

use adw::prelude::*;
use gtk::Align;
use rbxmgr_core::types::PlaceId;

use crate::state::Tile;
use crate::ui::panel::Panel;
use crate::ui::widgets::thumb;
use crate::ui::window::Window;

pub struct GroupSettings;

/// A game a group can launch into, as its settings list it.
struct Choice<'a> {
    /// None is no game.
    game: Option<(PlaceId, String)>,
    label: String,
    icon: Option<&'a std::path::Path>,
}

impl GroupSettings {
    pub fn open(w: &Window, gid: &str) {
        let Some(group) = w.state().accounts.groups().iter().find(|g| g.id == gid).cloned() else {
            return;
        };
        let games: Vec<Tile> = w.state().game_list.clone();
        let dialog = Panel::new("Group Settings");
        let page = adw::PreferencesPage::new();

        let about = adw::PreferencesGroup::new();
        let name = adw::EntryRow::builder().title("Name").text(&group.name).build();
        let (weak, id) = (w.weak(), gid.to_owned());
        name.connect_changed(move |e| {
            if let Some(w) = weak.upgrade() {
                w.rename_group(&id, e.text().trim());
            }
        });
        about.add(&name);
        page.add(&about);

        let game = adw::PreferencesGroup::builder()
            .title("Game")
            .description("Launch on the group's header sends every account here into this game.")
            .build();
        let mut options = vec![Choice { game: None, label: "No game".to_owned(), icon: None }];
        options.extend(games.iter().map(|t| Choice {
            game: Some((t.game.place_id.clone(), t.game.name.clone())),
            label: t.game.name.clone(),
            icon: t.icon.as_deref(),
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
        let mut first: Option<gtk::CheckButton> = None;
        for Choice { game: choice, label, icon } in options {
            let chosen = choice.as_ref().map(|(p, _)| p) == group.place_id.as_ref();
            let radio = gtk::CheckButton::builder().active(chosen).valign(Align::Center).build();
            match &first {
                Some(f) => radio.set_group(Some(f)),
                None => first = Some(radio.clone()),
            }
            let row = adw::ActionRow::builder()
                .title(&label)
                .use_markup(false)
                .activatable_widget(&radio)
                .build();
            // A prefix goes in front of those already there.
            if choice.is_some() {
                row.add_prefix(&thumb(icon, 32, "thumb small"));
            }
            row.add_prefix(&radio);
            let (weak, id) = (w.weak(), gid.to_owned());
            radio.connect_toggled(move |r| {
                if let (true, Some(w)) = (r.is_active(), weak.upgrade()) {
                    w.set_group_game(&id, choice.clone());
                }
            });
            game.add(&row);
        }
        page.add(&game);

        let delete = adw::ButtonRow::builder().title("Delete Group…").build();
        delete.add_css_class("destructive-action");
        // Weak: the dialog owns this row, and so this handler.
        let (weak, id, d) = (w.weak(), gid.to_owned(), dialog.downgrade());
        delete.connect_activated(move |_| {
            if let Some(w) = weak.upgrade() {
                let d = d.clone();
                w.confirm_delete_group_then(&id, move || {
                    if let Some(d) = d.upgrade() {
                        d.close();
                    }
                });
            }
        });
        let danger = adw::PreferencesGroup::new();
        danger.add(&delete);
        page.add(&danger);
        dialog.set_page(&page);

        // The header shows the name: drawn again once it is settled.
        let weak = w.weak();
        dialog.connect_closed(move |_| {
            if let Some(w) = weak.upgrade() {
                w.refresh_accounts();
                w.show_games();
            }
        });
        dialog.present(w);
        if group.name == "New group" {
            name.grab_focus();
        }
    }
}
