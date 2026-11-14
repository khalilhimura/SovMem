# SovMem P5 — Operational Hardening Design Specification

**Date:** 2026-08-17
**Status:** Draft design (pre-implementation)
**Phase:** P5 — Hardening, sentry, trust instrumentation, dogfood
**Prerequisites:** P0-P4 delivered and merged to `main` at `c159b59`

---

## 1. Scope and explicit non-scope

### In scope (7 workstreams)

1. **`trust` command** — deterministic ledger arithmetic computing per-source metrics.
2. **`eval` as regression gate** — CI fails on >ε drop vs frozen P2 baseline.
3. **Nightly sentry** — cron pipeline: `verify --level 4` → `index` → `eval`, fails loud on chain break or regression.
4. **`audit --sample N`** — weekly; biased toward blindspot cells after trust ships.
5. **Fuzz frontmatter parser** (time-boxed); **10k-doc scale test** (rebuild < 60s).
6. **SKILL.md runbook** — propose protocol, confidence elicitation, skeptic stance for harness agents.
7. **Dogfood cutover** — sentry writes to staging; trust report generated weekly.

### Explicit non-scope

- No model calls or network calls from the binary (A6).
- No trust-gated auto-accept (A7 — trust allocates attention, never authority).
- No `unsafe`, no async.
- No new dependencies without explicit approval.
- No frozen P2 artifact changes.
- No policy mutation from the binary; `trust` output is advisory.
- No MCP trust tool (trust is CLI-only, advisory).
- No incremental indexing or vector search.
- No GUI/TUI.

---

## 2. Authority and trust boundaries

### Frozen axioms (do not reopen)

| Axiom | Implication for P5 |
|-------|-------------------|
| A2 — Agents propose; only human verdicts commit | Trust scoring, sentry, and audit are read-only or advisory. No tool in P5 can commit to canon. |
| A6 — Deterministic core, probabilistic periphery | Trust math is ledger arithmetic only. No model calls, no network calls. |
| A7 — Trust is measured, never assumed | Trust may reorder queue, tune quotas, route skeptic review — never accept. Small-N guard: N < 30 verdicts per cell → no recommendation. |
| D11 — Cost ratio ≥ 100:1 | At this ratio the optimal derived policy is escalate-always (A2 is a theorem). Confirmed in `policy.toml`: `bad_accept_cost = 100`, `human_verdict_cost = 1`. |

### P5-specific safety boundaries

- **`trust` command output is advisory JSON.** No side effects on disk beyond writing its report to stdout/stderr or an optional output file. It must never mutate the ledger, store, policy, or canon.
- **`audit --sample` is read-only.** It reads the store, computes statistics, and outputs JSON/text. It must never write to the ledger or canon.
- **Sentry is a shell/CI script, not a daemon.** It orchestrates existing CLI commands.
- **SKILL.md is harness-side documentation**, not a binary artifact. It does not change the sovmem binary.

---

## 3. Decision resolutions

### D11 — Cost ratio (confirmed)

The existing `profiles/audy-ssa0-p3-v2/policy.toml` already has:

```toml
bad_accept_cost = 100
human_verdict_cost = 1
```

Ratio = 100:1, meeting the D11 minimum. **No change needed.** The `trust` command reads these from the active `policy.toml` to compute the optimal implicit threshold.

### Q12 — Trust window

**Decision: Rolling last-K verdicts, default K = 50.**

Rationale:
- Rolling K avoids calendar-edge effects (leap days, holidays).
- K = 50 matches the `max_open_total` queue alarm and provides a stable denominator before the small-N guard lifts.
- Configurable via `--window K` flag on the `trust` command.
- Time-based windows can be added later if dogfood evidence shows calendar patterns matter.

### Q9 (confirm) — Skeptic default stance

Already documented: adversarial bug-finding ("assume the proposal is wrong; list what fails"). Codified in the policy as `evaluator_protocol = "adversarial-bug-finding-v1"`. No P5 change needed.

### Sentry architecture

**Decision: Shell script + cron, no cloud dependency.**

