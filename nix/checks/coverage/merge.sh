#!/usr/bin/env bash

# Usage: merge.sh OUT TRACEFILE...
#
# Combines the coverage reports of every collection into one lcov tracefile
# and one Cobertura report.

set -euo pipefail

out=$1
shift

mkdir -p "$out"

merge_args=()
for tracefile in "$@"; do
  merge_args+=(--add-tracefile "$tracefile")
done

lcov "${merge_args[@]}" --output-file "$out/coverage.lcov"
lcov_cobertura "$out/coverage.lcov" --base-dir . --output "$out/cobertura.xml"
# lcov_cobertura stamps the current time; keep the report reproducible.
sed -i 's/ timestamp="[0-9]*"/ timestamp="0"/' "$out/cobertura.xml"
