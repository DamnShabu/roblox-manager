//! The design's modal: an icon tile, title and subtitle, a close button,
//! then content and a footer.

use adw::prelude::*;
use gtk::Align;

use super::widgets::{Btn, Fluent, LabelFluent, icon, lbl};

pub struct Modal {
    pub dialog: adw::Dialog,
    pub header: gtk::Box,
}

impl Modal {
    pub fn new(ic: &str, title: &str, sub: &str, width: i32) -> Self {
        let dialog = adw::Dialog::builder().content_width(width).title(title).build();
        dialog.add_css_class("modal");
        let title_label = lbl(title, "mtitle");
        let subtitle = lbl(sub, "msub").wrapped();
        let close = {
            let d = dialog.clone();
            Btn::new("mclose").icon("close").size(20).tip("Close").build(move || {
                d.close();
            })
        };
        let heading = vbox!(2, "", title_label, subtitle).hexpand().top(1);
        let header = hbox!(
            14,
            "mhdr",
            vbox!(0, "micon", icon(ic, 22, "")).valign(Align::Start),
            heading,
            close.button.valign(Align::Start)
        );
        Modal { dialog, header }
    }

    pub fn build(&self, body: &impl IsA<gtk::Widget>, footer: &impl IsA<gtk::Widget>) {
        self.dialog.set_child(Some(&vbox!(
            0,
            "",
            self.header.clone(),
            body.clone(),
            footer.clone()
        )));
    }

    pub fn present(&self, parent: &impl IsA<gtk::Widget>) {
        self.dialog.present(Some(parent));
    }

    /// Keep the dialog's own state alive while it shows: its widgets hold
    /// only weak handles on it, so nothing else would.
    pub fn keep_alive<T: 'static>(&self, state: std::rc::Rc<T>) {
        let held = std::cell::RefCell::new(Some(state));
        self.dialog.connect_closed(move |_| {
            held.take();
        });
    }
}
