# SovMem v0.3 Build Audit

> Generated from `docs/sovmem-build-state.json` with `npm run audit:sovmem`. Do not edit this report directly.
>
> The execution plan remains normative: `docs/sovmem_v0.3_execution_plan.md` is the authority, and this audit does not promote phases.

## Snapshot

- Overall completion: 100%
- Review date: 2026-09-16
- Gate-closure verification baseline: 775 total (85 JavaScript, 690 Rust).
- Source-tree fingerprint: `fef521b3de48998fd525985412eae40aada10809eb0a4f856ae7035b27c3050a`

## Phase breakdown

| Phase | Name | Completion | Gate |
| --- | --- | ---: | --- |
| P0 | Scaffold & ground truth | 100% | passed |
| P1 | Bundle substrate | 100% | passed |
| P2 | Derived index & query | 100% | passed |
| P3 | Verified write-back loop | 100% | passed |
| P4 | MCP adapter | 100% | implementation_delivered |
| P5 | Hardening, sentry, trust instrumentation, dogfood | 100% | implementation_delivered |

## Current state

R0 complete. All F01–F11 code-level repairs applied, compiled, CI green (2 runs), PR #8 merged to main. F01–F11 closures verified. R0 accepted by repository owner on 2026-09-16.

## Completed capabilities

### P0 — Scaffold & ground truth

- Locked the 62-document corpus, 25 human-approved golden queries, Audy Vault authority, single-crate layout, private repository discipline, five-verb ledger, public three-verb subset, and provenance enum.
- Pinned the official OKF reference and froze the reviewed Q3 threshold contract against the immutable golden-file bytes.

### P1 — Bundle substrate

- Implemented cumulative Level 1 structure, Level 2 raw-byte integrity, and Level 3 semantic and complete-corpus verification while preserving bare-verify compatibility.
- Implemented deterministic verified-bundle statistics and stable scenario-specific diagnostics for all five formal corruption failures.
- Passed explicit Level 3 on 62 accepted documents with 226 warnings, one exclusion, and zero errors.

### P2 — Derived index & query

- Built a disposable full-rebuild-only Tantivy index after explicit Level 3 verification of the authoritative 62-document bundle.
- Implemented deterministic boosted BM25 retrieval over title, tags, and body with canonical-ID tie-breaking and metadata-only results.
- Implemented deterministic typed-frontmatter graph projection and separate one-hop expansion that does not alter BM25 rank or score.
- Wrote the deterministic canonical retrieval baseline against the frozen 25-query set and immutable Q3 thresholds.
- Proved two-build, slot-switch, and deleted-index rebuild semantic determinism without changing the canonical bundle.
- Passed the integer Q3 GO gate with 23 of 25 queries hitting within k=5, above the required minimum of 20.

### P3 — Verified write-back loop

- Immutable candidate Markdown with bound proposal metadata and strict versioned policy.toml.
- Protected append-only intake authority with interactive intake-seal CLI (P3 v2).
- Five-verb human verdict CLI (ACCEPT, REJECT, REWRITE, TP, FP) with exact-byte ACCEPT and human-supplied exact-byte REWRITE.
- Deterministic Human Judgment documents and copy-on-write canonical generations with integrity manifests.
- Full P2 index rebuild before each verdict commit.
- Append-only BLAKE3-chained verdict ledger with verify --level 4 over store and ledger chain.
- Exclusive mutation locking with standard-library no-replace lease protocol (no direct rustix dependency).
- Idempotency, crash detection, deterministic recovery, and safe-clone real-data proof.
- P3 internal test refactoring: 399 tests extracted from lib modules to isolated integration test binaries with --features test-support, eliminating concurrent filesystem I/O failures and Condvar deadlocks. Authority boundary contract updated for canonical refactored visibility.

### P4 — MCP adapter

- Raw JSON-RPC 2.0 server over stdio, protocol revision 2026-07-28.
- Four tools: memory_search, memory_get, memory_propose, memory_status.
- Synchronous single-threaded server, no async, no new dependencies.
- 26 tests across 3 layers (unit, stdio simulation, P3 fixture integration).
- Feature-gated behind #[cfg(feature = 'mcp')] (default-enabled).
- Reviewed, fixed (jsonrpc 2.0 conformance, redaction, feature gate), merged to main.

