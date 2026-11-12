# Verus deductive verifier (formal/verus).
#
# Not in nixpkgs, so the upstream release bundle (which ships its own z3) is
# wrapped together with the exact rust release its rust_verify links against.
# Upstream publishes bundles for arm64/x86 macOS and Linux; a system is supported
# once its bundle hash is listed below.
{ lib, ... }:
{
  perSystem =
    { pkgs, system, ... }:
    let
      version = "0.2026.11.02.8bf68ee";
      bundles = {
        aarch64-darwin = {
          platform = "arm64-macos";
          hash = "sha256-QIlKURRpQNPWIiIU/3YGmhMd6NVIpEj1R7RdCvKTDz8=";
        };
        x86_64-linux = {
          platform = "x86-linux";
          hash = "sha256-m7u6bXGFeis39gpBlD5o3z4Btl1/CpbYuog03tzmKQA=";
        };
        aarch64-linux = {
          platform = "arm64-linux";
          hash = "sha256-oPeng3aQhXtbC3hf+MG/uTlR5yL4udF1ZYzC/rse8F4=";
        };
      };

      # From `rust-toolchain.toml` at the release/${version} tag.
      toolchain = pkgs.rust-bin.stable."1.99.1".default.override {
        extensions = [
          "rustc-dev"
          "llvm-tools"
          "rust-src"
        ];
      };

      # VERUS_USE_RUSTUP=0 makes the launcher exec rust_verify directly instead of
      # `rustup run <toolchain>`; the toolchain is supplied on PATH and loader path.
      verus =
        let
          b = bundles.${system};
          fetched = pkgs.fetchzip {
            url = "https://github.com/verus-lang/verus/releases/download/release/${version}/verus-${version}-${b.platform}.zip";
            inherit (b) hash;
          };
          # The Linux binaries are built for FHS paths: give them Nix's interpreter,
          # libstdc++/libgcc_s (z3) and the toolchain's librustc_driver (rust_verify).
          bundle =
            if pkgs.stdenv.isLinux then
              pkgs.runCommand "verus-bundle-${version}"
                {
                  nativeBuildInputs = [ pkgs.autoPatchelfHook ];
                  buildInputs = [
                    pkgs.stdenv.cc.cc.lib
                    toolchain
                  ];
                }
                ''
                  cp -r ${fetched} $out
                  chmod -R u+w $out
                  autoPatchelf $out
                ''
            else
              fetched;
          loaderVar = if pkgs.stdenv.isDarwin then "DYLD_LIBRARY_PATH" else "LD_LIBRARY_PATH";
        in
        pkgs.runCommand "verus-${version}" { nativeBuildInputs = [ pkgs.makeWrapper ]; } ''
          mkdir -p $out/bin
          for name in verus cargo-verus; do
            makeWrapper ${bundle}/$name $out/bin/$name \
              --set VERUS_USE_RUSTUP 0 \
              --set VERUS_Z3_PATH ${bundle}/z3 \
              --prefix PATH : ${toolchain}/bin \
              --prefix ${loaderVar} : ${toolchain}/lib
          done
        '';
      supported = bundles ? ${system};
    in
    {
      packages = lib.mkIf supported { inherit verus; };
      formal.packages = lib.optionals supported [ verus ];
    };
}
