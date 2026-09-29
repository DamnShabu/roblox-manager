//! The nested display a macro-ready client runs in: where it is linked, and
//! the command that runs a client inside it. One rule for both.

use std::path::{Path, PathBuf};

use crate::types::Profile;

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
