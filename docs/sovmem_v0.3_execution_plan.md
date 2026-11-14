# Sovereign Multi-Agent Memory — v0.3 Execution Plan

**Codename:** `sovmem` in the private Nous repository (D7).
**Status:** Normative execution authority. All five phases (P0–P5) are delivered. Tagged as `v0.3` at commit `3e0817f`. Final June 23 recovery remains accepted for source lineage. This document is archived for reference — phase-specific follow-up work is tracked in handover-p5.md.
**Supersedes:** v0.2 (2026-07-07). v0.2 absorbed the Loop Engineering gap map (Appendix B); v0.3 absorbs the Trust Threshold gap map (Appendix C).
**Consolidated specification:** All approved SovMem specs, plans, PRDs, and audits are consolidated in [`docs/sovmem-master-spec.md`](sovmem-master-spec.md). This execution plan remains normative; the master spec provides navigation, status, and context.
**Source lineage:** June 23 spec bundle (G1–G7, W1–W6) · life.thefutureissolo.com doctrine · Loop Engineering synthesis (HuaShu Orange Book — provenance: synthesis) · Trust Threshold references (§Sources — provenance: tier1/secondary, verified on arXiv and lab sites 2026-07-07).
**Audience:** A cheaper executor model running step-by-step under human supervision. Numbered steps, exit gates, tripwires. On any conflict between this plan and source docs: stop and ask for clarification.

## Changelog v0.2 → v0.3 (from the Trust Threshold gap analysis)

1. New axiom **A7** — trust is measured in the ledger; thresholds allocate attention, never authority (in v0.x).
2. Proposals gain an optional, untrusted **`confidence`** field (P(IS)-style, elicited pre-draft); displayed at verdict time, can never gate.
3. New CLI verb **`trust`** — deterministic per-source metrics from the ledger: acceptance/REWRITE/TP/FP rates, calibration gap, implicit propose-threshold vs the configured optimal threshold.
4. **Cost ratio made explicit** in `policy.toml` (cost of a bad ACCEPT vs cost of one verdict) — the optimal escalation threshold becomes a derived number, not dogma.
5. **Skeptic evaluator default stance codified** (Q9 answered): adversarial bug-finding framing, the empirically best-calibrated assessment mode.
6. New **blindspot report**: calibration residuals by (source × layer/type) localize where unknown unknowns cluster; feeds demotion review and biases the audit sample.
7. **Trust-informed attention allocation:** verdict-queue ordering, quota *recommendations*, mandatory-skeptic routing for low-trust sources. Policy changes remain human commits.
8. **E7 "graduated trust" parked** on the enterprise roadmap with named, evidence-based entry criteria. New risk **R10** (Goodharting trust scores).

---

## 0. What this document is

The June 23 session produced the architecture, limits analysis, PRD, and workstream plans. This document converts that spec into an executable build sequence: bundle substrate → derived index → verified write-back loop → MCP adapter → hardening and instrumentation. The document is deliberately opinionated about scope.

**The thesis in one line (from the spec):** the format (OKF) is a commodity; the defensibility is the typed judgment index plus the verified write-back loop. Truth lives in the bundle; intelligence lives in the index and the loop.

**Position in the loop stack:** sovmem is neither a loop nor a harness. It owns **Persistence** outright and the terminal half of **Verification** (integrity + human verdict), serves **Discovery** a queryable index, and — new in v0.3 — instruments **Escalation**: every proposal is an escalation to the human, and the ledger measures how well each source escalates.

---

## 1. Axioms (engineering invariants)

Violating any of these is a build failure.

