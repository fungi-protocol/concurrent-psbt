#!/usr/bin/env bash

set -euo pipefail

gate=$1
out=$2

work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

cat >"$work_dir/unit-only.lcov" <<'EOF'
TN:unit_only
SF:example.rs
DA:1,1
DA:2,0
LF:2
LH:1
end_of_record
EOF

cat >"$work_dir/prop-only.lcov" <<'EOF'
TN:prop_only
SF:example.rs
DA:1,0
DA:2,1
LF:2
LH:1
end_of_record
EOF

bash "$gate" "$work_dir/unit-gate" 50 "$work_dir/unit-only.lcov"
bash "$gate" "$work_dir/prop-gate" 50 "$work_dir/prop-only.lcov"

if bash "$gate" "$work_dir/incomplete-gate" 100 "$work_dir/unit-only.lcov"; then
  echo "coverage gate accepted an incomplete tracefile"
  exit 1
fi

bash "$gate" "$work_dir/combined-gate" 100 \
  "$work_dir/unit-only.lcov" \
  "$work_dir/prop-only.lcov"

test -s "$work_dir/combined-gate/coverage.lcov"
grep -q '^DA:1,1$' "$work_dir/combined-gate/coverage.lcov"
grep -q '^DA:2,1$' "$work_dir/combined-gate/coverage.lcov"

mkdir -p "$out"
