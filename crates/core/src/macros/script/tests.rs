use std::io::Cursor;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::Value;

use super::*;
use crate::macros::sight::Area;

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<String>>>);

impl Recorder {
    fn sent(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
    fn note(&self, what: String) -> io::Result<()> {
        self.0.lock().unwrap().push(what);
        Ok(())
    }
}

impl Input for Recorder {
    fn key(&mut self, code: u16, down: bool) -> io::Result<()> {
        self.note(format!("key {code} {}", if down { "down" } else { "up" }))
    }
    fn motion(&mut self, dx: f64, dy: f64) -> io::Result<()> {
        self.note(format!("motion {dx} {dy}"))
    }
    fn button(&mut self, code: u16, down: bool) -> io::Result<()> {
        self.note(format!("button {code:#x} {}", if down { "down" } else { "up" }))
    }
    fn move_to(&mut self, x: i32, y: i32) -> io::Result<()> {
        self.note(format!("move_to {x} {y}"))
    }
    fn scroll(&mut self, horizontal: bool, notches: i32) -> io::Result<()> {
        self.note(format!("scroll {horizontal} {notches}"))
    }
}

/// A 20 by 10 window, black but for a 2 by 2 red square at (12, 4).
struct Window;

impl Window {
    fn frame() -> Image {
        let (width, height) = (20u32, 10u32);
        let mut rgb = vec![0; (width * height * 3) as usize];
        for (x, y) in [(12, 4), (13, 4), (12, 5), (13, 5)] {
            let i = ((y * width + x) * 3) as usize;
            rgb[i] = 255;
        }
        Image { width, height, rgb }
    }
}

impl Eyes for Window {
    fn look(&mut self, area: Area) -> io::Result<Image> {
        let whole = Window::frame();
        let on = Area { x: 0, y: 0, w: whole.width, h: whole.height };
        let w = area.w.min(whole.width.saturating_sub(area.x as u32));
        let h = area.h.min(whole.height.saturating_sub(area.y as u32));
        let part = Area { x: area.x, y: area.y, w, h };
        whole.crop(on, part).ok_or_else(|| io::Error::other("off the window"))
    }
}

fn red() -> Image {
    Image { width: 2, height: 2, rgb: [255, 0, 0].repeat(4) }
}

fn image(name: &str) -> Result<Image, String> {
    if name == "red" { Ok(red()) } else { Err(format!("there is no image named {name}")) }
}

fn open() -> io::Result<Box<dyn Eyes + Send>> {
    Ok(Box::new(Window))
}

/// Serve `requests` (one a line) and return each reply line, and the
/// bytes after any.
fn serve(requests: &str, input: &Recorder) -> Vec<Value> {
    let mut rec = input.clone();
    let (stop, names) = (StopFlag::default(), vec!["red".to_owned()]);
    let running = || true;
    let mut session = Session::new(&mut rec, &open, &image, &names, &running, &stop);
    let mut out = Vec::new();
    session.serve(&mut Cursor::new(requests), &mut out).unwrap();
    session.release_all().unwrap();
    let mut replies = Vec::new();
    let mut rest = out.as_slice();
    while let Some(end) = rest.iter().position(|b| *b == b'\n') {
        let line: Value = serde_json::from_slice(&rest[..end]).unwrap();
        let after = line.get("bytes").and_then(Value::as_u64).unwrap_or(0) as usize;
        rest = &rest[end + 1 + after..];
        replies.push(line);
    }
    replies
}

#[test]
fn a_script_says_what_it_does_in_its_first_comment_or_docstring() {
    assert_eq!(
        about("#!/usr/bin/env python3\n# Farms coins.\nimport x"),
        Some("Farms coins.".into())
    );
    assert_eq!(about("\n\"\"\"Clicks the boss.\n\nMore.\"\"\""), Some("Clicks the boss.".into()));
    assert_eq!(about("'''Short.'''\n"), Some("Short.".into()));
    assert_eq!(about("import rbxmgr\n# late"), None);
    assert_eq!(about("#\n"), None);
}

#[test]
fn the_folder_lists_its_python_files_but_not_shared_modules() {
    let dir = tempfile::tempdir().unwrap();
    let scripts_dir = dir.path().join("advanced");
    assert!(scripts(&scripts_dir).unwrap().is_empty(), "no folder yet, no scripts");
    prepare(&scripts_dir).unwrap();
    fs::write(scripts_dir.join("farm.py"), "# Farms.\n").unwrap();
    fs::write(scripts_dir.join("_common.py"), "").unwrap();
    fs::write(scripts_dir.join("notes.txt"), "").unwrap();
    fs::create_dir(scripts_dir.join("dir.py")).unwrap();
    let found = scripts(&scripts_dir).unwrap();
    let names: Vec<_> = found.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["example", "farm"]);
    assert_eq!(found[1].about.as_deref(), Some("Farms."));
    fs::write(scripts_dir.join("example.py"), "# mine now\n").unwrap();
    prepare(&scripts_dir).unwrap();
    assert_eq!(fs::read_to_string(scripts_dir.join("example.py")).unwrap(), "# mine now\n");
}

