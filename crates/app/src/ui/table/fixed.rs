//! A layout whose natural width is set rather than asked of its child: a
//! cell laid out by it wants the same width in every row, whatever is in
//! it, so a long name never moves the columns after it. The child is drawn
//! into what the cell is given, and cuts its words to fit.

use std::cell::Cell;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct FixedNatural {
        pub natural: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FixedNatural {
        const NAME: &'static str = "RmFixedNatural";
        type Type = super::FixedNatural;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for FixedNatural {}

    impl LayoutManagerImpl for FixedNatural {
        fn measure(
            &self,
            widget: &gtk::Widget,
            orientation: gtk::Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let Some(child) = widget.first_child() else { return (0, 0, -1, -1) };
            let (min, nat, _, _) = child.measure(orientation, for_size);
            match orientation {
                gtk::Orientation::Horizontal => (min, self.natural.get().max(min), -1, -1),
                _ => (min, nat, -1, -1),
            }
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, baseline: i32) {
            if let Some(child) = widget.first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

glib::wrapper! {
    pub struct FixedNatural(ObjectSubclass<imp::FixedNatural>) @extends gtk::LayoutManager;
}

impl FixedNatural {
    pub fn new(natural: i32) -> Self {
        let l: Self = glib::Object::new();
        l.imp().natural.set(natural);
        l
    }
}

/// `child` in a box whose natural width is `natural`, whatever the child
/// would like.
pub fn fixed_natural(child: &impl IsA<gtk::Widget>, natural: i32) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.append(child);
    b.set_layout_manager(Some(FixedNatural::new(natural)));
    b
}
