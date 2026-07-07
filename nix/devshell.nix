{
  perSystem =
    {
      config,
      pkgs,
      toolchains,
      ...
    }:
    let
      mkDevShell =
        craneLib:
        craneLib.devShell {
          packages = with pkgs; [
            # binaryen (wasm-opt), wasm-bindgen-cli and wasm-pack are the wasm
            # packaging toolchain for the browser-facing crates; the wasm32
            # target itself comes from the rust toolchain (nix/toolchain.nix).
            binaryen
            # capnp (the Cap'n Proto schema compiler) is shelled out to by the
            # capnpc build script of transport-plugin-api; codegen needs the
            # tool, the generated code only needs the capnp crate.
            capnproto
            cargo-llvm-cov
            cargo-nextest
            cargo-sort
            config.packages.validate-commits
            config.treefmt.build.wrapper
            just
            nodejs
            rust-analyzer
            typescript
            wasm-bindgen-cli
            wasm-pack
          ];
        };
    in
    {
      devShells = builtins.mapAttrs (_: mkDevShell) toolchains // {
        default = mkDevShell toolchains.nightly;
      };
    };
}
