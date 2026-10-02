//! What a relay reports: the input its window received, and the line each
//! one is sent as.

/// One input the window received.
#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    Key {
        code: u16,
        down: bool,
    },
    Button {
        code: u16,
        down: bool,
    },
    /// The pointer at a point, from the window's top-left corner.
    Motion {
        x: f64,
        y: f64,
    },
    /// The wheel: 120 a notch, down (or right, when `horizontal`) positive.
    Scroll {
        horizontal: bool,
        value120: i32,
    },
}

/// An input, and when it came: seconds from the start of the recording.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub at: f64,
    pub heard: Heard,
}

impl Event {
    /// Its line in a report: `key 0.512 30 down`. Times to the millisecond;
    /// points exactly, fractions and all.
    pub fn line(&self) -> String {
        let at = self.at;
        let state = |down: bool| if down { "down" } else { "up" };
        match &self.heard {
            Heard::Key { code, down } => format!("key {at:.3} {code} {}", state(*down)),
            Heard::Button { code, down } => format!("button {at:.3} {code} {}", state(*down)),
            Heard::Motion { x, y } => format!("motion {at:.3} {x} {y}"),
            Heard::Scroll { horizontal, value120 } => {
                format!("scroll {at:.3} {} {value120}", if *horizontal { "h" } else { "v" })
            }
        }
    }

    /// A report's line read back; None for one this version does not know.
    pub fn parse(line: &str) -> Option<Event> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let (kind, rest) = words.split_first()?;
        let (at, args) = rest.split_first()?;
        let at = number(at).filter(|t| *t >= 0.0)?;
        let state = |word: &str| match word {
            "down" => Some(true),
            "up" => Some(false),
            _ => None,
        };
        let heard = match (*kind, args) {
            ("key", [code, s]) => Heard::Key { code: code.parse().ok()?, down: state(s)? },
            ("button", [code, s]) => Heard::Button { code: code.parse().ok()?, down: state(s)? },
            ("motion", [x, y]) => Heard::Motion { x: number(x)?, y: number(y)? },
            ("scroll", [axis, v]) => Heard::Scroll {
                horizontal: match *axis {
                    "h" => true,
                    "v" => false,
                    _ => return None,
                },
                value120: v.parse().ok()?,
            },
            _ => return None,
        };
        Some(Event { at, heard })
    }
}

fn number(word: &str) -> Option<f64> {
    word.parse().ok().filter(|n: &f64| n.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(at: f64, heard: Heard) -> Event {
        Event { at, heard }
    }

    #[test]
    fn each_input_is_one_line() {
        let lines = [
            (event(0.512, Heard::Key { code: 30, down: true }), "key 0.512 30 down"),
            (event(1.0, Heard::Button { code: 0x111, down: false }), "button 1.000 273 up"),
            (event(2.25, Heard::Motion { x: 640.5, y: 360.0 }), "motion 2.250 640.5 360"),
            (
                event(3.0, Heard::Scroll { horizontal: false, value120: -240 }),
                "scroll 3.000 v -240",
            ),
            (event(4.0, Heard::Scroll { horizontal: true, value120: 120 }), "scroll 4.000 h 120"),
        ];
        for (e, line) in lines {
            assert_eq!(e.line(), line);
            assert_eq!(Event::parse(line), Some(e), "{line}");
        }
    }

    #[test]
    fn a_line_this_version_does_not_know_is_passed_over() {
        for line in [
            "",
            "frob 1.0 2",
            "key 1.0 x down",
            "key 1.0 30 sideways",
            "key 1.0 30",
            "motion 1.0 5",
            "motion 1.0 inf 5",
            "scroll 1.0 d 120",
            "key -1.0 30 down",
        ] {
            assert_eq!(Event::parse(line), None, "{line:?}");
        }
    }

    #[test]
    fn a_pointer_between_pixels_keeps_its_fraction() {
        let e = event(0.0, Heard::Motion { x: 10.00390625, y: 0.5 });
        assert_eq!(Event::parse(&e.line()), Some(e));
    }
}
