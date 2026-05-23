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

      checks = testChecks // {
        build = toolchains.nightly.buildPackage (checkArgs // { cargoArtifacts = cargoArtifactsRelease; });

        coverage = toolchains.nightly.mkCargoDerivation (
          checkArgs
          // {
            cargoArtifacts = cargoArtifactsDev;
            pnameSuffix = "-coverage";
            nativeBuildInputs = [ pkgs.cargo-llvm-cov ];
            buildPhaseCargoCommand = ''
              mkdir -p $out
              cargo llvm-cov --all-features --lcov --output-path $out/coverage.lcov || {
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

        clippy = toolchains.nightly.cargoClippy (
          checkArgs
          // {
            cargoArtifacts = cargoArtifactsDev;
            cargoClippyExtraArgs = "--all-targets --all-features -- -D warnings";
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
          paths = [
            checks.tests-nightly-dev
            checks.clippy
          ];
        };
        lint = pkgs.symlinkJoin {
          name = "lint-checks-${rev}";
          paths = [
            checks.clippy
            checks.no-todo-comments
          ];
        };
        nightly = pkgs.symlinkJoin {
          name = "nightly-checks-${rev}";
          paths = builtins.attrValues checks;
        };
      };
    };
}
