#!/usr/bin/env bash

set -euo pipefail

out=$1
features=$2

cargo llvm-cov nextest \
  --no-report \
  --remap-path-prefix \
  --no-default-features \
  --features "$features"

mkdir -p "$out"

cargo llvm-cov report \
  --lcov \
  --output-path "$out/coverage.lcov"

test -s "$out/coverage.lcov"
