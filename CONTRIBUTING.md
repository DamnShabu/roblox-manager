# Contributing

Thanks for helping. Bug reports, fixes and ideas are all welcome.

## Reporting a bug or asking for a feature

[Open an issue](https://github.com/DamnShabu/roblox-manager/issues/new/choose)
and fill in the form. For a bug, the Activity log and the client's log
(`~/.cache/rbxmgr/logs`) usually say what went wrong. **Never paste a
session cookie** (`.ROBLOSECURITY`) into an issue.

Security problems go through a
[private advisory](https://github.com/DamnShabu/roblox-manager/security/advisories/new)
instead. See [SECURITY.md](SECURITY.md).

## Changing the code

Start with [docs/development.md](docs/development.md). It covers how the code
is laid out, how to build it and how releases are made.
[CONTEXT.md](CONTEXT.md) is the glossary: use its words in code and docs.

The rules every change keeps:

- `crates/core` holds every behaviour and doesn't depend on GTK.
  `crates/app` is the window. Inside each, one directory per area.
- No source file over 600 lines. Split it along a real seam.
- Deep modules with small interfaces. No `unwrap`/`expect` outside tests,
  and no swallowed errors.
- Every seam into the outside world is a trait with two adapters: the real
  one and the one the tests use.
- Data files (`~/.local/share/rbxmgr/*.json`) and keyring attributes stay
  readable when an earlier version wrote them.

Before you push:

```bash
nix develop -c cargo test
nix develop -c cargo clippy --all-targets -- -D warnings
nix develop -c cargo fmt --check
```

If you changed `Cargo.lock` (or `cordial/Cargo.lock`), also run
`packaging/flatpak/update-sources.sh` and commit what it changes.

## Pull requests

Open a pull request against `main`. CI runs the checks above plus a Nix
build and security scans, and [CodeRabbit](https://coderabbit.ai) reviews
the change. Resolve its comments (by fixing them or by replying to say why
not) before merging. Pull requests are squash-merged, so the title becomes
the commit message: say what changes for the user.

By contributing you agree that your work is released under the
[MIT License](LICENSE) and that you follow the
[Code of Conduct](CODE_OF_CONDUCT.md).
