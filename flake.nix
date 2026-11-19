{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    # Rust-to-Lean extraction (formal/aeneas). Pinned to the last revision on Lean v4.35.0, the
    # version nixpkgs' `leanPackages` (and its Mathlib) provides. Keeps its own nixpkgs pin.
    aeneas.url = "github:AeneasVerif/aeneas/5a392f4b80a4d0d05abc240eb2a7fa70622a9a35";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];

      imports = [
        ./nix/apps.nix
        ./nix/checks.nix
        ./nix/devshell.nix
        ./nix/formal
        ./nix/package.nix
        ./nix/toolchain.nix
        ./nix/treefmt.nix
      ];
    };
}
