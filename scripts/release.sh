#!/usr/bin/env bash
# Create a GitHub release for rxplain from the release binary.
#
# Requires: gh (GitHub CLI) and the release binary already built.
#
# Usage:
#   ./scripts/release.sh v0.1.0          # tag + release from the local release build
#
# The binary is uploaded as a compressed tarball named rxplain-<tag>-x86_64-unknown-linux-gnu.tar.gz

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

if [[ $# -lt 1 ]]; then
    echo "usage: $0 <tag>   e.g. $0 v0.1.0"
    exit 1
fi

TAG="$1"
BIN="$ROOT_DIR/target/release/rxplain"

if [[ ! -x "$BIN" ]]; then
    echo "release binary not found; run: cargo build --release"
    exit 1
fi

if ! command -v gh >/dev/null 2>&1; then
    echo "gh (GitHub CLI) is required."
    exit 1
fi

TMP="${TMPDIR:-/tmp}/rxplain-release-$TAG"
rm -rf "$TMP"
mkdir -p "$TMP/rxplain"

cp "$BIN" "$TMP/rxplain/rxplain"
cp "$ROOT_DIR/LICENSE" "$ROOT_DIR/README.md" "$TMP/rxplain/"

ARCHIVE="$TMP/rxplain-$TAG-x86_64-unknown-linux-gnu.tar.gz"
tar -C "$TMP" -czf "$ARCHIVE" rxplain

if gh release view "$TAG" >/dev/null 2>&1; then
    gh release upload "$TAG" "$ARCHIVE" || true
else
    gh release create "$TAG" \
        --title "rxplain $TAG" \
        --notes "See CHANGELOG.md for details." \
        "$ARCHIVE"
fi

echo
echo "Uploaded: $ARCHIVE"
