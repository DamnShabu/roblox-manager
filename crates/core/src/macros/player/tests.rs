use std::cell::RefCell;
use std::rc::Rc;

use chrono::TimeZone;

use super::*;
use crate::macros::grammar::parse;
use crate::macros::keys::BUTTON_LEFT;

#[derive(Clone, Debug, PartialEq)]
enum Sent {
    Key(u16, bool),
    Button(u16, bool),
    Motion(i32, i32),
}

/// An input that records what reaches it, can fail on one key's press, and
/// notes when it is let go of.
#[derive(Default)]
struct Recorder {
    sent: Rc<RefCell<Vec<Sent>>>,
    closed: Rc<RefCell<bool>>,
    fail_on: Option<u16>,
}

impl Input for Recorder {
    fn key(&mut self, code: u16, down: bool) -> io::Result<()> {
        if down && Some(code) == self.fail_on {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Broken pipe"));
        }
        self.sent.borrow_mut().push(Sent::Key(code, down));
        Ok(())
    }
    fn motion(&mut self, dx: i32, dy: i32) -> io::Result<()> {
        self.sent.borrow_mut().push(Sent::Motion(dx, dy));
        Ok(())
    }
    fn button(&mut self, code: u16, down: bool) -> io::Result<()> {
        self.sent.borrow_mut().push(Sent::Button(code, down));
        Ok(())
    }
}

impl Recorder {
    fn failing_on(code: u16) -> Self {
        Recorder { sent: Rc::default(), closed: Rc::default(), fail_on: Some(code) }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        *self.closed.borrow_mut() = true;
    }
}

fn step(text: &str) -> Step {
    parse(text).unwrap().steps.remove(0)
}

/// The top of every range, so waits are known.
fn top(_lo: f64, hi: f64) -> f64 {
    hi
}

#[test]
fn a_combo_is_pressed_in_order_held_and_released_in_reverse() {
    let mut r = Recorder::default();
    play_step(&mut r, &step("hold shift+w 1"), &StopFlag::default(), &|_, _| 0.0).unwrap();
    assert_eq!(
        *r.sent.borrow(),
        [Sent::Key(42, true), Sent::Key(17, true), Sent::Key(17, false), Sent::Key(42, false)]
    );
}

#[test]
fn typed_text_shifts_what_a_us_keyboard_shifts() {
    let mut r = Recorder::default();
    play_step(&mut r, &step("type -G"), &StopFlag::default(), &|_, _| 0.0).unwrap();
    assert_eq!(
        *r.sent.borrow(),
        [
            Sent::Key(12, true),
            Sent::Key(12, false),
            Sent::Key(42, true),
            Sent::Key(34, true),
            Sent::Key(34, false),
            Sent::Key(42, false),
        ]
    );
}

#[test]
fn a_click_at_a_point_homes_to_the_corner_first() {
    let mut r = Recorder::default();
    play_step(&mut r, &step("click 10 20"), &StopFlag::default(), &|_, _| 0.0).unwrap();
    assert_eq!(
        *r.sent.borrow(),
        [
            Sent::Motion(-100_000, -100_000),
            Sent::Motion(10, 20),
            Sent::Button(BUTTON_LEFT, true),
            Sent::Button(BUTTON_LEFT, false),
        ]
    );
}

#[test]
fn a_key_already_down_is_let_go_when_the_next_press_fails() {
    let mut r = Recorder::failing_on(17);
    assert!(play_step(&mut r, &step("hold shift+w 1"), &StopFlag::default(), &|_, _| 0.0).is_err());
    assert_eq!(*r.sent.borrow(), [Sent::Key(42, true), Sent::Key(42, false)]);
}

#[test]
fn a_stop_cuts_a_hold_short_and_still_lets_go() {
    let stop = StopFlag::default();
    stop.set();
    let mut r = Recorder::default();
    play_step(&mut r, &step("hold w 600"), &stop, &top).unwrap();
    assert_eq!(*r.sent.borrow(), [Sent::Key(17, true), Sent::Key(17, false)]);
}

