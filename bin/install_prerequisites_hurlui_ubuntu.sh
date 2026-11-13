#!/bin/bash
set -Eeuo pipefail

# Prerequisites to build Hurl UI (`cargo build --workspace`), in addition to the ones of Hurl: system libraries of
# Tauri. Node.js (>= 20) and npm are also needed.
echo "----- install Hurl UI prerequisite packages -----"
sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get -y install \
    build-essential \
    curl \
    file \
    pkg-config \
    libayatana-appindicator3-dev \
    libgtk-3-dev \
    librsvg2-dev \
    libssl-dev \
    libwebkit2gtk-4.1-dev \
    libxdo-dev
