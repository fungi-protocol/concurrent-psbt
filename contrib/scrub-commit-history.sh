#!/usr/bin/env bash
# scrub-commit-history — verify commit hygiene across history
#
# Walks commit history and checks each commit for unresolved work-item
# markers in messages and optionally runs nix flake check on each.
set -euo pipefail

# Kill entire process group on interrupt for responsive ^C
for sig in INT TERM; do
  # shellcheck disable=SC2064 # intentional: expand $sig now to bake in the signal name
  trap "trap - INT TERM; kill -$sig 0" "$sig"
done

usage() {
  cat >&2 <<EOF
Usage: ${0##*/} [options] [-- <revset-args>...]

For each commit in the revset, runs: message hygiene check, gitignore
monotonicity check, and a flake check phase.

The flake check phase builds checks.\$system.NAME for each commit.
Without --check, all checks run (all-checks mode).
With --check NAME, only that check runs (repeatable for multiple).

Traversal order (default: --reverse):
  --forward      oldest→newest  (CI: validate full history)
  --reverse      newest→oldest  (development: surface recent failures first)
  --bisect       midpoint-first (history rewriting: locate breakage in O(log n))

Flake check phase — check selection:
  --check NAME          run only checks.\$system.NAME (repeatable)
  --all-checks          explicitly run all checks (error if --check also given)
  --no-flake-checks     skip the flake check phase entirely
  --no-nom              disable nix-output-monitor (default: use nom on tty)
  --no-skip-missing     fail commits that lack a named --check (default: skip)

Quick pre-check (all-checks mode only):
  The flake may define checks.\$system.quick — a lightweight fast-feedback subset.

  --quick            shorthand for --quick=only
  --quick=only      run only checks.\$system.quick; implies --fail-fast
  --quick=first     (default) run quick as a separate pre-phase, then all checks
  --quick=deferred  no separate pre-phase; quick runs as part of all checks

Failure handling (default: report all per-commit failures, then continue):
  --fail-fast     stop at the first failing commit
  --keep-going    pass --keep-going to nix (build unrelated targets despite failures)

Batching:
  --parallel      collect all check targets across commits, build in one nix
                  invocation (nix schedules across cores); falls back to
                  sequential on failure to isolate broken commits
                  (best for CI or mostly-passing history)

Output:
  -h, --help              show this help
  -L, --print-build-logs  print nix build logs

VCS mode (default: jj if available, else git):
  --log=jj|git    select VCS for revset resolution
  --git           alias for --log=git

Extra arguments are passed through as-is to jj log or git rev-list.
A single bookmark/branch name or commit hash works in both modes.

Revset defaults (when no extra arguments given):
  jj mode:   -r 'trunk()..@' (ancestors of @ since trunk)
  git mode:  origin/main..HEAD (or all commits if no remote)

Revset options:
  --log-revset    use the VCS's own default revset (skip default range)
  --everything    scrub all branches (jj: all() ~ root(); git: --all)
EOF
  exit 1
}

order=reverse
shallow_param="${NIX_FLAKE_SHALLOW:+&shallow=1}"
run_flake_checks=true
check_names=()
no_skip_missing=false
all_checks=false
log_args=()
nix_build_args=()
vcs_mode=auto
everything=false
use_log_revset=false
mode=sequential
fail_fast=false
quick=auto # auto resolves after arg parsing

while [ $# -gt 0 ]; do
  case "$1" in
  -h | --help) usage ;;
  --forward) order=forward ;;
  --reverse) order=reverse ;;
  --bisect) order=bisect ;;
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
  --no-nom) use_nom=false ;;
  --parallel) mode=parallel ;;
  --fail-fast) fail_fast=true ;;
  --quick) quick=only ;;
  --quick=*) quick="${1#--quick=}" ;;
  -k | --keep-going | -L | --print-build-logs) nix_build_args+=("$1") ;;
  --)
    shift
    break
    ;;
  *)
    echo "error: unrecognized argument: $1" >&2
    echo "(hint: use -- to indicate the beginning of git/jj log arguments)" >&2
    exit 1
    ;;
  esac
  shift
done
log_args+=("$@")

# Resolve --quick before validation (--quick=only implies --fail-fast)
case "$quick" in
only)
  if [ ${#check_names[@]} -gt 0 ]; then
    echo "error: --quick=only cannot be combined with --check" >&2
    exit 1
  fi
  fail_fast=true
  ;;
first | deferred | auto) ;;
*)
  echo "error: --quick expects only|first|deferred (got '$quick')" >&2
  exit 1
  ;;
esac

