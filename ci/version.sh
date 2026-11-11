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
#   ci/version.sh               print the version, 1.2.0
#   ci/version.sh release       print the release, V1R2M0
#   ci/version.sh check [TAG]   fail unless Cargo.toml's workspace version is
#                               VERSION; with a tag, also unless the tag is
#                               v<VERSION> and CHANGELOG.md has its section
#   ci/version.sh names         fail if a file names a release, VxRyMz or
#                               x.y.z, that is not VERSION's (see below)
#   ci/version.sh notes         print CHANGELOG.md's section for VERSION
#   ci/version.sh prepare X.Y.Z write X.Y.Z to VERSION, Cargo.toml and
#                               Cargo.lock, turn CHANGELOG.md's Unreleased
#                               section into X.Y.Z's, dated today, and
#                               rewrite the release names in the files
#                               `names` checks (the two examples above too)
#
# Release names. Wherever a file writes this product's release out (an
# installer's file name in the README, a tag in docs/releasing.md), it is
# VERSION's, and `names` fails on any other:
# - a VxRyMz name, anywhere;
# - an x.y.z version on a line that has a VxRyMz name, or after `prepare`,
#   `release`, `version` or `tag`, or written as a tag, vx.y.z.
# Not checked:
# - CHANGELOG.md from its first release heading on: past releases keep
#   their names;
# - the files in names_exempt below.
set -euo pipefail
cd "$(dirname "$0")/.."

semver='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'

version() {
  local v
  v=$(tr -d ' \r\n' < VERSION)
  [[ $v =~ $semver ]] || { echo "VERSION: '$v' is not MAJOR.MINOR.PATCH" >&2; exit 1; }
  echo "$v"
}

