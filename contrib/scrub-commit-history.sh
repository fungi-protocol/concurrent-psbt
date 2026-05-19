#!/usr/bin/env bash
# scrub-commit-history — verify commit hygiene across history
#
# Walks commit history and checks each commit for unresolved work-item
# markers in messages and optionally runs nix flake check on each.
set -euo pipefail

usage() {
  cat >&2 <<EOF
Usage: ${0##*/} [options] [-- <revset-args>...]

Options:
  -h, --help            show this help
  -L, --print-build-logs  print build logs
  --check NAME          run only named check(s) (repeatable; default: all checks)
  --all-checks          run all checks (error if --check also given)
  --no-flake-checks     skip flake build checks (message-only mode)
  --no-skip-missing     fail commits missing a named check (default: skip them)
  --keep-going          pass --keep-going to nix (build unrelated targets despite failures)
  --log=jj|git          VCS for revset resolution (default: jj if available, else git)
  --git                 alias for --log=git
  --log-revset          use the VCS's own default revset (skip default range)
  --everything          scrub all branches (jj: all() ~ root(); git: --all)

Extra arguments are passed through as-is to jj log or git rev-list.
A single bookmark/branch name or commit hash works in both modes.

Revset defaults (when no extra arguments given):
  jj mode:   -r 'trunk()..@' (ancestors of @ since trunk)
  git mode:  origin/main..HEAD (or all commits if no remote)
EOF
  exit 1
}

run_flake_checks=true
check_names=()
no_skip_missing=false
all_checks=false
log_args=()
nix_build_args=()
vcs_mode=auto
everything=false
use_log_revset=false

while [ $# -gt 0 ]; do
  case "$1" in
  -h | --help) usage ;;
  --log=*) vcs_mode="${1#--log=}" ;;
  --git) vcs_mode=git ;;
  --everything) everything=true ;;
  --log-revset) use_log_revset=true ;;
  --no-skip-missing) no_skip_missing=true ;;
  --all-checks) all_checks=true ;;
  --check)
    shift
    [ $# -gt 0 ] || {
      echo "error: --check requires an argument" >&2
      exit 1
    }
    check_names+=("$1")
    ;;
  --no-flake-checks) run_flake_checks=false ;;
  -k | --keep-going | -L | --print-build-logs) nix_build_args+=("$1") ;;
  *) log_args+=("$1") ;;
  esac
  shift
done

# --all-checks and --check are mutually exclusive
if [ "$all_checks" = true ] && [ ${#check_names[@]} -gt 0 ]; then
  echo "error: --all-checks cannot be combined with --check" >&2
  exit 1
fi

repo_root=$(git rev-parse --show-toplevel)
system=$(nix eval --offline --raw --impure --expr builtins.currentSystem)

# Resolve VCS mode
if [ "$vcs_mode" = auto ]; then
  if [ -d "$repo_root/.jj" ]; then
    vcs_mode=jj
  else
    vcs_mode=git
  fi
fi
case "$vcs_mode" in
jj | git) ;;
*)
  echo "error: --log expects jj or git (got '$vcs_mode')" >&2
  exit 1
  ;;
esac

# --- Nix helpers ---
nix_build() { nix build --no-update-lock-file "${nix_build_args[@]}" "$@" --no-link; }
nix_flake_check() { nix flake check --no-update-lock-file "${nix_build_args[@]}" "$@"; }
check_exists() { nix eval --no-update-lock-file "$1" --apply 'x: true' >/dev/null 2>/dev/null; }
should_check() { [ "$no_skip_missing" = true ] || check_exists "$1"; }

