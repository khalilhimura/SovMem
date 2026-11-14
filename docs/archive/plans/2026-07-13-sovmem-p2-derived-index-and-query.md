> **ARCHIVED** — This implementation plan has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix A: Implementation Plan Index](../sovmem-master-spec.md#appendix-a-implementation-plan-index) for a summary of its scope.
>
# SovMem P2 Derived Index and Query Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and gate a disposable, Level-3-bound Tantivy BM25 index with typed Petgraph one-hop expansion, deterministic CLI query/evaluation interfaces, and a provenance-bound Q3 baseline over the frozen 62-document Audy corpus.

**Architecture:** Keep the existing single Rust crate and add four focused profile modules: verified index construction/opening, pure graph projection/adjacency, explicit BM25 query execution, and frozen evaluation/baseline publication. Every index build starts from explicit Level 3 verification, publishes a complete generation through a two-slot pointer, and binds the generation to canonical document and graph commitments. Query and eval reverify the bundle, reject stale or corrupted generations, use application-owned ranking/tie-breaking, and keep graph expansion separate from the BM25 top-five gate.

**Tech Stack:** Rust 1.84.0; Tantivy `=0.24.2` with `mmap` and `lz4-compression`; Petgraph `=0.8.3` with `std`; existing `clap`, `serde`, `serde_json`, `serde_yaml`, `blake3`, `anyhow`, and `thiserror`; Node.js 20 ESM and `node:test`; Git CLI; deterministic JSON/JSONL and Markdown.

## Global Constraints

- The approved design is `docs/superpowers/specs/2026-07-13-sovmem-p2-derived-index-and-query-design.md` at commit `16c328d6eb11a8be2dc9dd6a3311b856a75f3515`.
- `docs/sovmem_v0.3_execution_plan.md` remains normative. Stop on any contradiction.
- Work only in `.worktrees/sovmem-p2-derived-index-query` on `codex/sovmem-p2-derived-index-query` until the branch is manually reviewed.
- The implementation base is `origin/main` commit `3599bfbb4ed0c5d8522f8878fb022c3bd1c7d585` plus the approved design commit.
- Preserve the single Rust crate, edition 2021, `rust-version = "1.84"`, and `rust-toolchain.toml` channel `1.84.0`.
- Add exactly `tantivy = "=0.24.2"` with features `mmap,lz4-compression` and `petgraph = "=0.8.3"` with feature `std`; add no other retrieval dependency.
- Regenerate `sovmem/Cargo.lock` with `CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback`; never use `--ignore-rust-version` or raise Rust.
- Audy Vault remains canonical and read-only at `43824cfd9caed7b260aa442128fd1130ddb6e2ff`; bundle, index, graph, and baseline are derived.
- Require explicit `VerificationLevel::Three` before any index write and before every query/eval open.
- Preserve exactly 62 accepted documents: 8 core and 54 research, with 226 warnings, one exclusion, and zero errors in canonical evidence.
- Preserve the exact seven-field logical schema in order: `id`, `title`, `body`, `layer`, `type`, `tags`, `provenance`.
- Index only `title`, `tags`, and `body` for BM25; store metadata exactly as the design specifies; never index source paths or link text as search fields.
- Use tokenizer `sovmem-simple-v1`: `SimpleTokenizer`, `RemoveLongFilter::limit(80)`, then `LowerCaser`; no stemming, stop words, ASCII folding, synonyms, embeddings, or vectors.
- Use fixed field boosts `title=3.0`, `tags=2.0`, `body=1.0`; combine every query term/field clause with Boolean `Should`.
- Rank by raw BM25 score descending using `f32::total_cmp`, then canonical ID raw UTF-8 bytes ascending; truncate only after application sorting.
- Derive graph nodes only from verified document IDs and graph edges only from typed frontmatter `links[]`; never infer from body links, wikilinks, tags, paths, or titles.
- Keep external typed links counted but outside Petgraph. Current canonical evidence is 62 nodes, 19 internal edges, and 81 external typed links; derive rather than hard-code these counts.
- One-hop expansion visits outgoing before incoming edges, sorts by seed rank/relation/neighbor ID, deduplicates by neighbor ID, and never changes BM25 rank or Q3 metrics.
- Preserve the 25 human-approved golden queries and `evals/audy-sovmem-v0/thresholds.json` byte-for-byte.
- Evaluate exactly `k=5`; GO requires integer `queries_with_hit >= 20`; floating point is reporting-only.
- Do not perform a canonical evaluation until Tasks 1-11 have frozen dependency, tokenizer, scoring, expansion, CLI, corruption, and determinism behavior.
- If the first canonical evaluation is NO-GO, write the valid baseline, stop, and present D2. Do not tune code, boosts, queries, labels, fixtures, or thresholds.
- Support failure-atomic two-slot publication on macOS/Linux only. Other operating systems return `index.publish_unsupported` with exit `2`.
- Keep all diagnostics deterministic and redacted: no bodies, query text/tokens, title text on errors, source excerpts, absolute paths, environment values, or Tantivy internal filenames.
- Keep the binary offline: no HTTP client, socket, external model, embedding, vector database, or runtime network call.
- Preserve the provenance enum `tier1|secondary|synthesis|unknown`, the five-verb internal ledger, and the three-verb public subset without implementing P3.
- Do not implement P3 proposal/verdict/write-back work, P4 MCP, or P5 sentry/trust/dogfood work.
- The private repository uses manual pull-request merge discipline. Never claim branch protection, rulesets, required checks, or merge queues are enforced.
- Starting baseline is 49 JavaScript tests and 203 Rust tests; report final counts from actual output, never prediction.
- Every production task follows RED → GREEN → focused regression → commit. Do not combine task commits.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `sovmem/Cargo.toml` | Exact Tantivy/Petgraph direct pins and feature set. |
| `sovmem/Cargo.lock` | Committed Rust-1.84-compatible transitive resolution. |
| `sovmem/src/profile/index.rs` | Schema/tokenizer constants, verified-corpus intake, canonical documents, document/config commitments, Tantivy build/open, strict manifest validation, and two-slot publication. |
| `sovmem/src/profile/graph.rs` | Pure typed-link graph projection, strict deterministic `graph.json`, Petgraph reconstruction, and ordered one-hop witnesses. |
| `sovmem/src/profile/query.rs` | Query validation/tokenization, explicit boosted BM25 query, total result ordering, metadata results, and optional expansion joining. |
| `sovmem/src/profile/eval.rs` | Frozen golden/threshold loading, integer metrics, deterministic `baseline.json`, atomic baseline write, and Q3 decision. |
| `sovmem/src/profile/mod.rs` | Register/export P2 modules without changing existing interfaces. |
| `sovmem/src/cli.rs` | Add P2 Clap variants, shared format enums, dispatch, and exit mapping. |
| `sovmem/src/cli/p2.rs` | JSON/text success renderers and redacted command handlers for `index`, `query`, and `eval`. |
| `sovmem/tests/support/p2.rs` | Reusable synthetic 62-document Level 3 bundle, controlled metadata/body/link mutations, golden/threshold copies, temporary roots, and semantic snapshots. |
| `sovmem/tests/support/mod.rs` | Export the shared P2 fixture module. |
| `sovmem/tests/index_build.rs` | Dependency, schema, verified intake, document commitment, path safety, and publication tests; crate-local tests cover unpublished generation internals. |
| `sovmem/tests/graph_index.rs` | Typed-link-only projection, canonical JSON, external-link behavior, corruption, and expansion ordering. |
| `sovmem/tests/query.rs` | Tokenizer, query limits, boosts, BM25 order, canonical tie-break, metadata privacy, and no-match behavior. |
| `sovmem/tests/eval.rs` | Integer arithmetic, frozen evidence identity, deterministic baseline bytes, atomic replacement, GO, and NO-GO. |
| `sovmem/tests/p2_cli.rs` | Exact JSON/text outputs, exit classes, stable codes, no leaks, and argument restrictions. |
| `sovmem/tests/p2_determinism.rs` | Two clean builds, same-root slot switch, all-query semantic equality, and deleted-index rebuild equivalence. |
| `evals/audy-sovmem-v0/baseline.json` | Canonical deterministic P2 result; created only by the first frozen canonical evaluation. |
| `test/retrieval-corpus-version.test.js` | Exact dependency/root-command/baseline contracts and continued frozen-input checks. |
| `package.json` | `index:audy`, `query:audy`, and `eval:audy` canonical local commands. |
| `README.md` | Supported P2 interfaces, offline/derived boundary, formats, and Q3 result. |
| `index.md` | Root artifact registry for modules, tests, plan, baseline, and phase state. |
| `docs/sovmem_v0.3_execution_plan.md` | After GO only, record implemented P2 status without altering frozen architecture. |
| `scripts/lib/sovmem-build-audit.mjs` | After GO only, validate exact P2 evidence and gate arithmetic in audit schema v3. |
| `test/sovmem-build-audit.test.js` | After GO only, pin P2 audit evidence, 37% weighting, privacy, and manual discipline. |
| `docs/sovmem-build-state.json` | After GO only, reviewed P2=100/passed evidence. |
| `docs/SOVMEM-BUILD-AUDIT.md` | After GO only, regenerated approximately 37% report. |

---

### Task 1: Pin dependencies and freeze the Tantivy schema/tokenizer contract

**Files:**
- Modify: `sovmem/Cargo.toml`
- Modify: `sovmem/Cargo.lock`
- Create: `sovmem/src/profile/index.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Create: `sovmem/tests/index_build.rs`
- Modify: `test/retrieval-corpus-version.test.js`

**Interfaces:**
- Produces constants `INDEX_MANIFEST_SCHEMA`, `INDEX_SCHEMA_ID`, `TOKENIZER_ID`, `RANKING_ID`, `TANTIVY_VERSION`, and `PETGRAPH_VERSION`.
- Produces `IndexFields { id, title, body, layer, core_type, tags, provenance }`.
- Produces crate-private `build_schema() -> (tantivy::schema::Schema, IndexFields)` and `register_tokenizer(index: &tantivy::Index)`.
- Produces public `IndexSchemaDescriptor` and `index_schema_descriptor() -> IndexSchemaDescriptor` for contract tests.

- [ ] **Step 1: Add failing repository dependency tests**

Extend `test/retrieval-corpus-version.test.js`:

```js
test('pins the Rust 1.84 compatible P2 retrieval dependencies', () => {
  const cargo = readFileSync('sovmem/Cargo.toml', 'utf8');
  assert.match(cargo, /tantivy = \{ version = "=0\.24\.2", default-features = false, features = \["mmap", "lz4-compression"\] \}/);
  assert.match(cargo, /petgraph = \{ version = "=0\.8\.3", default-features = false, features = \["std"\] \}/);
  assert.doesNotMatch(cargo, /qdrant|embedding|vector/i);
});
```

- [ ] **Step 2: Add the failing schema contract test**

Create `sovmem/tests/index_build.rs`:

```rust
#[allow(dead_code)]
mod support;

use sovmem::profile::index_schema_descriptor;

#[test]
fn exposes_the_exact_p2_schema_and_tokenizer_contract() {
    let descriptor = index_schema_descriptor();
    assert_eq!(descriptor.schema_id, "sovmem-tantivy-schema-v1");
    assert_eq!(descriptor.tokenizer, "sovmem-simple-v1");
    assert_eq!(descriptor.ranking, "sovmem-bm25-v1");
    assert_eq!(
        descriptor.fields,
        ["id", "title", "body", "layer", "type", "tags", "provenance"]
    );
    assert_eq!(descriptor.token_limit_bytes, 80);
    assert_eq!(descriptor.title_boost, "3.0");
    assert_eq!(descriptor.tags_boost, "2.0");
    assert_eq!(descriptor.body_boost, "1.0");
    assert_eq!(descriptor.tantivy_version, "0.24.2");
    assert_eq!(descriptor.petgraph_version, "0.8.3");
}
```

- [ ] **Step 3: Run focused tests and verify RED**

Run:

```bash
node --test test/retrieval-corpus-version.test.js
cargo test --manifest-path sovmem/Cargo.toml --test index_build
```

Expected: Node fails because the dependency pins are absent; Rust fails because `index_schema_descriptor` does not exist.

- [ ] **Step 4: Add exact dependencies with the MSRV-aware resolver**

Modify `sovmem/Cargo.toml`:

```toml
tantivy = { version = "=0.24.2", default-features = false, features = ["mmap", "lz4-compression"] }
petgraph = { version = "=0.8.3", default-features = false, features = ["std"] }
```

Regenerate the lock with:

```bash
CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback cargo update --manifest-path sovmem/Cargo.toml
```

Expected: the resolver reports Rust-1.84-compatible selections; it must not select a package whose declared Rust version exceeds 1.84.

- [ ] **Step 5: Implement schema constants and exact field options**

Create `sovmem/src/profile/index.rs` with these public constants/types and schema construction:

```rust
use serde::Serialize;
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, STORED, STRING,
};
use tantivy::tokenizer::{LowerCaser, RemoveLongFilter, SimpleTokenizer, TextAnalyzer};
use tantivy::Index;

