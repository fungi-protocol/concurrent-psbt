{ inputs, ... }:
{
  perSystem =
    { system, ... }:
    let
      pkgs = import inputs.nixpkgs {
        inherit system;
        overlays = [ inputs.rust-overlay.overlays.default ];
      };

      commonExtensions = [
        "clippy"
        "rust-analyzer"
        "rust-src"
      ];

      # The host target is implicit. wasm32-unknown-unknown feeds the
      # browser-facing crates (the wasm-bindgen wrapper and the PWA build):
      # the rust-overlay override ships the wasm32 std rlibs alongside the
      # host ones, so `cargo check --target wasm32-unknown-unknown` works in
      # the devshell and in wasm package/check derivations.
      commonTargets = [ "wasm32-unknown-unknown" ];

      rustToolchains = {
        nightly = pkgs.rust-bin.selectLatestNightlyWith (
          t:
          t.default.override {
            extensions = commonExtensions ++ [ "llvm-tools" ];
            targets = commonTargets;
          }
        );
        beta = pkgs.rust-bin.beta.latest.default.override {
          extensions = commonExtensions;
          targets = commonTargets;
        };
        stable = pkgs.rust-bin.stable.latest.default.override {
          extensions = commonExtensions;
          targets = commonTargets;
        };
      };

      mkCraneLib = _: rust: (inputs.crane.mkLib pkgs).overrideToolchain rust;
      toolchains = builtins.mapAttrs mkCraneLib rustToolchains;
    in
    {
      _module.args = {
        inherit pkgs toolchains;
      };
    };
}
