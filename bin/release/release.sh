#!/bin/bash
set -Eeuo pipefail

PATH="$HOME"/.cargo/bin:$PATH
export PATH
# Builds hurl and hurlfmt. With `--workspace`, Hurl UI (hurlui) is built too: this is how packages are built
# (Node.js and the libraries of bin/install_prerequisites_hurlui_ubuntu.sh are needed).
cargo build --release --verbose --locked "$@"

# Return PATH var to parent shell
package_dir="$(cd target/release ; pwd)"
echo "Run this if you want to use fresh built Hurl package:"
echo "  export PATH=$package_dir:$PATH"

