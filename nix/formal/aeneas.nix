# Aeneas + Charon: extract the crate's Rust to Lean (formal/aeneas).
#
# Upstream's flake builds the `aeneas` OCaml binary and Charon (with its pinned nightly
# toolchain). Its Lean library imports Mathlib, which nixpkgs already packages in
# `leanPackages` at the same Lean version (v4.35.0) the pinned Aeneas revision targets, so the
# library is built with nixpkgs' `buildLakePackage` on top of that Mathlib.
{ inputs, lib, ... }:
{
  perSystem =
    { pkgs, system, ... }:
    let
      aeneasPkgs = inputs.aeneas.packages.${system} or { };
      supported = aeneasPkgs ? aeneas;
      aeneas = aeneasPkgs.aeneas;

      lp = pkgs.leanPackages;

      aeneasLean = lp.buildLakePackage {
        pname = "lean4-aeneas";
        version = "0-unstable";
        src = "${inputs.aeneas}/backends/lean";
        leanPackageName = "aeneas";
        leanDeps = [
          lp.mathlib
          lp.Cli
        ];
        # `AeneasMeta` precompiles its modules unless `CI` is set, which needs shared
        # libraries of every dependency.
        env.CI = "1";
      };
    in
    {
      packages = lib.mkIf supported {
        inherit aeneas;
        aeneas-lean = aeneasLean;
      };
    };
}
