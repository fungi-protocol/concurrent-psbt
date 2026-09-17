{
  perSystem =
    {
      checkArgs,
      cargoArtifactsDev,
      pkgs,
      toolchains,
      ...
    }:
    {
      checkTags.mutants = [ ];

      checks.mutants = toolchains.nightly.mkCargoDerivation (
        checkArgs
        // {
          cargoArtifacts = cargoArtifactsDev;
          CARGO_PROFILE = "dev";
          pnameSuffix = "-mutants";
          nativeBuildInputs = [
            pkgs.cargo-mutants
            pkgs.cargo-nextest
          ];
          buildPhaseCargoCommand = ''
            cargo mutants --in-place --test-tool nextest
          '';
          installPhase = "mkdir -p $out";
        }
      );
    };
}
