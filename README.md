# roblox-manager

Several Roblox accounts, launched into one server.

Roblox runs in Cordial, an open-source runtime for Roblox's official Android
build — this repo's fork of it (`cordial/`), built as two command-line tools
the manager drives: `cordial-run` for each account's client and
`cordial-fetch` for the Roblox build. One Cordial profile per account; the
manager gives each profile its session and starts it with the game's own deep
link. Nothing is injected into the client. Auth is a `.ROBLOSECURITY` cookie
kept in the Secret Service.

Macros: a "macro-ready" account's client runs in its own nested cage display,
and macros play into it through Wayland's virtual keyboard and pointer —
emulated input, so neither your devices nor the client are touched.

Roblox's rules treat simultaneous clients as a policy violation, and community
reports tie them to anti-cheat flags. The exposure starts when the second
client is launched, which is always an explicit click.

## Use

```bash
nix run .                               # the manager
nix build .#roblox-manager-appimage     # one file for other distros (x86_64)
```

NixOS: add this flake as an input and import `inputs.roblox-manager.nixosModules.default`.

Data lives in `~/.local/share/rbxmgr`, `~/.local/share/cordial`,
`~/.config/cordial` and (regenerable) `~/.cache/cordial`.

## Layout

A Rust workspace:

- `crates/core` (`rbxmgr-core`): everything except drawing, one directory per
  area -- `accounts`, `keyring`, `roblox`, `cordial`, `launch`, `macros`. No
  GTK; every seam into the outside world (Secret Service, Roblox's web API,
  processes, the Wayland display) is a trait with a production adapter and
  the one the tests use.
- `crates/app` (`roblox-manager`): the GTK4/libadwaita window on top of it.
  Slow work runs on worker threads; results come back to the main loop.

## Checks

```bash
nix develop -c cargo test                              # offline
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo fmt --check
```

No source file may pass 600 lines; `crates/core/tests/size_limit.rs` fails
the build when one does.

`design/` holds the app redesign mockups.
