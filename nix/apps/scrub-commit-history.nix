{
  perSystem =
    { pkgs, ... }:
    let
      scrub-commit-history = pkgs.writeShellApplication {
        name = "scrub-commit-history";
        runtimeInputs = with pkgs; [
          git
          jujutsu
        ];
        text = builtins.readFile ../../contrib/scrub-commit-history.sh;
      };
    in
    {
      apps.scrub-commit-history = {
        type = "app";
        program = "${scrub-commit-history}/bin/scrub-commit-history";
        meta.description = "Check commit history for hygiene and flake check regressions";
      };
    };
}
