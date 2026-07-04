#!/usr/bin/env bash
# Extension-contract §5 for the buying↔inventory receipt seam: prove the cross-module ACL/consumer
# wiring survives a regeneration of BOTH modules. Snapshots the seam files, regenerates buying AND
# inventory with --force, asserts byte-identical, and re-runs the end-to-end seam test green.
# Usage: DATABASE_URL=... bash scripts/receipt_seam_roundtrip.sh
set -euo pipefail
cd "$(dirname "$0")/.."

BUY_FILES=(
  src/application/service/buying_write_service.rs
  src/application/service/buying_events.rs
  tests/receipt_seam.rs
)
INV_FILES=(
  ../backbone-inventory/src/application/service/inventory_intake.rs
  ../backbone-inventory/src/application/service/inventory_write_service.rs
)

echo "→ snapshot seam consumer/ACL files (both modules)"
before=$(shasum -a 256 "${BUY_FILES[@]}" "${INV_FILES[@]}")

echo "→ regenerate BOTH modules (§5) — inventory then buying"
( cd ../backbone-inventory && metaphor schema schema generate --force >/dev/null )
metaphor schema schema generate --force >/dev/null

echo "→ verify every seam file is byte-identical after regen"
after=$(shasum -a 256 "${BUY_FILES[@]}" "${INV_FILES[@]}")
if [ "$before" != "$after" ]; then
  echo "✗ FAIL: a seam file changed during regen"; diff <(echo "$before") <(echo "$after") || true; exit 1
fi
echo "  ✓ all ${#BUY_FILES[@]}+${#INV_FILES[@]} seam files unchanged"

echo "→ re-run the end-to-end receipt seam post-regen"
cargo test --test receipt_seam -- --test-threads=1 >/dev/null
echo "  ✓ buying→inventory→accounting→buying seam still green after regenerating both modules"
echo "✓ §5 round-trip proven for the receipt seam."
