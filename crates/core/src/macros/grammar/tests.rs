use super::*;

fn hold(keys: &[u16], lo: f64, hi: f64) -> Step {
    Step::Hold { keys: keys.to_vec(), lo, hi }
}

#[test]
fn a_macro_parses_into_the_codes_a_us_keyboard_sends() {
    let m = parse(
        "# farm\nloop 3\ntap E\ntap f 0.2-0.3\nhold shift+w 1-2\n\ntype -Gg\nclick right 10 20\n\
         move -5 5\nwait 0.5\ntap F5\ntap /\n",
    )
    .unwrap();
    let (lo, hi) = TAP_PRESS;
    assert_eq!(m.loops, 3);
    assert_eq!(
        m.steps,
        vec![
            hold(&[18], lo, hi),
            hold(&[33], 0.2, 0.3),
            hold(&[42, 17], 1.0, 2.0),
            Step::Type(vec![(12, false), (34, true), (34, false)]),
            Step::Click { button: BUTTON_RIGHT, at: Some((10, 20)) },
            Step::Move(-5, 5),
            Step::Wait(0.5, 0.5),
            hold(&[63], lo, hi),
            hold(&[53], lo, hi),
        ]
    );
}

#[test]
fn a_tap_is_its_own_keys_code_never_escape() {
    let (lo, hi) = TAP_PRESS;
    assert_eq!(parse("tap j").unwrap().steps, [hold(&[36], lo, hi)]);
    assert_eq!(parse("tap space").unwrap().steps, [hold(&[57], lo, hi)]);
}

#[test]
fn values_too_large_to_play_are_refused_by_line() {
    for (bad, why) in [
        ("wait 1e13", "line 1"),
        ("tap e\nwait 1e20", "line 2"),
        ("hold w 100000", "longer than a day"),
        ("move 9000000 0", "too far"),
        ("click 70000 5", "too far"),
    ] {
        let err = parse(bad).unwrap_err().to_string();
        assert!(err.contains(why), "{bad:?}: {err}");
    }
    assert!(parse("wait 86400\nmove -65535 65535").is_ok(), "a day and a screen's width are fine");
}

#[test]
fn a_bad_macro_is_refused_and_says_where() {
    for (bad, why) in [
        ("tap -k", "unknown key"),
        ("tap e\nwait 3-1", "line 2"),
        ("click 5", "click"),
        ("jump", "don't understand"),
        ("type héllo", "cannot type"),
        ("# nothing", "no steps"),
    ] {
        let err = parse(bad).unwrap_err().to_string();
        assert!(err.contains(why), "{bad:?}: {err}");
    }
}

#[test]
fn the_editors_rows_name_types_keep_notes_and_hold_the_loop_apart() {
    let src = "# note\nstart 5\ntap j\nhold shift+w 2\nwait 60-70\nfrob 1\nloop 3\n";
    let row = |k: &str, v: &str| Row { kind: k.into(), value: v.into() };
    assert_eq!(
        rows(src),
        (
            vec![
                row("Note", "note"),
                row("Start", "5"),
                row("Key", "j"),
                row("Hold", "shift+w 2"),
                row("Wait", "60-70"),
                row("Frob", "1"),
            ],
            3
        )
    );
    let (r, loops) = rows(src);
    assert_eq!(to_text(&r, loops), src, "rows go back to the very text they came from");
}

#[test]
fn a_macro_with_no_loop_line_runs_until_stopped() {
    assert_eq!(rows("tap e").1, 0);
    assert_eq!(
        (loop_label(0), loop_label(1), loop_label(3)),
        ("Until stopped".into(), "Once".into(), "3 rounds".into())
    );
}

#[test]
fn an_editor_made_macro_is_one_the_player_runs() {
    let row = |k: &str, v: &str| Row { kind: k.into(), value: v.into() };
    let text = to_text(&[row("Key", "e"), row("Wait", "0.5"), row("Click", "960 540")], 1);
    assert_eq!(parse(&text).unwrap().loops, 1);
}

#[test]
fn the_editors_names_read_as_the_commands_they_stand_for() {
    let src = "Start 45\nKey j\nNote farm the boss\nWait 60-70\nloop 2\n";
    let m = parse(src).unwrap();
    assert_eq!((m.loops, m.steps.len()), (2, 3), "the note is no step");
    let (r, loops) = rows(src);
    assert_eq!(
        r.iter().map(|r| r.kind.as_str()).collect::<Vec<_>>(),
        ["Start", "Key", "Note", "Wait"]
    );
    assert_eq!(to_text(&r, loops), "start 45\ntap j\n# farm the boss\nwait 60-70\nloop 2\n");
}

