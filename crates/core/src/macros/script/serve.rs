//! Answering a script: one JSON request a line on its stdout, one JSON
//! reply a line on its stdin -- `{"ok": ...}`, or `{"error": "..."}` for a
//! request it can fix. A copy of the frame is the one reply with more after
//! it: `"bytes": n` on the line, and that many bytes of pixels behind it.
//!
//! Only what the display does ends the conversation: input it would not
//! take, or a frame it could not copy. A script that asks for something
//! wrong is told, and carries on.

use std::io::{self, BufRead, Write};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::macros::grammar::TAP_PRESS;
use crate::macros::grammar::when::IMAGE_LEAST;
use crate::macros::keys::{self, SHIFT};
use crate::macros::player::Input;
use crate::macros::sight::{Area, Eyes, Image, find};
use crate::stop::StopFlag;

/// The whole window, as an area to copy: the display keeps a copy to the
/// part on it, so this is all of it at any size.
const WHOLE: Area = Area { x: 0, y: 0, w: 1 << 16, h: 1 << 16 };
/// How long a look waits out frames that do not come -- a client loading
/// draws nothing for a while -- before the script is told it cannot see.
const LOOK_FOR: Duration = Duration::from_secs(3);
/// The longest a single tap or hold may last: anything longer is a script
/// that meant milliseconds and wrote them as seconds.
const LONGEST_HOLD: f64 = 600.0;

/// What a script's requests reach: the client's display, through its
/// input and its eyes, and the picked images.
pub struct Session<'a> {
    pub input: &'a mut dyn Input,
    /// Eyes on the display, opened at the first look: a script that only
    /// presses keys never asks the display for a frame.
    pub open: &'a dyn Fn() -> io::Result<Box<dyn Eyes + Send>>,
    pub image: &'a dyn Fn(&str) -> Result<Image, String>,
    pub image_names: &'a [String],
    pub running: &'a dyn Fn() -> bool,
    pub stop: &'a StopFlag,
    eyes: Option<Box<dyn Eyes + Send>>,
    /// Keys and buttons a request pressed and left down, in order.
    down: Vec<u16>,
}

/// A request answered: a value, and for a frame its pixels.
enum Answer {
    Ok(Value, Option<Vec<u8>>),
    Refused(String),
}

impl<'a> Session<'a> {
    pub fn new(
        input: &'a mut dyn Input,
        open: &'a dyn Fn() -> io::Result<Box<dyn Eyes + Send>>,
        image: &'a dyn Fn(&str) -> Result<Image, String>,
        image_names: &'a [String],
        running: &'a dyn Fn() -> bool,
        stop: &'a StopFlag,
    ) -> Self {
        Session { input, open, image, image_names, running, stop, eyes: None, down: Vec::new() }
    }

    /// Answer every request until the script stops asking (its stdout
    /// closed: it ended or was ended), it stops listening, or `stop` is
    /// set. An error is the display's, and ends the run.
    pub fn serve(&mut self, requests: &mut dyn BufRead, replies: &mut dyn Write) -> io::Result<()> {
        let mut line = String::new();
        loop {
            line.clear();
            // A pipe that breaks is the script going, as an end of it is.
            match requests.read_line(&mut line) {
                Ok(0) | Err(_) => return Ok(()),
                Ok(_) => {}
            }
            if self.stop.is_set() {
                return Ok(());
            }
            let answer = match serde_json::from_str::<Value>(&line) {
                Ok(request) => self.answer(&request)?,
                Err(e) => Answer::Refused(format!("not a request: {e}")),
            };
            let sent = match answer {
                Answer::Ok(value, None) => reply(replies, &json!({ "ok": value }), &[]),
                Answer::Ok(value, Some(bytes)) => {
                    reply(replies, &json!({ "ok": value, "bytes": bytes.len() }), &bytes)
                }
                Answer::Refused(why) => reply(replies, &json!({ "error": why }), &[]),
            };
            if sent.is_err() {
                return Ok(());
            }
        }
    }

    /// Let go of everything a request left down, the last pressed first.
    /// Every one is let go of, whatever fails; the first failure is the one
    /// returned.
    pub fn release_all(&mut self) -> io::Result<()> {
        let mut result = Ok(());
        while let Some(code) = self.down.pop() {
            let released = send(self.input, code, false);
            if result.is_ok() {
                result = released;
            }
        }
        result
    }

