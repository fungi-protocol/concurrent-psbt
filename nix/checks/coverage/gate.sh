#!/usr/bin/env bash

set -euo pipefail

out=$1
coverage_percent=$2
shift 2

mkdir -p "$out"

if (($# == 1)); then
  cp "$1" "$out/coverage.lcov"
else
  merge_args=()
  for tracefile in "$@"; do
    merge_args+=(--add-tracefile "$tracefile")
  done

  lcov "${merge_args[@]}" --output-file "$out/coverage.lcov"
fi

lcov \
  --summary "$out/coverage.lcov" \
  --fail-under-lines "$coverage_percent"
