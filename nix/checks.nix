{
  perSystem =
    {
      pkgs,
      craneLib,
      commonArgs,
      cargoArtifacts,
      ...
    }:
    {
      checks = {
        tests = craneLib.cargoNextest (
          commonArgs
          // {
            inherit cargoArtifacts;
          }
        );

        coverage = craneLib.mkCargoDerivation (
          commonArgs
          // {
            inherit cargoArtifacts;
            pnameSuffix = "-coverage";
            nativeBuildInputs = [ pkgs.cargo-llvm-cov ];
            buildPhaseCargoCommand = ''
              mkdir -p $out
              cargo llvm-cov --all-features --lcov --output-path $out/coverage.lcov
            '';
            installPhase = "true";
          }
        );
      };
    };
}
