{
  description = "mujō Roblox manager: several Roblox accounts, launched into one server";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    # Bundles a derivation's closure into one AppImage (userns-chroot AppRun).
    nix-appimage = {
      url = "github:ralismark/nix-appimage";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    nix-appimage,
  }: let
    system = "x86_64-linux";
    pkgs = nixpkgs.legacyPackages.${system};
  in {
    packages.${system} = {
      default = self.packages.${system}.roblox-manager;
      roblox-manager = import ./package.nix {inherit pkgs;};
      cordial-mujo = import ./cordial/package.nix {inherit pkgs;};
      roblox-manager-appimage = import ./appimage.nix {
        inherit pkgs nix-appimage;
        inherit (nixpkgs) lib;
        inherit (self.packages.${system}) roblox-manager;
      };
    };

    nixosModules.default = ./module.nix;
  };
}
