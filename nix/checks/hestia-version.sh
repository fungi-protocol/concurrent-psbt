#!/usr/bin/env bash

# Usage: hestia-version.sh WORKFLOW...
#
# Fails unless every Mic92/hestia step sets `version` to the release named in
# its pin comment.

set -euo pipefail

status=0
for workflow in "$@"; do
  # A tab separator would merge empty fields; | never occurs in them.
  while IFS='|' read -r uses pin version; do
    # yq prints an empty line for a workflow without hestia steps.
    [[ -n $uses ]] || continue
    if [[ -z $pin || $version != "$pin" ]]; then
      echo "$workflow: $uses pins '${pin:-no release comment}' but sets version '${version:-latest}'" >&2
      status=1
    fi
  done < <(
    yq -r '..
      | select(tag == "!!map" and has("uses"))
      | select(.uses | test("^Mic92/hestia@"))
      | [.uses, (.uses | line_comment), (.with.version // "")]
      | join("|")' "$workflow"
  )
done

exit "$status"
