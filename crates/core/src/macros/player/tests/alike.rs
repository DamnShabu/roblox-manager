//! Clients a macro is started on together play it alike: one seed, so the
//! same random moments, picked in the same order however late each one's
//! thread wakes.

use super::*;

#[test]
fn pickers_from_one_seed_pick_the_same_moments_in_their_ranges() {
    let seed = Seed::fresh();
    let (a, b) = (seed.picker(), seed.picker());
    let picks: Vec<(f64, f64)> = (0..100).map(|_| (a(0.04, 0.12), b(0.04, 0.12))).collect();
    assert!(picks.iter().all(|(x, y)| x == y && (0.04..=0.12).contains(x)), "{picks:?}");
    assert_eq!(a(5.0, 5.0), 5.0);
    let other = Seed::fresh().picker();
    assert!((0..100).any(|_| other(0.0, 1.0) != a(0.0, 1.0)), "another run picks others");
}

/// A display that records what reaches it into `.0` and takes `.1` over
/// every press: one slow to take input, its player's thread late after.
struct Lagging(Rc<RefCell<Vec<Sent>>>, Duration);

impl Lagging {
    fn note(&self, sent: Sent) -> io::Result<()> {
        if matches!(sent, Sent::Key(_, true) | Sent::Button(_, true)) {
            std::thread::sleep(self.1);
        }
        self.0.borrow_mut().push(sent);
        Ok(())
    }
}

impl Input for Lagging {
    fn key(&mut self, code: u16, down: bool) -> io::Result<()> {
        self.note(Sent::Key(code, down))
    }
    fn motion(&mut self, dx: f64, dy: f64) -> io::Result<()> {
        self.note(Sent::Motion(dx, dy))
    }
    fn button(&mut self, code: u16, down: bool) -> io::Result<()> {
        self.note(Sent::Button(code, down))
    }
    fn move_to(&mut self, x: i32, y: i32) -> io::Result<()> {
        self.note(Sent::MoveTo(x, y))
    }
    fn scroll(&mut self, horizontal: bool, notches: i32) -> io::Result<()> {
        self.note(Sent::Scroll(horizontal, notches))
    }
}

#[test]
fn clients_started_together_send_the_same_input_however_slow_each_display_is() {
    let m = parse("repeat e 0.5 0.05-0.1\nhold f 0.1-0.2\ntype hi\nwait 0-0.1\nclick\nloop 1\n")
        .unwrap();
    let start = Instant::now() + Duration::from_millis(50);
    let seed = Seed::fresh();
    // One thread a client, as the window plays them.
    let play = |lag: Duration| {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let display = |_: &Path| -> io::Result<Box<dyn Input>> {
            Ok(Box::new(Lagging(Rc::clone(&sent), lag)))
        };
        let pick = seed.picker();
        let player = Player {
            display: Path::new("/run/user/1000/rbxmgr/rbxmgr-7.wayland"),
            running: &|| true,
            connect: &display,
            report: &|_| {},
            pick: &pick,
            now: &Local::now,
            start,
            sight: &blind(),
        };
        player.play(&m, &StopFlag::default()).unwrap();
        sent.take()
    };
    let [quick, slow] = std::thread::scope(|s| {
        [Duration::ZERO, Duration::from_millis(15)]
            .map(|lag| s.spawn(move || play(lag)))
            .map(|client| client.join().unwrap())
    });
    let taps = quick.iter().filter(|s| **s == Sent::Key(18, true)).count();
    assert!(taps >= 5, "e is tapped every 0.1 s at most, for 0.5 s: {quick:?}");
    assert_eq!(quick, slow);
}
