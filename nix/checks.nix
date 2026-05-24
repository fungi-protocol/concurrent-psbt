{ inputs, ... }:
{
  perSystem =
    {
      pkgs,
      commonArgs,
      cargoArtifactsRelease,
      cargoArtifactsDev,
      toolchains,
      ...
    }:
    let
      rev = inputs.self.shortRev or "dirty";
      checkArgs = commonArgs // {
        version = rev;
        dontFixup = true;
        doInstallCargoArtifacts = false;
        CARGO_PROFILE = "";
      };
      src = commonArgs.src;

      profiles = {
        dev = "dev";
        release = "release";
      };

      mkTestCheck =
        profile: craneLib:
        let
          deps = craneLib.buildDepsOnly (commonArgs // { CARGO_PROFILE = profile; });
        in
        craneLib.cargoNextest (
          checkArgs
          // {
            cargoArtifacts = deps;
            CARGO_PROFILE = profile;
            cargoNextestExtraArgs = "--no-tests=warn";
          }
        );

      testChecks = pkgs.lib.concatMapAttrs (
        tcName: craneLib:
        pkgs.lib.mapAttrs' (
          profName: profile:
          pkgs.lib.nameValuePair "tests-${tcName}-${profName}" (mkTestCheck profile craneLib)
        ) profiles
      ) toolchains;

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
              bash ${./coverage/collect.sh} \
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
            bash ${./coverage/gate.sh} \
              "$out" \
              '${toString coveragePercent}' \
              ${pkgs.lib.escapeShellArgs (map (collection: "${collection}/coverage.lcov") collections)}
          '';

      coverageCollections = {
        coverage-collect-prop-only = mkCoverageCollection "-prop-only" "prop-tests";
        coverage-collect-unit-only = mkCoverageCollection "-unit-only" "unit-tests";
      };

      checks = testChecks // {
        coverage-gate-tests =
          pkgs.runCommand "coverage-gate-tests-${rev}"
            {
              flakeSrc = inputs.self;
              nativeBuildInputs = [ pkgs.lcov ];
            }
            ''
              bash ${./coverage/test-gate.sh} \
                "$flakeSrc/nix/coverage/gate.sh" \
                "$out"
            '';

        build = toolchains.nightly.buildPackage (checkArgs // { cargoArtifacts = cargoArtifactsRelease; });

        coverage-collect-prop-only = coverageCollections.coverage-collect-prop-only;
        coverage-collect-unit-only = coverageCollections.coverage-collect-unit-only;
        coverage = mkCoverageGate "" 100 (builtins.attrValues coverageCollections);
        coverage-prop-only = mkCoverageGate "-prop-only" 100 [
          coverageCollections.coverage-collect-prop-only
        ];
        coverage-unit-only = mkCoverageGate "-unit-only" 100 [
          coverageCollections.coverage-collect-unit-only
        ];

        clippy = toolchains.nightly.cargoClippy (
          checkArgs
          // {
            cargoArtifacts = cargoArtifactsDev;
            cargoClippyExtraArgs = "--all-targets --all-features -- -D warnings";
          }
        );

        doc = toolchains.nightly.cargoDoc (
          commonArgs
          // {
            cargoArtifacts = cargoArtifactsDev;
            CARGO_PROFILE = "dev";
            cargoDocExtraArgs = "--no-deps --all-features";
            RUSTDOCFLAGS = "-D warnings";
          }
        );

        cargo-sort =
          pkgs.runCommand "cargo-sort-${rev}"
            {
              inherit src;
              nativeBuildInputs = [ pkgs.cargo-sort ];
            }
            ''
              cargo-sort --check --workspace "$src"
              mkdir -p $out
            '';

        unused-lints = toolchains.nightly.mkCargoDerivation (
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

        no-todo-comments = pkgs.runCommand "no-todo-comments-${rev}" { inherit src; } ''
          if grep -rn --exclude-dir=contrib 'TO[D]O\|FIX[M]E' $src/ 2>/dev/null; then
            echo "FAIL: unresolved work-item markers found"
            exit 1
          fi
          mkdir -p $out
        '';
      };
    in
    {
      checks = checks // {
        quick = pkgs.symlinkJoin {
          name = "quick-checks-${rev}";
          paths = with checks; [
            tests-nightly-dev
            clippy
          ];
        };
        lint = pkgs.symlinkJoin {
          name = "lint-checks-${rev}";
          paths = with checks; [
            cargo-sort
            clippy
            doc
            unused-lints
            no-todo-comments
          ];
        };
        nightly = pkgs.symlinkJoin {
          name = "nightly-checks-${rev}";
          paths = builtins.attrValues checks;
        };
      };
    };
}
