> **ARCHIVED** — This document has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix B: Archive Map](../sovmem-master-spec.md#appendix-b-archive-map) for the master spec section that covers its content.
>
# Nous SSA Context and Audy-SovMem Reference Profile v0 Design

**Date:** 2026-07-10
**Status:** Approved for implementation planning
**Owner:** Khalil Nooh
**Base context schema:** `nous-ssa-context-v0`
**SovMem export profile:** `nous-sovmem-v0`
**Reference profile instance:** `audy-ssa0-v0`
**Applies to:** Generated SovMem bundles derived from Audy's Vault

## Purpose

Define the reusable SSA context architecture and the metadata, authority, export, and validation contract for turning a selected snapshot of Audy's Vault into a generated SovMem bundle without mutating the live vault.

The profile reconciles three existing sources:

1. OKF v0.1, which requires Markdown concept documents with YAML frontmatter and a non-empty `type`.
2. The Audy OKF profile draft, which adds title, description, resource identity, lifecycle, sensitivity, scope, and source-path metadata.
3. The SovMem v0.3 execution plan, which requires a small core type system, typed layers and links, provenance, deterministic export, and a human-only canon commit boundary.

## Nous development context

Nous is the operating system for a Solo Systems Architect. It provides the reusable contracts that let one accountable SSA work through multiple agent harnesses while retaining one owned, canonical external context plane.

Audy is not Nous itself. Audy is the first concrete familiar profile running on Nous for Khalil Nooh, SSA-0. Audy's Vault is the canonical context plane for that profile.

```text
Nous core - reusable SSA operating system
└── SSA profile instance
    ├── SSA identity and ownership
    ├── familiar identity and operating voice
    ├── one canonical external context plane
    ├── judgment ledger and trust history
    ├── authority, security, and autonomy policy
    ├── runtime-memory namespace
    └── harness adapters and scoped write lanes
```

For the first reference implementation:

```text
SSA identity:          SSA-0 / Khalil Nooh
Familiar profile:      Audy / audy-ssa0-v0
Canonical context:     Audy Vault
Canonical namespace:   ssa-0
Runtime memory:        Honcho and harness session stores
Harness consumers:     OpenClaw, Hermes, Claude, Codex, Antigravity, future adapters
```

The phrase "single external context" means one canonical context plane per SSA profile, shared across that SSA's authorized harnesses. It does not mean one global context shared across users. Each SSA profile requires isolated context, ledger, runtime-memory, credentials, policy, and trust history.

### Profile layers

| Layer | Purpose | Example |
| --- | --- | --- |
| Nous core | Reusable schemas, authority contracts, lifecycle rules, isolation, and adapter interfaces | `nous-ssa-context-v0` |
| Default SSA template | Barebones onboarding template extracted from proven non-personal Audy contracts | future `default-ssa-v0` |
| SSA profile instance | One owner's familiar identity, policies, namespaces, and context roots | `audy-ssa0-v0` |
| SovMem export profile | Generated bundle representation shared by all compatible SSA profiles | `nous-sovmem-v0` |

The current work validates the Audy/SSA-0 reference instance first. A future default profile is extracted from the reusable structure only after the reference profile works end-to-end.

## Current evidence

The mounted Audy Vault contains 416 Markdown files, but only 8 currently begin with YAML frontmatter and none has an OKF `type` in its frontmatter. A 92-document candidate set remains after excluding raw source snapshots from the primary doctrine, memory, protocol, research, and harness lanes. The vault is Git-backed, but the inspected working tree contains substantial modified and untracked material and has no configured remote.

Consequently:

- the live vault is not an ingest-ready SovMem bundle;
- a generated export must be the first integration surface;
- the first source snapshot must be explicit and reproducible;
- Git may provide source history, but it is not the SovMem verdict ledger;
- source files must not be bulk-rewritten to satisfy this profile.

## Selected approach

Use the reusable Nous/SovMem core schema for machine-critical fields, preserve each profile's richer taxonomy through the neutral `context_kind` extension, and bind every export to an isolated SSA profile instance. Audy's vocabulary seeds the reference profile without becoming a global Nous assumption.

| Approach | Benefit | Cost | Disposition |
| --- | --- | --- | --- |
| SovMem core `type` plus neutral `context_kind` extension | Preserves SovMem validator/index semantics while allowing each SSA profile to retain its domain vocabulary | Requires one deterministic mapping table per profile | **Selected** |
| Put Audy's rich kinds directly in `type` | Simple and valid under permissive OKF | Breaks the SovMem v0.3 `concept/resource/judgment` contract | Rejected |
| Maintain separate Audy and SovMem profiles with translation at every consumer | Maximum theoretical independence | Duplicates authority and creates drift at every integration | Rejected for v0 |

## Authority and system boundaries

```text
live Audy Vault
  = canonical curated context plane for Audy / SSA-0

Nous core
  = reusable schemas, policy boundaries, profile isolation, and adapter contracts

Audy / SSA-0 profile
  = first concrete familiar identity, ownership, policy, and context namespace

explicit source snapshot
  = immutable input identity for one export run

generated Audy-SovMem bundle
  = derived, regenerable machine interface

SovMem index
  = disposable retrieval structure

Honcho
  = profile-scoped runtime conversational and peer-model memory, not canon

Hermes and other harness lanes
  = scoped sources and proposal origins, not silent authority
```

The profile MUST NOT:

- mutate live vault notes during export;
- mix context, ledgers, runtime memory, credentials, or trust history across SSA profiles;
- promote raw Honcho messages, representations, or conclusions directly into canon;
- treat Hermes or another harness lane as canonical merely because it generated a note;
- infer a `judgment` from file location or prose;
- use model confidence, trust metrics, or evaluator output as commit authority;
- export restricted material unless an explicit private scope permits it;
- use filesystem modification time as authoritative knowledge time.

## Source precedence

For generated bundle semantics:

1. explicit decisions by the owning SSA and accepted Nous core contracts;
2. the SovMem v0.3 execution plan;
3. this approved profile once accepted;
4. the accepted June 23 source-lineage recovery;
5. the Audy OKF profile and OKF adoption research;
6. local SovMem/ANMS research syntheses;
7. exporter inference.

Inference MUST never override an explicit source value. Conflicting explicit values fail export for that document and enter the report.

## Required generated frontmatter

Every exported concept document MUST contain:

```yaml
---
okf_version: "0.1"
profile: nous-sovmem-v0
nous_schema: nous-ssa-context-v0
ssa_id: ssa-0
ssa_profile: audy-ssa0-v0
familiar_id: audy
context_namespace: ssa-0
id: ssa-0/01-Soul/SSA%20Archetypes
type: concept
context_kind: Doctrine
title: SSA Archetypes - Solo Systems Architect
description: Canonical SSA doctrine and capability maturity model.
layer: synthesis
source: vault://ssa-0/01-Soul/SSA%20Archetypes
source_path: 01-Soul/SSA Archetypes.md
resource: vault://ssa-0/01-Soul/SSA%20Archetypes
source_kind: vault-note
provenance: synthesis
status: active
authority: canonical
sensitivity: internal
created: 2026-05-17
updated: 2026-06-24
tags: [audy, tfis, ssa, doctrine]
links: []
inferred_fields: [type, context_kind, title, description, layer, source_kind, provenance, status, authority, sensitivity, tags, links]
---
```

`inferred_fields` MUST list every classification or content-metadata value inferred rather than copied from explicit source frontmatter. Fixed profile values, path-derived identifiers, source paths, resource URIs, and Git-derived dates are deterministic derivations and are not listed. An empty list means all classification and content metadata was explicit.

## Field contract

| Field | Rule |
| --- | --- |
| `okf_version` | MUST equal `"0.1"` for v0. |
| `profile` | MUST equal `nous-sovmem-v0`; this identifies the reusable export schema, not a person. |
| `nous_schema` | MUST equal `nous-ssa-context-v0`. |
| `ssa_id` | Immutable owner namespace assigned during onboarding. The reference value is `ssa-0`. |
| `ssa_profile` | Versioned profile-instance identifier. The reference value is `audy-ssa0-v0`. |
| `familiar_id` | Stable familiar identity within the SSA profile. The reference value is `audy`. |
| `context_namespace` | Isolation namespace for context, ledger, trust history, and runtime-memory bindings. The reference value is `ssa-0`. |
| `id` | Stable profile-scoped identity: `<context_namespace>/<percent-encoded relative path without .md>`. |
| `type` | MUST be `concept`, `resource`, or `judgment`. |
| `context_kind` | Human-readable profile classification. Audy's reference vocabulary is defined below; unknown values warn but do not invalidate OKF. |
| `title` | Explicit source title, otherwise first H1, otherwise filename stem. |
| `description` | Explicit source description, otherwise the first meaningful prose paragraph after the H1: strip Markdown link/inline emphasis syntax, collapse whitespace, take the first sentence, and cap at 240 characters without using a model. |
| `layer` | MUST be `synthesis`, `analysis`, or `concept`. |
| `source` | Original source identity. Vault-native notes use their `vault://<context_namespace>/...` URI; external research uses its primary URL or source-index identity. |
| `source_path` | Exact vault-relative source path. |
| `resource` | Stable `vault://<context_namespace>/...` URI for the source note. |
| `source_kind` | MUST be `vault-note`, `source-snapshot`, `generated-report`, or `external-reference`. |
| `provenance` | MUST be `tier1`, `secondary`, `synthesis`, or `unknown`. |
| `status` | MUST be `active`, `review`, `draft`, `generated`, `superseded`, or `archived`. |
| `authority` | MUST be `canonical`, `reviewed`, `working`, or `raw`. |
| `sensitivity` | MUST be `public`, `internal`, `private`, or `restricted`. |
| `created` | Explicit source value or earliest Git add date for the selected source snapshot. No export-time fallback. |
| `updated` | Explicit source value or latest Git commit date at the selected source snapshot. No filesystem-mtime fallback. |
| `tags` | Deterministic list derived from explicit tags, path, kind, and reserved TFIS vocabulary. |
| `links` | List of typed `{rel, target}` links to exported IDs. Empty is valid. |
| `inferred_fields` | List of required fields generated by exporter rules rather than copied from explicit metadata. |

Optional fields are `owners`, `valid_from`, `valid_to`, `supersedes`, and `superseded_by`. Consumers MUST preserve unknown extension fields when round-tripping.

## Core type rules

### `concept`

Use for doctrine, identity, operating contracts, protocols, memory indexes, health reports, project briefs, research syntheses, design briefs, concept maps, gap analyses, adoption/profile specifications, loop definitions, handoffs, and reviewed runtime notes.

### `resource`

Use when the document primarily describes or binds an external or preserved source artifact, such as a source snapshot, dataset, paper, PDF, API, repository, or live resource. A resource MUST have explicit source identity and provenance.

### `judgment`

Use only for a human verdict record produced through the SovMem verdict path. It MUST add:

```yaml
verdict: ACCEPT | REJECT | REWRITE | TP | FP
reason: <non-empty human reason>
subject: <proposal or governed artifact id>
```

The exporter MUST NOT infer `judgment` from Daily Court files, verdict-like prose, or harness output. Existing verdict artifacts may enter the initial export as `concept` plus an appropriate `context_kind` until a human approves a judgment migration.

## Reference context-kind vocabulary

The initial vocabulary below is derived from Audy and belongs to the `audy-ssa0-v0` reference profile. A future `default-ssa-v0` template MAY reuse neutral structural kinds, but MUST define its own reviewed mapping rather than inherit Audy's profile wholesale.

```text
Doctrine
Identity
Operating Contract
Memory Index
Health Report
Daily Log
Protocol
Person
Harness Lane
Project Brief
Research Brief
Design Brief
Source Snapshot
Concept Map
Gap Analysis
Adoption Spec
Profile Spec
Promotion Candidate Report
Loop Definition
Loop Output
Decision Log
Handoff
Runtime Note
```

New values MAY appear without breaking OKF, but validation MUST warn until the profile is updated or an explicit mapping is approved.

## Deterministic mapping rules

| Source pattern | `type` | `context_kind` | Default layer | Default authority |
| --- | --- | --- | --- | --- |
| `01-Soul/Audy Himura.md` | `concept` | `Identity` | `synthesis` | `canonical` |
| `01-Soul/*.md` | `concept` | `Doctrine` | `synthesis` | `canonical` |
| `AGENTS.md`, `OPERATIONS.md` | `concept` | `Operating Contract` | `synthesis` | `canonical` |
| `00-Index.md`, `02-Memory/MEMORY.md` | `concept` | `Memory Index` | `synthesis` | `canonical` |
| `03-Protocols/*.md` | `concept` | `Protocol` | `concept` | `reviewed` |
| `04-Research/**/README.md` | `concept` | `Research Brief` | `analysis` | `working` |
| `04-Research/**/*Concept-Map*.md` | `concept` | `Concept Map` | `analysis` | `working` |
| `04-Research/**/*Gap-Analysis*.md` | `concept` | `Gap Analysis` | `analysis` | `working` |
| `04-Research/**/*Adoption-Spec*.md` | `concept` | `Adoption Spec` | `analysis` | `working` |
| `04-Research/**/*Profile*.md` | `concept` | `Profile Spec` | `analysis` | `working` |
| `04-Research/**/source-pages/*.md` | `resource` | `Source Snapshot` | `concept` | `raw` |
| `08-Hermes/**/*.md` | `concept` | `Harness Lane` | `analysis` | `working` |
| `09-Claude/**/*.md`, `10-Codex/**/*.md`, `11-Antigravity/**/*.md` | `concept` | `Harness Lane` | `analysis` | `working` |
| `05-Daily/*.md` | `concept` | `Daily Log` | `analysis` | `raw` |
| `loops/**/references/*.md` | `concept` | `Loop Definition` | `concept` | `reviewed` |
| `loops/**/archive/*.md` | `concept` | `Loop Output` | `analysis` | `raw` |

Explicit metadata overrides these defaults. A path matching more than one rule MUST use the most specific rule and record overridden inferred fields in the export report.

## Sensitivity defaults

| Path or class | Default |
| --- | --- |
| Reviewed public allowlist | `public` |
| `01-Soul/`, `03-Protocols/`, operating contracts | `internal` |
| `02-Memory/`, `05-Daily/`, personal/runtime material | `private` |
| `04-Research/` reviewed synthesis | `internal` |
| Source snapshots of public material | `internal` until explicitly approved public |
| Secret, token, credential, private-key, or sensitive financial patterns | `restricted` and excluded by default |

Public export is allowlist-only. Path-based sensitivity is a conservative default, not evidence that a note is safe to release.

## Link conversion

The source vault keeps Obsidian wikilinks. The generated bundle converts resolvable wikilinks and Markdown links into standard Markdown body links and emits typed frontmatter links.

Default relations:

- explicit citations or source-index bindings -> `cites` or `derived_from`;
- explicit supersession -> `supersedes`;
- dependency language or declared dependency -> `depends_on`;
- ordinary internal wikilink -> `references`.

Ambiguous or broken links generate warnings. They do not invent a target.

## Default SSA template and onboarding

`default-ssa-v0` is a future barebones template extracted from the Audy/SSA-0 reference implementation after its contracts pass the canary and retrieval gates. It is not a copy of Audy's Vault.

The default template MAY contain:

- Nous core schemas and profile manifests;
- neutral vault structure and navigation;
- authority and source-precedence rules;
- scoped write-lane contracts;
- human-verdict and no-silent-canon-write boundaries;
- conservative sensitivity, export, and security defaults;
- empty ledger, trust-history, runtime-memory, and harness-adapter configuration;
- optional public TFIS/SSA doctrine explicitly approved for reuse;
- onboarding questions and verification checks.

It MUST exclude:

- Khalil Nooh's biography, preferences, private memory, project history, and judgments;
- Audy's personality, voice, relationship history, and peer representations;
- Audy Vault paths, device names, credentials, secrets, and runtime identifiers;
- existing Honcho messages, conclusions, representations, or workspace data;
- SSA-0's ledger, trust scores, verdict history, or source calibration;
- any private or internal artifact not explicitly approved as template material.

Onboarding a new SSA creates, at minimum:

1. a new immutable `ssa_id` and isolated `context_namespace`;
2. a new versioned `ssa_profile` and chosen `familiar_id`;
3. a separate canonical context vault from the default structure;
4. separate SovMem bundle, staging area, ledger, index, and trust history;
5. a separate Honcho workspace or equivalent runtime-memory namespace;
6. profile-scoped credentials, permissions, policy, and harness adapters;
7. explicit optional doctrine imports through a reviewed import record;
8. a clean verification run proving no SSA-0 or Audy data crossed the boundary.

No context, judgment, reputation, or trust score transfers between SSA profiles by default. Cross-profile transfer requires an explicit export/import artifact, source provenance, destination review, and a human verdict from the receiving SSA.

## Source snapshot contract

Every export run MUST identify:

- vault root identity;
- Nous schema, SSA identity, profile instance, familiar identity, and context namespace;
- selected Git commit when using committed mode;
- explicit inclusion and exclusion rules;
- whether any working-tree files were allowed;
- source-file hashes;
- profile version;
- exporter version.

The first canary export MUST use committed mode. Untracked or modified source files are excluded and reported, not silently ingested. Working-tree export may be added later as an explicit diagnostic mode, never as the reproducible baseline.

## Canary scope

The first implementation uses 10-20 committed, low-risk research documents. Selection SHOULD prioritize:

- `04-Research/OKF/` synthesis documents;
- committed ANMS and SovMem research syntheses after human review;
- files with a source index or clear provenance;
- no daily notes, raw Honcho material, private memory, generated media, or restricted content.

The canary validates the profile. It does not satisfy the later SovMem P0 seed-corpus gate by itself. After validation, expand to at least 50 selected documents for the retrieval baseline.

## Validation behavior

Hard failures:

- missing or malformed frontmatter in generated concepts;
- missing required fields;
- invalid core enum values;
- duplicate IDs or resources;
- `judgment` without valid verdict, reason, and subject;
- timestamp fabrication or filesystem-mtime fallback;
- restricted material in a scope that does not explicitly permit it;
- conflicting explicit source metadata;
- source mutation during export;
- source snapshot mismatch;
- missing or inconsistent SSA/profile namespace identity;
- any cross-profile artifact or runtime-memory reference not explicitly imported and reviewed.

Warnings:

- unknown `context_kind`;
- missing optional fields;
- unresolved internal links;
- inferred description or provenance;
- weak or missing citations in research syntheses;
- untracked or modified files excluded from committed mode.

## Export outputs

The implementation will produce:

```text
<output-root>/
├── bundle/
│   ├── index.md
│   └── ... generated concept documents
├── manifest.json
└── export-report.json
```

The output root MUST remain outside the live Audy Vault by default. No generated bundle, cache, index, or raw report becomes canonical merely by existing.

## Verification design

The implementation plan must include tests proving:

1. two exports from the same commit and profile are byte-identical except for explicitly excluded runtime report fields;
2. source files are unchanged before and after export;
3. all generated concepts validate OKF and this profile;
4. each `context_kind` maps to a valid SovMem `type`;
5. restricted material is denied from non-private scopes;
6. invalid judgment records fail;
7. broken and ambiguous links warn without invented targets;
8. committed mode excludes dirty and untracked files;
9. Honcho and harness material cannot bypass source selection or authority mapping;
10. the 10-20 document canary can expand to a 50+ document retrieval corpus without schema changes;
11. changing `ssa_id`, `ssa_profile`, or `context_namespace` cannot expose another profile's bundle, ledger, runtime memory, or trust history;
12. a generated `default-ssa` fixture contains no SSA-0, Khalil, Audy, vault-path, judgment, or Honcho data.

## Non-goals for profile v0

- Bulk frontmatter migration in the live vault.
- Honcho ingestion or synchronization.
- Direct write-back to Audy's Vault.
- Tantivy, Petgraph, MCP, verdict application, or trust scoring.
- Public export of the vault.
- Automatic classification by an LLM.
- Treating format conformance as truth or freshness.
- Shipping multi-SSA onboarding, tenancy, billing, or profile-management runtime in this profile-design phase.

## Approval gate

Approval was given on 2026-07-10. The Nous/SSA profile hierarchy, isolation rules, export fields, enums, mapping precedence, committed-source requirement, and no-mutation boundary are frozen for the exporter/validator implementation plan. Later changes require a versioned profile revision and migration note.
