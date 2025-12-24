{
  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    inputs@{ flake-parts, naersk, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      debug = false;
      systems = [
        "x86_64-linux"
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
      ];

      perSystem =
        { pkgs, self', ... }:
        let
          naersk' = pkgs.callPackage naersk { };
        in
        {
          packages.default = naersk'.buildPackage { src = ./.; };

          devShells.default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.cargo-dist
              pkgs.rustc
              pkgs.rustfmt
              pkgs.rustPackages.clippy

              self'.packages.default
            ];
          };
        };
    };
}
