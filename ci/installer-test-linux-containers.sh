#!/usr/bin/env bash
# Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
# licensed under the terms of the MIT license which can be found in the
# root directory of this project.
#
# Runs ci/installer-test-linux.sh in fresh Ubuntu 24.04 and AlmaLinux 9
# containers, as root (all users) and as an ordinary user (just me). Only
# the libraries the program loads and the desktop tools the installer runs
# are added to each image. See TESTING.md.
#
#   ci/installer-test-linux-containers.sh target/dist/QS400-Access-V1R2M0-linux-x86_64
set -euo pipefail
cd "$(dirname "$0")/.."
installer="$1"
ubuntu="ubuntu:24.04@sha256:fe81560990fae86dc4743202b2cb4d3df98d0bcf949173cecef15ce1127ef852"
alma="almalinux:9@sha256:245f8aab832fe800aa42bdab5fad75ebbd8e0b4c9f24820a83c638b177857ae2"
ubuntu_setup="apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
  libwayland-client0 libwayland-cursor0 libxkbcommon0 libx11-6 libxinerama1 libxcursor1 libxfixes3 \
  libfontconfig1 libpango-1.0-0 libpangocairo-1.0-0 libcairo2 libglib2.0-0 libdbus-1-3 \
  shared-mime-info desktop-file-utils xdg-utils libglib2.0-bin xvfb xauth procps >/dev/null"
alma_setup="dnf install -y -q libwayland-client libwayland-cursor libxkbcommon libX11 libXinerama libXcursor libXfixes \
  fontconfig pango cairo glib2 dbus-libs shared-mime-info desktop-file-utils xdg-utils xorg-x11-server-Xvfb \
  procps-ng findutils diffutils which >/dev/null"
status=0
for image in "$ubuntu" "$alma"; do
    setup="$ubuntu_setup"; name="Ubuntu 24.04"
    [ "$image" = "$alma" ] && setup="$alma_setup" && name="AlmaLinux 9"
    for scope in machine user; do
        echo "=== $name, $scope"
        inner="$setup; cp /in/installer /tmp/qsa-installer && chmod 755 /tmp/qsa-installer"
        if [ "$scope" = machine ]; then
            inner="$inner && /in/test.sh /tmp/qsa-installer machine"
        else
            inner="$inner && useradd -m tester && mkdir -p /home/tester/Desktop && chown tester /home/tester/Desktop \
              && su - tester -c 'PATH=\$HOME/.local/bin:\$PATH /in/test.sh /tmp/qsa-installer user'"
        fi
        docker run --rm -v "$(pwd -W 2>/dev/null || pwd)/$installer:/in/installer:ro" \
            -v "$(pwd -W 2>/dev/null || pwd)/ci/installer-test-linux.sh:/in/test.sh:ro" \
            "$image" bash -c "$inner" || status=1
    done
done
exit $status
