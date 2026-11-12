# Creusot deductive verifier (formal/creusot).
#
# Not in nixpkgs. Upstream's flake builds creusot-rustc, cargo-creusot, Why3, why3find and the
# SMT solvers from pinned sources; `free` selects alt-ergo-free over the non-commercially
# licensed alt-ergo. The input deliberately has no `follows`: it keeps its own nixpkgs,
# crane, flake-parts and rust-overlay pins, which upstream builds and tests the toolchain and
# solvers against.
{ inputs, lib, ... }:
{
  perSystem =
    { pkgs, system, ... }:
    let
      creusotPkgs = inputs.creusot.packages.${system} or { };
      supported = creusotPkgs ? free;
      creusot = creusotPkgs.free;

      craneLib = inputs.crane.mkLib pkgs;
      src = ../../formal/creusot;

      # The wrapped `cargo` patches `creusot-std` to its own path copy, so the crate needs only
      # that copy's registry dependencies. Vendoring upstream's lock covers them at the versions
      # upstream tested, and lets cargo resolve this crate offline without a lock of its own.
      # The lock is read from the flake input, not from the built package: reading it from the
      # build output would be import-from-derivation, and evaluating the check would then
      # build Creusot.
      cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = "${inputs.creusot}/Cargo.lock"; };

      verify =
        name: src:
        pkgs.runCommand name
          {
            inherit src;
            nativeBuildInputs = [ creusot ];
          }
          ''
            set -o pipefail
            export HOME=$TMPDIR CARGO_HOME=$TMPDIR/cargo CARGO_NET_OFFLINE=true
            mkdir -p $CARGO_HOME $out
            cat ${cargoVendorDir}/config.toml >> $CARGO_HOME/config.toml
            cp -r $src crate && chmod -R u+w crate && cd crate
            cargo creusot 2>&1 | tee $out/creusot.log
            cp -r verif $out/
          '';

      # `join_mut` pushes every value, so the result can hold duplicates. The `unique` loop
      # invariant must stop being provable.
      rejectsDuplicates = pkgs.testers.testBuildFailure' {
        drv = verify "formal-creusot-duplicates" (
          pkgs.applyPatches {
            name = "formal-creusot-duplicates-src";
            inherit src;
            patches = [ ./creusot/duplicates.patch ];
          }
        );
        expectedBuilderLogEntries = [ "Goal Coma.vc_join_mut_T: ✘" ];
      };
    in
    {
      packages = lib.mkIf supported { inherit creusot; };
      checks = lib.mkIf supported {
        formal-creusot = verify "formal-creusot" src;
        formal-creusot-rejects-duplicates = rejectsDuplicates;
      };
      formal.packages = lib.optionals supported [ creusot ];
    };
}
