#!/usr/bin/env bash

set -euo pipefail

out=$1
coverage_percent=$2
tracefile=$3

mkdir -p "$out"
cp "$tracefile" "$out/coverage.lcov"

lcov \
  --summary "$out/coverage.lcov" \
  --fail-under-lines "$coverage_percent"
