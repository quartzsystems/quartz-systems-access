#!/usr/bin/env bash
# docs/qap-protocol-spec.md is a read-only copy of QS/400's docs/QAP.md at the
# commit the workspace pins ipc, qap-proto and qapd to. Its first line names
# that commit; the rest is the file, byte for byte. This fails when:
#
# - the three crates are pinned to different commits;
# - the first line names another commit than Cargo.toml pins (the pin moved
#   and the copy was not refreshed in the same commit);
# - the rest differs from docs/QAP.md in cargo's checkout of that commit.
#
# The spec is never edited here: changes go to QS/400 first, then the pin
# and this copy move together. Pins renamed for tests of older servers
# (qapd_v1) are not the protocol's pin and are not checked.
set -euo pipefail
cd "$(dirname "$0")/.."
mirror=docs/qap-protocol-spec.md

revs="$(grep -E '^(ipc|qap-proto|qapd) = \{ git = ' Cargo.toml | grep -oE 'rev = "[0-9a-f]+"' | sort -u)"
if [ "$(printf '%s\n' "$revs" | wc -l)" -ne 1 ] || [ -z "$revs" ]; then
  echo "spec mirror: ipc, qap-proto and qapd must share one rev in Cargo.toml:" >&2
  printf '%s\n' "$revs" >&2
  exit 1
fi
rev="${revs#rev = \"}"
rev="${rev%\"}"

named="$(head -n 1 "$mirror" | tr -d '\r' | grep -oE 'at commit [0-9a-f]{40}' | cut -d' ' -f3 || true)"
if [ "$named" != "$rev" ]; then
  echo "spec mirror: $mirror names commit '${named:-none}' but Cargo.toml pins $rev." >&2
  echo "Copy QS/400's docs/QAP.md at $rev into it in the same commit as the re-pin." >&2
  exit 1
fi

# cargo's checkout of the pinned commit: qap-proto is work/qap-proto in it,
# under a folder named for the commit's first seven digits. Other checkouts
# (qapd_v1's older qap-proto) are left out.
manifest="$(cargo metadata --format-version 1 --locked 2>/dev/null \
  | grep -oE '"manifest_path":"[^"]*qap-proto[\\/]+Cargo\.toml"' \
  | sed -e 's/^"manifest_path":"//' -e 's/"$//' | tr -s '\\' '/' \
  | grep -F "/${rev:0:7}/" | head -n 1)"
if [ -z "$manifest" ]; then
  echo "spec mirror: cargo metadata did not list qap-proto" >&2
  exit 1
fi
source="$(dirname "$manifest")/../../docs/QAP.md"
if ! diff -q <(tail -n +2 "$mirror" | tr -d '\r') <(tr -d '\r' < "$source") >/dev/null; then
  echo "spec mirror: $mirror differs from docs/QAP.md at $rev:" >&2
  diff <(tail -n +2 "$mirror" | tr -d '\r') <(tr -d '\r' < "$source") | head -n 40 >&2 || true
  exit 1
fi
echo "spec mirror matches QS/400 docs/QAP.md at $rev"
