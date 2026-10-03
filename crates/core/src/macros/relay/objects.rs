//! One display connection's objects, as far as a relay follows them: which
//! are the client's keyboards and pointers, learned from its own requests,
//! and what their input events say.

use std::collections::HashSet;

use super::event::Heard;
use crate::macros::wire::{DISPLAY, header, read_str, word};

#[derive(Debug, Default)]
pub struct Objects {
    registries: HashSet<u32>,
    seats: HashSet<u32>,
    keyboards: HashSet<u32>,
    pointers: HashSet<u32>,
    /// The client's relative-pointer managers, and the relative pointers
    /// it gets from them: where raw mouse movement comes.
    relative_managers: HashSet<u32>,
    relatives: HashSet<u32>,
    /// What the next axis event on each axis (vertical, horizontal) turns,
    /// in 120ths of a notch, from the wheel event just before it.
    notches: [Option<i32>; 2],
    /// The last input, and the serial or time it came with.
    last: Option<(u32, Heard)>,
}

/// What an event carries for the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// An input the window has not had yet, and the display's time on it in
    /// milliseconds (an enter carries none).
    New(Heard, Option<u32>),
    /// The input just before, again: a display sends each input to every
    /// keyboard or pointer the client has. It goes where the first went.
    Again,
}

impl Objects {
    /// A request from the client: note the registries, seats, keyboards,
    /// pointers and relative pointers it makes. Made before the display can answer, so known by
    /// the time any event on them comes back.
    pub fn request(&mut self, msg: &[u8]) {
        let (obj, op) = header(msg);
        let body = msg.get(8..).unwrap_or_default();
        match op {
            1 if obj == DISPLAY => {
                self.registries.insert(word(body, 0)); // get_registry
            }
            0 if self.registries.contains(&obj) => {
                // bind(name, interface, version, id): the id after the string.
                let padded = (word(body, 4) as usize + 3) & !3;
                let id = word(body, 12 + padded);
                match read_str(body, 4).as_deref() {
                    Some("wl_seat") => self.seats.insert(id),
                    Some("zwp_relative_pointer_manager_v1") => self.relative_managers.insert(id),
                    _ => false,
                };
            }
            0 if self.seats.contains(&obj) => {
                self.pointers.insert(word(body, 0)); // get_pointer
            }
            1 if self.seats.contains(&obj) => {
                self.keyboards.insert(word(body, 0)); // get_keyboard
            }
            1 if self.relative_managers.contains(&obj) => {
                self.relatives.insert(word(body, 0)); // get_relative_pointer
            }
            _ => {}
        }
    }

    /// An event for the client: the input it carries, if any. An object the
    /// display deletes is forgotten, so an id made again later is unknown
    /// until a request says what it is.
    pub fn event(&mut self, msg: &[u8]) -> Option<Input> {
        let (obj, op) = header(msg);
        let body = msg.get(8..).unwrap_or_default();
        let (heard, time) = self.decode(obj, op, body)?;
        // Every input event leads with its serial or its time, which the
        // copies of one input sent to each keyboard or pointer share; raw
        // movement with its time in microseconds, whose low word moves on.
        let stamp = if self.relatives.contains(&obj) { word(body, 4) } else { word(body, 0) };
        let delivery = (stamp, heard);
        if self.last.as_ref() == Some(&delivery) {
            return Some(Input::Again);
        }
        let heard = delivery.1.clone();
        self.last = Some(delivery);
        Some(Input::New(heard, time))
    }

