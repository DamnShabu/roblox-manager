//! What used to be a dialog, shown in the window instead: the side pane's
//! inspector holds one panel at a time, beside the accounts, so settings,
//! sign-ins, macros and the log never cover the window or stack up.
//!
//! A panel takes the calls the dialogs made -- a child, `present`, `close`,
//! `connect_closed` -- so each one moved over without changing what it does.

use std::cell::RefCell;
use std::sync::OnceLock;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use gtk::glib::subclass::Signal;

use crate::ui::window::Window;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Panel {
        pub title: RefCell<String>,
        /// Takes the panel out of the inspector; set while it is shown.
        pub closer: RefCell<Option<Box<dyn Fn()>>>,
        pub default: RefCell<Option<gtk::Widget>>,
        pub focus: RefCell<Option<gtk::Widget>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Panel {
        const NAME: &'static str = "RbxmgrPanel";
        type Type = super::Panel;
        type ParentType = adw::Bin;
    }

    impl ObjectImpl for Panel {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![Signal::builder("closed").build()])
        }
    }
    impl WidgetImpl for Panel {}
    impl BinImpl for Panel {}
}

glib::wrapper! {
    pub struct Panel(ObjectSubclass<imp::Panel>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Panel {
    pub fn new(title: &str) -> Self {
        let p: Panel = glib::Object::new();
        p.imp().title.replace(title.to_owned());
        p.set_vexpand(true);
        p
    }

    pub fn title(&self) -> String {
        self.imp().title.borrow().clone()
    }

    /// A page of settings rows, under a header with the panel's title.
    pub fn with_page(title: &str, page: &adw::PreferencesPage) -> Self {
        let p = Panel::new(title);
        p.set_page(page);
        p
    }

    /// Show `page` in place of the one before (a page rebuilt as its
    /// account changes shape).
    pub fn set_page(&self, page: &adw::PreferencesPage) {
        let view = adw::ToolbarView::new();
        view.add_top_bar(&self.header());
        view.set_content(Some(page));
        self.set_child(Some(&view));
    }

    /// The panel's own header bar: its title and a close button, without
    /// the window's buttons (the window has those already).
    pub fn header(&self) -> adw::HeaderBar {
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .title_widget(&adw::WindowTitle::new(&self.title(), ""))
            .build();
        let close = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Close (Esc)")
            .css_classes(["flat", "circular"])
            .build();
        close.update_property(&[gtk::accessible::Property::Label("Close")]);
        let me = self.downgrade();
        close.connect_clicked(move |_| {
            if let Some(p) = me.upgrade() {
                p.close();
            }
        });
        header.pack_end(&close);
        header
    }

    /// Show it in the window's inspector, in place of any panel there.
    pub fn present(&self, w: &Window) {
        w.show_panel(self);
    }

    /// Take it out of the inspector, and say so to whoever listens.
    pub fn close(&self) {
        let closer = self.imp().closer.take();
        if let Some(closer) = closer {
            closer();
        }
        self.emit_by_name::<()>("closed", &[]);
    }

    /// Run `f` once the panel is closed, by its button, by Esc, or by
    /// another panel taking its place.
    pub fn connect_closed<F: Fn(&Panel) + 'static>(&self, f: F) {
        self.connect_closure(
            "closed",
            false,
            glib::closure_local!(move |p: Panel| {
                f(&p);
            }),
        );
    }

    pub(crate) fn set_closer(&self, closer: Box<dyn Fn()>) {
        self.imp().closer.replace(Some(closer));
    }

    pub fn is_shown(&self) -> bool {
        self.imp().closer.borrow().is_some()
    }

    /// The button Enter presses while the panel has focus.
    pub fn set_default_widget(&self, widget: Option<&impl IsA<gtk::Widget>>) {
        self.imp().default.replace(widget.map(|w| w.clone().upcast()));
    }

    pub fn default_widget(&self) -> Option<gtk::Widget> {
        self.imp().default.borrow().clone()
    }

    /// What takes the keyboard when the panel is shown.
    pub fn set_focus(&self, widget: Option<&impl IsA<gtk::Widget>>) {
        self.imp().focus.replace(widget.map(|w| w.clone().upcast()));
    }

    pub fn focus_target(&self) -> Option<gtk::Widget> {
        self.imp().focus.borrow().clone()
    }
}
