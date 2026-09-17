{
  perSystem =
    {
      commonArgs,
      pkgs,
      rev,
      ...
    }:
    {
      checkTags.no-todo-comments = [
        "lint"
        "nightly"
      ];

      checks.no-todo-comments = pkgs.runCommand "no-todo-comments-${rev}" { inherit (commonArgs) src; } ''
        if grep -rn --exclude-dir=contrib 'TO[D]O\|FIX[M]E' $src/ 2>/dev/null; then
          echo "FAIL: unresolved work-item markers found"
          exit 1
        fi
        mkdir -p $out
      '';
    };
}
