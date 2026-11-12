# Kani bounded model checker.
#
# Not in nixpkgs, so the upstream release bundle is wrapped together with the
# exact nightly its kani-compiler links against (librustc_driver is unstable ABI).
# Upstream publishes bundles for aarch64-darwin, x86_64-darwin, x86_64-linux and
# aarch64-linux; a system is supported once its bundle hash is listed below.
{ inputs, lib, ... }:
{
  perSystem =
    {
      pkgs,
      system,
      commonArgs,
      ...
    }:
    let
      version = "0.68.0";
      bundles = {
        aarch64-darwin = {
          triple = "aarch64-apple-darwin";
          hash = "sha256-aaKVM3kaM0pkfvFahP1Q4AeiwyXQ+Y0sD9VtGhHuYr0=";
        };
        x86_64-linux = {
          triple = "x86_64-unknown-linux-gnu";
          hash = "sha256-VHeWwkuZg+JVP4ZXU3Svwp4Px8cZRhIIOhmoMWrIkSQ=";
        };
        aarch64-linux = {
          triple = "aarch64-unknown-linux-gnu";
          hash = "sha256-5O9TAEkUIbQfWKgPyjHpXRmEDVRYqfrG1R2TSNOt6rs=";
        };
      };

      # From `rust-toolchain.toml` at the kani-${version} tag.
      toolchain = pkgs.rust-bin.nightly."2026-08-21".default.override {
        extensions = [
          "llvm-tools"
          "rustc-dev"
          "rust-src"
          "rustfmt"
        ];
      };

      # kani-driver expects $KANI_HOME/{bin,lib,library,...,toolchain}; kani-compiler
      # finds librustc_driver via its @loader_path/../toolchain/lib rpath. dyld resolves
      # @loader_path through symlinks, so bin/ must be a real copy next to toolchain/.
      kani =
        let
          b = bundles.${system};
          bundle = pkgs.fetchzip {
            url = "https://github.com/model-checking/kani/releases/download/kani-${version}/kani-${version}-${b.triple}.tar.gz";
            inherit (b) hash;
          };
          # The Linux binaries are built for FHS paths: give them Nix's interpreter and
          # libstdc++/libgcc_s (CBMC). kani-compiler's $ORIGIN/../toolchain/lib rpath
          # already finds librustc_driver; the toolchain is a search path for the check.
          linux = lib.optionalAttrs pkgs.stdenv.isLinux {
            nativeBuildInputs = [ pkgs.autoPatchelfHook ];
            buildInputs = [
              pkgs.stdenv.cc.cc.lib
              toolchain
            ];
          };
          home = pkgs.runCommand "kani-home-${version}" linux (
            ''
              mkdir -p $out
              for f in ${bundle}/*; do ln -s "$f" "$out/$(basename "$f")"; done
              rm $out/bin && cp -r ${bundle}/bin $out/bin
              ln -s ${toolchain} $out/toolchain
            ''
            + lib.optionalString pkgs.stdenv.isLinux ''
              chmod -R u+w $out/bin
              autoPatchelf $out/bin
            ''
          );
        in
        pkgs.runCommand "kani-${version}"
          {
            nativeBuildInputs = [ pkgs.makeWrapper ];
            # For checks that drive `cargo kani` through crane.
            passthru = { inherit toolchain kaniCheck; };
          }
          ''
            mkdir -p $out/bin $out/libexec
            # goto-cc preprocesses kani's C stubs by exec'ing `gcc`.
            ln -s ${pkgs.stdenv.cc}/bin/cc $out/libexec/gcc
            for name in kani cargo-kani; do
              makeWrapper ${home}/bin/kani-driver $out/bin/$name \
                --argv0 $name \
                --set KANI_HOME ${home} \
                --prefix PATH : ${home}/bin:${toolchain}/bin:$out/libexec
            done
          '';
      supported = bundles ? ${system};

      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain toolchain;

      # Harnesses live in-crate under `cfg(kani)` (they need crate-private constructors such
      # as `Conflict::from_values`). `cargo kani` exits non-zero if any harness fails.
      kaniCheck =
        {
          pname,
          src ? commonArgs.src,
          kaniArgs ? "",
        }:
        craneLib.mkCargoDerivation (
          commonArgs
          // {
            inherit pname src;
            version = "0";
            cargoArtifacts = null;
            nativeBuildInputs = [ kani ];
            buildPhaseCargoCommand = ''
              set -o pipefail
              export HOME=$TMPDIR
              mkdir -p $out
              # One harness at a time: with several CBMC processes per check, and several
              # checks per build, a 16 GB GitHub runner was killed mid-verification.
              cargo kani -p concurrent-psbt \
                -Z unstable-options --jobs 1 --output-format terse ${kaniArgs} \
                2>&1 | tee $out/kani.log
            '';
            installPhase = "true";
          }
        );
    in
    {
      packages = lib.mkIf supported { inherit kani; };
      checks = lib.mkIf supported { formal-kani = kaniCheck { pname = "formal-kani"; }; };
      formal.packages = lib.optionals supported [ kani ];
    };
}
