//! The side pane as an inspector: one panel at a time in place of the
//! macros and activity, beside the accounts -- never a window over them.

use adw::prelude::*;

use super::Window;
use crate::ui::panel::Panel;

impl Window {
    /// Show `panel` in the side pane, closing the one there before.
    pub fn show_panel(&self, panel: &Panel) {
        let old = self.0.ui.panel.borrow_mut().take();
        if let Some(old) = old
            && &old != panel
        {
            old.close();
        }
        let stack = &self.0.ui.inspector;
        if panel.parent().is_none() {
            stack.add_child(panel);
        }
        stack.set_visible_child(panel);
        self.0.ui.split.set_show_sidebar(true);
        self.0.ui.panel.replace(Some(panel.clone()));
        let (weak, me) = (self.weak(), panel.downgrade());
        panel.set_closer(Box::new(move || {
            if let (Some(w), Some(p)) = (weak.upgrade(), me.upgrade()) {
                w.drop_panel(&p);
            }
        }));
        if let Some(button) = panel.default_widget() {
            self.0.win.set_default_widget(Some(&button));
        }
        if let Some(target) = panel.focus_target() {
            target.grab_focus();
        }
    }

    /// Close the open panel; false when there is none.
    pub fn close_panel(&self) -> bool {
        let open = self.0.ui.panel.borrow().clone();
        match open {
            Some(p) => {
                p.close();
                true
            }
            None => false,
        }
    }

    fn drop_panel(&self, panel: &Panel) {
        let stack = &self.0.ui.inspector;
        stack.set_visible_child_name("home");
        if panel.parent().is_some() {
            stack.remove(panel);
        }
        let mut open = self.0.ui.panel.borrow_mut();
        if open.as_ref() == Some(panel) {
            *open = None;
        }
        drop(open);
        self.0.win.set_default_widget(None::<&gtk::Widget>);
    }
}