# In all-checks mode (no --check), default quick to 'first'; else no quick
if [ "$quick" = auto ]; then
  if [ ${#check_names[@]} -eq 0 ]; then
    quick=first
  else
    quick=none
  fi
fi

# --all-checks and --check are mutually exclusive
if [ "$all_checks" = true ] && [ ${#check_names[@]} -gt 0 ]; then
  echo "error: --all-checks cannot be combined with --check" >&2
  exit 1
fi

# --quick=only and --all-checks conflict: --quick=only skips the full phase
if [ "$quick" = only ] && [ "$all_checks" = true ]; then
  echo "error: --quick=only cannot be combined with --all-checks" >&2
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

# Use nom (nix-output-monitor) when available and on a tty, unless --no-nom
if [ "${use_nom-unset}" = unset ]; then
  use_nom=false
  if [ -t 1 ] && command -v nom >/dev/null 2>&1; then
    use_nom=true
  fi
fi
if [ "$use_nom" = true ]; then
  nix_cmd=(nom)
else
  nix_cmd=(nix)
fi

# --- Nix helpers ---
nix_build() { "${nix_cmd[@]}" build --no-update-lock-file "${nix_build_args[@]}" "$@" --no-link; }
nix_flake_check() {
  if [ "$use_nom" = true ]; then
    nix flake check --no-update-lock-file "${nix_build_args[@]}" "$@" --log-format internal-json |& nom --json
  else
    nix flake check --no-update-lock-file "${nix_build_args[@]}" "$@"
  fi
}
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

# Build index ordering for traversal
# ordered[] contains indices into linear[] (not hashes)
case "$order" in
forward)
  ordered=()
  for ((i = total - 1; i >= 0; i--)); do ordered+=("$i"); done
  ;;
reverse)
  ordered=()
  for ((i = 0; i < total; i++)); do ordered+=("$i"); done
  ;;
bisect)
  # BFS on midpoints — finds failures in O(log n) for sparse breakage
  ordered=()
  queue=("0 $((total - 1))")
  while [ ${#queue[@]} -gt 0 ]; do
    pair=${queue[0]}
    queue=("${queue[@]:1}")
    lo=${pair%% *}
    hi=${pair##* }
    if [ "$lo" -gt "$hi" ]; then continue; fi
    mid=$(((lo + hi) / 2))
    ordered+=("$mid")
    if [ "$lo" -lt "$mid" ]; then queue+=("$lo $((mid - 1))"); fi
    if [ "$mid" -lt "$hi" ]; then queue+=("$((mid + 1)) $hi"); fi
  done
  ;;
esac

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

  local flakeref="git+file://$repo_root?rev=$hash$shallow_param"

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

# Hardcoded skip maps — all commits in ::@ have flake.nix, none to skip
declare -A no_flake

# Hardcoded — these early commits lack checks.$system.quick
declare -A no_quick
no_quick=(
  [71e8510f31237c4772aeff0669927b54afb27ba4]=1 # xmqsmtn  nix flake scaffold
  [57c9ee5267fa26d29bd1a63b98fe98bc6725b660]=1 # tumuoyk  treefmt: enable nixfmt
  [576ad8b948c31f70855a29381818a8248f1dc51f]=1 # oypsnvw  ci: basic nix flake check workflow
  [96a9e838942b5946225f257b4f65b9c24b358b00]=1 # zzmypnm  Merge #2 nixfmt
)

# Pre-compute EXPECT-FAIL check names per commit
declare -A expect_fail_check
for hash in "${linear[@]}"; do
  ef=$(git log -1 --format='%B' "$hash" | sed -n 's/.*\[EXPECT-FAIL: \([^]]*\)\].*/\1/p' | head -1)
  if [ -n "$ef" ]; then
    expect_fail_check[$hash]=$ef
  fi
done

# Build phase-specific ordered arrays (pre-filtered)
flake_ordered=()
quick_ordered=()
for idx in "${ordered[@]}"; do
  hash=${linear[$idx]}
  [ -z "${no_flake[$hash]:-}" ] || continue
  flake_ordered+=("$idx")
  [ -z "${no_quick[$hash]:-}" ] || continue
  quick_ordered+=("$idx")
done

# Check each commit message for unresolved work-item markers
# Conflict check — jj conflict trees and git conflict markers
conflict_failed=()
echo "Checking for conflicts..."
for idx in "${ordered[@]}"; do
  hash=${linear[$idx]}
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
for idx in "${ordered[@]}"; do
  hash=${linear[$idx]}
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
  for idx in "${ordered[@]}"; do
    hash=${linear[$idx]}
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

# Quick pre-check (--quick=first, all-checks mode only)
build_failed=()
if [ "$run_flake_checks" = true ] && [ "$quick" = first ]; then
  if [ "$mode" = parallel ]; then
    quick_targets=()
    for idx in "${quick_ordered[@]}"; do
      hash=${linear[$idx]}
      [ -z "${expect_fail_check[$hash]:-}" ] || continue
      quick_targets+=("git+file://$repo_root?rev=$hash$shallow_param#checks.$system.quick")
    done
    if [ ${#quick_targets[@]} -gt 0 ]; then
      echo "Quick pre-check: building ${#quick_targets[@]} quick targets in parallel..."
      if nix_build "${quick_targets[@]}"; then
        echo "  ✓ quick pre-check passed"
      else
        echo "  ✗ quick pre-check had failures, falling back to sequential..."
        mode=sequential
      fi
    fi
  fi
  # Sequential quick: either started sequential, or parallel quick failed above.
  # In the fallback case this identifies which commits failed quick before the
  # expensive full phase. Nix caching prevents redundant builds.
  if [ "$mode" = sequential ]; then
    echo "Quick pre-check: running checks.$system.quick ($order)..."
    for idx in "${quick_ordered[@]}"; do
      hash=${linear[$idx]}
      [ -z "${expect_fail_check[$hash]:-}" ] || continue
      target="git+file://$repo_root?rev=$hash$shallow_param#checks.$system.quick"
      if ! nix_build "$target"; then
        echo "  ✗ $(fmt_commit "$hash") (quick failed)"
        build_failed+=("$hash")
        if [ "$fail_fast" = true ]; then break; fi
      fi
    done
  fi
fi

# Flake check phase (skipped with --no-flake-checks or --quick=only)
if [ "$run_flake_checks" = true ] && [ "$quick" != only ]; then
  if [ "$mode" = parallel ]; then
    echo "Flake check phase: collecting targets for parallel build..."
    # Categorize commits and collect nix build targets for batching
    flake_targets=()
    expect_fail_indices=()
    no_checks_indices=()
    for idx in "${flake_ordered[@]}"; do
      hash=${linear[$idx]}
      expect_fail="${expect_fail_check[$hash]:-}"
      if [ -n "$expect_fail" ]; then
        if expect_fail_matches_checks "$expect_fail"; then
          expect_fail_indices+=("$idx")
        fi
        continue
      fi
      flakeref="git+file://$repo_root?rev=$hash$shallow_param"
      n_before=${#flake_targets[@]} # track whether this commit adds any targets
      # --check mode: build only the named checks
      if [ ${#check_names[@]} -gt 0 ]; then
        for cn in "${check_names[@]}"; do
          target="$flakeref#checks.$system.$cn"
          if [ "$no_skip_missing" = true ]; then
            flake_targets+=("$target")
          elif nix eval --no-update-lock-file "$target" --apply 'x: true' >/dev/null 2>/dev/null; then
            flake_targets+=("$target")
          fi
        done
      else
        while IFS= read -r attr; do
          [ -n "$attr" ] || continue
          flake_targets+=("$flakeref#checks.$system.$attr")
        done < <(nix eval --no-update-lock-file "$flakeref#checks.$system" --apply 'cs: builtins.concatStringsSep "\n" (builtins.attrNames cs) + "\n"' --raw 2>/dev/null || true)
      fi
      if [ ${#flake_targets[@]} -eq "$n_before" ]; then
        no_checks_indices+=("$idx")
      fi
    done
    # Batch-build all collected targets; fall back to sequential on failure
    if [ ${#flake_targets[@]} -gt 0 ]; then
      n_commits=$((${#flake_ordered[@]} - ${#expect_fail_indices[@]} - ${#no_checks_indices[@]}))
      echo "Flake check phase: building ${#flake_targets[@]} targets across $n_commits commits..."
      if nix_build "${flake_targets[@]}"; then
        echo "  ✓ all flake checks passed"
      else
        echo "  ✗ parallel build had failures, falling back to sequential..."
        mode=sequential
      fi
    fi
    # Report skip/EXPECT-FAIL/no-checks only if parallel succeeded
    # (sequential fallback handles them in its own loop)
    if [ "$mode" = parallel ]; then
      for idx in "${expect_fail_indices[@]}"; do
        hash=${linear[$idx]}
        flakeref="git+file://$repo_root?rev=$hash$shallow_param"
        expect_fail="${expect_fail_check[$hash]:-}"
        named_target="$flakeref#checks.$system.$expect_fail"
        if ! nix eval --no-update-lock-file "$named_target" --apply 'x: true' >/dev/null 2>/dev/null; then
          echo "  ✗ $(fmt_commit "$hash") (EXPECT-FAIL: $expect_fail — check not defined)"
          build_failed+=("$hash")
        elif nix_build "$named_target"; then
          echo "  ✗ $(fmt_commit "$hash") (EXPECT-FAIL: $expect_fail unexpectedly passed)"
          build_failed+=("$hash")
        else
          echo "  ✓ $(fmt_commit "$hash") (EXPECT-FAIL: $expect_fail correctly fails)"
        fi
      done
      for idx in "${no_checks_indices[@]}"; do
        hash=${linear[$idx]}
        flakeref="git+file://$repo_root?rev=$hash$shallow_param"
        if nix_flake_check "$flakeref"; then
          echo "  ✓ $(fmt_commit "$hash") (flake check only)"
        else
          echo "  ✗ $(fmt_commit "$hash")"
          build_failed+=("$hash")
        fi
      done
    fi
  fi

  # Sequential mode: either started sequential, or parallel fell back above
  if [ "$mode" = sequential ]; then
    echo "Flake check phase: verifying $total commits ($order)..."
    for idx in "${ordered[@]}"; do
      check_one_commit "${linear[$idx]}" || {
        if [ "$fail_fast" = true ]; then break; fi
      }
    done
  fi
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
