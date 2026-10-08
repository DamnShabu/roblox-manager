//! Which Cordial clients are running, read from `pgrep -a -f cordial-run`.

use std::collections::BTreeMap;

use crate::types::Profile;

/// {pid: profile} from `pgrep -a`: one line per engine process,
/// `<pid> /nix/store/.../bin/cordial-run --lib-dir ... --profile <name> ...`.
///
/// Only processes whose argv[0] is the cordial-run binary count. `pgrep -f`
/// matches anywhere in a command line, so a `nice cordial-run ...` on its way
/// to exec, or a shell that merely mentions one, would otherwise pass for a
/// client -- and be stopped by Stop. pgrep joins argv with spaces, so a
/// profile name containing " --" would be cut short; the manager's never do.
pub fn parse(pgrep: &str) -> BTreeMap<u32, Profile> {
    pgrep
        .lines()
        .filter_map(|line| {
            let (pid, cmd) = line.trim().split_once(' ')?;
            let pid: u32 = pid.parse().ok()?;
            if !is_engine(cmd.split(" --").next()?) {
                return None;
            }
            let (_, rest) = cmd.split_once(" --profile ")?;
            let name = rest.split(" --").next()?;
            (!name.is_empty()).then(|| (pid, Profile::named(name)))
        })
        .collect()
}

/// Whether `program`, the command line up to its first flag, is the engine
/// itself. An updated Stacked runs from under the user's data directory,
/// which can hold a space (`/home/John Smith/...`): cutting at the first
/// space read that as `/home/John` and lost every client. A path is taken
/// whole, so what rules out a wrapper is that its words are not one path:
/// an option (`nice -n 10 cordial-run`, `bash -c ...`) or a second path.
fn is_engine(program: &str) -> bool {
    program == "cordial-run"
        || (program.starts_with('/')
            && program.ends_with("/cordial-run")
            && !program.contains(" -")
            && !program.contains(" /"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PGREP: &str = "\
4101 /app/bin/cordial-run --lib-dir /x/lib --apk /x/base.apk --host-libc --game-activity --run 0 --profile main
4102 /app/bin/cordial-run --lib-dir /x/lib --apk /x/base.apk --run 0 --profile alt 2 --join-url roblox-player:1+launchmode:play
4200 flatpak-spawn --host pgrep -a -f cordial-run
4300 bash -c /app/bin/cordial-run --profile main & sleep 1
4400 cordial-run --lib-dir /l --apk /a --run 0 --profile rbxmgr-7
4401 bwrap --args 74 -- cordial-run --profile rbxmgr-7
4500 /home/John Smith/.local/share/rbxmgr/stacked/0.22.0/usr/bin/cordial-run --run 0 --profile rbxmgr-9
4501 nice -n 10 /home/John Smith/.local/share/rbxmgr/stacked/current/bin/cordial-run --profile rbxmgr-9
4502 sh /x/cordial-run --profile rbxmgr-9
4503 /usr/bin/env /x/cordial-run --profile rbxmgr-9
junk line
";

    #[test]
    fn clients_are_read_with_their_profiles_spaces_and_all() {
        let parsed = parse(PGREP);
        let got: Vec<(u32, &str)> = parsed.iter().map(|(p, prof)| (*p, prof.as_str())).collect();
        assert_eq!(
            got,
            vec![(4101, "main"), (4102, "alt 2"), (4400, "rbxmgr-7"), (4500, "rbxmgr-9")]
        );
    }

    #[test]
    fn no_clients_is_an_empty_answer() {
        assert!(parse("").is_empty());
    }
}
