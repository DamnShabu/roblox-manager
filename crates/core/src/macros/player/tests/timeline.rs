//! A timeline played into a client, against what the steps around it hold.

use super::*;

#[test]
fn a_timelines_hold_leaves_a_key_a_press_before_it_holds() {
    let mut r = Recorder::default();
    let held = steps(&mut r, &["press shift", "timeline\nat 0 hold shift+w 0.01"]).unwrap();
    assert_eq!(
        *r.sent.borrow(),
        [Sent::Key(42, true), Sent::Key(17, true), Sent::Key(17, false)],
        "shift stays down, as the same hold outside a timeline leaves it"
    );
    assert_eq!(held.down, [42]);
}

#[test]
fn overlapping_holds_of_one_key_let_go_at_the_last_ones_end() {
    let mut r = Recorder::default();
    steps(&mut r, &["timeline\nat 0 hold w 0.04\nat 0.02 hold w 0.04\nat 0.05 tap e 0.001"])
        .unwrap();
    let w_up = r.sent.borrow().iter().position(|s| *s == Sent::Key(17, false)).unwrap();
    let e_down = r.sent.borrow().iter().position(|s| *s == Sent::Key(18, true)).unwrap();
    assert!(e_down < w_up, "w is still held at 0.05: {:?}", r.sent.borrow());
}

#[test]
fn a_timelines_release_lets_go_of_a_key_pressed_before_it() {
    let mut r = Recorder::default();
    let held = steps(&mut r, &["press shift", "timeline\nat 0 release shift"]).unwrap();
    assert_eq!(*r.sent.borrow(), [Sent::Key(42, true), Sent::Key(42, false)]);
    assert!(held.down.is_empty());
}

#[test]
fn a_timelines_press_stays_down_after_it() {
    let mut r = Recorder::default();
    let held = steps(&mut r, &["timeline\nat 0 hold w 0.01\nat 0 press w"]).unwrap();
    assert_eq!(*r.sent.borrow(), [Sent::Key(17, true)]);
    assert_eq!(held.down, [17]);
}
