# Development

## How it fits together

Roblox runs in Cordial, an open-source runtime for Roblox's official Android
build. The manager uses this repo's fork of it, Stacked, as two command-line
tools: `cordial-run` starts each account's client and `cordial-fetch` gets
the Roblox build. `cordial/source.json` pins the source, and
`cordial/README.md` covers what the manager needs from it and how to move the
pin.

Each account has its own Cordial profile (`rbxmgr-<user id>`). Before a
launch the manager writes the account's session into that profile, then
starts the client with the game's deep link. Nothing is injected into the
client. Sessions (`.ROBLOSECURITY` cookies) live only in the Secret Service.

A macro-ready account's client runs inside its own nested `cage` display.
Macros play into that display through Wayland's virtual keyboard and pointer,
so the user's real input devices are never involved.

The client itself runs behind a relay: the manager's own binary, started as
`roblox-manager --relay DISPLAY_FILE -- cordial-run ...` inside the cage. It
passes the client's Wayland connection through to cage unchanged and, only
while the editor has a recording armed on its report socket
(`<profile>.record`, beside the display link), reports the key, button,
pointer and wheel events the window receives. F8 starts and stops that report
and is kept from the game while a recording is armed
(`crates/core/src/macros/relay/`, turned into steps by
`crates/core/src/macros/recording.rs`).

## Layout

A Rust workspace:

- `crates/core` (`rbxmgr-core`) holds everything except drawing, with one
  directory per area: `accounts`, `keyring`, `roblox`, `cordial`, `launch`,
  `macros`, `update`. It has no GTK dependency. Every seam into the outside
  world (Secret Service, Roblox's web API, GitHub, PackageKit, processes, the
  Wayland display) is a trait with a production adapter and a test adapter.
- `crates/app` (`roblox-manager`) is the GTK4/libadwaita window on top of it.
  Slow work runs on worker threads and the results come back to the main
  loop. `resources/style.css` holds the few things the window adds to
  Adwaita: the amber accent, the warm surfaces (`style-dark.css` for the dark
  style), and a few pieces Adwaita has no class for. Every icon is one of
  Adwaita's symbolic icons.

`CONTEXT.md` is the glossary. Use its words in code and docs.

## Building

```bash
nix run .                               # run the manager
nix build .#roblox-manager-appimage     # an AppImage (x86_64)
packaging/flatpak/build.sh              # a Flatpak, installed for this user
```

## Checks

```bash
nix develop -c cargo test                              # offline
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo fmt --check
```

No source file may be longer than 600 lines.
`crates/core/tests/size_limit.rs` fails the build when one is.

## Releasing

```bash
packaging/release.sh 0.3.0        # version in Cargo.toml, Cargo.lock, metainfo; commit; tag
git push origin main v0.3.0       # the tag starts .github/workflows/release.yml
```

The workflow runs the checks, builds the AppImage with Nix, the distribution
packages from it (`packaging/linux/build.sh`, with nfpm) and the Flatpak
bundle, then publishes all of them with their `SHA256SUMS` as the GitHub
release that the in-app updater reads. A tag with a pre-release part
(`v0.3.0-rc.1`) is published as a pre-release, and the updater never offers
those.

## Scripting the window

Row and group actions are window actions, so they can be driven over D-Bus
(`org.gtk.Actions` on `/io/github/mujo/RobloxManager/window/1`):

```bash
gdbus call --session -d io.github.mujo.RobloxManager \
  -o /io/github/mujo/RobloxManager/window/1 \
  -m org.gtk.Actions.Activate account-settings '[<uint64 12345>]' '{}'
```

`crates/app/src/ui/window/actions.rs` lists them all.
