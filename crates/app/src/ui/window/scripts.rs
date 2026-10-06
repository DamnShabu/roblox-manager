//! The macros pane's Advanced tab: the Python scripts in the advanced
//! macros folder, and running them on accounts.

use std::path::{Path, PathBuf};

use adw::prelude::*;
use gtk::{Align, gio};
use rbxmgr_core::macros::script::{self, Python, Script, ScriptRun};
use rbxmgr_core::macros::sight::Eyes;
use rbxmgr_core::macros::sight::screencopy::Screencopy;
use rbxmgr_core::macros::{Input, StopFlag, VirtualInput, nested};
use rbxmgr_core::types::{Profile, UserId};

use super::Window;
use crate::state::MacroRun;
use crate::ui::accounts::leader::placeholder;
use crate::ui::macros::images;
use crate::ui::widgets::{self, Btn, boxed_list, clear, plural, toggle_class};
use crate::worker;

impl Window {
    /// The scripts in the folder, each with its Run; drawn with the macro
    /// cards, so their Runs follow what plays where.
    pub(super) fn draw_scripts(&self) {
        let list_box = &self.0.ui.scripts_box;
        clear(list_box);
        let dir = self.services().paths.macro_scripts();
        let list = boxed_list();
        match script::scripts(&dir) {
            Ok(found) if !found.is_empty() => {
                for s in &found {
                    list.append(&self.script_row(s));
                }
            }
            Ok(_) => list.append(&placeholder(
                "text-x-script-symbolic",
                "No advanced macros yet. Put a Python file in the folder and it shows up here, \
                 one Run for every selected account.",
            )),
            Err(e) => list.append(&placeholder(
                "dialog-warning-symbolic",
                &format!("Could not read {}: {e}", dir.display()),
            )),
        }
        let open = adw::ButtonRow::builder()
            .title("Open the Folder")
            .start_icon_name("folder-open-symbolic")
            .action_name("win.open-scripts")
            .build();
        list.append(&open);
        list_box.append(&list);
    }

    fn script_row(&self, s: &Script) -> adw::ActionRow {
        let about = s.about.clone().unwrap_or_else(|| "A Python script".to_owned());
        let row =
            adw::ActionRow::builder().title(&s.name).subtitle(&about).use_markup(false).build();
        row.set_subtitle_lines(2);
        let path = s.path.clone();
        let edit = Btn::new("flat circular")
            .icon("document-edit-symbolic")
            .tip("Open it in your editor")
            .build(self.act(move |w| w.open_file(&path)));
        edit.button.set_valign(Align::Center);
        let name = s.name.clone();
        let run = Btn::new("flat circular")
            .icon("media-playback-start-symbolic")
            .build(self.act(move |w| w.run_script(&name)));
        run.button.set_valign(Align::Center);
        row.add_suffix(&edit.button);
        row.add_suffix(&run.button);

        let (shown, run_name) = (row.clone(), script::run_name(&s.name));
        self.watch_macros(Box::new(move |st| {
            let playing = st.macro_runs.values().filter(|(_, m)| *m == run_name).count();
            toggle_class(&shown, "running", playing > 0);
            shown.set_subtitle(&if playing > 0 {
                format!("Playing on {}", plural(playing, "client", "clients"))
            } else {
                about.clone()
            });
            let (stop, n) = match st.macro_run(&run_name) {
                MacroRun::Start(fresh) => (false, fresh.len()),
                MacroRun::Stop => (true, 0),
                MacroRun::Nothing => (false, 0),
            };
            run.set_icon(if stop {
                "media-playback-stop-symbolic"
            } else {
                "media-playback-start-symbolic"
            });
            run.button.set_sensitive(stop || n > 0);
            let tip = if stop {
                "Stop it everywhere it plays".to_owned()
            } else if n == 0 {
                "Select the accounts to run it on".to_owned()
            } else {
                format!(
                    "Run it for {}",
                    plural(n, "selected account's client", "selected accounts' clients")
                )
            };
            run.button.set_tooltip_text(Some(&tip));
            widgets::name(&run.button, if stop { "Stop" } else { "Run" });
        }));
        row
    }

    /// The advanced macros folder in the file manager, made (with an
    /// example in it) if it is not there yet.
    pub fn open_scripts_folder(&self) {
        let dir = self.services().paths.macro_scripts();
        if let Err(e) = script::prepare(&dir) {
            return self.toast(&format!("Could not make {}: {e}", dir.display()));
        }
        self.refresh_macros();
        self.open_file(&dir);
    }

