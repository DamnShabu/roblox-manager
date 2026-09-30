//! The popup a join link from the browser opens: the game it names, and the
//! accounts to join with. Nothing starts until Join; one account joins as
//! itself, several share one server -- the link's, or the first one's.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, PolicyType, glib};
use rbxmgr_core::launch::Mode;
use rbxmgr_core::roblox::{JoinLink, PlaceDetails, Roblox};
use rbxmgr_core::types::{Label, UserId};

use super::widgets::{Btn, Fluent, IconButton, LabelFluent, avatar, boxed_list, lbl, thumb};
use super::window::{WeakWindow, Window};
use crate::state::{AppState, Chip};
use crate::worker;

/// The game's picture, in px.
const ART: i32 = 64;

/// "950", "18.2k", "1.4M": a player count in the room of a line.
pub fn players(n: u64) -> String {
    let short = |x: f64, unit: &str| {
        let s = format!("{x:.1}");
        format!("{}{unit}", s.strip_suffix(".0").unwrap_or(&s))
    };
    match n {
        0..1_000 => n.to_string(),
        1_000..999_950 => short(n as f64 / 1e3, "k"),
        _ => short(n as f64 / 1e6, "M"),
    }
}

/// What an account's line says it is doing, and its CSS class.
fn doing(s: &AppState, id: UserId, link_game: Option<(&str, &str)>) -> (String, &'static str) {
    match s.chip(id) {
        Chip::Running => {
            let place = s.playing.get(&id);
            let game = place.and_then(|p| {
                s.game_name(p).or(link_game.filter(|(lp, _)| *lp == p.as_str()).map(|(_, n)| n))
            });
            match game {
                Some(g) => (format!("in game · {g}"), "in-game"),
                None => ("in game".to_owned(), "in-game"),
            }
        }
        Chip::Joining => ("joining…".to_owned(), "in-game"),
        Chip::Starting => ("starting…".to_owned(), "in-game"),
        Chip::Expired => ("signed out".to_owned(), "expired"),
        Chip::Idle => ("idle".to_owned(), "dimmed"),
    }
}

pub struct LinkPopup {
    window: WeakWindow,
    popup: adw::Window,
    link: JoinLink,
    /// The accounts on offer, as drawn, with their tick boxes.
    rows: Vec<(UserId, Label, gtk::CheckButton)>,
    /// The game's name, once Roblox has said.
    game: RefCell<Option<String>>,
    count: gtk::Label,
    select_all: IconButton,
    join: IconButton,
    remember: gtk::CheckButton,
    /// Whether accounts were remembered before: unticking forgets them.
    remembered: bool,
}

impl LinkPopup {
    /// Build and show the popup; its window, for the caller to keep track of.
    pub fn open(w: &Window, link: JoinLink) -> adw::Window {
        let (accounts, picked) = {
            let s = w.state();
            let a = &s.accounts;
            let accounts: Vec<(UserId, Label)> = a
                .leader()
                .into_iter()
                .chain(a.visual_order())
                .map(|x| (x.user_id, x.name.clone()))
                .collect();
            let mut picked = a.link_accounts();
            if picked.is_empty() {
                picked.extend(accounts.first().map(|(id, _)| *id));
            }
            (accounts, picked)
        };
        let remembered = !w.state().accounts.link_accounts().is_empty();
        let popup =
            adw::Window::builder().title("Join Game").default_width(500).resizable(false).build();
        popup.add_css_class("link-popup");
        // Its own window of the app's: it keeps the app up while it is open.
        popup.set_application(w.gtk_window().application().as_ref());
        if let Some(parent) = w.popup_parent() {
            popup.set_transient_for(Some(parent));
            popup.set_modal(true);
        }
        let d = Rc::new(LinkPopup {
            window: w.weak(),
            popup,
            link,
            rows: accounts
                .into_iter()
                .map(|(id, label)| {
                    let check = gtk::CheckButton::new();
                    check.set_active(picked.contains(&id));
                    check.set_valign(Align::Center);
                    (id, label, check)
                })
                .collect(),
            game: RefCell::default(),
            count: lbl("", "caption dimmed"),
            select_all: Btn::new("flat").text("Select all").build(|| {}),
            join: Btn::new("suggested-action pill")
                .icon("media-playback-start-symbolic")
                .text("Join")
                .build(|| {}),
            remember: gtk::CheckButton::with_label("Remember these accounts for future links"),
            remembered,
        });
        d.remember.set_active(remembered);
        d.assemble(w);
        d.update();
        let held = RefCell::new(Some(d.clone()));
        let window = w.weak();
        d.popup.connect_close_request(move |_| {
            held.take();
            if let Some(w) = window.upgrade() {
                w.link_popup_closed();
            }
            glib::Propagation::Proceed
        });
        d.popup.present();
        d.popup.clone()
    }

