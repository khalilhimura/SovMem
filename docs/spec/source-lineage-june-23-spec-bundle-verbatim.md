# Source Lineage — June 23 Spec Bundle (Verbatim Recovery, rev. 2)

**What this is:** The sovmem execution plan (v0.1→v0.3) cites a six-file specification bundle produced 2026-06-23 in the session "Knowledge graph architecture for multi-agent memory systems." The original files lived in that session's ephemeral container (risk R8). This document is the recovered bundle — **rev. 2, after a six-query transcript-mining pass on 2026-07-07** (Drive connector unavailable; retried and errored).

**Recovery status:** `index.md` FULL · `04_workstream_plans.md` ~FULL · `03_PRD_mvp.md` ~70% (tail full, front missing) · `05_post_mvp_enterprise.md` ~60% · `02_oss_stack_primitives.md` ~50% · `01_architecture_and_limits.md` fragments only. Two of four closing conclusions.

**Decision — single file:** unchanged. Recovery is high but not total; one consolidated file with per-section fidelity markers stays the honest form. Fidelity legend: **[VERBATIM]** word-for-word · **[SUMMARY]** known via summaries/cross-refs · **[UNRECOVERED]** existence known, text lost.

**Provenance:** `secondary` (faithful transcript excerpts of a primary internal artifact).

---

## 0 · `index.md` — [VERBATIM, FULL]

```markdown
---
okf_version: "0.1"
---

# Sovereign Multi-Agent Memory — Deliverable Bundle

This bundle is itself conformant OKF v0.1 — the deliverable practices the format
it specifies. Read in order.

# Analysis & Specification

* [01 Architecture & OKF Limits](01_architecture_and_limits.md) - Is the
  KG→Rust CLI→MCP→Harness→Human chain sound? Theoretical size/depth/latency
  bounds under the real OKF v0.1 spec, plus ranked gap analysis (G1–G7).
* [02 OSS Stack Primitives](02_oss_stack_primitives.md) - Concrete open-source
  building blocks per layer; what to adopt (commodity) vs. build (defensible).
  The six-crate Rust spine.
* [03 PRD — MVP](03_PRD_mvp.md) - Product requirements for the MVP: data model,
  the Judgment concept, functional/non-functional requirements, and the
  six-step demo that proves the thesis.
* [04 Workstream Plans](04_workstream_plans.md) - Six sequenced workstreams
  (W1–W6) with dependencies, deliverables, and exit criteria, mapped to the gaps.
* [05 Post-MVP — Enterprise Scale](05_post_mvp_enterprise.md) - Features
  deferred from MVP that become mandatory at enterprise scale (E1–E6), ordered
  by when each assumption breaks.

# The thesis in one line

The format is a commodity (OKF); the defensibility is the typed judgment index
plus the verified write-back loop. Truth lives in the bundle; intelligence lives
in the index and the loop.
```

Delivery check (verbatim from session): all files passed an OKF conformance spot-check — parseable frontmatter + non-empty `type`.

## 1 · Closing conclusions — [VERBATIM ×2 · UNRECOVERED ×2]

> **On the path** — the spine (durable state → Rust → MCP → harness → human) is correct, but two joints were mis-specified. Your "KG" node is doing two jobs that must be split: the OKF bundle (portable substrate, where `:: reason` lives) and a derived index (your proprietary, fast, disposable query layer). And "Human" isn't a leaf — it's a source whose verdicts must write *back* with verified integrity. **The chain is a loop, not a line.**
>
> **On OKF limits** — the format imposes almost nothing by design; conformance is just parseable frontmatter + non-empty `type`. That's the trap and the opening: **you can't be defensible by conforming better.**

Conclusions three and four: **[UNRECOVERED]**.

## 2 · `01_architecture_and_limits.md` — [SUMMARY + cross-ref fragments]

Body (bounds analysis §2.x, ranked G1–G7 list): **[UNRECOVERED]**. Reconstructed from cross-references elsewhere in the bundle:

| Gap | Meaning (from cross-refs) | Closed by |
|---|---|---|
| G1 | Fast lexical retrieval over the bundle | tantivy inverted index (W2.1) |
| G2 | Typed relations + queryable judgments | petgraph typed edges (W2.2); `type: Judgment` |
| G3 | Write integrity / verified append | blake3 manifest (W2.5) + PolicyEvaluator + git2 commit (W3) |
| G4 | Orchestration by knowledge layer | Issue #93 `layer:` convention, synthesis-first loading (W2.4) |
| G5 | **[UNRECOVERED]** | — |
| G6 | Staleness / recency in ranking | Recency-decay ranking over `timestamp` (W2.3) |
| G7 | Retrieval quality regression | Golden-set eval as CI gate, P@5/R@5 (W5) |

