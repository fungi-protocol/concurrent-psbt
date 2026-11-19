# Aeneas + Charon: extract the crate's Rust to Lean and prove things about the extraction
# (formal/aeneas).
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

      crate = ../../crates/concurrent-psbt;

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

      lean = aeneasLean.lean4;
      leanPath = lib.concatMapStringsSep ":" (p: "${p}/.lake/build/lib/lean") (
        [ aeneasLean ] ++ aeneasLean.allLeanDeps
      );

      # Charon extracts the crate to LLBC and Aeneas turns that into Lean. A standalone
      # manifest would only matter to cargo, which `charon rustc` bypasses, so the crate's own
      # sources are compiled directly (no dependencies are needed outside the tests).
      extract =
        name: crate:
        pkgs.runCommand name
          {
            src = crate;
            nativeBuildInputs = [ aeneas ];
          }
          ''
            set -o pipefail
            export HOME=$TMPDIR
            mkdir -p $out
            charon rustc --dest-file concurrent_psbt.llbc --preset=aeneas \
              -- $src/src/lib.rs --crate-name=concurrent_psbt --crate-type=rlib \
              --edition=2024 --allow=unused 2>&1 | tee $out/charon.log
            aeneas concurrent_psbt.llbc -backend lean -dest $out \
              -no-progress-bar -print-error-emitters 2>&1 | tee $out/aeneas.log
            cp concurrent_psbt.llbc $out/
          '';

      # Typecheck the extraction and the proofs about it, then audit the axioms of the
      # theorems listed in Axioms.lean: the extraction declares an `axiom` for each external
      # function Aeneas has no model of, and no theorem may depend on one, or on `sorry`.
      verify =
        name: crate:
        let
          extracted = extract "${name}-extract" crate;
        in
        pkgs.runCommand name
          {
            src = ../../formal/aeneas;
            model = ../../formal/lean;
            inherit extracted;
            nativeBuildInputs = [ lean ];
          }
          ''
            set -o pipefail
            export LEAN_PATH=${leanPath}:$PWD/build
            mkdir -p build $out
            cp $model/JoinResult.lean $extracted/ConcurrentPsbt.lean $src/*.lean .
            for m in JoinResult ConcurrentPsbt Proofs; do
              lean -o build/$m.olean $m.lean | tee -a $out/lean.txt
            done
            lean Axioms.lean | tee $out/axioms.txt
            if grep -q -e sorryAx -e "declaration uses 'sorry'" $out/*.txt ||
              grep -q concurrent_psbt $out/axioms.txt; then
              echo "a theorem depends on sorry or on an external axiom" >&2
              exit 1
            fi
          '';
    in
    {
      packages = lib.mkIf supported {
        inherit aeneas;
        aeneas-lean = aeneasLean;
      };
      checks = lib.mkIf supported {
        formal-aeneas = verify "formal-aeneas" crate;

        # `err ⊔ ok` forgets the `ok` value, the same bug as the Lean fixture, now in the
        # crate's own code. The extracted `join` no longer calls `Join::join` in that arm,
        # so the proof of `join_refines` stops there. Lean doesn't name the failing
        # theorem, so the log entry is the tactic error, which doesn't depend on line numbers.
        formal-aeneas-rejects-dropped-value = pkgs.testers.testBuildFailure' {
          drv = verify "formal-aeneas-dropped-value" (
            pkgs.applyPatches {
              name = "formal-aeneas-dropped-value-src";
              src = crate;
              patches = [ ./aeneas/dropped-value.patch ];
            }
          );
          expectedBuilderLogEntries = [ "error: `simp` made no progress" ];
        };
      };
      formal.packages = lib.optionals supported [ aeneas ];
    };
}
