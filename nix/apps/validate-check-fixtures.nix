{
  perSystem =
    { config, pkgs, ... }:
    let
      validate-check-fixtures = pkgs.writeShellApplication {
        name = "validate-check-fixtures";
        runtimeInputs = [
          pkgs.git
          config.packages.validate-commits
        ];
        text = builtins.readFile ../validate-check-fixtures.sh;
      };
    in
    {
      packages.validate-check-fixtures = validate-check-fixtures;

      apps.validate-check-fixtures = {
        type = "app";
        program = "${validate-check-fixtures}/bin/validate-check-fixtures";
        meta.description = "Replay and validate expected-failure check fixtures";
      };
    };
}
