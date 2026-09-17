{
  perSystem =
    {
      checkArgs,
      cargoArtifactsDev,
      pkgs,
      toolchains,
      ...
    }:
    let
      mkCoverageCollection =
        suffix: features:
        toolchains.nightly.mkCargoDerivation (
          checkArgs
          // {
            cargoArtifacts = cargoArtifactsDev;
            pnameSuffix = "-coverage-collect${suffix}";
            nativeBuildInputs = with pkgs; [
              cargo-llvm-cov
              cargo-nextest
            ];
            buildPhaseCargoCommand = ''
              bash ${./collect.sh} \
                "$out" \
                '${features}'
            '';
            installPhase = "true";
          }
        );

      # The gate enforces the threshold over one tracefile; merging is the
      # report's job.
      mkCoverageGate =
        suffix: coveragePercent: collection:
        pkgs.runCommand "concurrent-psbt-coverage${suffix}"
          {
            nativeBuildInputs = [ pkgs.lcov ];
          }
          ''
            bash ${./gate.sh} \
              "$out" \
              '${toString coveragePercent}' \
              ${collection}/coverage.lcov
          '';

      coverageCollections = {
        coverage-collect-prop-only = mkCoverageCollection "-prop-only" "prop-tests";
        coverage-collect-unit-only = mkCoverageCollection "-unit-only" "unit-tests";
      };

      # One report over every collection, for publishing. The gates enforce the
      # threshold; this merges their native lcov output.
      merged =
        pkgs.runCommand "concurrent-psbt-coverage"
          {
            nativeBuildInputs = [ pkgs.lcov ];
          }
          ''
            bash ${./merge.sh} "$out" ${
              pkgs.lib.escapeShellArgs (
                map (collection: "${collection}/coverage.lcov") (builtins.attrValues coverageCollections)
              )
            }
          '';
    in
    {
      checkTags = pkgs.lib.genAttrs [
        "coverage"
        "coverage-collect-prop-only"
        "coverage-collect-unit-only"
        "coverage-prop-only"
        "coverage-unit-only"
      ] (_: [ "nightly" ]);

      checks = coverageCollections // {
        coverage = merged;
        coverage-prop-only = mkCoverageGate "-prop-only" 100 coverageCollections.coverage-collect-prop-only;
        coverage-unit-only = mkCoverageGate "-unit-only" 100 coverageCollections.coverage-collect-unit-only;
      };
    };
}
