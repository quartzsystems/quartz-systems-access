#!/usr/bin/env bash
# Fails if any Rust source file does not start with the project copyright header.
set -euo pipefail
cd "$(dirname "$0")/.."
expected='// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.'
bad=0
while IFS= read -r f; do
  [ -f "$f" ] || continue
  if [ "$(head -n 3 "$f" | tr -d '\r')" != "$expected" ]; then
    echo "missing copyright header: $f" >&2
    bad=1
  fi
done < <(git ls-files -co --exclude-standard -- '*.rs')
[ "$bad" -eq 0 ] && echo "copyright headers present"
exit "$bad"
