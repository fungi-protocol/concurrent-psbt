{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
    treefmt-nix.url = "github:numtide/treefmt-nix";
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" ];

      imports = [
        ./nix/toolchain.nix
        ./nix/package.nix
        ./nix/checks.nix
        ./nix/devshell.nix
        ./nix/treefmt.nix
      ];

      # TODO
      # - format nix and rust code using treefmt flake
      # - flake checks for:
      #   - tests
      #   - test coverage (cargo-llvm-cov, produce artifacts suitable for export in CI)
      #   - mutation testing
      #   - fuzzing (cargo-fuzz, honggfuzz, afl)
    };
}
