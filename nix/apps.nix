{ inputs, ... }:
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

      binutils-dev = pkgs.binutils-unwrapped.dev;
      binutils-lib = pkgs.binutils-unwrapped.lib;
      libunwind-dev = pkgs.libunwind.dev;
      libunwind-lib = pkgs.libunwind;
      xz-lib = pkgs.xz.out;

      mkApp = name: description: script: {
        type = "app";
        meta.description = description;
        program = toString script;
      };

      # FIXME
      # add verify-commits app, which maps nix flake check success or failure
      # status (failure should be recorded in git notes) in roughly bisect (or
      # breadth first order as far as the git merge span/dlimiter hierarchy).
      # 
      # all scripted diffs should be verified.
      #
      # failing test commits should be verified to fail, but kept out of the
      # main git history.

      # TODO
      # add archive-commits app. analyzes the history, and creates commit
      # references from the worklog history to ensure that any commits
      # referenced by text ID are not garbage collected by git. the worklog can
      # the form of directories sorted in an ISO-8601 date hiearchy, containing
      # text files that describe relations between git trees, commits etc.

      # Reusable setup scripts for each fuzzer. These are the single source
      # of truth for environment configuration. The apps exec into them, and
      # the checks invoke them with crash-test arguments.

      fuzz-libfuzzer-script = pkgs.writeShellScript "fuzz-libfuzzer" ''
        export PATH="${pkgs.cargo-fuzz}/bin:${rustNightly}/bin:$PATH"
        cd fuzz
        exec cargo fuzz run "$@"
      '';

      fuzz-honggfuzz-script = pkgs.writeShellScript "fuzz-honggfuzz" ''
        export PATH="${cargo-hfuzz}/bin:${rustNightly}/bin:$PATH"
        export C_INCLUDE_PATH="${binutils-dev}/include:${libunwind-dev}/include"
        export LIBRARY_PATH="${binutils-lib}/lib:${libunwind-lib}/lib:${xz-lib}/lib"
        export CFLAGS="-O3 -U_FORTIFY_SOURCE"
        export NIX_HARDENING_ENABLE=""
        cd hfuzz
        exec cargo hfuzz run "$@"
      '';

      fuzz-afl-script = pkgs.writeShellScript "fuzz-afl" ''
        export PATH="${cargo-afl}/bin:${rustStable}/bin:$PATH"
        export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
        export AFL_SKIP_CPUFREQ=1

        export CARGO_AFL_AFLPLUSPLUS_SRC="''${XDG_CACHE_HOME:-$HOME/.cache}/lattice-psbt/AFLplusplus"
        if [ ! -f "$CARGO_AFL_AFLPLUSPLUS_SRC/GNUmakefile" ]; then
          mkdir -p "$CARGO_AFL_AFLPLUSPLUS_SRC"
          cp -r --no-preserve=mode ${cargo-afl}/share/AFLplusplus/* "$CARGO_AFL_AFLPLUSPLUS_SRC/"
        fi

        cargo afl config --build 2>/dev/null || cargo afl config --build --verbose

        target="''${1:?usage: nix run .#fuzz-afl -- <target> [afl args...]}"
        shift
        cd afl
        cargo afl build --bin "$target"
        mkdir -p in
        test -n "$(ls -A in 2>/dev/null)" || echo -n "seed" > in/seed
        exec cargo afl fuzz -i in -o out -- "target/debug/$target" "$@"
      '';
    in
    {
      _module.args = {
        inherit fuzz-libfuzzer-script fuzz-honggfuzz-script fuzz-afl-script;
      };

      apps = {
        mutants = mkApp "run-mutants" "Run cargo-mutants mutation testing" (
          pkgs.writeShellScript "run-mutants" ''
            export PATH="${pkgs.cargo-mutants}/bin:${rustNightly}/bin:$PATH"
            exec cargo mutants --no-shuffle -vV "$@"
          ''
        );

        fuzz-libfuzzer =
          mkApp "fuzz-libfuzzer" "Run libFuzzer via cargo-fuzz: nix run .#fuzz-libfuzzer -- <target>"
            fuzz-libfuzzer-script;

        fuzz-honggfuzz =
          mkApp "fuzz-honggfuzz" "Run honggfuzz: nix run .#fuzz-honggfuzz -- <target>"
            fuzz-honggfuzz-script;

        fuzz-afl =
          mkApp "fuzz-afl" "Run AFL++ via cargo-afl: nix run .#fuzz-afl -- <target>"
            fuzz-afl-script;
      };
    };
}
