#!/usr/bin/env bash
# scrub-commit-history — verify commit hygiene across history
#
# Walks commit history and checks each commit for unresolved work-item
# markers in messages (TODO, FIXME, WIP, fixup!, squash!).
set -euo pipefail

usage() {
  cat >&2 <<EOF
Usage: ${0##*/} [options] [-r REVSET]...

Options:
  -h, --help          show this help
  ...                 jj log arguments (passed through, e.g. [ -r "ancestors(@)" ])

Range defaults to 'trunk()..@' (commits since trunk)
EOF
  exit 1
}

jj_log_args=()

while [ $# -gt 0 ]; do
  case "$1" in
  -h | --help) usage ;;
  *) jj_log_args+=("$1") ;;
  esac
  shift
done

# Resolve revsets to git commit IDs via jj
if [ ${#jj_log_args[@]} -gt 0 ]; then
  jj_args=(log --ignore-working-copy --no-graph -T 'commit_id ++ "\n"')
  jj_args+=("${jj_log_args[@]}")
  mapfile -t linear < <(jj "${jj_args[@]}" 2>/dev/null | grep -vE '^0*$')
else
  # Default: commits since trunk (falls back to all-but-root if trunk is root)
  mapfile -t linear < <(jj log --ignore-working-copy --no-graph -r 'trunk()..@' -T 'commit_id ++ "\n"' 2>/dev/null | grep -vE '^0*$')
fi

total=${#linear[@]}
if [ "$total" -eq 0 ]; then
  echo "No commits in range."
  exit 0
fi
fmt_commit() {
  jj log --ignore-working-copy --no-graph -r "$1" \
    -T 'change_id.shortest(7) ++ " " ++ commit_id.shortest(7) ++ " " ++ description.first_line()' \
    2>/dev/null || git log -1 --format='%h %s' "$1"
}

# Check each commit message for unresolved work-item markers
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

# Summary
if [ "${#msg_failed[@]}" -eq 0 ]; then
  echo "All $total commits passed."
else
  echo
  echo "${#msg_failed[@]} failure(s):"
  for h in "${msg_failed[@]}"; do
    echo "  message: $(fmt_commit "$h")"
  done
  exit 1
fi
