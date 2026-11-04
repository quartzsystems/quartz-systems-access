#!/usr/bin/env bash
# Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
# licensed under the terms of the MIT license which can be found in the
# root directory of this project.
#
# VERSION, at the root, is the product's version (MAJOR.MINOR.PATCH) and
# everything else follows it: the workspace version in Cargo.toml (and so
# CARGO_PKG_VERSION in every crate), the release VxRyMz derived from it, the
# installers' file names, the Windows version resources and the GitHub
# release.
#
#   ci/version.sh               print the version, 1.0.0
#   ci/version.sh release       print the release, V1R0M0
#   ci/version.sh check [TAG]   fail unless Cargo.toml's workspace version is
#                               VERSION; with a tag, also unless the tag is
#                               v<VERSION> and CHANGELOG.md has its section
#   ci/version.sh notes         print CHANGELOG.md's section for VERSION
#   ci/version.sh prepare X.Y.Z write X.Y.Z to VERSION, Cargo.toml and
#                               Cargo.lock, and turn CHANGELOG.md's
#                               Unreleased section into X.Y.Z's, dated today
set -euo pipefail
cd "$(dirname "$0")/.."

semver='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'

version() {
  local v
  v=$(tr -d ' \r\n' < VERSION)
  [[ $v =~ $semver ]] || { echo "VERSION: '$v' is not MAJOR.MINOR.PATCH" >&2; exit 1; }
  echo "$v"
}

# The first version = "..." line of Cargo.toml is [workspace.package]'s.
cargo_version() {
  sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1
}

# CHANGELOG.md's section for $1: from its "## $1" heading to the next "## ".
notes() {
  awk -v v="$1" '
    /^## / { if (on) exit; on = ($2 == v) ; next }
    on { print }
  ' CHANGELOG.md | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}'
}

case "${1:-}" in
  "")
    version
    ;;
  release)
    IFS=. read -r major minor patch <<< "$(version)"
    echo "V${major}R${minor}M${patch}"
    ;;
  check)
    v=$(version)
    c=$(cargo_version)
    [ "$c" = "$v" ] || { echo "Cargo.toml's workspace version is $c, VERSION is $v (ci/version.sh prepare $v)" >&2; exit 1; }
    if [ -n "${2:-}" ]; then
      [ "$2" = "v$v" ] || { echo "tag $2 is not v$v, the version in VERSION" >&2; exit 1; }
      [ -n "$(notes "$v")" ] || { echo "CHANGELOG.md has no '## $v' section, or it is empty" >&2; exit 1; }
    fi
    echo "version $v"
    ;;
  notes)
    notes "$(version)"
    ;;
  prepare)
    new=${2:-}
    [[ $new =~ $semver ]] || { echo "usage: ci/version.sh prepare MAJOR.MINOR.PATCH" >&2; exit 2; }
    if grep -q "^## $new\( \|$\)" CHANGELOG.md; then
      echo "CHANGELOG.md already has a section for $new" >&2; exit 1
    fi
    grep -q '^## Unreleased$' CHANGELOG.md || { echo "CHANGELOG.md has no '## Unreleased' section" >&2; exit 1; }
    printf '%s\n' "$new" > VERSION
    old=$(cargo_version)
    # Only the first match: [workspace.package]'s.
    sed -i "0,/^version = \"$old\"\$/s//version = \"$new\"/" Cargo.toml
    sed -i "s/^## Unreleased\$/## Unreleased\n\n## $new ($(date -u +%Y-%m-%d))/" CHANGELOG.md
    # The workspace crates' versions in the lock file.
    cargo update --workspace --quiet
    echo "prepared $new ($("$0" release)): review, commit, then tag v$new and push the tag"
    ;;
  *)
    echo "usage: ci/version.sh [release | check [TAG] | notes | prepare X.Y.Z]" >&2
    exit 2
    ;;
esac
