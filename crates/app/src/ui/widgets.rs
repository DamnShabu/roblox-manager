//! Small builders: the design is a lot of labelled boxes, and GTK's
//! constructors spell each one out at length.

use gtk::prelude::*;
use gtk::{Align, Label, Orientation, glib, pango};

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

/// Setters that hand the widget back, so a widget is built in one expression.
pub trait Fluent: IsA<gtk::Widget> + Sized {
    fn valign(self, a: Align) -> Self {
        self.set_valign(a);
        self
    }
    fn halign(self, a: Align) -> Self {
        self.set_halign(a);
        self
    }
    fn centered(self) -> Self {
        self.valign(Align::Center)
    }
    fn hexpand(self) -> Self {
        self.set_hexpand(true);
        self
    }
    fn vexpand(self) -> Self {
        self.set_vexpand(true);
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

/// A Material Symbols glyph: the font turns the name into the icon.
pub fn icon(name: &str, size: u32, css: &str) -> Label {
    let l = Label::new(Some(name));
    l.add_css_class("ms");
    l.add_css_class(&format!("s{size}"));
    classes(&l, css);
    l.set_valign(Align::Center);
    l.set_halign(Align::Center);
    l
}

/// The same glyph, filled.
pub fn icon_fill(name: &str, size: u32, css: &str) -> Label {
    icon(name, size, &format!("fill {css}"))
}

/// A button of an optional icon and optional text, both kept for relabelling.
#[derive(Clone)]
pub struct IconButton {
    pub button: gtk::Button,
    pub icon: Option<Label>,
    pub text: Option<Label>,
}

impl IconButton {
    pub fn set_icon(&self, name: &str) {
        if let Some(i) = &self.icon {
            i.set_label(name);
        }
    }

    pub fn set_text(&self, text: &str) {
        if let Some(t) = &self.text {
            t.set_label(text);
        }
    }
}

/// Builds an [`IconButton`]: `Btn::new("css").text("Launch").icon("play_arrow")`.
pub struct Btn {
    css: String,
    text: Option<String>,
    icon: Option<String>,
    size: u32,
    fill: bool,
    gap: i32,
    tip: Option<String>,
}

impl Btn {
    pub fn new(css: &str) -> Self {
        Btn {
            css: css.to_owned(),
            text: None,
            icon: None,
            size: 18,
            fill: false,
            gap: 6,
            tip: None,
        }
    }
    pub fn text(mut self, t: &str) -> Self {
        self.text = Some(t.to_owned());
        self
    }
    pub fn icon(mut self, name: &str) -> Self {
        self.icon = Some(name.to_owned());
        self
    }
    pub fn size(mut self, px: u32) -> Self {
        self.size = px;
        self
    }
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }
    pub fn gap(mut self, px: i32) -> Self {
        self.gap = px;
        self
    }
    pub fn tip(mut self, t: &str) -> Self {
        self.tip = Some(t.to_owned());
        self
    }

    pub fn build(self, on_click: impl Fn() + 'static) -> IconButton {
        let button = gtk::Button::new();
        button.add_css_class("b");
        classes(&button, &self.css);
        button.set_tooltip_text(self.tip.as_deref());
        button.set_cursor_from_name(Some("pointer"));
        let icon = self.icon.map(|i| {
            if self.fill { icon_fill(&i, self.size, "") } else { icon(&i, self.size, "") }
        });
        let text = self.text.map(|t| Label::new(Some(&t)));
        let content = hbox!(self.gap, "").halign(Align::Center);
        if let Some(i) = &icon {
            content.append(i);
        }
        if let Some(t) = &text {
            content.append(t);
        }
        button.set_child(Some(&content));
        button.connect_clicked(move |_| on_click());
        IconButton { button, icon, text }
    }
}

/// A destructive button that asks for a second click within 3 s.
pub fn armed_btn(
    text: &str,
    armed_text: &str,
    ic: &str,
    action: impl Fn() + 'static,
) -> gtk::Button {
    let b = Btn::new("dbtn").text(text).icon(ic).size(17).gap(5).build(|| {});
    let armed = std::rc::Rc::new(std::cell::Cell::new(None::<glib::SourceId>));
    let (text, armed_text) = (text.to_owned(), armed_text.to_owned());
    let label = b.text.clone();
    let button = b.button.clone();
    b.button.connect_clicked(move |_| {
        if let Some(timer) = armed.take() {
            timer.remove();
            action();
            return;
        }
        if let Some(l) = &label {
            l.set_label(&armed_text);
        }
        button.add_css_class("armed");
        let (armed2, label2, button2, text2) =
            (armed.clone(), label.clone(), button.clone(), text.clone());
        armed.set(Some(glib::timeout_add_local_once(
            std::time::Duration::from_secs(3),
            move || {
                armed2.set(None);
                if let Some(l) = &label2 {
                    l.set_label(&text2);
                }
                button2.remove_css_class("armed");
            },
        )));
    });
    b.button.halign(Align::Start)
}

pub fn switch(active: bool, tip: Option<&str>, on_change: impl Fn(bool) + 'static) -> gtk::Switch {
    let sw = gtk::Switch::new();
    sw.set_active(active);
    sw.set_valign(Align::Center);
    sw.add_css_class("sw");
    sw.set_tooltip_text(tip);
    sw.set_cursor_from_name(Some("pointer"));
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

/// A status dot, optionally with a pulsing ring.
pub fn dot(kind: &str, pulse: bool, size: u32) -> gtk::Widget {
    let core = new_box(Orientation::Horizontal, 0, &format!("dot d{size} {kind}"))
        .centered()
        .halign(Align::Center);
    if !pulse {
        return core.upcast();
    }
    let o = gtk::Overlay::new();
    o.set_child(Some(&core));
    o.set_valign(Align::Center);
    o.add_overlay(&new_box(Orientation::Horizontal, 0, &format!("dot d{size} {kind} ring")));
    o.upcast()
}

/// A glyph centred in a tile its CSS sizes. The tile is the label itself,
/// which centres its text both ways: a box would pack the glyph at the top,
/// and expanding the glyph instead spreads up and stretches every ancestor.
pub fn icon_tile(css: &str, ic: &str, size: u32) -> Label {
    icon(ic, size, css)
}

/// A section heading: icon tile, title, subtitle, then `suffix`.
pub fn section(ic: &str, title: &str, sub: &str, suffix: &[gtk::Widget]) -> gtk::Box {
    let head = hbox!(
        12,
        "",
        icon_tile("stile", ic, 19).centered(),
        vbox!(1, "", lbl(title, "stitle"), lbl(sub, "ssub").ellipsize()).hexpand().centered()
    );
    for w in suffix {
        head.append(w);
    }
    head
}

/// One labelled row of a settings panel: a 124 px label column, then the content.
pub fn setting(panel: &gtk::Box, title: &str, content: &impl IsA<gtk::Widget>, top: i32) {
    content.set_hexpand(true);
    let label = lbl(title, "plabel").valign(Align::Start).top(top).width(124).wrapped().chars(16);
    panel.append(&hbox!(18, "", label, content.clone()));
}

pub fn clear(b: &gtk::Box) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}

pub fn clear_wrap(b: &adw::WrapBox) {
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

thread_local! {
    /// Game icons, decoded once per run.
    static TEXTURES: std::cell::RefCell<std::collections::HashMap<std::path::PathBuf, gtk::gdk::Texture>> =
        std::cell::RefCell::default();
}

/// A game's icon, `size` px square, or the striped placeholder.
pub fn thumb(path: Option<&std::path::Path>, size: i32, css: &str) -> gtk::Box {
    let b = new_box(Orientation::Horizontal, 0, &format!("thumb t{size} {css}"));
    b.set_size_request(size, size);
    b.set_valign(Align::Center);
    b.set_halign(Align::Center);
    b.set_overflow(gtk::Overflow::Hidden);
    let texture = path.and_then(|p| {
        TEXTURES.with_borrow_mut(|cache| {
            if let Some(t) = cache.get(p) {
                return Some(t.clone());
            }
            // A half-written cache entry is a placeholder, not an error.
            let t = gtk::gdk::Texture::from_filename(p).ok()?;
            cache.insert(p.to_owned(), t.clone());
            Some(t)
        })
    });
    match texture {
        // An Image, not a Picture: a Picture asks for the icon's own 150 px
        // and would widen whatever holds it past the design's.
        Some(t) => b.append(&gtk::Image::builder().paintable(&t).pixel_size(size).build()),
        None => b.add_css_class("stripes"),
    }
    b
}

/// A thumbnail-sized tile with a symbol, styled by `ic_css`, centred on its
/// own background. Overlaid, not packed: centring a packed child takes
/// hexpand, and that spreads up and stretches every tile in the row.
pub fn symbol_thumb(
    size: i32,
    css: &str,
    ic: &str,
    ic_size: u32,
    ic_css: &str,
) -> (gtk::Overlay, gtk::Box) {
    let b = new_box(Orientation::Horizontal, 0, &format!("thumb t{size} {css}"));
    b.set_size_request(size, size);
    b.set_overflow(gtk::Overflow::Hidden);
    let over = gtk::Overlay::new();
    over.set_child(Some(&b));
    over.set_valign(Align::Center);
    over.set_halign(Align::Center);
    over.add_overlay(&icon(ic, ic_size, ic_css));
    (over, b)
}