- **A1 — Bundle is truth; index is disposable.** The `.md` bundle is portable and auditable. The tantivy/petgraph index is derived, regenerable, never authoritative.
- **A2 — Agents propose; only human verdicts commit.** No MCP write path to canon. Ever. Read + propose only. Verdicts are CLI-only, human-only.
- **A3 — Never trust an unverified write.** Every mutation of canon is blake3-verified and appended to a tamper-evident ledger.
- **A4 — Rebuild-from-zero always works.** Deleting the index and rebuilding from the bundle is a permanent CI test.
- **A5 — Conformance is not the moat.** OKF conformance is nearly free by design. Defensibility lives in the Judgment type, the `:: reason` field, and the loop.
- **A6 — Deterministic core, probabilistic periphery.** The sovmem binary makes **zero model calls and zero network calls**. Anything rule-bound is deterministic code; anything requiring a model lives harness-side and enters sovmem only as data on a proposal.
- **A7 — Trust is measured, never assumed (new).** Trust scores are computed from verdict history in the ledger — never from self-reports, never from a model's say-so. In v0.x, trust may reorder the queue, tune quotas, and route skeptic review; it may **never** commit to canon. Autonomy is earned in the ledger, and the ledger's cost math says: escalate everything (see D11).

---

## 2. Scope

### North star: the six-step demo
v0.3 is done when this runs end-to-end on real vault data, unassisted:

1. An agent (via MCP) queries memory and receives ranked canon documents.
2. The agent answers a task citing document ids.
3. The agent proposes a canon update capturing a new judgment — attaching pre-draft confidence; a harness-side skeptic evaluator may annotate it.
4. PolicyEvaluator gates the proposal into staging (or auto-rejects with reason) — quotas and dedup enforced.
5. The human issues `sovmem verdict <id> ACCEPT --reason "..."` — applied, re-hashed, re-indexed, ledger-chained.
6. A *fresh* agent session retrieves the improved document plus the verdict reason — and `sovmem trust` shows the source's updated calibration.

### Goals (P0 outcomes)
- G1: OKF-shaped bundle with schema, provenance, and integrity verification.
- G2: Deterministic derived index (BM25 + 1-hop graph); baseline precision@5 / recall@5 on a golden query set.
- G3: Verified write-back loop with 5-verb verdict grammar, PolicyEvaluator gating, hash-chained ledger, and flood control.
- G4: MCP adapter (stdio) usable from Claude Code / OpenClaw harnesses.
- G5: Dogfood cutover with nightly sentry, sampling audit, and **weekly trust report** running unattended.

### Non-goals (v0.3) — the parking lot
- **No embeddings / vector search / Qdrant.** Lexical + graph only; revisit only if the P2 gate fails (D2).
- **No incremental indexing.** Full rebuild only.
- **No HTTP/SSE transport.** stdio only (D6).
- **No model calls inside the binary** (A6). Evaluator agents are harness citizens.
- **No trust-gated auto-accept (new).** No proposal commits without a human verdict, regardless of trust score, confidence, or skeptic recommendation. Graduated autonomy is E7, entered only on evidence (Appendix C).
- **No loop orchestration.** Contract instead (Appendix A).
- **No migration/unification of Mem0 → Qdrant → Obsidian → NotebookLM** (R3).
- **No multi-user, no access control, no concurrency** (E1–E6 stand).
- **No GUI/TUI.** Verdict ergonomics via flags and batch mode.
- **No runtime self-modification of policy.** `policy.toml` changes are human commits — including quota changes recommended by `trust`.
- **No authority shift from the P2 derived layer.** The delivered Tantivy index, Petgraph projection, BM25 results, and evaluation baseline remain disposable evidence over the authoritative Markdown bundle; P2 adds no canon mutation or P3 behavior.

---

## 3. System shape (one screen)

```
vault-repo/
├── bundle/                  # CANON. Only the verdict-apply path writes here.
│   ├── index.md
│   ├── concepts/*.md        # type: concept|resource|judgment (YAML frontmatter)
│   └── manifest.json        # relpath -> blake3, + bundle root hash
├── staging/                 # proposals awaiting verdict (never indexed as canon)
│   └── <proposal-id>.md     # + .meta.json (base hashes, source, timestamp,
│                            #   optional confidence, optional evaluator block)
├── ledger/
│   └── verdicts.jsonl       # append-only, each line hash-chains the previous
├── policy.toml              # schema rules, layers, path allowlist, quotas, dedup,
│                            #   queue alarm, cost ratio (bad-ACCEPT : verdict)
├── evals/
│   ├── golden.jsonl         # {query, relevant_ids[]}   (retrieval suite)
│   └── baseline.json        # P@5 / R@5 per commit      (regression gate)
└── .index/                  # DISPOSABLE. gitignored. tantivy + petgraph
```

