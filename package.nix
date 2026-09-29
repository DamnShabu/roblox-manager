# The roblox-manager derivation, shared by the NixOS module (module.nix)
# and the portable AppImage (appimage.nix).
{pkgs}: let
  cordial = import ./cordial/package.nix {inherit pkgs;};
  # The icon font: Material Symbols Rounded alone, copied out of the package
  # so the other two families (~20 MB) stay out of the closure.
  symbols = pkgs.runCommand "rbxmgr-symbols" {} ''
    mkdir -p $out/share/fonts
    cp ${pkgs.material-symbols}/share/fonts/truetype/MaterialSymbolsRounded* $out/share/fonts/
  '';
  # The UI's typefaces and icons, added to whatever fonts the host already
  # has rather than installed system-wide: nothing else asks for them.
  fontDirs = [
    "${pkgs.source-sans}/share/fonts"
    "${pkgs.jetbrains-mono}/share/fonts"
    "${symbols}/share/fonts"
  ];
  fonts = pkgs.writeText "rbxmgr-fonts.conf" ''
    <?xml version="1.0"?>
    <!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
    <fontconfig>
      <include ignore_missing="yes">/etc/fonts/fonts.conf</include>
      ${pkgs.lib.concatMapStrings (d: "<dir>${d}</dir>\n") fontDirs}
    </fontconfig>
  '';
in
  # wrapGAppsHook4 rather than a hand-rolled GI_TYPELIB_PATH: a GTK4 +
  # libadwaita app also needs gsettings schemas, icon themes and pixbuf
  # loaders, and enumerating those by hand is how you get an app that starts
  # and then draws nothing.
  pkgs.rustPlatform.buildRustPackage {
    pname = "mujo-roblox-manager";
    version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
    src = pkgs.lib.fileset.toSource {
      root = ./.;
      # The Cordial pin and the Flatpak manifest: a test checks they agree.
      fileset = pkgs.lib.fileset.unions [
        ./Cargo.toml
        ./Cargo.lock
        ./clippy.toml
        ./crates
        ./cordial/source.json
        ./packaging/flatpak/io.github.mujo.RobloxManager.yml
      ];
    };
    cargoLock.lockFile = ./Cargo.lock;
    strictDeps = true;

    nativeBuildInputs = [pkgs.pkg-config pkgs.wrapGAppsHook4];
    # adwaita-icon-theme: libadwaita's symbolic icons, which a host outside
    # NixOS (the AppImage) is not guaranteed to have. librsvg: the pixbuf
    # loader that draws the app's own SVG icons.
    buildInputs = [pkgs.gtk4 pkgs.libadwaita pkgs.glib pkgs.adwaita-icon-theme pkgs.librsvg];

    # The self-check runs real sh, sleep and Unix sockets; the one test that
    # needs a session bus and a real keyring is ignored by default.
    nativeCheckInputs = [pkgs.bash pkgs.coreutils];

    # The design's mark, "stacked accounts": on a dark tile for launchers,
    # bare in the window's title bar. The Flatpak installs the same files.
    postInstall = ''
      install -Dm644 ${./packaging/icons/roblox-manager.svg} $out/share/icons/hicolor/scalable/apps/roblox-manager.svg
      install -Dm644 ${./packaging/icons/roblox-manager-mark.svg} $out/share/icons/hicolor/scalable/apps/roblox-manager-mark.svg
      install -Dm644 ${./packaging/icons/roblox-manager.svg} $out/share/icons/hicolor/scalable/apps/io.github.mujo.RobloxManager.svg
    '';

    # The fork of Cordial (cordial-run, cordial-fetch) is prepended, so it is
    # always the one run, whatever else is on PATH. cage is the macro engine's
    # display -- one nested compositor per macro-ready client, which the app
    # then types into itself -- and is appended, so a host's own copy wins.
    # pgrep, kill and nice come from the session.
    preFixup = ''
      gappsWrapperArgs+=(
        --set-default FONTCONFIG_FILE ${fonts}
        --prefix PATH : ${pkgs.lib.makeBinPath [cordial]}
        --suffix PATH : ${pkgs.lib.makeBinPath [pkgs.cage]}
      )
    '';

    # The AppImage writes its own fontconfig file; it adds these.
    passthru = {inherit fontDirs;};
    meta.mainProgram = "roblox-manager";
  }
