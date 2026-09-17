{
  perSystem =
    {
      commonArgs,
      cargoArtifactsDev,
      toolchains,
      ...
    }:
    {
      checkTags.unused-lints = [
        "lint"
        "nightly"
      ];

      checks.unused-lints = toolchains.nightly.mkCargoDerivation (
        commonArgs
        // {
          cargoArtifacts = cargoArtifactsDev;
          CARGO_PROFILE = "dev";
          pnameSuffix = "-unused-lints";
          buildPhaseCargoCommand = ''
            RUSTFLAGS="''${RUSTFLAGS:-} -D unused" cargo check --all-targets --all-features
          '';
          installPhase = "mkdir -p $out";
        }
      );
    };
}
