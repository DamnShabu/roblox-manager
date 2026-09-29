# The manager and its desktop entry. Host-specific pieces (impermanence
# paths, DNS pins for a filtering resolver) belong to the importing config.
#
# Note: Roblox's own rules treat simultaneous clients as a policy violation,
# and community reports tie them to anti-cheat flags. Installing this is
# inert; the exposure starts when the second client is launched, which is
# always an explicit click.
{pkgs, ...}: {
  environment.systemPackages = [
    (import ./package.nix {inherit pkgs;})
    (pkgs.makeDesktopItem {
      name = "roblox-manager";
      desktopName = "Roblox Manager";
      genericName = "Roblox Account Manager";
      comment = "Launch several Roblox accounts into the same server";
      icon = "roblox-manager";
      exec = "roblox-manager";
      terminal = false;
      # Game alone: listing a second main category makes the entry show up twice
      # in menus (desktop-file-validate warns about exactly this).
      categories = ["Game"];
      keywords = ["roblox" "cordial" "account" "alt" "multi" "instance"];
    })
  ];
}