# The release of version $1: major as V, minor as R, patch as M.
release_of() {
  local major minor patch
  IFS=. read -r major minor patch <<< "$1"
  echo "V${major}R${minor}M${patch}"
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

# Files `names` does not check, each for one of these reasons and no other.
names_exempt=(
  # QS/400's own document: the release names in it are the system's.
  docs/qap-protocol-spec.md
  # The release a system says it is at (QAP Hello, a spooled file's page
  # heading): QS/400's or a test system's, never this product's.
  qap-client/tests/common/v1.rs
  qap-client/tests/connection.rs
  qsaccess/tests/screenshots.rs
  qsaccess/tests/verify.rs
  qsaccess/tests/goldens/spool/QPJOBLOG.pages
  qsaccess/tests/goldens/spool/QPJOBLOG.txt
  qsa-print/tests/goldens/listing-132.pages
  qsa-print/tests/goldens/listing-132.txt
  qsa-print/tests/pdf_goldens.rs
  # Other packages' versions (the workspace's own is `check`'s).
  Cargo.lock
  THIRD_PARTY.md
)

# The text files `names` checks: tracked or new, and not exempt.
names_files() {
  local ex=() e
  for e in "${names_exempt[@]}"; do ex+=(-e "$e"); done
  # git grep -I leaves binary files out.
  git grep -I -l --untracked -e '' -- . | grep -vxF "${ex[@]}"
}

# Prints file:line:text for each line of the files $3... with a release name
# that is not release $1 (version $2). CHANGELOG.md is read up to its first
# release heading. In a Cargo.toml only VxRyMz names are looked at: every
# `version = "x.y.z"` there is a dependency's or a tool crate's.
names_scan() {
  local rel=$1 ver=$2
  shift 2
  [ $# -gt 0 ] || return 0
  awk -v rel="$rel" -v ver="$ver" '
    # Whether every x.y.z in s that is not part of a longer dotted number
    # is ver.
    function versions_agree(s,    t) {
      while (match(s, /(^|[^0-9.])[0-9]+[.][0-9]+[.][0-9]+([^0-9.]|$)/)) {
        t = substr(s, RSTART, RLENGTH)
        gsub(/^[^0-9]+|[^0-9]+$/, "", t)
        if (t != ver) return 0
        s = substr(s, RSTART + RLENGTH - 1)
      }
      return 1
    }
    FNR == 1 { past = 0 }
    FILENAME == "CHANGELOG.md" && /^## [0-9]/ { past = 1 }
    past { next }
    {
      line = $0
      sub(/\r$/, "", line)
      wrong = 0
      named = 0
      s = line
      while (match(s, /V[0-9]+R[0-9]+M[0-9]+/)) {
        named = 1
        if (substr(s, RSTART, RLENGTH) != rel) wrong = 1
        s = substr(s, RSTART + RLENGTH)
      }
      if (!wrong && FILENAME !~ /(^|\/)Cargo[.]toml$/) {
        if (named) {
          wrong = !versions_agree(line)
        } else {
          s = line
          while (!wrong && match(s, /((prepare|release|version|tag)[^0-9\/]{0,24}|(^|[^A-Za-z0-9_\/.-])v)[0-9]+[.][0-9]+[.][0-9]+([^0-9.]|$)/)) {
            t = substr(s, RSTART, RLENGTH)
            sub(/[^0-9]+$/, "", t)
            sub(/^.*[^0-9.]/, "", t)
            if (t != ver) wrong = 1
            s = substr(s, RSTART + RLENGTH - 1)
          }
        }
      }
      if (wrong) printf "%s:%d:%s\n", FILENAME, FNR, line
    }
  ' "$@"
}

# Fails, listing the lines, unless every release name is $1 ($2).
names_check() {
  local files=() hits
  mapfile -t files < <(names_files)
  hits=$(names_scan "$1" "$2" "${files[@]}")
  if [ -n "$hits" ]; then
    printf '%s\n' "$hits" >&2
    echo "the release names above are not $1 ($2), the version in VERSION" >&2
    return 1
  fi
  echo "release names agree with VERSION: $1 ($2), ${#files[@]} files"
}

# Rewrites release $1 (version $2) to $3 ($4) on each line names_scan reports
# now that VERSION is $4. CHANGELOG.md is left alone: its Unreleased section
# has just become $4's, and the rest is past releases.
names_rewrite() {
  local old_rel=$1 old_re=${2//./[.]} new_rel=$3 new=$4 f n
  while IFS= read -r f; do
    [ "$f" = CHANGELOG.md ] && continue
    for n in $(names_scan "$new_rel" "$new" "$f" | cut -d: -f2); do
      sed -i -E \
        -e "${n}s/$old_rel/$new_rel/g" \
        -e "${n}s/(^|[^0-9.])$old_re([^0-9.]|\$)/\\1$new\\2/g" \
        "$f"
    done
  done < <(names_files)
}

case "${1:-}" in
  "")
    version
    ;;
  release)
    release_of "$(version)"
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
  names)
    v=$(version)
    names_check "$(release_of "$v")" "$v"
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
    was=$(version)
    printf '%s\n' "$new" > VERSION
    old=$(cargo_version)
    # Only the first match: [workspace.package]'s.
    sed -i "0,/^version = \"$old\"\$/s//version = \"$new\"/" Cargo.toml
    sed -i "s/^## Unreleased\$/## Unreleased\n\n## $new ($(date -u +%Y-%m-%d))/" CHANGELOG.md
    # The workspace crates' versions in the lock file.
    cargo update --workspace --quiet
    # The release names written out in the README, the documents and the
    # scripts' examples.
    names_rewrite "$(release_of "$was")" "$was" "$(release_of "$new")" "$new"
    names_check "$(release_of "$new")" "$new"
    echo "prepared $new ($("$0" release)): review, commit, then tag v$new and push the tag"
    ;;
  *)
    echo "usage: ci/version.sh [release | check [TAG] | names | notes | prepare X.Y.Z]" >&2
    exit 2
    ;;
esac