**Implemented P0-P4 CLI surface:** `export`, `corpus`, `verify [--level 1..4]`, `verify-golden`, `stats`, `index`, `query`, `eval`, `propose`, `verdict`, `proposals`, `verdicts`, `serve`. **P5 additions:** `trust`, `audit` (sample stub).

**MCP tools (stdio, JSON-RPC 2.0, no SDK):** implemented in P4. `memory_search`, `memory_get`, `memory_propose`, `memory_status`. Protocol revision `2026-07-28` — stateless (no `initialize` handshake), per-request `_meta` for version and capabilities. No verdict tool, no write tool (A2), no trust-write tool (A7).

**Frontmatter schema:** `id`, `type` (concept|resource|judgment), `title`, `layer` (synthesis|analysis|concept), `tags[]`, `links[]` (`{rel, target}`), `source`, `provenance` (`tier1|secondary|synthesis|unknown`, D10), `created`, `updated`. Judgment adds `verdict`, `reason`, `subject`.

**Proposal `.meta.json`:** base hashes, source loop/agent, timestamp, **optional `confidence` (0–1, self-reported P(accept), elicited *before* drafting — see Appendix A)**, optional evaluator block `{agent, recommendation, reasons[]}`. Both fields are **display-only at verdict time; neither can gate** (A7). They exist so the ledger can later score the source's calibration.

**Trust metrics (deterministic, computed from the ledger — A6-safe):**
- Per source, rolling window (Q12): acceptance rate, REWRITE rate, TP/FP rates, proposal volume.
- **Calibration gap:** mean stated confidence − empirical acceptance rate (bucketed curve when N allows).
- **Implicit propose-threshold:** the confidence distribution at which the source chooses to propose — compared against the **optimal threshold** derived from the `policy.toml` cost ratio (D11).
- **Blindspot report:** (source × layer/type) cells with high stated confidence and low acceptance/high FP — candidate unknown-unknowns; feeds the demotion review and biases `audit --sample`.
- Small-N guard: no recommendation is emitted under N < 30 verdicts for a cell; counts shown instead.

---

## 4. Phases

Effort in **focused sessions** (≈ 2–3 hrs supervised). Total ≈ 14.

### P0 — Scaffold and ground truth (1 session)
1. **Resolved for P0/R8:** use `docs/spec/source-lineage-june-23-spec-bundle-verbatim.md` as the final available recovery and source-lineage record. This v0.3 plan is normative for execution; stop for human resolution where the recovery records a conflict or delta.
2. Keep one Rust crate, `sovmem`, with pinned `rust-toolchain.toml` and committed `Cargo.lock` (D5).
3. Pin the official OKF reference to repository `GoogleCloudPlatform/knowledge-catalog`, commit `d44368c15e38e7c92481c5992e4f9b5b421a801d`, normative file `okf/SPEC.md`. Tantivy, Petgraph, and other P2 dependencies are not added in this slice.
4. Treat Audy Vault as the canonical, Git-versioned, read-only source for this slice, pinned at `43824cfd9caed7b260aa442128fd1130ddb6e2ff` (Q1/Q2).
5. Lock 62 accepted documents: 8 core and 54 research. Lock 25 human-approved queries in `evals/audy-sovmem-v0/golden.jsonl`.

**Exit gate:** one crate builds; the accepted corpus is exactly 62 documents with 8 core and 54 research; all 25 queries are human-approved; the OKF reference and D10 provenance enum are pinned.
**Tripwires:** lossy normalization, a changed corpus boundary, or a changed golden file requires a new human review.

