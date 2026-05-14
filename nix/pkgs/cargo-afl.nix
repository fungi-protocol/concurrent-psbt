{
  pkgs,
  ...
}:
pkgs.rustPlatform.buildRustPackage {
  pname = "cargo-afl";
  version = "0.18.2";
  src = pkgs.fetchCrate {
    pname = "cargo-afl";
    version = "0.18.2";
    hash = "sha256-Fwa8pPLNBVYIgqjOHLEV/CDdSQm7YpdMlrWqBQl1N/c=";
  };
  cargoHash = "sha256-rr8Lb/6iBMC/tEh5NsAqYaFVpHErJcaS18oAxvdSyHk=";
  nativeBuildInputs = [ pkgs.llvmPackages.llvm ];
  buildInputs = [ pkgs.llvmPackages.llvm ];
}
