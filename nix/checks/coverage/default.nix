{
  perSystem =
    {
      checkArgs,
      cargoArtifactsDev,
      pkgs,
      rev,
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

      mkCoverageGate =
        suffix: coveragePercent: collections:
        pkgs.runCommand "concurrent-psbt-coverage${suffix}-${rev}"
          {
            nativeBuildInputs = [ pkgs.lcov ];
          }
          ''
            bash ${./gate.sh} \
              "$out" \
              '${toString coveragePercent}' \
              ${pkgs.lib.escapeShellArgs (map (collection: "${collection}/coverage.lcov") collections)}
          '';

      coverageCollections = {
        coverage-collect-prop-only = mkCoverageCollection "-prop-only" "prop-tests";
        coverage-collect-unit-only = mkCoverageCollection "-unit-only" "unit-tests";
      };
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
        coverage = mkCoverageGate "" 100 (builtins.attrValues coverageCollections);
        coverage-prop-only = mkCoverageGate "-prop-only" 100 [
          coverageCollections.coverage-collect-prop-only
        ];
        coverage-unit-only = mkCoverageGate "-unit-only" 100 [
          coverageCollections.coverage-collect-unit-only
        ];
      };
    };
}
