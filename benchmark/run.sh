#!/usr/bin/env bash

set -u

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CASES_DIR="$ROOT_DIR/benchmark/cases"
RXPLAIN="$ROOT_DIR/target/debug/rxplain"
TMP_DIR="$(mktemp -d)"

cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

if [[ ! -x "$RXPLAIN" ]]; then
    echo "Rxplain binary not found."
    echo "Run: cargo build"
    exit 1
fi

TOTAL=0
DETECTION_PASSED=0
EXPLANATION_PASSED=0
CONCEPT_PASSED=0
FIX_CASES=0
FIX_DETECTED=0
FIX_VERIFIED=0

echo "======================================"
echo "        Rxplain Benchmark"
echo "======================================"
echo

for case_dir in "$CASES_DIR"/*; do
    [[ -d "$case_dir" ]] || continue

    ERROR_CODE="$(basename "$case_dir")"
    TOTAL=$((TOTAL + 1))

    echo "--------------------------------------"
    echo "Case: $ERROR_CODE"

    OUTPUT="$("$RXPLAIN" "$case_dir" 2>&1 || true)"

    # 1. Error detection
    if echo "$OUTPUT" | grep -q "ERROR $ERROR_CODE"; then
        echo "  Detection     ✓"
        DETECTION_PASSED=$((DETECTION_PASSED + 1))
    else
        echo "  Detection     ✗"
    fi

    # 2. Explanation
    if echo "$OUTPUT" | grep -q "Fix classification"; then
        echo "  Explanation   ✓"
        EXPLANATION_PASSED=$((EXPLANATION_PASSED + 1))
    else
        echo "  Explanation   ✗"
    fi

    # 3. Concept coverage
    if echo "$OUTPUT" | grep -q "Concept:"; then
        echo "  Concept       ✓"
        CONCEPT_PASSED=$((CONCEPT_PASSED + 1))
    else
        echo "  Concept       —"
    fi

    # 4. Machine-applicable fix
    if echo "$OUTPUT" | grep -q "MachineApplicable"; then
        echo "  Safe fix      ✓"
        FIX_CASES=$((FIX_CASES + 1))
        FIX_DETECTED=$((FIX_DETECTED + 1))
    else
        echo "  Safe fix      —"
    fi

    # 5. Auto-fix verification (run on a copy to avoid mutating the fixture)
    case_copy="$TMP_DIR/$ERROR_CODE"
    cp -r "$case_dir" "$case_copy"
    rm -rf "$case_copy/target"

    FIX_OUTPUT="$("$RXPLAIN" --fix "$case_copy" 2>&1 || true)"

    if echo "$FIX_OUTPUT" | grep -q "Verified repair"; then
        echo "  Verified fix  ✓"
        FIX_VERIFIED=$((FIX_VERIFIED + 1))
    elif echo "$FIX_OUTPUT" | grep -q "human judgment"; then
        echo "  Verified fix  — (no safe fix)"
    else
        echo "  Verified fix  ✗"
    fi

    echo
done

echo "======================================"
echo "Results"
echo "======================================"
echo
echo "Cases tested:       $TOTAL"
echo "Detection:          $DETECTION_PASSED / $TOTAL"
echo "Explanation:        $EXPLANATION_PASSED / $TOTAL"
echo "Concept coverage:   $CONCEPT_PASSED / $TOTAL"
echo "Machine-safe fixes: $FIX_DETECTED / $FIX_CASES"
echo "Verified repairs:   $FIX_VERIFIED / $FIX_CASES"
echo

if [[ "$DETECTION_PASSED" -eq "$TOTAL" &&
      "$EXPLANATION_PASSED" -eq "$TOTAL" ]]; then
    echo "✓ Benchmark passed"
    exit 0
else
    echo "✗ Benchmark failed"
    exit 1
fi