    /// The input an event carries, and its time.
    fn decode(&mut self, obj: u32, op: u16, body: &[u8]) -> Option<(Heard, Option<u32>)> {
        if obj == DISPLAY && op == 1 {
            let id = word(body, 0); // delete_id
            for set in [
                &mut self.registries,
                &mut self.seats,
                &mut self.keyboards,
                &mut self.pointers,
                &mut self.relative_managers,
                &mut self.relatives,
            ] {
                set.remove(&id);
            }
            return None;
        }
        if self.relatives.contains(&obj) {
            // relative_motion(utime_hi, utime_lo, dx, dy, dx_unaccel, dy_unaccel):
            // the unaccelerated pair, which is what a camera reads.
            let micros = (u64::from(word(body, 0)) << 32) | u64::from(word(body, 4));
            return (op == 0).then(|| {
                let turn = Heard::Turn { dx: fixed(word(body, 16)), dy: fixed(word(body, 20)) };
                (turn, Some((micros / 1000) as u32))
            });
        }
        if self.keyboards.contains(&obj) {
            // key(serial, time, key, state); a repeat (2) is no new press.
            return match (op, word(body, 12)) {
                (3, state @ (0 | 1)) => Some((
                    Heard::Key { code: u16::try_from(word(body, 8)).ok()?, down: state == 1 },
                    Some(word(body, 4)),
                )),
                _ => None,
            };
        }
        if !self.pointers.contains(&obj) {
            return None;
        }
        let axis = |w: u32| usize::from(w == 1);
        match op {
            // enter(serial, surface, x, y) and motion(time, x, y)
            0 => Some((Heard::Motion { x: fixed(word(body, 8)), y: fixed(word(body, 12)) }, None)),
            2 => Some((
                Heard::Motion { x: fixed(word(body, 4)), y: fixed(word(body, 8)) },
                Some(word(body, 0)),
            )),
            // button(serial, time, button, state)
            3 => Some((
                Heard::Button {
                    code: u16::try_from(word(body, 8)).ok()?,
                    down: word(body, 12) == 1,
                },
                Some(word(body, 4)),
            )),
            // axis_discrete(axis, discrete) and axis_value120(axis, value120)
            8 => {
                self.notches[axis(word(body, 0))] =
                    Some((word(body, 4) as i32).saturating_mul(120));
                None
            }
            9 => {
                self.notches[axis(word(body, 0))] = Some(word(body, 4) as i32);
                None
            }
            // axis(time, axis, value): 15 to a notch when nothing said.
            4 => {
                let a = axis(word(body, 4));
                let value120 = self.notches[a]
                    .take()
                    .unwrap_or_else(|| (fixed(word(body, 8)) * 8.0).round() as i32);
                Some((Heard::Scroll { horizontal: a == 1, value120 }, Some(word(body, 0))))
            }
            _ => None,
        }
    }
}

