> **ARCHIVED** — This document has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix B: Archive Map](../sovmem-master-spec.md#appendix-b-archive-map) for the master spec section that covers its content.
>
# SovMem P3 Verified Write-Back Loop Design

**Date:** 2026-07-14

**Status:** Proposed for human review. No implementation is authorized by this document alone.

**Normative parent:** docs/sovmem_v0.3_execution_plan.md

**Source-lineage companion:** docs/spec/source-lineage-june-23-spec-bundle-verbatim.md

**Verified design base:** origin/main at 73c4be50e7df7ebcd1eec8cf2f1dedf4809348ba

## 1. Purpose

P3 adds the first reviewed write path to SovMem. It accepts immutable agent proposals, applies deterministic policy, requires an interactive human verdict, publishes verified canonical bytes for ACCEPT and REWRITE, records every human verdict in an append-only BLAKE3 chain, and completes a full P2 index rebuild before an applied verdict is committed.

This design resolves two authority conflicts without changing the frozen P0-P2 contracts:

1. Audy Vault remains the canonical external context source and is not written by P3.
2. The current 62-document Level 3 export remains immutable P2 evaluation evidence and is not converted in place into a mutable store.
3. P3 introduces a separately named, versioned SovMem canonical lane. Its seed is a byte-for-byte copy of the verified Level 3 export. Later generations are authoritative only for that P3 lane.
4. No automatic promotion or synchronization from the P3 lane back into Audy Vault exists.
5. A consumer must name the lane it is reading. It may not silently treat an evolved P3 generation as the frozen P2 corpus or as a synchronized Audy Vault.

The design commit changes no code, dependency, fixture, profile, policy, bundle, ledger, index, evaluation artifact, or audit state.

## 2. Verified starting point

The design starts from the following live evidence:

- PR #6 is merged into main at 73c4be50e7df7ebcd1eec8cf2f1dedf4809348ba.
- The reviewed P2 head 63fc8c5c6cfec53444f3e0bb283145eee1fd3c3f is the second parent of that merge.
- The isolated branch is codex/sovmem-p3-verified-write-back-loop.
- The untouched baseline passes 55 JavaScript tests and 285 Rust tests.
- npm run audit:sovmem:check, cargo fmt --all -- --check, and Clippy with warnings denied pass.
- The frozen export is bound to Audy Vault commit 43824cfd9caed7b260aa442128fd1130ddb6e2ff.
- The export contains 62 accepted documents, 8 core documents, 54 research documents, 226 warnings, one exclusion, and zero errors.
- The frozen export bundle hash is 49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65.
- The frozen P2 baseline records 23 of 25 top-five hits, Precision@5 0.1840, Recall@5 0.9200, and misses q-005 and q-011.
- The existing export manifest commits source paths and hashes from the pinned external vault. Editing those generated document bytes in place would invalidate source correspondence and the complete-corpus report.
- The existing index builder is full-rebuild-only, verifies Level 3 before writing, builds a complete inactive generation, self-checks it, and atomically switches a CURRENT pointer.

P3 preserves these properties and adds a separate evolving contract rather than weakening them.

## 3. Frozen decisions carried forward

The following decisions are inputs, not design questions:

1. Agents may propose. Only a human verdict may commit canon.
2. There is no automatic acceptance or trust-gated commit path.
3. Verdict application is CLI-only and human-only in P3.
4. No MCP verdict or write authority exists. A future P4 adapter may submit proposals only.
5. The internal verbs are ACCEPT, REJECT, REWRITE, TP, and FP.
6. The public pedagogical subset is ACCEPT, REJECT, and REWRITE.
7. Empty verdict reasons are invalid.
8. Confidence is optional, from 0 through 1 inclusive, captured before drafting, and never authoritative.
9. Evaluator and skeptic output is advisory only. The skeptic remains harness-side.
10. Filesystem staging plus the five-verb ledger is the write-back architecture.
11. Git is optional outer snapshot evidence, not the judgment ledger.
12. Hash targets are raw bytes.
13. Provenance remains tier1, secondary, synthesis, or unknown.
14. New resources require an explicit provenance value.
15. The project remains one Rust crate on Rust 1.84.
16. Tantivy remains exactly 0.24.2 and Petgraph exactly 0.8.3.
17. No dependency is added without separate explicit approval and an exact pin.
18. The P2 index remains disposable and full-rebuild-only.
19. The frozen corpus, golden queries, relevance labels, thresholds, and baseline are not edited to make P3 pass.
20. The binary performs no embeddings, vector search, model calls, or network calls.
21. No unsafe code is permitted.
22. No async is permitted outside the future MCP module.
23. The private repository continues to rely on manual pull-request merge discipline. P3 will not claim enforced branch protection, rulesets, required checks, or merge queues.

## 4. Scope

### 4.1 In scope

P3 designs and will eventually implement:

- immutable candidate Markdown and bound proposal metadata;
- a strict, versioned policy.toml contract and deterministic PolicyEvaluator;
- proposal intake receipts, queue/list/status interfaces, quotas, deduplication, and queue alarms;
- the five-verb human verdict CLI;
- exact-byte ACCEPT and human-supplied exact-byte REWRITE;
- deterministic human Judgment documents for applied verdicts;
- ledger-only REJECT, TP, and FP;
- copy-on-write canonical generations and reconstructed integrity manifests;
- a complete P2 full index rebuild before each applied verdict commits;
- an append-only BLAKE3-chained verdict ledger;
- verify --level 4 over the store and the complete ledger chain;
- stable, redacted machine diagnostics and stable text and JSON success output;
- exclusive mutation locking, idempotency, crash detection, and deterministic recovery;
- fixture-first policy, poisoning, corruption, crash, concurrency, and privilege tests;
- a safe-clone real-data proof;
- the P3 audit gate and approximately 63 percent overall audit result after, and only after, the entire gate passes.

### 4.2 Explicit non-scope

P3 does not include:

- any write to live Audy Vault;
- any write to the current frozen 62-document export;
- any automatic synchronization or promotion into Audy Vault;
- any automatic Git commit;
- a Git-native judgment ledger;
- MCP transport, an MCP verdict tool, or an MCP write tool;
- P4 adapter work;
- P5 sentry, trust command, trust routing, audit sampling, dogfood scheduling, or long-run hardening;
- TTF or outcome-calibration integration;
- automatic canon promotion;
- incremental indexing;
- changes to retrieval boosts, tokenization, ranking, golden queries, thresholds, relevance labels, or baseline;
- a new database, vector store, model call, network call, or dependency;
- cryptographic human identity attestation;
- multi-user authorization or protection from a fully compromised host.

## 5. Authority approaches considered

### 5.1 Approach A: a dedicated versioned SovMem canonical lane

Create a new store named audy-sovmem-writeback-v1. Copy the verified Level 3 export into its immutable seed area. Keep later canonical generations under that store, bind them to a versioned P3 profile and policy, and record their authority in the verdict ledger.

Advantages:

- preserves the README statement that Audy Vault is canonical external context;
- preserves the normative execution-plan rule that the SovMem bundle is canon for the write-back loop;
- leaves the P2 export and evaluation contract byte-identical;
- makes every post-verdict generation and its lineage explicit;
- permits safe clone testing without any live write;
- allows copy-on-write publication and deterministic crash recovery.

Costs:

- Audy Vault and the P3 lane can intentionally diverge;
- consumers must name which authority lane they read;
- promotion back to Audy Vault remains a separate human workflow.

This is the selected approach.

### 5.2 Approach B: write an approved clone of Audy Vault and re-export

Apply ACCEPT or REWRITE to an approved vault clone, create a new Git snapshot, then run the exporter again.

Advantages:

- preserves a single source-authoring format;
- reuses the current source-to-export direction.

Rejected for P3 because:

- the execution plan says the canonical bundle is written only by verdict application;
- source Markdown and generated SovMem Markdown are not the same schema;
- Git commit automation and vault synchronization are explicitly excluded;
- the write-back transaction would span a source repository, exporter, bundle, ledger, and index without one reviewed commit point;
- it would make the safe-clone path look deceptively similar to a live-vault write path.

### 5.3 Approach C: mutate the existing 62-document export

Edit var/audy-sovmem-retrieval-corpus in place and rebuild the index.

Rejected because it would:

- break the frozen source-hash and source-commit correspondence;
- break the current corpus report and exact document contract;
- make the canonical P2 baseline irreproducible;
- conflate derived export output with a durable write-back store;
- violate the instruction not to change frozen evidence to make P3 pass.

## 6. Exact authority model

Authority is operation-specific:

| Artifact | Authority | Mutable in P3 | Relationship |
| --- | --- | ---: | --- |
| Audy Vault at the pinned Git snapshot | Canonical external Audy context and source history | No | Source of the frozen seed export |
| Frozen Level 3 export | Canonical P2 evaluation input | No | Byte-locked evidence derived from Audy Vault |
| Frozen golden, thresholds, relevance labels, and baseline | Canonical P2 regression evidence | No | Always evaluated against the frozen export |
| audy-sovmem-writeback-v1 seed | Immutable P3 genesis input | No after sealing | Byte-for-byte copy of the verified Level 3 export |
| Active P3 canonical generation | Canon for P3 write-back queries and later P3 verdicts | Only through verdict transaction | Derived from seed plus applied human verdicts |
| policy.toml | Current human-authored deterministic policy | Only by an out-of-band human atomic file replacement | Its raw hash binds new proposals and verdicts |
| policies/<policy-hash>.toml | Immutable historical policy bytes | Copied only by the human verdict transaction | Lets Level 4 verify every historical policy hash |
| Proposal pair | Immutable proposed bytes and metadata | No after staging | Never canon |
| Verdict ledger | Human judgment history and transaction commit record | Logical append only | Not replaced by Git |
| Re-index receipt | Durable evidence that a complete P2 rebuild passed | No | Bound into an applied ledger event |
| Tantivy/Petgraph index | Disposable retrieval derivative | Rebuildable | Never authority |
| Git commit | Optional outer snapshot identity | Manual only | Never a verdict or ledger event |

No rule copies P3 changes back to Audy Vault. A later promotion design must specify direction, conflict handling, and human review. Until then, the P3 status output must state store_id and profile_version so no consumer can imply synchronization.

The exact canonical write target for an applied verdict is `<approved-p3-store>/canon/generations/<generation-id>/bundle/<target-path>`, plus the transaction-generated Judgment document and manifest in that same immutable generation. The approved P3 store root is separately human-provisioned, must resolve outside both the live Audy Vault and the frozen P2 export, and is passed explicitly with `--store`. Store/profile identity and seed hashes must match before any prompt. The deployment capability grants the verdict process write access only to that approved store; live Vault and frozen-export roots are absent or read-only. The safe-clone harness resolves all three roots and refuses equality, ancestor, or descendant overlap before granting that capability. The binary itself accepts only the exact P3 store shape and store ID, so a bare Vault or frozen export is not a valid store. There is no override flag.

## 7. Threat model and trust boundaries

### 7.1 Protected properties

P3 protects:

- canon from direct agent writes;
- canon from unruled or policy-invalid proposals;
- exact approved bytes from normalization or substitution;
- historical verdicts from deletion, reorder, or unnoticed mutation;
- the current head from stale-base or partial transaction use;
- the frozen P2 evidence from P3 writes;
- CLI logs and diagnostics from candidate bodies, reasons, absolute paths, and sensitive source excerpts;
- confidence and evaluator data from gaining commit authority.

### 7.2 Adversaries and faults in scope

Tests assume:

- a malicious or buggy proposer can submit malformed metadata and candidate bytes;
- a proposer can claim high confidence or favorable evaluator advice;
- an evaluator block can be malformed, unallowlisted, or bound to the wrong candidate;
- a proposer can attempt to forge a Judgment document;
- two proposal processes or two verdict processes can race;
- the process can crash at every named transaction boundary;
- files can be truncated, reordered, duplicated, replaced, symlinked, or edited;
- a stale proposal can be ruled after canon or policy changes;
- index construction, ledger publication, pointer publication, fsync, or cleanup can fail;
- stdout and stderr may be captured by automation.

### 7.3 Explicit boundary of the human-only claim

P3 does not pretend that a TTY proves a biological person or that a process running with Khalil's unrestricted OS identity can be distinguished cryptographically from Khalil.

The enforceable local boundary is:

1. Agents and future MCP adapters receive only proposal capability.
2. seed/, canon/, ledger/, policies/, evidence/, derived/, transactions/, locks/, and policy.toml are outside agent-writable filesystem roots. Proposers receive read access to current commitments and write access only to staging/ and its intake lease.
3. A harness that can run arbitrary local commands as the same unrestricted OS identity is not permitted access to the production canonical store.
4. Verdict application opens the controlling terminal directly, refuses non-interactive input, displays the exact decision material there, and requires a fresh typed confirmation.
5. There is no non-interactive approval flag, environment-variable bypass, stdin pipe, library API, MCP tool, or trust shortcut.
6. The actor recorded by the ledger comes from policy.toml, not a caller-supplied --actor flag.

This is a path-and-capability boundary suitable for the solo local P3 threat model. Hardware-backed signatures or multi-user authorization would require a new reviewed design and likely new dependencies.

### 7.4 Out of scope threats

- a compromised kernel or filesystem;
- root or administrator tampering;
- an attacker with unrestricted access to the human OS account and production store;
- cryptographic attribution of who physically typed a verdict;
- denial of service by deleting the entire store or withholding disk space.

Level 4 remains tamper-evident rather than tamper-proof.

## 8. Dependencies and exact pins

No dependency is added by P3.

The direct dependency contract remains:

| Dependency | Exact version |
| --- | --- |
| anyhow | 1.0.95 |
| blake3 | 1.5.5 |
| clap | 4.5.27 |
| serde | 1.0.219 |
| serde_json | 1.0.140 |
| serde_yaml | 0.9.34 |
| tantivy | 0.24.2 |
| petgraph | 0.8.3 |
| thiserror | 2.0.11 |

Rust remains 1.84.0 and the committed Cargo.lock remains authoritative.

policy.toml uses a deliberately small canonical TOML subset implemented inside the existing crate. It is not a general TOML parser:

- UTF-8 only;
- LF line endings and exactly one final LF;
- one flat key per line in the specified order;
- exact key, one ASCII space, equals sign, one ASCII space, value;
- values are quoted ASCII strings, decimal unsigned integers, booleans, or arrays of quoted ASCII strings;
- no tables, dotted keys, comments, escapes, multiline values, floats, implicit values, duplicate keys, or unknown keys.

This subset is valid TOML and sufficient for the fixed v1 schema. If implementation proves that a general TOML parser is necessary, work stops for explicit dependency approval.

## 9. Module boundaries and public interfaces

P3 remains inside the single sovmem crate.

### 9.1 New focused modules

- profile/store.rs
  - validates store roots and exact layouts;
  - resolves the ledger-authoritative current canonical generation;
  - enforces same-filesystem and symlink restrictions.

- profile/policy.rs
  - parses canonical policy.toml;
  - computes the raw policy hash;
  - evaluates ordered rules and returns stable rule identifiers.

- profile/proposal.rs
  - parses candidate bytes and proposal metadata;
  - computes IDs and commitments;
  - stages immutable proposal pairs and intake rejection receipts;
  - derives proposal state from staging plus ledger evidence.

- profile/judgment.rs
  - validates the five verbs;
  - validates reasons;
  - constructs deterministic human Judgment documents only for ACCEPT and REWRITE.

- profile/canon.rs
  - constructs copy-on-write generations;
  - reconstructs index.md and the canonical manifest;
  - performs P3 Level 1-3 verification.

- profile/ledger.rs
  - serializes strict canonical events;
  - appends by prefix-preserving atomic replacement;
  - verifies the BLAKE3 chain and ledger semantics.

- profile/transaction.rs
  - owns the only runtime canonical mutation coordinator;
  - acquires the protected verdict lease and then the staging intake lease;
  - constructs, verifies, re-indexes, commits, publishes, and recovers.

- profile/clock.rs
  - provides production UTC timestamps and injected deterministic test timestamps.

- cli/p3.rs
  - owns propose, proposals, verdict, verdicts, recover, Level 4, and store-stats rendering;
  - preserves the shared CLI diagnostic contract.

### 9.2 Existing modules extended, not replaced

- profile/verify.rs adds VerificationLevel::Four and dispatch for a store.
- profile/index.rs accepts a verified evolving canonical generation in addition to the frozen corpus export. Its schema, tokenizer, ranking, full rebuild, and self-check behavior do not change.
- profile/stats.rs adds queue and ledger evidence only in store mode. Existing bundle-mode JSON remains byte-compatible.
- profile/frontmatter.rs and profile/validate.rs validate exact candidate and human Judgment bytes.
- cli.rs adds command definitions and routes P3 execution to cli/p3.rs.

### 9.3 Visibility rule

The function that can install a canonical generation is crate-private to profile/transaction.rs. proposal.rs, CLI parsing, tests, and the future MCP module cannot call a lower-level canonical writer. Tests exercise the transaction coordinator through explicit test-only capability injection; production exposes only the interactive verdict CLI.

## 10. Versioned profile and canonical lane

P3 creates a new tracked profile configuration:

    profiles/audy-ssa0-p3-v1/profile.yaml

It copies the existing identity tuple unchanged:

- profile: nous-sovmem-v0
- nous_schema: nous-ssa-context-v0
- ssa_id: ssa-0
- ssa_profile: audy-ssa0-v0
- familiar_id: audy
- context_namespace: ssa-0
- vault_id: audy-vault

It preserves every P2 mapping and adds:

- context kind Human Judgment mapped to judgment;
- path judgments/*.md mapped to:
  - type judgment;
  - context kind Human Judgment;
  - layer concept;
  - authority canonical;
  - sensitivity internal.

The new file path is the profile version identifier audy-ssa0-p3-v1. Its raw BLAKE3 is recorded in policy, proposals, generation manifests, and ledger events. The existing profiles/audy-ssa0-v0/profile.yaml remains byte-frozen for P2.

The P3 profile is also immutable after genesis. Any profile-schema or mapping change requires a new versioned profile file, new store_id, and new reviewed migration design; it is not an in-place policy change.

The P3 store_id is:

    audy-sovmem-writeback-v1

The frozen P2 evaluation_id remains:

    audy-sovmem-v0

Neither identifier aliases the other.

## 11. Store layout

The approved local store layout is:

    <store>/
    ├── policy.toml
    ├── policies/
    │   └── <policy-hash>.toml
    ├── seed/
    │   ├── manifest.json
    │   ├── export-report.json
    │   ├── corpus-report.json
    │   └── bundle/
    ├── staging/
    │   ├── .intake-lock/
    │   │   └── owner.json
    │   ├── proposals/
    │   │   └── <two-hex>/<proposal-id>/
    │   │       ├── candidate.md
    │   │       └── proposal.meta.json
    │   └── intake-rejections/
    │       └── <receipt-id>.json
    ├── ledger/
    │   └── verdicts.jsonl
    ├── canon/
    │   ├── CURRENT
    │   └── generations/
    │       └── <generation-id>/
    │           ├── manifest.json
    │           └── bundle/
    ├── evidence/
    │   └── reindex/
    │       └── <verdict-id>.json
    ├── derived/
    │   └── indexes/
    │       └── <generation-id>/
    ├── transactions/
    │   └── <transaction-id>/
    │       ├── prepared.json
    │       ├── ledger-committed.json
    │       └── published.json
    └── locks/
        └── verdict.lock/
            └── owner.json

Angle-bracket values above are runtime identifiers, not unresolved design choices.

### 11.1 Seed provisioning and sealing

seed/ is provisioned before the P3 write-back authority begins:

1. In a safe sandbox, copy the complete verified P2 export root byte-for-byte.
2. Verify it at explicit Level 3 with the frozen P2 profile.
3. Assert its bundle hash, source commit, manifest hash, corpus-report hash, and exact file inventory.
4. Make the seed non-writable to the proposer boundary.
5. Place the P3 policy beside it.

There is no production canon-init command. The first human verdict transaction writes a genesis event and its verdict event together in one atomic ledger publication. Before that first verdict, proposals may bind to the verified seed, but verify --level 4 reports ledger.genesis_missing.

The genesis event seals the exact seed manifest hash, bundle hash, corpus-report hash, source commit, P3 profile hash, policy hash, and store_id. After genesis, changing seed bytes is a Level 4 failure.

The existing P1 corpus exporter is not allowed to target a sealed P3 store. Runtime code contains no path that replaces seed/.

### 11.2 Canonical generations

The seed is the logical generation zero and canon/CURRENT initially contains exactly:

    seed

followed by one LF.

Applied generations are immutable directories named:

    gen-0000000000000001

The decimal component is 16 digits, starts at one, and increases by one only for ACCEPT or REWRITE. REJECT, TP, and FP do not consume a generation number.

Generation directories never contain an index, staging data, transaction state, ledger, absolute path, hostname, username, PID, or random value. Their only contents are manifest.json and bundle/.

## 12. Identifier, time, and source formats

### 12.1 Proposal ID

Format:

    prp-<64 lowercase hexadecimal characters>

The hash is BLAKE3 over the domain sovmem.proposal.v1 plus length-framed semantic proposal fields in fixed order. The preimage includes submitted_at, source_id, operation, intent, target_id, target_path, all base commitments, candidate_hash, candidate byte count, policy_hash, profile_hash, confidence presence/value, and the evaluator commitment. It excludes proposal_id and meta_hash to avoid recursion.

The ID is content-bound, not a bearer secret. A repeated preimage produces the same ID and fails as proposal.duplicate_id.

### 12.2 Verdict ID

Format:

    ver-<64 lowercase hexadecimal characters>

It is BLAKE3 over the domain sovmem.verdict.v1 plus proposal_id, verb, exact reason bytes, rewrite hash or null, actor from policy, ruled_at, current ledger head, policy hash, and profile hash.

### 12.3 Ledger event ID

Format:

    evt-0000000000000000

The suffix is a 16-digit zero-based sequence. Event zero is genesis. The first verdict is event one. Event ID uniqueness is scoped to store_id and is independently protected by event_hash.

### 12.4 Transaction and rejection receipt IDs

Transaction:

    txn-<64 lowercase hexadecimal characters>

Receipt:

    rec-<64 lowercase hexadecimal characters>

Both are domain-separated BLAKE3 commitments over fixed semantic inputs. They are identifiers, not credentials.

### 12.5 Source identity

Format:

    src:<kind>:<name>

kind is exactly human, agent, harness, or fixture. name is 1 through 64 lowercase ASCII characters matching:

    [a-z0-9][a-z0-9._-]{0,63}

Production policy permits agent and harness proposers. fixture is accepted only by test policies. The human actor is fixed in production policy as src:human:khalil-nooh.

### 12.6 Timestamp

All persisted timestamps are UTC RFC 3339 with exactly nine fractional digits and Z:

    2026-07-14T04:30:00.000000000Z

Production uses SystemTime. Tests inject a Clock trait. IDs are not security tokens, so deterministic time-plus-content commitments are sufficient; any collision fails closed through create-new semantics.

## 13. policy.toml v1

### 13.1 Exact required schema and reviewed initial values

The initial production file contains every key below in this exact order:

    schema_version = "sovmem-policy-v1"
    store_id = "audy-sovmem-writeback-v1"
    profile_version = "audy-ssa0-p3-v1"
    human_actor = "src:human:khalil-nooh"
    allowed_proposer_prefixes = ["src:agent:", "src:harness:"]
    allowed_evaluator_prefixes = ["src:agent:", "src:harness:"]
    evaluator_protocol = "adversarial-bug-finding-v1"
    allowed_targets = ["01-Soul/*.md", "03-Protocols/*.md", "04-Research/**/*.md"]
    protected_layers = ["synthesis"]
    forbidden_proposal_types = ["judgment"]
    provenance_values = ["tier1", "secondary", "synthesis", "unknown"]
    max_candidate_bytes = 262144
    max_meta_bytes = 65536
    max_rewrite_bytes = 262144
    max_reason_bytes = 4096
    max_evaluator_reasons = 10
    max_evaluator_reason_bytes = 1024
    max_open_per_source = 10
    max_submissions_per_source_utc_day = 20
    max_open_total = 50
    queue_alarm_at = 25
    bad_accept_cost = 100
    human_verdict_cost = 1

There are no runtime default fallbacks. These are the reviewed template defaults; a missing field fails closed. The cost fields are retained as deterministic P3 policy evidence but do not compute trust or alter authority in P3.

allowed_targets patterns are ASCII and match the complete canonical target_path. A literal matches itself, * matches zero or more non-slash UTF-8 bytes inside one component, and ** is valid only as a complete component and matches zero or more complete components. ?, character classes, braces, escapes, empty components, parent components, platform prefixes, and any other metacharacter are invalid policy values. Pattern order never changes the result.

### 13.2 Policy hash and changes

policy_hash is the lowercase BLAKE3 of exact policy.toml bytes. No canonical normalization occurs after reading. A semantically equivalent byte change produces a different policy hash.

policy.toml is never modified by the binary. A human publishes a complete replacement out of band by same-directory temporary file, file sync, atomic rename, and parent-directory sync while neither lease exists. A partial or concurrently changed file fails closed. The human may optionally record the outer repository snapshot in Git.

The first verdict using a policy copies its exact bytes into policies/<policy_hash>.toml with create-new and fsync before ledger publication. An existing snapshot must match the current bytes exactly. Historical snapshots are immutable. The permitted policy snapshot set is the hashes referenced by genesis/verdict events plus the current policy hash, which may be awaiting its first verdict.

Any staged proposal with a different current policy_hash becomes stale and must be submitted again. Level 4 validates each historical event against its archived policy bytes; it does not require every historical event to use the current policy.

### 13.3 Parse failures

The strict parser emits:

- policy.read
- policy.utf8
- policy.line_endings
- policy.syntax
- policy.key_order
- policy.duplicate_key
- policy.unknown_field
- policy.missing_field
- policy.schema_unsupported
- policy.value_invalid
- policy.invariant_invalid
- policy.changed_during_transaction

Unknown fields and unsupported schema versions fail closed before proposal or verdict evaluation.

## 14. Proposal pair and binding

### 14.1 Immutable storage

Each accepted proposal is published by:

1. creating a hidden same-filesystem temporary directory;
2. writing candidate.md and proposal.meta.json with create-new semantics;
3. flushing and synchronizing both files;
4. synchronizing the temporary directory;
5. atomically renaming the directory to its sharded proposal path;
6. synchronizing the proposals parent.

After publication, proposal files are never edited, moved, or deleted in P3. State is derived from their bytes and the ledger.

### 14.2 Candidate bytes

candidate.md contains the exact bytes proposed for the target:

- valid UTF-8;
- complete Markdown with complete P3 profile frontmatter;
- no normalization, newline insertion, YAML reserialization, or body rewriting;
- no generated index.md;
- no Judgment document;
- no more than max_candidate_bytes;
- exact BLAKE3 recorded in metadata.

ACCEPT applies these exact bytes. REWRITE never applies them.

### 14.3 proposal.meta.json v1

The file is one compact JSON object followed by exactly one LF. Field order is fixed:

    {
      "schema_version": "sovmem-proposal-v1",
      "proposal_id": "prp-...",
      "submitted_at": "2026-07-14T04:30:00.000000000Z",
      "source_id": "src:agent:example",
      "operation": "create",
      "intent": "accept",
      "target_id": "ssa-0/04-Research/Example.md",
      "target_path": "04-Research/Example.md",
      "base": {
        "kind": "seed",
        "generation_id": null,
        "bundle_hash": "...",
        "manifest_hash": "...",
        "target_hash": null
      },
      "candidate_hash": "...",
      "candidate_bytes": 1234,
      "policy_hash": "...",
      "profile_hash": "...",
      "confidence": null,
      "evaluator": null
    }

Strict parsing denies unknown fields at every level.

operation is create or replace:

- create requires target_hash null and target absence;
- replace requires a 64-character target_hash equal to current raw target bytes.

intent is accept or rewrite:

- a protected synthesis target requires rewrite;
- intent is not a verdict and grants no authority;
- ACCEPT is permitted only for accept intent;
- REWRITE may rule either intent when the human supplies replacement bytes.

Base kind is seed or generation. generation_id is null for seed and required otherwise.

confidence is null or a JSON number from 0 through 1 inclusive. NaN, infinity, strings, and excess precision beyond six decimal places are rejected so serialization and later calibration remain deterministic. The value is copied to the ledger but never read by PolicyEvaluator when deciding pass or fail.

The loop-facing protocol requires the source to elicit confidence before drafting. Because propose receives an already written candidate, the binary can enforce the value and binding but cannot prove when the estimate was formed. Harness fixtures must prove pre-draft capture; the ledger treats the value as an untrusted self-report.

### 14.4 Evaluator block

When present:

    {
      "protocol": "adversarial-bug-finding-v1",
      "source_id": "src:agent:skeptic",
      "evaluated_at": "2026-07-14T04:29:00.000000000Z",
      "candidate_hash": "...",
      "recommendation": "looks-sound",
      "reasons": ["No policy conflict found."]
    }

recommendation is exactly looks-sound, needs-rewrite, or reject.

The block must:

- use the policy evaluator protocol;
- use an allowed evaluator prefix;
- bind the exact candidate hash;
- contain 1 through max_evaluator_reasons non-empty reasons;
- obey per-reason byte and control-character limits;
- use an evaluator source distinct from the proposer.

This validates structure and binding, not cryptographic identity. Under P3's source trust boundary, a mismatched, malformed, unallowlisted, or self-evaluated block is forged and rejected. A process that can impersonate an allowlisted source at the host boundary is outside P3's threat model.

Policy evaluation does not branch on recommendation, reasons, confidence, evaluator presence, or evaluator source.

## 15. Intake rejection receipts and privacy

A failed intake does not stage candidate bytes and does not enter the verdict ledger.

If the command can safely derive a receipt ID and timestamp, it writes an immutable rejection receipt:

    {
      "schema_version": "sovmem-intake-rejection-v1",
      "receipt_id": "rec-...",
      "received_at": "2026-07-14T04:30:00.000000000Z",
      "source_id": "src:agent:example",
      "rule_codes": ["proposal.base_stale"]
    }

source_id is null when it did not pass source validation.

Receipts never contain:

- candidate or metadata bytes;
- candidate, target, manifest, or policy hashes;
- target ID or target path;
- reason or evaluator text;
- absolute paths;
- parser excerpts.

Receipt publication uses create-new plus fsync and does not affect the verdict chain. If receipt publication itself fails, the proposal remains rejected and diagnostics include intake.receipt_write without exposing the rejected content.

rejected-at-intake is an intake-attempt state identified by receipt_id, not a staged proposal state.

## 16. Proposal state machine

The immutable proposal pair and ledger derive these states:

1. rejected-at-intake
   - no proposal pair exists;
   - a redacted receipt may exist;
   - never ruleable.

2. staged
   - immutable pair exists;
   - no terminal ledger event references proposal_id;
   - appears in the open queue.

3. ruled
   - exactly one terminal ledger event references proposal_id;
   - all five verbs reach this state.

4. applied
   - ruled with ACCEPT or REWRITE;
   - the ledger event references a verified canonical generation and re-index receipt;
   - the ledger-derived canonical head includes the applied target and Judgment document.

5. superseded
   - an applied proposal's target was changed by a later applied verdict;
   - derived by walking later applied target entries;
   - its generation and proposal remain retained.

6. retained
   - storage disposition orthogonal to decision state;
   - every staged proposal pair and every ledger event is retained for all of P3.

An incomplete transaction is not a proposal state. It is a store condition that makes mutation and P3 reads fail closed until recovery.

## 17. PolicyEvaluator order and stable rule identifiers

Evaluation is deterministic and stops at the first numbered stage, but emits all sorted, deduplicated failures within that stage.

### Stage 1: store, profile, and policy preconditions

- store.layout_invalid
- store.path_unsafe
- store.symlink
- profile.read
- profile.schema
- profile.hash_mismatch
- policy codes from Section 13

### Stage 2: proposal pair structure and binding

- proposal.pair_incomplete
- proposal.meta_too_large
- proposal.meta_parse
- proposal.meta_schema
- proposal.meta_unknown_field
- proposal.id_invalid
- proposal.id_mismatch
- proposal.content_hash_mismatch
- proposal.content_size_mismatch
- proposal.candidate_utf8

### Stage 3: source, confidence, and evaluator structure

- proposal.source_invalid
- proposal.source_forbidden
- proposal.confidence_invalid
- proposal.evaluator_invalid
- proposal.evaluator_forbidden
- proposal.evaluator_self_review
- proposal.evaluator_hash_mismatch

### Stage 4: uniqueness and flood control

- proposal.duplicate_id
- proposal.duplicate_content
- proposal.quota_source_open
- proposal.quota_source_daily
- proposal.quota_total

duplicate_content is the exact pair of target_id and candidate_hash across all retained proposals, including ruled proposals.

Open quotas count staged proposals with no terminal event. Daily source quotas count both staged pairs and redacted intake-rejection receipts whose source_id passed validation and whose binary-assigned UTC timestamp falls on the same UTC calendar date as the new binary-assigned timestamp. The caller cannot supply either timestamp. queue_alarm_at must be no greater than max_open_total; every other quota invariant is checked when policy is parsed.

### Stage 5: target path and base commitments

- proposal.target_unsafe
- proposal.target_forbidden
- proposal.operation_invalid
- proposal.intent_invalid
- proposal.base_kind_invalid
- proposal.base_generation_stale
- proposal.base_bundle_stale
- proposal.base_manifest_stale
- proposal.base_target_stale
- proposal.create_target_exists
- proposal.replace_target_missing

target_path must be canonical relative UTF-8, contain no parent, root, current-directory, empty, NUL, backslash, control, or platform-prefix component, end in .md, match target_id, match the profile mapping, and match policy allowed_targets. index.md, manifest.json, judgments/, staging/, ledger/, evidence/, derived/, transactions/, locks/, policy.toml, and CURRENT are always forbidden.

### Stage 6: candidate size and document conformance

- proposal.candidate_too_large
- existing frontmatter and document diagnostics
- proposal.profile_mismatch
- proposal.target_binding
- proposal.type_binding
- proposal.judgment_forbidden
- proposal.provenance_required
- proposal.synthesis_rewrite_required

All existing Level 1 identity, date, link, secret, sensitivity, and provenance checks remain in force. A new resource must contain provenance explicitly even when the value is unknown.

### Stage 7: advisory-authority invariants

The evaluator reruns the decision with confidence and evaluator fields erased. Pass/fail and rule codes must be byte-identical to the first run. A difference is:

- policy.advisory_authority_violation

This defense-in-depth invariant proves advisory fields cannot influence policy.

The same complete evaluation runs at proposal intake and again under the exclusive lock immediately before a verdict.

## 18. Five-verb semantics

Each proposal receives exactly one terminal human verb.

### 18.1 ACCEPT

- valid only for proposal intent accept;
- applies candidate.md raw bytes exactly to target_path;
- target may be create or replace as declared;
- mints one deterministic human Judgment document;
- reconstructs the complete canonical manifest and index.md;
- completes and self-checks a full P2 index rebuild;
- commits one verdict ledger event;
- advances the canonical generation by one.

### 18.2 REWRITE

- requires exactly one of --rewrite-file or an interactive rewrite capture; P3 initially implements --rewrite-file;
- reads exact raw bytes from a regular, non-symlink file;
- never records the supplied filesystem path;
- validates the rewrite as the full target document under current policy;
- applies the rewrite bytes, not candidate.md;
- permits a protected synthesis edit because the human supplied and confirmed the final bytes;
- mints one deterministic human Judgment document;
- rebuilds and commits exactly like ACCEPT.

### 18.3 REJECT

- means the human does not choose the proposed canonical change;
- records the proposal, reason, confidence, evaluator commitment, and unchanged canonical head in the ledger;
- does not create, edit, or delete any canonical document;
- does not build or switch an index.

### 18.4 TP

- means the proposal correctly surfaced a real issue or signal, but its candidate bytes are not accepted as canon;
- is useful later for source and detector measurement;
- is ledger-only and does not mutate canon or index.

### 18.5 FP

- means the proposal surfaced a spurious or incorrect issue or signal;
- is ledger-only and does not mutate canon or index.

TP and FP are not aliases for ACCEPT or REJECT. They classify signal quality while deliberately withholding canonical mutation.

## 19. Reason and REWRITE binding

A reason:

- is required for every verb;
- is valid UTF-8;
- contains at least one non-whitespace Unicode scalar;
- is no more than max_reason_bytes;
- contains no NUL and no control characters other than LF and tab;
- is preserved exactly in the ledger;
- is hashed as exact UTF-8 bytes;
- is never echoed in diagnostics or default success output.

The CLI accepts exactly one of:

- --reason <text>
- --reason-file <regular-non-symlink-file>

--reason-file is recommended because command-line history may retain --reason text. The path is never persisted or emitted.

REWRITE additionally requires:

- --rewrite-file <regular-non-symlink-file>

ACCEPT, REJECT, TP, and FP reject --rewrite-file. REWRITE rejects a missing rewrite file.

The rewrite hash, rewrite byte count, target output hash, Judgment document hash, manifest hash, and bundle hash are computed before the human confirmation prompt and shown on the controlling terminal.

## 20. Human Judgment document

ACCEPT and REWRITE create exactly one document:

    bundle/judgments/<verdict-id>.md

The document is generated only inside the verdict transaction. Agent proposals targeting judgments/ or declaring type judgment fail proposal.judgment_forbidden.

The document uses the P3 profile and includes:

- id bound to ssa-0/judgments/<verdict-id>.md;
- type judgment;
- context_kind Human Judgment;
- layer concept;
- source and resource equal to its canonical vault URI;
- source_kind generated-report;
- provenance synthesis;
- status active;
- authority canonical;
- sensitivity internal;
- created and updated equal to the UTC calendar date of ruled_at;
- tags human-verdict and the lowercase verb;
- one references link to target_id;
- verdict ACCEPT or REWRITE;
- exact reason;
- subject target_id;
- no inferred fields.

The body contains a deterministic heading, subject ID, verb, and exact reason so the existing P2 title/tags/body schema can retrieve it without changing retrieval behavior.

The vault:// value remains the profile's canonical namespace URI required by the existing document schema. It does not assert that the Judgment document has been synchronized into Audy Vault.

Before confirmation, the CLI writes the exact generated bytes to the controlling terminal and displays their raw BLAKE3. The human confirmation therefore approves both the target bytes and deterministic Judgment bytes.

REJECT, TP, and FP do not mint Judgment documents. Their reasons remain queryable through verdicts list, not through the P2 document index. This intentionally follows the frozen ledger-only requirement even though the older June 23 source recovery described a REJECT-centric query demo.

## 21. Canonical generation manifest v1

An applied generation contains strict manifest.json with schema:

    sovmem-canon-manifest-v1

Fixed field order:

    {
      "schema_version": "sovmem-canon-manifest-v1",
      "store_id": "audy-sovmem-writeback-v1",
      "profile_version": "audy-ssa0-p3-v1",
      "profile_hash": "...",
      "generation_id": "gen-0000000000000001",
      "parent_kind": "seed",
      "parent_generation_id": null,
      "parent_manifest_hash": "...",
      "seed_bundle_hash": "...",
      "seed_manifest_hash": "...",
      "policy_hash": "...",
      "applied_proposal_id": "prp-...",
      "applied_verdict_id": "ver-...",
      "documents": [],
      "bundle_hash": "..."
    }

Each documents entry has fixed fields:

    {
      "canonical_path": "04-Research/Example.md",
      "seed_source_path": "04-Research/Example.md",
      "output_path": "bundle/04-Research/Example.md",
      "id": "ssa-0/04-Research/Example.md",
      "resource": "vault://ssa-0/04-Research/Example.md",
      "hash": "...",
      "origin_kind": "verdict",
      "seed_source_hash": null,
      "proposal_id": "prp-...",
      "verdict_id": "ver-...",
      "previous_hash": null
    }

For seed-inherited documents:

- canonical_path is the canonical path inside the P3 lane;
- seed_source_path and seed_source_hash are copied from the frozen export manifest;
- origin_kind is seed;
- proposal_id, verdict_id, and previous_hash are null.

For verdict-written documents:

- origin_kind is verdict;
- proposal_id and verdict_id are required;
- previous_hash is required for replace and null for create;
- a replacement whose lineage began in the seed preserves the original seed_source_path and seed_source_hash only as historical lineage commitments;
- a created document has null seed_source_path and seed_source_hash.

seed_source_path never claims that the corresponding P3 bytes still exist in or have synchronized to Audy Vault. canonical_path is the only write target. A verdict-written hash is authoritative only inside the named P3 generation.

The manifest has no self-hash. manifest_hash is BLAKE3 of the exact manifest bytes and is recorded by the ledger and subsequent generations.

documents are sorted by raw UTF-8 canonical_path bytes. Duplicate canonical paths, IDs, resources, or output paths fail. The bundle root uses the existing P1 length-framed raw-byte BLAKE3 over every bundle/ path and exact bytes, including reconstructed index.md.

The manifest is a new schema, not a mutation of the frozen export manifest. Level 3 dispatches by manifest schema and never treats a canonical generation as satisfying the frozen complete-corpus contract.

## 22. P3 Level 1-3 verification

The evolving canonical verifier is cumulative and at least as strict as the existing verifier:

### Level 1

- exact root and file shape;
- strict manifest schema and profile/store binding;
- every document parses under the versioned P3 profile;
- every path, ID, resource, type, layer, source, provenance, date, sensitivity, authority, and Judgment field is valid;
- generated index.md exactly matches current document titles and paths;
- candidate secrets, link isolation, and profile identity rules remain enforced.

### Level 2

- exact raw document hashes;
- exact manifest document set and file set;
- exact bundle root;
- exact profile, policy, parent, proposal, verdict, and seed commitments;
- no unmanifested Markdown or orphan manifest entry.

### Level 3

- all typed links resolve or are classified under the existing rules;
- origin fields obey seed or verdict semantics;
- parent fields are structurally valid;
- applied target and Judgment entries match manifest claims;
- no frozen corpus count is asserted;
- no corpus-report.json is permitted inside a generation.

Level 3 on an evolving generation proves the generation itself. Level 4 proves its ancestry and ledger authority back to the frozen seed.

## 23. Verdict ledger

### 23.1 Logical append-only publication

ledger/verdicts.jsonl is logically append-only. Existing bytes must remain an exact prefix forever.

To prevent a crash from leaving a partial JSON line, the ledger writer requires its caller to hold both ordered leases:

1. verify the protected verdict lease and staging intake lease tokens;
2. read and fully verify the current ledger;
3. construct old bytes plus complete new canonical line or lines in memory;
4. write a same-directory temporary file with create-new;
5. flush and sync the temporary file;
6. recheck that the current ledger hash equals the previously read hash;
7. atomically rename the temporary file over verdicts.jsonl;
8. sync ledger/.

The physical inode may change, but no committed byte is changed or removed. Tests require the prior bytes to be an exact prefix of every successor ledger.

### 23.2 Canonical event encoding

Each event is one compact JSON object followed by one LF. JSON field order is fixed by a strict struct. Unknown fields are denied. Strings are JSON escaped by serde_json. Maps are not used in hash-critical payloads.

event_hash is:

    BLAKE3(
      "sovmem.ledger.event.v1" ||
      u64-le(payload-byte-length) ||
      canonical-payload-bytes-without-event_hash
    )

The line contains event_hash as the final field. previous_event_hash is null for genesis and the exact prior event_hash otherwise.

### 23.3 Genesis event

Event zero has event_type genesis and records:

- store_id;
- profile_version and profile_hash;
- policy_hash;
- seed manifest raw hash;
- seed corpus-report raw hash;
- seed bundle hash;
- pinned source commit;
- initial canonical reference seed;
- created_at;
- previous_event_hash null.

Exactly one genesis event is permitted, and it must be first.

### 23.4 Verdict event

Every verdict event records:

- schema_version and event_type;
- sequence and event_id;
- occurred_at;
- previous_event_hash;
- store_id;
- transaction_id;
- proposal_id and proposal meta hash;
- candidate hash;
- source_id;
- verb;
- exact reason;
- verdict_id;
- actor from policy;
- policy_hash and profile_hash;
- confidence null or exact value;
- evaluator meta commitment null or exact hash;
- base generation, manifest, bundle, and target commitments;
- rewrite hash and byte count or null;
- canonical result:
  - null generation fields for REJECT, TP, and FP;
  - generation ID, manifest hash, bundle hash, target prior/new hash, Judgment hash, and re-index receipt hash for ACCEPT and REWRITE;
- resulting canonical head;
- event_hash.

Candidate bodies, rewrite bodies, absolute paths, temporary paths, and index directory names never enter the ledger.

### 23.5 Final chain commitment

The Level 4 chain commitment is the last event_hash. For an empty, unsealed ledger there is no chain commitment and Level 4 fails ledger.genesis_missing.

## 24. Transaction commit model

The ledger is the authoritative transaction commit point. canon/CURRENT is a recoverable cache of the ledger-derived applied head.

### 24.1 Common preflight

Before the numbered mutating preflight, verdict performs a read-only, chain-verified idempotency check. If the proposal already has a terminal event, an exact verb, reason-hash, and rewrite-hash match returns the existing summary and idempotent true; different material fails verdict.already_ruled. Neither result opens the terminal, acquires a lease, or writes. The locked check in step 9 repeats this decision to close the concurrency window.

All verbs:

1. validate CLI usage without echoing private values;
2. open the real controlling terminal;
3. acquire the human-protected verdict lease;
4. acquire the staging intake lease second, following the only permitted lock order;
5. reject or recover any incomplete prior transaction before continuing;
6. verify policy, profile, seed, existing ledger, current head, and current generation through the required level;
7. load the immutable proposal pair;
8. re-run PolicyEvaluator against the current head and policy;
9. reject a stale base; if the proposal is already ruled, return the existing summary with idempotent true only when verb, exact reason hash, and rewrite hash are identical, otherwise fail verdict.already_ruled, in either case before a human prompt or write;
10. validate reason and optional rewrite bytes;
11. construct the exact decision, target, and Judgment bytes in memory;
12. display the candidate or rewrite, reason, confidence, evaluator advice, policy result, and all relevant hashes on /dev/tty;
13. require the exact typed phrase:

        APPLY <verb> <first-12-characters-of-proposal-id>

14. a mismatch or EOF aborts before any ledger or canonical write;
15. archive the exact current policy bytes under policies/<policy-hash>.toml, or verify the identical archive already exists;
16. re-read policy.toml and the immutable P3 profile and require exact equality with preflight bytes; otherwise fail policy.changed_during_transaction or profile.changed_during_transaction before any ledger commit.

### 24.2 REJECT, TP, and FP

1. Create the transaction journal and prepared marker.
2. Construct the verdict event with unchanged canonical head.
3. On the first verdict, prepend genesis in the same ledger replacement.
4. Atomically publish the ledger.
5. Write and sync ledger-committed.json.
6. Write and sync published.json.
7. create transactions/.cleanup-<transaction-id> with no-replace directory creation, hard-link each verified journal marker into it, sync it and transactions/, remove the original journal only after the complete linked cleanup form is durable, then remove the cleanup directory as a terminal liveness-only deletion with no later fallible transactions/ sync; an existing cleanup destination is incomplete-state evidence and is never replaced;
8. run internal Level 4 while allowing only the exact two lease tokens held by this transaction;
9. release the intake lease first, then sync staging/ while verdict.lock remains visible; finally verify verdict.lock, sync locks/ while it is still visible, and remove it as the final terminal deletion with no later fallible locks/ sync.

No canonical, manifest, index, pointer, or re-index receipt path is opened for writing.

### 24.3 ACCEPT and REWRITE

After common preflight:

1. Allocate the next generation ID and transaction ID.
2. Create transactions/<id>/ with create-new semantics.
3. Copy every current canonical document into a hidden generation staging directory as exact raw bytes.
4. Install the exact candidate bytes for ACCEPT or exact human rewrite bytes for REWRITE.
5. Construct the deterministic Judgment document.
6. Rebuild bundle/index.md from the complete document set.
7. Reconstruct manifest.json from raw bytes and lineage.
8. Verify the staged generation through P3 Level 3.
9. Build a complete P2 index from zero into a hidden same-generation derived root.
10. Reopen and self-check the index against the staged Level 3 generation.
11. Create a deterministic re-index receipt with document, graph, link, bundle, document-set, graph, schema, tokenizer, ranking, and index-config commitments.
12. Sync every generation file, directory, re-index receipt, and complete derived index.
13. Atomically rename the staged generation to its immutable generation ID and sync generations/.
14. Atomically rename the staged derived index and sync its parent. The index is still unselected.
15. Write and sync prepared.json, including all hashes that will enter the ledger.
16. Re-read policy.toml and the P3 profile again and require exact equality with preflight bytes.
17. Construct the verdict event. On the first verdict, prepend genesis.
18. Atomically publish the prefix-preserving ledger. This is the commit point.
19. Write and sync ledger-committed.json.
20. Atomically replace canon/CURRENT with the committed generation ID and one LF; sync canon/.
21. Write and sync published.json.
22. Create the complete no-replace hard-linked transactions/.cleanup-<transaction-id> form, sync it and transactions/, remove the original journal only after that form is durable, then remove the cleanup directory as a terminal liveness-only deletion with no later fallible transactions/ sync.
23. Run internal Level 4 while allowing only the exact two lease tokens held by this transaction.
24. Run one query proof against the selected full index when the caller requested the proof fixture.
25. Release the intake lease first and sync staging/ while verdict.lock remains visible; then verify and sync verdict.lock's parent while it is still visible, remove verdict.lock through the terminal liveness-only rule, and emit success.

If re-indexing or any step before ledger publication fails, the prior ledger head and prior CURRENT remain authoritative. The new immutable generation is uncommitted and ignored until recovery removes it.

If a step after ledger publication fails, the verdict is committed. All P3 reads fail transaction.incomplete until human recovery rolls CURRENT forward to the ledger event. A committed event is never rolled back or removed.

Approved recovery-tail amendment (2026-07-15): every create, write, rename, and parent sync that occurs while a journal, `.cleanup-*` form, lease, or lease-preparation artifact remains visible is durable and fallible. Intermediate intake or quarantined-lease deletion retains its parent sync while a protected verdict/recovery lease remains. A standalone intake or final protected lease first verifies exact ownership and syncs its parent while the lease is still visible, then deletes its directory with no subsequent fallible operation. Only that post-sync deletion of the final `.cleanup-*` directory or final protected lease directory is intentionally liveness-only. A crash can retain stale cleanup or lease evidence; public reads fail closed and human recovery resolves that liveness state. A reported failure can therefore never follow deletion of the last visible incomplete-state artifact.

### 24.4 Why the index is built before the ledger

An applied verdict is not committed unless a complete P2 rebuild already succeeded and produced a durable receipt. This prevents canon from becoming authoritative while retrieval is known to be unbuildable.

The actual index remains disposable. Deleting derived/indexes does not break Level 4 because the durable receipt proves the commit-time rebuild. Query fails closed until the exact current index is rebuilt.

## 25. Re-index receipt

evidence/reindex/<verdict-id>.json is strict deterministic JSON with:

- schema_version sovmem-reindex-receipt-v1;
- verdict_id and generation_id;
- profile_hash;
- manifest_hash and bundle_hash;
- documents_hash;
- document count;
- graph nodes, graph edges, and external typed-link count;
- graph hash;
- index schema, tokenizer, ranking, Tantivy version, Petgraph version, and index-config hash;
- selected generation semantic manifest hash.

It has no timestamp, elapsed time, path, slot, PID, hostname, username, body, reason, or query. Its raw BLAKE3 is stored in the verdict event.

Level 4 verifies the receipt bytes and binding. Query additionally verifies the current disposable index against it.

## 26. Locking, concurrency, and recovery

### 26.1 Two ordered leases

Proposal staging and intake receipt publication serialize through atomic creation of staging/.intake-lock/. A proposer may write only staging/ and this lease. It has read-only access to the profile, policy, seed, current ledger commitments, and current canonical commitments needed for proposal binding. Before creating the intake lease and again immediately after acquiring it, propose requires locks/verdict.lock/ and every quarantined recovery lease to be absent. If either check fails, it releases its own intact intake lease and returns store.locked without publishing.

Verdict application and recovery serialize through atomic creation of the human-protected locks/verdict.lock/. A verdict transaction acquires verdict.lock first and .intake-lock second. No code acquires them in the reverse order. Holding both freezes proposal publication while the human transaction revalidates and commits.

Each owner.json records:

- schema version;
- transaction or receipt ID;
- command class;
- started_at;
- process ID for local inspection only;
- a content-bound lease token that the owning process rechecks before every rename or ledger commit boundary.

No decision depends on PID liveness or lock age.

Approved implementation refinement (2026-07-14): Rust 1.84's standard filesystem API cannot perform a no-replace directory rename on macOS, and replace-capable rename would violate the no-steal rule for an existing empty lock directory. Lease publication therefore uses the standard-library-only no-replace protocol: create and sync a hidden same-parent directory containing the canonical owner file; atomically create the fixed lease directory with `create_dir`; hard-link the synced hidden owner file into the previously absent `owner.json` path; sync the fixed directory and parent; then remove and sync the hidden directory. `create_dir` and `hard_link` both fail if their destination exists, so no existing lock or owner file is replaced. A crash before the link leaves the empty fixed directory and hidden preparation directory as explicit incomplete evidence; a crash after the link leaves a verifiable owner. Public reads fail closed on either state.

A second proposal process receives proposal.locked and exit 1. A second verdict or recovery process receives store.locked and exit 1. Neither waits, steals, or deletes a live lease.

### 26.2 Stale lease behavior

A crash may leave either lease. No command automatically breaks one.

The human-only command:

    sovmem recover --profile <p3-profile> --store <store>

requires the same /dev/tty ceremony and performs no new verdict. It:

1. instructs the operator to terminate and verify the absence of the prior sovmem process;
2. displays the stale owner identifiers and requires the exact phrase RECOVER <owner-id>, where owner-id is the recorded transaction, proposal-intake, or rejection-receipt ID;
3. if a stale verdict lease exists, atomically renames it to a quarantined name, then atomically creates verdict.lock as the protected recovery lease; if another process wins that creation, recovery fails store.locked without touching ledger or canon, and leaves the quarantined stale lease as explicit incomplete-state evidence for a retry;
4. if no verdict lease exists, atomically creates verdict.lock directly as the protected recovery lease;
5. atomically quarantines a stale intake lease when present;
6. verifies the ledger to its last complete committed event;
7. selects exactly one recovery mode from the quarantined owner records: verdict-transaction recovery, intake-only recovery, or clean-finalization recovery; clean-finalization is permitted only when a verdict/recovery owner remains after its journals were removed and the ledger, CURRENT, generations, receipts, and all other protected paths already verify exactly;
8. creates and syncs exactly one recovery journal bound to the quarantined owner hashes, selected mode, observed ledger head, and original transaction journal hash or null; a later recovery resumes only an exact matching journal;
9. for a verdict transaction, inspects exactly one matching original journal or its recognized .cleanup form; if the ledger contains the transaction event, verifies the committed generation and re-index receipt and rolls CURRENT forward; otherwise it removes only that transaction's uncommitted generation, derived index, temporary ledger file, and temporary pointer;
10. for intake-only recovery, requires that no original verdict journal exists, preserves every completely published proposal pair or rejection receipt, and removes only unpublished candidate, metadata, receipt, or directory temporaries whose owner binding matches the quarantined intake lease;
11. for clean-finalization, changes no content or journal state other than the new recovery journal and the stale lease/quarantine cleanup;
12. never edits verdicts.jsonl, committed generation bytes, seed bytes, policy, published proposals, published rejection receipts, or frozen P2 files;
13. writes and syncs published.json in the recovery journal, removes the original journal or cleanup directory when applicable, atomically creates the recovery journal's no-replace `.cleanup` form and syncs it plus transactions/, and removes each quarantined lease with its parent sync while the protected recovery lease remains visible. It then runs the appropriate internal verification: Level 4 for a sealed store or the explicit seed-Level-3 plus policy/profile/staging verification for an unsealed store, allowing only the exact protected recovery lease token bound to the recovery journal. After that verification succeeds, it verifies and syncs the protected recovery lease parent while the lease remains visible, then removes that protected lease last as a terminal deletion with no later fallible parent sync;
14. returns the redacted recovery result. A subsequent clean read or idempotent recovery invocation runs public Level 4 for a sealed store, or the explicit unsealed verifier for a pre-genesis store.

Every mutating production path rechecks its lease token immediately before each publication boundary. A process whose lease was quarantined aborts. P3 still relies on the human instruction to terminate the prior process; it does not claim that PID inspection proves process death.

Ambiguous owner state, unexpected temporaries, more than one original journal, more than one recovery journal, or a journal whose owner/hash binding fails causes recovery to stop. The operator must inspect the state; the binary does not guess. A crash during recovery leaves its bound journal and leases, so the same ceremony can resume the exact recovery idempotently.

### 26.3 Concurrent proposals

The intake lease makes duplicate and quota checks plus proposal publication one serializable operation. Two exact submissions cannot both pass. A crashed proposal leaves either no visible proposal or one complete immutable pair.

### 26.4 Concurrent verdicts

Only one verdict transaction can hold the protected verdict lease. The loser fails store.locked before reading private decision content. After retry, it observes the new ledger head and either becomes stale or proceeds against the updated state.

### 26.5 Idempotency

- Repeating a committed verdict with the same proposal ID, verb, exact reason hash, and rewrite hash returns the existing verdict summary with idempotent true and exit 0.
- Repeating it with different decision material fails verdict.already_ruled and exit 1.
- A duplicate proposal ID fails proposal.duplicate_id.
- A duplicate target_id plus candidate_hash fails proposal.duplicate_content.
- recover is idempotent when the pointer already matches the committed ledger head.

## 27. Crash boundaries

Tests inject failure immediately:

1. after lease creation;
2. after proposal candidate sync;
3. after proposal metadata sync;
4. before and after proposal-directory rename;
5. after transaction journal creation;
6. after target bytes are staged;
7. after Judgment bytes are staged;
8. after manifest sync;
9. after Level 3 self-check;
10. during index build;
11. after index self-check;
12. after re-index receipt sync;
13. after generation rename;
14. after derived-index rename;
15. after prepared marker;
16. after ledger temporary file sync;
17. before ledger rename;
18. after ledger rename;
19. before CURRENT rename;
20. after CURRENT rename;
21. before published marker;
22. during cleanup.

For boundaries 1 through 17, the old ledger head remains authoritative. For boundaries 18 through 22, the new ledger head is authoritative and recovery rolls forward. No boundary permits a query or subsequent verdict to consume a pointer that disagrees with the ledger.

## 28. verify --level 4

### 28.1 CLI shape

Levels 1 through 3 retain the existing bundle mode:

    sovmem verify --profile <profile> --bundle <bundle> --level 3

Level 4 requires store mode:

    sovmem verify --profile <p3-profile> --store <store> --level 4

Rules:

- --bundle and --store are mutually exclusive;
- bare verify and levels 1 through 3 require --bundle and retain existing behavior;
- level 4 requires --store;
- --require-corpus is invalid with store mode;
- unsupported combinations are cli.usage with exit 2.

### 28.2 Level 4 algorithm

Level 4:

1. validates store path, exact root shape, and symlink restrictions;
2. parses and hashes the immutable P3 profile, current policy, and every historical policy snapshot referenced by the ledger;
3. verifies seed at the existing explicit P2 Level 3 contract;
4. requires one valid genesis event;
5. parses every ledger line as canonical UTF-8 JSON with one final LF;
6. requires contiguous sequence and exact event IDs;
7. recomputes every event hash and previous-event link;
8. rejects duplicate event, proposal, verdict, and transaction IDs;
9. requires exactly one terminal verdict per proposal;
10. reloads every referenced immutable proposal pair and verifies its hashes;
11. checks confidence equality and evaluator commitment equality;
12. reconstructs the historical head at each event, reruns the event-specific archived PolicyEvaluator against that base, and proves again that erasing confidence and evaluator data cannot change the result;
13. checks empty reason and five-verb semantics, including full archived-policy validation of human REWRITE bytes recovered from the applied target and equality of their raw hash and byte count to the event;
14. requires non-applying verbs to preserve the canonical head;
15. walks each applied generation in order;
16. verifies every generation through P3 Level 3;
17. verifies parent, seed, target, Judgment, manifest, bundle, and event-specific archived policy commitments;
18. verifies each re-index receipt;
19. derives the authoritative canonical head from applied events;
20. requires canon/CURRENT to equal that head;
21. rejects unexplained transaction journals, temporary ledger files, temporary pointers, uncommitted generation directories, or any lease. The transaction coordinator alone may call the same verifier with the exact currently held verdict and intake lease tokens after its journal is cleaned; the recovery coordinator may do so after its recovery cleanup with only the exact protected recovery-lease token bound to its recovery journal;
22. reports event count, verdict count by verb, applied generation count, current bundle hash, and final chain commitment.

It does not require a disposable index directory to exist.

### 28.3 Level 4 success output

JSON is one compact object and one LF:

    {"status":"ok","command":"verify","level":4,"events":21,"verdicts":20,"accept":4,"reject":4,"rewrite":4,"tp":4,"fp":4,"applied_generations":8,"current_generation":"gen-0000000000000008","bundle_hash":"...","ledger_head":"...","errors":0}

Text mode is not added to verify in P3; existing verify remains JSON-only.

### 28.4 Stable Level 4 corruption codes

- ledger.read
- ledger.utf8
- ledger.line_encoding
- ledger.parse
- ledger.schema
- ledger.genesis_missing
- ledger.genesis_duplicate
- ledger.sequence
- ledger.event_id
- ledger.previous_hash
- ledger.event_hash
- ledger.duplicate_id
- ledger.proposal_binding
- ledger.verdict_binding
- ledger.reason_empty
- ledger.confidence_binding
- ledger.evaluator_binding
- ledger.verb_semantics
- ledger.head_transition
- ledger.generation_missing
- ledger.generation_chain
- ledger.manifest_mismatch
- ledger.bundle_mismatch
- ledger.judgment_mismatch
- ledger.reindex_receipt_mismatch
- ledger.current_mismatch
- transaction.incomplete
- proposal.locked
- store.locked

Every failure is redacted and produces no success stdout.

## 29. P2 compatibility and frozen-baseline isolation

P2 and P3 use two explicit lanes:

### 29.1 Frozen evaluation lane

- profile: profiles/audy-ssa0-v0/profile.yaml
- corpus contract: profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml
- evaluation ID: audy-sovmem-v0
- document count: exactly 62
- golden, thresholds, labels, and baseline: unchanged
- export manifest and corpus report: unchanged
- Q3 reproduction always rebuilds from this lane.

### 29.2 Evolving write-back lane

- profile: profiles/audy-ssa0-p3-v1/profile.yaml
- store ID: audy-sovmem-writeback-v1
- seed: exact frozen Level 3 export copy
- document count: versioned and allowed to evolve
- manifest schema: sovmem-canon-manifest-v1
- evaluation baseline: none in P3
- full index rebuild: same P2 schema, tokenizer, ranking, and full-rebuild implementation.

The index builder accepts either:

1. a frozen verified corpus with complete corpus evidence; or
2. a verified P3 canonical generation with the P3 manifest.

It never treats the second as the first. The resulting index manifest records input_kind frozen-corpus or p3-canon, store_id when present, and the exact manifest hash. Frozen P2 index manifests and baseline bytes remain compatible.

P3 does not run the frozen golden suite against the evolved generation because relevance labels and the 62-document contract no longer describe that generation. P2 regression remains reproducible by running it against the frozen lane. P3 proof uses exact query assertions specific to the safe clone and does not edit the golden set.

## 30. CLI contracts

All new P3 commands support --format json or --format text except verify, which remains JSON-only. JSON success is one compact object plus LF. Text is stable line-oriented UTF-8. Success uses stdout only. Diagnostics are sorted, deduplicated JSON Lines on stderr.

### 30.1 propose

    sovmem propose \
      --profile <p3-profile> \
      --store <store> \
      --source src:agent:example \
      --operation create \
      --intent accept \
      --target-id ssa-0/04-Research/Example.md \
      --target-path 04-Research/Example.md \
      --candidate <candidate-file> \
      [--confidence 0.75] \
      [--evaluator <evaluator-json>] \
      [--format json|text]

The command derives base hashes from the current ledger-authoritative head. Callers cannot supply or override base hashes, candidate hash, timestamp, proposal ID, policy hash, or profile hash.

JSON success:

    {"status":"ok","command":"propose","proposal_id":"prp-...","state":"staged","operation":"create","intent":"accept","target_id":"ssa-0/04-Research/Example.md","candidate_hash":"...","base_kind":"seed","base_generation":null,"bundle_hash":"...","manifest_hash":"...","policy_hash":"...","submitted_at":"2026-07-14T04:30:00.000000000Z"}

Candidate file and evaluator file paths are never emitted.

### 30.2 proposals list

    sovmem proposals list \
      --profile <p3-profile> \
      --store <store> \
      [--state staged|ruled|applied|superseded|retained|rejected-at-intake] \
      [--format json|text]

Ordering is:

- staged proposals by submitted_at then proposal_id;
- ruled proposals by ledger sequence;
- rejection receipts by received_at then receipt_id.

Default JSON fields:

- proposal or receipt ID;
- state;
- submitted or received timestamp;
- source_id when valid;
- operation and intent for staged proposals;
- target_id;
- candidate hash;
- confidence;
- evaluator presence, source, recommendation, and reason count;
- terminal verb and verdict ID when ruled;
- current or superseded status.

Candidate bodies, reasons, evaluator reasons, target paths, and absolute paths are omitted.

### 30.3 proposals status

    sovmem proposals status <proposal-id> \
      --profile <p3-profile> \
      --store <store> \
      [--show-advice] \
      [--format json|text]

The default output adds base commitments, policy/profile hashes, metadata hash, and lifecycle state. --show-advice exposes evaluator reasons but never candidate bytes. Candidate bytes are inspected only by the interactive human verdict ceremony or by directly reading the retained proposal file under authorized local access.

### 30.4 verdict

    sovmem verdict <proposal-id> <ACCEPT|REJECT|REWRITE|TP|FP> \
      --profile <p3-profile> \
      --store <store> \
      (--reason <text> | --reason-file <file>) \
      [--rewrite-file <file>] \
      [--format json|text]

A new verdict always requires the real interactive human ceremony. There is no --yes, --force, --non-interactive, environment bypass, or stdin approval. An exact replay of an already committed verdict is a read-only lookup and may return idempotent true without the ceremony; it cannot create or change authority.

JSON success:

    {"status":"ok","command":"verdict","proposal_id":"prp-...","verdict_id":"ver-...","event_id":"evt-0000000000000001","verb":"ACCEPT","applied":true,"generation":"gen-0000000000000001","bundle_hash":"...","manifest_hash":"...","target_hash":"...","judgment_hash":"...","ledger_head":"...","reindex_receipt_hash":"...","idempotent":false}

Ledger-only verbs use null for generation, manifest, target, Judgment, and re-index fields and applied false. Reason text and rewrite paths are not in success output.

### 30.5 verdicts list

    sovmem verdicts list \
      --profile <p3-profile> \
      --store <store> \
      [--verb ACCEPT|REJECT|REWRITE|TP|FP] \
      [--source <source-id>] \
      [--show-reasons] \
      [--show-advice] \
      [--format json|text]

Ordering is ledger sequence.

Default fields:

- event and verdict IDs;
- timestamp;
- proposal ID;
- source ID;
- verb;
- actor;
- confidence;
- evaluator source, recommendation, and reason count;
- applied flag and generation;
- reason hash and byte count;
- candidate, rewrite, target, manifest, bundle, receipt, and event hashes as applicable.

--show-reasons exposes exact ledger reasons. --show-advice joins and verifies the retained evaluator block and exposes its reasons. Neither flag emits candidate or rewrite bodies.

### 30.6 recover

    sovmem recover \
      --profile <p3-profile> \
      --store <store> \
      [--format json|text]

recover is interactive, human-only, deterministic, and cannot create a verdict or alter committed ledger/canonical bytes.

### 30.7 stats in store mode

Existing bundle mode remains unchanged:

    sovmem stats --profile <profile> --bundle <bundle>

New store mode:

    sovmem stats --profile <p3-profile> --store <store>

After genesis, store mode verifies Level 4 first and returns existing bundle statistics plus:

    "queue": {
      "open": 25,
      "alarm_at": 25,
      "alarm": true,
      "oldest_submitted_at": "2026-07-14T04:30:00.000000000Z"
    },
    "ledger": {
      "events": 21,
      "verdicts": 20,
      "applied_generations": 8,
      "head": "..."
    },
    "store_id": "audy-sovmem-writeback-v1",
    "profile_version": "audy-ssa0-p3-v1",
    "current_generation": "gen-0000000000000008"

alarm is true when open is greater than or equal to queue_alarm_at. It is count-based and deterministic; no current-time age calculation enters output.

Before genesis, store mode verifies the seed at explicit Level 3, validates the current policy/profile and every staged pair, reports sealed false, events 0, ledger head null, and current_generation seed, then emits the same queue block. It does not claim Level 4 success. This makes the queue alarm usable before the first human verdict.

## 31. Exit codes and diagnostics

| Exit | Meaning |
| ---: | --- |
| 0 | Complete success or exact idempotent replay |
| 1 | Readable data, policy, chain, stale-base, quota, lock, authority state, or deterministic verification gate failed |
| 2 | Usage, configuration, unsupported platform/filesystem, unreadable input, unsafe path, missing TTY, or write/publication infrastructure failed before a valid data decision |

Stable command-level codes include:

- cli.usage
- human.tty_required
- human.confirmation_failed
- human.reason_invalid
- human.rewrite_required
- human.rewrite_forbidden
- proposal.read
- proposal.write
- proposal.not_found
- verdict.already_ruled
- verdict.verb_invalid
- verdict.intent_conflict
- verdict.write_failed
- store.read
- store.write
- store.layout_invalid
- store.path_unsafe
- store.symlink
- store.cross_filesystem
- profile.changed_during_transaction
- proposal.locked
- store.locked
- transaction.prepare_failed
- transaction.generation_failed
- transaction.reindex_failed
- transaction.ledger_failed
- transaction.publish_failed
- transaction.incomplete
- transaction.recovery_ambiguous

Policy and Level 4 codes are listed in their owning sections.

Diagnostics:

- contain severity, code, and one static message;
- never contain candidate, rewrite, reason, evaluator reason, title, body, query, absolute path, requested path, source excerpt, temporary name, PID, username, hostname, or OS error string;
- never echo malformed user input;
- use success stdout only after the transaction and required verification are complete;
- never emit partial success.

## 32. Privacy behavior

### 32.1 Persisted sensitive content

- Candidate and rewrite bytes are retained only in proposal/generation storage, never in the ledger or receipts.
- Rewrite bytes are retained through the immutable applied generation; no second rewrite copy is required.
- Reasons are deliberately durable in the human verdict ledger and applied Judgment documents.
- Evaluator reasons remain in immutable proposal metadata. The ledger stores their commitment, source, recommendation, and reason count.

### 32.2 Output defaults

- No command prints candidate or rewrite bodies on stdout or stderr.
- The interactive verdict ceremony uses /dev/tty, which is outside captured machine stdout/stderr.
- Reasons and evaluator reasons require explicit show flags in list commands.
- Canonical target IDs may be printed; filesystem paths are not.
- Absolute paths are never serialized.

### 32.3 Sensitive-intake failure

Malformed or policy-rejected candidate content is not persisted. Rejection receipts deliberately omit content hashes because low-entropy sensitive content could be guessed from a hash.

## 33. Determinism strategy

Production and tests use injected interfaces:

- Clock for UTC timestamps;
- HumanPresence for /dev/tty display and confirmation;
- FaultInjector for named crash boundaries;
- FilesystemOps for controlled write, fsync, rename, and cleanup failures.

Production implementations are private and use only std plus existing dependencies. Test implementations are deterministic and cannot be selected through a production CLI flag or environment variable.

IDs are deterministic content commitments. Event IDs are ledger sequence numbers. Generation IDs are applied-event sequence numbers. Directory enumeration is always sorted by raw UTF-8 bytes before hashing or serialization.

Each command samples its injected clock exactly once for each durable attempt. propose reuses one sample for submitted_at, proposal ID, and any receipt derived from that attempt. verdict samples once after the locked revalidation and before rendering the human decision, then reuses that value for ruled_at, occurred_at, verdict ID, transaction ID, Judgment dates, and first-verdict genesis created_at. A prompt crossing a wall-clock boundary does not change prepared bytes. recover samples once for its recovery transaction ID and markers. Production clock rollback does not reorder authority because ledger sequence is authoritative; timestamps are evidence only.

Canonical JSON:

- fixed struct field order;
- compact form plus one LF for proposal and ledger lines;
- pretty two-space form plus one LF for manifests and receipts;
- strict round-trip byte equality;
- deny unknown fields.

No timestamp enters a re-index receipt, canonical index manifest, graph, or retrieval result.

## 34. Fixture-first TDD coverage

Implementation must start with failing fixtures in this order.

### 34.1 Policy fixtures

- exact reviewed policy parses;
- unknown, missing, duplicate, reordered, malformed, non-LF, unsupported-schema, and invalid invariant cases fail;
- raw-byte policy changes change the hash;
- confidence/evaluator erasure leaves decisions identical;
- policy never self-modifies.

### 34.2 Proposal fixtures

- atomic pair publication;
- candidate/meta hash mismatch;
- duplicate ID;
- duplicate target/content;
- unsafe and symlink paths;
- create/replace mismatch;
- stale seed, generation, manifest, bundle, and target bases;
- max byte boundaries;
- quota boundaries and concurrent submissions;
- explicit provenance on new resources;
- protected synthesis requires rewrite intent;
- Judgment proposal poisoning;
- high confidence plus policy violation;
- favorable evaluator plus policy violation;
- malformed, unallowlisted, self-evaluated, and wrong-hash evaluator blocks;
- rejection receipt redaction.

### 34.3 Verdict fixtures

- all five verbs require non-empty reasons;
- ACCEPT applies candidate bytes exactly;
- REWRITE applies rewrite bytes exactly and never candidate bytes;
- REJECT, TP, and FP leave every canonical and index hash unchanged;
- ACCEPT on rewrite intent fails;
- synthesis ACCEPT fails and synthesis REWRITE passes;
- actor is policy-derived;
- non-TTY, piped input, wrong phrase, EOF, and unavailable /dev/tty fail before writes;
- forged Judgment path is unreachable outside transaction coordinator;
- exact idempotent replay succeeds; altered replay fails.

### 34.4 Canon and manifest fixtures

- seed bytes remain unchanged;
- raw bytes, manifest, index.md, bundle root, origin, parent, and applied hashes reconstruct exactly;
- create and replace document counts are correct;
- applied Judgment bytes are deterministic and queryable;
- missing, extra, reordered, duplicate, malformed, or rebound entries fail;
- current P2 export verifier and frozen fixtures remain byte-compatible.

### 34.5 Ledger fixtures

- exact genesis;
- every previous hash and event hash;
- line truncation, extra newline, compactness, unknown fields, reorder, deletion, duplication, and substitution fail;
- confidence and evaluator commitment tampering fail;
- verb/result semantic mismatch fails;
- prior ledger bytes are an exact prefix after every append;
- first verdict publishes genesis and verdict atomically;
- chain verifies after 20 verdicts with exactly four ACCEPT, four REJECT, four REWRITE, four TP, and four FP;
- that fixture produces eight applied generations and 21 total events including genesis.

### 34.6 Transaction and crash fixtures

- every boundary in Section 27;
- re-index failure leaves old ledger and head;
- ledger write failure leaves old ledger and head;
- post-ledger pointer failure produces an incomplete, fail-closed store;
- recovery rolls forward only committed events;
- recovery discards only uncommitted artifacts;
- a crash after journal cleanup but before lease release resumes only as verified clean-finalization;
- recovery itself resumes only from an exact owner/hash-bound recovery journal;
- ambiguous recovery fails;
- concurrent verdicts serialize;
- stale leases are never auto-stolen;
- fsync and same-directory rename failures before a terminal cleanup or lease deletion are surfaced; final cleanup/lease deletions are liveness-only and retain stale evidence on crash;
- old canon is always readable or the store fails closed; no ambiguous success exists.

### 34.7 CLI and privacy fixtures

- exact JSON and text outputs;
- sorted JSONL diagnostics;
- correct exit codes;
- no success stdout on failure;
- no body, reason, advice, query, private path, temporary name, or secret sentinel leaks;
- show flags expose only their named sensitive fields;
- existing P0-P2 CLI output remains compatible.

### 34.8 Full loop fixture

On a synthetic evolving store:

1. query seed and record no hit for a unique phrase;
2. submit a proposal containing the phrase;
3. apply a human ACCEPT through injected test HumanPresence;
4. verify exact target and Judgment bytes;
5. verify Level 3 generation and Level 4 chain;
6. verify a complete re-index receipt;
7. query the new index and retrieve both the target and Judgment reason;
8. prove frozen evaluation files are byte-unchanged.

No fixture reaches the network or live Audy Vault.

## 35. Safe real-data proof

The final P3 gate requires separate human authorization for the safe-clone location. It never uses /Volumes/himura/audy-vault or the existing frozen output as a write target.

Procedure:

1. Record BLAKE3 for the tracked golden, thresholds, baseline, P2 profile, corpus manifest, and the complete verified 62-document export.
2. Create a new sandbox root outside the live vault and outside the frozen export.
3. Copy the exact verified Level 3 export into sandbox seed/.
4. Verify the copied seed at Level 3 and compare every recorded hash.
5. Install the reviewed P3 profile and policy.
6. Submit one bounded create proposal using a unique retrieval phrase and an allowed non-sensitive research path.
7. Inspect exact candidate, reason, confidence, and optional evaluator advice on the real terminal.
8. Apply ACCEPT or REWRITE manually.
9. Confirm the applied target bytes, deterministic Judgment bytes, manifest, bundle root, complete index rebuild receipt, and Level 4 chain.
10. Re-query the evolved lane and retrieve the unique phrase plus the Judgment reason.
11. Delete the disposable evolved index, rebuild it from the active P3 generation, and repeat the query.
12. Verify the live Audy Vault working tree and Git commit are unchanged.
13. Recompute the frozen P2 artifact hashes and assert byte equality.
14. Re-run the frozen P2 Level 3, index, query, and eval commands against the frozen lane and reproduce the canonical baseline.
15. Retain only reviewed redacted evidence. Do not publish absolute sandbox or vault paths.

If no approved safe sandbox is available, the real-data gate remains blocked. Synthetic fixtures do not substitute for it.

## 36. Git behavior

The P3 binary:

- does not call Git;
- does not create commits;
- does not infer human authority from a Git author;
- does not make ledger success contingent on Git;
- does not synchronize Audy Vault.

A human may manually commit a successfully Level-4-verified store snapshot after the verdict transaction. That commit is outer source-history evidence only. Rewriting Git history cannot rewrite a valid ledger chain unnoticed, and a Git commit cannot substitute for a missing ledger event.

## 37. Audit-state impact

### 37.1 Design-only commit

This design does not change docs/sovmem-build-state.json or regenerate docs/SOVMEM-BUILD-AUDIT.md. P3 remains 0 percent and overall completion remains 37 percent.

### 37.2 Implementation gate

P3 may move to 100 percent only when:

- every required fixture passes;
- the complete 20-verdict mixed chain verifies;
- the full propose to human verdict to verified write to full rebuild to re-query proof passes;
- the approved safe real-data sandbox proof passes;
- frozen P2 artifacts and Q3 reproduction remain unchanged;
- formatting, Clippy, npm run audit:sovmem:check, and the complete test suite pass at the reviewed implementation head;
- the implementation and generated audit are reviewed together before manual merge.

The audit calculation is:

    (P0 1 + P1 2 + P2 2 + P3 3.5) / 13.5 = 62.96 percent

The generated audit should therefore report approximately 63 percent overall, using the audit generator's reviewed rounding rule.

Test counts, implementation commit, Level 4 evidence, chain head, safe-clone evidence, and exact P3 artifacts are recorded only after the gate passes.

## 38. Exit gates

P3 is complete only when all statements are true:

1. No runtime path writes canonical generations except the reviewed verdict transaction.
2. Seed provisioning is complete before genesis and cannot target a sealed store.
3. Agents can stage proposals but cannot invoke a production canonical writer through a library or MCP surface.
4. The production seed, canon, ledger, policy history, evidence, derived index, transaction, verdict-lock, and current policy paths are outside agent-writable roots; only staging and its intake lease are proposer-writable.
5. Every verb has a non-empty human reason.
6. ACCEPT applies candidate bytes exactly.
7. REWRITE applies human rewrite bytes exactly.
8. ACCEPT and REWRITE create deterministic Judgment bytes, rebuild integrity evidence, complete a full index rebuild, and become queryable.
9. REJECT, TP, and FP are ledger-only.
10. Confidence and evaluator advice demonstrably cannot change policy or authority.
11. Every named poisoning, stale-base, quota, duplicate, privilege, path, size, manifest, ledger, crash, and concurrency fixture fails with stable redacted diagnostics.
12. Level 4 verifies the complete chain and every canonical generation.
13. The chain verifies after at least 20 mixed verdicts.
14. A safe real-data propose to verdict to re-query cycle succeeds without touching live Audy Vault or frozen P2 evidence.
15. Deleting and rebuilding the current P3 index reproduces the query proof.
16. Frozen P2 Q3 evidence remains reproducible.
17. The audit is updated only after all earlier gates pass.

## 39. Tripwires and stop conditions

Stop for human decision if any of these occurs:

- a requirement would make Audy Vault or the frozen P2 export a P3 write target;
- a consumer cannot distinguish the frozen P2 lane from the evolving P3 lane;
- policy or manifest versioning would weaken existing Level 1-3 verification;
- a dependency appears necessary;
- an ACCEPT or REWRITE can commit before full re-index success;
- a ledger write and canonical head can disagree without a deterministic fail-closed recovery state;
- committed ledger bytes would need to be truncated, edited, or removed;
- a recovery rule must guess;
- a candidate, rewrite, reason, evaluator block, or private path leaks into diagnostics;
- a proposer can create a Judgment document;
- confidence, evaluator advice, trust, source identity, or quota status changes commit authority;
- a non-interactive approval path appears;
- the production store cannot be separated from agent write capability;
- filesystem semantics cannot provide same-directory atomic rename and directory fsync;
- a safe clone cannot be approved;
- any frozen golden, threshold, relevance, corpus, baseline, or profile file would need to change;
- implementation drifts into MCP, trust, sentry, TTF, outcome calibration, automatic Git, or live-vault synchronization.

## 40. Design decisions summary

The reviewed implementation plan must treat these as fixed if this design is approved:

- separate audy-sovmem-writeback-v1 lane;
- immutable frozen P2 lane;
- immutable proposal pair;
- strict in-crate canonical TOML subset and no new dependency;
- versioned P3 profile and manifest;
- exact content-bound IDs and injected deterministic clocks;
- agent Judgment proposals forbidden;
- applied verdicts create target plus human Judgment document;
- non-applying verbs ledger-only;
- copy-on-write generations;
- full P2 rebuild before ledger commit;
- prefix-preserving atomic ledger replacement;
- ledger head as commit authority and CURRENT as recoverable cache;
- a proposer-writable intake lease plus a human-protected verdict lease, acquired in fixed order and never stolen automatically;
- interactive /dev/tty human ceremony plus OS/sandbox capability separation;
- Level 4 verifies seed, ledger, proposal bindings, generations, receipts, head, and incomplete transactions;
- no automatic Git or Audy Vault synchronization;
- P3 audit publication only after the entire gate passes.
