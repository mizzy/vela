#!/usr/bin/env bash
# tests/cli_test.sh — CLI end-to-end smoke test
set -euo pipefail

VELA="cargo run --quiet --"
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# Generate test WASM file
if command -v wat2wasm &>/dev/null; then
    cat > "$TMPDIR/test.wat" <<'WAT'
(component
  (core module $m
    (func (export "f") (result i32) i32.const 1)
    (func (result i32) i32.const 2)
  )
  (core instance $i (instantiate $m))
  (func (export "f") (result u32) (canon lift (core func $i "f")))
)
WAT
    wat2wasm --enable-all "$TMPDIR/test.wat" -o "$TMPDIR/test.wasm"
else
    echo "SKIP: wat2wasm not available"
    exit 0
fi

# Test optimize
$VELA optimize "$TMPDIR/test.wasm" -o "$TMPDIR/out.wasm"
echo "PASS: optimize produced output"

# Test info
$VELA info "$TMPDIR/test.wasm"
echo "PASS: info ran successfully"

# Verify output is not larger
ORIG=$(wc -c < "$TMPDIR/test.wasm" | tr -d ' ')
OPT=$(wc -c < "$TMPDIR/out.wasm" | tr -d ' ')
echo "Original: ${ORIG}B, Optimized: ${OPT}B"

if [ "$OPT" -le "$ORIG" ]; then
    echo "PASS: output is not larger than input"
else
    echo "FAIL: output is larger"
    exit 1
fi

echo "All CLI tests passed!"
