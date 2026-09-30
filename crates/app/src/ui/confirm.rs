//! Asking before something that cannot be undone.

use adw::prelude::*;

use super::window::Window;

/// An alert with Cancel and a destructive `verb`; `on_yes` runs on the
/// window when the verb is chosen. Cancel is the default, so Enter is safe.
pub fn ask(w: &Window, heading: &str, body: &str, verb: &str, on_yes: impl Fn(&Window) + 'static) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_responses(&[("cancel", "_Cancel"), ("yes", verb)]);
    dialog.set_response_appearance("yes", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let act = w.act(on_yes);
    dialog.connect_response(Some("yes"), move |_, _| act());
    dialog.present(Some(w.gtk_window()));
}

/// A plain message with one button, for a reason too long for a toast.
pub fn tell(w: &Window, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_response("close", "_Close");
    dialog.set_body_use_markup(false);
    dialog.present(Some(w.gtk_window()));
}
