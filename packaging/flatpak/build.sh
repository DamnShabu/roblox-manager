#!/usr/bin/env bash
# Build the Flatpak and install it for this user.
#
#   packaging/flatpak/build.sh            build and install
#   packaging/flatpak/build.sh --bundle   also write roblox-manager.flatpak,
#                                         a single file to install elsewhere
#
# Needs flatpak and flatpak-builder (on NixOS: nix shell nixpkgs#flatpak-builder).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
manifest="$here/io.github.mujo.RobloxManager.yml"
app_id=io.github.mujo.RobloxManager

for tool in flatpak flatpak-builder; do
    command -v "$tool" >/dev/null || { echo "error: $tool is not installed" >&2; exit 1; }
done

# The runtime and SDK are numbered by GNOME; the Rust and LLVM extensions by
# the freedesktop base under it. Both numbers come from the manifest.
runtime=$(sed -n "s/^runtime-version: *'\(.*\)'/\1/p" "$manifest")
extensions=$(sed -n "s/^# sdk-extension-version: *'\(.*\)'/\1/p" "$manifest")
missing=()
for ref in "org.gnome.Platform//$runtime" "org.gnome.Sdk//$runtime" \
    "org.freedesktop.Sdk.Extension.rust-stable//$extensions" \
    "org.freedesktop.Sdk.Extension.llvm20//$extensions"; do
    flatpak info "$ref" >/dev/null 2>&1 || missing+=("$ref")
done
if (( ${#missing[@]} )); then
    flatpak install --user --noninteractive flathub "${missing[@]}"
fi

# Everything flatpak-builder writes stays under target/, beside cargo's.
cd "$root"
flatpak-builder --user --install --force-clean \
    --state-dir=target/flatpak-state --repo=target/flatpak-repo \
    target/flatpak-build "$manifest"

if [[ "${1:-}" == --bundle ]]; then
    flatpak build-bundle target/flatpak-repo roblox-manager.flatpak "$app_id"
    echo "wrote $root/roblox-manager.flatpak"
fi