    fn assemble(self: &Rc<Self>, w: &Window) {
        // -- the header: whose popup, and why it is here -----------------------
        let app_icon = gtk::Image::from_icon_name(crate::APP_ID);
        app_icon.set_pixel_size(28);
        let heading = hbox!(
            10,
            "",
            app_icon,
            vbox!(
                0,
                "",
                lbl("Roblox Manager", "heading"),
                lbl("Link opened from browser", "caption dimmed")
            )
            .centered()
        );
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
        header.pack_start(&heading);

        // -- the game ------------------------------------------------------------
        let kind = match self.link.server {
            Some(_) => hbox!(
                4,
                "tag",
                gtk::Image::from_icon_name("network-server-symbolic"),
                lbl("Server", "")
            ),
            None => hbox!(
                4,
                "tag",
                gtk::Image::from_icon_name("input-gaming-symbolic"),
                lbl("Game", "")
            ),
        };
        kind.set_halign(Align::Start);
        let known = w.state().game_name(&self.link.place).map(str::to_owned);
        let name = lbl(
            &known.clone().unwrap_or_else(|| format!("Place {}", self.link.place)),
            "game-name",
        )
        .ellipsize();
        let meta = lbl("Loading…", "caption dimmed").ellipsize();
        let art = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        art.append(&thumb(self.cached_icon(w).as_deref(), ART, "thumb"));
        let card = hbox!(
            14,
            "card game-card",
            art.clone(),
            vbox!(4, "", kind, name.clone(), meta.clone()).hexpand().centered()
        );
        self.game.replace(known);

        // -- the accounts --------------------------------------------------------
        let list = boxed_list();
        list.add_css_class("link-accounts");
        for (id, label, check) in &self.rows {
            list.append(&self.account_row(w, *id, label, check));
        }
        let me = Rc::downgrade(self);
        list.connect_row_activated(move |_, row| {
            let Some(d) = me.upgrade() else { return };
            if let Some((_, _, check)) =
                usize::try_from(row.index()).ok().and_then(|i| d.rows.get(i))
            {
                check.set_active(!check.is_active());
            }
        });
        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(300)
            .build();
        let me = Rc::downgrade(self);
        self.select_all.button.connect_clicked(move |_| {
            if let Some(d) = me.upgrade() {
                let every = d.rows.iter().all(|(_, _, c)| c.is_active());
                for (_, _, c) in &d.rows {
                    c.set_active(!every);
                }
            }
        });
        self.select_all.button.set_valign(Align::Center);
        let accounts_head = hbox!(
            8,
            "",
            vbox!(2, "", lbl("Join with", "heading"), self.count.clone()).hexpand().centered(),
            self.select_all.button.clone()
        );

        // -- the buttons ---------------------------------------------------------
        let popup = self.popup.downgrade();
        let close = move || {
            if let Some(p) = popup.upgrade() {
                p.close();
            }
        };
        let cancel = Btn::new("flat").text("_Cancel").build(close.clone());
        let me = Rc::downgrade(self);
        self.join.button.connect_clicked(move |_| {
            if let Some(d) = me.upgrade() {
                d.join();
            }
        });
        cancel.button.set_halign(Align::Start);
        let footer = hbox!(
            8,
            "link-footer",
            cancel.button.hexpand().valign(Align::Center),
            self.join.button.clone()
        );

        let body =
            vbox!(12, "", card, accounts_head.top(6), scroller, self.remember.clone()).margins(18);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&body));
        view.add_bottom_bar(&footer);
        self.popup.set_content(Some(&view));
        self.popup.set_default_widget(Some(&self.join.button));

        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.popup.add_controller(keys);

        let me = Rc::downgrade(self);
        let (art, name, meta) = (art.downgrade(), name.downgrade(), meta.downgrade());
        self.on_details(move |got, icon| {
            let (Some(d), Some(art), Some(name), Some(meta)) =
                (me.upgrade(), art.upgrade(), name.upgrade(), meta.upgrade())
            else {
                return;
            };
            match got {
                Ok(p) => {
                    name.set_label(&p.name);
                    let by = (!p.creator.is_empty()).then(|| format!("by {}", p.creator));
                    let playing = format!("{} playing", players(p.playing));
                    meta.set_label(
                        &by.into_iter().chain([playing]).collect::<Vec<_>>().join(" · "),
                    );
                    d.game.replace(Some(p.name));
                }
                Err(e) => meta.set_label(&format!("Could not load the game: {e}")),
            }
            if icon.is_some() {
                while let Some(c) = art.first_child() {
                    art.remove(&c);
                }
                art.append(&thumb(icon.as_deref(), ART, "thumb"));
            }
            // An account in this game now says so by name.
            if let Some(w) = d.window.upgrade() {
                w.refresh_states();
            }
        });
    }

    fn account_row(
        self: &Rc<Self>,
        w: &Window,
        id: UserId,
        label: &Label,
        check: &gtk::CheckButton,
    ) -> gtk::ListBoxRow {
        let username = w.state().accounts.get(id).and_then(|a| a.username.clone());
        let picture = w.services().avatars.cached(&id.to_string());
        let doing_label = lbl("", "caption").ellipsize();
        let line = hbox!(0, "");
        if let Some(u) = username {
            line.append(&lbl(&format!("@{u} · "), "caption dimmed").ellipsize());
        }
        line.append(&doing_label);
        let row = gtk::ListBoxRow::builder().activatable(true).selectable(false).build();
        row.add_css_class("link-account");
        row.set_child(Some(&hbox!(
            12,
            "",
            check.clone(),
            avatar(label.as_str(), picture.as_deref(), 32),
            vbox!(2, "", lbl(label.as_str(), "heading").ellipsize(), line).hexpand().centered()
        )));
        let me = Rc::downgrade(self);
        check.connect_toggled(move |_| {
            if let Some(d) = me.upgrade() {
                d.update();
            }
        });
        let me = Rc::downgrade(self);
        let shown = doing_label.downgrade();
        w.watch_while(Box::new(move |s| {
            let (Some(d), Some(l)) = (me.upgrade(), shown.upgrade()) else { return false };
            let game = d.game.borrow();
            let link_game = game.as_deref().map(|g| (d.link.place.as_str(), g));
            let (text, class) = doing(s, id, link_game);
            l.set_label(&text);
            l.set_css_classes(&["caption", class]);
            true
        }));
        row
    }

    /// The accounts ticked, in the order drawn.
    fn picked(&self) -> Vec<UserId> {
        self.rows.iter().filter(|(_, _, c)| c.is_active()).map(|(id, _, _)| *id).collect()
    }

    /// The count, Select all and Join, as the ticks are.
    fn update(&self) {
        let picked = self.picked();
        self.count.set_label(&format!("{} selected", picked.len()));
        let every = !self.rows.is_empty() && picked.len() == self.rows.len();
        self.select_all.set_text(if every { "Select none" } else { "Select all" });
        let first = self.rows.iter().find(|(id, _, _)| picked.first() == Some(id));
        self.join.set_text(&match (picked.len(), first) {
            (0, _) | (_, None) => "Select an account".to_owned(),
            (1, Some((_, label, _))) => format!("Join as {label}"),
            (n, _) => format!("Join with {n} accounts"),
        });
        self.join.button.set_sensitive(!picked.is_empty());
    }

    fn join(&self) {
        let ids = self.picked();
        let Some(w) = self.window.upgrade() else { return };
        if ids.is_empty() {
            return;
        }
        let remember = self.remember.is_active();
        if remember || self.remembered {
            w.state_mut().accounts.set_link_accounts(if remember { &ids[..] } else { &[] });
            w.save_accounts();
        }
        let JoinLink { place, server } = self.link.clone();
        let game = self.game.borrow().clone().unwrap_or_else(|| format!("place {place}"));
        // Several into a game share a server: the first leads, the rest follow.
        let mode = if server.is_none() && ids.len() > 1 { Mode::Group } else { Mode::Each };
        w.log(&format!("Joining {game} from a link"));
        w.launch(ids, mode, Some((Some(place), server)));
        self.popup.close();
    }

    fn cached_icon(&self, w: &Window) -> Option<std::path::PathBuf> {
        let s = w.state();
        let universe = s
            .game_list
            .iter()
            .find(|t| t.game.place_id == self.link.place)
            .map(|t| t.game.universe_id.clone())?;
        w.services().icons.cached(&universe)
    }

    /// Ask Roblox about the link's game and fetch its icon, off the main
    /// loop; `done` gets what came back.
    fn on_details(
        &self,
        done: impl FnOnce(Result<PlaceDetails, String>, Option<std::path::PathBuf>) + 'static,
    ) {
        let Some(w) = self.window.upgrade() else { return };
        let (roblox, icons, log) =
            (w.services().roblox.clone(), w.services().icons.clone(), w.logger());
        let place = self.link.place.clone();
        worker::run(
            move || {
                let details = roblox.place_details(&place).map_err(|e| e.to_string())?;
                let universe = details.universe_id.clone();
                let icon = icons.cached(&universe).or_else(|| {
                    let urls = roblox.icon_urls(std::slice::from_ref(&universe));
                    let url = urls
                        .map_err(|e| log.line(format!("The game's icon did not load: {e}")))
                        .ok()?
                        .remove(&universe)?;
                    icons
                        .fetch(roblox.transport(), &universe, &url)
                        .map_err(|e| log.line(format!("The game's icon did not load: {e}")))
                        .ok()
                });
                Ok((details, icon))
            },
            move |got: Result<(PlaceDetails, Option<std::path::PathBuf>), String>| match got {
                Ok((details, icon)) => done(Ok(details), icon),
                Err(e) => done(Err(e), None),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_counts_are_shortened() {
        assert_eq!(players(0), "0");
        assert_eq!(players(950), "950");
        assert_eq!(players(1_000), "1k");
        assert_eq!(players(18_204), "18.2k");
        assert_eq!(players(999_949), "999.9k");
        assert_eq!(players(999_950), "1M");
        assert_eq!(players(1_430_000), "1.4M");
    }
}
