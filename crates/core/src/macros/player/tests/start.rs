//! Where a macro starts on its clock: at the moment it was given, shared by
//! the clients started together, or its own turn after them.

use super::*;

/// A display that notes when its first input arrived, after taking `slow`
/// to connect to.
fn first_input_into(
    first: &Rc<RefCell<Option<Instant>>>,
    slow: Duration,
) -> impl Fn(&Path) -> io::Result<Box<dyn Input>> {
    let first = Rc::clone(first);
    move |_: &Path| -> io::Result<Box<dyn Input>> {
        std::thread::sleep(slow);
        Ok(Box::new(FirstInput(Rc::clone(&first))))
    }
}

struct FirstInput(Rc<RefCell<Option<Instant>>>);

impl FirstInput {
    fn note(&self) -> io::Result<()> {
        self.0.borrow_mut().get_or_insert_with(Instant::now);
        Ok(())
    }
}

impl Input for FirstInput {
    fn key(&mut self, _: u16, _: bool) -> io::Result<()> {
        self.note()
    }
    fn motion(&mut self, _: f64, _: f64) -> io::Result<()> {
        self.note()
    }
    fn button(&mut self, _: u16, _: bool) -> io::Result<()> {
        self.note()
    }
    fn move_to(&mut self, _: i32, _: i32) -> io::Result<()> {
        self.note()
    }
    fn scroll(&mut self, _: bool, _: i32) -> io::Result<()> {
        self.note()
    }
}

#[test]
fn clients_started_together_play_from_one_moment_however_slow_each_is_to_reach() {
    let start = Instant::now() + Duration::from_millis(150);
    // One thread a client, as the window plays them.
    let first_input = |slow: Duration| {
        let first = Rc::default();
        let player = Player {
            display: Path::new("/run/user/1000/rbxmgr/rbxmgr-7.wayland"),
            running: &|| true,
            connect: &first_input_into(&first, slow),
            report: &|_| {},
            pick: &top,
            now: &Local::now,
            start,
        };
        player.play(&parse("tap e\nloop 1\n").unwrap(), &StopFlag::default()).unwrap();
        first.borrow().unwrap()
    };
    let firsts: Vec<Instant> = std::thread::scope(|s| {
        let clients = [Duration::ZERO, Duration::from_millis(80)]
            .map(|slow| s.spawn(move || first_input(slow)));
        clients.into_iter().map(|c| c.join().unwrap()).collect()
    });
    for first in firsts {
        assert!(first >= start, "pressed {:?} early", start - first);
        assert!(first - start < Duration::from_millis(20), "pressed {:?} late", first - start);
    }
}

#[test]
fn a_client_waiting_its_turn_says_until_when() {
    let (sent, closed) = (Rc::default(), Rc::default());
    let stop = StopFlag::default();
    let reports = RefCell::new(Vec::new());
    let now =
        || Local.with_ymd_and_hms(2026, 9, 29, 11, 59, 59).unwrap() + Duration::from_millis(700);
    let player = Player {
        display: Path::new("/run/user/1000/rbxmgr/rbxmgr-7.wayland"),
        running: &|| true,
        connect: &recording_into(&sent, &closed, None),
        // Stopped as soon as it is told: the turn is never waited out.
        report: &|line: String| {
            stop.set();
            reports.borrow_mut().push(line);
        },
        pick: &top,
        now: &now,
        start: Instant::now() + Duration::from_secs(10),
    };
    player.play(&parse("tap e\n").unwrap(), &stop).unwrap();
    assert_eq!(reports.into_inner(), ["waiting 10s for its turn, until 12:00:09"]);
}