### P5 — Hardening, sentry, trust instrumentation, dogfood

- sovmem trust command: deterministic ledger arithmetic with per-source rates, calibration gap, optimal threshold, small-N guard, quota recommendations.
- Eval regression CI gate: baseline readability check in sovmem-audit.yml.
- Nightly sentry pipeline: scripts/sovmem-sentry.sh with npm run sentry:sovmem.
- Fuzz frontmatter parser: 9 no-panic test cases (time-boxed).
- 10k-doc scale test: #[ignore], excluded from default suite.
- SKILL.md runbook: propose protocol, confidence elicitation, skeptic stance, verdict loop, trust report guidance.
- audit --sample CLI stub registered (full implementation deferred).

## Remaining work

### P0 — Scaffold & ground truth

None recorded.

### P1 — Bundle substrate

None recorded.

### P2 — Derived index & query

None recorded.

### P3 — Verified write-back loop

- Additional dogfood hardening.

### P4 — MCP adapter

- Calendar-gated operational evidence is carried forward: seven sentry nights, seven dogfood days — handled by sentry pipeline (v0.4 build-phase requirement, not R0 blocker).

### P5 — Hardening, sentry, trust instrumentation, dogfood

- Calendar-gated operational evidence is carried forward: seven sentry nights, seven dogfood days (v0.4 build-phase requirement, not R0 blocker).
- Blindspot cell detection in compute_trust() — currently returns empty Vec stub.
- Full audit --sample implementation — needed for periodic sampling.

## Verification evidence

- Gate-closure verification baseline: 85 JavaScript and 690 Rust (775 total).
- Corpus: 62 accepted documents (8 core, 54 research), 226 warnings, 1 exclusion, 0 errors.
- Golden queries: 25 queries, 25 relevant IDs, 5 coverage groups, 0 errors; BLAKE3 `f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175`.
- Frozen thresholds: sovmem-retrieval-thresholds-v1, k=5, 20 hit queries required, frozen 2026-07-12.
- Verification: Level 3, 62 documents, 226 warnings, 0 errors.
- Bundle BLAKE3: `49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65`
- Q3 retrieval gate: 23 of 25 queries hit within k=5; minimum 20; GO.
- Derived index and graph: 62 documents, 62 graph nodes, 19 graph edges, 81 external typed links.
- Retrieval baseline BLAKE3: `c3fdeb201e849e86bd6bdf4018e44970892b062bd22de9ca05e07ab9734722d6`
- Index configuration hash: `95a2237b666e0d6557709bb98eb2b4fe54a7478f6dca17df0bd3a14049e17f8d`
- Implementation commit: `614f02fbc9033913bf99a2f6561435526146b0a8`

### P0 — Scaffold & ground truth

- docs/spec/source-lineage-june-23-spec-bundle-verbatim.md
- evals/audy-sovmem-v0/golden.jsonl
- evals/audy-sovmem-v0/thresholds.json
- profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml
- sovmem/Cargo.toml

### P1 — Bundle substrate

- sovmem/src/profile/stats.rs
- sovmem/src/profile/verify.rs
- sovmem/tests/bundle_verification.rs
- sovmem/tests/cli.rs

### P2 — Derived index & query

- sovmem/src/profile/index.rs
- sovmem/src/profile/graph.rs
- sovmem/src/profile/query.rs
- sovmem/src/profile/eval.rs
- sovmem/tests/p2_determinism.rs
- evals/audy-sovmem-v0/golden.jsonl
- evals/audy-sovmem-v0/thresholds.json
- evals/audy-sovmem-v0/baseline.json

### P3 — Verified write-back loop