pub const INDEX_MANIFEST_SCHEMA: &str = "sovmem-derived-index-v1";
pub const INDEX_SCHEMA_ID: &str = "sovmem-tantivy-schema-v1";
pub const TOKENIZER_ID: &str = "sovmem-simple-v1";
pub const RANKING_ID: &str = "sovmem-bm25-v1";
pub const TANTIVY_VERSION: &str = "0.24.2";
pub const PETGRAPH_VERSION: &str = "0.8.3";
pub const TOKEN_LIMIT_BYTES: usize = 80;
pub const TITLE_BOOST: f32 = 3.0;
pub const TAGS_BOOST: f32 = 2.0;
pub const BODY_BOOST: f32 = 1.0;

#[derive(Clone, Copy)]
pub(crate) struct IndexFields {
    pub id: Field,
    pub title: Field,
    pub body: Field,
    pub layer: Field,
    pub core_type: Field,
    pub tags: Field,
    pub provenance: Field,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct IndexSchemaDescriptor {
    pub schema_id: &'static str,
    pub tokenizer: &'static str,
    pub ranking: &'static str,
    pub fields: [&'static str; 7],
    pub token_limit_bytes: usize,
    pub title_boost: &'static str,
    pub tags_boost: &'static str,
    pub body_boost: &'static str,
    pub tantivy_version: &'static str,
    pub petgraph_version: &'static str,
}

pub fn index_schema_descriptor() -> IndexSchemaDescriptor {
    IndexSchemaDescriptor {
        schema_id: INDEX_SCHEMA_ID,
        tokenizer: TOKENIZER_ID,
        ranking: RANKING_ID,
        fields: ["id", "title", "body", "layer", "type", "tags", "provenance"],
        token_limit_bytes: TOKEN_LIMIT_BYTES,
        title_boost: "3.0",
        tags_boost: "2.0",
        body_boost: "1.0",
        tantivy_version: TANTIVY_VERSION,
        petgraph_version: PETGRAPH_VERSION,
    }
}

pub(crate) fn build_schema() -> (Schema, IndexFields) {
    let searchable = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(TOKENIZER_ID)
            .set_index_option(IndexRecordOption::WithFreqs),
    );
    let mut builder = Schema::builder();
    let id = builder.add_text_field("id", STRING | STORED);
    let title = builder.add_text_field("title", searchable.clone().set_stored());
    let body = builder.add_text_field("body", searchable.clone());
    let layer = builder.add_text_field("layer", STRING | STORED);
    let core_type = builder.add_text_field("type", STRING | STORED);
    let tags = builder.add_text_field("tags", searchable.set_stored());
    let provenance = builder.add_text_field("provenance", STRING | STORED);
    let schema = builder.build();
    (schema, IndexFields { id, title, body, layer, core_type, tags, provenance })
}

pub(crate) fn register_tokenizer(index: &Index) {
    let analyzer = TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(TOKEN_LIMIT_BYTES))
        .filter(LowerCaser)
        .build();
    index.tokenizers().register(TOKENIZER_ID, analyzer);
}
```

Register it from `sovmem/src/profile/mod.rs`:

```rust
mod index;
pub use index::*;
```

- [ ] **Step 6: Run focused GREEN and locked build**

Run:

```bash
node --test test/retrieval-corpus-version.test.js
cargo test --locked --manifest-path sovmem/Cargo.toml --test index_build
cargo check --locked --manifest-path sovmem/Cargo.toml
```

Expected: all commands pass; Cargo uses Rust 1.84 and the committed lock.

- [ ] **Step 7: Commit Task 1**

```bash
git add sovmem/Cargo.toml sovmem/Cargo.lock sovmem/src/profile/index.rs sovmem/src/profile/mod.rs sovmem/tests/index_build.rs test/retrieval-corpus-version.test.js
git commit -m "P2.1: pin retrieval dependencies and schema [gate: pending]"
```

---

### Task 2: Build the shared Level 3 fixture and verified corpus intake

**Files:**
- Create: `sovmem/tests/support/p2.rs`
- Modify: `sovmem/tests/support/mod.rs`
- Modify: `sovmem/tests/index_build.rs`
- Modify: `sovmem/src/profile/index.rs`

**Interfaces:**
- Produces test helper `P2Fixture::new(name: &str)`, with `profile_path`, `bundle_root`, `index_root(name)`, `golden_path`, and `thresholds_path`.
- Produces `IndexDocument { source_path, id, title, body, layer, core_type, tags, provenance }`.
- Produces crate-private `VerifiedCorpus` and `load_verified_corpus(profile_path: &Path, bundle_root: &Path) -> Result<VerifiedCorpus, IndexError>`.
- Produces `canonical_document_hash(documents: &[IndexDocument]) -> String`.
- Produces `IndexError::code() -> &'static str` and stable `index.bundle_unverified`/`index.output_invalid` classifications.

- [ ] **Step 1: Create the fixture API and failing verified-intake tests**

Add to `sovmem/tests/support/mod.rs`:

```rust
pub mod p2;
```

Create `sovmem/tests/support/p2.rs` around the existing approved corpus exporter. The public surface must be exactly:

```rust
use sovmem::profile::{export_corpus, load_corpus, CorpusExportRequest, PINNED_SOURCE_COMMIT};
use std::path::{Path, PathBuf};

pub struct P2Fixture {
    area: TemporaryArea,
    pub profile_path: PathBuf,
    pub bundle_root: PathBuf,
    pub golden_path: PathBuf,
    pub thresholds_path: PathBuf,
    _git_environment: GitEnvironmentGuard,
}

impl P2Fixture {
    pub fn new(name: &str) -> Self {
        let area = TemporaryArea::new(name);
        let profile_path = repository_path("profiles/audy-ssa0-v0/profile.yaml");
        let corpus_path = repository_path("profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml");
        let vault = area.root.join("vault");
        initialize_git_repo(&vault);
        for (position, path) in load_corpus(&corpus_path)
            .expect("load fixed corpus")
            .selected_paths()
            .into_iter()
            .enumerate()
        {
            write_source(&vault, &path, position);
        }
        let actual_commit = commit_all(&vault);
        let git_environment = install_git_shim(&actual_commit, PINNED_SOURCE_COMMIT);
        let bundle_root = area.root.join("bundle");
        export_corpus(CorpusExportRequest {
            vault_root: vault,
            output_root: bundle_root.clone(),
            profile_path: profile_path.clone(),
            corpus_path,
        }).expect("export synthetic Level 3 bundle");
        let golden_path = area.root.join("golden.jsonl");
        let thresholds_path = area.root.join("thresholds.json");
        std::fs::copy(repository_path("evals/audy-sovmem-v0/golden.jsonl"), &golden_path)
            .expect("copy frozen golden");
        std::fs::copy(repository_path("evals/audy-sovmem-v0/thresholds.json"), &thresholds_path)
            .expect("copy frozen thresholds");
        Self { area, profile_path, bundle_root, golden_path, thresholds_path, _git_environment: git_environment }
    }

    pub fn index_root(&self, name: &str) -> PathBuf {
        self.area.root.join(name)
    }

    pub fn remove_corpus_report(&self) {
        std::fs::remove_file(self.bundle_root.join("corpus-report.json"))
            .expect("remove fixture corpus report");
    }
}
```

`TemporaryArea`, `initialize_git_repo`, `commit_all`, and `install_git_shim` must use the same deterministic dates and pinned-commit substitution already proven in `sovmem/tests/golden_queries.rs`; centralize the copied logic in this new helper rather than importing private test items. `write_source` writes deterministic titles/bodies and adds typed links only through YAML source metadata accepted by the current exporter. It never reads the live Audy Vault.

Extend `sovmem/tests/index_build.rs`:

```rust
use sovmem::profile::prepare_verified_corpus;
use std::collections::BTreeSet;
use support::p2::P2Fixture;

#[test]
fn prepares_exactly_sixty_two_canonical_documents_from_level_three() {
    let fixture = P2Fixture::new("verified-intake");
    let corpus = prepare_verified_corpus(&fixture.profile_path, &fixture.bundle_root)
        .expect("Level 3 corpus prepares");
    assert_eq!(corpus.documents, 62);
    assert_eq!(corpus.core, 8);
    assert_eq!(corpus.research, 54);
    assert_eq!(corpus.source_commit, "43824cfd9caed7b260aa442128fd1130ddb6e2ff");
    assert_eq!(corpus.document_ids.len(), 62);
    assert_eq!(corpus.document_ids.iter().collect::<BTreeSet<_>>().len(), 62);
    assert!(corpus.source_paths.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(corpus.documents_hash.len(), 64);
}

#[test]
fn rejects_non_level_three_input_before_creating_an_index_root() {
    let fixture = P2Fixture::new("missing-corpus-report");
    let index_root = fixture.index_root("index");
    fixture.remove_corpus_report();
    let error = prepare_verified_corpus(&fixture.profile_path, &fixture.bundle_root)
        .expect_err("missing Level 3 evidence must fail");
    assert_eq!(error.code(), "index.bundle_unverified");
    assert!(!index_root.exists());
}
```

- [ ] **Step 2: Run RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test index_build prepares_exactly_sixty_two_canonical_documents_from_level_three -- --exact
cargo test --locked --manifest-path sovmem/Cargo.toml --test index_build rejects_non_level_three_input_before_creating_an_index_root -- --exact
```

Expected: compile failure because `prepare_verified_corpus` and the fixture do not yet exist.

- [ ] **Step 3: Implement strict verified intake and canonical commitments**

Add to `sovmem/src/profile/index.rs`:

```rust
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use thiserror::Error;
use super::{
    load_profile, verify_bundle, CoreType, Layer, ProfileError, Provenance,
    VerificationDiagnostic, VerificationLevel, VerificationOptions,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexDocument {
    pub source_path: String,
    pub id: String,
    pub title: String,
    pub body: String,
    pub layer: String,
    pub core_type: String,
    pub tags: Vec<String>,
    pub provenance: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedCorpusSummary {
    pub documents: usize,
    pub core: usize,
    pub research: usize,
    pub source_commit: String,
    pub bundle_hash: String,
    pub document_ids: Vec<String>,
    pub source_paths: Vec<String>,
    pub documents_hash: String,
}

pub(crate) struct VerifiedCorpus {
    pub profile: String,
    pub context_namespace: String,
    pub source_commit: String,
    pub bundle_hash: String,
    pub core: usize,
    pub research: usize,
    pub documents: Vec<IndexDocument>,
    pub documents_hash: String,
}

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("profile configuration could not be loaded")]
    Profile(#[source] ProfileError),
    #[error("bundle root could not be read")]
    BundleRead,
    #[error("bundle did not pass explicit Level 3")]
    BundleUnverified(Vec<VerificationDiagnostic>),
    #[error("index output is invalid")]
    OutputInvalid,
    #[error("index build failed")]
    Build,
    #[error("index self-check failed")]
    SelfCheck,
    #[error("index publication is unsupported")]
    PublishUnsupported,
    #[error("index publication failed")]
    Publish,
    #[error("current index is invalid")]
    CurrentInvalid,
    #[error("index manifest is invalid")]
    ManifestInvalid,
    #[error("index bundle binding does not match")]
    BundleMismatch,
    #[error("index schema does not match")]
    SchemaMismatch,
    #[error("index document set is invalid")]
    DocumentSetInvalid,
    #[error("index graph is invalid")]
    GraphInvalid,
    #[error("Tantivy index is invalid")]
    TantivyInvalid,
}

impl IndexError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Profile(error) => error.code(),
            Self::BundleRead => "bundle.read",
            Self::BundleUnverified(_) => "index.bundle_unverified",
            Self::OutputInvalid => "index.output_invalid",
            Self::Build => "index.build_failed",
            Self::SelfCheck => "index.self_check_failed",
            Self::PublishUnsupported => "index.publish_unsupported",
            Self::Publish => "index.publish_failed",
            Self::CurrentInvalid => "index.current_invalid",
            Self::ManifestInvalid => "index.manifest_invalid",
            Self::BundleMismatch => "index.bundle_mismatch",
            Self::SchemaMismatch => "index.schema_mismatch",
            Self::DocumentSetInvalid => "index.document_set_invalid",
            Self::GraphInvalid => "index.graph_invalid",
            Self::TantivyInvalid => "index.tantivy_invalid",
        }
    }
}
```

Implement `load_verified_corpus` by loading the profile, canonicalizing the bundle root, calling explicit Level 3, rejecting any error diagnostics, requiring manifest/corpus report evidence, asserting 62/8/54/zero-errors/gates-passed, converting enum values through explicit match expressions, sorting documents by `(source_path.as_bytes(), id.as_bytes())`, sorting/deduplicating tags, rejecting duplicate IDs/tags, and computing `documents_hash` with this framing helper:

```rust
fn frame(hasher: &mut blake3::Hasher, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_be_bytes());
    hasher.update(value);
}

pub(crate) fn canonical_document_hash(documents: &[IndexDocument]) -> String {
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, b"sovmem-index-documents-v1");
    for document in documents {
        for value in [
            document.id.as_bytes(),
            document.title.as_bytes(),
            document.body.as_bytes(),
            document.layer.as_bytes(),
            document.core_type.as_bytes(),
            document.provenance.as_bytes(),
        ] {
            frame(&mut hasher, value);
        }
        frame(&mut hasher, &(document.tags.len() as u64).to_be_bytes());
        for tag in &document.tags {
            frame(&mut hasher, tag.as_bytes());
        }
    }
    hasher.finalize().to_hex().to_string()
}
```

Expose the test-facing summary without exposing bodies:

```rust
pub fn prepare_verified_corpus(
    profile_path: &Path,
    bundle_root: &Path,
) -> Result<PreparedCorpusSummary, IndexError> {
    let corpus = load_verified_corpus(profile_path, bundle_root)?;
    Ok(PreparedCorpusSummary {
        documents: corpus.documents.len(),
        core: corpus.core,
        research: corpus.research,
        source_commit: corpus.source_commit,
        bundle_hash: corpus.bundle_hash,
        document_ids: corpus.documents.iter().map(|document| document.id.clone()).collect(),
        source_paths: corpus.documents.iter().map(|document| document.source_path.clone()).collect(),
        documents_hash: corpus.documents_hash,
    })
}
```

- [ ] **Step 4: Run GREEN and existing Level 3 regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test index_build
cargo test --locked --manifest-path sovmem/Cargo.toml --test bundle_verification
cargo test --locked --manifest-path sovmem/Cargo.toml --test corpus_report
```

Expected: all focused tests pass; fixture export remains offline and Level 3 strict.

- [ ] **Step 5: Commit Task 2**

```bash
git add sovmem/tests/support/mod.rs sovmem/tests/support/p2.rs sovmem/tests/index_build.rs sovmem/src/profile/index.rs
git commit -m "P2.2: require Level 3 corpus intake [gate: pending]"
```

---

### Task 3: Derive and validate the typed-link graph projection

**Files:**
- Create: `sovmem/src/profile/graph.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `sovmem/src/profile/index.rs`
- Create: `sovmem/tests/graph_index.rs`
- Modify: `sovmem/tests/support/p2.rs`

**Interfaces:**
- Produces `GraphProjection`, `GraphNode`, `GraphEdge`, `ExpansionWitness`, and `Direction`.
- Produces `derive_graph(bundle_hash: &str, documents: &[GeneratedDocument]) -> Result<GraphProjection, GraphError>`.
- Produces `GraphProjection::to_bytes()`, `GraphProjection::from_bytes()`, `GraphProjection::blake3()`, and `GraphProjection::adjacency()`.
- Produces `GraphAdjacency::expand_one_hop(seed_ids: &[String]) -> Vec<ExpansionWitness>`.
- Adds exact graph projection to `VerifiedCorpus`.

- [ ] **Step 1: Write failing typed-link-only projection tests**

Create `sovmem/tests/graph_index.rs` with direct synthetic `GeneratedDocument` helpers and these tests:

```rust
use sovmem::profile::{derive_graph, GeneratedDocument, GeneratedMetadata, LinkRelation, TypedLink};

#[test]
fn derives_only_internal_typed_frontmatter_edges() {
    let documents = graph_documents(vec![
        TypedLink { rel: LinkRelation::References, target: "ssa-0/beta#section".into() },
        TypedLink { rel: LinkRelation::Cites, target: "https://example.com/source".into() },
    ]);
    let graph = derive_graph(&"a".repeat(64), &documents).expect("derive graph");
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.external_typed_links, 1);
    assert_eq!(graph.edges[0].source, "ssa-0/alpha");
    assert_eq!(graph.edges[0].relation, "references");
    assert_eq!(graph.edges[0].target, "ssa-0/beta");
}

#[test]
fn body_links_never_create_graph_edges() {
    let mut documents = graph_documents(Vec::new());
    documents[0].body = "[[beta]] [beta](beta.md)".into();
    let graph = derive_graph(&"b".repeat(64), &documents).expect("derive graph");
    assert!(graph.edges.is_empty());
}

