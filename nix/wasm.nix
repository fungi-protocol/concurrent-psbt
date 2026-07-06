# wasm32 build-environment workaround, scoped to wasm derivations only.
#
# secp256k1-sys compiles C to wasm32 via clang. nixpkgs' cc-wrapper injects
# host hardening flags (zerocallusedregs, stackprotector,
# stackclashprotection) that clang rejects for the wasm32 target, so the C
# build fails under the default hardening set. The fix is build-env, not
# code: drop the unsupported hardening flags for derivations that compile to
# wasm32. Host derivations keep the full default hardening — do NOT apply
# this globally.
#
# Consumers (wasm package/check derivations) splice these attrs in:
#   * mkDerivation-style: `inherit (wasmBuildEnv) hardeningDisable;`
#   * crane/cargo-style (or ad-hoc devshell wasm builds): set
#     `NIX_HARDENING_ENABLE = wasmBuildEnv.env.NIX_HARDENING_ENABLE;`
#     (the allow-list equivalent of the same three-flag removal).
{
  perSystem = _: {
    _module.args.wasmBuildEnv = {
      hardeningDisable = [
        "zerocallusedregs"
        "stackprotector"
        "stackclashprotection"
      ];
      env.NIX_HARDENING_ENABLE = "fortify pic";
    };
  };
}
