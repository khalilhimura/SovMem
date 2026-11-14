> **ARCHIVED** — This document has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix B: Archive Map](../sovmem-master-spec.md#appendix-b-archive-map) for the master spec section that covers its content.
>
# SovMem P2 Derived Index and Query Design

**Date:** 2026-07-13

**Status:** Draft for repository-owner review

**Owner:** Nous / SovMem

**Authority:** Implementation-supporting design. `docs/sovmem_v0.3_execution_plan.md` remains normative. If this design conflicts with the execution plan or any frozen P0/P1/Q3 contract, implementation stops for human resolution.

## Purpose

Complete the P2 derived-index and query design before implementation begins.

P2 adds an offline, disposable retrieval layer over a formally verified Level 3 bundle. It does not create a second source of truth. The Markdown bundle remains authoritative; the Tantivy index, persisted graph projection, Petgraph adjacency, evaluation output, and all index generations are derived and may be deleted and rebuilt from the same verified bundle.

This design resolves only the implementation mechanics that P0, P1, and the frozen Q3 decision intentionally left open:

- compatible pinned Tantivy and Petgraph releases;
- the derived-index directory and publication protocol;
- exact schema indexing options and tokenization;
- canonical ingestion order;
- deterministic BM25 ranking and tie-breaking;
- exact one-hop expansion behavior;
- machine-readable and human-readable command contracts;
- the deterministic `baseline.json` contract;
- stable failures and fail-closed behavior;
- two-build and deleted-index rebuild proofs;
- the frozen Q3 GO/NO-GO evaluation.

No code, dependency, fixture, corpus, query, relevance label, threshold, canonical bundle, or audit-state change belongs in this design-only commit.

## Verified starting point

The design is grounded in `origin/main` at merge commit `3599bfbb4ed0c5d8522f8878fb022c3bd1c7d585`.

The isolated design worktree is on branch `codex/sovmem-p2-derived-index-query`. The intentionally stale root checkout was inspected but not used for implementation or document editing.

At the verified starting commit:

- the repository pins Rust `1.84.0`, edition 2021, and `rust-version = "1.84"`;
- the single Rust crate is `sovmem/Cargo.toml`;
- the public CLI implements `export`, `corpus`, `verify`, `stats`, and `verify-golden`;
- successful commands emit one compact JSON object to stdout;
- stable privacy-safe diagnostics are sorted JSON Lines on stderr;
- exit `0` means success, exit `1` means verified/readable data failed a gate, and exit `2` means usage, configuration, or required local input could not be loaded safely;
- a complete baseline run passes 49 JavaScript tests and 203 Rust tests;
- the formal Level 3 bundle verifies 62 documents, 226 warnings, one exclusion, zero errors, and bundle BLAKE3 `49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65`;
- the bundle is bound to Audy Vault commit `43824cfd9caed7b260aa442128fd1130ddb6e2ff`;
- the accepted corpus is exactly 8 core documents and 54 research documents;
- the frozen golden set has 25 human-approved queries and exactly one relevant document per query;
- the frozen golden-file BLAKE3 is `f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175`;
- the current bundle contains 62 potential graph nodes and 100 typed frontmatter links: 19 same-profile links between accepted documents and 81 external links;
- every current internal typed link has relation `references`; every current external typed link has relation `cites`;
- `docs/SOVMEM-BUILD-AUDIT.md` reports P0 100%, P1 100%, P2 10%, and 24% overall completion.

The 19/81 link split is observed evidence for the frozen current bundle, not a new corpus rule. The graph builder must derive the counts again from verified frontmatter and fail if the derived state does not correspond to its persisted projection.

## Frozen requirements carried forward unchanged

The following requirements are inputs to this design, not questions reopened by it:

1. The bundle is authoritative. The index is disposable and regenerable.
2. Indexing is full-rebuild only. There is no incremental update path.
3. The Tantivy schema has exactly these logical fields: `id`, `title`, `body`, `layer`, `type`, `tags`, and `provenance`.
4. Graph edges originate only in typed frontmatter `links[]`.
5. The binary performs no model calls and no network calls.
6. The repository keeps one Rust crate.
7. Audy Vault remains the pinned canonical source.
8. The accepted corpus remains exactly 62 documents: 8 core and 54 research.
9. All 25 golden queries remain human-approved and byte-immutable throughout P2.
10. `evals/audy-sovmem-v0/thresholds.json` remains byte-immutable throughout P2.
11. Q3 uses `k = 5`, macro Precision@5 at least `0.16`, macro Recall@5 at least `0.80`, and at least 20 of 25 queries with a hit in the first five.
12. The Q3 gate uses integer hit counts, not rounded floating-point comparisons.
13. The provenance enum remains `tier1 | secondary | synthesis | unknown`.
14. The future internal ledger remains five-verb; the public pedagogical subset remains three-verb. P2 does not implement or alter either contract.
15. The private repository uses manual pull-request merge discipline. The implementation and documentation must not claim branch protection, rulesets, required checks, or merge queues are enforced.

## Scope

### In scope

1. Pin Tantivy and Petgraph versions that compile under Rust 1.84.
2. Build a complete new Tantivy index only after explicit Level 3 bundle verification passes.
3. Persist a deterministic graph projection derived only from typed frontmatter links and reconstruct Petgraph adjacency from that projection.
4. Bind every published index generation to the exact verified bundle, source commit, profile identity, schema, tokenizer, ranking contract, and dependency versions.
5. Implement stable BM25 top-k retrieval over `title`, `tags`, and `body`.
6. Implement optional deterministic one-hop graph expansion without changing BM25 order or score.
7. Add `index`, `query`, and `eval` CLI commands with JSON and text success formats.
8. Generate a deterministic tracked `evals/audy-sovmem-v0/baseline.json` from the frozen evaluation set.
9. Prove two-build semantic determinism and deleted-index rebuild equivalence.
10. Execute the frozen Q3 GO/NO-GO gate without tuning the frozen inputs.
11. Add fixture-first tests for success, corruption, stale state, path safety, publication rollback, determinism, privacy, and gate arithmetic.
12. On a full P2 GO only, update the build-state source, regenerate the audit, and verify audit freshness before a manually reviewed pull request may merge.

### Explicit non-scope

- P3 proposal staging, `PolicyEvaluator`, proposal confidence, evaluator advice, verdict application, bundle writes, re-index-after-verdict, or ledger work;
- P4 MCP tools or transport;
- P5 sentry scheduling, regression policy beyond the frozen first baseline, trust reporting, audit sampling, dogfood, fuzzing, or long-run hardening;
- embeddings, vector search, Qdrant, hybrid retrieval, reranking models, model calls, or network calls;
- incremental indexing, document updates in place, or mutable index generations;
- mutation of Audy Vault, the canonical bundle, corpus manifests, golden queries, rationales, relevance labels, thresholds, or frozen evaluation fixtures;
- expanding external URLs into graph nodes;
- body-link or wikilink graph inference;
- snippets, body excerpts, query logging, analytics, telemetry, a GUI, a TUI, or a service daemon;
- repository visibility changes or claims of enforced GitHub controls;
- raw-byte equality of Tantivy's private on-disk files across builds. P2 determinism is semantic and contract-level, as defined below.

## Approaches considered

### Approach A — application-controlled BM25 with a separate graph-expansion channel

Build the exact Tantivy schema, tokenize the query with the same named analyzer used at index time, construct term queries explicitly, collect all matching documents, and apply the stable application tie-break. Return BM25 seeds and graph expansions as separate ordered arrays.

Advantages:

- query punctuation cannot activate Tantivy query-language syntax;
- every field weight, token rule, score ordering rule, and tie-break is explicit;
- graph expansion cannot silently displace a BM25 top-five result;
- the Q3 arithmetic remains bound to a single unambiguous ordered list;
- expansion provenance can record the seed, relation, and direction without inventing a graph score;
- upgrades remain gated by explicit retrieval-format identifiers.

Costs:

- requires a small query-construction layer instead of using `QueryParser` directly;
- must retrieve stored IDs before final application sorting.

**Decision:** Use Approach A.

### Approach B — Tantivy `QueryParser` plus graph interleaving

Pass natural-language input to Tantivy's query parser, take its top-k, and insert graph neighbors into the ranked list.

Advantages:

- less query-construction code;
- exposes phrase and Boolean syntax automatically.

Reasons rejected:

- punctuation, colons, quotes, and Boolean words gain syntax that the frozen human queries did not approve as query language;
- parser behavior and default field behavior become a larger hidden compatibility surface;
- interleaving requires a synthetic graph score or arbitrary displacement rule;
- a graph neighbor could alter Q3 without a principled relevance score;
- output would no longer be plainly described as deterministic BM25 top-k.

### Approach C — persist only Tantivy and rebuild Petgraph from the bundle on every query

Store only Tantivy's directory. Reverify and reparse the bundle to build a transient graph for every query.

Advantages:

- fewer persisted files;
- graph always comes directly from the bundle at query time.

Reasons rejected:

- the published index would not carry a reviewable, hashable graph projection;
- graph counts could not be checked as part of index-open integrity before query execution;
- two-build graph determinism would be harder to isolate from query-time parsing;
- the `index` command would not fully construct the P2 derived retrieval artifact.

The selected design still re-verifies the authoritative bundle before query and evaluation. Persisting `graph.json` does not make it authoritative; it makes the derived graph auditable and corruption-detectable.

## Dependencies and toolchain compatibility

### Direct dependency pins

The implementation adds these exact direct dependencies:

```toml
tantivy = { version = "=0.24.2", default-features = false, features = ["mmap", "lz4-compression"] }
petgraph = { version = "=0.8.3", default-features = false, features = ["std"] }
```

No other retrieval library is added.

### Why these versions

- Tantivy `0.24.2` declares Rust `1.81`, provides BM25, custom tokenizers, exact text-field options, single-threaded writing, and on-disk indexes, and therefore fits the repository's Rust `1.84` pin.
- Tantivy `0.26.1` declares Rust `1.86` and is incompatible with the frozen repository toolchain. P2 does not raise Rust merely to use the newest Tantivy release.
- Petgraph `0.8.3` declares Rust `1.64`, so it is compatible with Rust `1.84`.
- A separate dependency probe resolved an MSRV-compatible lock and compiled Tantivy `0.24.2` plus Petgraph `0.8.3` successfully with `rustc 1.84.0` and `cargo 1.84.0`.

### Lockfile and resolver behavior

Both direct versions use exact requirements and the resulting `sovmem/Cargo.lock` is committed.

Cargo 1.84's MSRV-aware resolver must be enabled while adding or regenerating the lock:

```text
CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback
```

This is required because permissive transitive requirements can otherwise select newer `time`, `uuid`, `indexmap`, or build-support releases whose declared Rust versions exceed 1.84 even though Tantivy itself is compatible.

The implementation plan must include an explicit clean Rust 1.84 `cargo test --locked` proof. Updating the lock with incompatible-rust-version behavior set to `allow`, using `--ignore-rust-version`, or raising `rust-toolchain.toml` is outside the approved P2 implementation.

### Feature choices

- `mmap` supports the committed on-disk index interface.
- `lz4-compression` supplies Tantivy's stored-field compression.
- the optional stop-word list is not enabled or used;
- stemming is not enabled or used;
- Petgraph enables only `std`; `Graph` plus an application-owned canonical ID map is sufficient;
- Petgraph serialization is not enabled. The application owns the strict `graph.json` schema and reconstructs Petgraph from it.

Dependency downloads occur only at build time through Cargo. The compiled `sovmem` binary has no network client or network-call path.

## Module boundaries and public interfaces

P2 remains in the existing `sovmem` crate.

### `sovmem/src/profile/index.rs`

Responsibilities:

- require explicit Level 3 bundle verification;
- convert verified `GeneratedDocument` values into canonical index documents;
- define and validate the exact Tantivy schema;
- register the named tokenizer;
- write one single-threaded Tantivy segment in canonical ingestion order;
- construct the deterministic index manifest;
- publish a complete generation through the two-slot protocol;
- open and validate the current generation for query/evaluation.

Public interface:

```rust
pub struct IndexRequest {
    pub profile_path: PathBuf,
    pub bundle_root: PathBuf,
    pub index_root: PathBuf,
}

pub struct IndexSummary {
    pub documents: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub external_typed_links: usize,
    pub bundle_hash: String,
    pub index_config: String,
}

pub fn build_index(request: IndexRequest) -> Result<IndexSummary, IndexError>;
```

Internal `open_current_index` returns only after the current pointer, strict index manifest, graph projection, Tantivy schema, document IDs, document count, bundle binding, and configuration identifiers all correspond.

The open path receives the freshly Level-3-verified corpus snapshot. It recomputes the canonical indexed-document commitment and the exact graph projection from that snapshot, then requires them to match the manifest and persisted graph before returning a search handle.

### `sovmem/src/profile/graph.rs`

Responsibilities:

- derive graph nodes and internal directed edges from verified generated frontmatter;
- count external typed links without creating external graph nodes;
- serialize and parse strict deterministic `graph.json`;
- reconstruct `petgraph::Graph<String, LinkRelation>` plus `BTreeMap<String, NodeIndex>`;
- return deterministic incoming and outgoing one-hop witnesses.

Public data structures are serializable summaries and expansion witnesses. Petgraph node indices are private implementation details and never enter persisted or CLI output.

### `sovmem/src/profile/query.rs`

Responsibilities:

- validate query input and top-k;
- reverify and bind the authoritative bundle before opening the derived index;
- tokenize with the exact index analyzer;
- construct the explicit boosted BM25 term query;
- collect, re-sort, and truncate stable BM25 results;
- optionally derive the separate one-hop expansion list;
- return metadata-only results.

Public interface:

```rust
pub enum ExpansionMode {
    None,
    OneHop,
}

pub struct QueryRequest {
    pub profile_path: PathBuf,
    pub bundle_root: PathBuf,
    pub index_root: PathBuf,
    pub query: String,
    pub top_k: usize,
    pub expansion: ExpansionMode,
}

pub fn query_index(request: QueryRequest) -> Result<QueryResult, QueryError>;
```

### `sovmem/src/profile/eval.rs`

Responsibilities:

- validate the frozen golden and threshold files through the existing strict loaders;
- evaluate every query in golden-file order with exactly `k = 5` and no graph expansion;
- compute integer hit counts and exact rational metric components;
- build deterministic `baseline.json` bytes;
- atomically replace the baseline only after a complete valid evaluation;
- return GO or NO-GO without tuning any input or retrieval parameter.

Public interface:

```rust
pub struct EvalRequest {
    pub profile_path: PathBuf,
    pub bundle_root: PathBuf,
    pub index_root: PathBuf,
    pub golden_path: PathBuf,
    pub thresholds_path: PathBuf,
    pub baseline_path: PathBuf,
}

pub enum Q3Decision {
    Go,
    NoGo,
}

pub fn evaluate(request: EvalRequest) -> Result<EvalOutcome, EvalError>;
```

### `sovmem/src/cli/p2.rs`

The existing `sovmem/src/cli.rs` keeps Clap definitions, top-level dispatch, shared diagnostic sorting, exit-code mapping, and compact JSON support. P2 command execution and text renderers live in `sovmem/src/cli/p2.rs` so the already large CLI module does not absorb indexing, scoring, or evaluation logic.

### Test modules

The fixture-first test surface is split by responsibility:

- `sovmem/tests/index_build.rs`;
- `sovmem/tests/graph_index.rs`;
- `sovmem/tests/query.rs`;
- `sovmem/tests/eval.rs`;
- `sovmem/tests/p2_cli.rs`;
- focused fixture helpers under `sovmem/tests/support/p2.rs`.

No test helper reads Audy Vault or requires network access. The final canonical gate is an additional local proof over the ignored verified bundle.

## Verified-corpus ingestion contract

### Mandatory precondition

`index` must run:

```rust
verify_bundle(
    &profile,
    &bundle_root,
    VerificationOptions::level(VerificationLevel::Three),
)
```

Compatibility verification is insufficient. Missing corpus evidence, malformed reports, changed document bytes, a mismatched bundle root, invalid provenance, dangling internal typed links, incomplete accepted-corpus evidence, or any other Level 1-3 error prevents all index writes.

The command additionally requires the verified result to contain:

- exactly 62 generated documents;
- a manifest and corpus report;
- 8 accepted core documents and 54 accepted research documents;
- zero corpus errors and passed corpus gates;
- the profile identity declared by the supplied profile;
- the source commit and bundle hash declared by the mutually consistent manifest/report evidence.

These are defense-in-depth assertions over Level 3 output, not a replacement for Level 3.

### Canonical document representation

Each verified `GeneratedDocument` becomes one index document:

| Field | Source | Cardinality | Tantivy behavior |
| --- | --- | ---: | --- |
| `id` | `metadata.id` | one | exact `STRING`, stored |
| `title` | `metadata.title` | one | tokenized BM25 text, stored |
| `body` | generated Markdown body after frontmatter | one | tokenized BM25 text, not stored |
| `layer` | serialized `metadata.layer` | one | exact `STRING`, stored |
| `type` | serialized `metadata.core_type` | one | exact `STRING`, stored |
| `tags` | every `metadata.tags[]` value, raw UTF-8 byte-sorted | zero or more | multivalued tokenized BM25 text, stored |
| `provenance` | serialized `metadata.provenance` | one | exact `STRING`, stored |

No other generated or source field enters the Tantivy schema. In particular, source paths, source text outside the generated body, sensitivity, authority, dates, owners, resource URIs, links, inferred fields, and source hashes are not indexed as searchable fields. Duplicate tags are rejected as invalid derived input rather than indexed twice.

### Canonical ingestion order

Verified documents are sorted before graph or Tantivy insertion by this tuple:

1. raw UTF-8 bytes of `metadata.source_path` ascending;
2. raw UTF-8 bytes of `metadata.id` ascending as a defensive secondary key.

Source paths are already unique and canonically bound at Level 3, so the second key should never decide a valid pair. Retaining it makes the rule total and reviewable.

The same ordered sequence defines:

- Tantivy document insertion order;
- persisted graph node order;
- Petgraph node insertion order;
- the bundle ID set used when classifying typed link targets.

Tantivy uses one writer thread. After the single commit, the builder waits for Tantivy's merge threads to finish before strict reopen and publication. P2 does not add documents concurrently, update documents, or delete documents.

## Tantivy schema and tokenizer

### Retrieval-format identifiers

The implementation defines exact constants:

```text
INDEX_SCHEMA_ID = sovmem-tantivy-schema-v1
TOKENIZER_ID = sovmem-simple-v1
RANKING_ID = sovmem-bm25-v1
INDEX_MANIFEST_SCHEMA = sovmem-derived-index-v1
GRAPH_SCHEMA = sovmem-derived-graph-v1
```

Changing any indexed/stored option, field boost, token filter, term-combination rule, tie-break, or expansion rule requires a new identifier and a new reviewed design. Existing generations with mismatched identifiers fail closed as stale/incompatible.

### Indexed text options

`title`, `body`, and `tags` use `TextFieldIndexing` with:

- tokenizer name `sovmem-simple-v1`;
- `IndexRecordOption::WithFreqs`;
- field norms enabled;
- no positions, because P2 has no phrase query interface;
- stored values only where specified in the field table.

Term frequencies and field norms are required for BM25. Positions are deliberately absent because accepting phrase syntax would reopen the query-language contract and enlarge the index without serving the frozen evaluation.

### Exact tokenizer pipeline

The named analyzer is:

```rust
TextAnalyzer::builder(SimpleTokenizer::default())
    .filter(RemoveLongFilter::limit(80))
    .filter(LowerCaser)
    .build()
```

The same analyzer instance definition is registered when building and opening the index, and is used to tokenize queries.

Consequences:

- whitespace and punctuation split terms;
- Unicode letters and numbers remain eligible tokens under Tantivy's simple tokenizer;
- lowercasing uses Tantivy's Unicode-aware lowercaser;
- tokens longer than 80 UTF-8 bytes are removed;
- there is no stemming;
- there is no stop-word removal;
- there is no ASCII folding;
- there are no synonyms, n-grams, language detection, or dynamically loaded tokenizers.

The 80-byte limit is an input-safety bound, not a corpus-specific tuning parameter. Tokenizer fixture tests freeze behavior for ASCII case, punctuation, hyphens, apostrophes, Unicode, `CRISP²`, and over-limit tokens.

## Query construction and BM25 scoring

### Query input validation

A query must:

- be valid UTF-8 supplied by Clap as a Rust `String`;
- contain at least one non-whitespace character;
- contain no control character;
- be at most 4,096 UTF-8 bytes;
- produce at least one and at most 64 tokens through `sovmem-simple-v1`.

The binary never echoes a rejected query, a successful query, or its tokens in diagnostics. Query input is not written to disk or included in `baseline.json`.

Interactive `query` accepts `--top-k` from 1 through 62 and defaults to 5. Values outside the range are deterministic usage errors. `eval` does not accept an override and always takes `k` from the frozen threshold file.

### Explicit field query

P2 does not expose Tantivy query-language syntax.

For each query token, the application creates one `TermQuery` for each searchable field with `IndexRecordOption::WithFreqs`, wraps it in a fixed `BoostQuery`, and combines every clause with Boolean `Should`:

| Field | Boost |
| --- | ---: |
| `title` | 3.0 |
| `tags` | 2.0 |
| `body` | 1.0 |

Repeated query tokens remain repeated clauses and therefore retain their natural repeated-term influence. Tokens are not silently deduplicated.

The boosts are fixed before the first Q3 evaluation. They express document structure—titles are curated summaries, tags are curated descriptors, and bodies are full text—rather than knowledge of any golden relevance label. The implementation must not change them after observing the canonical evaluation result during P2.

`id`, `layer`, `type`, and `provenance` are not part of BM25 scoring. P2 does not add filter syntax.

### Stable result ordering

Tantivy's score is the retrieval score, but Tantivy's internal document address is never accepted as a public tie-break.

For the 62-document corpus, the search path asks `TopDocs` for up to the verified document count so an internal collector cannot truncate a tied group before the application tie-break. It resolves the stored metadata, rejects a missing/duplicate/non-finite result, and sorts with this total order:

1. raw BM25 `f32` score descending using `f32::total_cmp`;
2. raw UTF-8 bytes of canonical `id` ascending.

Only after that sort does the application truncate to `top_k`.

This avoids segment ID, insertion address, heap collector, filesystem order, or hash-map iteration becoming observable ranking behavior.

Query JSON reports a fixed six-decimal score string for diagnostics and human comparison. Ranking uses the unrounded score. `baseline.json` deliberately omits scores and stores ordered IDs only, so the frozen gate is not coupled to display rounding or insignificant cross-platform floating representation differences.

No matching document is a successful query with empty result arrays. A query that produces no tokens is an error.

## Graph data model

### Nodes

There is exactly one graph node for each verified accepted document ID. Node order follows canonical document ingestion order. The current bundle therefore produces exactly 62 nodes.

### Edges

For each `GeneratedMetadata.links[]` entry:

1. preserve its typed relation;
2. remove an optional `vault://` prefix from the target for identity comparison;
3. remove an optional heading fragment beginning with `#` for node identity;
4. if the normalized target is an exact ID in the verified 62-document ID set, create one directed edge from the source document to that target;
5. otherwise classify it as an external typed link and create no node or edge.

No relative body link, Markdown link, wikilink, title similarity, shared tag, directory relation, source-path relation, or external URL creates a graph edge.

The persisted internal edge key is:

```text
(source_id UTF-8 bytes, relation string UTF-8 bytes, target_id UTF-8 bytes)
```

Edges are sorted by that key and must be unique. A duplicate edge is rejected as `graph.duplicate_edge`; it is not silently collapsed. Level 3 should already make this unreachable for a valid generated bundle, so the check is defense in depth.

The current verified bundle is expected to derive 19 internal edges and 81 external typed links. An unexpected count stops canonical gate closure and requires inspection; the implementation must not hard-code the counts into the graph builder.

### Persisted `graph.json`

The file is strict JSON with unknown fields denied:

```json
{
  "schema_version": "sovmem-derived-graph-v1",
  "bundle_hash": "<64 lowercase hex>",
  "nodes": [
    {"id": "<canonical document id>"}
  ],
  "edges": [
    {"source": "<id>", "relation": "references", "target": "<id>"}
  ],
  "external_typed_links": 81
}
```

The actual file uses deterministic pretty JSON, two-space indentation, stable struct field order, canonical node/edge order, and exactly one trailing newline.

On open, every node and edge is validated against the index manifest and stored Tantivy IDs before the Petgraph value is constructed. The parser does not trust persisted Petgraph indices.

The freshly verified bundle is also projected again through the same pure graph function. Its canonical `graph.json` bytes must equal the selected generation's bytes exactly. This rejects a valid-looking injected, removed, or relabeled edge even when all referenced IDs exist.

## One-hop expansion semantics

Graph expansion is optional and has two CLI values:

```text
--expand none
--expand one-hop
```

The default is `none`.

When `one-hop` is selected:

1. the stable BM25 top-k list is finalized first;
2. for every BM25 seed in rank order, inspect both outgoing and incoming internal edges;
3. create candidate witnesses sorted by:
   - seed rank ascending;
   - direction, `outgoing` before `incoming`;
   - relation string raw UTF-8 bytes ascending;
   - neighbor ID raw UTF-8 bytes ascending;
4. remove candidates whose neighbor is already a BM25 seed;
5. deduplicate by neighbor ID, keeping the earliest witness in the order above;
6. return every remaining unique neighbor in a separate `expanded` array.

Each expansion result records:

- expansion rank;
- neighbor document metadata;
- `via_id`, the BM25 seed;
- `direction` relative to the seed;
- the typed relation.

Expansion is exactly one hop. Expanded nodes are never expanded again.

Graph results have no BM25 score, receive no synthetic graph score, never alter BM25 score/order, never displace a BM25 top-k document, and are not included in Q3 Precision@5 or Recall@5. This separation makes the optional graph feature useful for context discovery without smuggling an unreviewed reranking formula into the frozen gate.

Two-build testing compares both the BM25 arrays and the optional expansion arrays for every golden query.

## Derived-index directory and publication

### CLI-owned index root

The user supplies an explicit `--index <index-root>` path. The repository's exact Audy root command uses `var/audy-sovmem-derived-index/`, an ignored sibling of `var/audy-sovmem-retrieval-corpus/`, not a child of the verified bundle root.

The index root must not equal, contain, or be contained by the verified bundle root. This preserves the Level 3 exact-file-set contract and prevents a derived index from changing authoritative bundle verification.

The index path is never serialized into the index manifest, graph, baseline, audit, or CLI success object.

### Two-slot layout

```text
<index-root>/
├── CURRENT                 # exactly "slot-a
" or "slot-b
"
├── slot-a/                 # current or inactive complete generation
│   ├── index-manifest.json
│   ├── graph.json
│   └── tantivy/
├── slot-b/                 # current or inactive complete generation
│   ├── index-manifest.json
│   ├── graph.json
│   └── tantivy/
└── .building-*             # unpublished temporary generation, if interrupted
```

The slot name is publication state only. It is excluded from all semantic output and hashes.

### Full rebuild protocol

Every `index` invocation performs these steps:

1. Load and validate the profile and paths.
2. Explicitly verify the bundle through Level 3 and assert the fixed accepted-corpus contract.
3. Read `CURRENT` if present. A valid current value selects the other slot; an absent current selects `slot-a`. An invalid current does not block a full rebuild but is reported in the successful rebuild summary as `replaced_invalid_current: true`.
4. Build a complete generation in a same-filesystem `.building-*` sibling under the index root.
5. Write Tantivy, `graph.json`, and `index-manifest.json`.
6. Reopen the temporary generation through the same strict open path used by `query` and assert its bundle binding, schema, IDs, document count, graph count, and queryability.
7. Close the Tantivy writer, merge threads, reader, and every temporary-generation file handle. P2 requires failure-atomic publication but does not claim power-loss durability for disposable index bytes.
8. Remove only the inactive target slot. Never remove or modify the slot named by a valid `CURRENT`.
9. Rename the complete temporary generation to the inactive slot.
10. Write a temporary pointer file containing the inactive slot name, flush and `sync_all` that pointer file, and atomically replace `CURRENT` with a same-directory rename on the supported repository platforms.
11. The newly named slot becomes current. The former current slot remains complete and becomes inactive until the next successful rebuild.
12. Remove stale `.building-*` directories only after identifying them as unpublished derived output beneath this exact index root.

There is no add/update/delete API against a current Tantivy generation. Rebuild means constructing every node, edge, and document again from zero.

### Publication failure behavior

- Any failure before the `CURRENT` replacement leaves the previous valid current generation selected and queryable.
- An incomplete temporary directory is never selected by `CURRENT`.
- A failed inactive-slot cleanup or rename leaves the old current untouched.
- If the platform cannot provide same-directory atomic file replacement, publication fails closed before selecting the new generation; P2 does not emulate success with a partially written pointer.
- Query and eval accept only exact pointer values and complete validated generations.
- A first build that fails leaves no `CURRENT`; subsequent query/eval returns `index.current_invalid` rather than reading temporary files.

The P2 publication target is a local macOS or Linux filesystem that provides POSIX same-directory rename semantics. On any other operating system, `index` refuses publication with `index.publish_unsupported` and exit `2`; cross-platform publication hardening requires a later reviewed design.

On the supported platforms, the protocol guarantees that a reader selects a complete old or complete new generation, never a partially constructed generation. P2 does not claim general multi-process concurrency support; concurrent rebuild/query stress is P5 hardening.

## `index-manifest.json`

Each complete generation contains strict deterministic JSON:

```json
{
  "schema_version": "sovmem-derived-index-v1",
  "index_schema": "sovmem-tantivy-schema-v1",
  "tokenizer": "sovmem-simple-v1",
  "ranking": "sovmem-bm25-v1",
  "sovmem_version": "0.1.0",
  "tantivy_version": "0.24.2",
  "petgraph_version": "0.8.3",
  "profile": "nous-sovmem-v0",
  "context_namespace": "ssa-0",
  "source_commit": "43824cfd9caed7b260aa442128fd1130ddb6e2ff",
  "bundle_hash": "<verified bundle BLAKE3>",
  "documents_hash": "<canonical indexed-document BLAKE3>",
  "graph_hash": "<raw graph.json BLAKE3>",
  "documents": 62,
  "graph_nodes": 62,
  "graph_edges": 19,
  "external_typed_links": 81,
  "config_hash": "<BLAKE3 of framed retrieval configuration>"
}
```

The example count values describe the current verified bundle. The builder derives them. Angle-bracket values in JSON and command examples are explicit runtime-derived metavariables, not unresolved design decisions.

`config_hash` covers exact length-framed UTF-8 bytes for:

- every retrieval-format identifier;
- field names in schema order;
- indexed/stored/index-record options;
- tokenizer filters and 80-byte limit;
- field boosts represented by fixed canonical strings;
- query term-combination mode;
- rank and tie-break rules;
- expansion rules;
- exact Tantivy and Petgraph versions.

`documents_hash` covers the canonical document sequence and, for every document, exact length-framed field values in schema order. It includes every raw body byte represented as UTF-8 and every raw-byte-sorted tag, but it is serialized only as a 64-character BLAKE3. `graph_hash` is the raw-byte BLAKE3 of the canonical persisted `graph.json`.

The manifest has no timestamp, hostname, username, absolute path, slot name, temporary generation name, process ID, random value, source excerpt, body, or query text.

### Root command wiring

The implementation adds exact repository scripts for the canonical local paths:

```text
npm run index:audy
npm run query:audy -- --query "<text>" [--top-k <n>] [--expand one-hop]
npm run eval:audy
```

`index:audy` and `query:audy` bind `var/audy-sovmem-retrieval-corpus/` to `var/audy-sovmem-derived-index/`. `eval:audy` additionally binds the frozen golden and threshold files and writes `evals/audy-sovmem-v0/baseline.json`. The scripts do not read Audy Vault and do not accept alternate frozen evaluation inputs.

## CLI contracts

### Shared format and diagnostics

Each P2 command accepts:

```text
--format json|text
```

The default is `json`, matching the current CLI.

- JSON success is exactly one compact object and one trailing newline on stdout.
- Text success is a stable line-oriented renderer intended for human inspection.
- Diagnostics remain sorted, deduplicated, privacy-safe JSON Lines on stderr in both modes, preserving the current greppable contract.
- Success emits no stderr.
- Failure caused by invalid input/index state emits no success summary.
- A valid Q3 NO-GO is an evaluation outcome: it writes and prints the valid result, emits the stable `eval.q3_no_go` diagnostic, and exits `1`.

No diagnostic contains source bodies, query text, query tokens, title text, absolute paths, environment variables, or Tantivy internal filenames.

### `index`

Command:

```text
sovmem index \
  --profile <profile.yaml> \
  --bundle <verified-bundle-root> \
  --index <derived-index-root> \
  [--format json|text]
```

Successful JSON field order:

```json
{"status":"ok","command":"index","schema_version":"sovmem-derived-index-v1","documents":62,"graph_nodes":62,"graph_edges":19,"external_typed_links":81,"bundle_hash":"<hash>","index_config":"<config hash>","replaced_invalid_current":false}
```

Text form:

```text
status: ok
command: index
documents: 62
graph-nodes: 62
graph-edges: 19
external-typed-links: 81
bundle-hash: <hash>
index-config: <config hash>
replaced-invalid-current: false
```

No slot or absolute output path is exposed.

### `query`

Command:

```text
sovmem query \
  --profile <profile.yaml> \
  --bundle <verified-bundle-root> \
  --index <derived-index-root> \
  --query <text> \
  [--top-k <1..62>] \
  [--expand none|one-hop] \
  [--format json|text]
```

Successful JSON shape:

```json
{
  "status": "ok",
  "command": "query",
  "top_k": 5,
  "expansion": "none",
  "bundle_hash": "<hash>",
  "results": [
    {
      "rank": 1,
      "id": "<canonical id>",
      "title": "<frontmatter title>",
      "type": "concept",
      "layer": "synthesis",
      "provenance": "synthesis",
      "score": "12.345678"
    }
  ],
  "expanded": []
}
```

With one-hop expansion, each `expanded` entry is:

```json
{
  "rank": 1,
  "id": "<neighbor id>",
  "title": "<frontmatter title>",
  "type": "resource",
  "layer": "analysis",
  "provenance": "unknown",
  "via_id": "<seed id>",
  "direction": "outgoing",
  "relation": "references"
}
```

Text form is exact line-oriented output. Metadata strings after `title=` use JSON string escaping:

```text
status: ok
command: query
top-k: 5
expansion: one-hop
bundle-hash: <hash>
bm25[1]: id=<id> score=12.345678 title="<escaped title>"
expanded[1]: id=<id> via=<seed id> direction=outgoing relation=references title="<escaped title>"
```

BM25 lines occur in BM25 rank order. Expanded lines are absent for `none` and occur in expansion rank order for `one-hop`. Empty sections emit no result lines. Text output never prints a body or snippet.

The command does not echo the query.

### `eval`

Command:

```text
sovmem eval \
  --profile <profile.yaml> \
  --bundle <verified-bundle-root> \
  --index <derived-index-root> \
  --golden <golden.jsonl> \
  --thresholds <thresholds.json> \
  --baseline <baseline.json> \
  [--format json|text]
```

There is no `--top-k`, threshold override, relevance override, query selector, boost override, tokenizer override, or expansion override.

GO JSON:

```json
{"status":"go","command":"eval","evaluation_set":"audy-sovmem-v0","queries":25,"k":5,"queries_with_hit":20,"required_queries_with_hit":20,"precision_numerator":20,"precision_denominator":125,"recall_numerator":20,"recall_denominator":25,"baseline_written":true}
```

NO-GO uses the same fields with `status: "no-go"`, the actual integer count, `baseline_written: true`, diagnostic `eval.q3_no_go`, and exit `1`.

Text form:

```text
Q3: GO
evaluation-set: audy-sovmem-v0
queries: 25
k: 5
hits: 20/25
required-hits: 20/25
macro-precision-at-5: 20/125
macro-recall-at-5: 20/25
baseline-written: true
```

The text renderer ends with one deterministic line containing missed query IDs in golden-file order:

```text
missed-query-ids: q-003,q-017
```

If no query misses, it prints `missed-query-ids: none`. It never prints query text or rationales.

## Stable error taxonomy and exits

### Exit meanings

| Exit | Meaning |
| ---: | --- |
| `0` | Command completed and every requested gate passed. |
| `1` | Readable data, index state, evaluation evidence, or Q3 failed a deterministic gate. |
| `2` | CLI usage, profile/configuration, path safety, required local input, or publication/write setup failed safely. |

### Index build codes

| Code | Exit | Meaning |
| --- | ---: | --- |
| `index.bundle_unverified` | 1 | Explicit Level 3 verification or the fixed accepted-corpus assertion failed. Underlying Level 1-3 diagnostics are emitted too. |
| `index.output_invalid` | 2 | Index root is unreadable, unsafe, a symlink alias, or overlaps the bundle. |
| `index.build_failed` | 2 | Tantivy, graph, or manifest construction could not complete. |
| `index.self_check_failed` | 1 | The complete temporary generation did not reopen as the exact state just built. |
| `index.publish_unsupported` | 2 | The operating system does not provide the approved macOS/Linux publication semantics. |
| `index.publish_failed` | 2 | The current generation could not be replaced safely. |

### Index-open codes

| Code | Exit | Meaning |
| --- | ---: | --- |
| `index.current_invalid` | 1 | `CURRENT` is absent, malformed, unsafe, or selects no complete slot. |
| `index.manifest_invalid` | 1 | Strict manifest parsing or invariant validation failed. |
| `index.bundle_mismatch` | 1 | Current generation is bound to another verified bundle, profile, or source commit. |
| `index.schema_mismatch` | 1 | Tantivy schema, tokenizer, ranking/config identifier, or dependency version differs. |
| `index.document_set_invalid` | 1 | Stored IDs are missing, duplicated, unexpected, or do not equal verified bundle IDs. |
| `index.graph_invalid` | 1 | Graph JSON, counts, node set, edge set, or Petgraph reconstruction is invalid. |
| `index.tantivy_invalid` | 1 | Tantivy cannot open/search the complete selected directory. |

### Query codes

| Code | Exit | Meaning |
| --- | ---: | --- |
| `query.invalid` | 2 | Query contains controls, exceeds 4,096 bytes, or top-k is out of range. |
| `query.empty` | 2 | Query is blank or produces zero tokens. |
| `query.too_many_tokens` | 2 | Analyzer produces more than 64 tokens. |
| `query.execute_failed` | 1 | A validated index failed during search or result reconstruction. |

### Evaluation codes

Existing exact `golden.*` and `thresholds.*` codes remain authoritative for those files.

| Code | Exit | Meaning |
| --- | ---: | --- |
| `eval.baseline_output_invalid` | 2 | Baseline destination is unsafe, unreadable, or cannot be prepared without risking the prior file. |
| `eval.write_failed` | 2 | Complete baseline bytes could not be atomically published; prior baseline remains. |
| `eval.q3_no_go` | 1 | Valid evaluation produced fewer than the required 20 hit queries. |

Errors never fall back to a partial result, previous baseline presented as current, compatibility-level bundle verification, a stale index generation, or a best-effort query.

## `baseline.json` contract

### Location and version

The tracked canonical output is:

```text
evals/audy-sovmem-v0/baseline.json
```

Its schema version is:

```text
sovmem-retrieval-baseline-v1
```

The file uses strict deterministic pretty JSON, two-space indentation, serializer-defined field order, golden-file query order, stable result ID order, and exactly one trailing newline.

### Schema

```json
{
  "schema_version": "sovmem-retrieval-baseline-v1",
  "evaluation_set": {
    "id": "audy-sovmem-v0",
    "golden_file": "evals/audy-sovmem-v0/golden.jsonl",
    "golden_blake3": "f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175",
    "thresholds_file": "evals/audy-sovmem-v0/thresholds.json",
    "thresholds_blake3": "979566e38e59f3dc4d144c8112e8280d78fa741f63f82cc292b48d4be479d021",
    "query_count": 25,
    "relevant_documents_per_query": 1,
    "k": 5
  },
  "corpus": {
    "profile": "nous-sovmem-v0",
    "context_namespace": "ssa-0",
    "source_commit": "43824cfd9caed7b260aa442128fd1130ddb6e2ff",
    "bundle_hash": "49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65",
    "documents": 62,
    "core": 8,
    "research": 54
  },
  "retrieval": {
    "engine": "tantivy-bm25",
    "sovmem_version": "0.1.0",
    "tantivy_version": "0.24.2",
    "petgraph_version": "0.8.3",
    "index_schema": "sovmem-tantivy-schema-v1",
    "tokenizer": "sovmem-simple-v1",
    "ranking": "sovmem-bm25-v1",
    "index_config": "<config hash>",
    "graph_expansion": "none"
  },
  "index": {
    "documents": 62,
    "documents_hash": "<canonical indexed-document BLAKE3>",
    "graph_nodes": 62,
    "graph_edges": 19,
    "external_typed_links": 81,
    "graph_hash": "<raw graph.json BLAKE3>"
  },
  "metrics": {
    "queries_with_hit": 20,
    "queries_without_hit": 5,
    "precision_at_5": {
      "numerator": 20,
      "denominator": 125,
      "decimal": "0.1600"
    },
    "recall_at_5": {
      "numerator": 20,
      "denominator": 25,
      "decimal": "0.8000"
    },
    "gate": {
      "minimum_queries_with_hit": 20,
      "status": "go"
    }
  },
  "queries": [
    {
      "id": "q-001",
      "relevant_ids": ["<frozen relevant id>"],
      "result_ids": ["<rank 1>", "<rank 2>", "<rank 3>", "<rank 4>", "<rank 5>"],
      "hit_rank": 1
    }
  ]
}
```

The example metric values illustrate the minimum GO boundary. The generated file contains actual results.

Each `result_ids` array contains zero through five IDs in final BM25 order. `hit_rank` is the one-based position of the single frozen relevant ID or JSON `null` when it is absent. Per-query scores, query text, and rationales are not serialized.

### Evaluation-set identity and provenance

The baseline is valid only if:

- the strict frozen threshold loader accepts the threshold and golden files;
- both raw-byte hashes are recorded;
- the golden count and single-relevance contract correspond to the threshold identity;
- the bundle passes explicit Level 3 at evaluation time;
- the current derived generation exactly matches that bundle;
- retrieval identifiers, direct dependency versions, and `config_hash` match compiled constants;
- canonical indexed-document and graph hashes match the freshly verified bundle projection;
- all 25 queries execute successfully in file order.

There is no timestamp, elapsed time, hostname, username, absolute path, Git worktree path, index slot, random identifier, score, query text, rationale, source body, or source excerpt in the baseline.

### Metric arithmetic

For query `i` with one relevant document:

```text
hit_i = 1 if relevant_id is in ordered result_ids[0..5], otherwise 0
P@5_i = hit_i / 5
R@5_i = hit_i / 1
H = sum(hit_i) over all 25 queries
Macro P@5 = H / (25 * 5) = H / 125
Macro R@5 = H / 25
```

The gate is:

```text
GO iff H >= 20
```

No floating point participates in the decision. Decimal strings are derived for reporting only after the integer decision and are formatted with exactly four digits after the decimal point. At the frozen single-relevance contract, `H >= 20` is exactly equivalent to both macro threshold comparisons.

### Baseline publication

Evaluation constructs and serializes the entire baseline in memory, writes a sibling temporary file, synchronizes it, and atomically replaces the destination through the repository's supported local-filesystem publication helper.

- Invalid inputs or query/index errors do not touch a prior baseline.
- A valid NO-GO writes the actual deterministic baseline because it is necessary decision evidence.
- A write failure preserves the previous baseline and reports `eval.write_failed`.
- The CLI never reports `baseline_written: true` until the exact final bytes are in place.

## Determinism proof

### What must be identical

For two clean builds from the exact same verified bundle and compiled retrieval configuration:

- document count;
- graph node count;
- internal edge count;
- external typed-link count;
- `index-manifest.json` bytes;
- `graph.json` bytes;
- ordered BM25 top-five IDs for all 25 golden queries;
- ordered optional one-hop expansion entries for all 25 golden queries;
- generated `baseline.json` bytes.

Tantivy's private segment filenames and other raw on-disk bytes are not compared. They are not public format and may contain library-owned generation details. Query equivalence, strict manifest/graph equality, and exact baseline equality are the P2 determinism contract.

### Canonical two-build proof

The final P2 gate performs:

1. Verify the canonical bundle at Level 3.
2. Build index root A from zero.
3. Run `eval` against A to baseline A.
4. Run every golden query against A with `--expand one-hop` and save a metadata-only semantic snapshot.
5. Build separate index root B from zero.
6. Run `eval` against B to baseline B.
7. Run every golden query against B with `--expand one-hop` and save the same semantic snapshot.
8. Compare the required counts, manifest bytes, graph bytes, baseline bytes, BM25 IDs, and expansion entries exactly.

Any mismatch is `index.self_check_failed` or a failing determinism test; it is not normalized away.

### Deleted-index rebuild equivalence

After the two-build comparison:

1. Delete only derived index root A.
2. Assert the bundle, manifest, corpus report, golden file, threshold file, and baseline source inputs are byte-unchanged.
3. Rebuild root A from zero.
4. Repeat the semantic snapshot and evaluation.
5. Require exact equality with the first A result and baseline.

Automated integration coverage performs the same sequence on a synthetic Level 3 fixture entirely under the test temporary directory. The canonical proof is run locally against the ignored 62-document bundle before audit publication.

## TDD and fixture-first coverage

Implementation occurs only after a separately reviewed plan and follows test-driven development.

### Dependency and schema fixtures

- exact direct dependency versions are present;
- `cargo test --locked` runs on Rust 1.84;
- the Tantivy schema has exactly seven logical fields in the approved order;
- indexed, stored, tokenizer, and record options match the schema contract;
- index-open rejects every schema/config/version mutation.

### Bundle gate fixtures

- valid synthetic Level 3 corpus builds;
- compatibility-verifiable but non-Level-3 bundle is rejected;
- missing corpus report is rejected before any index write;
- malformed frontmatter is rejected before any index write;
- document hash, bundle root, provenance, source correspondence, corpus count, and link corruption preserve the previous current index;
- an invalid current index does not prevent a clean full rebuild from a valid bundle;
- bundle and canonical source fixtures are never mutated.

### Index and publication fixtures

- canonical ingestion is independent of filesystem enumeration order;
- one writer thread creates the exact document ID set;
- failed build/self-check/pointer publication keeps the old current slot;
- `CURRENT` cannot select traversal, an absolute path, a symlink alias, a temporary directory, or an incomplete slot;
- index root overlap with the bundle is rejected;
- interrupted `.building-*` state is never queryable;
- replacing an invalid current state reports the stable boolean without leaking paths;
- full rebuild does not call any incremental document update path.

### Graph fixtures

- typed internal frontmatter links create directed labeled edges;
- a Markdown link or wikilink in the body with no matching frontmatter link creates no edge;
- external typed links create no node/edge and increment the external count;
- `vault://` and heading-fragment normalization follow the exact identity rule;
- duplicate edges, missing nodes, count mismatches, and unexpected graph fields fail;
- incoming/outgoing expansion order and witness deduplication are exact;
- expansion is one hop and never expands an expansion result.

### Tokenization and scoring fixtures

- case, punctuation, hyphens, apostrophes, Unicode, `CRISP²`, and 80-byte removal behavior are frozen;
- title/tag/body boosts are exact;
- query language characters remain ordinary punctuation rather than operators;
- repeated tokens remain repeated clauses;
- blank, control-containing, oversized, zero-token, over-64-token, and out-of-range top-k inputs fail with stable codes;
- tied BM25 scores sort by canonical ID, not document address;
- a changed insertion order cannot change final result order;
- no result body or query text appears in output or diagnostics.

### Evaluation fixtures

- exactly 20 hits yields GO, precision `20/125`, and recall `20/25`;
- 19 hits yields NO-GO even if a rounded display might look close;
- 25 hits yields exact upper-bound metrics;
- golden order controls query result record order;
- golden/threshold byte changes retain the existing stable rejection codes;
- evaluation cannot override k, thresholds, labels, boosts, tokenizer, or expansion;
- valid NO-GO writes baseline and exits 1;
- invalid evaluation preserves a prior baseline;
- baseline bytes contain no clock, path, slot, score, query text, or environment state.

### Determinism fixtures

- two independent builds have exact manifest/graph and semantic equality;
- rebuilding the same root switches slots without changing semantic output;
- deleting the entire derived root and rebuilding produces the same output;
- all fixture queries produce identical ordered BM25 and expansion arrays across builds;
- baseline A and baseline B are byte-identical.

### Privacy and offline fixtures

- no absolute bundle/index/baseline path is serialized or emitted;
- errors do not echo unsafe query input, bodies, titles, or Tantivy filenames;
- index and evaluation contain no secret-like source excerpt;
- no P2 module imports or invokes an HTTP, socket, model, embedding, or external-process API;
- command execution succeeds with network unavailable.

## Q3 GO/NO-GO behavior

### GO

Q3 is GO only when all of these are true at the reviewed implementation head:

1. The canonical bundle passes explicit Level 3 with exactly 62 documents, 8 core, 54 research, 226 warnings, one exclusion, and zero errors.
2. `index`, `query`, and `eval` satisfy their exact contracts.
3. Two independent clean builds and one deleted-index rebuild satisfy every determinism comparison.
4. `baseline.json` is valid, deterministic, and bound to the frozen evaluation and retrieval configuration.
5. At least 20 of 25 golden queries hit the single relevant document within the BM25 top five.
6. Optional one-hop expansion is deterministic for every golden query.
7. The full JavaScript and Rust suites pass under the pinned toolchains and committed lock.
8. `npm run audit:sovmem:check` passes after the implementation audit update.

On GO, the implementation may record P2 100% and `gate: passed` in the reviewed build state, with the generated audit reporting approximately 37% overall completion.

### NO-GO

If all implementation and determinism checks pass but fewer than 20 queries hit:

1. Write the actual valid NO-GO `baseline.json`.
2. Emit `eval.q3_no_go` and exit 1.
3. Stop P2 gate closure.
4. Mark P2 blocked rather than passed in the reviewed build state.
5. Record D2 as the human decision required next.
6. Present the per-query ordered IDs and misses without changing queries, labels, fixtures, thresholds, boosts, tokenizer, or scoring.
7. Do not begin embeddings, hybrid retrieval, P3, MCP, or any other escape path without a new explicit human decision and reviewed design.

There is no second P2 tuning run after observing the canonical result. A change to retrieval behavior after NO-GO is a new reviewed decision, not an implementation correction hidden inside the existing gate.

## Audit-state impact

### Design-only commit

This design commit does not change:

- `docs/sovmem-build-state.json`;
- `docs/SOVMEM-BUILD-AUDIT.md`;
- P2's current 10%;
- the current 24% overall completion;
- test evidence counts;
- implementation evidence.

The design file alone is not proof that retrieval exists.

### Eventual GO implementation

After complete implementation and only if Q3 is GO:

- set P2 completion to 100 and gate to `passed`;
- replace the P2 remaining-work item with exact completed capabilities;
- add tracked P2 evidence including `evals/audy-sovmem-v0/baseline.json`, retrieval modules, tests, and the frozen evaluation files;
- record actual final JavaScript/Rust test counts rather than predicted counts;
- record the reviewed implementation commit, canonical bundle hash, gate hit count, and deterministic-build evidence in the audit contract as supported by its schema;
- regenerate `docs/SOVMEM-BUILD-AUDIT.md` with `npm run audit:sovmem`;
- require `npm run audit:sovmem:check` and the complete test suite at the reviewed head.

With effort weights unchanged, P0 + P1 + P2 fully passed is `5 / 13.5`, which rounds to 37% overall.

### Eventual NO-GO implementation

If the implementation is complete but Q3 fails:

- do not set P2 to 100 or `passed`;
- use the audit's existing `blocked` gate state;
- identify D2 as the blocker and retain the actual deterministic baseline as evidence;
- preserve the frozen inputs and report the next human decision without claiming phase closure.

No audit text may claim enforced GitHub branch protection, rulesets, required checks, or merge queues. Manual pull-request review and merge remains the repository discipline.

## Implementation sequence constraints

The post-approval implementation plan must be fixture-first and must preserve this order:

1. Freeze dependency/MSRV and schema/tokenizer tests.
2. Build strict graph and index-manifest fixtures.
3. Implement Level-3-gated in-memory document conversion.
4. Implement temporary generation build and strict reopen.
5. Implement two-slot publication and rollback tests.
6. Implement explicit BM25 query construction and tie-break tests.
7. Implement separate one-hop expansion and witness tests.
8. Implement frozen evaluation arithmetic and baseline publication.
9. Wire CLI JSON/text surfaces and root scripts.
10. Run synthetic determinism and deleted-index tests.
11. Run canonical two-build, deleted-index, and Q3 evaluation once.
12. On GO only, update and regenerate audit state; on NO-GO, stop at D2.

The plan must not schedule canonical baseline evaluation before retrieval behavior and its tests are frozen. This prevents evaluation feedback from becoming implicit query or ranking tuning.

## Exit gates and stop conditions

### Design exit gate

This design slice is complete only when:

- the document contains no unresolved placeholder;
- every required design question has one exact answer;
- frozen P0/P1/Q3 decisions are unchanged;
- only this design document is committed;
- the repository owner reviews and approves the committed specification.

No implementation plan is written before that approval.

### Plan exit gate

After design approval, use `superpowers:writing-plans` to produce a detailed fixture-first TDD implementation plan, commit only that plan slice, and stop for repository-owner approval.

No implementation begins before plan approval.

### Immediate stop conditions during eventual implementation

Stop and ask for human resolution if any of these occurs:

- live `origin/main`, the pinned bundle, corpus counts, source commit, golden hash, or threshold bytes contradict this design;
- Tantivy `0.24.2` or Petgraph `0.8.3` cannot pass the committed Rust 1.84 locked build;
- explicit Level 3 does not pass the canonical bundle before indexing;
- an implementation requires changing the seven-field logical schema;
- a graph edge would require body parsing or an external node;
- deterministic ranking would require Tantivy document addresses or unstable iteration;
- publication can expose a partial current generation;
- baseline determinism would require timestamps, paths, scores, or normalization of mismatched results;
- fewer than 20 golden queries hit in the first five;
- any proposed fix changes a frozen query, relevance ID, threshold, corpus fixture, or canonical source;
- implementation drifts into P3, P4, P5, embeddings, networking, model calls, or repository governance claims.

## Reference links

- Tantivy `0.24.2` crate metadata and source: <https://docs.rs/crate/tantivy/0.24.2>
- Petgraph `0.8.3` crate metadata and source: <https://docs.rs/crate/petgraph/0.8.3>
- Cargo MSRV-aware resolver stabilized in Rust 1.84: <https://blog.rust-lang.org/2025/01/09/Rust-1.84.0/>
- Cargo dependency resolver reference: <https://doc.rust-lang.org/cargo/reference/resolver.html>

## Final design decision

P2 will publish a complete, offline, Level-3-bound derived generation containing a seven-field Tantivy BM25 index and a strict typed-link graph projection. Retrieval uses a frozen custom tokenizer, fixed structural field boosts, application-owned total ordering, and canonical-ID tie-breaking. Optional one-hop graph neighbors are returned separately and never alter the frozen BM25 top-five gate. Evaluation produces one deterministic, provenance-bound baseline and decides Q3 using the integer number of hit queries. A result below 20 stops at D2; it does not trigger tuning or scope expansion.
