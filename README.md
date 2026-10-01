# roblox-manager

Several Roblox accounts, launched into one server.

![The window: the game strip, the leader and its auto-join list, a group, and the macros pane](docs/screenshots/window-dark.png)

Roblox runs in Cordial, an open-source runtime for Roblox's official Android
build — this repo's fork of it, built as two command-line tools the manager
drives: `cordial-run` for each account's client and `cordial-fetch` for the
Roblox build. `cordial/source.json` pins the source, and `cordial/README.md`
says what the manager needs from it and how to move it. One Cordial profile per account; the
manager gives each profile its session and starts it with the game's own deep
link. Nothing is injected into the client. Auth is a `.ROBLOSECURITY` cookie
kept in the Secret Service.

Macros: a "macro-ready" account's client runs in its own nested cage display,
and macros play into it through Wayland's virtual keyboard and pointer —
emulated input, so neither your devices nor the client are touched.

Roblox's rules treat simultaneous clients as a policy violation, and community
reports tie them to anti-cheat flags. The exposure starts when the second
client is launched, which is always an explicit click.

## Install

Every [release](https://github.com/DamnShabu/roblox-manager/releases) has
the app for any x86_64 distro: a `.deb` (Debian, Ubuntu, Mint, Pop!_OS), an
`.rpm` (Fedora, openSUSE), a `.pkg.tar.zst` (Arch, Manjaro), a `.flatpak`
bundle and an AppImage. The packages hold the same bundle as the AppImage,
unpacked under `/opt/roblox-manager`, so they need nothing from the distro
but a Secret Service (gnome-keyring, KWallet) and, for macros, a Wayland
session.

From source:

```bash
nix run .                               # the manager
nix build .#roblox-manager-appimage     # one file for other distros (x86_64)
packaging/flatpak/build.sh              # a Flatpak, installed for this user
```

NixOS: add this flake as an input and import `inputs.roblox-manager.nixosModules.default`.

## The window

A GTK4/libadwaita app in your desktop's light or dark style, or the one
chosen in its main menu.

- **Accounts** sign in with Roblox Quick Login (the add button, Ctrl+N):
  you approve a short code on a device where you are already signed in, and
  no password is typed here. Each row shows its avatar, its state, and
  play/stop; its menu (⋮, or a right click) holds its settings, leader and
  auto-join, groups, sessions, and removing it. Search (Ctrl+F) finds
  accounts by label, Roblox name or note.
- **The leader** launches first; its **auto-join** accounts follow into its
  server, in the order shown. Drag an account onto the leader's card to
  add it, onto a group to move it, or onto another row to reorder.
- **Groups** each have a game: Launch on a group's header sends every
  account in it there.
- **Launch Into** picks where Launch Selected and Launch as Group go: a
  favourite game of any account, Roblox's own games browser, or a friend's
  server.
- **Join links**: the app is the desktop's handler for `roblox-player:` and
  `roblox:` links, so the website's Play button (or a server's join link)
  opens a small popup rather than the whole window: the game, and the
  accounts to join with. Nothing starts until Join; several accounts share
  one server. "Remember these accounts" picks the same ones next time.
  Private-server and follow-a-user links are not supported yet.
  The Flatpak and the NixOS module register the handler themselves; if
  another launcher (Sober, Vinegar) gets the links, or you run the AppImage
  or a bare build, pick **Open Roblox Links Here** in the main menu. It
  sets the default in `~/.config/mimeapps.list`, and for an AppImage or bare
  build writes `~/.local/share/applications/io.github.mujo.RobloxManager.desktop`,
  which follows the AppImage if you move it.
- **Macros** (the side pane, F9) play keys and clicks into macro-ready
  clients; How Macros Work (F1) explains the steps and their timing.
- **Activity** keeps what happened this run; Ctrl+L shows all of it.
- **Update** (the main menu's Update All) brings everything up to date in
  one go: the newest Roblox build, the newest Stacked, and Roblox Manager
  itself. The app checks GitHub at start and every six hours, and an
  Update button appears in the header bar when it or Stacked has a newer
  release. The app replaces itself the way it was installed: an AppImage
  swaps its own file, the Flatpak reinstalls its bundle, and a `.deb`,
  `.rpm` or Arch package is installed through PackageKit (your desktop's
  password prompt; on Arch without PackageKit you are told where the file
  is). Every download is checked against the release's `SHA256SUMS`. A
  Nix or hand-built copy only says that a new version is out. Restart, in
  the banner, starts the new version.

<p>
  <img src="docs/screenshots/window-light.png" width="49%" alt="The window in the light style">
  <img src="docs/screenshots/macro-editor.png" width="49%" alt="The macro editor">
</p>

| Keys | Does |
| --- | --- |
| Ctrl+N | Add an account |
| Ctrl+F | Search accounts |
| Ctrl+R, F5 | Check sessions and reload favourites |
| Ctrl+Enter | Launch as group |
| Ctrl+Shift+Enter | Launch selected |
| Ctrl+Shift+. | Stop all |
| Ctrl+Shift+N | New macro |
| F9 | Show or hide the macros pane |
| Ctrl+L | Activity log |
| F1 | How macros work |
| Ctrl+? | Every shortcut, and the macros' hotkeys |

Row and group actions are window actions too, so they can be scripted over
D-Bus (`org.gtk.Actions` on `/io/github/mujo/RobloxManager/window/1`), and a
macro's hotkey can be bound anywhere with
`gapplication action io.github.mujo.RobloxManager run-macro "'NAME'"`.

## Files

Data lives in `~/.local/share/rbxmgr`, `~/.local/share/cordial`,
`~/.config/cordial`, the window's size in `~/.local/state/rbxmgr`, and
(regenerable) game icons, avatars and client logs in `~/.cache/rbxmgr` and
`~/.cache/cordial`.

## Layout

A Rust workspace:

- `crates/core` (`rbxmgr-core`): everything except drawing, one directory per
  area -- `accounts`, `keyring`, `roblox`, `cordial`, `launch`, `macros`,
  `update`. No GTK; every seam into the outside world (Secret Service,
  Roblox's web API, GitHub, PackageKit, processes, the Wayland display) is a
  trait with a production adapter and the one the tests use.
- `crates/app` (`roblox-manager`): the GTK4/libadwaita window on top of it.
  Slow work runs on worker threads; results come back to the main loop.
  `resources/style.css` holds the little the window adds to Adwaita: the
  amber accent, the warm surfaces (`style-dark.css` for the dark style), and
  a few pieces Adwaita has no class for. Every icon is one of Adwaita's
  symbolic icons.

## Releasing

```bash
packaging/release.sh 0.3.0        # version in Cargo.toml, Cargo.lock, metainfo; commit; tag
git push origin main v0.3.0       # the tag starts .github/workflows/release.yml
```

The workflow runs the checks below, builds the AppImage with Nix, the
distribution packages from it (`packaging/linux/build.sh`, with nfpm), and
the Flatpak bundle, and publishes them with their `SHA256SUMS` as the
GitHub release that Update reads. A tag with a pre-release part
(`v0.3.0-rc.1`) is published as a pre-release, which Update never offers.

## Checks

```bash
nix develop -c cargo test                              # offline
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo fmt --check
```

No source file may pass 600 lines; `crates/core/tests/size_limit.rs` fails
the build when one does.