### P1 — Bundle substrate (2 sessions)
1. Level 1 validates frontmatter, required types, the complete profile identity, stable bindings, and `provenance` against `tier1|secondary|synthesis|unknown`.
2. Level 2 adds raw-byte BLAKE3 document hashes, manifest and exact-file integrity, source-hash commitments, the index, and the independently recomputed framed bundle root.
3. Level 3 adds typed-link and source/report correspondence and requires complete corpus evidence. Bare `verify` retains compatibility behavior; explicit `verify --level 3` is the formal P1 gate.
4. `stats` deterministically reports document count, provenance/type/layer distributions, warnings, exclusions, and the verified bundle hash.
5. Fixtures prove `bundle.document_hash_mismatch`, `frontmatter.malformed`, `document.provenance_invalid`, `bundle.root_hash_mismatch`, and `links.dangling` at their owning levels.

**Exit gate:** explicit `verify --level 3` is green on the 62 accepted documents, `stats` agrees with the verified evidence, and all five corruption fixtures fail with stable greppable error codes.

### P2 — Derived index & query (2 sessions) — **GO/NO-GO gate passed**
Q3 is frozen in `evals/audy-sovmem-v0/thresholds.json`: `k = 5`, macro Precision@5 ≥ `0.16`, macro Recall@5 ≥ `0.80`, and `minimum_queries_with_hit = 20` out of all 25 queries. Because each query has one relevant document, both macro thresholds reduce to the same integer gate: at least 20 queries must hit within the top five. The golden queries, thresholds, and fixtures must not change during P2.

1. tantivy schema (`id,title,body,layer,type,tags,provenance`); full rebuild only.
2. petgraph adjacency from frontmatter `links[]` (D4).
3. `query`: BM25 top-k + optional 1-hop expansion.
4. Determinism test: two builds → identical counts and top-5 on all golden queries.
5. `eval`: P@5/R@5 → `baseline.json`.

**Exit gate (GO/NO-GO): PASSED.** The canonical baseline recorded 23 of 25 queries with a hit within `k=5` (Precision@5 `0.1840`, Recall@5 `0.9200`), exceeding the frozen integer minimum of 20. The verified derived artifact contains 62 documents, 62 graph nodes, 19 internal typed-link edges, and 81 external typed links. Baseline BLAKE3 is `c3fdeb201e849e86bd6bdf4018e44970892b062bd22de9ca05e07ab9734722d6`; misses are `q-005` and `q-011`.
**Tripwires:** no tuning thresholds, golden queries, or fixtures to pass gates.

### P3 — Verified write-back loop (3.5 sessions) — **PASSED through reviewed safe-clone proof**
1. `propose`: candidate `.md` + `.meta.json` (incl. optional pre-draft `confidence`) into `staging/`. Never touches `bundle/`.
2. PolicyEvaluator (`policy.toml`): schema-invalid → reject; stale base hash → reject; `layer: synthesis` edits without REWRITE intent → reject; new resources without provenance → reject; size caps; path allowlist; per-source quotas; duplicate (target, content-hash) rejection.
3. `verdict <id> <VERB> --reason`: ACCEPT/REWRITE apply → re-hash → re-index → ledger; REJECT/TP/FP ledger-only. Empty reason = build failure. Ledger lines record the proposal's stated confidence, so calibration is computable later.
4. Ledger hash chain; `verify --level 4` walks it. Queue alarm in `stats`.
5. Evaluator + confidence passthrough surfaced in `verdicts list`; **fixtures proving neither can gate** (confidence=0.99 + policy violation → rejected; evaluator "looks-sound" + violation → rejected).
6. Red-team fixtures (write first): stale base, protected-layer edit, tampered ledger line, hash-mismatch injection, duplicate id, empty reason, quota breach, duplicate proposal, forged evaluator block, **confidence-as-gate attempt**.

**Exit gate: PASSED.** Red-team fixtures are blocked with stable redacted diagnostics; a 20-verdict mixed chain verifies at Level 4; and the approved safe-clone ceremony completed one human-only ACCEPT, canonical reconstruction, index rebuild, and target/Judgment re-query. The safe-clone proof preserves the frozen P2 Q3 result (23/25, Precision@5 `0.1840`, Recall@5 `0.9200`, misses `q-005` and `q-011`) without changing the Vault or frozen P2 artifacts.
**Tripwires:** any write to `bundle/` outside verdict-apply → stop. Any auto-accept urge → stop (E7 parking lot).

