#!/usr/bin/env bash
# Cut a release: set the version everywhere it is written, commit, and tag.
# Pushing the tag is left to you; it is what starts the release workflow
# (.github/workflows/release.yml), which builds and publishes every package.
#
#   packaging/release.sh 0.3.0
#   git push origin main v0.3.0
set -euo pipefail

version="${1:-}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
    echo "usage: $0 VERSION   (e.g. 0.3.0, or 0.3.0-rc.1 for a pre-release)" >&2
    exit 2
fi
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
    echo "error: commit or stash your changes first" >&2
    exit 1
fi
if git rev-parse -q --verify "refs/tags/v$version" >/dev/null; then
    echo "error: v$version is already tagged" >&2
    exit 1
fi

# The workspace's version: the first `version =` line, under [workspace.package].
sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
# Cargo.lock records the workspace's crates' versions too; only those change.
cargo=(cargo)
command -v cargo >/dev/null || cargo=(nix develop -c cargo)
"${cargo[@]}" update --workspace --quiet

# The metainfo's release list, newest first, for software centres. A
# pre-release is not listed.
metainfo=packaging/flatpak/io.github.mujo.RobloxManager.metainfo.xml
if [[ "$version" != *-* ]] && ! grep -q "<release version=\"$version\"" "$metainfo"; then
    sed -i "s|<releases>|<releases>\n    <release version=\"$version\" date=\"$(date +%F)\"/>|" "$metainfo"
fi

"${cargo[@]}" test --quiet >/dev/null
git commit --quiet -am "release: v$version"
git tag -a "v$version" -m "Roblox Manager $version"
echo "Tagged v$version. Publish it with:"
echo "  git push origin $(git branch --show-current) v$version"
