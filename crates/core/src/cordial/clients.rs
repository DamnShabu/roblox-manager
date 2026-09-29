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
            let program = cmd.split(' ').next()?;
            if program.rsplit('/').next() != Some("cordial-run") {
                return None;
            }
            let (_, rest) = cmd.split_once(" --profile ")?;
            let name = rest.split(" --").next()?;
            (!name.is_empty()).then(|| (pid, Profile::named(name)))
        })
        .collect()
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
junk line
";

    #[test]
    fn clients_are_read_with_their_profiles_spaces_and_all() {
        let parsed = parse(PGREP);
        let got: Vec<(u32, &str)> = parsed.iter().map(|(p, prof)| (*p, prof.as_str())).collect();
        assert_eq!(got, vec![(4101, "main"), (4102, "alt 2"), (4400, "rbxmgr-7")]);
    }

    #[test]
    fn no_clients_is_an_empty_answer() {
        assert!(parse("").is_empty());
    }
}
