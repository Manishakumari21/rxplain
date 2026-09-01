#!/usr/bin/env bash
# Install rxplain from source (builds the release binary).
#
# Usage:
#   ./scripts/install.sh                 # install to $CARGO_HOME/bin (or ~/.cargo/bin)
#   ./scripts/install.sh /some/bin/path  # install to a specific directory

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

if [[ $# -ge 1 ]]; then
    INSTALL_DIR="$1"
else
    INSTALL_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
fi

echo "Building rxplain (release)…"
( cd "$ROOT_DIR" && cargo build --release )

mkdir -p "$INSTALL_DIR"
cp "$ROOT_DIR/target/release/rxplain" "$INSTALL_DIR/rxplain"

echo
echo "Installed rxplain to: $INSTALL_DIR/rxplain"
echo
echo "Ensure it is on your PATH, then verify with:"
echo "    rxplain --version"