### P4 — MCP adapter (2 sessions) — **IMPLEMENTED**
1. Raw JSON-RPC 2.0 server over stdio, protocol revision `2026-07-28`, synchronous single-threaded dispatch. No SDK dependency — uses `serde_json` (already in `Cargo.toml`).
2. Four tools: `memory_search`, `memory_get`, `memory_propose`, `memory_status`. Stateless per-request `_meta` for version and capabilities.
3. 26 tests across 3 layers (unit, stdio simulation, P3 fixture integration).
4. Feature-gated behind `#[cfg(feature = "mcp")]` (default-enabled).
5. Pending: six-step live-agent demo (requires harness + safe clone).

**Exit gate:** implementation merged to main at commit 894a6c1. Live-agent ceremony remains to be demonstrated.
**Tripwires:** none triggered.

### P5 — Hardening, sentry, trust instrumentation, dogfood — ✅ DELIVERED (v0.3 tagged)

1. `eval` as regression gate — baseline readability check in CI (step added to sovmem-audit.yml). Full comparison (fresh eval vs baseline) deferred — requires Tantivy index in CI.
2. Nightly sentry — `scripts/sovmem-sentry.sh` with `npm run sentry:sovmem`: `verify --level 4` → `index` → `eval` → `trust`. Fail-loud. Cron-ready.
3. `audit --sample N` — CLI stub present (`sovmem audit sample`). Full blindspot-biased sampling deferred.
4. **`trust` command** — implemented: `sovmem trust --profile --store [--window 50] [--focus] [--output] [--format json|text]`. Deterministic ledger arithmetic: per-source rates, calibration gap, optimal threshold (D11), small-N guard at 30. 10 unit tests. Blindspot cell detection deferred (empty Vec).
5. Fuzz frontmatter parser — 9 test cases (time-boxed). 10k-doc scale test — `#[ignore]`, excluded from default suite.
6. Runbook — `docs/sovmem-skill.md` with propose protocol, confidence elicitation, skeptic stance, verdict loop, trust report guidance.
7. Dogfood cutover — infrastructure ready. Live execution pending safe clone setup.

**Exit gate:** v0.3 tagged at commit 3e0817f.

---

## 5. Key decisions

| # | Decision | Options | Recommendation | Owner | When |
|---|---|---|---|---|---|
| D1 | Hash target | raw bytes vs canonical form | **Raw bytes.** | Decided | P0 |
| D2 | Retrieval | lexical+graph vs hybrid | **Lexical+graph.** The canonical P2 baseline passed; embeddings and hybrid retrieval remain out of scope. | Decided | P2 |
| D3 | Staging & audit | filesystem+ledger vs git-native | **Filesystem + five-verb ledger.** Git supplies snapshot identity and repository review history, not the judgment ledger. | Decided | P0 |
| D4 | Graph edges | frontmatter only vs +wikilinks | **Frontmatter only.** | Decided | P2 |
| D5 | Crate layout | single crate vs six-crate workspace | **Single crate.** | Decided | P0 |
| D6 | MCP transport | stdio vs +HTTP | **stdio only.** | Decided | P4 |
| D7 | Name / repo / visibility | separate/public vs Nous/private | **Nous repository, private, with manual pull-request merge discipline.** | Decided | P0 |
| D8 | Verdict grammar | 5-verb vs 3-verb | **Five internal verbs; `ACCEPT`, `REJECT`, and `REWRITE` are the public pedagogical subset.** | Decided | P0 |
| D9 | Skeptic placement | in-binary vs harness-side | **Harness-side, advisory-only** (A6). | Decided | P3 |
| D10 | Provenance enum | tiers; required where | **`tier1|secondary|synthesis|unknown`; required on new resources.** | Decided | before P1 |
| D11 | Cost ratio *(new)* | value of (bad-ACCEPT cost : verdict cost) in policy.toml | **Set explicitly; start ≥ 100:1.** At that ratio the derived optimal policy for canon is escalate-always — A2 becomes a theorem, not a preference. Revisit per doc class at E7. | Khalil | before P5.4 |
| D12 | Confidence protocol *(new)* | scale, timing, required? | **0–1, elicited pre-draft, optional.** Pre-execution estimates discriminate better than post-hoc self-review; never required, never gating. | Decided | P3 |

