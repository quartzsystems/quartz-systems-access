# The CI Linux job's environment, for running the tests and blessing the
# Linux screenshot goldens locally (see TESTING.md). Keep in step with the
# linux job in .github/workflows/ci.yml.
FROM ubuntu:24.04@sha256:fe81560990fae86dc4743202b2cb4d3df98d0bcf949173cecef15ce1127ef852
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git build-essential \
    cmake ninja-build pkg-config \
    libx11-dev libxext-dev libxft-dev libxinerama-dev libxcursor-dev \
    libxrender-dev libxfixes-dev libfontconfig1-dev \
    libpango1.0-dev libcairo2-dev \
    libwayland-dev wayland-protocols libxkbcommon-dev libdbus-1-dev \
    libgl1-mesa-dev libglu1-mesa-dev \
    xvfb xauth weston \
 && rm -rf /usr/share/fonts/* /usr/local/share/fonts/* && fc-cache -f
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal -c clippy,rustfmt
ENV PATH=/root/.cargo/bin:$PATH
RUN git config --global --add safe.directory /src
