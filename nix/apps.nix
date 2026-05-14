{
  perSystem =
    { pkgs, craneLib, ... }:
    let
      rustToolchainBin = craneLib.rustc;
    in
    {
      apps.mutants = {
        type = "app";
        meta.description = "run cargo mutants on the repository";
        program = toString (
          pkgs.writeShellScript "run-mutants" ''
            export PATH="${pkgs.cargo-mutants}/bin:${rustToolchainBin}/bin:$PATH"
            exec cargo mutants --no-shuffle -vV "$@"
          ''
        );
      };
    };
}
