use super::*;
use crate::macros::grammar::parse;

/// The top of every range, so times are known.
fn top(_lo: f64, hi: f64) -> f64 {
    hi
}

/// What the timeline written in `lines` sends, and when, with the pointer
/// starting at `at`.
fn sends(lines: &[&str], at: Option<(i32, i32)>) -> Vec<(f64, Send)> {
    let text = format!("timeline\n{}", lines.join("\n"));
    let Step::Timeline { items, .. } = parse(&text).unwrap().steps.remove(0) else {
        panic!("not a timeline: {text}")
    };
    // To the millisecond, so sums of floats compare.
    let ms = |t: f64| (t * 1000.0).round() / 1000.0;
    schedule(&items, at, &top).0.into_iter().map(|d| (ms(d.t), d.what)).collect()
}

#[test]
fn steps_play_over_one_another_each_at_its_own_time() {
    let got = sends(&["at 0 hold w 1", "at 0.5 tap space 0.1", "at 0.8 scroll down 2"], None);
    assert_eq!(
        got,
        [
            (0.0, Send::Down(17)),
            (0.5, Send::Down(57)),
            (0.6, Send::Up(57)),
            (0.8, Send::Scroll(false, 2)),
            (1.0, Send::Up(17)),
        ]
    );
}

#[test]
fn the_order_lines_are_written_in_does_not_matter() {
    let ordered = sends(&["at 0 hold w 1", "at 0.5 tap e 0.1"], None);
    assert_eq!(sends(&["at 0.5 tap e 0.1", "at 0 hold w 1"], None), ordered);
}

#[test]
fn a_key_let_go_and_held_again_at_once_is_let_go_first() {
    let got = sends(&["at 0.5 hold w 0.5", "at 0 hold w 0.5"], None);
    assert_eq!(
        got,
        [(0.0, Send::Down(17)), (0.5, Send::Up(17)), (0.5, Send::Down(17)), (1.0, Send::Up(17))]
    );
}

#[test]
fn a_path_glides_a_pixel_at_a_time_from_its_first_point() {
    let got = sends(&["at 1 path 0 10 10, 0.04 14 10"], None);
    assert_eq!(
        got,
        [
            (1.0, Send::MoveTo(10, 10)),
            (1.01, Send::MoveTo(11, 10)),
            (1.02, Send::MoveTo(12, 10)),
            (1.03, Send::MoveTo(13, 10)),
            (1.04, Send::MoveTo(14, 10)),
        ]
    );
}

#[test]
fn a_path_that_rests_sends_nothing_while_it_rests() {
    let got = sends(&["at 0 path 0 5 5, 1 5 5, 1.01 6 5"], None);
    assert_eq!(got, [(0.0, Send::MoveTo(5, 5)), (1.01, Send::MoveTo(6, 5))]);
}

#[test]
fn a_turn_sends_exactly_the_distance_written_however_it_is_cut() {
    let got = sends(&["at 0 turn 0.03 1 -0.5, 0.05 1 2"], None);
    let (dx, dy) = got.iter().fold((0.0, 0.0), |(x, y), (_, s)| match s {
        Send::Motion(dx, dy) => (x + dx, y + dy),
        other => panic!("{other:?}"),
    });
    assert_eq!((dx, dy), (1.0, 2.0));
    assert_eq!(got.len(), 5, "a tick's worth each: {got:?}");
    assert_eq!(got[0].0, 0.01);
}

#[test]
fn a_move_to_glides_from_where_the_pointer_is_or_goes_at_once() {
    assert_eq!(sends(&["at 0 move to 3 0 0.02"], None), [(0.0, Send::MoveTo(3, 0))]);
    let got = sends(&["at 0 move to 4 0 0.02"], Some((0, 0)));
    assert_eq!(
        got,
        [(0.0, Send::MoveTo(0, 0)), (0.01, Send::MoveTo(2, 0)), (0.02, Send::MoveTo(4, 0))]
    );
}

#[test]
fn a_click_goes_to_its_point_and_presses_there() {
    let got = sends(&["at 2 click right 7 8"], None);
    assert_eq!(got, [(2.0, Send::MoveTo(7, 8)), (2.0, Send::Down(0x111)), (2.08, Send::Up(0x111))]);
}

#[test]
fn the_pointer_is_left_where_the_last_move_put_it() {
    let text = "timeline\nat 0 path 0 1 1, 0.1 50 60\nat 0.2 turn 0.1 10 -5";
    let Step::Timeline { items, .. } = parse(text).unwrap().steps.remove(0) else { panic!() };
    assert_eq!(schedule(&items, None, &top).1, Some((60, 55)));
}
