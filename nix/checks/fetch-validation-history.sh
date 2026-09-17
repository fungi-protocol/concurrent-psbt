#!/usr/bin/env bash
# Test suite for .github/scripts/fetch-validation-history.sh.
#
# Usage: fetch-validation-history.sh <path to the helper>
#
# The path is given as an argument for execution in a the nix sandbox.
#
# Each scenario below builds a test git repo, makes a shallow clone of it, then
# runs the helper against a base commit and compares the resulting range
# with the complete history in the remote.
set -euo pipefail

helper=$1
[[ $helper == /* ]] || helper=$PWD/$helper
test_root=$(mktemp -d)
trap 'rm -rf "$test_root"' EXIT
export GIT_AUTHOR_NAME=Test GIT_AUTHOR_EMAIL=test@example.com
export GIT_COMMITTER_NAME=Test GIT_COMMITTER_EMAIL=test@example.com

# The main fixture: a merge queue commit joining a 40 commit pull request
# branch onto a base branch 160 commits deep.
#
#   root - main 0 ... main 159 (= base) ------- merge (= head, branch queue)
#                             \\                /
#                              side 0 ... side 39
#
# The 40 side commits are longer than the first two deepening rounds together
# (8 + 16 = 24), so completing the range takes three rounds and exercises the
# doubling.  The 160 main commits exceed the total those rounds fetch on the
# base side (8 + 16 + 32 = 56), so deepening never reaches the root and the
# clones stay shallow, which check_range asserts.
git init -q "$test_root/remote"
cd "$test_root/remote"
tree=$(git mktree </dev/null)
base=$(git commit-tree "$tree" -m root)
for ((i = 0; i < 160; i++)); do
  base=$(git commit-tree "$tree" -p "$base" -m "main $i")
done
side=$base
for ((i = 0; i < 40; i++)); do
  side=$(git commit-tree "$tree" -p "$side" -m "side $i")
done
head=$(git commit-tree "$tree" -p "$base" -p "$side" -m merge)
git update-ref refs/heads/main "$base"
git update-ref refs/heads/queue "$head"
expected=$(git rev-list "$base..$head" | sort)

# The helper must complete the range while leaving the repository shallow, i.e.
# without falling back to fetching the entire history.
check_range() {
  [[ $(git rev-list "$base..HEAD" | sort) == "$expected" ]]
  [[ $(git rev-parse --is-shallow-repository) == true ]]
}

# A depth 2 clone holds the merge and both of its parents, so the base is
# already an ancestor of HEAD although the side branch behind it is truncated.
# This is the case that makes merge-base --is-ancestor insufficient.
git clone -q --depth=2 --branch=queue "file://$test_root/remote" "$test_root/side"
cd "$test_root/side"
git merge-base --is-ancestor "$base" HEAD
[[ $(git rev-list "$base..HEAD" | sort) != "$expected" ]]
if [[ ! -f $helper ]]; then
  echo 'FAIL: bounded validation-history fetch is not implemented' >&2
  exit 1
fi
bash "$helper" "$base"
check_range
echo 'PASS: deepen a truncated merge parent even when base is already reachable'

# A depth 1 clone, as in the merge queue workflow, does not even contain the
# base commit; the helper has to fetch it before it can reason about the range.
git clone -q --depth=1 --branch=queue "file://$test_root/remote" "$test_root/missing"
cd "$test_root/missing"
if git cat-file -e "$base^{commit}" 2>/dev/null; then
  echo 'FAIL: expected missing base' >&2
  exit 1
fi
bash "$helper" "$base"
check_range
echo 'PASS: fetch a missing base and complete the range without full history'

# Depth 42 covers the merge, the 40 side commits and the base, so the range is
# complete while the repository is still shallow on the base side.  Pointing
# origin at a nonexistent path proves the helper does not fetch at all, both
# for this complete range and for the empty range HEAD..HEAD.
git clone -q --depth=42 --branch=queue "file://$test_root/remote" "$test_root/complete"
cd "$test_root/complete"
check_range
git remote set-url origin "$test_root/unavailable"
bash "$helper" "$base"
bash "$helper" "$head"
echo 'PASS: complete and empty ranges need no fetch despite shallow history'

# When deepening is required and the fetch fails, the helper must fail rather
# than let validation run on a truncated range.
git clone -q --depth=2 --branch=queue "file://$test_root/remote" "$test_root/failure"
cd "$test_root/failure"
git remote set-url origin "$test_root/unavailable"
if bash "$helper" "$base" >"$test_root/failure.log" 2>&1; then
  echo 'FAIL: fetch failure was accepted as complete history' >&2
  exit 1
fi
echo 'PASS: fetch failure stops validation'

# An already complete repository needs no fetch and must succeed.
git clone -q --branch=queue "file://$test_root/remote" "$test_root/full"
cd "$test_root/full"
git remote set-url origin "$test_root/unavailable"
bash "$helper" "$base"
echo 'PASS: a full clone succeeds without fetching'

# Deepening can remove the shallow file when the entire history is fetched.
# Validating against the root commit forces exactly that, and the helper must
# then stop without stumbling over the missing file.
root=$(git rev-list --max-parents=0 HEAD)
git clone -q --depth=2 --branch=queue "file://$test_root/remote" "$test_root/unshallow"
cd "$test_root/unshallow"
bash "$helper" "$root" 2>"$test_root/unshallow.log"
[[ $(git rev-parse --is-shallow-repository) == false ]]
if grep -q '^grep:' "$test_root/unshallow.log"; then
  echo 'FAIL: read the shallow file after it was removed' >&2
  exit 1
fi
echo 'PASS: deepening to full history stops without reading a removed file'

# Compare the local range against the complete history in the remote.
assert_range() {
  local remote=$1 base=$2
  [[ $(git rev-list "$base..HEAD" | sort) == "$(git -C "$remote" rev-list "$base..queue" | sort)" ]]
}

# A pull request that forked before the base branch advanced, fetched from a
# depth 1 checkout as in the merge queue.
#
#   root (= fork) - main 0 ... main 39 (= main) - merge (= queue)
#                 \\                              /
#                  pr ---------------------------
#
# Deepening reaches the root on the pull request side after the first round
# while the base side, 40 commits long, is still truncated after two rounds
# (8 + 16 = 24).  The shallow boundary is then an ancestor of the base tip and
# never enters the range, yet the range still contains shared main history
# that must not be accepted as part of the pull request.
git init -q "$test_root/behind-remote"
cd "$test_root/behind-remote"
fork=$(git commit-tree "$tree" -m root)
pr=$(git commit-tree "$tree" -p "$fork" -m pr)
main=$fork
for ((i = 0; i < 40; i++)); do
  main=$(git commit-tree "$tree" -p "$main" -m "main $i")
done
queue=$(git commit-tree "$tree" -p "$main" -p "$pr" -m merge)
git update-ref refs/heads/main "$main"
git update-ref refs/heads/queue "$queue"
git clone -q --depth=1 --branch=queue "file://$test_root/behind-remote" "$test_root/behind"
cd "$test_root/behind"
bash "$helper" "$main"
assert_range "$test_root/behind-remote" "$main"
echo 'PASS: exclude the shared history when the pull request is behind the base'

# The base branch merged a branch that forked from the parent of the pull
# request's fork point.
#
#   root - shared 0 ... shared 199 - fork - main 0 ... main 19 - merge branch (= main) - merge (= queue)
#                                  \\    \\                     /                          /
#                                   \\    branch ---------------                          /
#                                    pr ------------------------------------------------
#
# Deepening reaches shared 199 through the two commit merged branch, so no
# commit in the range is parentless, while the fork point itself is reachable
# from the base tip only through its 21 commit first-parent history, which is
# still truncated.  The fork point must not stay in the range, and the 200
# shared commits below it must not be fetched to decide that: the repository
# has to remain shallow.
git init -q "$test_root/merged-remote"
cd "$test_root/merged-remote"
shared=$(git commit-tree "$tree" -m root)
for ((i = 0; i < 200; i++)); do
  shared=$(git commit-tree "$tree" -p "$shared" -m "shared $i")
done
fork=$(git commit-tree "$tree" -p "$shared" -m fork)
branch=$(git commit-tree "$tree" -p "$shared" -m branch)
main=$fork
for ((i = 0; i < 20; i++)); do
  main=$(git commit-tree "$tree" -p "$main" -m "main $i")
done
main=$(git commit-tree "$tree" -p "$main" -p "$branch" -m 'merge branch')
pr=$(git commit-tree "$tree" -p "$fork" -m pr)
queue=$(git commit-tree "$tree" -p "$main" -p "$pr" -m merge)
git update-ref refs/heads/main "$main"
git update-ref refs/heads/queue "$queue"
git clone -q --depth=1 --branch=queue "file://$test_root/merged-remote" "$test_root/merged"
cd "$test_root/merged"
bash "$helper" "$main"
assert_range "$test_root/merged-remote" "$main"
[[ $(git rev-parse --is-shallow-repository) == true ]]
echo 'PASS: exclude a fork point the base reaches only through truncated history'

# Inject command errors while keeping the real shallow Git fixture.  The
# helper is run as a child Bash process, so a shell function exported with
# export -f shadows the git command inside it.
cd "$test_root/complete"
expect_error() {
  local expected=$1
  local actual=0
  bash "$helper" "$base" || actual=$?
  if [[ $actual != "$expected" ]]; then
    echo "FAIL: expected exit $expected, got $actual" >&2
    exit 1
  fi
}

# Invoked by the helper's child Bash process via export -f.  Each override
# lives in a subshell so that the outer script keeps the real command.
(
  # SC2329 warns about a function that is never invoked; this one is, but only
  # indirectly by the helper's child Bash process, which ShellCheck cannot see.
  # shellcheck disable=SC2329
  git() {
    if [[ $1 == rev-list ]]; then return 42; fi
    command git "$@"
  }
  export -f git
  expect_error 42
)
echo 'PASS: revision listing errors stop validation'
