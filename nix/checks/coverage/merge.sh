#!/usr/bin/env bash

# Usage: merge.sh OUT TRACEFILE...
#
# Combines the coverage reports of every collection into one lcov tracefile.

set -euo pipefail

out=$1
shift

mkdir -p "$out"

merge_args=()
for tracefile in "$@"; do
  merge_args+=(--add-tracefile "$tracefile")
done

lcov "${merge_args[@]}" --output-file "$out/coverage.lcov"
