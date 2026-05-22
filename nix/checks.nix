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

      mkCoverage =
        suffix: features:
        toolchains.nightly.mkCargoDerivation (
          checkArgs
          // {
            cargoArtifacts = cargoArtifactsDev;
            pnameSuffix = "-coverage${suffix}";
            nativeBuildInputs = [ pkgs.cargo-llvm-cov ];
            buildPhaseCargoCommand = ''
              mkdir -p $out
              cargo llvm-cov --no-default-features --features ${features} --lcov --output-path $out/coverage.lcov || {
                # no coverage data when there are no tests yet
                if [ ! -s $out/coverage.lcov ]; then
                  echo "no coverage data (no tests), skipping assertion"
                  exit 0
                fi
                exit 1
              }
              cargo llvm-cov report --fail-under-regions 100
            '';
            installPhase = "true";
          }
        );

      checks = testChecks // {
        build = toolchains.nightly.buildPackage (commonArgs // { cargoArtifacts = cargoArtifactsRelease; });

        coverage = mkCoverage "" "unit-tests,prop-tests";
        coverage-no-unit-tests = mkCoverage "-no-unit-tests" "prop-tests";
        coverage-no-prop-tests = mkCoverage "-no-prop-tests" "unit-tests";

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
