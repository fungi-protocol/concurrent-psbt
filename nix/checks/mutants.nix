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
          # Crane disables incremental compilation, but cargo-mutants rebuilds
          # the crate once per mutant in the same target directory.
          CARGO_BUILD_INCREMENTAL = "true";
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