Recovered §-refs: **§2.3** — LLM query planning is the latency-dominant stage; optimize retrieval *precision* (fewer retries), not milliseconds. **§2.4** — concurrency ceiling; file lock suffices at solo scale; `automerge` CRDT or write queue post-MVP. Recovered acceptance bound: depth-6 typed traversal fitting a 200K context budget at branching factor ≤ 10.

## 3 · `02_oss_stack_primitives.md` — [VERBATIM, ~50%]

Layers 1–2 tables (substrate, index): **[UNRECOVERED]** — contents inferable from the manifest below. Layer 3 tail recovered:

> The MCP layer is a dumb adapter so the same CLI can be driven by a shell, a cron job, or any harness. If you find yourself writing logic in the MCP server, it belongs one layer down.

**Layer 4 — Agent Harness (consumption) — [VERBATIM]:**

| Need | OSS primitive | Adopt / Build | Notes |
|---|---|---|---|
| Harness | OpenClaw / Claude Code / Codex (your existing five-harness stack) | Adopt | The harness is a *consumer* of the MCP tools; it is interchangeable |
| Orchestration by layer | Issue #93 `layer` field convention | **Adopt the convention now** | Closes G4. Tag concepts `layer: synthesis/analysis/concept`; orchestrator loads synthesis first |
| Query planning | the harness's own LLM | Adopt | This is the latency-dominant stage (§2.3); optimize precision, not speed |
| Verifier sub-agent | planner–generator–evaluator pattern (Anthropic harness) | Adopt | A verifier sub-agent checks retrieved context before reasoning — outperforms self-critique |

> **Build nothing here.** The harness is deliberately swappable. If your memory system only works with one harness, you've coupled wrong.

**Layer 5 — Human Judgment Capture (write-back) — YOUR SECOND MOAT — [VERBATIM]:**

