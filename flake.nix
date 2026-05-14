{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" ];

      perSystem = { system, pkgs, ... }: {
        _module.args.pkgs = import inputs.nixpkgs {
          inherit system;
          overlays = [ inputs.rust-overlay.overlays.default ];
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            (rust-bin.selectLatestNightlyWith (t: t.default))
            cargo-nextest
            rust-analyzer
          ];
        };
      };

      # TODO
      # - crane based package derivations
      # - format nix and rust code using treefmt flake
      # - flake checks for:
      #   - tests
      #   - test coverage (produce coverage artifacts suitable for export in CI)
      #   - mutation testing
      #   - fuzzing
    };
}
