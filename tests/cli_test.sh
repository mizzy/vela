#!/usr/bin/env bash
# tests/cli_test.sh — CLI end-to-end smoke test
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VELA="cargo run --quiet --"
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# Build the test WASM generator helper using cargo
if [ -f "$SCRIPT_DIR/Cargo.toml" ]; then
    cargo build --manifest-path "$SCRIPT_DIR/Cargo.toml" --quiet 2>/dev/null && \
    GEN_WASM="$SCRIPT_DIR/target/debug/gen_test_wasm" || \
    GEN_WASM=""
else
    GEN_WASM=""
fi

# Generate test WASM file
if [ -x "${GEN_WASM:-}" ]; then
    "$GEN_WASM" "$TMPDIR/test.wasm"
elif command -v wat2wasm &>/dev/null; then
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
    echo "SKIP: gen_test_wasm helper and wat2wasm not available"
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
