//! The nested display a macro-ready client runs in: where it is linked, and
//! the command that runs a client inside it. One rule for both.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};
use rustix::io::Errno;

use crate::types::Profile;

/// Keep cages off the display they open their windows on. Each cage names
/// its own display after the first `wayland-N` whose `.lock` it can take,
/// and treats an unlocked socket there as stale: it deletes it and binds its
/// own. Inside a Flatpak the compositor's socket comes without its lock, so
/// the second cage would take the compositor's name, and every cage after it
/// would open inside that one. A shared lock on that name, held for as long
/// as the returned file lives, keeps every cage off it.
///
/// None when there is nothing to hold: no Wayland display, or one given by
/// path, or one whose compositor holds its lock itself (a native install).
pub fn hold_parent_display(
    runtime_dir: &Path,
    wayland_display: Option<&str>,
) -> io::Result<Option<File>> {
    let Some(name) = wayland_display.filter(|d| !d.is_empty() && !d.contains('/')) else {
        return Ok(None);
    };
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o640)
        .open(runtime_dir.join(format!("{name}.lock")))?;
    match flock(&lock, FlockOperation::NonBlockingLockShared) {
        Ok(()) => Ok(Some(lock)),
        Err(Errno::WOULDBLOCK) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Where a macro-ready client's display is linked while it runs, under the
/// user's runtime directory -- for the link (`cage_argv`) and for macros alike.
pub fn display_file(runtime_dir: &Path, profile: &Profile) -> PathBuf {
    runtime_dir.join("rbxmgr").join(format!("{profile}.wayland"))
}

/// `argv` in a cage of its own. The shell hard-links cage's socket to the
/// display file while the client runs, where macros connect to it, and
/// removes it when the client ends -- cage then exits with it. A link, not
/// the display's name, so input can only ever reach this cage: once cage is
/// gone the link refuses connections, whoever gets its name next. The link's
/// path goes in as `$0`, never into the script.
pub fn cage_argv(display_file: &Path, argv: &[String]) -> Vec<String> {
    let script = r#"f="$0"; mkdir -p "${f%/*}"; ln -f "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" "$f"; "$@"; rm -f "$f""#;
    let mut out: Vec<String> = ["cage", "--", "sh", "-c", script].map(String::from).into();
    out.push(display_file.display().to_string());
    out.extend_from_slice(argv);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn the_display_file_is_per_profile_under_the_runtime_dir() {
        assert_eq!(
            display_file(Path::new("/run/user/1000"), &Profile::named("rbxmgr-7")),
            PathBuf::from("/run/user/1000/rbxmgr/rbxmgr-7.wayland")
        );
    }

    #[test]
    fn a_held_parent_display_cannot_be_taken_by_a_cage() {
        let dir = tempfile::tempdir().unwrap();
        let held = hold_parent_display(dir.path(), Some("wayland-1")).unwrap().unwrap();
        // What a cage tries: the name's lock, exclusively, without waiting.
        let cage = File::open(dir.path().join("wayland-1.lock")).unwrap();
        assert_eq!(flock(&cage, FlockOperation::NonBlockingLockExclusive), Err(Errno::WOULDBLOCK));
        // Another manager can hold it alongside.
        assert!(hold_parent_display(dir.path(), Some("wayland-1")).unwrap().is_some());
        drop(held);
    }

    #[test]
    fn a_parent_display_its_compositor_locks_is_left_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let compositor = File::create(dir.path().join("wayland-1.lock")).unwrap();
        flock(&compositor, FlockOperation::NonBlockingLockExclusive).unwrap();
        assert!(hold_parent_display(dir.path(), Some("wayland-1")).unwrap().is_none());
    }

    #[test]
    fn no_display_or_one_given_by_path_has_nothing_to_hold() {
        let dir = tempfile::tempdir().unwrap();
        assert!(hold_parent_display(dir.path(), None).unwrap().is_none());
        assert!(hold_parent_display(dir.path(), Some("")).unwrap().is_none());
        assert!(hold_parent_display(dir.path(), Some("/run/w/wayland-0")).unwrap().is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn the_display_is_linked_while_the_client_runs_then_removed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("wayland-9"), "").unwrap();
        let file = display_file(dir.path(), &Profile::named("rbxmgr-7"));
        let check =
            format!("[ '{}' -ef \"$XDG_RUNTIME_DIR/wayland-9\" ] && echo linked", file.display());
        let argv = cage_argv(&file, &["sh".into(), "-c".into(), check]);
        assert_eq!(&argv[..2], ["cage", "--"]);
        let out = Command::new(&argv[2])
            .args(&argv[3..])
            .env("XDG_RUNTIME_DIR", dir.path())
            .env("WAYLAND_DISPLAY", "wayland-9")
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "linked\n", "{out:?}");
        assert!(!file.exists());
    }
}
