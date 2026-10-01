#!/usr/bin/env bash
# A release's distribution packages, from its AppImage: .deb (Debian, Ubuntu,
# Mint, Pop!_OS), .rpm (Fedora, openSUSE) and .pkg.tar.zst (Arch, Manjaro).
#
#   packaging/linux/build.sh APPIMAGE VERSION OUTDIR
#
# Each holds the same bundle, unpacked under /opt/roblox-manager and started
# through its own AppRun, so one build runs on every distro whatever its
# GTK. What a package adds over the AppImage: the package manager knows it,
# the desktop finds its entry, icon and link handler without being asked,
# and the app updates itself through PackageKit (crates/core/src/update).
# The file names are the ones the app looks for (update/channel.rs;
# crates/core/tests/release_assets.rs checks they agree).
#
# Needs nfpm and unsquashfs (squashfs-tools); where nix is available they are
# brought in with `nix shell`.
set -euo pipefail

if (( $# != 3 )); then
    echo "usage: $0 APPIMAGE VERSION OUTDIR" >&2
    exit 2
fi
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
appimage="$(realpath "$1")"
VERSION="$2"
out="$(realpath -m "$3")"

if ! command -v nfpm >/dev/null || ! command -v unsquashfs >/dev/null; then
    if command -v nix >/dev/null && [[ -z "${RBXMGR_IN_NIX_SHELL:-}" ]]; then
        RBXMGR_IN_NIX_SHELL=1 exec nix shell nixpkgs#nfpm nixpkgs#squashfsTools \
            -c "${BASH_SOURCE[0]}" "$@"
    fi
    for tool in nfpm unsquashfs; do
        command -v "$tool" >/dev/null || { echo "error: $tool is not installed" >&2; exit 1; }
    done
fi

work="$(mktemp -d)"
# The bundle's store paths are read-only, as Nix made them.
trap 'chmod -R u+w "$work" && rm -rf "$work"' EXIT
# The image is read straight out of the AppImage, not by running it (NixOS
# hands every AppImage to appimage-run): its SquashFS starts where the
# runtime's ELF section headers end -- e_shoff + e_shentsize * e_shnum, as
# crates/core/src/cordial/stacked/appimage.rs reads it.
field() { od -An -t "u$2" -j "$1" -N "$2" "$appimage" | tr -d ' '; }
offset=$(( $(field 40 8) + $(field 58 2) * $(field 60 2) ))
unsquashfs -quiet -no-progress -offset "$offset" -dest "$work/bundle" "$appimage" >/dev/null
mkdir -p "$out"

# One launcher per format: the variable is how the app knows which package
# to update itself with (crates/core/src/install.rs).
package() {
    local format=$1 packager=$2 target=$3
    cat > "$work/roblox-manager-$format" <<LAUNCHER
#!/bin/sh
# Roblox Manager, from its $format package.
RBXMGR_PACKAGE=$format
export RBXMGR_PACKAGE
exec /opt/roblox-manager/AppRun "\$@"
LAUNCHER
    cat > "$work/nfpm-$format.yaml" <<CONFIG
name: roblox-manager
version: "$VERSION"
release: 1
arch: amd64
platform: linux
section: games
maintainer: "mujō <noreply@github.com>"
description: |
  Several Roblox accounts, launched into one server.
  Signs in Roblox accounts with Quick Login, launches them together behind a
  leader, and plays input macros into them. Bundles its own GTK and the
  Cordial runtime the clients run in.
homepage: https://github.com/DamnShabu/roblox-manager
license: MIT AND GPL-3.0-or-later
contents:
  - src: $work/bundle/
    dst: /opt/roblox-manager
    type: tree
  - src: $work/roblox-manager-$format
    dst: /usr/bin/roblox-manager
    file_info: {mode: 0755}
  - src: $root/packaging/flatpak/io.github.mujo.RobloxManager.desktop
    dst: /usr/share/applications/io.github.mujo.RobloxManager.desktop
  - src: $root/packaging/flatpak/io.github.mujo.RobloxManager.metainfo.xml
    dst: /usr/share/metainfo/io.github.mujo.RobloxManager.metainfo.xml
  - src: $root/packaging/icons/roblox-manager.svg
    dst: /usr/share/icons/hicolor/scalable/apps/io.github.mujo.RobloxManager.svg
  - src: $here/apparmor-profile
    dst: /usr/share/roblox-manager/apparmor-profile
    packager: deb
deb:
  compression: zstd
rpm:
  compression: zstd
scripts:
  postinstall: $here/postinst
  postremove: $here/postrm
CONFIG
    nfpm package --config "$work/nfpm-$format.yaml" --packager "$packager" --target "$out/$target"
}

package deb deb "roblox-manager_${VERSION}_amd64.deb"
package rpm rpm "roblox-manager-${VERSION}-1.x86_64.rpm"
package arch archlinux "roblox-manager-${VERSION}-1-x86_64.pkg.tar.zst"