- Pipeline: `sovmem verify --level 4 --store <path>` → `sovmem index --store <path>` → `sovmem eval --store <path> ...` → `sovmem trust --store <path> --window 50`.
- Fail loud: non-zero exit on any step halts the pipeline and writes to stderr.
- Integration: `npm run sentry:sovmem` in `package.json`. Cron runs as the controlling user, not as a service.
- Notification: stderr output (already captured by cron's MAILTO or a wrapper script). No email library, no Slack webhook.

### `audit --sample` spec

- **Sample size:** N = 5 (default), configurable via `--sample N`. Weekly.
- **Bias:** Once `trust` ships, sample is weighted toward blindspot cells (high confidence + low acceptance / high FP).
- **Pre-trust:** Uniform random sample from accepted documents.
- **Output format:** JSON lines to stdout: `{ "doc_id": "...", "current_hash": "...", "reason": "...", "blindspot_weight": 0.0 }`.

### Fuzz and scale test time-boxing

- **Fuzz frontmatter parser:** 1 session. Random mutation of valid Markdown frontmatter → assert graceful error (no panic, no undefined behavior). Use `quickcheck` or hand-rolled property testing. If fuzz exceeds 1 session, stop.
- **10k-doc scale test:** 1 session. Generate 10,000 synthetic documents with valid frontmatter, build bundle, verify at Level 3, build index, run eval. Assert rebuild < 60s.

### Dogfood sources

- Initial sources: Hermes and daily-court sentry agents write to the staging queue.
- Verdicts/week counted from sentry logs.
- First trust report generated after ≥30 verdicts in a single source cell (small-N guard).

---

## 4. `trust` command — detailed design

### 4.1 CLI surface

```text
sovmem trust
  --profile <path>           Required. Profile YAML.
  --store <path>             Required. Store root.
  [--window K]               Optional. Rolling window size. Default: 50.
  [--focus <source_id>]      Optional. Narrow to one source.
  [--output <path>]          Optional. Write JSON report to file instead of stdout.
  [--format text|json]       Default: json.
```

### 4.2 Inputs

| Input | Source | Notes |
|-------|--------|-------|
| Verdict events | `ledger/verdicts.jsonl` | Read via `read_ledger()` |
| Active policy | `policy.toml` | `bad_accept_cost`, `human_verdict_cost` |
| Rolling window K | CLI arg | Default 50 |

### 4.3 Per-source metrics

For each unique `source_id` in the ledger (within the rolling window):

| Metric | Formula | Description |
|--------|---------|-------------|
| `proposals` | Count of verdict events | Total verdicts involving this source's proposals |
| `accepted` | Count of ACCEPT | Number accepted |
| `rejected` | Count of REJECT | Number rejected |
| `rewritten` | Count of REWRITE | Number rewritten |
| `tp` | Count of TP | True positives (source proposed, human agreed) |
| `fp` | Count of FP | False positives (source proposed, human disagreed) |
| `accept_rate` | accepted / proposals | Empirical acceptance rate |
| `rewrite_rate` | rewritten / proposals | REWRITE fraction |
| `tp_rate` | tp / (tp + fp) | Precision of proposals |
| `fp_rate` | fp / (tp + fp) | False discovery rate |
| `mean_confidence` | Mean of proposal confidence values | Average stated pre-draft confidence |
| `calibration_gap` | mean_confidence - accept_rate | Over/under-confidence |
| `optimal_implied_threshold` | bad_accept_cost / (bad_accept_cost + human_verdict_cost) | From D11: 100/101 ≈ 0.99 |
| `implicit_threshold` | The accept_rate itself | The source's de facto bar |
| `threshold_gap` | optimal_implied_threshold - accept_rate | How far from optimal |

### 4.4 Blindspot report

Blindspots are cells where a source has:
- High stated confidence (≥ 0.8)
- Low acceptance rate (≤ 0.3) OR high FP rate (≥ 0.5)
- N ≥ 30 verdicts in the cell

**Cells are (source_id × layer × type).** Output:

```json
{
  "blindspots": [
    {
      "source_id": "src:agent:hermes",
      "layer": "analysis",
      "type": "concept",
      "verdicts": 35,
      "mean_confidence": 0.85,
      "accept_rate": 0.20,
      "fp_rate": 0.60,
      "blindspot": true
    }
  ]
}
```

### 4.5 Small-N guard

When `verdicts < 30` for a (source_id, layer, type) cell, the trust report:
- Sets `recommendation` to `null`
- Sets `small_n` to `true`
- Includes the raw counts so the human can interpret

### 4.6 Quota recommendations

When a source's acceptance rate is below the optimal implied threshold (0.99):
- No recommendation change (they're already undervaluing their proposals).
- When acceptance rate significantly exceeds 0.99 (empirically impossible with human judges, but preserved for correctness): recommend source quota reduction because the source is over-submitting noise.

### 4.7 Module location

New module: `sovmem/src/profile/trust.rs` (pub mod trust in profile/mod.rs).

Exports:
```rust
pub struct TrustReport { /* per-source and blindspot data */ }
pub fn compute_trust(ledger: &LedgerView, policy: &Policy, window: usize) -> TrustReport
pub fn render_trust_text(report: &TrustReport) -> String
pub fn render_trust_json(report: &TrustReport) -> Value
```

### 4.8 Test strategy (fixture-first)

| Test | Layer | What it proves |
|------|-------|----------------|
| Empty ledger → empty report | Unit | Trust handles no data |
| Single source, single verdict → per-source metrics | Unit | Basic arithmetic |
| Multiple sources → independent metrics per source | Unit | Source isolation |
| Confidence + acceptance → calibration gap | Unit | Gap arithmetic |
| Small-N guard under 30 verdicts | Unit | Guard works |
| Blindspot detection | Unit | Cell analysis |
| Cost ratio → optimal threshold | Unit | D11 derived value |
| Rolling window truncation | Unit | Window boundary |
| Policy cost ratio changes propagate | Unit | Policy read |
| Red-team: forged high-confidence low-acceptance | Red | No auto-accept |
| Red-team: trust output attempts to write canon | Red | Read-only enforced |
| Combined with ProvisionedP3Fixture | Integration | Real ledger format |

---

## 5. `eval` as regression gate

### 5.1 Current state

The `eval` command exists in P2 and produces `baseline.json`. The P2 frozen baseline is at `c3fdeb20...` with 23/25 hits.

### 5.2 P5 change

Modify the CI workflow (`.github/workflows/sovmem-audit.yml`) to:
1. Add a step that runs `sovmem eval --store <path>` on the store's current index.
2. Compare `queries_with_hit` against the frozen baseline value (23).
3. Fail if `queries_with_hit < 23 - ε` where ε = 1 (a single query regression is acceptable; 2+ is a failure).

The ε tolerance prevents flaky failures from binding changes while still catching meaningful regressions.

### 5.3 Implementation

The `sovmem eval` command already exists and produces a JSON output. Adding regression-gate logic can be done:
- In the JS audit script (`scripts/lib/sovmem-build-audit.mjs`) by adding a cross-check between the store's current eval output and the frozen baseline.
- OR as a separate `npm run regression:sovmem` script.

**Decision: Add regression check to `npm run audit:sovmem:check`.** The audit script already reads `baseline.json`; it can optionally accept a `--store` flag to run eval and compare.

---

## 6. Nightly sentry

### 6.1 Pipeline

```bash
#!/usr/bin/env bash
# scripts/sovmem-sentry.sh — nightly sentry pipeline
set -euo pipefail

STORE="${1:-var/sovmem-store}"
PROFILE="${2:-profiles/audy-ssa0-p3-v2/profile.yaml}"

echo "=== sentry: verify --level 4 ==="
sovmem verify --level 4 --store "$STORE"
echo "=== sentry: verify passed ==="

echo "=== sentry: index ==="
sovmem index --store "$STORE"
echo "=== sentry: index done ==="

echo "=== sentry: eval ==="
sovmem eval --store "$STORE" \
  --golden evals/audy-sovmem-v0/golden.jsonl \
  --thresholds evals/audy-sovmem-v0/thresholds.json \
  --baseline evals/audy-sovmem-v0/baseline.json

echo "=== sentry: trust report ==="
sovmem trust --store "$STORE" --profile "$PROFILE" --format text
```

### 6.2 Integration

- Added as `npm run sentry:sovmem` script in `package.json`.
- Cron: `0 2 * * * cd /path/to/repo && npm run sentry:sovmem 2>&1 | logger -t sovmem-sentry`
- Fail-loud: `set -euo pipefail` means any non-zero exit halts the pipeline.

### 6.3 Scope

- Pure shell script, no cloud dependency.
- No daemon, no PM2, no systemd unit (yet) — cron is sufficient for v0.3.

---

## 7. `audit --sample` command

### 7.1 CLI surface

```text
sovmem audit --sample N
  --store <path>    Required. Store root.
  [--sample 5]     Number of documents to sample. Default: 5.
  [--format text|json]   Default: json.
```

### 7.2 Behavior

1. Read the current canonical generation's manifest.
2. Sample N documents from the canonical bundle.
3. For each sampled document, emit its canonical ID, current hash, and path.
4. If a trust report exists (from `sovmem trust --output`), weight sampling toward blindspot cells using the `blindspot_weight` field.

### 7.3 Output

```json
{
  "schema_version": "sovmem-audit-sample-v1",
  "store_id": "...",
  "sample_date": "2026-08-17",
  "sample_size": 5,
  "samples": [
    {
      "doc_id": "ssa-0/01-Soul/Audy%20Himura.md",
      "current_hash": "abc123...",
      "path": "01-Soul/Audy Himura.md",
      "blindspot_weight": 0.0
    }
  ]
}
```

### 7.4 Module location

Add to existing `sovmem/src/profile/stats.rs` or a new `sovmem/src/profile/audit.rs` module.

---

## 8. Fuzz and scale tests

### 8.1 Fuzz frontmatter parser

- **Scope:** `parse_source_document()` and `validate_document()` functions in the existing frontmatter/validate modules.
- **Method:** Generate random byte sequences and valid YAML mutations; pass them to parse functions; assert no panic, assert graceful error return (not crash).
- **Time-box:** 1 session. If fuzzing framework setup exceeds 30 minutes, fall back to hand-written property tests.
- **Location:** `sovmem/tests/fuzz_frontmatter.rs`

### 8.2 10k-doc scale test

- **Scope:** Bundle verification and index rebuild at scale.
- **Method:** Generate 10,000 synthetic documents (valid frontmatter, random bodies). Build bundle, `verify --level 3`, build Tantivy index, run `eval`. Assert total time < 60s.
- **Location:** `sovmem/tests/scale_10k.rs`
- **Note:** This test is inherently slow and must be excluded from default `npm test`. Add a separate `npm run test:sovmem:scale` script.

---

## 9. SKILL.md runbook

### 9.1 Format

Standard Markdown in `docs/sovmem-skill.md` (or `SKILL.md` in the repo root). Sections:

1. **Propose Protocol** — step-by-step MCP flow for an agent to query, read, propose.
2. **Confidence Elicitation** — how agents should state pre-draft confidence (0–1 scale, before writing the candidate).
3. **Skeptic Stance** — default adversarial bug-finding evaluation posture.
4. **Verdict Loop** — what happens after proposal: human seals intake, human issues verdict, index rebuilds.
5. **Trust Report** — how to read and apply the trust output (advisory only, no self-modification).

### 9.2 Audience

- Claude Code agents (the primary harness during P4/P5)
- OpenClaw/Hermes sentry agents (writing to staging)
- Future harness agents (the SKILL.md is the binding contract)

---

## 10. Dogfood cutover

### 10.1 Phases

| Phase | Duration | Activity |
|-------|----------|---------|
| Setup | Day 1 | Create staging store (safe clone). Wire sentry cron to write to it. |
| Ramp | Days 1–7 | Agents queue proposals. Human issues verdicts daily. |
| Measure | Day 7+ | Count verdicts/week. Generate first trust report. |
| Sustained | Days 8–14 | 7 consecutive nights sentry green. ≥ 20 verdicts/week. |

### 10.2 Store

- Use an approved safe clone of the Audy Vault (same P3 safe-clone infrastructure).
- Separate staging store from the verified read-only P2 base.

### 10.3 Metrics

- Verdicts/week (target ≥ 20)
- Sentry green nights (target 7 consecutive)
- Trust report generation (even if all cells under small-N guard)

---

## 11. Module boundaries

```
sovmem/src/
├── cli.rs                  # +Trust, +AuditSample commands
├── profile/
│   ├── mod.rs              # +pub mod trust; +pub mod audit;
│   ├── trust.rs            # NEW — trust computation and report
│   ├── audit.rs            # NEW (optional) — audit sampling
│   ├── ledger.rs           # Unchanged (already provides read_ledger)
│   ├── policy.rs           # Unchanged (already provides Policy with cost fields)
│   └── stats.rs            # May grow audit sample output
```

No new files outside `sovmem/src/profile/`. Sentry and runbook are script-level artifacts.

---

## 12. Dependency policy

| Dependency | Status | Notes |
|-----------|--------|-------|
| serde_json | Already pinned | Trust and audit output |
| serde | Already pinned | Trust and audit structs |
| blake3 | Already pinned | Any hashing needed |
| clap | Already pinned | New CLI args |
| New crate for TOML parsing | **Rejected** | Policy is already in TOML but parsed by the manual parser in `policy.rs` — trust reads policy via `load_policy()` |
| quickcheck / proptest | **Deferred** | If fuzz frontmatter exceeds time-box, use hand-rolled instead |

---

## 13. Test strategy overview

| Layer | What | Count (est.) |
|-------|------|-------------|
| Unit — trust arithmetic | Deterministic ledger math, empty/edge cases | 15 |
| Unit — blindspot detection | Cell analysis, small-N guard | 5 |
| Unit — audit sampling | Sampling logic | 5 |
| Integration — store + trust | ProvisionedP3Fixture + trust | 3 |
| Integration — sentry script | Shell pipeline simulation | 1 |
| Red-team — trust authority | No canon write, no auto-accept | 3 |
| Fuzz — frontmatter | Random mutations | 1 |
| Scale — 10k docs | Rebuild < 60s | 1 |

---

## 14. Frozen artifact protection

- **P2 baseline (`baseline.json`, `golden.jsonl`, `thresholds.json`):** unmodified.
- **P2 index config hash (`95a2237b...`):** preserved. If the eval regression gate detects a change, it must be a genuine regression, not a tuning.
- **P3 safe-clone proof:** preserved. P5 dogfood uses separate safe clones.
- **P4 MCP tools:** preserved. No trust or audit tool added to MCP.

---

## 15. Build-state transition

| Phase | Weight | Completion | Cumulative |
|-------|--------|------------|-----------|
| P0 | 1 | 100% | |
| P1 | 2 | 100% | |
| P2 | 2 | 100% | |
| P3 | 3.5 | 100% | |
| P4 | 2 | 100% | → ~78% |
| P5 | 3 | 0% → 100% | → **100%** |

After P5 gate:
- `docs/sovmem-build-state.json`: P5 = 100%, overall = 100%.
- Tag `v0.3` on the accepted commit.

---

## 16. Tripwires and stop conditions

| Condition | Action |
|-----------|--------|
| Trust score, confidence, or evaluator output can commit to canon | **Stop** — violates A7 |
| Model call or network call added to binary | **Stop** — violates A6 |
| `unsafe` or async introduced | **Stop** — ban list |
| Dependency added without approval | **Stop** — ban list |
| Frozen P2 artifact changed to pass regression gate | **Stop** — contract violation |
| Cost ratio set such that auto-accept is optimal (ratio < 100:1) | **Stop** — undermines A2 |
| Trust recommendations presented as authoritative rather than advisory | **Stop** — violates A7 |
| Fuzz or scale test exceeds 1 session | **Stop and reassess** |
| Sentry pipeline in `npm test` (too slow) | **Stop** — sentry belongs in cron, not CI |

---

## 17. Open questions resolved in this design

| # | Question | Resolution |
|---|----------|------------|
| Q12 | Trust window? | Rolling last-K, default K=50 |
| D11 | Cost ratio confirmed? | 100:1, already set in policy.toml |
| Sentry architecture | Cron runner? | Shell script, `npm run sentry:sovmem`, cron |
| `audit --sample` | Format? | JSON lines, N=5 default, biased after trust |
| SKILL.md | Location? | `docs/sovmem-skill.md` |
| Dogfood sources | Which agents? | Hermes + daily-court sentry |
