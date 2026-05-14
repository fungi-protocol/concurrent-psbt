{
  perSystem =
    {
      pkgs,
      craneLib,
      cargo-afl,
      ...
    }:
    let
      rustToolchainBin = craneLib.rustc;

      mkApp = name: description: script: {
        type = "app";
        meta.description = description;
        program = toString (pkgs.writeShellScript name script);
      };
    in
    {
      apps = {
        mutants = mkApp "run-mutants" "Run cargo-mutants mutation testing" ''
          export PATH="${pkgs.cargo-mutants}/bin:${rustToolchainBin}/bin:$PATH"
          exec cargo mutants --no-shuffle -vV "$@"
        '';

        fuzz = mkApp "fuzz" "Run cargo-fuzz (libFuzzer): nix run .#fuzz -- <target>" ''
          export PATH="${pkgs.cargo-fuzz}/bin:${rustToolchainBin}/bin:$PATH"
          cd fuzz
          exec cargo fuzz run "$@"
        '';

        hfuzz = mkApp "hfuzz" "Run honggfuzz: nix run .#hfuzz -- <target>" ''
          export PATH="${pkgs.honggfuzz}/bin:${rustToolchainBin}/bin:$PATH"
          cd hfuzz
          export HFUZZ_RUN_ARGS="''${HFUZZ_RUN_ARGS:---timeout 10 -n 1}"
          exec cargo hfuzz run "$@"
        '';

        afl = mkApp "afl" "Run AFL++: nix run .#afl -- <target>" ''
          export PATH="${cargo-afl}/bin:${pkgs.aflplusplus}/bin:${rustToolchainBin}/bin:$PATH"
          target="''${1:?usage: nix run .#afl -- <target> [afl args...]}"
          shift
          cd afl
          cargo afl build --bin "$target"
          mkdir -p in
          test -n "$(ls -A in 2>/dev/null)" || echo -n "seed" > in/seed
          exec cargo afl fuzz -i in -o out "target/debug/$target" "$@"
        '';
      };
    };
}
