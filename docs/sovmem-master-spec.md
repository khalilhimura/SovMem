# SovMem Master Specification

**Date:** 2026-08-17 (P5 delivery + v0.3 tag)  
**Status:** Living consolidation of approved SovMem designs, plans, and audits  
**Owner:** Nous / Khalil Nooh  
**Normative parent:** [`docs/sovmem_v0.3_execution_plan.md`](sovmem_v0.3_execution_plan.md)  

> This document consolidates every approved SovMem specification, implementation plan, product PRD, and audit report into one navigable reference. It does **not** replace the v0.3 execution plan, which remains the normative development authority. Archived source documents are preserved in `docs/archive/` with full cross-references.

---

## How to use this document

| If you want to | Start here |
|---|---|
| Understand what SovMem is | [§1. Architecture overview](#1-architecture-overview) |
| See what has been built | [§2. Phase status](#2-phase-status) |
| Read the approved design for a specific phase | The phase subsection below |
| Find an archived plan or spec | [Appendix B: Archive map](#appendix-b-archive-map) |
| See all implementation plans and their scope | [Appendix A: Implementation plan index](#appendix-a-implementation-plan-index) |
| Read the build audit or gap analysis | [§9. Audits](#9-audits) |
| Understand non-SovMem context | [§10. Non-SovMem context](#10-non-sovmem-context) |

---

## 1. Architecture overview

SovMem (Sovereign Multi-Agent Memory) is the memory and judgment subsystem within the Nous operating system for Solo Systems Architects. Its governing principle:

> **Agents may read and propose. Only accountable human judgment may commit canon.**

SovMem v0.3 is divided into five phases, all delivered and tagged as [`v0.3`](https://github.com/khalilhimura/nous/releases/tag/v0.3):

| Phase | Name | Status |
|-------|------|--------|
| P0 | Scaffold and ground truth | ✅ Closed |
| P1 | Bundle substrate | ✅ Closed |
| P2 | Derived index and query | ✅ Closed |
| P3 | Verified write-back loop | ✅ Closed (v1 + v2 amendments) |
| P4 | MCP adapter | ✅ Delivered |
| P5 | Operational hardening | ✅ **Delivered — v0.3 tagged** |

### Key architectural invariants

- The Markdown **bundle** is authoritative; indexes are disposable and regenerable.
- **Audy Vault** is the canonical external context source (pinned at Git commit `43824cfd9caed7b260aa442128fd1130ddb6e2ff`).
- Generated bundles, manifests, reports, indexes, and corpus artifacts are **derived** — never canonical.
- **Provenance** is required and uses the enum `tier1 | secondary | synthesis | unknown`.
- The binary performs **no model calls, no network calls**.
- `unsafe` is prohibited. No async runtime used anywhere in the binary (the MCP adapter uses synchronous I/O on a dedicated thread).
- One Rust crate (`sovmem`). Rust 1.84, edition 2021.
- Private repository with manual pull-request merge discipline.

For complete detail, see [`docs/sovmem_v0.3_execution_plan.md`](sovmem_v0.3_execution_plan.md).

---

## 2. Phase status

```
P0: ████████████████████████████████ 100%  ✅ Gate closed
P1: ████████████████████████████████ 100%  ✅ Gate closed
P2: ████████████████████████████████ 100%  ✅ Gate closed
P3: ████████████████████████████████ ~63% ✅ Gate closed
P4: ████████████████████████████████ 100%  ✅ Delivered
P5: ████████████████████████████████ 100%  ✅ **Delivered — v0.3 tagged**
```

> The P3 percentage reflects the build audit's last measured state. The gate is closed on the v2 intake-authority-hardened implementation.

---

## 3. Phase 0: Foundation (P0)

**Status:** ✅ Gate closed  
**Key docs archived:** [`docs/archive/specs/2026-07-10-nous-root-index-design.md`](archive/specs/2026-07-10-nous-root-index-design.md), [`docs/archive/specs/2026-07-10-audy-sovmem-profile-v0-design.md`](archive/specs/2026-07-10-audy-sovmem-profile-v0-design.md)

### Scope

- Establish the repository-wide root index (`/index.md`) as the entry point for humans and agents.
- Define the SSA context architecture and the Audy-SovMem reference profile v0.
- Create the profile model: SSA identity, familiar identity, canonical context plane, judgment ledger, trust history, policy, runtime memory, and harness adapters.

### Key decisions

| Decision | Value |
|----------|-------|
| Corpus ground truth | 62 documents: 8 canonical-core + 54 accepted research |
| Golden ground truth | 25 human-approved queries, each with one relevant document |
| Canonical source | Audy Vault at `43824cfd9caed7b260aa442128fd1130ddb6e2ff` |
| Verdict grammar | Internal: `ACCEPT`, `REJECT`, `REWRITE`, `TP`, `FP`. Public: `ACCEPT`, `REJECT`, `REWRITE` |
| Provenance | `tier1`, `secondary`, `synthesis`, `unknown` |
| Crate layout | Single Rust crate (`sovmem`) |
| Repository | Private, manual PR merge discipline |

---

## 4. Phase 1: Retrieval and queries (P1)

**Status:** ✅ Gate closed  
**Key docs archived:** [`docs/archive/specs/2026-07-12-audy-sovmem-retrieval-corpus-design.md`](archive/specs/2026-07-12-audy-sovmem-retrieval-corpus-design.md), [`docs/archive/specs/2026-07-12-audy-sovmem-retrieval-corpus-v1-design.md`](archive/specs/2026-07-12-audy-sovmem-retrieval-corpus-v1-design.md), [`docs/archive/specs/2026-07-12-audy-golden-query-draft-design.md`](archive/specs/2026-07-12-audy-golden-query-draft-design.md)

### Scope

- Create the curated, reproducible Audy SovMem retrieval corpus (v0 → v1).
- Produce 25 human-authored golden queries with verified relevant document IDs.
- Define acceptance gates: minimum 53 total documents, 10/15/10/10 theme distribution.

### Key decisions

- Active corpus schema: `audy-retrieval-corpus-v1` (v0 preserved but inactive).
- Source commit: pinned at `43824cfd9caed7b260aa442128fd1130ddb6e2ff`.
- Golden queries are human-only; model-generated drafts require explicit human approval before promotion.
- Corpus manifest is the complete source-selection contract; globs are discovery aids only.

### Corpus architecture

Thematic quotas with exact file paths:

| Theme | Candidates | Minimum accepted |
|-------|-----------|-----------------|
| Canonical core | 8 (required) | 8 |
| `tfis-ssa` | 12 | 10 |
| `sovereign-context` | 15 | 15 |
| `agent-operations` | 14 | 10 |
| `governance-evidence` | 14 | 10 |

---

## 5. Phase 2: Build audit and gate closure (P2)

**Status:** ✅ Gate closed  
**Key docs archived:** [`docs/archive/specs/2026-07-12-sovmem-build-audit-design.md`](archive/specs/2026-07-12-sovmem-build-audit-design.md), [`docs/archive/specs/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds-design.md`](archive/specs/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds-design.md), [`docs/archive/specs/2026-07-13-sovmem-p2-derived-index-and-query-design.md`](archive/specs/2026-07-13-sovmem-p2-derived-index-and-query-design.md)

### Scope

- Build audit: deterministic Markdown report from reviewed structured state + source fingerprint.
- P0/P1 gate closure: verification levels 1–3, corruption coverage, threshold freeze.
- P2 derived index and query: Tantivy 0.24.2 + Petgraph 0.8.3 retrieval layer over a verified Level 3 bundle.

### Build audit artifacts

| Artifact | Path | Description |
|----------|------|-------------|
| Human-readable report | `docs/SOVMEM-BUILD-AUDIT.md` | Generated from state + source fingerprint |
| Reviewed state | `docs/sovmem-build-state.json` | Human-reviewed JSON input |
| Schema version | `sovmem-build-audit-v2` | Current frozen version |

### P2 retrieval details

- Tantivy schema fields: `id`, `title`, `body`, `layer`, `type`, `tags`, `provenance`.
- Full-rebuild only — no incremental update.
- Graph edges from typed frontmatter `links[]`.
- Q3 thresholds frozen: `k = 5`, macro P@5 ≥ 0.16, macro R@5 ≥ 0.80, ≥20/25 queries with a top-5 hit.
- P2 baseline: 23/25 top-five hits, P@5 0.1840, R@5 0.9200.

---

## 6. Phase 3: Verified write-back loop (P3)

**Status:** ✅ Gate closed (v1 + v2 amendments)  
**Key docs archived:** [`docs/archive/specs/2026-07-14-sovmem-p3-verified-write-back-loop-design.md`](archive/specs/2026-07-14-sovmem-p3-verified-write-back-loop-design.md), [`docs/archive/specs/2026-07-21-sovmem-p3-intake-authority-amendment-design.md`](archive/specs/2026-07-21-sovmem-p3-intake-authority-amendment-design.md), [`docs/archive/specs/2026-07-21-sovmem-p3-lock-capability-boundary-amendment.md`](archive/specs/2026-07-21-sovmem-p3-lock-capability-boundary-amendment.md)

### Overview

P3 adds the first reviewed write path to SovMem. It accepts immutable agent proposals, applies deterministic policy, requires an interactive human verdict, publishes verified canonical bytes for ACCEPT and REWRITE, records every human verdict in an append-only BLAKE3 chain, and completes a full P2 index rebuild before each applied verdict is committed.

### Authority model

P3 introduces a **dedicated versioned SovMem canonical lane** (`audy-sovmem-writeback-v1`) — a separate store from both Audy Vault and the frozen P2 export.

| Artifact | Authority | Mutable in P3 |
|----------|-----------|:---:|
| Audy Vault (pinned) | Canonical external context | No |
| Frozen Level 3 export | Canonical P2 evaluation input | No |
| P3 canonical generation | Canon for P3 queries + verdicts | Only through verdict |
| `policy.toml` | Current human-authored policy | Out-of-band replacement |
| Verdict ledger | Human judgment history | Append only |
| Tantivy/Petgraph index | Disposable derivative | Rebuildable |

### Intake authority amendment (P3 v2)

The original P3 design had an authority contradiction: a proposer could write, remove, or backdate the intake evidence that both quotas and Level 4 replay treated as authoritative.

**Resolution:** A separate, protected, append-only **intake authority** was introduced:

- `intake/events.jsonl` — prefix-preserving, BLAKE3-hash-chained intake history.
- `intake seal` — interactive CLI command (requires real `/dev/tty`, confirmation `SEAL INTAKE <id>`).
- The intake seal is **not** a verdict — it cannot commit canon.
- Only the human verdict ceremony can write canon.
- `propose` returns `submitted` (not `staged`); `intake seal` turns it into `STAGED` or `REJECTED`.

### Lock capability-boundary amendment

P3 uses a local path-and-capability boundary for `locks/`:

- `locks/` is outside every untrusted write root.
- Only the trusted P3 verdict/intake/recovery process may write to `locks/`.
- No-replace creation + canonical owner bytes for lease acquisition.
- Direct `rustix` dependency removed; standard-library lease protocol used.

### P3 scope (delivered)

- Immutable candidate Markdown with bound proposal metadata.
- Strict, versioned `policy.toml` and deterministic `PolicyEvaluator`.
- Proposal intake receipts, queue/list/status, quotas, deduplication, queue alarms.
- Five-verb human verdict CLI (`ACCEPT`, `REJECT`, `REWRITE`, `TP`, `FP`).
- Exact-byte ACCEPT and human-supplied exact-byte REWRITE.
- Deterministic Human Judgment documents.
- Copy-on-write canonical generations with integrity manifests.
- Full P2 index rebuild before each verdict commit.
- Append-only BLAKE3-chained verdict ledger.
- `verify --level 4` over store + ledger chain.
- Exclusive mutation locking, idempotency, crash detection, deterministic recovery.
- Safe-clone real-data proof.
- **Test infrastructure refactor** (Aug 2026): All 399 heavy P3 tests moved from lib modules to isolated integration test binaries. Tests no longer flaky under concurrent execution. See [`docs/archive/plans/p3-internal-test-refactor.md`](archive/plans/p3-internal-test-refactor.md).

### Explicit P3 non-scope

- No writes to Audy Vault or the frozen P2 export.
- No automatic synchronization or Git commit.
- No MCP transport, verdict tool, or write tool.
- No P4 adapter, P5 sentry/trust.
- No incremental indexing.
- No model calls, network calls, or new dependencies.

---

## 7. Phases 4–5: Delivered and remaining

**P4 Status:** ✅ Delivered  
**P5 Status:** ✅ **Delivered — v0.3 tagged at [`3e0817f`](https://github.com/khalilhimura/nous/commit/3e0817f)**

### P4 — MCP adapter

- MCP protocol revision `2026-07-28` over stdio using raw JSON-RPC 2.0 (no SDK dependency).
- Four tools: `memory_search`, `memory_get`, `memory_propose`, `memory_status`.
- Stateless protocol: no `initialize` handshake; version and capabilities in per-request `_meta`.
- Synchronous single-threaded server (no async, no `tokio`), no new dependencies.
- No MCP verdict, intake seal, canon write, or policy mutation authority.
- Feature-gated behind `#[cfg(feature = "mcp")]` (default-enabled).
- **Design spec:** [`docs/superpowers/specs/2026-07-28-sovmem-p4-mcp-adapter-design.md`](superpowers/specs/2026-07-28-sovmem-p4-mcp-adapter-design.md)
- **Implementation plan:** [`docs/superpowers/plans/2026-07-28-sovmem-p4-mcp-adapter.md`](superpowers/plans/2026-07-28-sovmem-p4-mcp-adapter.md)
- **Key modules:** `sovmem/src/mcp/` — `mod.rs` (server loop, dispatch), `tools.rs` (four tool handlers), `redact.rs` (diagnostic redaction)
- **Tests:** 8 layer-1 (unit), 5 layer-2 (stdio simulation), 13 layer-3 (P3 fixture integration)

### P5 — Operational hardening (delivered)

Trust command, trust routing, audit sampling, sentry, dogfood, runbook, scale tests, eval regression gate. Tagged as `v0.3`.

**Design spec:** [`docs/archive/specs/2026-08-17-sovmem-p5-operational-hardening-design.md`](archive/specs/2026-08-17-sovmem-p5-operational-hardening-design.md)
**Implementation plan:** [`docs/archive/plans/2026-08-17-sovmem-p5-operational-hardening.md`](archive/plans/2026-08-17-sovmem-p5-operational-hardening.md)

#### Delivered

- `sovmem trust` command — deterministic ledger arithmetic: per-source acceptance/REWRITE/TP/FP rates, calibration gap, optimal threshold (D11), small-N guard (N<30), quota recommendations. Rolling last-K window (default 50).
- 10 trust unit tests passing.
- CLI integration: `sovmem trust --profile --store [--window] [--focus] [--output] [--format]`.
- `sovmem audit --sample` CLI stub (full implementation deferred).
- Eval regression CI gate — baseline readability check with documented path to full comparison.
- Nightly sentry pipeline: `scripts/sovmem-sentry.sh` + `npm run sentry:sovmem`.
- 9 frontmatter fuzz test cases (no-panic).
- 10k-doc scale test (`#[ignore]`, excluded from default suite).
- `docs/sovmem-skill.md` — harness agent runbook (propose protocol, confidence elicitation, skeptic stance, verdict loop, trust report interpretation).
- Build-state updated: P5=100%, overall=100%, v0.3 tagged.

#### Pending follow-up

- Blindspot cell detection in `compute_trust()` (empty Vec stub).
- Full `audit --sample` implementation (currently returns diagnostic).
- Full eval regression: fresh `sovmem eval` vs. frozen baseline (requires Tantivy index in CI).
- Dogfood cutover live execution: safe clone + staging store + daily verdicts.

---

## 8. Learning product: SovMem: Zero to Hero

**Status:** ✅ Approved product requirements and design; implementation pending in separate repo (`sovmem-learn`)  
**Key docs archived:** [`docs/archive/specs/2026-07-21-sovmem-zero-to-hero-design.md`](archive/specs/2026-07-21-sovmem-zero-to-hero-design.md), [`docs/archive/product/2026-07-21-sovmem-zero-to-hero-prd.md`](archive/product/2026-07-21-sovmem-zero-to-hero-prd.md)

### Summary

A free, public, self-paced learning product (~3-4 hours) teaching SovMem from zero prerequisites through progressively deeper views:

- **General:** concepts, analogies, value, human decisions.
- **Developer:** tool-neutral integration contracts.
- **Technical:** architecture, invariants, authority, recovery, threat models.

### Key product decisions

- First release runs entirely in browser: no accounts, payments, analytics, or server-side data.
- Progress stored locally, exportable/importable via validated record.
- Deterministic browser simulations of the authority model.
- Phase-aware labeling: delivered vs. hardening vs. planned behavior.
- Course destination: `https://sovmem.thefutureissolo.com` (future: `https://course.thefutureissolo.com`).

---

## 9. Audits

### 9.1 Active build audit

**Archived:** [`docs/archive/audits/SOVMEM-BUILD-AUDIT.md`](archive/audits/SOVMEM-BUILD-AUDIT.md)

- Generated from `docs/sovmem-build-state.json` via `npm run audit:sovmem`.
- Shows weighted completion, per-phase status, evidence, blockers.
- Do not edit directly — managed by the generator.

### 9.2 P0–P2 independent gap analysis

**Archived:** [`docs/archive/audits/SOVMEM-P0-P2-INDEPENDENT-AUDIT.md`](archive/audits/SOVMEM-P0-P2-INDEPENDENT-AUDIT.md)

- Independent gap analysis and code quality audit (2026-07-27).
- Covers P0 through P2 implementation quality and coverage gaps.

---

## 10. Non-SovMem context

This section catalogs documents and subsystems in the Nous repository that are not part of SovMem but provide important context. They are preserved in the archive with cross-references.

### 10.1 TTF canary (Trust Threshold Framework)

**Archived:** [`docs/archive/ttf-canary/`](archive/ttf-canary/)

The Trust Threshold Framework is a **separate subsystem** from SovMem. It evaluates task-scoped runtime evidence to determine whether an action should be allowed, constrained, escalated, or denied.

| Field | Value |
|-------|-------|
| Status | Draft |
| Canonical document | `CANONICAL-SPEC.md` |
| Sensing primitive | LLM Canary |
| Core verbs | `allow`, `constrain`, `escalate`, `deny` |

**Relationship to SovMem:** TTF governs what an agent may do **now** (runtime behavior). SovMem preserves what humans judged, why they judged it, and how that judgment changes canon (memory). Neither subsystem grants the other silent authority. Runtime outcomes may become governed evidence; memory and precedent may provide context for future TTF decisions. An outcome-calibrated addendum exploring shared event and outcome-feedback models remains advisory.

**TTF document bundle (all Draft):**

| Document | Description |
|----------|-------------|
| `00-manifesto.md` | Core proposition: trust must be earned at runtime |
| `01-concept-evolution.md` | Evolution of the TTF concept |
| `02-rfc.md` | RFC for the TTF architecture |
| `03-technical-spec.md` | Technical specification |
| `04-prd.md` | Product requirements document (v0.1) |
| `05-ard.md` | Architecture requirements/decision record |
| `06-benchmark.md` | CanaryBench reference benchmark |
| `07-telemetry-and-scoring.md` | Telemetry and scoring contracts |
| `08-threat-model.md` | Threat model analysis |
| `09-implementation-roadmap.md` | Implementation roadmap |
| `10-research-sources.md` | Research sources |
| `11-apis-and-schemas.md` | API and schema specifications |
| `12-policy-and-trust-engine.md` | Policy and trust engine design |
| `13-observability-and-operations.md` | Observability and operations |
| `14-integration-guide.md` | Integration guide |
| `15-open-questions.md` | Open questions and decisions |

### 10.2 Nous root index

**Archived:** [`docs/archive/specs/2026-07-10-nous-root-index-design.md`](archive/specs/2026-07-10-nous-root-index-design.md)

The Nous root index (`/index.md`) is the repository-wide entry point that helps humans and agents understand what Nous is, which subsystems exist, and how they relate. It was the first approved design in this repository. It introduces SovMem and TTF-Canary as the two initial subsystems.

**Relationship to SovMem:** The root index **contains** SovMem as one of its registered subsystems (alongside TTF-Canary). It is the organizational shell; SovMem is one of the substantive subsystems within that shell.

### 10.3 Audy-SovMem reference profile

**Archived:** [`docs/archive/specs/2026-07-10-audy-sovmem-profile-v0-design.md`](archive/specs/2026-07-10-audy-sovmem-profile-v0-design.md)

Defines the reusable SSA context architecture and the metadata, authority, export, and validation contract for turning a selected snapshot of Audy's Vault into a generated SovMem bundle. Audy is the first concrete familiar profile running on Nous for Khalil Nooh (SSA-0).

**Relationship to SovMem:** The profile is the **interface contract** between the SSA context and SovMem's bundle generation. SovMem exports use this profile to produce deterministic, verified Markdown bundles from the pinned vault.

### 10.4 Source lineage (June 23 bundle)

**Location:** [`docs/spec/source-lineage-june-23-spec-bundle-verbatim.md`](spec/source-lineage-june-23-spec-bundle-verbatim.md) *(Kept in place — still the lineage authority)*

Recovery of the original six-file specification bundle produced 2026-06-23. The original files lived in an ephemeral session container (risk R8). This document preserves the recovered content with fidelity markers:

- `index.md` — FULL
- `04_workstream_plans.md` — ~FULL
- `03_PRD_mvp.md` — ~70%
- `05_post_mvp_enterprise.md` — ~60%
- `02_oss_stack_primitives.md` — ~50%
- `01_architecture_and_limits.md` — fragments only

---

## Appendix A: Implementation plan index

This appendix summarizes every implementation plan that was created to deliver SovMem, with its phase, scope, and key decisions. Full plans are archived in [`docs/archive/plans/`](archive/plans/).

| Plan | Phase | Scope summary |
|------|-------|---------------|
| [`2026-07-10-nous-root-index`](archive/plans/2026-07-10-nous-root-index.md) | P0 | Create `/index.md` as the repository-wide entry point with artifact registry |
| [`2026-07-10-audy-sovmem-canary-exporter`](archive/plans/2026-07-10-audy-sovmem-canary-exporter.md) | P0 | Build the canary exporter that produces deterministic, verified SovMem bundles |
| [`2026-07-12-audy-golden-query-draft`](archive/plans/2026-07-12-audy-golden-query-draft.md) | P1 | Generate the 25-query golden query draft for human review |
| [`2026-07-12-audy-sovmem-retrieval-corpus`](archive/plans/2026-07-12-audy-sovmem-retrieval-corpus.md) | P1 | Build the v0 retrieval corpus |
| [`2026-07-12-audy-sovmem-retrieval-corpus-v1`](archive/plans/2026-07-12-audy-sovmem-retrieval-corpus-v1.md) | P1 | V1 corpus fix (DSOM privacy-guardian → palace protocol swap) |
| [`2026-07-12-sovmem-build-audit`](archive/plans/2026-07-12-sovmem-build-audit.md) | P2 | Build audit generator and state management |
| [`2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds`](archive/plans/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds.md) | P0/P1/P2 | Close P0 and P1 gates; freeze Q3 thresholds for P2 |
| [`2026-07-13-sovmem-p2-derived-index-and-query`](archive/plans/2026-07-13-sovmem-p2-derived-index-and-query.md) | P2 | Implement Tantivy + Petgraph retrieval layer |
| [`2026-07-14-sovmem-p3-verified-write-back-loop`](archive/plans/2026-07-14-sovmem-p3-verified-write-back-loop.md) | P3 | Implement P3 v1 write-back loop (pre-intake-amendment) |
| [`2026-07-21-sovmem-p3-intake-authority-amendment`](archive/plans/2026-07-21-sovmem-p3-intake-authority-amendment.md) | P3 v2 | Add protected append-only intake authority (886 lines) |
| [`2026-07-21-sovmem-p3-lock-capability-boundary`](archive/plans/2026-07-21-sovmem-p3-lock-capability-boundary.md) | P3 v2 | Lock capability boundary with standard-library lease protocol (445 lines) |
| [`2026-07-21-sovmem-zero-to-hero`](archive/plans/2026-07-21-sovmem-zero-to-hero.md) | Product | SovMem learning product implementation plan |
| [`p3-internal-test-refactor`](archive/plans/p3-internal-test-refactor.md) | P3 | Extract flaky P3 internal tests from lib modules into isolated integration test binaries |

---

## Appendix B: Archive map

Every moved document, its original location, and where it maps in this spec.

### Archived specs

| Original path | Archived path | Master spec section |
|---------------|---------------|--------------------|
| `docs/superpowers/specs/2026-07-10-nous-root-index-design.md` | `docs/archive/specs/2026-07-10-nous-root-index-design.md` | [§3. P0](#3-phase-0-foundation-p0), [§10.2](#102-nous-root-index) |
| `docs/superpowers/specs/2026-07-10-audy-sovmem-profile-v0-design.md` | `docs/archive/specs/2026-07-10-audy-sovmem-profile-v0-design.md` | [§3. P0](#3-phase-0-foundation-p0), [§10.3](#103-audy-sovmem-reference-profile) |
| `docs/superpowers/specs/2026-07-12-audy-golden-query-draft-design.md` | `docs/archive/specs/2026-07-12-audy-golden-query-draft-design.md` | [§4. P1](#4-phase-1-retrieval-and-queries-p1) |
| `docs/superpowers/specs/2026-07-12-audy-sovmem-retrieval-corpus-design.md` | `docs/archive/specs/2026-07-12-audy-sovmem-retrieval-corpus-design.md` | [§4. P1](#4-phase-1-retrieval-and-queries-p1) |
| `docs/superpowers/specs/2026-07-12-audy-sovmem-retrieval-corpus-v1-design.md` | `docs/archive/specs/2026-07-12-audy-sovmem-retrieval-corpus-v1-design.md` | [§4. P1](#4-phase-1-retrieval-and-queries-p1) |
| `docs/superpowers/specs/2026-07-12-sovmem-build-audit-design.md` | `docs/archive/specs/2026-07-12-sovmem-build-audit-design.md` | [§5. P2](#5-phase-2-build-audit-and-gate-closure-p2) |
| `docs/superpowers/specs/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds-design.md` | `docs/archive/specs/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds-design.md` | [§5. P2](#5-phase-2-build-audit-and-gate-closure-p2) |
| `docs/superpowers/specs/2026-07-13-sovmem-p2-derived-index-and-query-design.md` | `docs/archive/specs/2026-07-13-sovmem-p2-derived-index-and-query-design.md` | [§5. P2](#5-phase-2-build-audit-and-gate-closure-p2) |
| `docs/superpowers/specs/2026-07-14-sovmem-p3-verified-write-back-loop-design.md` | `docs/archive/specs/2026-07-14-sovmem-p3-verified-write-back-loop-design.md` | [§6. P3](#6-phase-3-verified-write-back-loop-p3) |
| `docs/superpowers/specs/2026-07-21-sovmem-p3-intake-authority-amendment-design.md` | `docs/archive/specs/2026-07-21-sovmem-p3-intake-authority-amendment-design.md` | [§6. P3](#6-phase-3-verified-write-back-loop-p3) |
| `docs/superpowers/specs/2026-07-21-sovmem-p3-lock-capability-boundary-amendment.md` | `docs/archive/specs/2026-07-21-sovmem-p3-lock-capability-boundary-amendment.md` | [§6. P3](#6-phase-3-verified-write-back-loop-p3) |
| `docs/superpowers/specs/2026-07-21-sovmem-zero-to-hero-design.md` | `docs/archive/specs/2026-07-21-sovmem-zero-to-hero-design.md` | [§8. Learning Product](#8-learning-product-sovmem-zero-to-hero) |

### Archived plans

| Original path | Archived path | Master spec section |
|---------------|---------------|--------------------|
| `docs/superpowers/plans/2026-07-10-nous-root-index.md` | `docs/archive/plans/2026-07-10-nous-root-index.md` | [Appendix A](#appendix-a-implementation-plan-index) |
| `docs/superpowers/plans/2026-07-10-audy-sovmem-canary-exporter.md` | `docs/archive/plans/2026-07-10-audy-sovmem-canary-exporter.md` | [Appendix A](#appendix-a-implementation-plan-index) |
| `docs/superpowers/plans/2026-07-12-audy-golden-query-draft.md` | `docs/archive/plans/2026-07-12-audy-golden-query-draft.md` | [Appendix A](#appendix-a-implementation-plan-index) |
| `docs/superpowers/plans/2026-07-12-audy-sovmem-retrieval-corpus.md` | `docs/archive/plans/2026-07-12-audy-sovmem-retrieval-corpus.md` | [Appendix A](#appendix-a-implementation-plan-index) |
| `docs/superpowers/plans/2026-07-12-audy-sovmem-retrieval-corpus-v1.md` | `docs/archive/plans/2026-07-12-audy-sovmem-retrieval-corpus-v1.md` | [Appendix A](#appendix-a-implementation-plan-index) |
| `docs/superpowers/plans/2026-07-12-sovmem-build-audit.md` | `docs/archive/plans/2026-07-12-sovmem-build-audit.md` | [Appendix A](#appendix-a-implementation-plan-index) |
| `docs/superpowers/plans/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds.md` | `docs/archive/plans/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds.md` | [Appendix A](#appendix-a-implementation-plan-index) |
| `docs/superpowers/plans/2026-07-13-sovmem-p2-derived-index-and-query.md` | `docs/archive/plans/2026-07-13-sovmem-p2-derived-index-and-query.md` | [Appendix A](#appendix-a-implementation-plan-index) |
| `docs/superpowers/plans/2026-07-14-sovmem-p3-verified-write-back-loop.md` | `docs/archive/plans/2026-07-14-sovmem-p3-verified-write-back-loop.md` | [Appendix A](#appendix-a-implementation-plan-index) |
| `docs/superpowers/plans/2026-07-21-sovmem-p3-intake-authority-amendment.md` | `docs/archive/plans/2026-07-21-sovmem-p3-intake-authority-amendment.md` | [Appendix A](#appendix-a-implementation-plan-index) |
| `docs/superpowers/plans/2026-07-21-sovmem-p3-lock-capability-boundary.md` | `docs/archive/plans/2026-07-21-sovmem-p3-lock-capability-boundary.md` | [Appendix A](#appendix-a-implementation-plan-index) |
| `docs/superpowers/plans/2026-07-21-sovmem-zero-to-hero.md` | `docs/archive/plans/2026-07-21-sovmem-zero-to-hero.md` | [Appendix A](#appendix-a-implementation-plan-index) |
| `.handover/p3-internal-test-refactor.md` | `docs/archive/plans/p3-internal-test-refactor.md` | [Appendix A](#appendix-a-implementation-plan-index), [§6. P3](#6-phase-3-verified-write-back-loop-p3) |

### Archived audits

| Original path | Archived path | Master spec section |
|---------------|---------------|--------------------|
| `docs/SOVMEM-BUILD-AUDIT.md` | `docs/archive/audits/SOVMEM-BUILD-AUDIT.md` | [§9.1](#91-active-build-audit) |
| `docs/SOVMEM-P0-P2-INDEPENDENT-AUDIT.md` | `docs/archive/audits/SOVMEM-P0-P2-INDEPENDENT-AUDIT.md` | [§9.2](#92-p0p2-independent-gap-analysis) |

### Archived product PRD

| Original path | Archived path | Master spec section |
|---------------|---------------|--------------------|
| `docs/product/2026-07-21-sovmem-zero-to-hero-prd.md` | `docs/archive/product/2026-07-21-sovmem-zero-to-hero-prd.md` | [§8. Learning Product](#8-learning-product-sovmem-zero-to-hero) |

### Archived TTF Canary

| Original path | Archived path | Master spec section |
|---------------|---------------|--------------------|
| `docs/ttf-canary/CANONICAL-SPEC.md` | `docs/archive/ttf-canary/CANONICAL-SPEC.md` | [§10.1](#101-ttf-canary-trust-threshold-framework) |
| `docs/ttf-canary/docs/*.md` (14 files) | `docs/archive/ttf-canary/docs/*.md` | [§10.1](#101-ttf-canary-trust-threshold-framework) |

### Documents retained in place

| Path | Reason |
|------|--------|
| `docs/sovmem_v0.3_execution_plan.md` | Still the normative development authority |
| `docs/sovmem-build-state.json` | Reviewed state input for the build audit generator |
| `docs/spec/source-lineage-june-23-spec-bundle-verbatim.md` | Still the accepted source-lineage artifact |
| `docs/archive/` | Now the canonical archive root |

---

## Appendix C: Status anomalies corrected

The following documents had stale or incorrect status headers on their original files. The status has been corrected in the archive copy and noted here:

| Document | Old status | Corrected status |
|----------|------------|------------------|
| `specs/2026-07-13-sovmem-p2-derived-index-and-query-design.md` | "Draft for repository-owner review" | ✅ Approved (P2 gate closed) |
| `specs/2026-07-14-sovmem-p3-verified-write-back-loop-design.md` | "Proposed for human review" | ✅ Approved (P3 gate closed) |
| `specs/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds-design.md` | "Human-approved design; committed specification review pending" | ✅ Approved (gate closed) |
| `specs/2026-07-21-sovmem-p3-lock-capability-boundary-amendment.md` | "approved design choice; implementation remains paused pending review" | ✅ Approved (implementation complete) |
| `ttf-canary/CANONICAL-SPEC.md` | "Draft single source of truth" | Kept as Draft (unimplemented) |
| `ttf-canary/docs/03-technical-spec.md` | "Draft" | Kept as Draft (unimplemented) |
| `ttf-canary/docs/04-prd.md` | "Draft v0.1" | Kept as Draft (unimplemented) |
