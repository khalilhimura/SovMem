> **ARCHIVED** — This document has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix B: Archive Map](../sovmem-master-spec.md#appendix-b-archive-map) for the master spec section that covers its content.
>
# SovMem Build Audit Report Design

**Date:** 2026-07-12

**Status:** Approved

**Owner:** Nous / SovMem

**Authority:** Implementation-supporting design; the v0.3 execution plan remains normative

> **2026-07-12 gate-closure supersession:** The approved P0/P1 gate-closure design moves reviewed evidence to `sovmem-build-audit-v2` and records the repository's current private, manual pull-request merge discipline. The workflow remains useful evidence, but this repository does not claim branch protection, required checks, rulesets, or merge queues are enforced.

## Purpose

Create one durable, readable audit artifact that answers how much of SovMem v0.3 is built, what evidence supports that assessment, what remains, and what should happen next. Keep the report synchronized with every SovMem implementation pull request so merged code and its status assessment arrive on `main` as one reviewed unit.

The report is navigational and evidentiary. It does not override phase definitions, gates, decisions, or invariants in `docs/sovmem_v0.3_execution_plan.md`.

## Selected approach

Update the audit report inside each relevant pull request before merge. A deterministic generator produces the Markdown report from reviewed structured state plus a fingerprint of the SovMem source tree. Continuous integration fails when the committed report is stale or invalid.

This approach was selected over a post-merge bot commit because it keeps implementation, status, review, and history atomic. `main` never intentionally contains a completed SovMem change with a known-stale audit report.

## Artifacts

### Human-readable report

`docs/SOVMEM-BUILD-AUDIT.md` is the primary reference for humans and agents. It contains:

- the overall weighted completion percentage;
- P0 through P5 completion percentages and formal gate states;
- completed capabilities and remaining gaps;
- current corpus, golden-query, test, and verification evidence;
- active blockers and open decisions;
- the recommended next build target;
- a deterministic source-tree fingerprint;
- phase-publication history.

The report declares its generated status and points readers to the state file and regeneration command. Direct manual edits are unsupported because the next generator run replaces them.

### Reviewed state

`docs/sovmem-build-state.json` is the human-reviewed status input. JSON is used because Node can validate and consume it without adding dependencies.

The top-level object contains:

- `schema_version`: fixed at `sovmem-build-audit-v2`;
- `spec_version`: fixed at `v0.3` for this report series;
- `as_of`: explicit `YYYY-MM-DD` review date;
- `summary`: concise current-state statement;
- `next_step`: concise recommended next build target;
- `blockers`: sorted array of unresolved blocking decisions or conditions;
- `evidence`: current test totals; corpus warnings and exclusions; golden-query BLAKE3; frozen-threshold identity; explicit Level 3 outcome; bundle BLAKE3; and the reviewed implementation commit;
- `phases`: exactly six ordered entries, `P0` through `P5`;
- `history`: ordered phase-publication records.

Each phase entry contains:

- `id`, `name`, and execution-plan effort weight;
- integer `completion_percent` from 0 through 100;
- `gate`: `not-started`, `in-progress`, `blocked`, or `passed`;
- concise `completed` and `remaining` arrays;
- `evidence` references using repository-relative paths, commands, test counts, or commit identifiers.

A phase may use `gate: passed` only at 100% completion and only when its evidence array is non-empty. A phase below 100% cannot claim `passed`. Completion percentages remain reviewed judgments rather than metrics inferred from lines of code.

### Generator

`scripts/generate-sovmem-build-audit.mjs` owns validation, calculation, fingerprinting, rendering, writing, and checking.

It supports two modes:

- default mode writes the canonical report;
- `--check` generates in memory, compares exact bytes with the committed report, and exits nonzero when missing or stale.

The generator must:

1. validate the state schema and reject unknown or missing fields;
2. enforce the six fixed execution-plan phase identifiers and weights;
3. calculate overall completion as the weighted mean of phase completion percentages using the execution plan's 13.5-session total;
4. round the displayed overall result to the nearest whole percentage while preserving phase integers;
5. fingerprint the tracked SovMem implementation surface deterministically;
6. render stable Markdown with one terminal newline;
7. avoid timestamps, absolute paths, environment-specific values, generated `var/` output, and Rust build output;
8. emit concise errors without serializing private corpus content.

## Source-tree fingerprint

The fingerprint changes whenever the reviewed SovMem implementation surface changes, even if the phase percentage does not.

The input set consists of tracked files under:

- `sovmem/`, excluding `sovmem/target/`;
- `profiles/`;
- `evals/`;
- `docs/sovmem_v0.3_execution_plan.md`;
- `docs/spec/source-lineage-june-23-spec-bundle-verbatim.md`;
- root `package.json`, `package-lock.json`, and `rust-toolchain.toml`.

The generator obtains the tracked file list from Git, sorts repository-relative paths bytewise, and hashes a framed stream containing each path and its file bytes. SHA-256 from Node's standard library is sufficient because the fingerprint detects report staleness; it does not replace SovMem's BLAKE3 bundle-integrity contract.

The Markdown report includes the resulting digest and labels it as a source-tree fingerprint, not a canon or bundle hash. Merge commits do not alter the fingerprint when their tree matches the reviewed pull-request head.

## Update and publication flow

For every pull request that changes the fingerprinted SovMem surface:

1. update `docs/sovmem-build-state.json` when completion, evidence, blockers, or next-step judgment changed;
2. run `npm run audit:sovmem`;
3. review and commit both structured state and generated Markdown as applicable;
4. run repository tests and `npm run audit:sovmem:check`;
5. push, open the pull request, and review the implementation and regenerated audit together;
6. manually merge only after `npm run audit:sovmem:check` and `npm test` pass at the reviewed head.

Any source change alters the fingerprint, so even a code change that leaves the reviewed phase percentages unchanged requires regeneration and a report diff. A completed build phase additionally requires a new history entry and updated phase evidence before publication.

## Commands and continuous integration

Root package commands:

- `npm run audit:sovmem` regenerates the report;
- `npm run audit:sovmem:check` verifies exact freshness without writing.

A GitHub Actions workflow for pull requests and merge groups runs on Node 20 and performs:

1. checkout with sufficient Git history for tracked-file enumeration;
2. `npm ci`;
3. `npm run audit:sovmem:check`;
4. `npm test`.

The workflow does not write to the branch, commit, push, or mutate the audit state. The report remains a reviewed pull-request artifact. It runs for pull requests and merge groups and supplies useful read-only evidence. This repository currently relies on manual pull-request merge discipline; the workflow's presence does not establish or prove branch protection, required checks, rulesets, or merge queues.

## Initial audit baseline (historical)

The first state file records the live assessment already verified against merged `main`:

- P0: 85%;
- P1: 55%;
- P2: 10%;
- P3: 0%;
- P4: 0%;
- P5: 0%;
- weighted overall completion: 16%;
- corpus: 62 accepted documents, comprising 8 core and 54 research documents;
- golden set: 25 human-approved queries covering core and four research themes;
- verification baseline: 6 JavaScript and 185 Rust tests;
- active bundle hash: `49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65`;
- merged corpus milestone: merge commit `326a8e7f5a648f8da48b45ce307d30660dd07e10`.

The initial baseline distinguishes completed preparatory milestones from formal execution-plan phase gates. It must not describe the corpus milestone as retrieval, ranking, write-back, MCP, trust instrumentation, or full SovMem completion.

## Gate-closure audit baseline

The v2 state supersedes the initial percentage and evidence shape while preserving its history:

- P0: 100%, passed;
- P1: 100%, passed;
- P2: 10%, in progress;
- P3-P5: 0%, not started;
- weighted overall completion: 24%;
- corpus: 62 accepted documents, comprising 8 core and 54 research documents, with 226 warnings, one exclusion, and zero errors;
- golden set: 25 human-approved queries with pinned BLAKE3 identity;
- frozen Q3 thresholds: `k=5` and at least 20 of 25 hit queries;
- explicit Level 3: 62 documents, 226 warnings, zero errors;
- the independently verified bundle BLAKE3 and implementation commit.

The gate-closure baseline does not claim P2 retrieval, ranking, query execution, or an evaluation baseline exists.

## Root index and execution-plan integration

The same change registers the report, state file, generator, tests, and workflow in `index.md` with owner, type/status, purpose, and relationship.

The execution plan receives a short executor-protocol rule: before publishing a completed build phase through push, pull request, and merge, update reviewed audit state, regenerate the report, and pass its freshness check. This procedural addition does not change any phase gate or technical requirement.

## Failure behavior

The generator fails closed for:

- invalid JSON or schema version;
- missing, extra, duplicate, or out-of-order phases;
- invalid weights, percentages, or gate states;
- a passed gate below 100%;
- missing evidence for a passed gate;
- malformed dates or history entries;
- missing fingerprint inputs;
- untracked files inside the configured fingerprint surface when referenced by state;
- stale or missing Markdown in check mode.

Failures identify the field or invariant without printing corpus bodies, absolute private paths, or environment values.

## Testing

Node tests cover:

- the approved gate-closure weighted calculation (`24%`);
- stable output and terminal newline;
- source-tree fingerprint stability and change detection;
- exact six-phase ordering and weights;
- percentage, gate, evidence, date, and unknown-field validation;
- passed-gate consistency;
- stale-report detection in `--check` mode;
- absence of absolute workspace paths and restricted generated directories;
- required report sections and authority disclaimer;
- root command wiring;
- workflow command wiring;
- index registration and execution-plan publication rule.

The full existing JavaScript and Rust suite remains the regression gate.

## Security and authority boundaries

- The report never contains Audy Vault absolute paths or document bodies.
- Generated corpus output remains ignored and is not fingerprinted as authority.
- The structured state is reviewed human judgment; the generator does not claim semantic phase completion autonomously.
- CI verifies freshness and consistency but cannot promote a phase to passed.
- The execution plan remains the normative build specification.
- The report cannot alter canon, verdict authority, retrieval behavior, or any SovMem runtime decision.

## Acceptance criteria

The design is implemented when:

1. the baseline report renders deterministically from validated structured state;
2. its weighted percentage and evidence match the approved live assessment;
3. a fingerprinted source change makes check mode fail until the report is regenerated;
4. invalid or overstated phase state fails closed;
5. root commands, tests, workflow, index, and execution protocol are wired;
6. the complete repository test suite and audit freshness check pass;
7. the implementation is published through a reviewed pull request with the report included before manual merge.

Before manually merging a SovMem implementation pull request, update `docs/sovmem-build-state.json`, regenerate `docs/SOVMEM-BUILD-AUDIT.md`, pass `npm run audit:sovmem:check` and `npm test` at the reviewed head, and review the implementation and audit together. This repository currently relies on manual pull-request merge discipline; documentation and audit evidence must not represent branch protection, required checks, rulesets, or merge queues as enforced unless independently verified later.
