//! Choosing one of the running macro-ready clients to act in: the only one
//! up, or the one picked from a menu under the button that asked.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::ui::window::{ReadyClient, Window};

/// The only macro-ready client up, or the one picked from a menu under
/// `anchor`; with none up, `then` hears `none`. A menu closed without a pick
/// says nothing.
pub fn choose(
    w: &Window,
    anchor: &gtk::Button,
    none: &str,
    then: impl FnOnce(Result<ReadyClient, String>) + 'static,
) {
    let mut clients = w.macro_ready_clients();
    match clients.len() {
        0 => then(Err(none.to_owned())),
        1 => then(Ok(clients.remove(0))),
        _ => menu(anchor, clients, then),
    }
}

fn menu(
    anchor: &gtk::Button,
    clients: Vec<ReadyClient>,
    then: impl FnOnce(Result<ReadyClient, String>) + 'static,
) {
    // Shared by every entry; the first one clicked takes it.
    let then = Rc::new(RefCell::new(Some(then)));
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let popover = gtk::Popover::builder().child(&list).build();
    popover.set_parent(anchor);
    for client in clients {
        let b = gtk::Button::builder().label(&client.label).css_classes(["flat"]).build();
        let (then, pop, client) =
            (Rc::clone(&then), popover.downgrade(), RefCell::new(Some(client)));
        b.connect_clicked(move |_| {
            if let Some(p) = pop.upgrade() {
                p.popdown();
            }
            if let (Some(f), Some(c)) = (then.borrow_mut().take(), client.borrow_mut().take()) {
                f(Ok(c));
            }
        });
        list.append(&b);
    }
    popover.connect_closed(|p| {
        // Unparented once its click has been handled.
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
}
