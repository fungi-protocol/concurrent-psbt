# Bolero harnesses (formal/bolero): one `bolero::check!` per law, run randomized under
# `cargo test`.
#
# The harnesses live in the crate (`lattice/bolero_laws.rs`) behind the off-by-default
# `bolero-tests` feature, so the regular test and coverage checks don't see them.
{
  perSystem =
    {
      pkgs,
      commonArgs,
      cargoArtifactsDev,
      toolchains,
      ...
    }:
    let
      # `cargo test` over the Bolero harnesses only. Harnesses run on random inputs (1000
      # iterations by default, more where a harness asks for it).
      boleroCheck =
        {
          pname,
          src ? commonArgs.src,
          testArgs ? "bolero_laws::",
        }:
        toolchains.nightly.mkCargoDerivation (
          commonArgs
          // {
            inherit pname src;
            version = "0";
            cargoArtifacts = cargoArtifactsDev;
            CARGO_PROFILE = "dev";
            buildPhaseCargoCommand = ''
              set -o pipefail
              mkdir -p $out
              cargo test -p concurrent-psbt --lib --no-default-features --features bolero-tests \
                ${testArgs} 2>&1 | tee $out/bolero.log
            '';
            installPhase = "true";
          }
        );

      # `Err ⊔ Ok` forgets the `Ok` value, which breaks commutativity even for a flat
      # `try_join`. Random inputs find it at once; only the one harness runs.
      rejectsDroppedValue = pkgs.testers.testBuildFailure' {
        drv = boleroCheck {
          pname = "formal-bolero-dropped-value";
          src = pkgs.applyPatches {
            name = "formal-bolero-dropped-value-src";
            src = commonArgs.src;
            patches = [ ./bolero/dropped-value.patch ];
          };
          testArgs = "-- --exact lattice::bolero_laws::flat_commutative";
        };
        # `cargo test` exits 101 when a test fails.
        expectedBuilderExitCode = 101;
        expectedBuilderLogEntries = [ "bolero_laws::flat_commutative ... FAILED" ];
      };
    in
    {
      checks = {
        formal-bolero = boleroCheck { pname = "formal-bolero"; };
        formal-bolero-rejects-dropped-value = rejectsDroppedValue;
      };
    };
}
