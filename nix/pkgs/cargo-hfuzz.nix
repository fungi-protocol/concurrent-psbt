{
  pkgs,
  ...
}:
pkgs.rustPlatform.buildRustPackage {
  pname = "cargo-hfuzz";
  version = "0.5.60";
  src = pkgs.fetchCrate {
    pname = "honggfuzz";
    version = "0.5.60";
    hash = "sha256-btHYe+rN28bVeDWZB3AQCeF5mk30YNIINMXOOoTIjJk=";
  };
  cargoHash = "sha256-9jlu9PDqQRW3r+ZJrGxDXB533gTa8XexZuK5LXcNY3s=";
}
