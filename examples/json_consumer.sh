#!/usr/bin/env bash
# rxplain --json consumer example.
#
# Demonstrates how an editor, LSP server, or CI tool can consume rxplain's
# machine-readable JSON report. This script:
#   1. Runs `rxplain --json` on a broken project
#   2. Uses `jq` to project just the fields a diagnostics UI needs
#   3. Prints a compact, editor-friendly summary
#
# Usage:
#   bash examples/json_consumer.sh [project-dir]

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RXPLAIN="$ROOT_DIR/target/debug/rxplain"
PROJECT="${1:-$ROOT_DIR/examples/broken_project}"

if [[ ! -x "$RXPLAIN" ]]; then
    echo "rxplain not built; run: cargo build"
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "This example requires jq: https://stedolan.github.io/jq/"
    exit 1
fi

echo "Running rxplain --json on: $PROJECT"
echo

JSON="$("$RXPLAIN" "$PROJECT" --json 2>/dev/null)"

echo "Total diagnostics: $(echo "$JSON" | jq '.errors | length')"
echo

# Project each error down to the fields an editor diagnostics panel needs.
echo "$JSON" | jq -r '
  .errors[] |
  "--------------------------------------------------\n" +
  "[\(.code)] \(.message)\n" +
  "  at      \(.locations[0].file):\(.locations[0].line):\(.locations[0].column)\n" +
  "  concept \(.explanation.concept // "none")\n" +
  "  summary \(.explanation.summary)\n" +
  "  fix     \(.fix.kind) \(.fix.replacement // "—")\n"
'
