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

      # A gate enforces a threshold over the tracefile of one report: a
      # collection, or the merged report.
      mkCoverageGate =
        suffix: coveragePercent: report:
        pkgs.runCommand "concurrent-psbt-coverage${suffix}"
          {
            nativeBuildInputs = [ pkgs.lcov ];
          }
          ''
            bash ${./gate.sh} \
              "$out" \
              '${toString coveragePercent}' \
              ${report}/coverage.lcov
          '';

      # Line coverage each suite must reach on its own, independently of the
      # other. They only catch a collapse of one category of test; lower them
      # deliberately, from measured coverage, never to make a failing change
      # pass.
      coverageThresholds = {
        prop-only = 95;
        unit-only = 95;
      };

      coverageCollections = {
        coverage-collect-prop-only = mkCoverageCollection "-prop-only" "prop-tests";
        coverage-collect-unit-only = mkCoverageCollection "-unit-only" "unit-tests";
      };

      # One report over every collection, ungated so that it can be published
      # even when a gate fails.
      merged =
        pkgs.runCommand "concurrent-psbt-coverage-report"
          {
            nativeBuildInputs = [
              pkgs.lcov
              (pkgs.callPackage ./lcov2xml.nix { })
            ];
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
        "coverage-report"
        "coverage-unit-only"
      ] (_: [ "nightly" ]);

      checks = coverageCollections // {
        coverage-report = merged;
        # Together the suites must always cover every line, so this threshold
        # is fixed. Only the per-suite thresholds may be lowered, deliberately.
        coverage = mkCoverageGate "" 100 merged;
        coverage-prop-only =
          mkCoverageGate "-prop-only" coverageThresholds.prop-only
            coverageCollections.coverage-collect-prop-only;
        coverage-unit-only =
          mkCoverageGate "-unit-only" coverageThresholds.unit-only
            coverageCollections.coverage-collect-unit-only;
      };
    };
}
