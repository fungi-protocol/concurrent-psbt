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
              # capnp for transport-plugin-api's build.rs (clobbers commonArgs)
              pkgs.capnproto
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
              # capnp for transport-plugin-api's build.rs (clobbers commonArgs)
              capnproto
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

      ptj-bin = toolchains.nightly.buildPackage (
        commonArgs
        // {
          cargoArtifacts = cargoArtifactsDev;
          pnameSuffix = "-ptj";
        }
      );

      checks = testChecks // {
        build = toolchains.nightly.buildPackage (checkArgs // { cargoArtifacts = cargoArtifactsDev; });

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

        demo-gui-webgui-assets = pkgs.runCommand "demo-gui-webgui-assets-${rev}" { inherit src; } ''
          test -f "$src/contrib/demo-gui/dist/app.js"
          test -f "$src/contrib/demo-gui/dist/backend.js"
          grep -q 'backend\.js' "$src/contrib/demo-gui/dist/app.js"
          grep -q 'const BACKEND_JS' "$src/crates/ptj/src/webgui.rs"
          grep -q '"/dist/backend\.js"' "$src/crates/ptj/src/webgui.rs"
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

        # ---- webrtc-over-OHTTP e2e, stage 1: authored guard (cheap, mainline).
        # Analog of demo-gui-webgui-assets: assert the e2e-oblivious harness +
        # its gating exist and are well-formed WITHOUT running the network path.
        # The greps are the anti-rot contract: the rust peer stays
        # required-features-gated off the default target set; the deferred
        # transport keeps its TODO(ground-deps) until a conscious promotion;
        # and the anti-false-positive assertions (directory obliviousness +
        # P2P-only data path) cannot be deleted without failing this check.
        webrtc-e2e-authored =
          pkgs.runCommand "webrtc-e2e-authored-${rev}"
            {
              inherit src;
              testScripts = ../contrib/tests;
            }
            ''
              # transport-str0m is GROUNDED (real str0m dep, feature-on compiles);
              # transport-payjoin-dir is still deferred. If the deferral state
              # changes, this check forces a conscious update.
              grep -q 'str0m = { version' "$src/crates/transport-str0m/Cargo.toml"
              grep -q 'ohttp' "$src/crates/transport-payjoin-dir/Cargo.toml"
              grep -q 'payjoin-dir' "$src/crates/transport-payjoin-dir/Cargo.toml"
              grep -q 'TO[D]O(ground-deps)' "$src/crates/transport-payjoin-dir/Cargo.toml"

              # The rust peer is feature-gated OUT of the default target set
              # (required-features drops the whole [[bin]] until `e2e-peer` is on).
              grep -Eq 'required-features *= *\[[^]]*"e2e-peer"' "$src/crates/ptj-e2e-peer/Cargo.toml"

              # The node harness + spec exist and PARSE (no rot). `node --check`
              # is a syntax pass only: it executes nothing.
              for f in ohttp-harness assertions webrtc-ohttp.spec; do
                test -f "$src/contrib/demo-gui/test/e2e-oblivious/$f.mjs"
                ${pkgs.nodejs}/bin/node --check "$src/contrib/demo-gui/test/e2e-oblivious/$f.mjs"
              done
              ${pkgs.bash}/bin/bash -n "$testScripts/webrtc-e2e-fixtures.sh"

              # The spec asserts obliviousness AND the P2P data path: fail if
              # someone deletes the A2/A3 assertions (which is exactly how a
              # false positive would sneak back).
              grep -q 'assertDirectoryOblivious' "$src/contrib/demo-gui/test/e2e-oblivious/assertions.mjs"
              grep -q 'assertDataPathIsP2P' "$src/contrib/demo-gui/test/e2e-oblivious/assertions.mjs"
              grep -q 'assertDirectoryOblivious' "$src/contrib/demo-gui/test/e2e-oblivious/webrtc-ohttp.spec.mjs"
              mkdir -p "$out"
            '';

        # ---- webrtc-over-OHTTP e2e, stage 2: the live run — DEFERRED STUB.
        # Deliberately FAILS (never fakes green) until its prerequisites exist:
        # transport-payjoin-dir's network deps grounded (payjoin 0.25 has no
        # directory-mailbox client; imp.rs needs a rewrite), ptj-e2e-peer
        # buildable with --features e2e-peer, and payjoin-directory +
        # ohttp-relay binaries in the pinned nixpkgs (mocking the oblivious
        # layer would be exactly the false positive A3 guards against). Lives
        # ONLY in the `frontier` aggregate, so the mainline aggregates
        # (quick/lint/nightly) never see it. The full recipe: the staged
        # harness in contrib/demo-gui/test/e2e-oblivious/ (see its README.md
        # for the manual run) mirrors demo-gui-playwright's store-Chromium
        # plumbing + the sneakernet checks' bitcoind fixtures.
        webrtc-e2e-live = pkgs.runCommand "webrtc-e2e-live-${rev}" { } ''
          echo "webrtc-e2e-live: DEFERRED — see contrib/demo-gui/test/e2e-oblivious/README.md" >&2
          echo "prereqs: grounded transport-payjoin-dir deps; ptj-e2e-peer --features e2e-peer;" >&2
          echo "payjoin-directory + ohttp-relay packages; the built PWA wasm bundle." >&2
          exit 1
        '';

        no-todo-comments = pkgs.runCommand "no-todo-comments-${rev}" { inherit src; } ''
          if grep -rn --exclude-dir=contrib 'TO[D]O\|FIX[M]E' $src/ 2>/dev/null; then
            echo "FAIL: unresolved work-item markers found"
            exit 1
          fi
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
            demo-gui
            demo-gui-webgui-assets
            demo-gui-playwright
            webrtc-e2e-authored
          ];
        };
        lint = pkgs.symlinkJoin {
          name = "lint-checks-${rev}";
          paths = with checks; [
            cargo-sort
            clippy
            demo-gui
            demo-gui-webgui-assets
            doc
            unused-lints
            no-todo-comments
            webrtc-e2e-authored
          ];
        };
        nightly = pkgs.symlinkJoin {
          name = "nightly-checks-${rev}";
          paths = builtins.attrValues (
            removeAttrs checks [
              "mutants"
              # The deferred live e2e stub fails by design until its deps
              # ground; it rides ONLY the frontier aggregate. Delete this line
              # to promote it once it runs.
              "webrtc-e2e-live"
            ]
          );
        };
        # The frontier aggregate: the browser-webrtc<->rust-str0m-over-OHTTP
        # e2e, kept OFF quick/lint/nightly so its deferred live stage can never
        # break the mainline. Stage 1 is green; stage 2 fails until grounded.
        frontier = pkgs.symlinkJoin {
          name = "frontier-checks-${rev}";
          paths = with checks; [
            webrtc-e2e-authored
            webrtc-e2e-live
          ];
        };
      };
    };
}