- sovmem/src/cli/p3.rs
- sovmem/src/profile/intake.rs
- sovmem/src/profile/judgment.rs
- sovmem/src/profile/ledger.rs
- sovmem/src/profile/policy.rs
- sovmem/src/profile/proposal.rs
- sovmem/src/profile/store.rs
- sovmem/src/profile/transaction.rs
- sovmem/src/profile/canon.rs
- sovmem/src/profile/clock.rs
- sovmem/tests/p3_cli.rs
- sovmem/tests/p3_privacy.rs
- sovmem/tests/p3_canon.rs
- sovmem/tests/p3_index.rs
- sovmem/tests/p3_internal.rs
- sovmem/tests/p3_proposal.rs
- sovmem/tests/p3_stats.rs
- sovmem/tests/p3_transaction.rs
- profiles/audy-ssa0-p3-v2/profile.yaml
- profiles/audy-ssa0-p3-v2/policy.toml
- test/sovmem-p3-safe-clone.test.js

### P4 — MCP adapter

- sovmem/src/mcp/mod.rs
- sovmem/src/mcp/tools.rs
- sovmem/tests/mcp_integration.rs
- sovmem/tests/mcp_stdio.rs

### P5 — Hardening, sentry, trust instrumentation, dogfood

- sovmem/src/profile/trust.rs
- sovmem/tests/trust_unit.rs
- sovmem/src/cli.rs
- sovmem/src/cli/p3.rs
- test/sovmem-build-audit.test.js
- sovmem/tests/source_contract.rs

## Active blockers

None recorded.

## Recommended next step

v0.4 and OKF v0.2 migration is the next governed phase. Calendar-gated operational evidence (seven sentry nights, seven dogfood days) and audit --sample implementation are carried forward as v0.4 build-phase requirements, not R0 blockers.

## Phase publication history

| Date | Phase | Summary | Commit |
| --- | --- | --- | --- |
| 2026-07-12 | P0/P1 | Closed the ground-truth and bundle-substrate gates and froze Q3. | `51025666fb9761e998b3e00cd6e33244b937358c` |
| 2026-07-13 | P2 | Delivered the canonical P2 retrieval gate closure with Q3 GO at 23/25 top-five hits. | `614f02fbc9033913bf99a2f6561435526146b0a8` |
| 2026-07-27 | P3 | Delivered P3 v2 with intake-authority hardening, lock capability-boundary amendment, and safe-clone real-data proof. Master spec consolidation complete. | `6d744194d82fb71850577996020c234cb90a3621` |
| 2026-08-17 | P3 | P3 internal test refactoring: 399 tests extracted from lib to isolated integration test binaries, eliminating concurrent filesystem I/O failures. Authority boundary contract updated to match refactored visibility. | `a9c8285c8431b7c9d2ee3dca20043ced067a7b47` |
| 2026-08-17 | P4/P5 | P4 MCP adapter delivered and merged. P5 operational hardening delivered: trust command, sentry, eval regression, fuzz/scale tests, runbook. v0.3 tagged at 3e0817f. | `3e0817faf7b4dd8f3998cbe19f16aebb5f366e05` |
| 2026-08-18 | P4/P5 | Source-contract authority boundary updated: strip_cfg_gates() removes cfg-gated constructs before scanning P3 production paths; 'trust' removed from forbidden-list since it is a delivered P5 read-only measurement command. Rust test count updated to 690 (was 151). | `e0891fea22c152bd7f286bf3c214381098be9d11` |
| 2026-09-16 | R0 | R0 repair complete. F01–F11 code-level findings resolved. Repository source recovered from damaged worktree, verified by independent review. CI green at 1a735c9. PR #8 merged. Calendar-gated operational evidence carried forward to v0.4 build-phase requirements. | `1a735c9a5efe6d5efa3e56a607f12c4d06d0ff9c` |

## Update contract

1. Update the reviewed state when completion, evidence, blockers, or next-step judgment changes.
2. Regenerate with `npm run audit:sovmem`.
3. Verify exact freshness with `npm run audit:sovmem:check`.
4. Before manual merge, review the implementation and audit together and pass `npm test` at the reviewed head. This repository relies on manual pull-request merge discipline and does not claim branch protection, required checks, rulesets, or merge queues are enforced.
