{ ... }:
{
  perSystem =
    { config, pkgs, ... }:
    let
      ptj-demo-sneakernet = pkgs.writeShellApplication {
        name = "ptj-demo-sneakernet";
        runtimeInputs = with pkgs; [
          bitcoind
          coreutils
          jq
          config.packages.ptj
        ];
        text = builtins.readFile ../../contrib/demo/ptj-demo-sneakernet.sh;
      };
    in
    {
      packages.ptj-demo-sneakernet = ptj-demo-sneakernet;

      apps.ptj-demo-sneakernet = {
        type = "app";
        program = "${ptj-demo-sneakernet}/bin/ptj-demo-sneakernet";
        meta.description = "Create persistent regtest artifacts for the ptj sneakernet demo";
      };
    };
}