#[test]
fn requests_press_keys_and_whatever_is_left_down_is_let_go_of() {
    let input = Recorder::default();
    let replies = serve(
        concat!(
            r#"{"op":"tap","keys":["e"],"secs":0}"#,
            "\n",
            r#"{"op":"press","keys":["shift","w"]}"#,
            "\n",
            r#"{"op":"click","x":5,"y":6,"button":"mouse2"}"#,
            "\n",
            r#"{"op":"move","dx":10,"dy":-3}"#,
            "\n",
        ),
        &input,
    );
    assert!(replies.iter().all(|r| r.get("error").is_none()), "{replies:?}");
    assert_eq!(
        input.sent(),
        [
            "key 18 down",
            "key 18 up",
            "key 42 down",
            "key 17 down",
            "move_to 5 6",
            "button 0x111 down",
            "button 0x111 up",
            "motion 10 -3",
            // The run's end: the last pressed first.
            "key 17 up",
            "key 42 up",
        ]
    );
}

#[test]
fn a_wrong_request_is_answered_with_why_and_the_script_carries_on() {
    let input = Recorder::default();
    let replies = serve(
        "{\"op\":\"tap\",\"keys\":[\"nokey\"]}\nnot json\n{\"op\":\"fly\"}\n\
         {\"op\":\"find\",\"image\":\"blue\"}\n{\"op\":\"tap\",\"keys\":[\"e\"],\"secs\":0}\n",
        &input,
    );
    let errors: Vec<_> = replies.iter().filter_map(|r| r.get("error")?.as_str()).collect();
    assert_eq!(errors.len(), 4, "{replies:?}");
    assert_eq!(errors[0], "\"nokey\" is no key");
    assert_eq!(errors[3], "there is no image named blue");
    assert_eq!(input.sent(), ["key 18 down", "key 18 up"]);
}

#[test]
fn a_script_sees_the_window_through_the_copy_of_its_frame() {
    let replies = serve(
        concat!(
            r#"{"op":"find","image":"red"}"#,
            "\n",
            r#"{"op":"find","image":"red","area":[0,0,8,8]}"#,
            "\n",
            r#"{"op":"pixel","x":13,"y":5}"#,
            "\n",
            r#"{"op":"frame","area":[10,3,4,3]}"#,
            "\n",
            r#"{"op":"images"}"#,
            "\n",
        ),
        &Recorder::default(),
    );
    let ok: Vec<_> = replies.iter().map(|r| r["ok"].clone()).collect();
    assert_eq!(ok[0], serde_json::json!([12, 4, 2, 2]));
    assert_eq!(ok[1], Value::Null, "not in that area");
    assert_eq!(ok[2], serde_json::json!([255, 0, 0]));
    assert_eq!(ok[3], serde_json::json!({"x": 10, "y": 3, "width": 4, "height": 3}));
    assert_eq!(replies[3]["bytes"], 4 * 3 * 3);
    assert_eq!(ok[4], serde_json::json!(["red"]));
}

/// Python, when this machine has it: the tests that run a real script
/// through the real helper say so and pass without it.
fn python() -> Option<Python> {
    let has = Command::new("python3").arg("--version").output().is_ok_and(|o| o.status.success());
    if !has {
        eprintln!("no python3 here: the end-to-end script tests do not run");
    }
    has.then(|| Python::new(PathBuf::new()))
}