    fn open_file(&self, path: &Path) {
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
        let weak = self.weak();
        let shown = path.display().to_string();
        launcher.launch(Some(self.gtk_window()), None::<&gio::Cancellable>, move |done| {
            if let (Err(e), Some(w)) = (done, weak.upgrade()) {
                w.toast(&format!("Could not open {shown}: {e}"));
            }
        });
    }

    /// Run: the script on every selected account not already running it;
    /// once every selected one runs it, Stop wherever it runs.
    fn run_script(&self, name: &str) {
        let run_name = script::run_name(name);
        let chosen = match self.state().macro_run(&run_name) {
            MacroRun::Start(chosen) => chosen,
            MacroRun::Stop => return self.stop_macro(&run_name),
            MacroRun::Nothing => return self.toast("Select the accounts to run it on"),
        };
        let path = self.services().paths.macro_scripts().join(&run_name);
        if !path.is_file() {
            self.refresh_macros();
            return self.toast(&format!("{run_name} is no longer in the folder"));
        }
        for id in chosen {
            // A script cannot reach a normal window once you look away.
            self.state_mut().accounts.set_nested(id, true);
            self.start_script(id, name, path.clone());
        }
        self.save_accounts();
    }

    /// Run the script at `path` for the account's macro-ready client, as a
    /// process of its own, answered on a thread.
    fn start_script(&self, id: UserId, name: &str, path: PathBuf) {
        let label = {
            let s = self.state();
            let Some(label) = s.accounts.get(id).map(|a| a.name.to_string()) else { return };
            if s.recording == Some(id) {
                drop(s);
                return self.toast(&format!("{label} is being recorded: stop that first"));
            }
            label
        };
        let run_name = script::run_name(name);
        // One macro per client: two typing into one display would interleave.
        let stop = StopFlag::default();
        let previous = {
            let mut s = self.state_mut();
            s.macro_progress.remove(&id);
            s.macro_runs.insert(id, (stop.clone(), run_name.clone()))
        };
        if let Some((old, _)) = previous {
            old.set();
        }
        self.refresh_states();
        self.log(&format!("{label}: running {run_name}"));
        let paths = &self.services().paths;
        let display = nested::display_file(paths.runtime_dir(), &Profile::of(id));
        // Every picked image, read here where GDK reads them: a script may
        // look for any.
        let images = images::all(&paths.macro_images());
        let python = Python::new(paths.script_helper());
        let (profiles, log) = (self.services().profiles.clone(), self.logger());
        let mine = stop.clone();
        let progress = self.show_progress(id, &stop);
        let weak = self.weak();
        worker::run(
            move || {
                let profile = Profile::of(id);
                // Only a client seen gone ends the run, as for a plain macro.
                let running = || match profiles.running() {
                    Ok(up) => up.contains(&profile),
                    Err(e) => {
                        log.line(format!("{label}: {run_name} carries on -- {e}"));
                        true
                    }
                };
                let connect = |p: &Path| -> std::io::Result<Box<dyn Input>> {
                    Ok(Box::new(VirtualInput::connect(p)?))
                };
                let open = |p: &Path| -> std::io::Result<Box<dyn Eyes + Send>> {
                    Ok(Box::new(Screencopy::connect(p)?))
                };
                let image = |name: &str| {
                    images
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| Err(format!("there is no image named {name}")))
                };
                let mut names: Vec<String> =
                    images.iter().filter(|(_, i)| i.is_ok()).map(|(n, _)| n.clone()).collect();
                names.sort();
                let report = |line: String| {
                    log.line(format!("{label}: {run_name} -- {line}"));
                    // The receiver only goes away with the main loop.
                    let _ = progress.send_blocking(line);
                };
                let run = ScriptRun {
                    display: &display,
                    running: &running,
                    connect: &connect,
                    open: &open,
                    image: &image,
                    image_names: &names,
                    report: &report,
                    interpreter: &python,
                    account: (&label, id.0),
                };
                match run.run(&path, &stop) {
                    Ok(()) => {
                        let end = if stop.is_set() { "stopped" } else { "finished" };
                        log.line(format!("{label}: {run_name} {end}"));
                        None
                    }
                    Err(e) => {
                        let why = format!("{label}: {run_name} stopped -- {e}");
                        log.line(why.clone());
                        Some(why)
                    }
                }
            },
            move |failed: Option<String>| {
                let Some(w) = weak.upgrade() else { return };
                let mut s = w.state_mut();
                // Only this run's entry: a newer run may have replaced it.
                if s.macro_runs.get(&id).is_some_and(|(flag, _)| flag.same_as(&mine)) {
                    s.macro_runs.remove(&id);
                    s.macro_progress.remove(&id);
                }
                drop(s);
                w.refresh_states();
                if let Some(why) = failed {
                    w.toast(&why);
                }
            },
        );
    }
}
