# Builds the Linux installer against glibc 2.31 (Ubuntu 20.04), so that it
# runs on every distribution with that glibc or newer. The base image is
# pinned by digest and CMake by version and checksum, so the build only
# changes when this file does. See ci/dist-linux.sh and README.md.
FROM ubuntu:20.04@sha256:8feb4d8ca5354def3d8fce243717141ce31e2c428701f6682bd2fafe15388214
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git build-essential pkg-config ninja-build \
    libx11-dev libxext-dev libxft-dev libxinerama-dev libxcursor-dev \
    libxrender-dev libxfixes-dev libfontconfig1-dev \
    libpango1.0-dev libcairo2-dev \
    libwayland-dev wayland-protocols libxkbcommon-dev libdbus-1-dev \
    libgl1-mesa-dev libglu1-mesa-dev \
 && rm -rf /var/lib/apt/lists/*
# FLTK needs CMake 3.28 or later; 20.04 has 3.16.
RUN curl -sSfLo /tmp/cmake.tgz https://github.com/Kitware/CMake/releases/download/v3.31.6/cmake-3.31.6-linux-x86_64.tar.gz \
 && echo "5a1133ff103c71eb5120e2cc3de922733e7d8a26a98ae716397e8676adb367bf  /tmp/cmake.tgz" | sha256sum -c - \
 && tar -xzf /tmp/cmake.tgz -C /opt && ln -s /opt/cmake-3.31.6-linux-x86_64/bin/cmake /usr/local/bin/cmake \
 && rm /tmp/cmake.tgz
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.98.0
ENV PATH=/root/.cargo/bin:$PATH
RUN git config --global --add safe.directory /src
