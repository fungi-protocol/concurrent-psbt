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
      demoGuiSrc = inputs.self + /contrib/demo-gui;

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
          }
        );

      mkMutants =
        suffix: features:
        toolchains.nightly.mkCargoDerivation (
          checkArgs
          // {
            cargoArtifacts = cargoArtifactsDev;
            CARGO_PROFILE = "dev";
            pnameSuffix = "-mutants${suffix}";
            nativeBuildInputs = [
              pkgs.cargo-mutants
              pkgs.cargo-nextest
            ];
            buildPhaseCargoCommand = ''
              cargo mutants --in-place --test-tool nextest --no-default-features --features ${features}
            '';
            installPhase = "mkdir -p $out";
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
        suffix: features: failUnder:
        toolchains.nightly.mkCargoDerivation (
          checkArgs
          // {
            cargoArtifacts = cargoArtifactsDev;
            pnameSuffix = "-coverage${suffix}";
            nativeBuildInputs = with pkgs; [
              cargo-llvm-cov
              cargo-nextest
            ];
            buildPhaseCargoCommand = ''
              cargo llvm-cov nextest --no-report --no-default-features --features ${features}

              mkdir -p $out
              find target/llvm-cov-target -name '*.prof*' -print -quit | grep -q . || {
                echo "coverage: no profile data produced; refusing to pass vacuously" >&2
                exit 1
              }

              cargo llvm-cov report --summary-only
              cargo llvm-cov report --lcov --output-path $out/coverage.lcov --fail-under-regions ${toString failUnder}
            '';
            installPhase = "true";
          }
        );

      checks = testChecks // {
        build = toolchains.nightly.buildPackage (commonArgs // { cargoArtifacts = cargoArtifactsRelease; });

        demo-gui =
          pkgs.runCommand "demo-gui-${rev}"
            {
              inherit demoGuiSrc;
              nativeBuildInputs = with pkgs; [
                nodejs
                typescript
              ];
            }
            ''
              cp -R "$demoGuiSrc" ./demo-gui
              chmod -R u+w ./demo-gui
              cd ./demo-gui

              tsc -p tsconfig.model.json
              tsc -p tsconfig.json
              node --test \
                --experimental-test-coverage \
                --test-coverage-include='dist/backend.js' \
                --test-coverage-include='dist/model.js' \
                --test-coverage-lines=100 \
                --test-coverage-branches=100 \
                --test-coverage-functions=100 \
                test/*.mjs

              mkdir -p "$out"
              cp -R dist "$out/dist"
            '';

        demo-gui-playwright =
          pkgs.runCommand "demo-gui-playwright-${rev}"
            {
              inherit demoGuiSrc;
              nativeBuildInputs = with pkgs; [
                nodejs
                playwright-test
                typescript
              ];
            }
            ''
              cp -R "$demoGuiSrc" ./demo-gui
              chmod -R u+w ./demo-gui
              cd ./demo-gui

              tsc -p tsconfig.model.json
              tsc -p tsconfig.json

              export HOME="$TMPDIR"
              export PLAYWRIGHT_BROWSERS_PATH="${pkgs.playwright-driver.browsers}"
              export PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=true
              export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
              export PLAYWRIGHT_CORE="${pkgs.playwright-test}/lib/node_modules/playwright-core/index.mjs"

              CHROMIUM_BIN=""
              for _c in \
                "$PLAYWRIGHT_BROWSERS_PATH"/chromium-*/chrome-linux64/chrome \
                "$PLAYWRIGHT_BROWSERS_PATH"/chromium-*/chrome-linux/chrome \
                "$PLAYWRIGHT_BROWSERS_PATH"/chromium-*/chrome-mac*/*.app/Contents/MacOS/*; do
                [ -e "$_c" ] && CHROMIUM_BIN="$_c" && break
              done
              if [ -z "$CHROMIUM_BIN" ]; then
                echo "ERROR: no store Chromium under $PLAYWRIGHT_BROWSERS_PATH" >&2
                exit 1
              fi
              export CHROMIUM_BIN
              export DEMO_GUI_HTML="$PWD/index.html"

              echo "node $(node --version); chromium=$CHROMIUM_BIN"
              for spec in test/e2e/*.spec.mjs; do
                echo "--- $(basename "$spec") ---"
                node "$spec"
              done

              mkdir -p "$out"
            '';

        coverage = mkCoverage "" "unit-tests,prop-tests" 100;
        coverage-no-unit-tests = mkCoverage "-no-unit-tests" "prop-tests" 100;
        coverage-no-prop-tests = mkCoverage "-no-prop-tests" "unit-tests" 100;

        mutants = mkMutants "" "unit-tests,prop-tests";

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
            demo-gui
            demo-gui-playwright
          ];
        };
        lint = pkgs.symlinkJoin {
          name = "lint-checks-${rev}";
          paths = with checks; [
            cargo-sort
            clippy
            demo-gui
            doc
            unused-lints
            no-todo-comments
          ];
        };
        nightly = pkgs.symlinkJoin {
          name = "nightly-checks-${rev}";
          paths = builtins.attrValues (
            removeAttrs checks [
              "mutants"
            ]
          );
        };
      };
    };
}