#[test]
fn a_picked_point_replaces_the_clicks_point_and_keeps_its_button() {
    assert_eq!(click_at("", 960, 540), "960 540");
    assert_eq!(click_at("10 20", 960, 540), "960 540");
    assert_eq!(click_at("right", 5, 6), "right 5 6");
    assert_eq!(click_at(" middle 1 2 ", 5, 6), "middle 5 6");
    assert!(click(&["right", "5", "6"]).is_ok(), "what it writes parses");
}

#[test]
fn steps_describe_themselves() {
    assert_eq!(describe(&hold(&[42, 17], 1.0, 1.0)), "pressing shift+w");
    assert_eq!(describe(&Step::Click { button: BUTTON_LEFT, at: None }), "clicking");
    assert_eq!(describe(&Step::Type(vec![])), "typing");
}

#[test]
fn press_and_release_leave_keys_and_buttons_down_between_steps() {
    let m = parse("press w\npress mouse2\nrelease mouse2\nrelease shift+w\n").unwrap();
    assert_eq!(
        m.steps,
        [
            Step::Press(vec![17]),
            Step::Press(vec![BUTTON_RIGHT]),
            Step::Release(vec![BUTTON_RIGHT]),
            Step::Release(vec![42, 17]),
        ]
    );
}

#[test]
fn a_move_to_goes_to_a_point_at_once_or_over_a_time() {
    let m = parse("move to 640 360\nmove to 10 20 0.25\nmove to 0 0 1-2\nmove 5 -5\n").unwrap();
    assert_eq!(
        m.steps,
        [
            Step::MoveTo { x: 640, y: 360, lo: 0.0, hi: 0.0 },
            Step::MoveTo { x: 10, y: 20, lo: 0.25, hi: 0.25 },
            Step::MoveTo { x: 0, y: 0, lo: 1.0, hi: 2.0 },
            Step::Move(5, -5),
        ]
    );
}

#[test]
fn a_scroll_turns_the_wheel_by_notches_down_and_right_positive() {
    let m = parse("scroll down\nscroll up 3\nscroll right 2\nscroll left\n").unwrap();
    assert_eq!(
        m.steps,
        [
            Step::Scroll { horizontal: false, notches: 1 },
            Step::Scroll { horizontal: false, notches: -3 },
            Step::Scroll { horizontal: true, notches: 2 },
            Step::Scroll { horizontal: true, notches: -1 },
        ]
    );
}

#[test]
fn a_bad_press_move_to_or_scroll_is_refused_and_says_why() {
    for (bad, why) in [
        ("press", "don't understand"),
        ("release -k", "unknown key"),
        ("move to 5", "don't understand"),
        ("move to x 5", "not a number"),
        ("move to 70000 5", "too far"),
        ("move to 5 5 -1", "not a duration"),
        ("scroll sideways", "scroll up|down|left|right"),
        ("scroll down 0", "1 to 1000"),
        ("scroll down 1001", "1 to 1000"),
    ] {
        let err = parse(bad).unwrap_err().to_string();
        assert!(err.contains(why), "{bad:?}: {err}");
    }
}

#[test]
fn the_new_steps_have_editor_types_and_go_back_to_the_same_text() {
    let src = "press w\nmove to 10 20 0.25\nscroll up 2\nrelease w\n";
    let (r, loops) = rows(src);
    assert_eq!(
        r.iter().map(|r| (r.kind.as_str(), r.value.as_str())).collect::<Vec<_>>(),
        [("Press", "w"), ("Move", "to 10 20 0.25"), ("Scroll", "up 2"), ("Release", "w")]
    );
    assert_eq!(to_text(&r, loops), src);
}

#[test]
fn the_new_steps_describe_themselves() {
    assert_eq!(describe(&Step::Press(vec![17])), "holding down w");
    assert_eq!(describe(&Step::Release(vec![BUTTON_LEFT])), "letting go of mouse1");
    assert_eq!(describe(&Step::MoveTo { x: 1, y: 2, lo: 0.0, hi: 0.0 }), "moving the mouse");
    assert_eq!(describe(&Step::Scroll { horizontal: false, notches: 2 }), "scrolling");
}