---

## 6. Risks — ranked. ⚠ = could sink the project

- **R1 ⚠ Verdict friction kills the loop.** *Mitigation:* batch mode; skeptic annotations; quotas + dedup; trust-ordered queue; verdicts/week as primary KPI.
- **R2 ⚠ Lexical retrieval below the bar.** *Mitigation result:* the frozen P2 GO/NO-GO passed at 23 of 25 top-five hits; D2 remains the explicit future escape hatch only if later reviewed evidence reopens the decision.
- **R3 ⚠ Scope creep into stack unification.** *Mitigation:* non-goal; additions require removals.
- **R4 — MCP/JSON-RPC implementation complexity.** *Mitigation:* stdio only; raw JSON-RPC (no SDK); synchronous single-threaded; budget-boxed.
- **R5 — OKF drift.** *Mitigation:* pin commit; conformance floor + export mapping (A5).
- **R6 — Executor-model Rust hazards.** *Mitigation:* ban list; async in `mcp/` only; fixtures-first.
- **R7 — Integrity theater.** Hashes prove *unchanged*, not *true*; poisoning happens at ACCEPT time. *Mitigation:* mandatory reasons; provenance visible at verdict; TP/FP tracked; audit sample (blindspot-biased); demotion review. **The trust report now also scores the judge: inconsistent verdicting shows up as noise in every source's calibration.**
- **R8 — Spec bundle lost.** *Mitigation:* P0 locate-or-declare.
- **R9 — Staging flood.** *Mitigation:* quotas, dedup, queue alarm, kill criteria.
- **R10 — Goodharting trust scores (new).** Sources can farm acceptance with trivial, safe proposals, or sandbag confidence to look calibrated; a high trust score then argues for autonomy it hasn't earned. *Mitigation:* trust allocates attention, never authority (A7); REWRITE rate and proposal triviality visible in the report; small-N guard; `audit --sample` re-opens accepted work; E7 entry criteria include *value* signals, not just acceptance rates.

---

## 7. Open questions

**Resolved for P0/P1:**
- **Q1/Q2:** Audy Vault is canonical, Git-versioned, read-only for this slice, and pinned at `43824cfd9caed7b260aa442128fd1130ddb6e2ff`.
- **Q3:** `thresholds.json` freezes `k = 5`, macro Precision@5 ≥ `0.16`, macro Recall@5 ≥ `0.80`, and `minimum_queries_with_hit = 20` for the immutable 25-query set.
- **Q4/Q10:** Resolved by D7, D8, and D10.

**Non-blocking (during build):**
- **Q5:** P4 smoke-test harness — Claude Code, OpenClaw/Hermes, or both?
- **Q6:** License, if public.
- **Q7:** Long-run Qdrant/Mem0 positioning.
- **Q8:** SSA-CMM route card ↔ sovmem state integration.
- **Q9 (answered in v0.3, confirm):** Skeptic default = adversarial bug-finding stance ("assume the proposal is wrong; list what fails"), the best-calibrated assessment framing in the overconfidence literature. Model choice still open.
- **Q11 (Khalil, new):** Confirm the D11 cost ratio (or supply your own estimate of poisoned-canon cost vs a 10-second verdict).
- **Q12 (new):** Trust window — rolling last-K verdicts (default K=50) vs time-based (default 30 days)?

---

## 8. Executor protocol (for the cheaper model)

**Session loop:** read this doc → state current phase step → failing fixture first → implement → `cargo test` + `sovmem verify` → commit → report vs exit gate.