/// A player whose client is up when `up` says so and whose display is `input`.
fn run(
    m: &str,
    stop: &StopFlag,
    up: &dyn Fn() -> bool,
    connect: &dyn Fn(&Path) -> io::Result<Box<dyn Input>>,
) -> (Result<(), MacroError>, Vec<String>) {
    let reports = RefCell::new(Vec::new());
    // Just before a second boundary, so a short wait shows in the clock time.
    let now =
        || Local.with_ymd_and_hms(2026, 9, 29, 11, 59, 59).unwrap() + Duration::from_millis(700);
    let player = Player {
        display: Path::new("/run/user/1000/rbxmgr/rbxmgr-7.wayland"),
        running: up,
        connect,
        report: &|line| reports.borrow_mut().push(line),
        pick: &top,
        now: &now,
    };
    let got = player.play(&parse(m).unwrap(), stop);
    (got, reports.into_inner())
}

#[test]
fn a_playing_macro_says_what_it_is_doing_and_when_a_wait_ends() {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let closed = Rc::new(RefCell::new(false));
    let (s2, c2) = (Rc::clone(&sent), Rc::clone(&closed));
    let connect = move |_: &Path| -> io::Result<Box<dyn Input>> {
        Ok(Box::new(Recorder { sent: Rc::clone(&s2), closed: Rc::clone(&c2), fail_on: None }))
    };
    let (got, reports) =
        run("start 0.4\ntap e\nwait 0\nloop 2", &StopFlag::default(), &|| true, &connect);
    got.unwrap();
    assert_eq!(
        reports,
        [
            "round 1: waiting 0s, until 12:00:00",
            "round 1: pressing e",
            "round 1: waiting 0s, until 11:59:59",
            "round 2: pressing e",
            "round 2: waiting 0s, until 11:59:59",
        ]
    );
    assert_eq!(sent.borrow().len(), 4, "two taps, each a press and a release");
    assert!(*closed.borrow(), "the display is let go of");
}

#[test]
fn a_client_that_is_not_running_gets_nothing() {
    let connect = |_: &Path| -> io::Result<Box<dyn Input>> { unreachable!("never connects") };
    let (got, _) = run("tap e", &StopFlag::default(), &|| false, &connect);
    assert_eq!(got, Err(MacroError::NotRunning));
}

#[test]
fn a_client_in_a_normal_window_is_named_as_such() {
    let connect = |_: &Path| -> io::Result<Box<dyn Input>> { Err(io::ErrorKind::NotFound.into()) };
    assert_eq!(
        run("tap e", &StopFlag::default(), &|| true, &connect).0,
        Err(MacroError::NotNested)
    );
    let refused =
        |_: &Path| -> io::Result<Box<dyn Input>> { Err(io::ErrorKind::ConnectionRefused.into()) };
    assert_eq!(
        run("tap e", &StopFlag::default(), &|| true, &refused).0,
        Err(MacroError::NotNested)
    );
}

#[test]
fn a_display_that_goes_away_ends_the_macro_with_a_reason() {
    let closed = Rc::new(RefCell::new(false));
    let c2 = Rc::clone(&closed);
    let connect = move |_: &Path| -> io::Result<Box<dyn Input>> {
        Ok(Box::new(Recorder { sent: Rc::default(), closed: Rc::clone(&c2), fail_on: Some(18) }))
    };
    let (got, _) = run("tap e", &StopFlag::default(), &|| true, &connect);
    assert!(
        matches!(&got, Err(MacroError::WentAway(why)) if why.contains("Broken pipe")),
        "{got:?}"
    );
    assert!(*closed.borrow());
}

#[test]
fn a_stopped_macro_sends_nothing_more() {
    let stop = StopFlag::default();
    stop.set();
    let sent = Rc::new(RefCell::new(Vec::new()));
    let s2 = Rc::clone(&sent);
    let connect = move |_: &Path| -> io::Result<Box<dyn Input>> {
        Ok(Box::new(Recorder { sent: Rc::clone(&s2), closed: Rc::default(), fail_on: None }))
    };
    run("tap e\nclick right", &stop, &|| true, &connect).0.unwrap();
    assert!(sent.borrow().is_empty());
}

#[test]
fn a_random_pick_stays_in_its_range() {
    for _ in 0..100 {
        let p = random_pick(0.04, 0.12);
        assert!((0.04..=0.12).contains(&p), "{p}");
    }
    assert_eq!(random_pick(5.0, 5.0), 5.0);
}
