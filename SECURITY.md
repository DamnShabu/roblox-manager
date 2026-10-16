# Security

Roblox Manager holds your Roblox sessions and installs software on your
machine, so a security problem in it matters. Thank you for reporting one.

## Supported versions

Only the [latest release](https://github.com/DamnShabu/roblox-manager/releases/latest)
gets fixes. The app updates itself, so a fix reaches people through
**Update**.

## Reporting a vulnerability

**Please don't open a public issue.** Report it privately instead:
[open a security advisory](https://github.com/DamnShabu/roblox-manager/security/advisories/new).

Say what an attacker can do, what they need first (another local user, a
malicious website, a network position, ...), and how to reproduce it. Leave
out real session cookies: a made-up value shows the problem just as well.

You'll get an answer within a week. Once a fix is released, the advisory is
published with credit to you, unless you'd rather not be named.

## What is in scope

- **Sessions.** `.ROBLOSECURITY` cookies are kept only in the Secret Service
  (GNOME Keyring or KWallet) and written into an account's Cordial profile
  just before it launches. Anything that leaks them, to disk, to a log, to
  another account's profile or over the network to anywhere but Roblox, is
  in scope.
- **Updates.** The manager downloads its own releases and the Stacked engine
  from GitHub and installs them. Its own downloads are checked against the
  release's `SHA256SUMS`. A way to get a different file installed is in
  scope.
- **Roblox links.** The manager handles `roblox://` and `roblox-player:`
  links from the browser. A link that does anything without the user
  clicking **Join** is in scope.
- **Macro-ready windows.** A macro plays into a nested display through a
  relay, and the macro editor records through a socket beside it
  (`crates/core/src/macros/relay/`). Another program using that socket to
  read input or inject it is in scope.
- **Packaging.** The Flatpak's permissions, the `.deb`'s AppArmor profile and
  the package scripts.

Out of scope: Roblox's own client and services, Cordial/Stacked bugs that
aren't caused by how the manager runs it (report those to
[Stacked](https://github.com/DamnShabu/stacked)), and anything that needs
an attacker who already runs code as your user.

## Verifying a download

Every release asset comes with a signed build-provenance attestation. To
check that a file was built from this repository by its release workflow:

```bash
gh attestation verify roblox-manager-*-x86_64.AppImage -R DamnShabu/roblox-manager
```

`SHA256SUMS` on the release page lists every asset's checksum
(`sha256sum -c SHA256SUMS --ignore-missing`).