**Ban list (hard):**
1. No writes to `bundle/` except through the verdict-apply path.
2. No new dependencies without approval.
3. No `unsafe`. No async anywhere — the binary uses synchronous I/O throughout (the MCP adapter uses a dedicated thread with BufReader/BufWriter).
4. No schema changes after the P1 gate without a human verdict in the ledger.
5. No tuning eval thresholds, golden queries, or fixtures to pass gates.
6. No skipping fixtures-first.
7. No model calls, no network calls from the binary (A6). MCP is stdio only.
8. **No code path where `confidence`, evaluator output, or trust score alters commit authority (A7).** They may reorder, annotate, throttle — never accept.
9. Before manually merging a SovMem implementation pull request, update `docs/sovmem-build-state.json`, regenerate `docs/SOVMEM-BUILD-AUDIT.md`, pass `npm run audit:sovmem:check` and `npm test` at the reviewed head, and review the implementation and audit together. This repository currently relies on manual pull-request merge discipline; documentation and audit evidence must not represent branch protection, required checks, rulesets, or merge queues as enforced unless independently verified later.

**Stop conditions:** any tripwire; a gate failing twice on one cause; plan/spec conflict; any step > 1 session over budget.

**Commit format:** `P<phase>.<step>: <what> [gate: pass|pending]`.

---

## 9. Success metrics and stop criteria

**Leading (dogfood weeks 1–4):** six-step demo green; verdicts/week ≥ 20; P@5 ≥ threshold; rebuild < 60s @ 10k docs; zero canon writes outside the verdict path; staging queue median ≤ 25; sentry green nightly; weekly audit sample (≥5, blindspot-biased) read; **weekly trust report generated**.
**Lagging (quarter):** % of agent sessions citing vault doc ids; promotions *and demotions* via the daily-court gradient; **per-source calibration gaps narrowing (or sources with widening gaps throttled)**; Khalil's own SSA-CMM Accountability + Security readings move.

**Stop criteria (armed at P5 cutover):**
- Verdicts/week < 10 for 2 consecutive weeks → pause loop write sources; friction retro.
- Staging queue > 50 for 3 consecutive days → same (R9).
- Sentry red 2 consecutive nights unresolved → freeze canon writes.
- **Calibration gap for any active source widens 2 consecutive windows → that source's quota halves and its proposals get mandatory skeptic review until the trend reverses (attention, not authority).**

---

## 10. Contrarian view

1. **The real competitor is `grep` + Obsidian.** v0.3 loses to the status quo being good enough. The P2 retrieval gate and the R1 friction KPI remain the two hinges.
2. **The build is the proof artifact.** The private Nous repository and its reviewable hash-chained verdict ledger are the thesis running; publication is a separate future decision.
3. **Verification can become its own theater.** The hash chain protects against tampering, not against you. Reasons taken seriously, provenance visible, audit sample actually read.
4. **The playbook's memory is state, not taste.** Loop-engineering persistence keeps facts alive; only judgment compounds. Own the organ, not the organism.
5. **The trust threshold's honest output is a defense of distrust (new).** Run the cost-benefit at solo scale and the optimal policy for canon writes is escalate-always — the escalation literature, applied to this workload, *derives* A2 rather than eroding it. The valuable product isn't a gate; it's a mirror: measured calibration for every proposing agent, and — uncomfortably — for the judge, whose inconsistency shows up as noise across every source's curve. The ledger scores you too.

---

## Appendix A — Loop-facing contract (v0.3)

- **Read:** `memory_search`, `memory_get` — unrestricted.
- **Write:** `memory_propose` only; quota, dedup, PolicyEvaluator apply. No other write path exists.
- **Confidence protocol (new):** a proposing source SHOULD attach `confidence` = its estimate of P(this proposal is ACCEPTed as-is), elicited **before drafting the artifact** (pre-execution estimates discriminate better than post-hoc self-review). Omitting it is allowed; inflating it is self-defeating — calibration is scored.
- **Skeptic protocol (new):** the evaluator agent's stance is adversarial bug-finding — assume the proposal is wrong, list what fails — not "verify correctness." Its block rides the proposal as advice.
- **Working state stays loop-side.** Only judged doctrine crosses the verdict gate.
- **sovmem's own loop is the sentry.** Loops must not schedule their own reindexing.
- **Failure-mode coverage:** amnesiac → canon + ledger survive flushes; nodding → skeptic + human verdict; manual → sentry cron; blind/tangled → harness-layer, out of scope.

