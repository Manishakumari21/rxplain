#!/usr/bin/env bash
#
# Rxplain — scripted demo
#
# Runs a guided tour of the tool against prepared example projects:
#   1. A mismatched-types error  (E0308)
#   2. A borrow conflict         (E0502)
#   3. A moved value             (E0382)
#   4. JSON output mode
#   5. Automatic safe fix        (--fix) with verification
#
# Usage:
#   bash demo.sh
#

set -u

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RXPLAIN="$ROOT_DIR/target/debug/rxplain"
DEMO_CASES="$ROOT_DIR/demo/cases"

BOLD='\033[1m'
DIM='\033[2m'
CYAN='\033[36m'
GREEN='\033[32m'
RESET='\033[0m'

section() {
    printf "\n${BOLD}${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}\n"
    printf "${BOLD}${CYAN}  %s${RESET}\n" "$1"
    printf "${BOLD}${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}\n"
}

step() {
    printf "${BOLD}\n▶ %s${RESET}\n" "$1"
}

if [[ ! -x "$RXPLAIN" ]]; then
    echo "Building rxplain..."
    (cd "$ROOT_DIR" && cargo build --quiet) || { echo "Build failed"; exit 1; }
fi

section "Rxplain — Guided Demo"
printf "${DIM}A deterministic, offline explainer for Rust compiler errors.${RESET}\n"

section "1 · A mismatched-types error (E0308)"
step "rxplain demo/cases/E0308"
"$RXPLAIN" "$DEMO_CASES/E0308"

section "2 · A borrow conflict (E0502)"
step "rxplain demo/cases/E0502"
"$RXPLAIN" "$DEMO_CASES/E0502"

section "3 · A moved value (E0382)"
step "rxplain demo/cases/E0382"
"$RXPLAIN" "$DEMO_CASES/E0382"

section "4 · Machine-readable JSON output"
step "rxplain demo/cases/E0308 --json"
"$RXPLAIN" "$DEMO_CASES/E0308" --json

section "5 · Automatic safe fix with verification"
step "rxplain demo/cases/E0384 --fix   (applies the compiler's 'mut' suggestion)"
printf "${DIM}Running on a copy so the fixture is preserved...${RESET}\n"

WORK_DIR="$(mktemp -d)"
cp -r "$DEMO_CASES/E0384" "$WORK_DIR/project"
"$RXPLAIN" "$WORK_DIR/project" --fix
rm -rf "$WORK_DIR"

section "Demo complete"
printf "${GREEN}✓ All demo scenarios executed.${RESET}\n"
printf "\nFixtures live under demo/cases/. Run the full evaluation suite with:\n"
printf "  ${BOLD}./benchmark/run.sh${RESET}\n"
