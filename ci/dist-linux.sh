#!/usr/bin/env bash
# Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
# licensed under the terms of the MIT license which can be found in the
# root directory of this project.
#
# Builds the Linux installer in the pinned glibc 2.31 image
# (ci/dist-linux.Dockerfile) and writes it to target/dist/.
set -euo pipefail
cd "$(dirname "$0")/.."
docker build -q -t qsa-dist-linux -f ci/dist-linux.Dockerfile ci >/dev/null
docker run --rm -v "$PWD:/src" -v qsa-dist-target:/target -v qsa-dist-cargo:/root/.cargo/registry \
  -e CARGO_TARGET_DIR=/target -w /src qsa-dist-linux \
  bash -c 'cargo xtask dist && mkdir -p /src/target/dist && cp /target/dist/QS400-Access-*-linux-x86_64 /src/target/dist/'
