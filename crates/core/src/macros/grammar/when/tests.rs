use super::*;
use crate::macros::grammar::{Row, parse, rows, to_text};

#[test]
fn a_when_s_do_lines_go_under_it_and_the_macro_s_own_steps_go_on_around() {
    let src =
        "tap w\nwhen image coin 812 40 95%\ndo tap e\ndo wait 0.5\n# a note\ndo click 4 5\ntap q\n";
    let m = parse(src).unwrap();
    assert_eq!(m.steps.len(), 2, "tap w and tap q");
    assert_eq!(m.handlers.len(), 1);
    let h = &m.handlers[0];
    let image = Sight::Image { name: "coin".into(), place: Place::Near(812, 40), least: 0.95 };
    assert_eq!(h.when, Condition { sight: image, not: false });
    assert_eq!(h.steps.len(), 3);
    let (r, loops) = rows(src);
    assert_eq!(r[1], Row { kind: "When".into(), value: "image coin 812 40 95%".into() });
    assert_eq!(r[2], Row { kind: "Do".into(), value: "tap e".into() });
    assert_eq!(to_text(&r, loops), src);
}

#[test]
fn colours_and_nots_read_with_their_defaults() {
    let m = parse("when not color 1 2 #FF8000\ndo tap e\nwhen colour 3 4 #000000 10\ndo tap f\n");
    let m = m.unwrap();
    assert_eq!(
        m.handlers[0].when,
        Condition {
            sight: Sight::Color { x: 1, y: 2, rgb: [255, 128, 0], within: COLOR_WITHIN },
            not: true,
        }
    );
    assert_eq!(m.handlers[1].when.sight, Sight::Color { x: 3, y: 4, rgb: [0; 3], within: 10 });
    assert!(m.steps.is_empty(), "a macro of only whens is a macro");
    let image = parse("when image a-1 0 0\ndo tap e\n").unwrap();
    assert!(
        matches!(image.handlers[0].when.sight, Sight::Image { least, .. } if least == IMAGE_LEAST)
    );
}

#[test]
fn a_when_says_what_is_wrong_with_it_by_line() {
    let err = |src: &str| parse(src).unwrap_err();
    assert_eq!(err("tap e\nwhen image coin 1 2\ntap f\n").line, Some(2), "no do under it");
    assert_eq!(err("tap e\ndo tap f\n").line, Some(2), "a do with no when");
    assert_eq!(err("when image coin 1 2\ntap f\ndo tap g\n").line, Some(3), "a step between");
    assert!(err("when image ../x 1 2\ndo tap e\n").message.contains("not an image name"));
    assert!(err("when image coin 1 2 150%\ndo tap e\n").message.contains("percentage"));
    assert!(err("when color 1 2 red\ndo tap e\n").message.contains("#RRGGBB"));
    assert!(err("when color -1 2 #000000\ndo tap e\n").message.contains("off the window"));
    assert!(err("when image coin 1 2\ndo repeat e 5\n").message.contains("cannot 'repeat'"));
    assert!(err("when sound 1\ndo tap e\n").message.contains("expected when"));
}

#[test]
fn an_image_is_looked_for_anywhere_near_a_point_or_in_a_rectangle() {
    let place = |src: &str| match condition(src).unwrap().sight {
        Sight::Image { place, least, .. } => (place, least),
        Sight::Color { .. } => panic!("not an image"),
    };
    assert_eq!(place("image coin"), (Place::Anywhere, IMAGE_LEAST));
    assert_eq!(place("image coin 80%"), (Place::Anywhere, 0.8));
    assert_eq!(place("image coin 3 4"), (Place::Near(3, 4), IMAGE_LEAST));
    assert_eq!(
        place("image coin in 0 10 400 300 95%"),
        (Place::Within { x: 0, y: 10, w: 400, h: 300 }, 0.95)
    );
    for bad in ["image coin 3", "image coin in 0 0 0 10", "image coin on 0 0 1 1", "image"] {
        assert!(condition(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_picked_image_is_looked_for_anywhere_unless_the_line_said_where() {
    assert_eq!(picked_image("", "image3"), "image image3");
    assert_eq!(picked_image("not image old 1 1 80%", "new"), "not image new 80%");
    assert_eq!(picked_image("image old in 0 0 50 60", "new"), "image new in 0 0 50 60");
    assert_eq!(picked_image("color 1 1 #ffffff", "new"), "image new");
}

#[test]
fn a_when_describes_what_it_waits_for() {
    let c = |src: &str| describe(&condition(src).unwrap());
    assert_eq!(c("image coin 1 2"), "coin shows");
    assert_eq!(c("not image coin 1 2"), "coin is gone");
    assert_eq!(c("color 1 2 #FF0000"), "1, 2 turns #ff0000");
    assert_eq!(c("not color 1 2 #ff0000"), "1, 2 is no longer #ff0000");
}
