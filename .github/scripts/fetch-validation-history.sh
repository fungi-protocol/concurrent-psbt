#!/usr/bin/env bash
#
# This helper ensures that shallow checkouts (which are preferred in CI) are
# sufficiently deep to inspect every commit in the range, by deepening until
# every commit that is not an ancestor of base but is an ancestor of the merge
# tip is within the shallow commit boundary.

set -euo pipefail

base=${1:?Usage: fetch-validation-history.sh <base-sha>}
head=$(git rev-parse HEAD)

# ensure base commit is fetched
if ! git cat-file -e "$base^{commit}" 2>/dev/null; then
  git fetch --no-tags --depth=1 origin "$base"
fi

base=$(git rev-parse "$base^{commit}")
shallow_file=$(git rev-parse --git-path shallow)

# git merge-base --is-ancestor $base $head doesn't mean all the commits are present.
#
# Once the shallow boundary can no longer affect the range, no deepening is required:
#
# 1. No commit in the range lacks parents, because shallow boundary commits are
#    grafted with no parents. (note: merging unrelated histories will break
#    this) and a true root inside the range means the base side has not been
#    deepened down to the fork point yet.
#
# 2. Every parentless commit on the base side is an ancestor of every join,
#    i.e. of every excluded parent of a range commit.  Otherwise a commit in
#    the range may still be an ancestor of $base through history that has not
#    been fetched: it would have to be an ancestor of that parentless commit
#    and a descendant of a join, which is impossible once 2 holds.
range_needs_deepening() {
  [[ -s $shallow_file ]] || return 1

  local roots joins join
  roots=$(git rev-list --max-parents=0 "$base..$head") || exit "$?"
  [[ -z $roots ]] || return 0

  joins=$(git rev-list --boundary "$base..$head" | sed -n 's/^-//p') || exit "$?"
  for join in $joins; do
    roots=$(git rev-list --max-parents=0 "$base" --not "$join") || exit "$?"
    [[ -z $roots ]] || return 0
  done
  return 1
}

deepen=8
while range_needs_deepening; do
  git fetch --no-tags --deepen="$deepen" origin "$head" "$base"
  deepen=$((deepen * 2))
done