    fn answer(&mut self, r: &Value) -> io::Result<Answer> {
        let op = r.get("op").and_then(Value::as_str).unwrap_or_default();
        let done = || Ok(Answer::Ok(Value::Null, None));
        macro_rules! or_refuse {
            ($e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(why) => return Ok(Answer::Refused(why)),
                }
            };
        }
        match op {
            "tap" => {
                let codes = or_refuse!(codes(r));
                let secs = match r.get("secs") {
                    None | Some(Value::Null) => TAP_PRESS.0,
                    Some(_) => or_refuse!(number(r, "secs")),
                };
                if !(0.0..=LONGEST_HOLD).contains(&secs) {
                    return Ok(Answer::Refused(format!(
                        "a hold lasts 0 to {LONGEST_HOLD} seconds, not {secs}"
                    )));
                }
                self.hold(&codes, secs)?;
                done()
            }
            "press" => {
                for code in or_refuse!(codes(r)) {
                    if !self.down.contains(&code) {
                        send(self.input, code, true)?;
                        self.down.push(code);
                    }
                }
                done()
            }
            "release" => {
                for code in or_refuse!(codes(r)) {
                    if let Some(i) = self.down.iter().position(|c| *c == code) {
                        self.down.remove(i);
                        send(self.input, code, false)?;
                    }
                }
                done()
            }
            "release_all" => {
                self.release_all()?;
                done()
            }
            "type" => {
                let text = or_refuse!(string(r, "text"));
                let typed: Option<Vec<_>> = text.chars().map(keys::char_code).collect();
                let Some(typed) = typed else {
                    return Ok(Answer::Refused(format!(
                        "a US keyboard cannot type all of {text:?}"
                    )));
                };
                for (code, shifted) in typed {
                    let codes: &[u16] = if shifted { &[SHIFT, code] } else { &[code] };
                    self.hold(codes, TAP_PRESS.0)?;
                    if self.stop.wait(crate::macros::grammar::TYPE_GAP.0) {
                        break;
                    }
                }
                done()
            }
            "click" => {
                let name = r.get("button").and_then(Value::as_str).unwrap_or("mouse1");
                let button = keys::key_code(name).filter(|c| keys::is_button(*c));
                let Some(button) = button else {
                    return Ok(Answer::Refused(format!("{name} is no mouse button")));
                };
                match (r.get("x"), r.get("y")) {
                    (None | Some(Value::Null), None | Some(Value::Null)) => {}
                    _ => {
                        let (x, y) = (or_refuse!(int(r, "x")), or_refuse!(int(r, "y")));
                        self.input.move_to(x, y)?;
                    }
                }
                self.hold(&[button], TAP_PRESS.0)?;
                done()
            }
            "move_to" => {
                let (x, y) = (or_refuse!(int(r, "x")), or_refuse!(int(r, "y")));
                self.input.move_to(x, y)?;
                done()
            }
            "move" => {
                let (dx, dy) = (or_refuse!(int(r, "dx")), or_refuse!(int(r, "dy")));
                self.input.motion(f64::from(dx), f64::from(dy))?;
                done()
            }
            "scroll" => {
                let notches = or_refuse!(int(r, "notches"));
                let horizontal = r.get("horizontal").and_then(Value::as_bool).unwrap_or(false);
                self.input.scroll(horizontal, notches)?;
                done()
            }
            "wait" => {
                let secs = or_refuse!(number(r, "secs"));
                Ok(Answer::Ok(Value::Bool(self.stop.wait(secs.max(0.0))), None))
            }
            "find" => {
                let name = or_refuse!(string(r, "image"));
                let template = or_refuse!((self.image)(&name));
                let within = or_refuse!(area(r));
                let least = match r.get("least") {
                    None | Some(Value::Null) => IMAGE_LEAST,
                    Some(_) => or_refuse!(number(r, "least")),
                };
                let shows = within.unwrap_or(WHOLE).on_display();
                let frame = or_refuse!(self.look(shows)?);
                let at = find(&frame, shows, &template, shows, least, None);
                let (w, h) = (template.width, template.height);
                Ok(Answer::Ok(at.map_or(Value::Null, |(x, y)| json!([x, y, w, h])), None))
            }
            "pixel" => {
                let (x, y) = (or_refuse!(int(r, "x")), or_refuse!(int(r, "y")));
                if x < 0 || y < 0 {
                    return Ok(Answer::Refused(format!("({x}, {y}) is off the window")));
                }
                let shows = Area { x, y, w: 1, h: 1 };
                let frame = or_refuse!(self.look(shows)?);
                match frame.pixel(0, 0) {
                    Some([r, g, b]) => Ok(Answer::Ok(json!([r, g, b]), None)),
                    None => Ok(Answer::Refused(format!("({x}, {y}) is off the window"))),
                }
            }
            "frame" => {
                let shows = or_refuse!(area(r)).unwrap_or(WHOLE).on_display();
                let frame = or_refuse!(self.look(shows)?);
                let shape = json!({
                    "x": shows.x, "y": shows.y, "width": frame.width, "height": frame.height,
                });
                Ok(Answer::Ok(shape, Some(frame.rgb)))
            }
            "images" => Ok(Answer::Ok(json!(self.image_names), None)),
            "running" => Ok(Answer::Ok(Value::Bool((self.running)()), None)),
            other => Ok(Answer::Refused(format!("no such request: {other:?}"))),
        }
    }

    /// Press `codes` in order, hold them `secs` (or until stopped), and let
    /// go in reverse -- every one that went down, whatever failed. One the
    /// script already holds down stays down.
    fn hold(&mut self, codes: &[u16], secs: f64) -> io::Result<()> {
        let mut pressed = Vec::new();
        let mut result = Ok(());
        for &code in codes.iter().filter(|c| !self.down.contains(c)) {
            match send(self.input, code, true) {
                Ok(()) => pressed.push(code),
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        if result.is_ok() {
            self.stop.wait(secs);
        }
        for &code in pressed.iter().rev() {
            let released = send(self.input, code, false);
            if result.is_ok() {
                result = released;
            }
        }
        result
    }

    /// A copy of `area` of the display, or why the script cannot have one
    /// (it is told, and may ask again). A display that fails outright is
    /// the error, and ends the run.
    fn look(&mut self, area: Area) -> io::Result<Result<Image, String>> {
        if area.w == 0 || area.h == 0 {
            return Ok(Err("that area is empty, or off the window".into()));
        }
        let eyes = match &mut self.eyes {
            Some(eyes) => eyes,
            None => match (self.open)() {
                Ok(eyes) => self.eyes.insert(eyes),
                Err(e) => return Ok(Err(format!("cannot look at its window: {e}"))),
            },
        };
        let since = Instant::now();
        loop {
            match eyes.look(area) {
                Ok(frame) => return Ok(Ok(frame)),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::ResourceBusy
                    ) =>
                {
                    if since.elapsed() >= LOOK_FOR || self.stop.is_set() {
                        return Ok(Err(format!("cannot see its window: {e}")));
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}

fn reply(out: &mut dyn Write, line: &Value, bytes: &[u8]) -> io::Result<()> {
    let mut text = line.to_string();
    text.push('\n');
    out.write_all(text.as_bytes())?;
    out.write_all(bytes)?;
    out.flush()
}

/// A key to the keyboard, a mouse button to the pointer.
fn send(input: &mut dyn Input, code: u16, down: bool) -> io::Result<()> {
    if keys::is_button(code) { input.button(code, down) } else { input.key(code, down) }
}

fn codes(r: &Value) -> Result<Vec<u16>, String> {
    let names = r.get("keys").and_then(Value::as_array).ok_or("which keys?")?;
    if names.is_empty() {
        return Err("which keys?".into());
    }
    names
        .iter()
        .map(|n| {
            let name = n.as_str().ok_or_else(|| format!("{n} is no key name"))?;
            keys::key_code(name).ok_or_else(|| format!("{name:?} is no key"))
        })
        .collect()
}

fn number(r: &Value, field: &str) -> Result<f64, String> {
    r.get(field)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{field} must be a number"))
}

fn int(r: &Value, field: &str) -> Result<i32, String> {
    let n = number(r, field)?.round();
    if (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&n) {
        Ok(n as i32)
    } else {
        Err(format!("{field} is out of range"))
    }
}

fn string(r: &Value, field: &str) -> Result<String, String> {
    r.get(field).and_then(Value::as_str).map(str::to_owned).ok_or(format!("{field} must be text"))
}

/// `[x, y, w, h]`, or None for the whole window.
fn area(r: &Value) -> Result<Option<Area>, String> {
    let Some(v) = r.get("area").filter(|v| !v.is_null()) else { return Ok(None) };
    let parts: Option<Vec<i64>> =
        v.as_array().map(|a| a.iter().filter_map(Value::as_i64).collect());
    match parts.as_deref() {
        Some(&[x, y, w, h]) if w > 0 && h > 0 => Ok(Some(Area {
            x: i32::try_from(x).map_err(|_| "area x is out of range")?,
            y: i32::try_from(y).map_err(|_| "area y is out of range")?,
            w: u32::try_from(w).map_err(|_| "area width is out of range")?,
            h: u32::try_from(h).map_err(|_| "area height is out of range")?,
        })),
        _ => Err("an area is [x, y, width, height], each a whole number".into()),
    }
}
