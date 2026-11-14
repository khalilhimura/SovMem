#!/usr/bin/env bash
# scripts/sovmem-sentry.sh — nightly sentry pipeline
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STORE="${1:-var/sovmem-store}"
PROFILE="${2:-profiles/audy-ssa0-p3-v2/profile.yaml}"

cd "$ROOT"

echo "=== sentry: verify --level 4 ==="
cargo run --quiet --manifest-path sovmem/Cargo.toml -- verify --profile "$PROFILE" --level 4 --store "$STORE"
echo "=== sentry: verify passed ==="

echo "=== sentry: index ==="
cargo run --quiet --manifest-path sovmem/Cargo.toml -- index --profile "$PROFILE" --store "$STORE"
echo "=== sentry: index done ==="

echo "=== sentry: eval ==="
cargo run --quiet --manifest-path sovmem/Cargo.toml -- eval \
  --profile "$PROFILE" \
  --bundle "$STORE/seed/bundle" \
  --index "$STORE/derived/indexes/seed" \
  --golden evals/audy-sovmem-v0/golden.jsonl \
  --thresholds evals/audy-sovmem-v0/thresholds.json \
  --baseline evals/audy-sovmem-v0/baseline.json

echo "=== sentry: trust report ==="
cargo run --quiet --manifest-path sovmem/Cargo.toml -- trust \
  --profile "$PROFILE" --store "$STORE" --format text

echo "=== sentry: all checks passed ==="
