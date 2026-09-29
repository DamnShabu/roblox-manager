# Stacked, mujō's fork of Cordial -- the runtime that runs Roblox's official
# Android build natively -- built as two command-line tools and nothing else.
#
#   cordial-run    one account's game client (the fork's engine binary)
#   cordial-fetch  installs the Roblox build (0002: the fork's `stacked
#                  install` / `update`, reporting in JSON for the manager)
#
# Everything a launcher does -- accounts, sessions, joining servers, updates --
# is the Roblox manager's job (crates/), so the fork's own launcher (the
# `stacked` binary), its Deno plugin runtime and its packaging are left out.
# The engine's window is still GTK4: it lives in the cordial-shell crate, and
# cordial-run links it as a library.
#
# Which source is built is source.json, the one pin this file and the
# Flatpak manifest share (crates/core/tests/cordial_pin.rs keeps them in
# step). README.md beside it says what the manager needs from that source
# and how to move it to another commit.
#
# Patches (source.json's "patches", applied in order):
#   0002  cordial-fetch. Commit it to the fork and this list is empty.
#
# Keeping the cursor inside a fullscreen window is the fork's own (commit
# 4de9752, CORDIAL_NO_FULLSCREEN_CONFINE=1 turns it off); the patch this
# repo carried for it before (0001, in git history) no longer applies.
{pkgs}: let
  inherit (pkgs) lib;
  source = lib.importJSON ./source.json;
in
  pkgs.rustPlatform.buildRustPackage {
    pname = "cordial-mujo";
    inherit (source) version;

    src = pkgs.fetchFromGitHub {
      inherit (source) owner repo rev hash;
      # mcpelauncher-linker and libjnivm are submodules the build needs.
      fetchSubmodules = true;
    };
    patches = map (p: ./. + "/${p}") source.patches;
    # The source's own lock, copied here: buildRustPackage needs it before
    # the source is unpacked. Replace it with the new one on every bump.
    cargoLock.lockFile = ./Cargo.lock;

    stdenv = pkgs.clangStdenv;
    nativeBuildInputs = with pkgs; [clang cmake pkg-config wrapGAppsHook4];
    # cmake is for the build scripts' native parts, not a project to configure.
    dontUseCmakeConfigure = true;
    buildInputs = with pkgs; [
      gtk4
      libadwaita
      glib
      gdk-pixbuf
      cairo
      pango
      graphene
      wayland
      libxkbcommon
      gsettings-desktop-schemas
      # The audio backends are compiled in only when these are found.
      pipewire
      alsa-lib
      libpulseaudio
      zlib
    ];

    cargoBuildFlags = ["-p" "cordial-runtime" "--bin" "cordial-run" "-p" "cordial-update" "--bin" "cordial-fetch"];
    # The fork's suite drives GTK and a compositor.
    doCheck = false;

    CORDIAL_GIT_SHA = "${builtins.substring 0 7 source.rev}-mujo";

    # Loaded with dlopen at run time, so not found through the ELF's own
    # dependencies: Vulkan for the engine, and whichever audio backend answers.
    preFixup = ''
      gappsWrapperArgs+=(
        --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath (with pkgs; [vulkan-loader libGL pipewire libpulseaudio alsa-lib])}"
      )
    '';

    meta = {
      description = "Stacked (mujō's Cordial fork), trimmed to its command-line runtime";
      homepage = "https://github.com/${source.owner}/${source.repo}";
      license = lib.licenses.gpl3Plus;
      platforms = ["x86_64-linux"];
      mainProgram = "cordial-run";
    };
  }
