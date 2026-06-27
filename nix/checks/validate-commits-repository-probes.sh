#!/usr/bin/env bash
set -euo pipefail

validator=$1
real_git=$(command -v git)
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

mkdir -p "$tmpdir/bin"

cat >"$tmpdir/bin/git" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

for arg in "$@"; do
  if [ "$arg" = "${VALIDATE_COMMITS_FAIL_GIT_SUBCOMMAND:-}" ]; then
    echo "injected git $arg failure" >&2
    exit 2
  fi
done

exec "$REAL_GIT" "$@"
EOF
chmod +x "$tmpdir/bin/git"

cat >"$tmpdir/bin/nix" <<'EOF'
#!/usr/bin/env bash
if [ "${1:-}" = eval ]; then
  printf '%s' x86_64-linux
  exit 0
fi
exit 99
EOF
chmod +x "$tmpdir/bin/nix"

repo="$tmpdir/repo"
mkdir -p "$repo"
"$real_git" -C "$repo" init -q
"$real_git" -C "$repo" config user.name Tester
"$real_git" -C "$repo" config user.email tester@example.com
printf '/ignored\n' >"$repo/.gitignore"
printf 'fixture\n' >"$repo/tracked"
"$real_git" -C "$repo" add .gitignore tracked
"$real_git" -C "$repo" commit -q -m clean

expect_probe_failure() {
  local subcommand=$1
  local output

  if output=$(
    cd "$repo"
    PATH="$tmpdir/bin:$PATH" \
      REAL_GIT="$real_git" \
      VALIDATE_COMMITS_FAIL_GIT_SUBCOMMAND="$subcommand" \
      bash "$validator" --git --no-flake-checks -- HEAD 2>&1
  ); then
    echo "validator unexpectedly accepted a git $subcommand failure" >&2
    exit 1
  fi

  grep -F "error: git $subcommand failed" <<<"$output" >/dev/null || {
    echo "validator did not identify the git $subcommand failure" >&2
    printf '%s\n' "$output" >&2
    exit 1
  }
}

expect_probe_failure ls-tree
expect_probe_failure grep
expect_probe_failure check-ignore

if output=$(
  cd "$repo"
  PATH="$tmpdir/bin:$PATH" \
    REAL_GIT="$real_git" \
    bash "$validator" --git --no-flake-checks 2>&1
); then
  echo "validator unexpectedly accepted a missing origin/main merge base" >&2
  exit 1
fi
grep -F 'error: could not find merge-base with origin/main' <<<"$output" >/dev/null
grep -F 'hint: pass an explicit range after --' <<<"$output" >/dev/null

output=$(
  cd "$repo"
  PATH="$tmpdir/bin:$PATH" \
    REAL_GIT="$real_git" \
    bash "$validator" --git --no-flake-checks -- HEAD 2>&1
)
grep -F 'All 1 commits passed.' <<<"$output" >/dev/null
