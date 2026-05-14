{
  pkgs,
  rustStable,
  ...
}:
let
  # cargo-afl-common contains the bundled AFLplusplus source that
  # `cargo afl config --build` copies into the XDG data directory at runtime.
  cargo-afl-common-src = pkgs.fetchCrate {
    pname = "cargo-afl-common";
    version = "0.18.2";
    hash = "sha256-4aSC7mEcNEkQL6TiOb+rvjlrxC+CExd23qEVqFWDYR8=";
  };
in
pkgs.rustPlatform.buildRustPackage.override
  {
    rustc = rustStable;
    cargo = rustStable;
  }
  {
    pname = "cargo-afl";
    version = "0.18.2";
    src = pkgs.fetchCrate {
      pname = "cargo-afl";
      version = "0.18.2";
      hash = "sha256-dXpNiVHW6In675X7v/ev9d6yOlaewMqO4JEx/LW1n+8=";
    };
    cargoHash = "sha256-Qkx4MoJjuL+coIW3lBiukRG6kQQMwy5s8ybVYjxArAw=";
    doCheck = false;
    nativeBuildInputs = [ pkgs.llvmPackages.llvm ];
    buildInputs = [ pkgs.llvmPackages.llvm ];

    # Patch the vendored cargo-afl-common to read AFL++ source path from
    # $CARGO_AFL_AFLPLUSPLUS_SRC at runtime instead of the compile-time
    # CARGO_MANIFEST_DIR (which points into the nix build sandbox).
    postConfigure = ''
      local config_rs
      config_rs=$(find /build -path '*/cargo-afl-common-*/src/config.rs' | head -1)
      if [ -n "$config_rs" ]; then
        chmod +w "$(dirname "$config_rs")" "$config_rs"
        substituteInPlace "$config_rs" \
          --replace-fail \
            'let afl_src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(AFL_SRC_PATH);' \
            'let afl_src_dir = std::env::var("CARGO_AFL_AFLPLUSPLUS_SRC").map(std::path::PathBuf::from).unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join(AFL_SRC_PATH));'
      else
        echo "WARNING: could not find cargo-afl-common config.rs to patch"
      fi
    '';

    postInstall = ''
      mkdir -p $out/share/AFLplusplus
      cp -r ${cargo-afl-common-src}/AFLplusplus/* $out/share/AFLplusplus/
    '';
  }
