#!/usr/bin/env bash
# validate-check-fixtures — replay negative check fixtures onto a target commit
set -euo pipefail

usage() {
  cat >&2 <<EOF
Usage: ${0##*/} [options]

Replay every fixture registered as a non-first parent of a merge commit onto
the target revision, then verify its [EXPECT-FAIL: NAME] annotation with
validate-commits.

Options:
  --target REV       revision to test (default: HEAD)
  --registry REV     fixture registry merge (default: origin/check-fixtures)
  -L, --print-build-logs
                     print Nix build logs
  -h, --help         show this help

The registry's first parent is its base. Every remaining parent must be a
single fixture commit whose message contains [EXPECT-FAIL: NAME].
EOF
  exit 1
}

target=HEAD
registry=origin/check-fixtures
validate_args=()

while [ $# -gt 0 ]; do
  case "$1" in
  --target)
    shift
    [ $# -gt 0 ] || {
      echo "error: --target requires an argument" >&2
      exit 1
    }
    target=$1
    ;;
  --registry)
    shift
    [ $# -gt 0 ] || {
      echo "error: --registry requires an argument" >&2
      exit 1
    }
    registry=$1
    ;;
  -L | --print-build-logs) validate_args+=("$1") ;;
  -h | --help) usage ;;
  *)
    echo "error: unrecognized argument: $1" >&2
    exit 1
    ;;
  esac
  shift
done

repo_root=$(git rev-parse --show-toplevel)
target_hash=$(git rev-parse --verify "$target^{commit}") || {
  echo "error: target is not a commit: $target" >&2
  exit 1
}
registry_hash=$(git rev-parse --verify "$registry^{commit}") || {
  echo "error: fixture registry is not available: $registry" >&2
  echo "hint: fetch the check-fixtures branch before running this command" >&2
  exit 1
}

read -r -a registry_line < <(git rev-list --parents -n 1 "$registry_hash")
if [ ${#registry_line[@]} -lt 3 ]; then
  echo "error: fixture registry must be a merge with at least one fixture parent" >&2
  exit 1
fi
fixtures=("${registry_line[@]:2}")

validator=${VALIDATE_COMMITS:-validate-commits}
if ! command -v "$validator" >/dev/null 2>&1; then
  echo "error: validate-commits is not available" >&2
  exit 1
fi

tmpdir=$(mktemp -d)
worktree=$tmpdir/worktree
cleanup() {
  git -C "$repo_root" worktree remove --force "$worktree" >/dev/null 2>&1 || true
  rm -rf "$tmpdir"
}
trap cleanup EXIT INT TERM

echo "Validating ${#fixtures[@]} check fixture(s) against ${target_hash:0:12}..."
failed=()

for fixture in "${fixtures[@]}"; do
  subject=$(git log -1 --format=%s "$fixture")
  read -r -a fixture_line < <(git rev-list --parents -n 1 "$fixture")
  if [ ${#fixture_line[@]} -ne 2 ]; then
    echo "  ✗ ${fixture:0:12} $subject (fixture must be a non-merge commit)"
    failed+=("$fixture")
    continue
  fi
  annotation=$(git log -1 --format=%B "$fixture" |
    sed -n 's/.*\[EXPECT-FAIL: \([^]]*\)\].*/\1/p' | head -1)
  if [ -z "$annotation" ]; then
    echo "  ✗ ${fixture:0:12} $subject (missing [EXPECT-FAIL: NAME])"
    failed+=("$fixture")
    continue
  fi

  git -C "$repo_root" worktree add --quiet --detach "$worktree" "$target_hash"
  if ! git -C "$worktree" \
    -c user.name=check-fixtures \
    -c user.email=check-fixtures@invalid \
    cherry-pick --quiet "$fixture"; then
    echo "  ✗ ${fixture:0:12} $subject (does not apply to target)"
    failed+=("$fixture")
  else
    replayed=$(git -C "$worktree" rev-parse HEAD)
    echo "  → ${fixture:0:12} $subject"
    if ! (
      cd "$worktree"
      "$validator" --git --quick=deferred "${validate_args[@]}" -- "$target_hash..$replayed"
    ); then
      failed+=("$fixture")
    fi
  fi
  git -C "$repo_root" worktree remove --force "$worktree" >/dev/null
done

if [ ${#failed[@]} -gt 0 ]; then
  echo "${#failed[@]} fixture(s) failed validation"
  exit 1
fi

echo "All ${#fixtures[@]} check fixture(s) passed."