#[test]
fn a_repeat_taps_every_so_often_for_a_time() {
    assert_eq!(
        parse("repeat e 10").unwrap().steps,
        [Step::Repeat { keys: vec![18], lo: 10.0, hi: 10.0, every: REPEAT_EVERY }]
    );
    assert_eq!(
        parse("Repeat shift+e 5-6 0.3-0.5").unwrap().steps,
        [Step::Repeat { keys: vec![42, 18], lo: 5.0, hi: 6.0, every: (0.3, 0.5) }]
    );
    let (rows, _) = rows("repeat e 10 0.5\n");
    assert_eq!(rows, [Row { kind: "Repeat".into(), value: "e 10 0.5".into() }]);
    for (bad, why) in [("repeat e", "don't understand"), ("repeat e 5 0", "at most every")] {
        let err = parse(bad).unwrap_err().to_string();
        assert!(err.contains(why), "{bad:?}: {err}");
    }
}

#[test]
fn a_timeline_holds_the_at_steps_under_it() {
    let src = "start 5\ntimeline 3\n# a note inside\nat 0 hold w 1.4\nat 0.95 tap space 0.08\n\
               at 0.3 path 0 400 300, 0.25 520 310\nat 1 turn 0.5 40.5 -3\nwait 1\n";
    let m = parse(src).unwrap();
    assert_eq!(m.steps.len(), 3);
    let Step::Timeline { secs, items } = &m.steps[1] else { panic!("{:?}", m.steps[1]) };
    assert_eq!(*secs, (3.0, 3.0));
    let at: Vec<(f64, &Step)> = items.iter().map(|i| (i.at.0, &i.step)).collect();
    assert_eq!(
        at,
        [
            (0.0, &Step::Hold { keys: vec![17], lo: 1.4, hi: 1.4 }),
            (0.95, &Step::Hold { keys: vec![57], lo: 0.08, hi: 0.08 }),
            (0.3, &Step::Path(vec![(0.0, 400, 300), (0.25, 520, 310)])),
            (1.0, &Step::Turn(vec![(0.5, 40.5, -3.0)])),
        ]
    );
    let (r, loops) = rows(src);
    assert_eq!(r[1], Row { kind: "Timeline".into(), value: "3".into() });
    assert_eq!(r[3], Row { kind: "At".into(), value: "0 hold w 1.4".into() });
    assert_eq!(to_text(&r, loops), src);
}

#[test]
fn an_at_step_outside_a_timeline_or_one_it_cannot_time_is_refused() {
    for (bad, why) in [
        ("at 1 tap e", "goes under a 'timeline'"),
        ("timeline\nwait 1\nat 1 tap e", "goes under a 'timeline'"),
        ("timeline\nat 1 wait 2", "a timeline cannot 'wait'"),
        ("timeline\nat 1 type hi", "a timeline cannot 'type'"),
        ("timeline\nat x tap e", "not a duration"),
        ("timeline\nat 1", "expected at SECONDS STEP"),
        ("path 1 2 3, 0.5 4 5", "times go forward"),
        ("path 0 2", "expected path"),
        ("turn 0 1 1e9", "a turn reaches at most"),
    ] {
        let err = parse(bad).unwrap_err().to_string();
        assert!(err.contains(why), "{bad:?}: {err}");
    }
}

#[test]
fn a_timeline_lasts_until_its_last_step_ends() {
    let m = parse("timeline 1\nat 0.5 hold w 2\nat 0 path 0 1 1, 3 2 2").unwrap();
    assert_eq!(timeline::length(&m.steps[0]), 3.0);
    assert_eq!(describe(&m.steps[0]), "playing a timeline");
    assert_eq!(describe(&Step::Turn(vec![])), "turning the camera");
}

#[test]
fn a_stagger_spaces_the_accounts_a_macro_starts_on_and_is_no_step() {
    let src = "stagger 5\ntap e\nloop 1\n";
    let m = parse(src).unwrap();
    assert_eq!((m.stagger, m.steps.len()), (5.0, 1));
    let first = std::time::Instant::now();
    assert_eq!(
        [0, 1, 3].map(|nth| m.start_of(first, nth) - first),
        [0, 5, 15].map(std::time::Duration::from_secs)
    );
    let (r, loops) = rows(src);
    assert_eq!(r[0], Row { kind: "Stagger".into(), value: "5".into() });
    assert_eq!(to_text(&r, loops), src);
}

#[test]
fn a_macro_with_no_stagger_starts_every_account_at_once() {
    let m = parse("tap e\n").unwrap();
    let first = std::time::Instant::now();
    assert_eq!((m.stagger, m.start_of(first, 4)), (0.0, first));
}

#[test]
fn a_stagger_is_one_length() {
    assert!(parse("stagger 2-4\ntap e\n").unwrap_err().message.contains("one number"));
    assert!(parse("stagger\ntap e\n").is_err());
}
