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

        mutants = craneLib.mkCargoDerivation (
          commonArgs
          // {
            inherit cargoArtifacts;
            pnameSuffix = "-mutants-smoke";
            nativeBuildInputs = [ pkgs.cargo-mutants ];
            buildPhaseCargoCommand = ''
              timeout 10 cargo mutants --no-shuffle -vV || test $? -eq 124
            '';
            installPhase = ''
              mkdir -p $out
              cp -r mutants.out/* $out/ 2>/dev/null || true
            '';
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
