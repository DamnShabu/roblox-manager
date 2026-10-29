//! Gliding the pointer from wherever the macro last put it.

use super::*;

#[test]
fn a_glide_from_far_off_the_display_does_not_overflow() {
    let mut r = Recorder::default();
    let mut held = Held { at: Some((i32::MAX, i32::MIN)), ..Held::default() };
    let stop = StopFlag::default();
    play_step(&mut r, &step("move to -5 5 0.02"), &stop, &top, &mut held).unwrap();
    assert_eq!(r.sent.borrow().last(), Some(&Sent::MoveTo(-5, 5)));
}

#[test]
fn a_timeline_glide_from_far_off_the_display_does_not_overflow() {
    let mut r = Recorder::default();
    let mut held = Held { at: Some((i32::MAX, i32::MIN)), ..Held::default() };
    let stop = StopFlag::default();
    play_step(&mut r, &step("timeline\nat 0 move to -5 5 0.02"), &stop, &top, &mut held).unwrap();
    assert_eq!(r.sent.borrow().last(), Some(&Sent::MoveTo(-5, 5)));
}