| Need | OSS primitive | Adopt / Build | Notes |
|---|---|---|---|
| Verdict grammar | your `ACCEPT/REJECT/REWRITE/TP/FP :: reason` | **Build** (it's already yours) | This is the irreplaceable asset; no primitive exists because it's domain judgment |
| Judgment as OKF concept | `type: Judgment` doc convention | **Build** | Each verdict → its own concept with typed frontmatter (`verdict`, `target`, `reason`) so it's queryable (closes G2) |
| Verified append | `blake3` hash + `git2` commit + PolicyEvaluator | **Build** | Closes G3. Only the human-judgment path may write `type: Judgment` docs |
| Capture UI | Obsidian (your Audy's Vault) as the human-facing editor | Adopt | Obsidian *is* an OKF consumer/producer — frontmatter + cross-links map 1:1 |
| Concurrency (post-MVP) | `automerge` (CRDT, Rust) **or** a write queue | Build/Adopt | Closes G3/concurrency ceiling (§2.4). For solo MVP, a simple file lock suffices |

> **Build the write-back loop. This is moat #2.** The judgment capture + verified write is what turns OKF from an encyclopedia into a memory. Obsidian is the only adopt here because it already speaks the format.

**Consolidated dependency manifest ("Six load-bearing crates") — [VERBATIM]:**

```toml
[dependencies]
# Substrate
git2 = "*"              # git-native bundle ops
serde_yaml = "*"        # frontmatter
pulldown-cmark = "*"    # body + link extraction
# Defensible core
tantivy = "*"           # inverted index (G1)
petgraph = "*"          # typed adjacency graph (G2)
blake3 = "*"            # integrity hashing (G3)
notify = "*"            # incremental reindex
clap = "*"              # CLI
# Protocol
rmcp = "*"              # MCP server (thin adapter)
# Optional semantic layer (defer until proven necessary)
# fastembed = "*"
# usearch = "*"
```

## 4 · `03_PRD_mvp.md` — [VERBATIM, ~70%]

Original CLI name: **`smm`**. §1–§5 (incl. §5.1 concept-type vocabulary / data model), FR-1, FR-2: **[UNRECOVERED]**.

**FR-3 (tail) — typed traversal:** output the typed subgraph, depth-controllable. *Acceptance: depth-6 typed traversal returns only relevant-edge concepts, fitting in a 200K context budget for branching factor ≤ 10.*

**FR-4 — Judge (`smm judge` / MCP `memory_judge`):** input verdict (`ACCEPT/REJECT/REWRITE/TP/FP`), a `target` concept ID, a `reason`. Effect: mints a `type: Judgment` concept, verified-writes it, reindexes. *Acceptance: judgment queryable by `verdict` and `target` within one reindex cycle; write rejected if PolicyEvaluator fails (FR-6).*

**FR-5 — Get concept (`smm get` / `memory_get_concept`):** return raw markdown of a concept by ID for the harness to load into context. *Acceptance: O(1) by path.*

**FR-6 — Verified write-back (PolicyEvaluator):** every write (agent-proposed or human judgment) passes: (1) conformance check (parseable frontmatter, non-empty `type`); (2) integrity check (no overwrite of an existing concept whose hash doesn't match last-known-good unless the writer is privileged); (3) privilege check (`type: Judgment` writes require the human-judgment path; agent proposals of judgments are rejected). *Acceptance: a simulated poisoning write (malformed, or an agent forging a Judgment) is rejected and logged.*

**FR-7 — Retrieval eval (`smm eval`):** a golden set of `intent → expected concept IDs`. Runs as CI gate. *Acceptance: precision@5 and recall@5 reported; build fails below threshold (closes G7).*

**FR-8 — MCP server (`smm serve`):** thin `rmcp` adapter exposing FR-2..FR-5 as tools over stdio. *Acceptance: any harness can drive the memory with zero memory-specific code in the harness.*

**§7 Non-functional requirements — [VERBATIM]:**

| NFR | Target | Rationale |
|---|---|---|
| Index-layer query latency | <50 ms @ 10⁵ concepts | Keep retrieval off the LLM critical path (§2.3) |
| Reindex (incremental) | <100 ms / changed concept | Write-back must feel instant |
| Cold full reindex | <60 s @ 10⁵ concepts | Disposable index must be cheap to rebuild |
| Memory footprint | Runs on Himura-Mini (16GB) | Sovereign = self-hostable on the operator's own machine |
| Zero external services | No DB server, no cloud dependency | Sovereignty axiom; `tantivy`/`petgraph` are embedded |
| Auditability | Every judgment traceable to a human + reason | The moat is only a moat if it's inspectable |

**§9 The six-step demo — [VERBATIM from step 2; step 1 truncated in recovery]:**

> 1. *[truncated — seeds the vertical bundle / agent receives a categorization task]*
> 2. AI Employee answers a categorization query via `memory_query` — correct, <50 ms retrieval, cites the taxonomy rule.
> 3. Human disagrees, issues `REJECT :: reason` via Obsidian → `smm judge`.
> 4. Verified write-back mints a `Judgment` concept; reindex.
> 5. Same query re-run — the AI Employee now retrieves the Judgment *and* the original rule, and changes its answer, citing the human reason.
> 6. A simulated poisoning write (agent forging a Judgment) is **rejected**.
>
> If steps 1–6 pass, the thesis ("format is commodity, index + loop are product") is demonstrated.

**§10 Explicit risks & mitigations — [VERBATIM]:**

| Risk | Likelihood | Mitigation |
|---|---|---|
| Typed-edge extraction from prose is unreliable | High | Anchor on the `target` frontmatter field for Judgments (deterministic); treat prose-edge typing as best-effort enrichment, not a correctness dependency |
| Bundle outgrows flat `index.md` | Medium | Hierarchical index generation; adopt Issue #93 `layer` field from day one |
| LLM query-planning dominates latency | Certain | Optimize retrieval *precision* (fewer retries) rather than chasing ms; this is the §2.3 insight |
| Index drifts from bundle | Medium | `notify`-driven incremental reindex + `blake3` manifest detects drift |
| Solo-maintained stack rots | Medium | Keep dependency list to 6 crates; everything regenerable from the bundle |

**§11 Open questions to resolve before build — [VERBATIM]:**

> - Q1: Is lexical + typed retrieval sufficient, or is a semantic layer needed for the target vertical? (Resolve empirically with FR-7 golden set before adding `fastembed`.)
> - Q2: What is the exact privileged-writer mechanism for Judgments on a solo setup — a signed commit, a local key, or a path-based gate? (Simplest that is tamper-evident wins.)
> - Q3: Does the `reason` field need a controlled sub-grammar (e.g., citing the governing standard like MFRS) to be queryable, or is free text acceptable for MVP? (Free text for MVP; structure later if eval shows retrieval suffers.)

## 5 · `04_workstream_plans.md` — [VERBATIM, ~FULL]

**Frontmatter:**

```yaml
---
type: Playbook
title: Workstream Plans — Sovereign Multi-Agent Memory MVP
description: Six sequenced workstreams from substrate to verified write-back, each with dependencies, deliverables, and exit criteria. Mapped to the gaps in 01_architecture_and_limits.md.
tags: [workstream, plan, mvp, sequencing]
timestamp: 2026-06-23
---
```

**Sequencing logic:** build **bottom-up along the spine**: substrate → index → write-back → protocol → consumption → eval. Each workstream produces a runnable artifact. Do not start a workstream until its dependency's exit criteria are met. Critical path W1 → W2 → W3; W4/W5/W6 can partially parallelize once W2 lands.

```
W1 Substrate ──▶ W2 Index Core ──▶ W3 Verified Write-Back ──▶ W6 Demo Loop
                      │                    │
                      ├──▶ W4 MCP Adapter ─┤
                      └──▶ W5 Eval Harness ┘
```

**W1 — Substrate & Conformance** (closes nothing yet; enables everything). Goal: a conformant OKF bundle and a parser that reads it correctly. Deps: none. Tasks: W1.1 define the concept-type vocabulary (PRD §5.1) as documented convention · W1.2 hand-author a seed bundle (~50 concepts) for one vertical; include at least 5 `Judgment` docs by hand to lock the schema · W1.3 integrate Google's `okf-validate.mjs` as a conformance check · W1.4 Rust: `serde_yaml` frontmatter parser + `pulldown-cmark` body/link extractor → in-memory model of (concept-ID, frontmatter, links) · W1.5 `smm validate` subcommand wrapping W1.3 + W1.4. Deliverable: `smm validate <bundle>` passes on the seed bundle; fails loudly on a deliberately malformed concept. **Exit:** parser round-trips every seed concept; conformance gate green; a malformed doc (missing `type`) rejected with a precise error.

**W2 — Index Core** (closes G1, G2, G4, G6) — **THE DEFENSIBLE CORE**. Goal: fast typed retrieval that decouples query latency from bundle size. Deps: W1. Tasks: W2.1 `tantivy` inverted index over body + **every** frontmatter field (so `verdict:REJECT` and `target:...` are queryable) [G1] · W2.2 `petgraph` typed adjacency list; edges from markdown links; type inferred from (a) the `target` field for Judgments [deterministic], (b) best-effort prose parsing for the rest [G2] · W2.3 recency-decay ranking function over `timestamp` [G6] · W2.4 hierarchical index generation + adopt Issue #93 `layer` field so an orchestrator can load `layer: synthesis` first [G4] · W2.5 `blake3` integrity manifest (concept-ID → hash) [feeds G3 in W3] · W2.6 `notify`-driven incremental reindex · W2.7 subcommands `smm index`, `smm query`, `smm traverse`. Deliverable: `smm query`/`smm traverse` answer against a 10⁵-concept synthetic bundle. **Exit (NFR gates):** query <50 ms @ 10⁵; `verdict:REJECT target:taxonomy/*` returns only matching Judgments; depth-6 typed traversal relevant-edge-only; incremental reindex <100 ms; cold reindex <60 s @ 10⁵.

**W3 — Verified Write-Back & Judgment Capture** (closes G3) — **MOAT #2**. Goal: memory that can be written safely and captures human judgment durably. Deps: W2 (integrity manifest + reindex). Tasks: W3.1 `type: Judgment` mint flow: verdict + target + reason → conformant doc · W3.2 PolicyEvaluator (PRD FR-6): conformance + integrity + privilege checks · W3.3 privileged-writer mechanism for Judgments (resolve PRD Q2 — simplest tamper-evident option; e.g., path-gated + signed commit) · W3.4 `git2` verified commit: hash, policy-check, then commit + reindex atomically · W3.5 `smm judge` subcommand · W3.6 poisoning red-team: scripted attempts (malformed doc; agent forging a Judgment; overwriting a concept with mismatched hash) must all be rejected. Deliverable: `smm judge --verdict REJECT --target <id> --reason "..."` mints a queryable Judgment; the red-team script reports all attacks blocked. **Exit:** all FR-6 checks pass; every red-team write rejected and logged; a legitimate human judgment queryable within one reindex cycle.

**W4 — MCP Adapter** (closes nothing new; enables consumption). Goal: any harness drives the memory with zero memory-specific code. Deps: W2 + W3. Tasks: W4.1 `rmcp` server scaffold, stdio transport · W4.2 tools `memory_query`, `memory_traverse`, `memory_get_concept`, `memory_judge` — each a thin wrapper over the CLI, **no logic in this layer** · W4.3 pre-dispatch PolicyEvaluator hook (same evaluator as W3.2) on the `judge` tool, so MCP-originated writes are gated identically to CLI writes · W4.4 `smm serve`. Deliverable: connect one harness (Claude Code or OpenClaw) to `smm serve`; it queries and traverses with no bespoke integration. **Exit:** a harness completes query→traverse→get over MCP; an MCP-originated forged-Judgment write is rejected by the same policy as W3.

**W5 — Retrieval Eval Harness** (closes G7). Goal: measurable retrieval quality, wired as a CI gate. Deps: W2; can start in parallel once W2.7 lands. Tasks: W5.1 author a golden set `intent → expected concept IDs` (~30 intents) · W5.2 `smm eval` computes precision@5 / recall@5 · W5.3 CI gate: build fails below threshold (**start: precision@5 ≥ 0.8**) · W5.4 use eval to resolve PRD Q1 — lexical+typed vs semantic layer; **decision is data-driven, not assumed**. Deliverable: `smm eval` prints a scorecard; CI red below threshold. **Exit:** golden set runs in CI; the lexical-vs-semantic decision made on measured numbers, not vibes.

**W6 — Demo Loop** (proves the thesis). Goal: the end-to-end loop from PRD §9, demonstrable in one sitting. Deps: W2, W3, W4. Tasks: W6.1 seed the real vertical bundle (**~500 concepts; Bukku DuitNow taxonomy**) · W6.2 script the six-step loop (query → human REJECT → write-back → re-query → answer changes → poisoning rejected) · W6.3 capture latency + precision numbers for the record. Deliverable: a reproducible demo where the AI Employee changes its answer *because* a human judgment with a `:: reason` entered durable memory — and a poisoning attempt is visibly blocked. **Exit:** all six PRD §9 steps pass on the real vertical bundle.

**Effort shape (relative, not calendar — you're a solo operator) — [VERBATIM]:**

| Workstream | Relative effort | Risk | Why |
|---|---|---|---|
| W1 Substrate | S | Low | Mostly adopting existing parsers + Google's validator |
| W2 Index Core | **L** | **Med** | The product; typed-edge extraction is the hard part |
| W3 Write-Back | M | **High** | Integrity/privilege correctness is subtle; red-team it |
| W4 MCP | S | Low | Thin adapter by design |
| W5 Eval | M | Med | Golden-set authoring is the real cost |
| W6 Demo | S | Low | Integration, if W2/W3 are solid |

> Concentrate effort on **W2 and W3**. They are the two moats. W1/W4 are commodity; W5 is the discipline that keeps W2 honest; W6 is the proof.

**The one rule that governs all six — [VERBATIM]:**

> **Truth lives in the bundle; intelligence lives in the index and the loop.** If any workstream starts putting irreplaceable state into the index or the MCP layer instead of the OKF bundle, stop — it belongs in the bundle, where a human can `cat` it and a regulator can audit it. The disposability of the index is the sovereignty guarantee.

## 6 · `05_post_mvp_enterprise.md` — [VERBATIM, ~60%]

E1–E5 section bodies: **[UNRECOVERED]**. E6 recovered:

> **Feature:** Resource resolution layer. For `resource`-bound concepts, a resolver fetches and caches the live asset (schema from a live BigQuery table, a rendered PDF page) at query time. Keeps the bundle as curated *knowledge about* assets while binding to the *live* asset for current values — the RAG-vs-OKF distinction Google draws, operationalized. **Where it sits:** a resolver sidecar invoked by `memory_get_concept`. The bundle stores the curated concept; the resolver fetches volatile reality.

**Enterprise feature dependency map — [VERBATIM]:**

```
                 ┌───────────────────────────────────────────┐
   MVP (W1–W6) ──┤  bundle (truth) + index + verified loop    │
                 └───────────────────────────────────────────┘
                        │
   breaks 1st  ──▶ E1 Concurrency/Merge ──── extends W3
   breaks 2nd  ──▶ E2 ACL/Multi-tenancy ──── filters W2→W4
   breaks 3rd  ──▶ E3 Cross-org Exchange ─── substrate conventions
   ongoing     ──▶ E4 Governance/Audit ───── hardens W3 evaluator
   at 10⁷+     ──▶ E5 Distributed Index ──── re-architects W2
   when modal  ──▶ E6 Resource Resolver ──── sidecar on get_concept
```

**The enterprise thesis, compressed — [VERBATIM]:**

> The MVP proves that **judgment-bearing, verified memory** beats a plain bundle. Enterprise scale is mostly about **defending that same property against more writers, more trust boundaries, and more volume** — without ever sacrificing the two invariants that make it sovereign:
>
> 1. **The bundle is the truth and stays portable OKF** — so audit, exit, and interop remain possible no matter how large the deployment.
> 2. **The index and the loop carry the intelligence and stay regenerable** — so scaling them out never creates a new lock-in or a new source of truth.
>
> Every enterprise feature that respects those two invariants is additive. Any feature that violates them — by hiding truth in a proprietary index, or by making the bundle un-`cat`-able — trades the entire thesis for a quarter's convenience. Don't.

## 7 · Deltas: original spec → plan v0.3 (updated with rev. 2 recovery)

1. **Naming:** `smm` → `sovmem` (both placeholders; D7 open).
2. **Incremental reindex:** original had `notify` + <100 ms incremental NFR; v0.3 chose full-rebuild-only. Original position is the pre-existing fallback if verdict-time reindex feels slow.
3. **git-native spine:** original W3.4 specified `git2` verified commits (hash → policy-check → atomic commit+reindex). **Direct evidence toward git-native in D3.**
4. **Crate layout:** "six-crate spine" = dependency manifest, not workspace. D5's "deviation" is nearly nil.
5. **`memory_judge` over MCP:** original FR-8/W4.2 exposed it (gated by the W4.3 pre-dispatch hook). v0.3 removed it entirely (A2 hardening). Intentional delta, on the record.
6. **Semantic layer:** deferred in both; original made it explicitly data-driven via W5.4/Q1. Continuity with D2.
7. **Demo centering (new):** original §9 demo is **REJECT-centric** — the answer changes because a human *rejected* — plus a visible poisoning block. v0.3's reconstruction was ACCEPT-centric. Recommend v0.4 restore the REJECT beat; a memory that only demos acceptance demos half the moat.
8. **Recency decay (new):** original G6/W2.3 ranked by timestamp decay. **Absent from v0.3.** Candidate v0.4 addition to the index (cheap, deterministic, A6-safe).
9. **Eval threshold (new):** original starting bar was **precision@5 ≥ 0.8** (W5.3). Directly seeds v0.3's open Q3.
10. **Reason sub-grammar (new):** original Q3 (free text for MVP; structure later if eval shows retrieval suffers, e.g., MFRS citations) — carry into v0.3's question register.
11. **Seed sizing (new):** original W1.2 = ~50 hand-authored concepts + ≥5 hand Judgments; W6.1 = ~500 concepts (Bukku DuitNow taxonomy). Corroborates v0.3's P0 "< 50 docs" tripwire and answers part of Q1.

## 8 · Still unrecovered

`01` body (size/depth/latency bounds §2.1–§2.4 in full; the ranked G1–G7 list itself; **G5's identity**) · `03` §1–§5 front (incl. §5.1 concept-type vocabulary), FR-1, FR-2, demo step 1 · `02` Layers 1–3 tables · `05` E1–E5 bodies · closing conclusions 3–4. Further transcript mining hit repeats; remaining recovery requires the verbatim originals (Obsidian / Drive when the connector recovers / local downloads). R8's P0 locate step stands; if nothing surfaces by P0 exit, this file + plan v0.3 are jointly canonical.

---

*Rev. 2 — recovered 2026-07-07 via six-query transcript mining of the 2026-06-23 session. Provenance: secondary. Unrecovered sections are marked, not paraphrased into existence.*
