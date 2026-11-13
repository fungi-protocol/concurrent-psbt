{
  perSystem =
    {
      fuzzArgs,
      lib,
      pkgs,
      toolchains,
      ...
    }:
    let
      # Not in nixpkgs. The version must match the afl dependency in
      # fuzz/afl/Cargo.toml.
      version = "0.18.2";

      cargo-afl = pkgs.rustPlatform.buildRustPackage {
        pname = "cargo-afl";
        inherit version;
        src = pkgs.fetchCrate {
          pname = "cargo-afl";
          inherit version;
          hash = "sha256-dXpNiVHW6In675X7v/ev9d6yOlaewMqO4JEx/LW1n+8=";
        };
        cargoHash = "sha256-Qkx4MoJjuL+coIW3lBiukRG6kQQMwy5s8ybVYjxArAw=";
        doCheck = false;
        meta.mainProgram = "cargo-afl";
      };

      rust = toolchains.stable.rustc;

      # The AFL++ runtime that cargo afl build links targets against, and
      # afl-fuzz, built by cargo afl config --build into the per-rustc
      # directory under $XDG_DATA_HOME/afl.rs that cargo-afl reads them from.
      aflplusplus = pkgs.stdenv.mkDerivation {
        pname = "cargo-afl-aflplusplus";
        inherit version;
        # cargo-afl copies the AFL++ sources bundled in cargo-afl-common from
        # that crate's build-time path, which is gone once cargo-afl is built.
        # Seed its build directory with them instead; an empty .git there
        # makes it skip the copy.
        src = pkgs.fetchCrate {
          pname = "cargo-afl-common";
          inherit version;
          hash = "sha256-4aSC7mEcNEkQL6TiOb+rvjlrxC+CExd23qEVqFWDYR8=";
        };
        nativeBuildInputs = [
          cargo-afl
          rust
        ];
        # macOS's default System V shared memory limits are too small for
        # afl-fuzz, and raising them takes root (cargo afl system-config).
        # Build AFL++ against POSIX shared memory there instead.
        env = lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin { TEST_MMAP = "1"; };
        buildPhase = ''
          export HOME=$TMPDIR XDG_DATA_HOME=$TMPDIR/share
          mkdir -p $XDG_DATA_HOME/afl.rs
          cp -r AFLplusplus $XDG_DATA_HOME/afl.rs/AFLplusplus
          chmod -R u+w $XDG_DATA_HOME/afl.rs/AFLplusplus
          touch $XDG_DATA_HOME/afl.rs/AFLplusplus/.git
          cargo afl config --build --verbose
        '';
        installPhase = ''
          mkdir -p $out/share/afl.rs $out/bin
          cp -r $XDG_DATA_HOME/afl.rs/rustc-* $out/share/afl.rs/
          ln -s $out/share/afl.rs/rustc-*/afl.rs-*/afl/bin/afl-fuzz $out/bin/afl-fuzz
        '';
        meta.mainProgram = "afl-fuzz";
      };

      # Every target in fuzz/afl, instrumented by cargo-afl and built in the
      # sandbox, so that the app and the check run the same binaries.
      mkTargets =
        {
          pnameSuffix,
          RUSTFLAGS ? "",
        }:
        toolchains.stable.mkCargoDerivation (
          fuzzArgs "afl"
          // {
            inherit pnameSuffix RUSTFLAGS;
            nativeBuildInputs = [ cargo-afl ];
            XDG_DATA_HOME = "${aflplusplus}/share";
            buildPhaseCargoCommand = "cargo afl build --release --bins";
            installPhase = ''
              mkdir -p $out/bin
              for bin in src/bin/*.rs; do
                cp target/release/$(basename $bin .rs) $out/bin/
              done
            '';
          }
        );

      targets = mkTargets { pnameSuffix = "-fuzz-afl"; };

      aflEnv = {
        AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES = "1";
        AFL_SKIP_CPUFREQ = "1";
      };
    in
    {
      packages = {
        inherit cargo-afl;
        fuzz-afl = targets;
      };

      apps.fuzz-afl = {
        type = "app";
        meta.description = "Run an AFL++ target: nix run .#fuzz-afl -- <target> [afl-fuzz args]";
        program = lib.getExe (
          pkgs.writeShellApplication {
            name = "fuzz-afl";
            runtimeEnv = aflEnv;
            text = ''
              target="''${1:?usage: nix run .#fuzz-afl -- <target> [afl-fuzz args...]}"
              shift
              # afl-fuzz creates the output directory, but not its parents.
              mkdir -p fuzz/afl/in fuzz/afl/out
              [ -n "$(ls -A fuzz/afl/in)" ] || printf seed >fuzz/afl/in/seed
              # One output directory per target, so targets never share a queue.
              exec ${lib.getExe aflplusplus} -i fuzz/afl/in -o "fuzz/afl/out/$target" "$@" -- "${targets}/bin/$target"
            '';
          }
        );
      };

      # A fuzzer with broken coverage feedback runs as cleanly as one with
      # nothing to find, so assert that AFL++ finds the crash_test panic.
      #
      # This is an app rather than a check: whether the fuzzer finds the crash
      # within the time limit depends on its random choices, and a check that
      # can fail without a change to its inputs would blame the wrong commit.
      apps.fuzz-afl-verify =
        let
          verifyTargets = mkTargets {
            pnameSuffix = "-fuzz-afl-verify-targets";
            RUSTFLAGS = "--cfg fuzzer_verify_crash";
          };
        in
        {
          type = "app";
          meta.description = "Confirm AFL++ finds the crash_test panic: nix run .#fuzz-afl-verify -- [afl-fuzz args]";
          program = lib.getExe (
            pkgs.writeShellApplication {
              name = "fuzz-afl-verify";
              runtimeInputs = [ pkgs.coreutils ];
              runtimeEnv = aflEnv // {
                AFL_NO_UI = "1";
                AFL_BENCH_UNTIL_CRASH = "1";
              };
              text = ''
                scratch=$(mktemp -d)
                trap 'rm -rf "$scratch"' EXIT
                mkdir "$scratch/in"
                printf seed >"$scratch/in/seed"
                # afl-fuzz's exit status says nothing useful here; what it left
                # in its output directory does.
                ${lib.getExe aflplusplus} -i "$scratch/in" -o "$scratch/out" -V 600 "$@" -- ${verifyTargets}/bin/afl_crash_test || true
                crashes=("$scratch"/out/default/crashes/id:*)
                if [ ! -e "''${crashes[0]}" ]; then
                  echo "AFL++ did not find the crash" >&2
                  exit 1
                fi
              '';
            }
          );
        };
    };
}
