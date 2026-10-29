# Roblox Manager

Run several Roblox accounts on Linux and keep them working while you do
something else.

[![CI](https://github.com/DamnShabu/roblox-manager/actions/workflows/ci.yml/badge.svg)](https://github.com/DamnShabu/roblox-manager/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/DamnShabu/roblox-manager)](https://github.com/DamnShabu/roblox-manager/releases/latest)
[![CodeQL](https://github.com/DamnShabu/roblox-manager/actions/workflows/codeql.yml/badge.svg)](https://github.com/DamnShabu/roblox-manager/actions/workflows/codeql.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/DamnShabu/roblox-manager/badge)](https://scorecard.dev/viewer/?uri=github.com/DamnShabu/roblox-manager)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

Sign each account in once, then launch any set of them into a game with one
click. The window shows what runs, what needs you, and how much of the
machine each client takes. Roblox itself runs in Stacked, a fork of
[Cordial](cordial/README.md), the runtime for Roblox's official Android
build. The manager installs it and keeps it up to date.

> [!WARNING]
> Roblox's terms don't allow running more than one client at once, and people
> have reported anti-cheat flags for it. Installing the app does nothing on its
> own. The risk starts when you launch a second client, and that only happens
> when you click Launch. Use alts you can afford to lose.

## What it does

- **Macros while you work.** Key-and-click macros play into a client's own
  display, so they keep going while you use your desktop, and with the
  window hidden. Record them by playing, or have them react when something
  shows on screen.
- **One click, many accounts.** Tick accounts and launch them together, into
  a game or into a friend's server. Groups give each set of alts a game of
  its own.
- **The Play button on roblox.com** opens a small popup where you pick which
  accounts join.
- **A dozen clients without melting the machine.** Each account has a
  performance level, from 10 FPS on the lowest graphics to the game's top
  graphics, and the window shows each client's CPU and memory.
- **Signed out? Fixed in place.** Accounts Roblox signed out show up under
  **Needs attention**, with **Sign in again** beside them.
- **No passwords.** Accounts sign in with Roblox Quick Login, and sessions
  live only in your desktop's keyring.
- **Updates itself.** **Update** gets the newest manager and Roblox runtime;
  **Update All** gets the newest Roblox build too.

## Install

Download a package from the
[latest release](https://github.com/DamnShabu/roblox-manager/releases/latest)
(x86_64 only):

| Distro | File | Install |
| --- | --- | --- |
| Debian, Ubuntu, Mint, Pop!_OS | `roblox-manager_*_amd64.deb` | `sudo apt install ./roblox-manager_*_amd64.deb` |
| Fedora, openSUSE | `roblox-manager-*.x86_64.rpm` | `sudo dnf install ./roblox-manager-*.x86_64.rpm` |
| Arch, Manjaro | `roblox-manager-*-x86_64.pkg.tar.zst` | `sudo pacman -U roblox-manager-*.pkg.tar.zst` |
| Any distro (Flatpak) | `roblox-manager-*-x86_64.flatpak` | `flatpak install --user roblox-manager-*.flatpak` |
| Any distro (single file) | `roblox-manager-*-x86_64.AppImage` | `chmod +x` it and run it |

**NixOS:** add this repo as a flake input and import
`inputs.roblox-manager.nixosModules.default`. Or run it directly with
`nix run github:DamnShabu/roblox-manager`.

You'll also need:

- **A keyring** (GNOME Keyring, or KWallet with Secret Service support).
  Sessions are stored there and nowhere else.
- **A Wayland session** if you want to use macros.

The first launch downloads the Roblox build. That takes a few minutes, and
progress shows under **Activity**.

## Quick start

1. **Add an account.** Click **+** (<kbd>Ctrl</kbd>+<kbd>N</kbd>), open
   Quick Login on any device where you're signed in, and enter the code.
   Repeat for each account. [Adding accounts](docs/guide.md#add-an-account)
2. **Pick where to go.** The target picker next to **Launch** lists your
   favourite games, Roblox's games browser and your friends' servers.
   [Launch targets](docs/guide.md#launch-into-a-game)
3. **Launch.** Tick the accounts and click **Launch**
   (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Enter</kbd>).
4. **Leave them a macro.** Make one under **Macros** in the sidebar, pick it
   in an account's settings, launch again and press **Run**.
   [Setting up macros](docs/guide.md#macros)

## Documentation

- **[The guide](docs/guide.md)**: accounts, the window, launching, groups,
  links from the website, account settings, macros, updates, shortcuts and
  troubleshooting.
- **[When steps](docs/macros-when.md)**: macros that react to what's on
  screen.
- **[Advanced macros](docs/advanced-macros.md)**: macros written in Python,
  through the `rbxmgr` module.
- **[Development](docs/development.md)**: how the code fits together,
  building, checks and releases.
- **[Cordial](cordial/README.md)**: the runtime the clients run in, and how
  the manager keeps it updated.

## Privacy and security

The manager never sees your password, and sessions are kept only in your
keyring. When the manager updates itself, the download is checked against
the release's `SHA256SUMS`, and each release asset carries a
build-provenance attestation you can check with `gh attestation verify`.
See [SECURITY.md](SECURITY.md) to report a problem.

| What | Where |
| --- | --- |
| Accounts, groups, macros | `~/.local/share/rbxmgr` |
| Sessions | Your keyring only |
| Cordial profiles and config | `~/.local/share/cordial`, `~/.config/cordial` |
| Window size and style | `~/.local/state/rbxmgr` |
| Game icons, avatars, client logs (safe to delete) | `~/.cache/rbxmgr`, `~/.cache/cordial` |

## Contributing

Bug reports and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md)
says how, and `nix run .` builds and runs the manager from a checkout.

## Licence

[MIT](LICENSE). Roblox Manager is not affiliated with, endorsed by or
sponsored by Roblox Corporation. "Roblox" is a trademark of Roblox
Corporation.
