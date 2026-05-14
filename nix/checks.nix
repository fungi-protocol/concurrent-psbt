{ inputs, ... }:
{
  perSystem =
    {
      pkgs,
      craneLibNightly,
      rustStable,
      commonArgs,
      cargoArtifacts,
      cargo-afl,
      cargo-hfuzz,
      ...
    }:
    let
      rustNightly = craneLibNightly.rustc;

      binutils-dev = pkgs.binutils-unwrapped.dev;
      binutils-lib = pkgs.binutils-unwrapped.lib;
      libunwind-dev = pkgs.libunwind.dev;
      libunwind-lib = pkgs.libunwind;
      xz-lib = pkgs.xz.out;

      # Full source including fuzz subdirectories (cleanCargoSource strips them).
      fullSrc = pkgs.lib.cleanSource inputs.self;

      # 60 second timeout: an instrumented fuzzer should find the 8-byte
      # state machine crash well within this. Without instrumentation the
      # search space is 256^8 which is infeasible.
      fuzzerTimeout = "60";
    in
    {
      checks = {
        tests = craneLibNightly.cargoNextest (
          commonArgs
          // {
            inherit cargoArtifacts;
          }
        );

        mutants = craneLibNightly.mkCargoDerivation (
          commonArgs
          // {
            inherit cargoArtifacts;
            pnameSuffix = "-mutants-smoke";
            nativeBuildInputs = [ pkgs.cargo-mutants ];
            buildPhaseCargoCommand = ''
              timeout 10 cargo mutants --no-shuffle -vV || test $? -eq 124
            '';
            installPhase = ''
              mkdir -p $out
              cp -r mutants.out/* $out/ 2>/dev/null || true
            '';
          }
        );

        coverage = craneLibNightly.mkCargoDerivation (
          commonArgs
          // {
            inherit cargoArtifacts;
            pnameSuffix = "-coverage";
            nativeBuildInputs = [ pkgs.cargo-llvm-cov ];
            buildPhaseCargoCommand = ''
              mkdir -p $out
              cargo llvm-cov --all-features --lcov --output-path $out/coverage.lcov
            '';
            installPhase = "true";
          }
        );

        fuzz-libfuzzer-verify = pkgs.stdenv.mkDerivation {
          name = "fuzz-libfuzzer-verify";
          src = fullSrc;
          nativeBuildInputs = [
            rustNightly
            pkgs.cargo-fuzz
          ];
          buildPhase = ''
            export HOME=$(mktemp -d)
            cd fuzz
            # cargo-fuzz exits non-zero when a crash is found.
            # We expect the crash, so invert the exit code.
            if RUSTFLAGS="--cfg fuzzer_verify_crash" \
               timeout ${fuzzerTimeout} \
               cargo fuzz run fuzz_crash_test -- -max_total_time=50 2>&1; then
              echo "FAIL: fuzzer exited cleanly without finding a crash"
              exit 1
            fi
            echo "OK: libFuzzer found the crash"
          '';
          installPhase = ''
            mkdir -p $out
            cp -r fuzz/artifacts/fuzz_crash_test/* $out/ 2>/dev/null || true
          '';
        };

        fuzz-honggfuzz-verify = pkgs.stdenv.mkDerivation {
          name = "fuzz-honggfuzz-verify";
          src = fullSrc;
          nativeBuildInputs = [
            rustNightly
            cargo-hfuzz
          ];
          buildPhase = ''
            export HOME=$(mktemp -d)
            export C_INCLUDE_PATH="${binutils-dev}/include:${libunwind-dev}/include"
            export LIBRARY_PATH="${binutils-lib}/lib:${libunwind-lib}/lib:${xz-lib}/lib"
            export CFLAGS="-O3 -U_FORTIFY_SOURCE"
            export NIX_HARDENING_ENABLE=""
            export HFUZZ_RUN_ARGS="--timeout 5 -n 1 --run_time ${fuzzerTimeout} --exit_upon_crash"

            cd hfuzz
            RUSTFLAGS="--cfg fuzzer_verify_crash" \
              cargo hfuzz run hfuzz_crash_test || true

            if ls hfuzz_workspace/hfuzz_crash_test/SIGABRT* hfuzz_workspace/hfuzz_crash_test/*.fuzz 2>/dev/null; then
              echo "OK: honggfuzz found the crash"
            else
              echo "FAIL: honggfuzz did not find crash within timeout"
              exit 1
            fi
          '';
          installPhase = ''
            mkdir -p $out
            cp -r hfuzz/hfuzz_workspace/hfuzz_crash_test/* $out/ 2>/dev/null || true
          '';
        };

        fuzz-afl-verify = pkgs.stdenv.mkDerivation {
          name = "fuzz-afl-verify";
          src = fullSrc;
          nativeBuildInputs = [
            rustStable
            cargo-afl
          ];
          buildPhase = ''
            export HOME=$(mktemp -d)
            export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
            export AFL_SKIP_CPUFREQ=1

            export CARGO_AFL_AFLPLUSPLUS_SRC="$HOME/.cache/aflplusplus"
            mkdir -p "$CARGO_AFL_AFLPLUSPLUS_SRC"
            cp -r --no-preserve=mode ${cargo-afl}/share/AFLplusplus/* "$CARGO_AFL_AFLPLUSPLUS_SRC/"

            cargo afl config --build

            cd afl
            RUSTFLAGS="--cfg fuzzer_verify_crash" cargo afl build --bin afl_crash_test

            mkdir -p in
            echo -n "seed" > in/seed

            timeout ${fuzzerTimeout} \
              cargo afl fuzz -i in -o out -V ${fuzzerTimeout} -- target/debug/afl_crash_test \
              || true

            if ls out/default/crashes/id:* 2>/dev/null; then
              echo "OK: AFL++ found the crash"
            else
              echo "FAIL: AFL++ did not find crash within timeout"
              exit 1
            fi
          '';
          installPhase = ''
            mkdir -p $out
            cp -r afl/out/default/crashes/* $out/ 2>/dev/null || true
          '';
        };
      };
    };
}