#[test]
fn graph_json_is_canonical_and_rejects_unknown_or_duplicate_edges() {
    let graph = derive_graph(&"c".repeat(64), &graph_documents(one_internal_link()))
        .expect("derive graph");
    let bytes = graph.to_bytes().expect("serialize graph");
    assert_eq!(bytes.last(), Some(&b'
'));
    assert_eq!(GraphProjection::from_bytes(&bytes).unwrap(), graph);
    assert_eq!(graph.blake3().len(), 64);
    let duplicated = duplicate_first_edge(&bytes);
    assert_eq!(GraphProjection::from_bytes(&duplicated).unwrap_err().code(), "graph.duplicate_edge");
}
```

- [ ] **Step 2: Run RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test graph_index
```

Expected: compile failure because graph types/functions do not exist.

- [ ] **Step 3: Implement strict graph projection and deterministic JSON**

Create `sovmem/src/profile/graph.rs`:

```rust
use petgraph::graph::{Graph, NodeIndex};
use petgraph::{Direction as PetDirection, Directed};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;
use super::{GeneratedDocument, LinkRelation};

pub const GRAPH_SCHEMA: &str = "sovmem-derived-graph-v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct GraphNode { pub id: String }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct GraphEdge { pub source: String, pub relation: String, pub target: String }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphProjection {
    pub schema_version: String,
    pub bundle_hash: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub external_typed_links: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GraphError {
    #[error("graph schema is invalid")] Schema,
    #[error("graph node set is invalid")] Node,
    #[error("graph edge is duplicated")] DuplicateEdge,
    #[error("graph edge target is invalid")] Edge,
    #[error("graph JSON is invalid")] Parse,
}

impl GraphError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Schema => "graph.schema",
            Self::Node => "graph.node",
            Self::DuplicateEdge => "graph.duplicate_edge",
            Self::Edge => "graph.edge",
            Self::Parse => "graph.parse",
        }
    }
}
```

`derive_graph` sorts documents by source-path bytes, emits one node per ID, normalizes `vault://` and `#fragment`, creates edges only for exact node IDs, counts every other target as external, sorts `(source,relation,target)`, and rejects duplicate edges. Serialize with `serde_json::to_vec_pretty` plus one newline. `from_bytes` uses deny-unknown-fields, verifies lowercase 64-hex bundle hash, strictly sorted unique nodes/edges, known relation strings, and internal endpoints.

Implement adjacency with application-owned IDs:

```rust
pub struct GraphAdjacency {
    graph: Graph<String, String, Directed>,
    by_id: BTreeMap<String, NodeIndex>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction { Outgoing, Incoming }

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExpansionWitness {
    pub id: String,
    pub via_id: String,
    pub direction: Direction,
    pub relation: String,
}
```

`expand_one_hop` enumerates seed IDs in supplied rank order, outgoing before incoming, sorts relation/neighbor raw bytes, excludes seeds, and retains the first candidate per neighbor in a `BTreeSet<String>`.

Register/export from `profile/mod.rs`, then add `graph: GraphProjection` to `VerifiedCorpus` and derive it from the already verified `GeneratedDocument` values before they are converted into `IndexDocument`.

- [ ] **Step 4: Add expansion-order tests and run GREEN**

Add tests that prove two seeds sharing one neighbor retain the earlier seed, outgoing precedes incoming, relation byte order precedes neighbor ID, and expanded nodes never become seeds. Then run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test graph_index
cargo test --locked --manifest-path sovmem/Cargo.toml --test link_conversion
cargo test --locked --manifest-path sovmem/Cargo.toml --test validation
```

Expected: all tests pass; existing link conversion semantics remain unchanged.

- [ ] **Step 5: Commit Task 3**

```bash
git add sovmem/src/profile/graph.rs sovmem/src/profile/mod.rs sovmem/src/profile/index.rs sovmem/tests/graph_index.rs sovmem/tests/support/p2.rs
git commit -m "P2.3: derive typed-link graph projection [gate: pending]"
```

---

### Task 4: Build and strictly reopen one complete index generation

**Files:**
- Modify: `sovmem/src/profile/index.rs`

**Interfaces:**
- Produces strict `IndexManifest` with document/graph/config commitments.
- Produces crate-private `build_generation(corpus: &VerifiedCorpus, generation_root: &Path) -> Result<IndexManifest, IndexError>`.
- Produces `OpenedIndex` and `open_generation(corpus: &VerifiedCorpus, generation_root: &Path) -> Result<OpenedIndex, IndexError>`.
- Produces `IndexRequest`, `IndexSummary`, and later publication-compatible manifest bytes.

- [ ] **Step 1: Write failing crate-local complete-generation tests**

Add a `#[cfg(test)] mod tests` to `sovmem/src/profile/index.rs`. Construct a minimal in-memory `VerifiedCorpus` with two canonical documents and a matching graph, so the tests exercise only generation mechanics and do not create a second public build API:

```rust
#[test]
fn builds_and_reopens_one_complete_generation() {
    let fixture = GenerationFixture::new("complete-generation");
    let generation = fixture.generation_root();
    let built = build_generation(&fixture.corpus, &generation).expect("build generation");
    assert_eq!(built.documents, 2);
    assert_eq!(built.graph_nodes, 2);
    assert!(generation.join("tantivy/meta.json").is_file());
    assert!(generation.join("graph.json").is_file());
    assert!(generation.join("index-manifest.json").is_file());
    let opened = open_generation(&fixture.corpus, &generation).expect("reopen generation");
    assert_eq!(opened.manifest.documents, 2);
    assert_eq!(opened.metadata.len(), 2);
}

#[test]
fn rejects_manifest_graph_schema_and_document_set_corruption() {
    for mutation in ["manifest", "graph", "tantivy"] {
        let fixture = GenerationFixture::new(mutation);
        let generation = fixture.generation_root();
        build_generation(&fixture.corpus, &generation).unwrap();
        fixture.corrupt_generation(&generation, mutation);
        let error = open_generation(&fixture.corpus, &generation)
            .expect_err("corruption must fail");
        assert!(matches!(error.code(),
            "index.manifest_invalid" | "index.graph_invalid" |
            "index.schema_mismatch" | "index.document_set_invalid" |
            "index.tantivy_invalid"));
    }
}
```

`GenerationFixture` is private to the test module, uses `tempfile`, and sets stable in-memory identity/hash values. It must not be exported from `profile`.

- [ ] **Step 2: Run RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::index::tests::builds_and_reopens_one_complete_generation -- --exact
```

Expected: compile failure because generation build/open functions do not exist.

- [ ] **Step 3: Implement exact manifest and config hash**

Add to `index.rs`:

```rust
#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct IndexManifest {
    pub schema_version: String,
    pub index_schema: String,
    pub tokenizer: String,
    pub ranking: String,
    pub sovmem_version: String,
    pub tantivy_version: String,
    pub petgraph_version: String,
    pub profile: String,
    pub context_namespace: String,
    pub source_commit: String,
    pub bundle_hash: String,
    pub documents_hash: String,
    pub graph_hash: String,
    pub documents: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub external_typed_links: usize,
    pub config_hash: String,
}
```

Implement `retrieval_config_hash()` with the same length framing as document hashing over every format identifier, field option, tokenizer step/limit, boost string, `boolean-should`, `score-desc-id-asc`, `one-hop-separated`, and dependency version in the design's exact order. Test the hash is 64 lowercase hexadecimal characters and identical across calls.

- [ ] **Step 4: Implement deterministic Tantivy generation build**

Build the on-disk index with one writer thread:

```rust
let (schema, fields) = build_schema();
std::fs::create_dir_all(generation_root.join("tantivy"))
    .map_err(|_| IndexError::Build)?;
let index = tantivy::Index::create_in_dir(generation_root.join("tantivy"), schema)
    .map_err(|_| IndexError::Build)?;
register_tokenizer(&index);
let mut writer = index.writer_with_num_threads(1, 50_000_000)
    .map_err(|_| IndexError::Build)?;
for source in &corpus.documents {
    let mut document = tantivy::TantivyDocument::new();
    document.add_text(fields.id, &source.id);
    document.add_text(fields.title, &source.title);
    document.add_text(fields.body, &source.body);
    document.add_text(fields.layer, &source.layer);
    document.add_text(fields.core_type, &source.core_type);
    for tag in &source.tags { document.add_text(fields.tags, tag); }
    document.add_text(fields.provenance, &source.provenance);
    writer.add_document(document).map_err(|_| IndexError::Build)?;
}
writer.commit().map_err(|_| IndexError::Build)?;
writer.wait_merging_threads().map_err(|_| IndexError::Build)?;
```

Write canonical `graph.json`, compute its raw BLAKE3, write the strict manifest as pretty JSON plus one newline, and never write absolute paths/timestamps/slot/process IDs into either file.

- [ ] **Step 5: Implement strict generation open**

`open_generation` must:

1. parse the manifest with unknown fields denied;
2. compare every identity/version/config/count/hash field to the freshly verified `VerifiedCorpus`;
3. recompute exact graph bytes and require raw equality with `graph.json`;
4. open Tantivy and require `index.schema() == build_schema().0`;
5. register the tokenizer and open a manual-reload reader;
6. run `AllQuery` with `TopDocs::with_limit(corpus.documents.len())`;
7. reconstruct stored ID/title/type/layer/tags/provenance and require exact equality with the verified canonical metadata map;
8. reject missing/duplicate/unexpected IDs and any Tantivy error.

- [ ] **Step 6: Run GREEN and corruption regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::index::tests
cargo test --locked --manifest-path sovmem/Cargo.toml --test graph_index
```

Expected: complete generation and every corruption class behave as asserted; no source body appears in output.

- [ ] **Step 7: Commit Task 4**

```bash
git add sovmem/src/profile/index.rs
git commit -m "P2.4: build and validate complete index generations [gate: pending]"
```

---

### Task 5: Publish full rebuilds through the two-slot current pointer

**Files:**
- Modify: `sovmem/src/profile/index.rs`
- Modify: `sovmem/tests/index_build.rs`

**Interfaces:**
- Produces `build_index(request: IndexRequest) -> Result<IndexSummary, IndexError>`.
- Produces crate-private `open_current_index(profile_path, bundle_root, index_root) -> Result<OpenedIndex, IndexError>`.
- Publishes exact `CURRENT` values `slot-a
` or `slot-b
` and never exposes `.building-*`.

- [ ] **Step 1: Write failing first-build, slot-switch, and rollback tests**

Add:

```rust
use sovmem::profile::{build_index, IndexRequest};

fn request(fixture: &P2Fixture, index_root: PathBuf) -> IndexRequest {
    IndexRequest {
        profile_path: fixture.profile_path.clone(),
        bundle_root: fixture.bundle_root.clone(),
        index_root,
    }
}

#[test]
fn publishes_first_build_to_slot_a_and_second_to_slot_b() {
    let fixture = P2Fixture::new("slot-switch");
    let root = fixture.index_root("index");
    let first = build_index(request(&fixture, root.clone())).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("CURRENT")).unwrap(), "slot-a
");
    let second = build_index(request(&fixture, root.clone())).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("CURRENT")).unwrap(), "slot-b
");
    assert_eq!(first.bundle_hash, second.bundle_hash);
    assert_eq!(first.index_config, second.index_config);
}

#[test]
fn failed_rebuild_preserves_the_selected_complete_generation() {
    let fixture = P2Fixture::new("rollback");
    let root = fixture.index_root("index");
    build_index(request(&fixture, root.clone())).unwrap();
    let before = fixture.semantic_tree_hash(&root.join("slot-a"));
    fixture.block_inactive_slot(&root.join("slot-b"));
    assert_eq!(build_index(request(&fixture, root.clone())).unwrap_err().code(), "index.publish_failed");
    assert_eq!(std::fs::read_to_string(root.join("CURRENT")).unwrap(), "slot-a
");
    assert_eq!(fixture.semantic_tree_hash(&root.join("slot-a")), before);
}
```

Also test missing/malformed/traversing/symlink `CURRENT`, bundle/index overlap, `.building-*` not queryable, and `index.publish_unsupported` under a test-only platform predicate seam.

- [ ] **Step 2: Run RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test index_build publishes_first_build_to_slot_a_and_second_to_slot_b -- --exact
```

Expected: compile failure because `build_index` and `IndexRequest` do not exist.

- [ ] **Step 3: Implement path safety and two-slot publication**

Add exact public request/summary:

```rust
pub struct IndexRequest {
    pub profile_path: PathBuf,
    pub bundle_root: PathBuf,
    pub index_root: PathBuf,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct IndexSummary {
    pub documents: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub external_typed_links: usize,
    pub bundle_hash: String,
    pub index_config: String,
    pub replaced_invalid_current: bool,
}
```

`build_index` verifies the corpus before creating the index root, rejects output equal/ancestor/descendant of the canonical bundle root, rejects symlink aliases, chooses the inactive slot, builds `.building-<pid>-<counter>`, strictly reopens it, drops every handle, replaces only the inactive slot, writes/syncs `CURRENT.next`, renames it over `CURRENT`, and leaves the previous slot complete.

Use exact current parsing:

```rust
fn parse_current(bytes: &[u8]) -> Result<&'static str, IndexError> {
    match bytes {
        b"slot-a
" => Ok("slot-a"),
        b"slot-b
" => Ok("slot-b"),
        _ => Err(IndexError::CurrentInvalid),
    }
}
```

On non-macOS/non-Linux, return `PublishUnsupported` before creating a staging generation. All temporary names remain beneath the exact index root and are absent from summaries/manifests.

- [ ] **Step 4: Run GREEN and complete index tests**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test index_build
```

Expected: all publication, rollback, path, and corruption tests pass.

- [ ] **Step 5: Commit Task 5**

```bash
git add sovmem/src/profile/index.rs sovmem/tests/index_build.rs
git commit -m "P2.5: publish full rebuilds atomically [gate: pending]"
```

---

### Task 6: Implement deterministic BM25 query execution

**Files:**
- Create: `sovmem/src/profile/query.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Create: `sovmem/tests/query.rs`
- Modify: `sovmem/tests/support/p2.rs`

**Interfaces:**
- Produces `ExpansionMode`, `QueryRequest`, `QueryResult`, `QueryHit`, `ExpandedHit`, and `QueryError`.
- Produces `query_index(request: QueryRequest) -> Result<QueryResult, QueryError>`.
- Produces crate-private `search_opened_index(opened: &OpenedIndex, query: &str, top_k: usize, expansion: ExpansionMode) -> Result<QueryResult, QueryError>` for eval reuse.

- [ ] **Step 1: Write failing tokenizer, validation, and tie-break tests**

Create `sovmem/tests/query.rs`:

```rust
#[allow(dead_code)]
mod support;

use sovmem::profile::{build_index, query_index, ExpansionMode, IndexRequest, QueryRequest};
use support::p2::P2Fixture;

#[test]
fn tokenizes_case_punctuation_unicode_and_crisp_squared_deterministically() {
    assert_eq!(
        sovmem::profile::tokenize_query("CRISP², Agent-Work!"),
        vec!["crisp²", "agent", "work"]
    );
}

#[test]
fn rejects_blank_control_oversized_and_over_token_queries_without_echoing() {
    let cases = [
        ("   ", "query.empty"),
        ("private\u{0007}query", "query.invalid"),
        (&"x".repeat(4097), "query.invalid"),
        (&(0..65).map(|n| format!("t{n}")).collect::<Vec<_>>().join(" "), "query.too_many_tokens"),
    ];
    for (query, code) in cases {
        assert_eq!(sovmem::profile::validate_query(query, 5).unwrap_err().code(), code);
    }
}

#[test]
fn ranks_by_bm25_then_canonical_id_and_never_returns_bodies() {
    let fixture = P2Fixture::new("query-order");
    let index_root = fixture.index_root("index");
    build_index(IndexRequest::new(&fixture.profile_path, &fixture.bundle_root, &index_root)).unwrap();
    let result = query_index(QueryRequest {
        profile_path: fixture.profile_path.clone(),
        bundle_root: fixture.bundle_root.clone(),
        index_root,
        query: "synthetic evidence".into(),
        top_k: 5,
        expansion: ExpansionMode::None,
    }).unwrap();
    assert!(result.results.windows(2).all(|pair| {
        let left = pair[0].score.parse::<f32>().unwrap();
        let right = pair[1].score.parse::<f32>().unwrap();
        left > right || (left == right && pair[0].id < pair[1].id)
    }));
    let json = serde_json::to_string(&result).unwrap();
    assert!(!json.contains("body"));
    assert!(!json.contains("synthetic evidence"));
}
```

Add a title/tag/body fixture where each field independently matches the same term and assert scores order in the fixed `3.0 > 2.0 > 1.0` structural order. Add a dedicated equal-score fixture that asserts raw-ID ascending order, plus no-match success and `top_k` 0/63 failures. Score text is sufficient for the monotonic assertion above; the equal-score fixture proves the exact internal tie path without exposing an implementation-only score field.

- [ ] **Step 2: Run RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test query
```

Expected: compile failure because query types/functions do not exist.

- [ ] **Step 3: Implement input validation and shared analyzer tokenization**

Create `query.rs` with:

```rust
use serde::Serialize;
use std::path::PathBuf;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, BoostQuery, Occur, Query, TermQuery};
use tantivy::schema::{IndexRecordOption, Term};
use tantivy::tokenizer::{TokenStream, Tokenizer};
use thiserror::Error;
use super::{open_current_index, BODY_BOOST, TAGS_BOOST, TITLE_BOOST};

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExpansionMode { None, OneHop }

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("index could not be opened")]
    Index(#[from] super::IndexError),
    #[error("query input is invalid")]
    Invalid,
    #[error("query produced no terms")]
    Empty,
    #[error("query produced too many terms")]
    TooManyTokens,
    #[error("query execution failed")]
    Execute,
}
```

`validate_query` enforces UTF-8 `String`, no controls, 4,096 bytes, top-k 1..=62, and 1..=64 analyzed tokens. `tokenize_query` constructs the same analyzer pipeline as index registration and returns token text without logging it.

- [ ] **Step 4: Implement explicit boosted Boolean BM25 and total ordering**

Build clauses exactly:

```rust
fn term_clauses(opened: &OpenedIndex, tokens: &[String]) -> Vec<(Occur, Box<dyn Query>)> {
    let mut clauses = Vec::with_capacity(tokens.len() * 3);
    for token in tokens {
        for (field, boost) in [
            (opened.fields.title, TITLE_BOOST),
            (opened.fields.tags, TAGS_BOOST),
            (opened.fields.body, BODY_BOOST),
        ] {
            let term = Term::from_field_text(field, token);
            let query = TermQuery::new(term, IndexRecordOption::WithFreqs);
            clauses.push((Occur::Should, Box::new(BoostQuery::new(Box::new(query), boost))));
        }
    }
    clauses
}
```

Search with `TopDocs::with_limit(opened.manifest.documents)`, resolve stored metadata through the already validated `OpenedIndex` metadata map, reject non-finite scores, sort `right.score.total_cmp(&left.score).then_with(|| left.id.as_bytes().cmp(right.id.as_bytes()))`, then truncate.

Use a private `ScoredHit { score: f32, ... }` while collecting and sorting. Convert it only after ordering/truncation to the public output type, which serializes score as six-decimal text:

```rust
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct QueryHit {
    pub rank: usize,
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub core_type: String,
    pub layer: String,
    pub provenance: String,
    pub score: String,
}
```

`QueryResult` contains `status` only at the CLI layer, not library layer, and never includes raw query text/tokens.

- [ ] **Step 5: Run GREEN and locked regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test query
cargo test --locked --manifest-path sovmem/Cargo.toml --test index_build
```

Expected: tokenizer, fixed boosts, tie-break, limits, no-match, and privacy tests pass.

- [ ] **Step 6: Commit Task 6**

```bash
git add sovmem/src/profile/query.rs sovmem/src/profile/mod.rs sovmem/tests/query.rs sovmem/tests/support/p2.rs
git commit -m "P2.6: add deterministic BM25 query path [gate: pending]"
```

---

### Task 7: Join optional one-hop graph expansion without reranking

**Files:**
- Modify: `sovmem/src/profile/query.rs`
- Modify: `sovmem/src/profile/graph.rs`
- Modify: `sovmem/tests/query.rs`
- Modify: `sovmem/tests/graph_index.rs`

**Interfaces:**
- Completes `ExpansionMode::OneHop` in `search_opened_index`.
- Produces `ExpandedHit { rank, id, title, type, layer, provenance, via_id, direction, relation }`.
- Preserves identical `results` arrays between expansion `none` and `one-hop`.

- [ ] **Step 1: Write failing separation and witness tests**

Add to `query.rs` integration tests:

```rust
#[test]
fn one_hop_expansion_is_separate_and_cannot_change_bm25_top_five() {
    let fixture = P2Fixture::new_with_internal_links("expansion");
    let root = fixture.index_root("index");
    build_index(IndexRequest::new(&fixture.profile_path, &fixture.bundle_root, &root)).unwrap();
    let base = fixture.query(&root, "graph seed", ExpansionMode::None);
    let expanded = fixture.query(&root, "graph seed", ExpansionMode::OneHop);
    assert_eq!(base.results, expanded.results);
    assert!(!expanded.expanded.is_empty());
    assert!(expanded.expanded.iter().all(|hit| {
        !expanded.results.iter().any(|seed| seed.id == hit.id)
    }));
    assert_eq!(expanded.expanded[0].via_id, expanded.results[0].id);
}
```

Add graph-level tests for outgoing before incoming, lexical relation order, neighbor ID order, first-witness deduplication, and exactly one hop.

- [ ] **Step 2: Run RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test query one_hop_expansion_is_separate_and_cannot_change_bm25_top_five -- --exact
```

Expected: test fails because query output does not yet join graph witnesses.

- [ ] **Step 3: Implement metadata join without graph scores**

Add exact output type:

```rust
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExpandedHit {
    pub rank: usize,
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub core_type: String,
    pub layer: String,
    pub provenance: String,
    pub via_id: String,
    pub direction: super::Direction,
    pub relation: String,
}
```

After BM25 truncation only, call `opened.graph.expand_one_hop(&seed_ids)`, join each witness against the validated metadata map, enumerate one-based ranks, and return the separate `expanded` vector. Do not add a score, change `results`, or feed expanded IDs back into adjacency.

- [ ] **Step 4: Run GREEN and graph/query regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test graph_index
cargo test --locked --manifest-path sovmem/Cargo.toml --test query
```

Expected: expansion order is exact and BM25 arrays remain byte-equivalent across modes.

- [ ] **Step 5: Commit Task 7**

```bash
git add sovmem/src/profile/query.rs sovmem/src/profile/graph.rs sovmem/tests/query.rs sovmem/tests/graph_index.rs
git commit -m "P2.7: add deterministic one-hop expansion [gate: pending]"
```

---

### Task 8: Add `index` and `query` CLI JSON/text contracts

**Files:**
- Modify: `sovmem/src/cli.rs`
- Create: `sovmem/src/cli/p2.rs`
- Create: `sovmem/tests/p2_cli.rs`
- Modify: `sovmem/tests/support/p2.rs`

**Interfaces:**
- Adds `sovmem index` and `sovmem query` commands.
- Adds Clap `OutputFormat { Json, Text }` and `ExpansionArg { None, OneHop }`.
- Preserves one JSON object on success, sorted JSONL diagnostics on stderr, and exit classes 0/1/2.

- [ ] **Step 1: Write failing exact CLI tests**

Create `sovmem/tests/p2_cli.rs` with helpers invoking `env!("CARGO_BIN_EXE_sovmem")`. Assert exact JSON keys/order rather than unstable filesystem values:

```rust
#[test]
fn index_prints_one_compact_json_summary() {
    let fixture = P2Fixture::new("cli-index");
    let root = fixture.index_root("index");
    let output = run_index(&fixture, &root, "json");
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "ok");
    assert_eq!(value["command"], "index");
    assert_eq!(value["schema_version"], "sovmem-derived-index-v1");
    assert_eq!(value["documents"], 62);
    assert_eq!(value["graph_nodes"], 62);
    assert_eq!(value["bundle_hash"].as_str().unwrap().len(), 64);
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout.iter().filter(|byte| **byte == b'
').count(), 1);
}

#[test]
fn query_json_never_echoes_query_or_body_and_text_is_exactly_escaped() {
    let fixture = P2Fixture::new("cli-query");
    let root = fixture.index_root("index");
    assert!(run_index(&fixture, &root, "json").status.success());
    let secret_query = "private-query-sentinel";
    let json = run_query(&fixture, &root, secret_query, "none", "json");
    assert!(json.status.success());
    let combined = format!("{}{}", String::from_utf8_lossy(&json.stdout), String::from_utf8_lossy(&json.stderr));
    assert!(!combined.contains(secret_query));
    assert!(!combined.contains("body"));
    let text = run_query(&fixture, &root, "synthetic", "one-hop", "text");
    let stdout = String::from_utf8(text.stdout).unwrap();
    assert!(stdout.starts_with("status: ok
command: query
top-k: 5
expansion: one-hop
"));
    assert!(stdout.lines().all(|line| !line.contains('')));
}
```

Add table tests for `query.invalid`, `query.empty`, `query.too_many_tokens`, every `index.*` open/build code, exit `2` for usage/configuration, exit `1` for data/index corruption, no success stdout on failure, and no absolute paths/query/title/body in diagnostics.

- [ ] **Step 2: Run RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p2_cli
```

Expected: CLI rejects unknown `index`/`query` subcommands.

- [ ] **Step 3: Add Clap variants and dispatch**

In `cli.rs`:

```rust
mod p2;

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum OutputFormat { Json, Text }

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum ExpansionArg { None, OneHop }
```

Add command variants with exact paths/flags and defaults from the design. Dispatch to `p2::run_index`, `p2::run_query`; those functions return exact process exit codes. Do not modify existing command output structs or compatibility behavior.

- [ ] **Step 4: Implement redacted handlers and exact renderers**

In `cli/p2.rs`, define serializer-owned JSON structs in field order and text renderers. Map `IndexError`/`QueryError` to `VerificationDiagnostic::error(code, static_message)` without formatting the source error or input value.

Use score/title escaping in text exactly:

```rust
fn render_query_text(result: &QueryResult) {
    println!("status: ok");
    println!("command: query");
    println!("top-k: {}", result.top_k);
    println!("expansion: {}", result.expansion.as_str());
    println!("bundle-hash: {}", result.bundle_hash);
    for hit in &result.results {
        println!(
            "bm25[{}]: id={} score={} title={}",
            hit.rank,
            hit.id,
            hit.score,
            serde_json::to_string(&hit.title).expect("title JSON")
        );
    }
    for hit in &result.expanded {
        println!(
            "expanded[{}]: id={} via={} direction={} relation={} title={}",
            hit.rank, hit.id, hit.via_id, hit.direction.as_str(), hit.relation,
            serde_json::to_string(&hit.title).expect("title JSON")
        );
    }
}
```

JSON output never includes the internal raw score, raw query, tokens, paths, body, or graph slot.

- [ ] **Step 5: Run GREEN and existing CLI regression**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p2_cli
cargo test --locked --manifest-path sovmem/Cargo.toml --test cli
```

Expected: P2 contracts pass and all existing CLI outputs remain compatible.

- [ ] **Step 6: Commit Task 8**

```bash
git add sovmem/src/cli.rs sovmem/src/cli/p2.rs sovmem/tests/p2_cli.rs sovmem/tests/support/p2.rs
git commit -m "P2.8: expose index and query CLI contracts [gate: pending]"
```

---

### Task 9: Implement frozen evaluation arithmetic and deterministic baseline bytes

**Files:**
- Create: `sovmem/src/profile/eval.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Create: `sovmem/tests/eval.rs`
- Modify: `sovmem/tests/support/p2.rs`

**Interfaces:**
- Produces `EvalRequest`, `EvalOutcome`, `Q3Decision`, `EvalError`, and strict baseline structs.
- Produces `evaluate(request: EvalRequest) -> Result<EvalOutcome, EvalError>`.
- Produces crate-private `score_evaluation(records, thresholds) -> Metrics` using integer arithmetic only.
- Publishes `baseline.json` atomically after all 25 queries complete.

- [ ] **Step 1: Write failing integer boundary and baseline determinism tests**

Create `sovmem/tests/eval.rs`:

```rust
#[allow(dead_code)]
mod support;

use sovmem::profile::{evaluate_hits_for_test, Q3Decision};

#[test]
fn twenty_hits_is_go_and_nineteen_is_no_go_without_float_comparison() {
    let go = evaluate_hits_for_test(20);
    assert_eq!(go.decision, Q3Decision::Go);
    assert_eq!(go.precision_numerator, 20);
    assert_eq!(go.precision_denominator, 125);
    assert_eq!(go.recall_numerator, 20);
    assert_eq!(go.recall_denominator, 25);
    assert_eq!(go.precision_decimal, "0.1600");
    assert_eq!(go.recall_decimal, "0.8000");
    assert_eq!(evaluate_hits_for_test(19).decision, Q3Decision::NoGo);
}

#[test]
fn two_evaluations_write_identical_baseline_bytes() {
    let fixture = support::p2::P2Fixture::new("eval-determinism");
    let root = fixture.index_root("index");
    fixture.build_index(&root);
    let first = fixture.evaluate(&root, fixture.index_root("baseline-a.json"));
    let second = fixture.evaluate(&root, fixture.index_root("baseline-b.json"));
    assert_eq!(std::fs::read(first.baseline), std::fs::read(second.baseline));
}
```

Add tests for `hit_rank` one-based/null, result IDs up to five, golden order, raw golden/threshold hashes, no timestamp/path/score/query/rationale/body, prior baseline preservation on invalid index, and valid NO-GO baseline write.

- [ ] **Step 2: Run RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test eval
```

Expected: compile failure because evaluation types/functions do not exist.

- [ ] **Step 3: Implement strict baseline types and exact arithmetic**

Create `eval.rs` with deny-unknown-fields baseline structs and:

```rust
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Q3Decision { Go, NoGo }

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct MetricFraction {
    pub numerator: usize,
    pub denominator: usize,
    pub decimal: String,
}

fn metrics(hit_queries: usize) -> EvaluationMetrics {
    EvaluationMetrics {
        queries_with_hit: hit_queries,
        queries_without_hit: 25 - hit_queries,
        precision_at_5: MetricFraction {
            numerator: hit_queries,
            denominator: 125,
            decimal: format!("{:.4}", hit_queries as f64 / 125.0),
        },
        recall_at_5: MetricFraction {
            numerator: hit_queries,
            denominator: 25,
            decimal: format!("{:.4}", hit_queries as f64 / 25.0),
        },
        gate: EvaluationGate {
            minimum_queries_with_hit: 20,
            status: if hit_queries >= 20 { Q3Decision::Go } else { Q3Decision::NoGo },
        },
    }
}
```

Only `hit_queries >= thresholds.minimum_queries_with_hit()` decides the gate. Decimal calculation occurs after the decision and cannot be read by the gate.

- [ ] **Step 4: Implement one-open, 25-query evaluation and atomic write**

`evaluate` must:

1. precheck readable profile/bundle/index/golden/threshold/baseline parent;
2. call `load_retrieval_thresholds` and `validate_golden_file`;
3. parse the same frozen golden bytes through crate-private `parse_queries`;
4. call `open_current_index` once;
5. run every query in golden-file order with `top_k=thresholds.retrieval.k` and `ExpansionMode::None`;
6. store only query ID, frozen relevant ID, ordered result IDs, and one-based/null hit rank;
7. compute raw BLAKE3 of threshold and golden files;
8. serialize strict baseline pretty JSON plus one newline;
9. write/sync a sibling temporary file and rename it over the destination;
10. return GO or NO-GO with `baseline_written=true` only after publication.

Define errors/codes exactly:

```rust
#[derive(Debug, Error)]
pub enum EvalError {
    #[error("index evaluation failed")]
    Index(#[from] super::IndexError),
    #[error("query evaluation failed")]
    Query(#[from] super::QueryError),
    #[error("golden evaluation input failed")]
    Golden(#[from] super::GoldenError),
    #[error("threshold evaluation input failed")]
    Threshold(#[from] super::ThresholdError),
    #[error("baseline output is invalid")]
    BaselineOutputInvalid,
    #[error("baseline write failed")]
    Write,
}
```

Map `BaselineOutputInvalid` to `eval.baseline_output_invalid` and `Write` to `eval.write_failed`; preserve underlying `golden.*`, `thresholds.*`, `index.*`, and `query.*` codes.

- [ ] **Step 5: Run GREEN and threshold/golden regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test eval
cargo test --locked --manifest-path sovmem/Cargo.toml --test thresholds
cargo test --locked --manifest-path sovmem/Cargo.toml --test golden_queries
```

Expected: arithmetic boundaries, deterministic bytes, frozen identity, and prior-file preservation pass.

- [ ] **Step 6: Commit Task 9**

```bash
git add sovmem/src/profile/eval.rs sovmem/src/profile/mod.rs sovmem/tests/eval.rs sovmem/tests/support/p2.rs
git commit -m "P2.9: add frozen Q3 evaluation baseline [gate: pending]"
```

---

### Task 10: Add `eval` CLI, canonical root commands, and P2 usage documentation

**Files:**
- Modify: `sovmem/src/cli.rs`
- Modify: `sovmem/src/cli/p2.rs`
- Modify: `sovmem/tests/p2_cli.rs`
- Modify: `package.json`
- Modify: `test/retrieval-corpus-version.test.js`
- Modify: `README.md`
- Modify: `index.md`

**Interfaces:**
- Adds `sovmem eval` with no retrieval/threshold overrides.
- Adds `npm run index:audy`, `npm run query:audy`, and `npm run eval:audy` using exact derived paths.
- Defines GO exit `0`, valid NO-GO exit `1` plus `eval.q3_no_go`, and no success output for invalid inputs.

- [ ] **Step 1: Write failing eval CLI and root-command tests**

Add CLI tests:

```rust
#[test]
fn eval_go_prints_integer_summary_and_writes_baseline() {
    let fixture = P2Fixture::new_retrievable("cli-eval-go", 25);
    let root = fixture.index_root("index");
    assert!(run_index(&fixture, &root, "json").status.success());
    let baseline = fixture.index_root("baseline.json");
    let output = run_eval(&fixture, &root, &baseline, "json");
    assert_eq!(output.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "go");
    assert_eq!(value["queries"], 25);
    assert_eq!(value["k"], 5);
    assert_eq!(value["required_queries_with_hit"], 20);
    assert_eq!(value["baseline_written"], true);
    assert!(baseline.is_file());
}

#[test]
fn valid_no_go_writes_evidence_exits_one_and_emits_stable_code() {
    let fixture = P2Fixture::new_retrievable("cli-eval-no-go", 19);
    let root = fixture.index_root("index");
    fixture.build_index(&root);
    let baseline = fixture.index_root("baseline.json");
    let output = run_eval(&fixture, &root, &baseline, "json");
    assert_eq!(output.status.code(), Some(1));
    assert!(baseline.is_file());
    assert!(String::from_utf8_lossy(&output.stderr).contains("eval.q3_no_go"));
}
```

Extend Node contract tests:

```js
test('exposes exact offline P2 root commands', () => {
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
  assert.match(pkg.scripts['index:audy'], /--bundle var\/audy-sovmem-retrieval-corpus --index var\/audy-sovmem-derived-index/);
  assert.match(pkg.scripts['query:audy'], / query .*--bundle var\/audy-sovmem-retrieval-corpus .*--index var\/audy-sovmem-derived-index/);
  assert.match(pkg.scripts['eval:audy'], /--golden evals\/audy-sovmem-v0\/golden\.jsonl/);
  assert.match(pkg.scripts['eval:audy'], /--thresholds evals\/audy-sovmem-v0\/thresholds\.json/);
  assert.match(pkg.scripts['eval:audy'], /--baseline evals\/audy-sovmem-v0\/baseline\.json/);
});
```

- [ ] **Step 2: Run RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p2_cli eval_go_prints_integer_summary_and_writes_baseline -- --exact
node --test test/retrieval-corpus-version.test.js
```

Expected: eval subcommand/root scripts are absent.

- [ ] **Step 3: Add exact eval command and handler**

Add `Command::Eval` with required `profile`, `bundle`, `index`, `golden`, `thresholds`, `baseline`, and defaulted `format`. Do not add top-k, expansion, boost, tokenizer, query selection, label, or threshold flags.

The handler prints one exact JSON summary or this text form plus deterministic missed IDs:

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
missed-query-ids: q-003,q-017
```

For zero misses print `missed-query-ids: none`. On NO-GO print the same complete result, emit one `eval.q3_no_go` diagnostic, and return `1`.

- [ ] **Step 4: Add exact package scripts and documentation**

Add scripts using `cargo run --locked --manifest-path sovmem/Cargo.toml --` and the exact paths from the design. `query:audy` ends after fixed profile/bundle/index arguments so `npm run query:audy -- --query "text"` appends query flags.

Update `README.md` with:

- bundle remains authority; derived index is disposable;
- exact index/query/eval commands and JSON/text formats;
- BM25 fields/boosts and separate one-hop expansion;
- no embeddings/model/network calls;
- frozen Q3 behavior and stop-on-NO-GO;
- no P3/P4/P5 claim.

Update root `index.md` artifact registry with the approved spec, this plan, four P2 modules, six P2 test/helper files, and baseline path marked pending until canonical evaluation.

- [ ] **Step 5: Run GREEN and documentation contracts**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p2_cli
node --test test/retrieval-corpus-version.test.js
npm run audit:sovmem:check
```

Expected: P2 CLI/root contracts pass; pre-GO audit remains exactly current at P2 10%.

- [ ] **Step 6: Commit Task 10**

```bash
git add sovmem/src/cli.rs sovmem/src/cli/p2.rs sovmem/tests/p2_cli.rs package.json test/retrieval-corpus-version.test.js README.md index.md
git commit -m "P2.10: wire evaluation and root commands [gate: pending]"
```

---

### Task 11: Prove corruption rejection and semantic rebuild determinism

**Files:**
- Create: `sovmem/tests/p2_determinism.rs`
- Modify: `sovmem/tests/index_build.rs`
- Modify: `sovmem/tests/query.rs`
- Modify: `sovmem/tests/eval.rs`
- Modify: `sovmem/tests/p2_cli.rs`
- Modify: `sovmem/tests/support/p2.rs`

**Interfaces:**
- Produces metadata-only `SemanticSnapshot { manifest_bytes, graph_bytes, counts, bm25_ids, expansion_witnesses, baseline_bytes }`.
- Proves two independent roots, same-root slot switch, and delete/rebuild equivalence.
- Completes formal P2 corruption/privacy/offline coverage before canonical evaluation.

- [ ] **Step 1: Write the complete failing determinism test**

Create `sovmem/tests/p2_determinism.rs`:

```rust
#[allow(dead_code)]
mod support;

use support::p2::P2Fixture;

#[test]
fn two_clean_builds_and_deleted_rebuild_are_semantically_identical() {
    let fixture = P2Fixture::new_frozen_eval("two-build-proof");
    let root_a = fixture.index_root("index-a");
    let root_b = fixture.index_root("index-b");

    fixture.build_index(&root_a);
    let first_a = fixture.semantic_snapshot(&root_a);

    fixture.build_index(&root_b);
    let first_b = fixture.semantic_snapshot(&root_b);
    assert_eq!(first_a, first_b);

    std::fs::remove_dir_all(&root_a).expect("delete derived index A");
    assert!(!root_a.exists());
    fixture.build_index(&root_a);
    let rebuilt_a = fixture.semantic_snapshot(&root_a);
    assert_eq!(first_a, rebuilt_a);
}

#[test]
fn same_root_slot_switch_preserves_every_semantic_output() {
    let fixture = P2Fixture::new_frozen_eval("slot-proof");
    let root = fixture.index_root("index");
    fixture.build_index(&root);
    let before = fixture.semantic_snapshot(&root);
    assert_eq!(std::fs::read_to_string(root.join("CURRENT")).unwrap(), "slot-a
");
    fixture.build_index(&root);
    assert_eq!(std::fs::read_to_string(root.join("CURRENT")).unwrap(), "slot-b
");
    assert_eq!(before, fixture.semantic_snapshot(&root));
}
```

`semantic_snapshot` reads the selected slot's exact manifest/graph bytes, runs all 25 golden queries with `ExpansionMode::OneHop`, stores only ordered BM25 IDs and expansion `(id,via,direction,relation)`, evaluates to a temporary baseline, and excludes slot/path/score/timing state.

- [ ] **Step 2: Run the determinism test and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p2_determinism -- --nocapture
```

Expected: test fails until snapshot helpers and any remaining deterministic ordering gaps are complete.

- [ ] **Step 3: Complete corruption matrix before fixing determinism**

Add exact table-driven cases across focused test files:

| Mutation | Required code | Prior current/baseline |
| --- | --- | --- |
| Missing `corpus-report.json` | `index.bundle_unverified` | unchanged |
| Changed generated Markdown bytes | underlying `bundle.document_hash_mismatch` plus `index.bundle_unverified` | unchanged |
| Repaired document hash but stale bundle root | `bundle.root_hash_mismatch` plus `index.bundle_unverified` | unchanged |
| Invalid provenance | `document.provenance_invalid` plus `index.bundle_unverified` | unchanged |
| Injected body-only link | no graph edge | valid |
| Duplicate graph edge | `index.graph_invalid` | query blocked |
| Injected valid-ID graph edge absent from bundle | `index.graph_invalid` | query blocked |
| Unknown manifest field | `index.manifest_invalid` | query blocked |
| Changed bundle/config/version binding | `index.bundle_mismatch` or `index.schema_mismatch` | query blocked |
| Missing Tantivy file | `index.tantivy_invalid` | query blocked |
| Traversing/symlink `CURRENT` | `index.current_invalid` | query blocked |
| Failed inactive-slot publication | `index.publish_failed` | old current unchanged |
| Changed golden byte | `thresholds.golden_hash` | baseline unchanged |
| Changed threshold field | `thresholds.*` | baseline unchanged |
| Valid 19-hit evaluation | `eval.q3_no_go` | new NO-GO baseline written |

Every CLI failure test concatenates stdout/stderr and asserts absence of query/body/title sentinels, absolute fixture paths, source paths, and Tantivy filenames.

- [ ] **Step 4: Make the minimal deterministic fixes**

Fix only unstable ordering/serialization revealed by the tests. Approved tools are `BTreeMap`, `BTreeSet`, explicit vector sort by raw UTF-8 tuples, strict structs, one writer thread, all-match collection, total float ordering, and pretty JSON plus one newline. Do not change boosts, tokenizer, expansion semantics, golden data, labels, or thresholds.

- [ ] **Step 5: Run the complete P2 focused matrix**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test index_build
cargo test --locked --manifest-path sovmem/Cargo.toml --test graph_index
cargo test --locked --manifest-path sovmem/Cargo.toml --test query
cargo test --locked --manifest-path sovmem/Cargo.toml --test eval
cargo test --locked --manifest-path sovmem/Cargo.toml --test p2_cli
cargo test --locked --manifest-path sovmem/Cargo.toml --test p2_determinism -- --nocapture
```

Expected: every test passes. This is the last allowed retrieval-behavior change before canonical Q3 evaluation.

- [ ] **Step 6: Run full preflight and commit Task 11**

```bash
npm test
npm run audit:sovmem:check
git diff --check
```

Expected: full suite passes; audit remains current at pre-GO P2 10%; no whitespace errors.

```bash
git add sovmem/tests/p2_determinism.rs sovmem/tests/index_build.rs sovmem/tests/query.rs sovmem/tests/eval.rs sovmem/tests/p2_cli.rs sovmem/tests/support/p2.rs
git commit -m "P2.11: prove rebuild determinism and corruption gates [gate: pending]"
```

---

### Task 12: Run the one-time canonical two-build and frozen Q3 gate

**Files:**
- Create on GO or NO-GO: `evals/audy-sovmem-v0/baseline.json`
- No production-code modification is permitted in this task.
- No golden, threshold, corpus, profile, or canonical bundle modification is permitted.

**Interfaces:**
- Consumes the ignored verified bundle at `var/audy-sovmem-retrieval-corpus/` or an explicitly supplied equivalent Level 3 bundle.
- Produces exact canonical baseline bytes and two-build/deleted-rebuild evidence.
- Branches only on `queries_with_hit >= 20`.

- [ ] **Step 1: Prove frozen inputs and clean implementation head**

Run:

```bash
git status --short
git diff --quiet origin/main -- evals/audy-sovmem-v0/golden.jsonl evals/audy-sovmem-v0/thresholds.json profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml
npm run verify:corpus:audy
npm run verify:golden:audy
npm test
```

Expected: clean tree; frozen files unchanged; Level 3 reports 62 documents, 226 warnings, zero errors, and bundle hash `49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65`; all tests pass.

- [ ] **Step 2: Build canonical root A and evaluate baseline A**

Use clean ignored roots:

```bash
cargo run --locked --manifest-path sovmem/Cargo.toml -- index --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus --index var/p2-proof/index-a --format json
cargo run --locked --manifest-path sovmem/Cargo.toml -- eval --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus --index var/p2-proof/index-a --golden evals/audy-sovmem-v0/golden.jsonl --thresholds evals/audy-sovmem-v0/thresholds.json --baseline var/p2-proof/baseline-a.json --format json
```

Expected: index reports 62 documents, 62 graph nodes, 19 graph edges, 81 external typed links, and the canonical bundle hash. Record the eval JSON exactly without interpreting rounded decimals.

- [ ] **Step 3: Build canonical root B and compare semantic outputs**

```bash
cargo run --locked --manifest-path sovmem/Cargo.toml -- index --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus --index var/p2-proof/index-b --format json
cargo run --locked --manifest-path sovmem/Cargo.toml -- eval --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus --index var/p2-proof/index-b --golden evals/audy-sovmem-v0/golden.jsonl --thresholds evals/audy-sovmem-v0/thresholds.json --baseline var/p2-proof/baseline-b.json --format json
cmp var/p2-proof/baseline-a.json var/p2-proof/baseline-b.json
cmp var/p2-proof/index-a/slot-a/index-manifest.json var/p2-proof/index-b/slot-a/index-manifest.json
cmp var/p2-proof/index-a/slot-a/graph.json var/p2-proof/index-b/slot-a/graph.json
```

Run the already passing `p2_determinism` semantic snapshot helper against both canonical roots so all 25 BM25 top-five arrays and all 25 expansion arrays compare exactly. Do not compare private Tantivy segment bytes.

- [ ] **Step 4: Prove deleted-index rebuild equivalence**

Delete only derived `var/p2-proof/index-a`, rebuild it from the unchanged bundle, rerun eval to `baseline-rebuilt-a.json`, and require exact equality with `baseline-a.json`, manifest, graph, BM25 IDs, and expansion witnesses. Obtain explicit command approval before the destructive deletion if the execution environment requires it.

- [ ] **Step 5: Run the canonical eval to the tracked baseline path exactly once**

```bash
npm run index:audy
npm run eval:audy
```

Read the compact JSON and branch only on `status` and integer `queries_with_hit`.

**If `status` is `no-go` or `queries_with_hit < 20`:**

- confirm `evals/audy-sovmem-v0/baseline.json` contains the actual NO-GO result;
- do not modify production code, golden queries, labels, fixtures, thresholds, boosts, tokenizer, or graph behavior;
- do not run Task 13;
- stop and present D2 with the ordered per-query IDs/misses and the exact integer hit count for human decision.

**If `status` is `go` and `queries_with_hit >= 20`:** continue to Task 13 without changing retrieval behavior.

---

### Task 13: Publish P2 GO audit state and run final verification

**Conditional:** Run this task only after Task 12 returns GO.

**Files:**
- Modify: `scripts/lib/sovmem-build-audit.mjs`
- Modify: `test/sovmem-build-audit.test.js`
- Modify: `test/retrieval-corpus-version.test.js`
- Modify: `docs/sovmem-build-state.json`
- Regenerate: `docs/SOVMEM-BUILD-AUDIT.md`
- Modify: `docs/sovmem_v0.3_execution_plan.md`
- Modify: `README.md`
- Modify: `index.md`
- Track: `evals/audy-sovmem-v0/baseline.json`

**Interfaces:**
- Advances audit schema to `sovmem-build-audit-v3` with exact P2 retrieval evidence.
- Sets P2 completion 100%, gate `passed`, overall completion 37%.
- Records actual test counts, canonical hit count, index/graph counts, hashes, and implementation commit.

- [ ] **Step 1: Write failing GO audit tests before state changes**

Extend `test/sovmem-build-audit.test.js` to require these new evidence keys:

```js
assert.deepEqual(Object.keys(state.evidence.retrieval).sort(), [
  'baseline_blake3',
  'bundle_hash',
  'documents',
  'external_typed_links',
  'graph_edges',
  'graph_nodes',
  'index_config',
  'k',
  'minimum_queries_with_hit',
  'queries',
  'queries_with_hit',
].sort());
assert.equal(state.phases.find(phase => phase.id === 'P2').completion_percent, 100);
assert.equal(state.phases.find(phase => phase.id === 'P2').gate, 'passed');
assert.equal(calculateOverallCompletion(state), 37);
```

Add mutation tests rejecting `queries_with_hit < 20`, counts other than 62/62/19/81 for the current canonical baseline, mismatched bundle hash, missing tracked baseline evidence, malformed BLAKE3/config hashes, and claims of enforced GitHub controls.

Extend `test/retrieval-corpus-version.test.js` to parse the committed baseline, require schema `sovmem-retrieval-baseline-v1`, exact frozen hashes, 25 query records, `k=5`, `gate.status="go"`, and `queries_with_hit >= 20` without hard-coding a rounded metric.

- [ ] **Step 2: Run audit tests and verify RED**

```bash
node --test test/sovmem-build-audit.test.js test/retrieval-corpus-version.test.js
```

Expected: tests fail because audit schema/state still describes P2 10% in progress.

- [ ] **Step 3: Update audit validator and reviewed state from actual evidence**

Add strict retrieval evidence validation, include `evals/audy-sovmem-v0/baseline.json` in the fingerprint, and render exact Q3 evidence in the audit. Update state:

- `schema_version`: `sovmem-build-audit-v3`;
- P2 `completion_percent`: `100`;
- P2 `gate`: `passed`;
- P2 completed list: verified full rebuild, deterministic BM25, separate one-hop expansion, deterministic baseline, two-build/deleted-rebuild proof, and integer Q3 GO;
- P2 remaining list: empty;
- evidence values from actual manifest/baseline/test output only;
- history entry dated `2026-07-13` with implementation commit;
- next step: P3 design/review, not implementation claim.

Do not predict or invent final test counts. Run `npm test`, then insert the actual JavaScript and Rust pass counts into state.

- [ ] **Step 4: Update normative/readme/index status without scope creep**

Update the execution plan status and P2 heading from “not implemented” to the exact passed behavior. Preserve D2, no embeddings, full rebuild only, typed frontmatter links, one crate, and every P3-P5 future statement.

Update README/root index to mark P2 delivered and P3 unimplemented. Preserve manual PR discipline and avoid claims of branch protection/rulesets/check enforcement.

- [ ] **Step 5: Regenerate audit and run complete final verification**

```bash
npm run audit:sovmem
npm run audit:sovmem:check
npm test
cargo fmt --manifest-path sovmem/Cargo.toml -- --check
cargo clippy --locked --manifest-path sovmem/Cargo.toml --all-targets -- -D warnings
git diff --check
```

Expected:

- audit check says current;
- all JavaScript and Rust tests pass with zero failures;
- rustfmt and clippy pass;
- generated audit reports P2 100% and overall 37%;
- no whitespace errors.

- [ ] **Step 6: Verify exact final scope and frozen bytes**

```bash
git diff --name-only origin/main
git diff --quiet origin/main -- evals/audy-sovmem-v0/golden.jsonl evals/audy-sovmem-v0/thresholds.json profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml
git status --short
```

Expected: only approved P2 implementation/plan/design/docs/audit/baseline files differ; frozen golden, threshold, and corpus manifest bytes are unchanged; no generated `var/` or `sovmem/target/` file is tracked.

- [ ] **Step 7: Commit P2 GO gate closure**

```bash
git add sovmem/Cargo.toml sovmem/Cargo.lock sovmem/src/profile/index.rs sovmem/src/profile/graph.rs sovmem/src/profile/query.rs sovmem/src/profile/eval.rs sovmem/src/profile/mod.rs sovmem/src/cli.rs sovmem/src/cli/p2.rs sovmem/tests/support/mod.rs sovmem/tests/support/p2.rs sovmem/tests/index_build.rs sovmem/tests/graph_index.rs sovmem/tests/query.rs sovmem/tests/eval.rs sovmem/tests/p2_cli.rs sovmem/tests/p2_determinism.rs evals/audy-sovmem-v0/baseline.json package.json test/retrieval-corpus-version.test.js scripts/lib/sovmem-build-audit.mjs test/sovmem-build-audit.test.js docs/sovmem-build-state.json docs/SOVMEM-BUILD-AUDIT.md docs/sovmem_v0.3_execution_plan.md README.md index.md
git commit -m "P2.13: close derived index and query gate [gate: pass]"
```

- [ ] **Step 8: Stop for branch review and manual PR handling**

Report:

- exact head commit;
- exact JavaScript/Rust test counts;
- canonical bundle/index/graph counts;
- integer Q3 hit count and GO status;
- baseline BLAKE3;
- audit check/rustfmt/clippy results;
- branch remains local until the repository owner chooses publish/PR handling.

Do not claim automatic branch protection, required checks, rulesets, merge queue, or merge.

---

## Plan Exit Gate

This plan is ready for implementation only after repository-owner review and approval. Approval authorizes Task 1, not an implementation shortcut: each task still requires its own RED/GREEN evidence and commit.

Execution must stop at either terminal:

- **Q3 NO-GO:** valid baseline written, D2 presented, no Task 13, no tuning, no P3.
- **Q3 GO:** Task 13 completes with P2 100%, overall 37%, fresh full verification, and manual branch review requested.
