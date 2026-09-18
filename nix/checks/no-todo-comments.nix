{ inputs, ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      checkTags.no-todo-comments = [
        "lint"
        "nightly"
      ];

      checks.no-todo-comments = pkgs.runCommand "no-todo-comments" { src = inputs.self; } ''
        if grep -Irn 'TO[D]O\|FIX[M]E' "$src" 2>/dev/null; then
          echo "FAIL: unresolved work-item markers found"
          exit 1
        fi
        mkdir -p "$out"
      '';
    };
}
