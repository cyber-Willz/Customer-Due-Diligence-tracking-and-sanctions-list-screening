#!/usr/bin/env bash
# Fetch the current Consolidated Belize Sanctions List (XML) from the Belize FIU site and load it.
# The file name changes with each edition (e.g. .../2026/07/belize_consolidated_sanctions_listJULY2026.xml),
# so the link is read from the FIU page rather than hard-coded.
#   usage: scripts/update_belize_list.sh STATE.json [path/to/cddctl]
# Schedule it (cron/systemd) at least weekly, then run `cddctl --state STATE.json stale` as a monitor.
set -euo pipefail
STATE=${1:?usage: update_belize_list.sh STATE.json [cddctl]}
BIN=${2:-cddctl}
PAGE=https://fiubelize.org/consolidated-belize-sanctions-list/
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT

curl -fsSL --retry 3 "$PAGE" -o "$TMP/page.html"
URL=$(grep -oE 'https://fiubelize\.org/wp-content/uploads/[^"'"'"' <>]+\.xml' "$TMP/page.html" | head -n1 || true)
[ -n "$URL" ] || { echo "no XML link found on $PAGE (page layout changed?)" >&2; exit 1; }

curl -fsSL --retry 3 "$URL" -o "$TMP/list.xml"
head -c 200 "$TMP/list.xml" | grep -q '<?xml' || { echo "downloaded file is not XML: $URL" >&2; exit 1; }

# The engine itself refuses an empty list or one that shrinks by more than half (truncated download).
"$BIN" --state "$STATE" --actor "${USER:-scheduler}" --role analyst \
  load-list BZ-FIU belize "$TMP/list.xml" --label "$(basename "$URL" .xml)"
