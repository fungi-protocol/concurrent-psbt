#!/usr/bin/env bash
# Build concurrent-psbt-wasm into a browser-loadable module for the PWA.
#
# Encodes the Feasibility findings. Run from the crate dir (crates/
# concurrent-psbt-wasm in the real repo). NOT part of the default nix flake
# check — this is the dedicated PWA build.
#
# Prereqs (all provided by the flake devshell):
#   - the rust toolchain WITH the wasm32 target (nix/toolchain.nix ships
#     targets = [ "wasm32-unknown-unknown" ] on every rust-overlay override);
#   - wasm-pack (or wasm-bindgen-cli) + binaryen (wasm-opt), in
#     nix/devshell.nix. NOTE: wasm-bindgen in Cargo.toml is pinned =0.2.121 to
#     match the devshell wasm-bindgen-cli; bump the two together.
#
# The two hard build-env facts from the probe:
#   1. getrandom wasm backend: selected via the wasm_js Cargo feature (already
#      in Cargo.toml under [target.'cfg(target_arch="wasm32")'.dependencies]).
#      The belt-and-suspenders RUSTFLAGS form is set below too; either alone
#      worked in the probe.
#   2. nix cc-wrapper hardening: secp256k1-sys compiles C (secp256k1.c, wasm.c,
#      precomputed_ecmult*) via cc-rs using the nix-wrapped clang, which injects
#      the host-only flag -fzero-call-used-regs=used-gpr from
#      NIX_HARDENING_ENABLE. clang REJECTS that flag for target
#      wasm32-unknown-unknown -> "error occurred in cc-rs". Strip the host-only
#      hardening for the wasm C build (this is the ONLY hard blocker, and it is
#      entirely in the nix build env, not in any Rust code).
set -euo pipefail

OUT_DIR="${1:-pkg}"
PROFILE="${2:-release}" # release | dev

# --- Feasibility fix #2: strip host-only hardening so secp256k1-sys' C builds ---
# Keep fortify+pic; drop the flags clang rejects for wasm32.
export NIX_HARDENING_ENABLE="fortify pic"
# Equivalent nix-expression form (if wrapping this in a derivation instead):
# splice in the `wasmBuildEnv` module arg from nix/wasm.nix.

# --- Feasibility fix #1: getrandom js backend (redundant with Cargo feature) ---
export RUSTFLAGS="${RUSTFLAGS:-} --cfg getrandom_backend=\"wasm_js\""

TARGET="web" # web | bundler | nodejs — PWA uses `web` (ES module + init()).

if command -v wasm-pack >/dev/null 2>&1; then
  echo "building with wasm-pack (target=$TARGET, profile=$PROFILE) -> $OUT_DIR"
  if [ "$PROFILE" = "dev" ]; then
    wasm-pack build --dev --target "$TARGET" --out-dir "$OUT_DIR" -- --features debug-panic-hook
  else
    wasm-pack build --release --target "$TARGET" --out-dir "$OUT_DIR"
  fi
else
  echo "wasm-pack not found; falling back to cargo + wasm-bindgen-cli"
  cargo build --lib --target wasm32-unknown-unknown --"$PROFILE"
  WASM_IN="target/wasm32-unknown-unknown/$PROFILE/concurrent_psbt_wasm.wasm"
  wasm-bindgen "$WASM_IN" --out-dir "$OUT_DIR" --target "$TARGET"
  # Optional size pass; harmless if binaryen absent.
  if command -v wasm-opt >/dev/null 2>&1; then
    wasm-opt -Oz "$OUT_DIR/concurrent_psbt_wasm_bg.wasm" \
      -o "$OUT_DIR/concurrent_psbt_wasm_bg.wasm"
  fi
fi

echo "done -> $OUT_DIR/ (import default init + named ops from concurrent_psbt_wasm.js)"