## Appendix B — Gap map: Loop Engineering synthesis → sovmem (v0.2, retained)

Accepted: skeptic evaluator (advisory) · provenance fields · flood control · `audit --sample` · nightly sentry · kill criteria · axiom A6 · SKILL.md runbook. Rejected: orchestration in-scope · evaluator in-binary · loop state as canon. Exceeds: integrity-chained persistence; non-removable human door; reasons as compounding asset.

## Appendix C — Gap map: Trust Threshold references → sovmem (v0.3)

| # | Gap surfaced | Verdict | Landed as |
|---|---|---|---|
| 1 | No confidence capture (P(IS)) | ACCEPT | Optional pre-draft `confidence` in `.meta.json` (D12) |
| 2 | No trust/calibration metrics | ACCEPT | `trust` command — deterministic ledger arithmetic (A6-safe) |
| 3 | Optimal threshold implicit | ACCEPT | Explicit cost ratio in `policy.toml` (D11); escalate-always becomes a derived result |
| 4 | Skeptic stance unspecified | ACCEPT | Adversarial bug-finding default (Q9) — best-calibrated framing |
| 5 | No blindspot surfacing | ACCEPT | Calibration-residual report by (source × layer); biases audit, feeds demotion |
| 6 | Static oversight posture | ACCEPT (attention only) | Trust-ordered queue, quota recommendations, mandatory-skeptic routing |
| 7 | No evidence path to graduated autonomy | ACCEPT (roadmap) | **E7 — graduated trust**, entry criteria: ≥500 verdicts on a doc class, calibration gap < 5pp over 3 windows, FP rate ~0, *and* an explicit human decision to re-run D11 for that class |
| 8 | Trust-gated auto-commit in v0.x | REJECT | Cost asymmetry (poisoned canon ≫ one verdict) makes escalate-always optimal at solo scale; A7 |
| 9 | Model-estimated thresholds in-binary | REJECT | A6; trust math is ledger arithmetic only |
| 10 | Trusting self-reports | REJECT | Agents succeeding ~22% report ~77%; confidence is data *about* the source, not evidence about the proposal |

**Alignment with the governance references:** the verdict gate = approval for consequential actions; the ledger + provenance = legibility and attributability; stop criteria = interruptibility; PolicyEvaluator + quotas = layered deterministic guardrails with human-in-the-loop as the terminal safeguard. The oversight literature's observed arc — from per-action approval toward monitor-and-intervene as trust accumulates — is exactly what the ledger instruments for, and exactly what E7 gates on evidence.

---

## Consolidated reference

The complete set of approved SovMem designs, implementation plans, audits, and product PRDs has been consolidated into [`docs/sovmem-master-spec.md`](sovmem-master-spec.md). Archived source documents are preserved in [`docs/archive/`](archive/).

## Sources (provenance-tagged)

- *Act or Escalate? Evaluating Escalation Behavior in Automation with Language Models* — arXiv:2604.08588 — **tier1** (verified on arxiv.org 2026-07-07).
- *Agentic Uncertainty Reveals Agentic Overconfidence* — Kaddour et al., arXiv:2602.06948 — **tier1** (verified).
- *Demystifying evals for AI agents* — Anthropic Engineering, Jan 2026 — **tier1**.
- *Measuring AI agent autonomy in practice* — Anthropic Research, Feb 2026 — **tier1**.
- *Practices for Governing Agentic AI Systems* — OpenAI white paper (§4 practices: constrained action-space + approval, legibility, monitoring, attributability, interruptibility) — **tier1**.
- *A Practical Guide to Building AI Agents* (Guardrails section) — OpenAI — **tier1**.
- *Loop Engineering* Orange Book — HuaShu — **synthesis**.

---

*Plan version 3.2 — 2026-07-19. Supersedes v0.2. June 23 source lineage is preserved in the accepted final recovery; this plan controls execution. P0–P5 are delivered, the frozen Q3 gate remains passed, and v0.3 is tagged at commit 3e0817f. Escalation/overconfidence references are measurement foundations, not authorities over A2.*
