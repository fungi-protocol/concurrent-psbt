# Lean 4 proofs about the `JoinResult` lattice model (formal/lean).
#
# Core Lean only (no Mathlib/lake), so the check is a plain `lean` invocation.
# Typechecking proves the theorems; the audit rejects any `sorry`.
{
  perSystem =
    { pkgs, ... }:
    let
      lean = pkgs.lean4;

      # Typecheck every module of `src`, then audit the axioms of the theorems
      # listed in Axioms.lean. A `sorry` shows up as a warning when its module
      # is checked and as `sorryAx` in the axiom list.
      prove =
        name: src:
        pkgs.runCommand name
          {
            inherit src;
            nativeBuildInputs = [ lean ];
          }
          ''
            set -o pipefail
            cp $src/*.lean . && mkdir -p $out
            for m in JoinResult Modulo; do
              lean -o $out/$m.olean $m.lean | tee -a $out/lean.txt
            done
            LEAN_PATH=$out lean Axioms.lean | tee $out/axioms.txt
            if grep -q -e sorryAx -e "declaration uses 'sorry'" $out/*.txt; then
              echo "a theorem depends on sorry" >&2
              exit 1
            fi
          '';

      src = ../../formal/lean;
    in
    {
      formal.packages = [ lean ];

      checks.formal-lean = prove "formal-lean" src;

      # `err ⊔ ok` forgets the `ok` value, so the proofs of `flat_comm` and
      # `flat_assoc` no longer typecheck. Lean doesn't name the failing theorem,
      # so the log entries are the error and a goal that mentions the entry
      # `err ⊔ ok` should have kept; neither depends on line numbers.
      checks.formal-lean-rejects-dropped-value = pkgs.testers.testBuildFailure' {
        drv = prove "formal-lean-dropped-value" (
          pkgs.applyPatches {
            name = "formal-lean-dropped-value-src";
            inherit src;
            patches = [ ./lean/dropped-value.patch ];
          }
        );
        expectedBuilderLogEntries = [
          "error: unsolved goals"
          "⊢ Conflict.singleton"
        ];
      };
    };
}
