{
  perSystem =
    { pkgs, ... }:
    let
      validate-commits = pkgs.writeShellApplication {
        name = "validate-commits";
        runtimeInputs = with pkgs; [
          git
          jujutsu
          nix-output-monitor
        ];
        text = builtins.readFile ../validate-commits.sh;
      };
    in
    {
      packages.validate-commits = validate-commits;

      apps.validate-commits = {
        type = "app";
        program = "${validate-commits}/bin/validate-commits";
        meta.description = "Validate repository invariants across commits";
      };
    };
}
