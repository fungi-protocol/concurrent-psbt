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