# --everything / --log-revset: validate and expand
if [ "$everything" = true ]; then
  if [ ${#log_args[@]} -gt 0 ] || [ "$use_log_revset" = true ]; then
    echo "error: --everything cannot be combined with explicit revsets or --log-revset" >&2
    exit 1
  fi
  if [ "$vcs_mode" = jj ]; then
    log_args=(-r 'all() ~ root()')
  else
    log_args=(--all)
  fi
elif [ "$use_log_revset" = true ]; then
  if [ ${#log_args[@]} -gt 0 ]; then
    echo "error: --log-revset cannot be combined with explicit revsets" >&2
    exit 1
  fi
fi

# Resolve revsets to git commit IDs
if [ "$vcs_mode" = jj ]; then
  if [ ${#log_args[@]} -gt 0 ]; then
    jj_args=(log --ignore-working-copy --no-graph -T 'commit_id ++ "\n"')
    jj_args+=("${log_args[@]}")
    mapfile -t linear < <(jj "${jj_args[@]}" 2>/dev/null | grep -vE '^0*$')
  elif [ "$use_log_revset" = true ]; then
    # Use jj's configured default log revset (no -r flag)
    mapfile -t linear < <(jj log --ignore-working-copy --no-graph -T 'commit_id ++ "\n"' 2>/dev/null | grep -vE '^0*$')
  else
    # Default: all ancestors of working copy (excluding root)
    mapfile -t linear < <(jj log --ignore-working-copy --no-graph -r 'trunk()..@' -T 'commit_id ++ "\n"' 2>/dev/null | grep -vE '^0*$')
  fi
else
  if [ ${#log_args[@]} -gt 0 ]; then
    mapfile -t linear < <(git rev-list "${log_args[@]}" 2>/dev/null)
  elif [ "$use_log_revset" = true ]; then
    mapfile -t linear < <(git rev-list HEAD 2>/dev/null)
  else
    merge_base=$(git merge-base HEAD origin/main 2>/dev/null || true)
    if [ -n "$merge_base" ]; then
      mapfile -t linear < <(git rev-list "$merge_base..HEAD" 2>/dev/null)
    else
      mapfile -t linear < <(git rev-list HEAD 2>/dev/null)
    fi
  fi
fi

total=${#linear[@]}
if [ "$total" -eq 0 ]; then
  echo "No commits in range."
  exit 0
fi

fmt_commit() {
  if [ "$vcs_mode" = jj ]; then
    jj log --ignore-working-copy --no-graph -r "$1" \
      -T 'change_id.shortest(7) ++ " " ++ commit_id.shortest(7) ++ " " ++ description.first_line()' \
      2>/dev/null || git log -1 --format='%h %s' "$1"
  else
    git log -1 --format='%h %s' "$1"
  fi
}

fmt_rerun_hint() {
  if [ "$vcs_mode" = jj ]; then
    local change_id
    change_id=$(jj log --ignore-working-copy --no-graph -r "$1" \
      -T 'change_id.shortest(7)' 2>/dev/null || true)
    if [ -n "$change_id" ]; then
      echo "    rerun: nix run .#scrub-commit-history -- -r $change_id"
    fi
  else
    echo "    rerun: nix run .#scrub-commit-history -- --git ${1:0:12}"
  fi
}

# --- Flake check helpers ---

# Extract [EXPECT-FAIL: name] annotation from commit message, if any
# True if the EXPECT-FAIL name matches a --check filter (or no filter is set)
expect_fail_matches_checks() {
  local ef_name="$1"
  if [ ${#check_names[@]} -eq 0 ]; then return 0; fi
  for cn in "${check_names[@]}"; do
    if [ "$cn" = "$ef_name" ]; then return 0; fi
  done
  return 1
}

# Check a single commit. Returns 0=pass, 1=fail.
# Appends to build_failed[] on failure.
check_one_commit() {
  local hash=$1
  if [ -n "${no_flake[$hash]:-}" ]; then
    if [ "$no_skip_missing" = true ]; then
      echo "  ✗ $(fmt_commit "$hash") (no flake.nix)"
      build_failed+=("$hash")
      return 1
    else
      echo "  - $(fmt_commit "$hash") (no flake.nix)"
      return 0
    fi
  fi

  local flakeref="git+file://$repo_root?rev=$hash"

  # EXPECT-FAIL commits: verify the named check does fail
  local expect_fail="${expect_fail_check[$hash]:-}"
  if [ -n "$expect_fail" ] && expect_fail_matches_checks "$expect_fail"; then
    local named_target="$flakeref#checks.$system.$expect_fail"
    if ! nix eval --no-update-lock-file "$named_target" --apply 'x: true' >/dev/null 2>/dev/null; then
      echo "  ✗ $(fmt_commit "$hash") (EXPECT-FAIL: $expect_fail — check not defined)"
      build_failed+=("$hash")
      return 1
    fi
    if nix_build "$named_target"; then
      echo "  ✗ $(fmt_commit "$hash") (EXPECT-FAIL: $expect_fail unexpectedly passed)"
      build_failed+=("$hash")
      return 1
    else
      echo "  ✓ $(fmt_commit "$hash") (EXPECT-FAIL: $expect_fail correctly fails)"
    fi
    return 0
  fi

  # --check mode: build only the named checks
  if [ ${#check_names[@]} -gt 0 ]; then
    local failed=false
    for cn in "${check_names[@]}"; do
      local target="$flakeref#checks.$system.$cn"
      if should_check "$target" && ! nix_build "$target"; then
        echo "  ✗ $(fmt_commit "$hash") ($cn failed)"
        failed=true
      fi
    done
    if [ "$failed" = true ]; then
      build_failed+=("$hash")
      return 1
    else
      echo "  ✓ $(fmt_commit "$hash")"
    fi
  else
    # All-checks mode: run full nix flake check
    if nix_flake_check "$flakeref"; then
      echo "  ✓ $(fmt_commit "$hash")"
    else
      echo "  ✗ $(fmt_commit "$hash")"
      build_failed+=("$hash")
      return 1
    fi
  fi
  return 0
}

# --- Checks ---

# Pre-compute per-commit skip maps (avoids redundant probes in each phase)
declare -A no_flake
for hash in "${linear[@]}"; do
  if ! git cat-file -e "$hash:flake.nix" 2>/dev/null; then
    no_flake[$hash]=1
  fi
done

# Pre-compute EXPECT-FAIL check names per commit
declare -A expect_fail_check
for hash in "${linear[@]}"; do
  ef=$(git log -1 --format='%B' "$hash" | sed -n 's/.*\[EXPECT-FAIL: \([^]]*\)\].*/\1/p' | head -1)
  if [ -n "$ef" ]; then
    expect_fail_check[$hash]=$ef
  fi
done

# Check each commit message for unresolved work-item markers
# Conflict check — jj conflict trees and git conflict markers
conflict_failed=()
echo "Checking for conflicts..."
for hash in "${linear[@]}"; do
  reason=""
  if git ls-tree --name-only "$hash" 2>/dev/null | grep -qE '^\.jj(conflict-|-do-not-resolve)'; then
    reason="jj conflict tree"
  elif git grep -q -E '<{7}|>{7}|={7}' "$hash" -- 2>/dev/null; then
    reason="conflict markers"
  fi
  if [ -n "$reason" ]; then
    echo "  ✗ $(fmt_commit "$hash") ($reason)"
    conflict_failed+=("$hash")
  fi
done
if [ "${#conflict_failed[@]}" -gt 0 ]; then
  echo "${#conflict_failed[@]} commit(s) have unresolved conflicts"
else
  echo "  all clean"
fi

msg_failed=()
echo "Checking commit messages..."
for hash in "${linear[@]}"; do
  if git log -1 --format='%B' "$hash" | grep -qE '^\s*[#\[]*\s*(TODO|FIXME|WIP)\b|\bfixup! |\bsquash! '; then
    echo "  ✗ $(fmt_commit "$hash")"
    msg_failed+=("$hash")
  fi
done
if [ "${#msg_failed[@]}" -gt 0 ]; then
  echo "${#msg_failed[@]} commit(s) have unresolved work items"
else
  echo "  all clean"
fi

# Gitignore monotonicity: check no historical commit has files that
# the tip's .gitignore would exclude (using a scratch repo for check-ignore)
gitignore_failed=()
tip_hash=${linear[0]}
if git cat-file -e "$tip_hash:.gitignore" 2>/dev/null; then
  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT
  git show "$tip_hash:.gitignore" >"$tmpdir/.gitignore"
  git init -q "$tmpdir/repo"
  cp "$tmpdir/.gitignore" "$tmpdir/repo/.gitignore"

  echo "Checking gitignore monotonicity..."
  for hash in "${linear[@]}"; do
    leaked=$(git ls-tree -r --name-only "$hash" 2>/dev/null |
      git -C "$tmpdir/repo" check-ignore --stdin 2>/dev/null || true)
    if [ -n "$leaked" ]; then
      first=$(echo "$leaked" | head -1)
      count=$(echo "$leaked" | wc -l)
      echo "  ✗ $(fmt_commit "$hash") — $count file(s) leaked, e.g. $first"
      gitignore_failed+=("$hash")
    fi
  done
  if [ "${#gitignore_failed[@]}" -eq 0 ]; then
    echo "  all clean"
  else
    echo "${#gitignore_failed[@]} commit(s) have gitignore leaks"
  fi
fi

# Flake check phase (skipped with --no-flake-checks)
build_failed=()
if [ "$run_flake_checks" = true ]; then
  echo "Flake check phase: verifying $total commits..."
  for hash in "${linear[@]}"; do
    check_one_commit "$hash" || true
  done
fi

# Summary
n_issues=$((${#conflict_failed[@]} + ${#msg_failed[@]} + ${#gitignore_failed[@]} + ${#build_failed[@]}))
if [ "$n_issues" -eq 0 ]; then
  echo "All $total commits passed."
else
  echo
  echo "$n_issues issue(s):"
  for h in "${conflict_failed[@]}"; do
    echo "  conflict: $(fmt_commit "$h")"
  done
  for h in "${build_failed[@]}"; do
    echo "  build: $(fmt_commit "$h")"
    fmt_rerun_hint "$h"
  done
  for h in "${msg_failed[@]}"; do
    echo "  message: $(fmt_commit "$h")"
  done
  for h in "${gitignore_failed[@]}"; do
    echo "  gitignore: $(fmt_commit "$h")"
  done
  exit 1
fi
