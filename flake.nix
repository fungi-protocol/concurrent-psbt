{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # hax's Lean backend is the Aeneas pipeline: `cargo hax into lean` runs charon and aeneas.
    # hax pins the aeneas fork commit its `hax-lean` library was built against.
    hax.url = "github:cryspen/hax/a045d15a3bf43fe05096146f488668b521a4ae44";
    aeneas-hax.url = "github:pereq-dev/aeneas/2b9e3ea4662b48c19262be759dbb704f20c24364";
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
