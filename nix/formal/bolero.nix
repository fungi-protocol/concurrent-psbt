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
    in
    {
      checks = {
        formal-bolero = boleroCheck { pname = "formal-bolero"; };
      };
    };
}
