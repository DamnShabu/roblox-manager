//! Small builders: the window is a lot of labelled boxes, and GTK's
//! constructors spell each one out at length.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use adw::prelude::*;
use gtk::{Align, Label, Orientation, gdk, pango};

/// A box of `children`, in a direction, with CSS classes.
macro_rules! boxed {
    ($dir:expr, $spacing:expr, $css:expr $(, $child:expr)* $(,)?) => {{
        let b = $crate::ui::widgets::new_box($dir, $spacing, $css);
        $( b.append(&$child); )*
        b
    }};
}

/// A horizontal box: `hbox!(spacing, "css classes", children...)`.
macro_rules! hbox {
    ($spacing:expr, $css:expr $(, $child:expr)* $(,)?) => {
        boxed!(gtk::Orientation::Horizontal, $spacing, $css $(, $child)*)
    };
}

/// A vertical box: `vbox!(spacing, "css classes", children...)`.
macro_rules! vbox {
    ($spacing:expr, $css:expr $(, $child:expr)* $(,)?) => {
        boxed!(gtk::Orientation::Vertical, $spacing, $css $(, $child)*)
    };
}

pub fn new_box(dir: Orientation, spacing: i32, css: &str) -> gtk::Box {
    let b = gtk::Box::new(dir, spacing);
    classes(&b, css);
    b
}

pub fn classes(w: &impl IsA<gtk::Widget>, css: &str) {
    for c in css.split_whitespace() {
        w.add_css_class(c);
    }
}

/// Add or remove one CSS class.
pub fn toggle_class(w: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        w.add_css_class(class);
    } else {
        w.remove_css_class(class);
    }
}

/// Setters that hand the widget back, so a widget is built in one expression.
pub trait Fluent: IsA<gtk::Widget> + Sized {
    fn valign(self, a: Align) -> Self {
        self.set_valign(a);
        self
    }
    fn centered(self) -> Self {
        self.valign(Align::Center)
    }
    fn hexpand(self) -> Self {
        self.set_hexpand(true);
        self
    }
    fn tip(self, text: &str) -> Self {
        self.set_tooltip_text(Some(text));
        self
    }
    fn width(self, px: i32) -> Self {
        self.set_width_request(px);
        self
    }
    fn top(self, px: i32) -> Self {
        self.set_margin_top(px);
        self
    }
    fn margins(self, px: i32) -> Self {
        self.set_margin_top(px);
        self.set_margin_bottom(px);
        self.set_margin_start(px);
        self.set_margin_end(px);
        self
    }
    fn visible(self, on: bool) -> Self {
        self.set_visible(on);
        self
    }
    fn css(self, css: &str) -> Self {
        classes(&self, css);
        self
    }
}

impl<T: IsA<gtk::Widget>> Fluent for T {}

/// A label, left-aligned.
pub fn lbl(text: &str, css: &str) -> Label {
    let l = Label::new(Some(text));
    l.set_xalign(0.0);
    classes(&l, css);
    l
}

/// Label options that hand the label back.
pub trait LabelFluent {
    fn ellipsize(self) -> Self;
    fn wrapped(self) -> Self;
    fn chars(self, n: i32) -> Self;
    fn xalign(self, x: f32) -> Self;
    fn selectable(self) -> Self;
}

impl LabelFluent for Label {
    fn ellipsize(self) -> Self {
        self.set_ellipsize(pango::EllipsizeMode::End);
        self
    }
    fn wrapped(self) -> Self {
        self.set_wrap(true);
        self.set_wrap_mode(pango::WrapMode::WordChar);
        self
    }
    fn chars(self, n: i32) -> Self {
        self.set_max_width_chars(n);
        self
    }
    fn xalign(self, x: f32) -> Self {
        self.set_xalign(x);
        self
    }
    fn selectable(self) -> Self {
        self.set_selectable(true);
        self
    }
}

/// A symbolic icon from the icon theme, at the theme's 16 px.
pub fn icon(name: &str) -> gtk::Image {
    gtk::Image::from_icon_name(name)
}

/// A button of an optional icon and optional label, kept for relabelling.
#[derive(Clone)]
pub struct IconButton {
    pub button: gtk::Button,
    /// Set when the button shows both an icon and a label.
    content: Option<adw::ButtonContent>,
}

impl IconButton {
    pub fn set_icon(&self, name: &str) {
        match &self.content {
            Some(c) => c.set_icon_name(name),
            None => self.button.set_icon_name(name),
        }
    }

    pub fn set_text(&self, text: &str) {
        match &self.content {
            Some(c) => c.set_label(text),
            None => self.button.set_label(text),
        }
    }
}

/// Builds an [`IconButton`]: `Btn::new("flat").text("Launch").icon("media-playback-start-symbolic")`.
pub struct Btn {
    css: String,
    text: Option<String>,
    icon: Option<String>,
    tip: Option<String>,
}

impl Btn {
    pub fn new(css: &str) -> Self {
        Btn { css: css.to_owned(), text: None, icon: None, tip: None }
    }
    pub fn text(mut self, t: &str) -> Self {
        self.text = Some(t.to_owned());
        self
    }
    pub fn icon(mut self, name: &str) -> Self {
        self.icon = Some(name.to_owned());
        self
    }
    pub fn tip(mut self, t: &str) -> Self {
        self.tip = Some(t.to_owned());
        self
    }

