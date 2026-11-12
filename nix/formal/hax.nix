# hax: Rust to Lean extraction and the Lean libraries its output is built against.
#
# hax's Lean backend does not use hax's own frontend or engine: `cargo hax extract` runs
# Charon and Aeneas and scaffolds a Lean package around the result, built against hax's own
# Lean models of `core` and `alloc` (`CoreModels`) and tactics (`Hax`). So this module needs
# upstream hax (the `cargo-hax` driver), the Aeneas fork commit hax pins (the `aeneas` binary
# and Charon) and the Lean library of both. The library imports Mathlib, which nixpkgs'
# `leanPackages` already packages at the Lean version (v4.35.0) hax targets.
{ inputs, lib, ... }:
{
  perSystem =
    { pkgs, system, ... }:
    let
      haxPkgs = inputs.hax.packages.${system} or { };
      aeneasPkgs = inputs.aeneas-hax.packages.${system} or { };
      supported = haxPkgs ? hax && aeneasPkgs ? aeneas;
      hax = haxPkgs.hax;
      aeneas = aeneasPkgs.aeneas;

      lp = pkgs.leanPackages;

      aeneasLean = lp.buildLakePackage {
        pname = "lean4-aeneas";
        version = "0-unstable";
        src = "${inputs.aeneas-hax}/backends/lean";
        leanPackageName = "aeneas";
        leanDeps = [
          lp.mathlib
          lp.Cli
        ];
        # `AeneasMeta` precompiles its modules unless `CI` is set, which needs shared
        # libraries of every dependency.
        env.CI = "1";
      };

      # `Hax` (tactics) and `CoreModels` (Lean extraction of hax's models of core and alloc).
      # Its lakefile fetches `aeneas` with git; lake is told to use the Nix-built one instead.
      haxLean = lp.buildLakePackage {
        pname = "lean4-hax";
        version = "0.3.41";
        src = "${inputs.hax}/hax-lib/proof-libs/lean";
        leanPackageName = "hax";
        leanDeps = [ aeneasLean ];
        buildTargets = [
          "Hax"
          "CoreModels"
        ];
        env.CI = "1";
      };
    in
    {
      packages = lib.mkIf supported {
        inherit hax;
        hax-lean = haxLean;
      };
      checks = lib.mkIf supported {
        formal-hax-tools =
          pkgs.runCommand "formal-hax-tools"
            {
              nativeBuildInputs = [
                hax
                aeneas
              ];
              lib = haxLean;
            }
            ''
              mkdir -p $out
              { cargo-hax hax --version || true; aeneas -version; charon version; } 2>&1 | tee $out/versions.txt
            '';
      };
      formal.packages = lib.optionals supported [ hax ];
    };
}
