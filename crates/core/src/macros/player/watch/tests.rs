use super::*;
use crate::macros::grammar::parse;

const RED: [u8; 3] = [255, 0, 0];
const BLACK: [u8; 3] = [0, 0, 0];

fn pixel(rgb: [u8; 3]) -> Image {
    Image { width: 1, height: 1, rgb: rgb.to_vec() }
}

/// Frames shown one a look, in turn; past the last, looking is done.
struct Frames {
    frames: Vec<io::Result<Image>>,
    done: StopFlag,
    asked: Vec<Area>,
}

impl Eyes for Frames {
    fn look(&mut self, area: Area) -> io::Result<Image> {
        self.asked.push(area);
        if self.frames.len() <= 1 {
            self.done.set();
        }
        if self.frames.is_empty() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "none left"));
        }
        self.frames.remove(0)
    }
}

/// Watch `text`'s whens through `frames`; how often the player was rung,
/// and what was seen.
fn watched(text: &str, frames: Vec<io::Result<Image>>) -> (u64, Seen, Vec<Area>) {
    let m = parse(text).unwrap();
    let looks = Looks::new(&m.handlers, &|_| Ok(pixel(RED))).unwrap();
    let (stop, done, seen) = (StopFlag::default(), StopFlag::default(), Seen::default());
    let mut eyes = Frames { frames, done: done.clone(), asked: Vec::new() };
    watch(&mut eyes, &looks, &seen, &stop, &done);
    (stop.rings(), seen, eyes.asked)
}

#[test]
fn a_when_plays_once_each_time_what_it_waits_for_appears() {
    let frames = [BLACK, RED, RED, BLACK, RED].map(|c| Ok(pixel(c))).into();
    let (rings, seen, asked) = watched("when color 7 9 #ff0000\ndo tap e\n", frames);
    assert_eq!(rings, 2, "on, staying on, off, and on again");
    assert_eq!(seen.next().unwrap(), Some(0));
    assert_eq!(seen.next().unwrap(), None, "due once, however often it saw");
    assert_eq!(asked[0], Area { x: 7, y: 9, w: 1, h: 1 }, "just the pixel it looks at");
}

#[test]
fn a_when_sees_what_is_there_from_the_first_look() {
    let (rings, ..) = watched("when color 0 0 #ff0000\ndo tap e\n", vec![Ok(pixel(RED))]);
    assert_eq!(rings, 1);
}

#[test]
fn a_when_not_waits_to_see_it_go() {
    let text = "when not color 0 0 #ff0000\ndo tap e\n";
    let (rings, ..) = watched(text, [BLACK, BLACK].map(|c| Ok(pixel(c))).into());
    assert_eq!(rings, 0, "never there, so never gone");
    let (rings, ..) = watched(text, [RED, BLACK].map(|c| Ok(pixel(c))).into());
    assert_eq!(rings, 1);
}

#[test]
fn an_image_is_looked_for_around_where_it_was_picked() {
    let text = "when image coin 10 20\ndo tap e\n";
    let (_, _, asked) = watched(text, vec![Ok(pixel(BLACK))]);
    let s = SLACK as u32;
    assert_eq!(asked[0], Area { x: 10 - SLACK, y: 20 - SLACK, w: 1 + 2 * s, h: 1 + 2 * s });
}

#[test]
fn a_frame_slow_to_come_is_waited_on_but_a_failed_display_stops_the_macro() {
    let slow = vec![Err(io::Error::new(io::ErrorKind::TimedOut, "late")), Ok(pixel(RED))];
    let (rings, seen, _) = watched("when color 0 0 #ff0000\ndo tap e\n", slow);
    assert_eq!((rings, seen.next().unwrap()), (1, Some(0)));
    let gone = vec![Err(io::Error::new(io::ErrorKind::BrokenPipe, "its display closed"))];
    let (rings, seen, _) = watched("when color 0 0 #ff0000\ndo tap e\n", gone);
    assert_eq!(rings, 1);
    let e = seen.next().unwrap_err().to_string();
    assert!(e.contains("its display closed"), "{e}");
}

#[test]
fn a_when_naming_an_image_there_is_not_cannot_look() {
    let m = parse("when image gone 1 1\ndo tap e\n").unwrap();
    let got = Looks::new(&m.handlers, &|name| Err(format!("no image named {name}")));
    assert_eq!(got.err().as_deref(), Some("no image named gone"));
}
