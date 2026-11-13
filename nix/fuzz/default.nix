{ lib, ... }:
{
  # Import every module in this directory, so that each fuzzer's module can be
  # added or removed without touching a list the others also edit.
  imports = lib.pipe (builtins.readDir ./.) [
    (lib.filterAttrs (
      name: type: type == "regular" && name != "default.nix" && lib.hasSuffix ".nix" name
    ))
    lib.attrNames
    (map (name: ./${name}))
  ];
}
