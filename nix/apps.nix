{
  perSystem =
    {
      pkgs,
      craneLibNightly,
      rustStable,
      cargo-afl,
      cargo-hfuzz,
      ...
    }:
    let
      rustNightly = craneLibNightly.rustc;

      mkApp = name: description: script: {
        type = "app";
        meta.description = description;
        program = toString (pkgs.writeShellScript name script);
      };

      # Build inputs needed by honggfuzz's C runtime
      binutils-dev = pkgs.binutils-unwrapped.dev;
      binutils-lib = pkgs.binutils-unwrapped.lib;
      libunwind-dev = pkgs.libunwind.dev;
      libunwind-lib = pkgs.libunwind;
      xz-lib = pkgs.xz.out;
    in
    {
      apps = {
        mutants = mkApp "run-mutants" "Run cargo-mutants mutation testing" ''
          export PATH="${pkgs.cargo-mutants}/bin:${rustNightly}/bin:$PATH"
          exec cargo mutants --no-shuffle -vV "$@"
        '';

        fuzz-libfuzzer = mkApp "fuzz-libfuzzer" "Run libFuzzer via cargo-fuzz: nix run .#fuzz-libfuzzer -- <target>" ''
          export PATH="${pkgs.cargo-fuzz}/bin:${rustNightly}/bin:$PATH"
          cd fuzz
          exec cargo fuzz run "$@"
        '';

        fuzz-honggfuzz = mkApp "fuzz-honggfuzz" "Run honggfuzz: nix run .#fuzz-honggfuzz -- <target>" ''
          export PATH="${cargo-hfuzz}/bin:${rustNightly}/bin:$PATH"
          export C_INCLUDE_PATH="${binutils-dev}/include:${libunwind-dev}/include"
          export LIBRARY_PATH="${binutils-lib}/lib:${libunwind-lib}/lib:${xz-lib}/lib"
          export CFLAGS="-O3 -U_FORTIFY_SOURCE"
          export NIX_HARDENING_ENABLE=""
          cd hfuzz
          exec cargo hfuzz run "$@"
        '';

        fuzz-afl = mkApp "fuzz-afl" "Run AFL++ via cargo-afl: nix run .#fuzz-afl -- <target>" ''
          export PATH="${cargo-afl}/bin:${rustStable}/bin:$PATH"
          export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
          export AFL_SKIP_CPUFREQ=1

          # Provide a writable copy of the bundled AFLplusplus source so that
          # cargo-afl can build the instrumentation runtime on first use.
          export CARGO_AFL_AFLPLUSPLUS_SRC="''${XDG_CACHE_HOME:-$HOME/.cache}/lattice-psbt/AFLplusplus"
          if [ ! -f "$CARGO_AFL_AFLPLUSPLUS_SRC/GNUmakefile" ]; then
            mkdir -p "$CARGO_AFL_AFLPLUSPLUS_SRC"
            cp -r --no-preserve=mode ${cargo-afl}/share/AFLplusplus/* "$CARGO_AFL_AFLPLUSPLUS_SRC/"
          fi

          # Build the AFL++ runtime if not already done for this toolchain.
          cargo afl config --build 2>/dev/null || cargo afl config --build --verbose

          target="''${1:?usage: nix run .#fuzz-afl -- <target> [afl args...]}"
          shift
          cd afl
          cargo afl build --bin "$target"
          mkdir -p in
          test -n "$(ls -A in 2>/dev/null)" || echo -n "seed" > in/seed
          exec cargo afl fuzz -i in -o out -- "target/debug/$target" "$@"
        '';
      };
    };
}
