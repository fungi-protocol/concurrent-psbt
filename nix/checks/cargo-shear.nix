{
  perSystem =
    {
      commonArgs,
      cargoArtifactsDev,
      pkgs,
      toolchains,
      ...
    }:
    {
      checkTags.cargo-shear = [
        "lint"
        "nightly"
      ];

      checks.cargo-shear = toolchains.nightly.mkCargoDerivation (
        commonArgs
        // {
          cargoArtifacts = cargoArtifactsDev;
          CARGO_PROFILE = "dev";
          pnameSuffix = "-cargo-shear";
          nativeBuildInputs = [ pkgs.cargo-shear ];
          buildPhaseCargoCommand = "cargo shear";
          installPhase = "mkdir -p $out";
        }
      );
    };
}