/// What a script sent, what it printed, and how it ended.
type Ran = (Vec<String>, Vec<String>, Result<(), MacroError>);

/// Run `source` as a script, with `stop` set after `stop_after` seconds if
/// given: what it sent, what it printed, and how it ended.
fn run_script(source: &str, stop_after: Option<f64>) -> Option<Ran> {
    let mut python = python()?;
    let dir = tempfile::tempdir().unwrap();
    python.helper_dir = dir.path().join("helper");
    let script = dir.path().join("it.py");
    fs::write(&script, source).unwrap();
    let input = Recorder::default();
    let printed = Mutex::new(Vec::new());
    let report = |line: String| printed.lock().unwrap().push(line);
    let connect = {
        let input = input.clone();
        move |_: &Path| -> io::Result<Box<dyn Input>> { Ok(Box::new(input.clone())) }
    };
    let open = |_: &Path| open();
    let names = vec!["red".to_owned()];
    let run = ScriptRun {
        display: Path::new("display"),
        running: &|| true,
        connect: &connect,
        open: &open,
        image: &image,
        image_names: &names,
        report: &report,
        interpreter: &python,
        account: ("alt one", 42),
    };
    let stop = StopFlag::default();
    let ended = thread::scope(|s| {
        if let Some(secs) = stop_after {
            let stop = stop.clone();
            s.spawn(move || {
                thread::sleep(Duration::from_secs_f64(secs));
                stop.set();
            });
        }
        run.run(&script, &stop)
    });
    let printed = printed.into_inner().unwrap();
    Some((input.sent(), printed, ended))
}

#[test]
fn a_python_script_plays_through_the_helper() {
    let source = r#"
import rbxmgr as rb
print("hello", rb.account, rb.user_id)
rb.tap("ctrl+w", secs=0)
rb.key_down("shift")
at = rb.find_center("red")
print("red at", at, "pixel", rb.pixel(12, 4))
try:
    rb.tap("nokey")
except rb.MacroError as e:
    print("refused:", e)
f = rb.frame((10, 3, 4, 3))
print("frame", f.width, f.height, f.pixel(12, 4), f.pixel(0, 0))
"#;
    let Some((sent, printed, ended)) = run_script(source, None) else { return };
    assert_eq!(ended, Ok(()), "{printed:?}");
    assert_eq!(
        printed,
        [
            "hello alt one 42",
            "red at (13, 5) pixel (255, 0, 0)",
            "refused: \"nokey\" is no key",
            "frame 4 3 (255, 0, 0) None",
        ]
    );
    assert_eq!(
        sent,
        ["key 29 down", "key 17 down", "key 17 up", "key 29 up", "key 42 down", "key 42 up"]
    );
}

#[test]
fn a_script_that_fails_is_told_of_by_its_last_words() {
    let Some((_, printed, ended)) =
        run_script("import rbxmgr\nraise ValueError('no coins')\n", None)
    else {
        return;
    };
    assert_eq!(ended, Err(MacroError::Script("ValueError: no coins".into())));
    assert!(printed.iter().any(|l| l.starts_with("Traceback")), "{printed:?}");
}

#[test]
fn stop_ends_a_script_wherever_it_is_and_lets_go_of_its_keys() {
    let looping = "import rbxmgr as rb\nrb.key_down('w')\nwhile True:\n    rb.wait(0.05)\n";
    let sleeping = "import time, rbxmgr as rb\nrb.key_down('w')\ntime.sleep(60)\n";
    // Ignores the polite ask: ended outright, a grace later.
    let stubborn = "import signal, time, rbxmgr as rb\nsignal.signal(signal.SIGTERM, \
                    signal.SIG_IGN)\nrb.key_down('w')\ntime.sleep(60)\n";
    for source in [looping, sleeping, stubborn] {
        let started = Instant::now();
        let Some((sent, printed, ended)) = run_script(source, Some(0.5)) else { return };
        assert_eq!(ended, Ok(()), "{printed:?}");
        assert_eq!(sent, ["key 17 down", "key 17 up"]);
        assert!(started.elapsed() < Duration::from_secs(5), "{source}");
    }
}
