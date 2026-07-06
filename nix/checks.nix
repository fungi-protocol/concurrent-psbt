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
            cargoNextestExtraArgs = "--user-config-file ${./nextest-record.toml}";
            nativeBuildInputs = [ pkgs.unzip ];
            preCheck = ''
              export NEXTEST_STATE_DIR="$TMPDIR/nextest-state"
              mkdir -p "$NEXTEST_STATE_DIR"
            '';
            postCheck = ''
              cargo nextest store export \
                --no-pager \
                --user-config-file ${./nextest-record.toml} \
                --archive-file "$out/nextest-run.zip" \
                latest
              unzip -tqq "$out/nextest-run.zip"
            '';
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

      ptj-bin = toolchains.nightly.buildPackage (
        commonArgs
        // {
          cargoArtifacts = cargoArtifactsDev;
          pnameSuffix = "-ptj";
        }
      );

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
        mutants = toolchains.nightly.mkCargoDerivation (
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

        validate-commits-repository-probes =
          pkgs.runCommand "validate-commits-repository-probes-${rev}"
            {
              nativeBuildInputs = [ pkgs.git ];
            }
            ''
              bash ${./checks/validate-commits-repository-probes.sh} ${./validate-commits.sh}
              mkdir -p $out
            '';

        joinpsbt-gap =
          pkgs.runCommand "joinpsbt-gap-${rev}"
            {
              nativeBuildInputs = with pkgs; [
                bitcoind
                jq
              ];
              testScripts = ../contrib/tests;
            }
            ''
              export PATH="${ptj-bin}/bin:$PATH"
              bash "$testScripts/joinpsbt-gap.sh"
            '';

        sneakernet-lattice =
          pkgs.runCommand "sneakernet-lattice-${rev}"
            {
              nativeBuildInputs = with pkgs; [
                bitcoind
                jq
              ];
              testScripts = ../contrib/tests;
            }
            ''
              export PATH="${ptj-bin}/bin:$PATH"
              bash "$testScripts/sneakernet-lattice.sh"
            '';

        ptj-sneakernet =
          pkgs.runCommand "ptj-sneakernet-${rev}"
            {
              nativeBuildInputs = with pkgs; [
                bitcoind
                jq
              ];
              testScripts = ../contrib/tests;
            }
            ''
              export PATH="${ptj-bin}/bin:$PATH"
              bash "$testScripts/ptj-sneakernet.sh"
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
            validate-commits-repository-probes
            unused-lints
            no-todo-comments
          ];
        };
        nightly = pkgs.symlinkJoin {
          name = "nightly-checks-${rev}";
          paths = builtins.attrValues (removeAttrs checks [ "mutants" ]);
        };
      };
    };
}