/// A wl_fixed (24.8 fixed point) as a number.
fn fixed(raw: u32) -> f64 {
    f64::from(raw as i32) / 256.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macros::wire::{DISPLAY, message, wire_str, words};

    /// wl_fixed: 24.8 fixed point.
    fn fixed(v: f64) -> u32 {
        (v * 256.0).round() as i32 as u32
    }

    /// A client that has bound seat 3 from registry 2, with pointer 4 and
    /// keyboard 6 -- and a compositor, 8, that is neither.
    fn client() -> Objects {
        let mut o = Objects::default();
        o.request(&message(DISPLAY, 1, &words(&[2]))); // get_registry
        let bind = |name: u32, iface: &str, id: u32| {
            message(2, 0, &[words(&[name]), wire_str(iface), words(&[7, id])].concat())
        };
        o.request(&bind(5, "wl_seat", 3));
        o.request(&bind(1, "wl_compositor", 8));
        o.request(&message(3, 0, &words(&[4]))); // get_pointer
        o.request(&message(3, 1, &words(&[6]))); // get_keyboard
        o
    }

    fn key(obj: u32, code: u32, state: u32) -> Vec<u8> {
        message(obj, 3, &words(&[11, 900, code, state]))
    }

    /// A new input, at the time every test event but an enter carries.
    fn new(heard: Heard) -> Option<Input> {
        Some(Input::New(heard, Some(900)))
    }

    #[test]
    fn a_client_s_keyboard_and_pointer_are_found_from_its_requests() {
        let mut o = client();
        assert_eq!(o.event(&key(6, 30, 1)), new(Heard::Key { code: 30, down: true }));
        assert_eq!(o.event(&key(6, 30, 0)), new(Heard::Key { code: 30, down: false }));
        let motion = message(4, 2, &words(&[900, fixed(10.5), fixed(20.0)]));
        assert_eq!(o.event(&motion), new(Heard::Motion { x: 10.5, y: 20.0 }));
        assert_eq!(o.event(&key(8, 30, 1)), None, "the compositor's opcode 3 is no key");
        assert_eq!(o.event(&key(9, 30, 1)), None, "nor is an object never made");
    }

    #[test]
    fn a_key_held_down_and_repeating_is_no_new_press() {
        assert_eq!(client().event(&key(6, 30, 2)), None);
    }

    #[test]
    fn a_pointer_says_where_it_enters_and_what_is_clicked() {
        let mut o = client();
        let enter = message(4, 0, &words(&[12, 3, fixed(640.0), fixed(360.25)]));
        let entered = Heard::Motion { x: 640.0, y: 360.25 };
        assert_eq!(o.event(&enter), Some(Input::New(entered, None)), "an enter has no time");
        let button = message(4, 3, &words(&[13, 900, 0x111, 1]));
        assert_eq!(o.event(&button), new(Heard::Button { code: 0x111, down: true }));
        let frame = message(4, 5, &[]);
        assert_eq!(o.event(&frame), None);
    }

    #[test]
    fn a_wheel_turn_is_counted_in_120ths_of_a_notch() {
        let mut o = client();
        let scroll = |horizontal, value120| new(Heard::Scroll { horizontal, value120 });
        assert_eq!(o.event(&message(4, 6, &words(&[0]))), None, "axis_source");
        assert_eq!(o.event(&message(4, 9, &words(&[0, 240]))), None, "axis_value120");
        assert_eq!(o.event(&message(4, 4, &words(&[900, 0, fixed(30.0)]))), scroll(false, 240));
        assert_eq!(o.event(&message(4, 8, &words(&[1, -1i32 as u32]))), None, "axis_discrete");
        assert_eq!(o.event(&message(4, 4, &words(&[900, 1, fixed(-15.0)]))), scroll(true, -120));
        // A touchpad's: no notches, only distance, 15 to a notch.
        assert_eq!(o.event(&message(4, 4, &words(&[900, 0, fixed(7.5)]))), scroll(false, 60));
    }

    #[test]
    fn an_input_sent_to_each_of_a_client_s_keyboards_and_pointers_is_one_input() {
        let mut o = client();
        o.request(&message(3, 1, &words(&[7]))); // a second keyboard
        o.request(&message(3, 0, &words(&[9]))); // a second pointer
        let key = |obj, serial, state| message(obj, 3, &words(&[serial, 900, 30, state]));
        assert_eq!(o.event(&key(6, 20, 1)), new(Heard::Key { code: 30, down: true }));
        assert_eq!(o.event(&key(7, 20, 1)), Some(Input::Again));
        assert_eq!(o.event(&key(6, 21, 0)), new(Heard::Key { code: 30, down: false }));
        assert_eq!(o.event(&key(7, 21, 0)), Some(Input::Again));
        let motion = |obj| message(obj, 2, &words(&[901, fixed(5.0), fixed(6.0)]));
        let moved = Heard::Motion { x: 5.0, y: 6.0 };
        assert_eq!(o.event(&motion(4)), Some(Input::New(moved, Some(901))));
        assert_eq!(o.event(&message(4, 5, &[])), None, "a frame between");
        assert_eq!(o.event(&motion(9)), Some(Input::Again));
        assert_eq!(
            o.event(&key(6, 22, 1)),
            new(Heard::Key { code: 30, down: true }),
            "a new press"
        );
    }

    #[test]
    fn raw_mouse_movement_is_heard_unaccelerated_each_time_it_comes() {
        let mut o = client();
        let bind = message(
            2,
            0,
            &[words(&[9]), wire_str("zwp_relative_pointer_manager_v1"), words(&[1, 10])].concat(),
        );
        o.request(&bind);
        o.request(&message(10, 1, &words(&[11, 4]))); // get_relative_pointer
        let moved = |micros: u32| {
            let body = [0, micros, fixed(9.0), fixed(9.0), fixed(1.5), fixed(-2.0)];
            message(11, 0, &words(&body))
        };
        let turn = Heard::Turn { dx: 1.5, dy: -2.0 };
        assert_eq!(o.event(&moved(5_000)), Some(Input::New(turn.clone(), Some(5))));
        assert_eq!(o.event(&moved(13_000)), Some(Input::New(turn, Some(13))), "the same again");
        assert_eq!(o.event(&message(10, 0, &[])), None, "the manager's opcode 0 is no movement");
    }

    #[test]
    fn objects_the_display_deletes_are_forgotten() {
        let mut o = client();
        assert_eq!(o.event(&message(DISPLAY, 1, &words(&[6]))), None, "delete_id");
        assert_eq!(o.event(&key(6, 30, 1)), None);
        // The id made again, now as something else: still no keyboard.
        o.request(&message(DISPLAY, 0, &words(&[6]))); // sync's callback
        assert_eq!(o.event(&key(6, 30, 1)), None);
    }
}