    pub fn build(self, on_click: impl Fn() + 'static) -> IconButton {
        let button = gtk::Button::new();
        classes(&button, &self.css);
        button.set_tooltip_text(self.tip.as_deref());
        let content = match (self.icon, self.text) {
            (Some(icon), Some(text)) => {
                let c = adw::ButtonContent::builder()
                    .icon_name(icon)
                    .label(text)
                    .use_underline(true)
                    .build();
                button.set_child(Some(&c));
                Some(c)
            }
            (Some(icon), None) => {
                button.set_icon_name(&icon);
                None
            }
            (None, Some(text)) => {
                button.set_label(&text);
                button.set_use_underline(true);
                None
            }
            (None, None) => None,
        };
        button.connect_clicked(move |_| on_click());
        IconButton { button, content }
    }
}

pub fn switch(active: bool, tip: Option<&str>, on_change: impl Fn(bool) + 'static) -> gtk::Switch {
    let sw = gtk::Switch::new();
    sw.set_active(active);
    sw.set_valign(Align::Center);
    sw.set_tooltip_text(tip);
    sw.connect_active_notify(move |s| on_change(s.is_active()));
    sw
}

/// Children that wrap onto new lines.
pub fn wrap(spacing: i32, children: &[gtk::Widget]) -> adw::WrapBox {
    let b = adw::WrapBox::new();
    b.set_child_spacing(spacing);
    b.set_line_spacing(spacing);
    for c in children {
        b.append(c);
    }
    b
}

/// A state in a pill: a dot and a word, coloured by `kind` (running,
/// starting, joining, expired, failed, idle). A live state's dot pulses.
pub fn status(kind: &str, text: &str, live: bool) -> gtk::Box {
    let dot = new_box(Orientation::Horizontal, 0, if live { "dot live" } else { "dot" });
    dot.set_valign(Align::Center);
    hbox!(6, &format!("status {kind}"), dot, lbl(text, "")).centered()
}

/// A hotkey as keycaps: "Ctrl" "F7".
pub fn keycaps(accel: &str) -> gtk::Box {
    let b = hbox!(3, "").centered();
    for key in hotkey_label(Some(accel)).split('+').filter(|k| !k.is_empty()) {
        b.append(&lbl(key, "keycap").xalign(0.5));
    }
    b
}

/// A heading over a list: a title, a dimmed line under it, then `suffix`.
pub fn section_header(title: &str, sub: Option<&str>, suffix: &[gtk::Widget]) -> gtk::Box {
    heading("title", title, sub, suffix)
}

/// A heading over a part of the page, above its sections.
pub fn page_header(title: &str, sub: Option<&str>, suffix: &[gtk::Widget]) -> gtk::Box {
    heading("title-4", title, sub, suffix)
}

fn heading(css: &str, title: &str, sub: Option<&str>, suffix: &[gtk::Widget]) -> gtk::Box {
    let text = vbox!(2, "", lbl(title, css));
    if let Some(sub) = sub {
        text.append(&lbl(sub, "caption dimmed").wrapped());
    }
    let head = hbox!(8, "section-header", text.hexpand().centered());
    for w in suffix {
        head.append(w);
    }
    head
}

/// A list drawn as Adwaita's rounded card of rows.
pub fn boxed_list() -> gtk::ListBox {
    let l = gtk::ListBox::new();
    l.set_selection_mode(gtk::SelectionMode::None);
    l.add_css_class("boxed-list");
    l
}

pub fn clear(b: &gtk::Box) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}

/// A GTK accelerator as people read it ("Ctrl+Q"), or "—" for none.
pub fn hotkey_label(accel: Option<&str>) -> String {
    let Some(accel) = accel.filter(|a| !a.is_empty()) else { return "—".into() };
    match gtk::accelerator_parse(accel) {
        Some((key, mods)) => gtk::accelerator_get_label(key, mods).to_string(),
        None => accel.to_owned(),
    }
}

/// "1 account", "3 accounts".
pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

thread_local! {
    /// Images off disk (game icons, avatars), decoded once per run and
    /// dropped when the file changes.
    static TEXTURES: RefCell<HashMap<PathBuf, (std::time::SystemTime, gdk::Texture)>> =
        RefCell::default();
}

/// The image at `path`, decoded once. A half-written cache entry is no
/// texture, not an error: the caller draws its placeholder.
pub fn texture(path: &Path) -> Option<gdk::Texture> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    TEXTURES.with_borrow_mut(|cache| {
        if let Some((_, t)) = cache.get(path).filter(|(when, _)| *when == modified) {
            return Some(t.clone());
        }
        let t = gdk::Texture::from_filename(path).ok()?;
        cache.insert(path.to_owned(), (modified, t.clone()));
        Some(t)
    })
}

/// A game's icon, `size` px square with rounded corners, or a striped
/// placeholder while it has none.
pub fn thumb(path: Option<&Path>, size: i32, css: &str) -> gtk::Widget {
    let frame = gtk::Box::new(Orientation::Horizontal, 0);
    classes(&frame, css);
    frame.set_size_request(size, size);
    frame.set_valign(Align::Center);
    frame.set_halign(Align::Center);
    frame.set_overflow(gtk::Overflow::Hidden);
    match path.and_then(texture) {
        // An Image, not a Picture: a Picture asks for the icon's own 150 px
        // and would widen whatever holds it.
        Some(t) => frame.append(&gtk::Image::builder().paintable(&t).pixel_size(size).build()),
        None => frame.add_css_class("placeholder-art"),
    }
    frame.upcast()
}

/// An account's picture: its Roblox headshot when one is cached, else its
/// initials on a colour of its own.
pub fn avatar(label: &str, picture: Option<&Path>, size: i32) -> adw::Avatar {
    let a = adw::Avatar::new(size, Some(label), true);
    if let Some(t) = picture.and_then(texture) {
        a.set_custom_image(Some(&t));
    }
    a.set_valign(Align::Center);
    a
}
