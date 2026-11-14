> **ARCHIVED** — This document has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix B: Archive Map](../sovmem-master-spec.md#appendix-b-archive-map) for the master spec section that covers its content.
>
# SovMem P0/P1 Gate Closure and P2 Threshold Freeze Design

**Date:** 2026-07-12

**Status:** Human-approved design; committed specification review pending

**Owner:** Nous / SovMem

**Authority:** Implementation-supporting design. `docs/sovmem_v0.3_execution_plan.md` remains normative, and this document records the human decisions that close its P0/P1 blockers for this slice.

## Purpose

Close the formal P0 scaffold-and-ground-truth gate, close the formal P1 bundle-substrate gate, and freeze the Q3 retrieval evaluation thresholds before any P2 retrieval implementation begins.

This slice turns the already reviewed corpus, golden queries, exporter, and offline verifier into explicit phase contracts. It does not build retrieval. Tantivy, Petgraph, BM25, ranking, query execution, index construction, graph traversal, or evaluation scoring remain outside this slice.

The implementation must preserve the existing read-only Audy corpus workflow and current `verify` callers while adding explicit verification levels, deterministic statistics, corruption fixtures with stable codes, the official OKF reference pin, and a machine-readable threshold freeze.

## Verified starting point

The design is grounded in `origin/main` at commit `65c96b2543e21eebb7d0403827aa2772b2aaf7d7`, PR #4's merge commit.

At that commit:

- the audit reports 16% overall completion: P0 85%, P1 55%, P2 10%;
- the reviewed live corpus contains 62 accepted documents: 8 canonical core and 54 research;
- the active v1 corpus manifest contains 8 required core paths and 55 reviewed research candidates, with one research candidate excluded by the accepted live export;
- `evals/audy-sovmem-v0/golden.jsonl` contains exactly 25 human-approved queries and exactly one relevant document per query;
- the raw-byte BLAKE3 of the golden file is `f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175`;
- the current public CLI implements `export`, `corpus`, `verify`, and `verify-golden`;
- current verification already checks generated document hashes, the framed bundle root, profile identity, source/document correspondence, generated metadata, links, export evidence, and optional corpus evidence, but does not expose formal levels;
- the provenance enum is `tier1 | secondary | synthesis | unknown`;
- the crate remains one Rust package at `sovmem/Cargo.toml`;
- the repository is private and uses manual pull-request merge discipline.

Generated bundles remain ignored, derived output. Audy Vault remains the canonical source and is read only at the pinned source commit already declared by the active profile and corpus contract, `43824cfd9caed7b260aa442128fd1130ddb6e2ff`.

## Scope

### In scope

1. Record the remaining P0 decisions in the normative execution plan and supporting project documentation.
2. Pin the official Open Knowledge Format reference to an exact repository, commit, and normative file.
3. Add cumulative verification levels 1 through 3 while preserving current `verify` compatibility.
4. Make explicit `verify --level 3` the formal P1 corpus gate.
5. Add deterministic `stats` output for a verified bundle.
6. Add corruption coverage for the five approved failure classes with stable, scenario-specific error codes.
7. Add and validate `evals/audy-sovmem-v0/thresholds.json` as the frozen Q3 decision.
8. Protect the 25-query golden file and threshold file from mutation during P2.
9. Update root commands and documentation needed to run and understand the gates.
10. Regenerate and verify the build audit in the implementation pull request.

### Explicitly out of scope

- adding Tantivy or Petgraph dependencies;
- building an index or adjacency graph;
- implementing BM25, graph expansion, ranking, query execution, or an `eval` scorer;
- tuning retrieval behavior or measuring a retrieval baseline;
- changing any golden query, rationale, relevance judgment, or threshold to improve a later score;
- implementing P3 proposals, verdict application, ledger mutation, trust scoring, MCP, or live-vault writes;
- changing the five-verb ledger grammar or granting the public three-verb subset write authority;
- changing the provenance enum;
- configuring or claiming GitHub branch protection, rulesets, or merge queues;
- making the repository public.

## Locked P0 decisions

The implementation updates the decision and open-question tables in `docs/sovmem_v0.3_execution_plan.md` so these choices are no longer shown as unresolved P0/P1 blockers.

| Contract | Locked decision |
| --- | --- |
| Corpus ground truth | The accepted Audy evaluation corpus is exactly 62 documents: 8 canonical-core documents and 54 accepted research documents. The active manifest's 55 reviewed research candidates and its deterministic one-document exclusion remain evidence for how the accepted 54 were obtained. |
| Golden ground truth | `evals/audy-sovmem-v0/golden.jsonl` is the human-approved 25-query evaluation set. Every query has exactly one relevant document. |
| Canonical source | Audy Vault is canonical. Export and evidence use the committed snapshot pinned at `43824cfd9caed7b260aa442128fd1130ddb6e2ff`; generated bundles, manifests, reports, stats, and future indexes are derived and non-canonical. |
| Staging and audit direction | Preserve the execution plan's filesystem-plus-ledger direction for the future P3 write-back loop. Git supplies immutable source-snapshot identity and repository review history; it does not replace the five-verb judgment ledger. This slice does not implement staging or the ledger. |
| Crate layout | Keep one Rust crate. Do not create a workspace or split the implementation into multiple crates in this slice. |
| Repository | Keep the current Nous repository name and private visibility. Development changes merge through manually reviewed pull requests. Documentation must not claim that GitHub branch protection or a ruleset is enforced. |
| Verdict grammar | Preserve `ACCEPT`, `REJECT`, `REWRITE`, `TP`, and `FP` in the internal ledger contract. Preserve `ACCEPT`, `REJECT`, and `REWRITE` as the public pedagogical subset. The subset does not redefine the internal ledger. |
| Provenance | Preserve `tier1`, `secondary`, `synthesis`, and `unknown`. Provenance remains required where the current profile contract requires it, including new resources. |
| Q3 | Freeze the approved top-five retrieval thresholds and 25-query evaluation identity described below before P2 retrieval code exists. |

These decisions close Q1, Q2, Q3, Q4, and Q10 for the P0/P1 boundary. Later P3-P5 questions remain open unless this document explicitly resolves them.

## Official OKF reference pin

The implementation adds a small, tracked, machine-readable reference artifact at `docs/spec/okf-reference.json` with this exact identity:

```json
{
  "schema_version": "sovmem-okf-reference-v1",
  "repository": "GoogleCloudPlatform/knowledge-catalog",
  "commit": "d44368c15e38e7c92481c5992e4f9b5b421a801d",
  "normative_file": "okf/SPEC.md"
}
```

The artifact is a source pin, not a vendored copy of the external specification and not a runtime network dependency. Tests validate the exact repository name, lowercase 40-character commit, normative path, schema version, absence of unknown fields, and stable serialization. Runtime export and verification remain offline.

The execution plan and root artifact registry point to this pin. The pin establishes which OKF text informs the v0.3 conformance floor; it does not expand the deliberately stricter Nous profile schema or grant the external repository authority over SovMem's human judgment, provenance, integrity, or write-back rules.

## Verification interface

### Command shape

The existing command gains an optional bounded level:

```text
sovmem verify --profile <profile.yaml> --bundle <bundle-root> [--level <1|2|3>] [--require-corpus]
```

Levels are cumulative. Level 2 includes every Level 1 check; Level 3 includes every Level 1 and Level 2 check.

### Compatibility contract

Current callers must continue to work:

- `verify` with no `--level` keeps the current compatibility behavior: run all verification checks that apply to the supplied bundle, validate corpus evidence when present, but do not require a corpus report for a canary bundle;
- compatibility-mode success output remains byte-for-byte compatible in field set and does not gain a `level` field;
- `--require-corpus` remains accepted and retains its current strict-corpus behavior;
- explicit `--level 3` requires complete corpus evidence and becomes the preferred formal P1 gate;
- `--require-corpus` with no explicit level is a compatibility spelling of the strict Level 3 corpus requirement;
- `--require-corpus` combined with `--level 1` or `--level 2` is a CLI configuration error because it requests Level 3 evidence while selecting a lower level;
- existing successful JSON fields and exit-code meanings remain stable.

This preserves `npm run verify:audy` for the 13-document canary and keeps the existing `npm run verify:corpus:audy` command valid. The implementation may update the latter to use explicit `--level 3` after a compatibility test proves the old spelling remains accepted.

### Level 1: structure and identity

Level 1 proves that the generated documents can be interpreted as the expected profile, without claiming byte integrity or complete corpus correspondence.

It validates:

- readable, terminated, parseable frontmatter;
- the required generated frontmatter fields;
- `type` as `concept | resource | judgment` and judgment-specific required fields;
- the complete profile identity tuple: OKF version, export profile, Nous schema, SSA identity, SSA profile, familiar identity, context namespace, and vault identity;
- stable document, resource, source, and source-path bindings;
- `provenance` as exactly `tier1 | secondary | synthesis | unknown`;
- other existing fail-closed profile rules that are structural rather than hash-, link-, or source-correspondence checks.

Level 1 must not report success if malformed frontmatter or an invalid provenance value is merely hidden by a manifest edit.

### Level 2: byte integrity

Level 2 adds:

- the manifest's structure and unique document/source identities;
- raw-byte BLAKE3 for every generated document;
- the presence, shape, uniqueness, and cross-evidence consistency of the raw-byte BLAKE3 source-hash commitments recorded in the manifest; offline verification cannot recompute source hashes without source bytes;
- the exact expected generated file set;
- the bundle index;
- the existing deterministic framed bundle-root calculation over sorted UTF-8 relative paths and exact file bytes;
- consistency of bundle-hash claims across manifest and applicable reports.

Hashing remains over raw bytes. YAML normalization, newline normalization, Markdown rendering, Unicode normalization, or reparsing must never substitute for raw-byte integrity.

Level 2 independently recomputes the bundle root. Repairing each document's manifest hash after tampering cannot make a stale or forged bundle-root claim pass.

### Level 3: semantic correspondence and complete corpus evidence

Level 3 adds:

- typed frontmatter link validation;
- generated-body link state corresponding exactly to generated frontmatter link state;
- no typed generated link that claims a nonexistent same-profile target;
- source-to-document identity and source-hash correspondence;
- export-report correspondence with the manifest, exclusions, warnings, and errors;
- a required `corpus-report.json`;
- complete, mutually consistent evidence for every selected corpus candidate as accepted or explicitly excluded;
- the fixed Audy corpus profile identity, required eight-document core, accepted research count, theme gates, zero validation errors, and the accepted 62-document result;
- correspondence among manifest, export report, corpus report, and bundle root.

Level 3 is offline. It verifies the published evidence against the bundle and the pinned contract; it does not read or modify the live vault.

The accepted corpus currently carries deterministic `links.broken` and `links.ambiguous` warnings from source/body links whose targets are outside the selected corpus or cannot be promoted safely into typed generated links. Level 3 preserves those as warnings when the generated link state and export evidence correspond exactly. It fails a dangling typed-link claim or tampered link-state correspondence. Closing P1 must not require expanding the approved corpus or rewriting canonical source bodies merely to erase reported warnings.

The formal P1 command is:

```bash
cargo run --manifest-path sovmem/Cargo.toml -- verify \
  --profile profiles/audy-ssa0-v0/profile.yaml \
  --bundle var/audy-sovmem-retrieval-corpus \
  --level 3
```

### Output and exit behavior

Successful verification writes exactly one deterministic JSON object to stdout and no diagnostics to stderr. When the user supplies an explicit level, the successful object adds that level without removing existing fields:

```json
{"status":"ok","command":"verify","level":3,"documents":62,"warnings":226,"errors":0,"bundle_hash":"49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65"}
```

Diagnostics remain privacy-safe, one deterministic JSON value per line on stderr, sorted by severity, code, and stable non-sensitive context. They never include document bodies, source excerpts, absolute vault paths, or environment values.

Exit codes remain:

- `0`: all requested checks passed;
- `1`: readable bundle or corpus data failed verification;
- `2`: CLI usage, profile configuration, or required input could not be loaded safely.

## Deterministic `stats` interface

### Command shape

```text
sovmem stats --profile <profile.yaml> --bundle <bundle-root>
```

`stats` reuses the compatibility verification path and emits no summary when verification has errors. If corpus evidence exists, it must correspond to the manifest and reports before statistics are emitted. `stats` does not read the live vault and does not create or mutate bundle files.

### Successful output

The command emits exactly one compact JSON object with fields in a fixed serializer-defined order:

```json
{
  "status": "ok",
  "command": "stats",
  "documents": 62,
  "by_provenance": {
    "secondary": 0,
    "synthesis": 26,
    "tier1": 0,
    "unknown": 36
  },
  "by_type": {
    "concept": 26,
    "judgment": 0,
    "resource": 36
  },
  "by_layer": {
    "analysis": 18,
    "concept": 36,
    "synthesis": 8
  },
  "warnings": 226,
  "exclusions": 1,
  "bundle_hash": "49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65"
}
```

The example shows the expected output for the currently accepted live corpus evidence. Implementation still computes every value from the verified generated documents and reports rather than hard-coding the example.

Determinism rules:

- document count equals the verified manifest document count;
- provenance, type, and layer objects contain every allowed enum key, including zero-count buckets;
- distribution keys are lexicographically ordered;
- counts are non-negative integers and each distribution sums to `documents`;
- warnings and exclusions come from verified report evidence, not a fresh scan or filesystem order;
- bundle hash is the independently recomputed Level 2 root;
- repeated runs over byte-identical input produce byte-identical stdout with one terminal newline;
- output contains no paths, titles, bodies, query text, or timestamps.

Invalid input uses the same privacy-safe diagnostic format and exit-code classes as `verify`.

## Corruption contract and error codes

The implementation adds dedicated fixtures for all five approved corruption classes. Each fixture begins from a valid deterministic corpus bundle, changes only the intended corruption dimension, runs the relevant verification level, and asserts a nonzero exit plus an exact scenario-specific code.

| Corruption | Minimum level | Required stable code | Required behavior |
| --- | ---: | --- | --- |
| Modified generated document bytes with the manifest unchanged | 2 | `bundle.document_hash_mismatch` | Fail on the changed raw bytes. |
| Dangling typed link claim to a nonexistent same-profile target | 3 | `links.dangling` | Fail even if document and bundle hashes are repaired; existing reported source/body-link warnings remain warnings. |
| Malformed or unterminated generated frontmatter | 1 | `frontmatter.malformed` | Fail before metadata can be treated as valid. |
| Provenance outside the locked enum | 1 | `document.provenance_invalid` | Fail even if the YAML is otherwise valid and all hashes are repaired. |
| Every modified document hash repaired individually while the bundle root remains invalid | 2 | `bundle.root_hash_mismatch` | Recompute the framed root and reject the stale or forged root claim. |

The scenario-specific codes are the contract tests grep. Existing umbrella codes such as `bundle.hash_mismatch`, `bundle.document_invalid`, and `links.broken` may remain as additional compatibility diagnostics, but they do not replace the required specific code. No existing documented code is removed in this slice without a compatibility test and explicit review.

Diagnostics must be deterministic when more than one invariant fails. Tests assert the required code is present and that output does not leak corrupted content or absolute paths.

## Q3 threshold freeze

### Artifact

Add `evals/audy-sovmem-v0/thresholds.json` with a strict schema equivalent to:

```json
{
  "schema_version": "sovmem-retrieval-thresholds-v1",
  "evaluation_set": {
    "id": "audy-sovmem-v0",
    "golden_file": "evals/audy-sovmem-v0/golden.jsonl",
    "golden_blake3": "f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175",
    "query_count": 25,
    "relevant_documents_per_query": 1
  },
  "retrieval": {
    "k": 5,
    "macro_precision_at_k_minimum": "0.16",
    "macro_recall_at_k_minimum": "0.80",
    "minimum_queries_with_hit": 20
  },
  "approval": {
    "human_approved": true,
    "authority": "repository-owner",
    "freeze_date": "2026-07-12",
    "frozen_through_phase": "P2"
  }
}
```

Decimal thresholds are canonical decimal strings for display and schema review. Runtime gate decisions must not parse or compare rounded floating-point metrics.

### Integer gate

For this frozen evaluation set, a query is a hit when its one relevant document appears once within ranks 1 through 5. Let `h` be the integer number of hit queries.

With 25 queries, one relevant document per query, and `k = 5`:

```text
Macro Precision@5 = h / (25 * 5)
Macro Recall@5    = h / 25
```

Both approved thresholds reduce exactly to:

```text
h >= 20
```

Future P2 evaluation code must gate on `minimum_queries_with_hit` after validating the frozen assumptions. It may calculate decimal metrics for reporting, but it must not decide pass/fail by comparing rounded floats. A query count other than 25, `k` other than 5, more or fewer than one relevant document for any query, a golden-file hash mismatch, or a non-human-authored query invalidates the evaluation input before scoring.

### Freeze enforcement

Before P2 begins, tests must:

- recompute raw-byte BLAKE3 for `golden.jsonl` and compare it with `thresholds.json`;
- validate all 25 query IDs, human authorship markers, and one-document relevance lists;
- validate the threshold schema, exact values, approval record, and date;
- reject unknown fields, malformed decimals, inconsistent integer gates, and path traversal;
- lock the exact golden and threshold bytes or their reviewed digests in a test independent of the files being checked.

During P2, neither `golden.jsonl` nor `thresholds.json` may change in an implementation, tuning, or regression-fix pull request. A future change requires stopping and restarting the P2 gate through a separate rebaseline decision, a new reviewed schema/evaluation-set version, a human approval record, and an audit update. It cannot be bundled with retrieval changes intended to pass the gate.

This slice freezes the gate only. It does not create `baseline.json`, score retrieval, or claim the Q3 threshold has been met.

## Implementation surfaces

The detailed implementation plan may refine function and test placement while preserving these artifact and interface boundaries. Expected surfaces are:

- `docs/sovmem_v0.3_execution_plan.md` for resolved decisions, questions, and formal gate wording;
- `docs/spec/okf-reference.json` for the OKF source pin;
- `evals/audy-sovmem-v0/thresholds.json` for the Q3 freeze;
- `sovmem/src/cli.rs` for CLI arguments, output, and compatibility routing;
- focused verifier modules under `sovmem/src/profile/` so level policy, bundle parsing, integrity, links, and stats do not remain one unbounded CLI function;
- Rust integration tests for level semantics, compatibility, stats, and corruption fixtures;
- JavaScript contract tests for tracked pins, root commands, and P2 freeze immutability;
- `package.json` for portable Level 3 and stats commands;
- `README.md` and `index.md` for user-facing commands, artifact registration, and scope boundaries;
- `docs/sovmem-build-state.json` and generated `docs/SOVMEM-BUILD-AUDIT.md` for reviewed completion evidence.

No new Rust dependency is required for this slice. Existing `blake3`, `serde`, `serde_json`, `serde_yaml`, and `clap` are sufficient.

## Deterministic and security behavior

- All runtime verification and stats operations are offline.
- Audy Vault is read only by export; verification and stats read only the derived bundle.
- Hashes use exact bytes and lowercase BLAKE3 hex.
- Map and diagnostic ordering is explicit and independent of filesystem enumeration or hash-map iteration.
- Successful JSON is compact, stable, and terminated by one newline.
- Failures do not echo private source content, query text, absolute paths, environment values, or malformed payloads.
- Symlinks, unexpected root artifacts, path traversal, duplicate identities, and non-UTF-8 paths continue to fail closed under the applicable level.
- Stats never establishes truth or authority; it summarizes already verified evidence.
- The OKF pin never triggers network access and cannot silently update.
- Generated `var/` output and Rust `target/` output remain ignored and non-authoritative.

## Test design

Implementation follows TDD after the detailed plan is approved. The plan must put each behavior behind a failing focused test before production changes.

### Verification-level tests

- CLI accepts only levels 1, 2, and 3.
- Levels are cumulative.
- Bare `verify` preserves the current canary and corpus behavior.
- Existing `--require-corpus` callers remain valid.
- Explicit Level 3 rejects a bundle without complete corpus evidence.
- Level 1 catches malformed frontmatter and invalid provenance independently of repaired hashes.
- Level 2 catches modified bytes and stale bundle roots.
- Level 3 catches dangling links, source mismatch, and incomplete corpus evidence.
- Successful output reports the effective level deterministically.
- Exit codes remain compatible.

### Stats tests

- exact field set and output ordering;
- correct document count and all enum buckets;
- each distribution sums to the document count;
- verified warning and exclusion counts;
- independently recomputed bundle hash;
- byte-identical output on repeated runs;
- invalid bundle produces diagnostics and no stdout summary;
- no sensitive paths or contents in success or error output.

### Corruption tests

Each of the five approved corruptions has a focused fixture and exact-code assertion. Repaired-hash tests must deliberately repair the lower-level claims required to reach the intended higher-level failure, proving that the verifier is not passing merely because an earlier mismatch masks the target invariant.

### Pin and threshold tests

- exact OKF repository, commit, normative path, and schema;
- exact golden BLAKE3 and 25-query identity;
- exact `k = 5`, display thresholds, and integer `20/25` hit requirement;
- strict unknown-field rejection and stable path rules;
- human approval and freeze date required;
- threshold/golden mutation fails a repository contract test;
- no Tantivy, Petgraph, BM25, index, query, ranking, or eval implementation enters the dependency graph or command surface.

### Regression and live-evidence tests

- full JavaScript and Rust suites;
- audit freshness check;
- deterministic corpus export from the pinned Audy Vault snapshot;
- explicit Level 3 verification of the generated corpus;
- golden-query validation against the generated corpus;
- two stats invocations over the same bundle produce exact matching bytes;
- existing canary export and bare verification remain green.

## Exit gates

### P0 gate: 100%

P0 may be marked passed only when:

1. the locked decisions above are recorded in the normative plan without unresolved P0/P1 wording;
2. the official OKF reference artifact is committed and contract-tested;
3. the single crate builds with its existing approved dependencies;
4. the accepted corpus evidence proves 62 documents: 8 core and 54 research;
5. the golden set proves 25 human-approved queries with one relevant document each;
6. the Audy Vault source identity and pinned commit remain explicit and no generated bundle is promoted to canon;
7. repository privacy and manual PR merge discipline are described accurately without a branch-protection claim;
8. the five-verb internal ledger, three-verb public subset, and provenance enum remain unchanged.

### P1 gate: 100%

P1 may be marked passed only when:

1. explicit Levels 1 through 3 are implemented cumulatively;
2. current `verify` compatibility is covered by tests;
3. `verify --level 3` passes on the generated 62-document corpus;
4. manifest document hashes and the bundle root use raw-byte BLAKE3;
5. source, link, report, exclusion, and complete corpus correspondence pass;
6. deterministic stats reports count, distributions, warnings, exclusions, and bundle hash;
7. every approved corruption fixture fails with its required greppable code;
8. the full repository suite and audit freshness check pass.

### P2 state after this slice

P2 remains approximately 10%. Q3 is frozen and removed from active blockers, but the P2 retrieval gate has not been attempted or passed. The audit must continue to list the unimplemented index, graph, BM25 query path, evaluation baseline, and determinism gate as remaining work.

### Slice completion gate

Before the implementation pull request is eligible for manual merge:

- all P0 and P1 exit evidence is present and reviewed;
- the Q3 freeze is committed and protected by tests;
- `docs/sovmem-build-state.json` marks P0 and P1 at 100% with passed gates, keeps P2 near 10%, and records no false retrieval capability;
- `npm run audit:sovmem` regenerates `docs/SOVMEM-BUILD-AUDIT.md`;
- `npm run audit:sovmem:check` and `npm test` pass from a clean checkout;
- the expected weighted overall completion rounds to approximately 24%;
- no forbidden P2 implementation or dependency appears in the diff;
- publication uses a reviewed pull request and manual merge discipline.

## Audit impact

The implementation pull request updates reviewed audit state and regenerates the report in the same change as the code and evidence.

Expected reviewed phase state after successful implementation:

| Phase | Expected completion | Gate | Meaning |
| --- | ---: | --- | --- |
| P0 | 100% | passed | Ground truth and blocking decisions are formally locked. |
| P1 | 100% | passed | The bundle substrate passes explicit Level 3 and corruption gates. |
| P2 | approximately 10% | in-progress | Q3 is frozen; retrieval remains unimplemented. |
| P3-P5 | 0% | not-started | Unchanged. |

Using the execution plan's fixed effort weights, this state rounds to approximately 24% overall. The audit evidence records fresh test counts, the verified corpus result, golden and threshold identities, the bundle hash, explicit Level 3 success, and the implementation commit or merge evidence available at publication time.

The report must not claim:

- retrieval quality met the threshold;
- P2 passed;
- the repository is public;
- GitHub branch protection, required checks, rulesets, or merge queues are enforced;
- the generated bundle or threshold file is canon;
- any P3-P5 capability exists.

## Self-review checklist

Before committing this design document and before approving the later implementation plan, verify that:

- all approved counts, hashes, commits, thresholds, dates, verbs, and enum values are exact;
- bare `verify`, `--require-corpus`, and explicit Level 3 semantics do not contradict one another;
- every level is cumulative and has a clear boundary;
- each corruption has one required scenario-specific code;
- stats fields have one authoritative data source and deterministic ordering;
- the Q3 gate uses integer hit counts rather than rounded floats;
- the golden set and thresholds are frozen during P2;
- the OKF pin is exact and offline;
- the audit can reach P0 100%, P1 100%, P2 approximately 10%, and overall approximately 24% without implying retrieval implementation;
- no placeholder, TODO, unresolved choice, absolute private path, or accidental P2/P3 scope appears;
- manual merge discipline is described without claiming enforced GitHub controls.

## Review and implementation sequence

1. Commit only this design document on `codex/sovmem-p0-p1-gate-closure`.
2. Stop for human review of the committed specification.
3. After approval, use `superpowers:writing-plans` to create a detailed TDD implementation plan.
4. Commit only the plan and stop for a second human approval.
5. Do not begin implementation before that approval.
