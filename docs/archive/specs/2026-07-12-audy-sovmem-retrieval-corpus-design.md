> **ARCHIVED** — This document has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix B: Archive Map](../sovmem-master-spec.md#appendix-b-archive-map) for the master spec section that covers its content.
>
# Audy SovMem Retrieval Corpus and Golden Queries Design

**Status:** Approved design for the corpus-first milestone on 2026-07-12.

## Purpose

The completed Audy canary proves that Nous can export and verify a deterministic, profile-isolated, read-only SovMem bundle from a pinned Audy Vault commit. Its 13 low-risk research documents validate the interface, but they are too narrow to ground Audy's identity, TFIS doctrine, and a useful retrieval evaluation.

This milestone creates the ground truth required before a retrieval engine is built:

1. a curated, reproducible corpus containing Audy's canonical core plus broad reviewed research; and
2. a human-authored golden query set whose relevant document identifiers are checked against that corpus.

The result is a retrieval-corpus foundation, not a retrieval quality claim. Tantivy indexing, Petgraph expansion, BM25 ranking, evaluation metrics, and tuning remain a separately gated phase.

## Authority and invariants

- `docs/sovmem_v0.3_execution_plan.md` remains the normative development plan.
- `docs/superpowers/specs/2026-07-10-audy-sovmem-profile-v0-design.md` remains authoritative for profile identity, mapping, sensitivity, isolation, and source authority.
- Audy Vault remains the canonical external context for `ssa-0`. Every generated corpus, report, manifest, and future index is derived and non-canonical.
- The source is the committed Git snapshot `43824cfd9caed7b260aa442128fd1130ddb6e2ff`. No working-tree fallback, filesystem timestamp, model classification, or live-vault write is allowed.
- The existing `nous-ssa-context-v0` and `nous-sovmem-v0` schemas are unchanged. The milestone may select more documents but must not add core types, context kinds, or profile identities.
- Source content remains internal. Generated output stays under ignored `var/`; committed configuration, reports used as gates, and golden queries must be safe for the repository's visibility.

## Scope

### In scope

- An exact, human-reviewed corpus manifest bound to the pinned source commit.
- Eight required canonical-core documents.
- Exactly fifty-five reviewed research candidates in `audy-retrieval-corpus-v0`, of which at least forty-five must pass all gates. Any later expansion requires a versioned manifest revision.
- A final accepted corpus of at least fifty-three documents: all eight canonical-core documents plus at least forty-five research documents.
- A deterministic coverage and acceptance report.
- At least twenty-five human-authored golden queries with verified relevant document IDs.
- Validation commands and tests for corpus composition, isolation, reproducibility, and golden-query integrity.

### Out of scope

- Tantivy, Petgraph, BM25, graph expansion, ranking, query execution, or `baseline.json` metrics.
- Embeddings, vector databases, model-generated queries, or model-assigned relevance.
- Daily notes, private memory, credentials, raw Hermes material, harness-runtime lanes, or unrestricted vault ingestion.
- Bulk frontmatter migration or any edit to Audy Vault.
- Judgments, verdict application, ledger mutation, trust scoring, MCP, or a default SSA profile.

## Corpus architecture

The corpus uses thematic quotas backed by exact file paths. Globs may be used only as discovery aids; they never define the accepted corpus. The committed manifest is the complete source-selection contract.

### Required canonical core

All eight paths are mandatory and must export successfully:

1. `01-Soul/Audy Himura.md` — Audy identity.
2. `01-Soul/Identity.md` — identity doctrine.
3. `01-Soul/SSA Archetypes.md` — SSA role model.
4. `01-Soul/TFIS Axioms.md` — foundational TFIS assertions.
5. `01-Soul/TFIS Canon.md` — canonical TFIS doctrine.
6. `AGENTS.md` — agent operating contract.
7. `OPERATIONS.md` — system operating contract.
8. `00-Index.md` — canonical context navigation.

These documents use the existing `Identity`, `Doctrine`, `Operating Contract`, and `Memory Index` mappings. Their generated sensitivity remains governed by the approved profile. A required core document cannot be silently excluded or replaced by a research summary.

### Research candidates and quotas

The v0 manifest contains exactly fifty-five research paths beneath `04-Research/`. Every candidate has exactly one primary theme so a document cannot satisfy multiple minimums. A different candidate count requires a versioned manifest revision rather than silent expansion.

| Primary theme | Minimum accepted | Purpose |
| --- | ---: | --- |
| `tfis-ssa` | 10 | TFIS thesis, SSA role, solo-system architecture, and related critiques |
| `sovereign-context` | 15 | SovMem, context architecture, OKF, AVM, provenance, memory, and portability |
| `agent-operations` | 10 | Agent loops, operating systems, protocols, handoffs, and bounded execution |
| `governance-evidence` | 10 | Governance, evaluation, evidence quality, assurance, and failure analysis |

At least forty-five research documents must survive. The thematic minima sum to forty-five and are individually binding. Candidates above the minima form a buffer for validation failures; they do not weaken any quota.

### Selection record

The design introduces `profiles/audy-ssa0-v0/retrieval-corpus.yaml` with this conceptual shape:

```yaml
schema: audy-retrieval-corpus-v0
mode: committed
source_commit: 43824cfd9caed7b260aa442128fd1130ddb6e2ff
minimum_documents: 53
minimum_research_documents: 45
core:
  - path: 01-Soul/Audy Himura.md
    role: identity
research:
  - path: 04-Research/OKF/README.md
    theme: sovereign-context
exclude:
  - 02-Memory/**
  - 05-Daily/**
  - 08-Hermes/**
  - 09-Claude/**
  - 10-Codex/**
  - 11-Antigravity/**
```

The shown research entry is an already validated canary document. The implementation extends the list only through human content review; it must not invent candidates from filenames alone. Candidate review is a human content-selection action, while the exporter enforces the recorded contract.

## Data flow

1. Confirm the vault identity, pinned commit, and pre-run working-state fingerprint.
2. Load the existing Audy profile and the retrieval-corpus manifest.
3. Reject malformed manifests, duplicate paths, unknown themes, insufficient candidate counts, forbidden-lane paths, or an exact selection canceled by an exclusion.
4. Read each selected source from the pinned Git object database using the existing read-only source boundary.
5. Parse, normalize, map, resolve links, validate, and render with the existing profile schema.
6. Require every canonical-core document to succeed. Collect research validation results by primary theme.
7. Stop before publication if any core document fails, fewer than forty-five research documents survive, any theme misses its quota, or the total is below fifty-three.
8. Publish atomically to ignored `var/audy-sovmem-retrieval-corpus/` only after all gates pass.
9. Emit the existing manifest and export report plus a deterministic corpus coverage report.
10. Validate the golden query set against the published manifest without executing retrieval.
11. Confirm the post-run vault commit and working-state fingerprint equal the pre-run values.

## Coverage report

`corpus-report.json` is deterministic evidence for the selection gate. It contains no source bodies and no absolute filesystem paths. It records:

- schema and profile identifiers;
- pinned source commit;
- candidate, accepted, rejected, warning, and error counts;
- required-core presence;
- accepted research count by primary theme;
- accepted generated IDs and source hashes;
- rejected candidate paths with stable redacted reason codes;
- corpus bundle hash; and
- whether all acceptance gates passed.

The report is derived output under `var/`. The committed manifest and tests define the durable contract.

## Golden query set

The durable query set lives at `evals/audy-sovmem-v0/golden.jsonl`. Each line uses this schema:

```json
{"id":"q-001","query":"What defines the Solo Systems Architect?","relevant_ids":["ssa-0/01-Soul/SSA%20Archetypes"],"rationale":"The canonical archetype document directly defines the role.","authorship":"human"}
```

Requirements:

- At least twenty-five unique query IDs and unique non-empty query strings.
- `authorship` is exactly `human`; model-generated or model-expanded queries are inadmissible.
- Every query has at least one relevant ID present in the accepted corpus manifest.
- Relevant IDs are unique, sorted, and profile-scoped.
- Query text, rationale, and IDs must be safe to commit. No private memory, credentials, personal incident detail, or raw harness content may appear.
- Coverage must include every primary research theme and the canonical core.
- Khalil, as SSA-0, authors or explicitly approves every query and relevance set before the gate passes.
- Queries and relevance labels freeze before retrieval implementation. Later changes require an explicit evaluation-set revision, not silent tuning to improve metrics.

The query validator checks structure and referential integrity only. It cannot certify that a relevance judgment is correct; that authority remains human.

## Failure behavior

### Hard failures

- The vault or pinned commit is unavailable or mismatched.
- Any required core path is missing, dirty, untracked, excluded, unmapped, invalid, or rejected for sensitivity.
- A candidate path enters a forbidden lane or contains unsafe characters/components.
- The manifest has duplicate paths, invalid themes, ambiguous configuration, or insufficient candidates.
- Accepted research falls below forty-five, a thematic quota is missed, or total accepted documents fall below fifty-three.
- A schema change, new context kind, or new core type would be required to reach the minimum.
- Generated identities collide, cross a profile boundary, or fail source-derived identity checks.
- Golden queries are fewer than twenty-five, malformed, duplicated, non-human, unsafe to commit, or reference absent IDs.
- Source state changes during the run or publication is non-atomic.

### Warnings

- Inferred descriptions or provenance.
- Weak research citations.
- Broken or ambiguous links that do not invent a target.
- A non-required research candidate rejected while all count and theme gates still pass.

Warnings remain visible in deterministic reports. They are never silently dropped, and they never authorize schema relaxation.

## Privacy and isolation

- Corpus output is internal derived material and remains ignored.
- The manifest may include only paths approved as safe to expose in repository history.
- Golden queries and rationales must be public-safe even when they evaluate internal generated documents.
- `02-Memory`, daily notes, raw Hermes, Claude, Codex, Antigravity, and other runtime-memory or harness lanes are denied at configuration and verification boundaries.
- No content or identity from another SSA profile may enter the corpus, manifest, query set, report, or generated IDs.
- The neutral `default-ssa-v0` fixture remains free of Audy, Khalil, SSA-0, vault-path, judgment, and harness data.

## Verification strategy

The implementation plan must use test-driven changes and prove:

1. all eight exact core documents are required and represented;
2. exactly fifty-five reviewed research candidates are recorded with one valid primary theme each;
3. at least forty-five research documents and fifty-three total documents are accepted;
4. every thematic quota is met without double counting;
5. forbidden lanes fail before source reading or publication;
6. two exports from the same profile, manifest, and commit are byte-identical;
7. the source vault commit and working-state fingerprint are unchanged;
8. the output remains outside the vault and publishes atomically;
9. corpus reports contain stable counts/codes but no source bodies, secrets, or absolute paths;
10. at least twenty-five human-authored queries validate against accepted IDs;
11. missing, duplicate, foreign-profile, or unsafe golden references fail closed;
12. the existing 13-document canary, offline verifier, neutral-profile isolation, and full repository suite remain green.

## Acceptance gate

The milestone is complete only when:

- the exact manifest is human-reviewed and bound to the pinned commit;
- all eight canonical-core documents export;
- at least forty-five research documents export and every thematic quota passes;
- the accepted corpus contains at least fifty-three documents;
- at least twenty-five human-authored golden queries pass referential and privacy validation;
- repeated exports are byte-identical;
- the vault is unchanged;
- existing canary and isolation tests remain green; and
- no retrieval engine, ranking metric, or tuning logic has entered the milestone.

After this gate, a separate design and implementation plan may add Tantivy/Petgraph retrieval, freeze evaluation thresholds before execution, and produce the first measured `baseline.json`.
