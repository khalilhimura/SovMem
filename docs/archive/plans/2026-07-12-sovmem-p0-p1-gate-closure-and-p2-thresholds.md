> **ARCHIVED** — This implementation plan has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix A: Implementation Plan Index](../sovmem-master-spec.md#appendix-a-implementation-plan-index) for a summary of its scope.
>
# SovMem P0/P1 Gate Closure and P2 Threshold Freeze Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close SovMem P0 and P1 with explicit, compatible verification levels, deterministic bundle statistics, stable corruption diagnostics, pinned ground-truth references, and a frozen integer-gated P2 evaluation contract without implementing retrieval.

**Architecture:** Keep the single Rust crate and extract the current offline bundle verifier from the CLI into a focused profile module before adding behavior. A strict threshold loader binds the immutable golden JSONL bytes to the approved Q3 contract, cumulative verification options gate structure, integrity, and corpus correspondence, and a stats module summarizes only verified evidence. Repository-level contract tests pin external references and immutable evaluation bytes; the reviewed audit schema then publishes P0/P1 closure and the unchanged P2 implementation boundary.

**Tech Stack:** Rust 1.84.0; existing `clap`, `serde`, `serde_json`, `serde_yaml`, `blake3`, `anyhow`, and `thiserror`; Node.js 20 ESM and standard library; `node:test`; Git CLI; Markdown and JSON.

## Global Constraints

- The approved design is `docs/superpowers/specs/2026-07-12-sovmem-p0-p1-gate-closure-and-p2-thresholds-design.md` at commit `f333edf`.
- `docs/sovmem_v0.3_execution_plan.md` remains the normative build authority after its approved decisions are recorded.
- Work only in `.worktrees/sovmem-p0-p1-gate-closure` on `codex/sovmem-p0-p1-gate-closure`.
- Keep one Rust crate and add no dependency.
- Audy Vault remains canonical, read-only, and pinned at `43824cfd9caed7b260aa442128fd1130ddb6e2ff`; generated bundles remain ignored derived output.
- The accepted corpus is exactly 62 documents: 8 canonical core and 54 research, with 1 of 55 research candidates excluded by verified evidence.
- The golden set is exactly 25 human-approved queries with exactly one relevant ID per query.
- Preserve `ACCEPT|REJECT|REWRITE|TP|FP` internally and `ACCEPT|REJECT|REWRITE` as the public pedagogical subset.
- Preserve provenance as `tier1|secondary|synthesis|unknown`.
- Preserve bare `verify` and legacy `--require-corpus` behavior and successful JSON field sets.
- Explicit Level 3 is the formal P1 corpus gate; deterministic source/body-link warnings remain warnings, while forged dangling typed links fail.
- Freeze `k=5`, Macro Precision@5 minimum `0.16`, Macro Recall@5 minimum `0.80`, and the integer gate `minimum_queries_with_hit=20`.
- Neither `evals/audy-sovmem-v0/golden.jsonl` nor `evals/audy-sovmem-v0/thresholds.json` may change during P2.
- Do not add Tantivy, Petgraph, BM25, indexing, graph traversal, query execution, ranking, `eval`, or `baseline.json`.
- Do not implement P3-P5 behavior, write to the live vault, call a model, or add runtime network access.
- Keep diagnostics deterministic and redacted: no bodies, query text, absolute private paths, environment values, or malformed payload echoes.
- The repository remains private with manual pull-request merge discipline. Do not claim branch protection, required checks, rulesets, or merge queues are enforced.
- Every production task follows RED → GREEN → focused regression → commit. Do not combine task commits.

---

## File structure

| File | Responsibility |
| --- | --- |
| `docs/spec/okf-reference.json` | Exact official OKF repository, commit, normative file, and reference schema pin. |
| `evals/audy-sovmem-v0/thresholds.json` | Human-approved Q3 threshold and evaluation-set freeze. |
| `sovmem/src/profile/thresholds.rs` | Strict threshold deserialization, golden-byte binding, integer-gate validation, and stable errors. |
| `sovmem/src/profile/verify.rs` | Extracted offline bundle parsing, level policy, integrity checks, semantic correspondence, and verified evidence access. |
| `sovmem/src/profile/stats.rs` | Deterministic distributions and counts derived from a successful bundle verification. |
| `sovmem/src/profile/golden.rs` | Reuse strict golden parsing and human-authorship checks from threshold validation; consume verified corpus evidence from `profile::verify`. |
| `sovmem/src/profile/mod.rs` | Register `thresholds`, `verify`, and `stats`; continue exporting focused profile interfaces. |
| `sovmem/src/cli.rs` | Keep command orchestration and redacted JSON I/O; add `--level` and `stats` without retaining verifier internals. |
| `sovmem/tests/thresholds.rs` | Exact Q3 schema, byte hash, golden identity, approval, integer arithmetic, and mutation failures. |
| `sovmem/tests/bundle_verification.rs` | Direct module-level compatibility and level-boundary tests. |
| `sovmem/tests/cli.rs` | CLI compatibility, explicit level output, five corruption codes, stats, privacy, and exit classes. |
| `test/retrieval-corpus-version.test.js` | Exact OKF pin, immutable golden/threshold byte digests, package commands, and forbidden dependency/scope checks. |
| `package.json` | Formal Level 3 corpus command and deterministic Audy stats command. |
| `docs/sovmem_v0.3_execution_plan.md` | Resolved P0 decisions, Level 1-3 contract, Q3 freeze, manual merge discipline, and remaining P2 work. |
| `README.md` | Supported commands, formal gates, warning semantics, and explicit non-retrieval status. |
| `index.md` | Register the design, plan, pins, modules, tests, and revised phase boundary. |
| `scripts/lib/sovmem-build-audit.mjs` | Validate and render gate-closure evidence under audit schema v2. |
| `test/sovmem-build-audit.test.js` | Audit v2 schema, 24% weighting, evidence, registry, and manual-discipline wording. |
| `docs/sovmem-build-state.json` | Reviewed P0=100, P1=100, P2=10 state and exact gate evidence. |
| `docs/SOVMEM-BUILD-AUDIT.md` | Regenerated 24% human-readable audit. |
| `docs/superpowers/specs/2026-07-12-sovmem-build-audit-design.md` | Transparent supersession note for audit v2 and manual merge discipline. |

---

### Task 1: Pin OKF and freeze the Q3 evaluation contract

**Files:**
- Create: `docs/spec/okf-reference.json`
- Create: `evals/audy-sovmem-v0/thresholds.json`
- Create: `sovmem/src/profile/thresholds.rs`
- Create: `sovmem/tests/thresholds.rs`
- Modify: `sovmem/src/profile/golden.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `test/retrieval-corpus-version.test.js`

**Interfaces:**
- Produces: `load_retrieval_thresholds(thresholds_path: &Path, golden_path: &Path) -> Result<RetrievalThresholds, ThresholdError>`.
- Produces: `RetrievalThresholds::minimum_queries_with_hit() -> usize` and `RetrievalThresholds::golden_blake3() -> &str`.
- Produces stable codes `thresholds.read`, `thresholds.parse`, `thresholds.schema`, `thresholds.evaluation_set`, `thresholds.golden_hash`, `thresholds.golden_contract`, `thresholds.retrieval`, and `thresholds.approval`.
- Consumes crate-private `parse_queries(bytes: &[u8])` and `validate_query_fields(queries: &[GoldenQuery])` from `profile::golden`.

- [ ] **Step 1: Write the failing repository pin tests**

Extend `test/retrieval-corpus-version.test.js` with exact pin and immutable-byte tests:

```js
test('pins the official Open Knowledge Format reference', () => {
  const path = 'docs/spec/okf-reference.json';
  const bytes = readFileSync(path, 'utf8');
  const expected = {
    schema_version: 'sovmem-okf-reference-v1',
    repository: 'GoogleCloudPlatform/knowledge-catalog',
    commit: 'd44368c15e38e7c92481c5992e4f9b5b421a801d',
    normative_file: 'okf/SPEC.md',
  };
  assert.deepEqual(JSON.parse(bytes), expected);
  assert.equal(bytes, `${JSON.stringify(expected, null, 2)}
`);
});

test('locks the approved golden and threshold bytes independently', () => {
  const golden = createHash('sha256')
    .update(readFileSync('evals/audy-sovmem-v0/golden.jsonl'))
    .digest('hex');
  const thresholds = createHash('sha256')
    .update(readFileSync('evals/audy-sovmem-v0/thresholds.json'))
    .digest('hex');
  assert.equal(golden, '698e56224d8c5358948557f8e4c6965eeeab7cff76afbdaf382f2faa3091af7d');
  assert.equal(thresholds, '33038018a4ca9fecaa960839f7bfb9e277f5306ac657a6ef30df62dbfa57f280');
});
```

- [ ] **Step 2: Write the failing Rust threshold tests**

Create `sovmem/tests/thresholds.rs` with an approved-load test and mutation table:

```rust
use serde_json::{json, Value};
use sovmem::profile::load_retrieval_thresholds;
use std::fs;
use std::path::{Path, PathBuf};

fn repository_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(relative)
}

#[test]
fn loads_the_human_approved_q3_freeze() {
    let thresholds = load_retrieval_thresholds(
        &repository_path("evals/audy-sovmem-v0/thresholds.json"),
        &repository_path("evals/audy-sovmem-v0/golden.jsonl"),
    ).expect("approved thresholds load");
    assert_eq!(thresholds.evaluation_set.id, "audy-sovmem-v0");
    assert_eq!(thresholds.evaluation_set.query_count, 25);
    assert_eq!(thresholds.evaluation_set.relevant_documents_per_query, 1);
    assert_eq!(thresholds.retrieval.k, 5);
    assert_eq!(thresholds.retrieval.macro_precision_at_k_minimum, "0.16");
    assert_eq!(thresholds.retrieval.macro_recall_at_k_minimum, "0.80");
    assert_eq!(thresholds.minimum_queries_with_hit(), 20);
    assert_eq!(thresholds.approval.freeze_date, "2026-07-12");
}
```

Add a `TemporaryThresholds` fixture that copies the approved JSON and golden bytes into a temporary directory, mutates one public field, and asserts these exact cases:

```rust
struct TemporaryThresholds {
    root: PathBuf,
    thresholds: PathBuf,
    golden: PathBuf,
}

impl TemporaryThresholds {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "sovmem-thresholds-{name}-{}",
            std::process::id(),
        ));
        fs::create_dir(&root).expect("create threshold area");
        let thresholds = root.join("thresholds.json");
        let golden = root.join("golden.jsonl");
        fs::copy(repository_path("evals/audy-sovmem-v0/thresholds.json"), &thresholds)
            .expect("copy thresholds");
        fs::copy(repository_path("evals/audy-sovmem-v0/golden.jsonl"), &golden)
            .expect("copy golden");
        Self { root, thresholds, golden }
    }

    fn error(&self) -> &'static str {
        load_retrieval_thresholds(&self.thresholds, &self.golden)
            .expect_err("fixture must fail")
            .code()
    }
}

impl Drop for TemporaryThresholds {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
```

For JSON mutations, deserialize the copied threshold file into `Value`, apply the case function, and write `serde_json::to_vec_pretty` plus one newline before calling `error()`.

```rust
let cases: &[(&str, fn(&mut Value), &str)] = &[
    ("schema", |v| v["schema_version"] = json!("foreign-v1"), "thresholds.schema"),
    ("identity", |v| v["evaluation_set"]["id"] = json!("foreign"), "thresholds.evaluation_set"),
    ("count", |v| v["evaluation_set"]["query_count"] = json!(24), "thresholds.evaluation_set"),
    ("k", |v| v["retrieval"]["k"] = json!(10), "thresholds.retrieval"),
    ("precision", |v| v["retrieval"]["macro_precision_at_k_minimum"] = json!("0.15"), "thresholds.retrieval"),
    ("recall", |v| v["retrieval"]["macro_recall_at_k_minimum"] = json!("0.79"), "thresholds.retrieval"),
    ("hits", |v| v["retrieval"]["minimum_queries_with_hit"] = json!(19), "thresholds.retrieval"),
    ("approval", |v| v["approval"]["human_approved"] = json!(false), "thresholds.approval"),
];
```

Also test unknown-field rejection as `thresholds.parse`, one non-human golden query as `thresholds.golden_contract`, two relevant IDs as `thresholds.golden_contract`, and one changed golden byte as `thresholds.golden_hash`.

- [ ] **Step 3: Run the focused tests and verify RED**

Run:

```bash
node --test test/retrieval-corpus-version.test.js
cargo test --manifest-path sovmem/Cargo.toml --test thresholds
```

Expected: Node fails because both new JSON artifacts are absent; Rust fails because `load_retrieval_thresholds` and `thresholds.rs` do not exist.

- [ ] **Step 4: Add the exact tracked JSON artifacts**

Create `docs/spec/okf-reference.json` exactly as:

```json
{
  "schema_version": "sovmem-okf-reference-v1",
  "repository": "GoogleCloudPlatform/knowledge-catalog",
  "commit": "d44368c15e38e7c92481c5992e4f9b5b421a801d",
  "normative_file": "okf/SPEC.md"
}
```

Create `evals/audy-sovmem-v0/thresholds.json` exactly as:

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

- [ ] **Step 5: Implement strict threshold loading**

In `sovmem/src/profile/golden.rs`, change only the visibility of the existing `parse_queries` and `validate_query_fields` functions from private `fn` to `pub(crate) fn`. Leave both parameter lists, return types, and function bodies unchanged.

Create `sovmem/src/profile/thresholds.rs` with deny-unknown-fields structs and this validation flow:

```rust
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use thiserror::Error;
use super::{parse_queries, validate_query_fields};

pub const THRESHOLDS_SCHEMA: &str = "sovmem-retrieval-thresholds-v1";
pub const EVALUATION_SET_ID: &str = "audy-sovmem-v0";
pub const GOLDEN_FILE: &str = "evals/audy-sovmem-v0/golden.jsonl";
pub const GOLDEN_BLAKE3: &str = "f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RetrievalThresholds {
    pub schema_version: String,
    pub evaluation_set: EvaluationSet,
    pub retrieval: RetrievalGate,
    pub approval: ThresholdApproval,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvaluationSet {
    pub id: String,
    pub golden_file: String,
    pub golden_blake3: String,
    pub query_count: usize,
    pub relevant_documents_per_query: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RetrievalGate {
    pub k: usize,
    pub macro_precision_at_k_minimum: String,
    pub macro_recall_at_k_minimum: String,
    pub minimum_queries_with_hit: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ThresholdApproval {
    pub human_approved: bool,
    pub authority: String,
    pub freeze_date: String,
    pub frozen_through_phase: String,
}
```

Implement `ThresholdError::code()` with the exact codes from the Interfaces block. `load_retrieval_thresholds` must read and strictly deserialize thresholds, validate every literal against the approved values, read golden bytes, compare raw-byte BLAKE3, call the existing golden parser/field validator, require 25 queries with one relevant ID each, and verify the integer identities:

```rust
let precision_passes = thresholds.retrieval.minimum_queries_with_hit * 100
    >= 16 * thresholds.evaluation_set.query_count * thresholds.retrieval.k;
let recall_passes = thresholds.retrieval.minimum_queries_with_hit * 100
    >= 80 * thresholds.evaluation_set.query_count;
if !precision_passes || !recall_passes {
    return Err(ThresholdError::Retrieval);
}
```

Register `mod thresholds; pub use thresholds::*;` in `profile/mod.rs`.

- [ ] **Step 6: Run focused and full regression tests**

Run:

```bash
node --test test/retrieval-corpus-version.test.js
cargo test --manifest-path sovmem/Cargo.toml --test thresholds
cargo test --manifest-path sovmem/Cargo.toml --test golden_queries
```

Expected: all three commands exit 0; the golden file remains byte-identical.

- [ ] **Step 7: Commit Task 1**

```bash
git add docs/spec/okf-reference.json evals/audy-sovmem-v0/thresholds.json sovmem/src/profile/thresholds.rs sovmem/src/profile/golden.rs sovmem/src/profile/mod.rs sovmem/tests/thresholds.rs test/retrieval-corpus-version.test.js
git commit -m "P0.1: lock OKF and Q3 ground truth [gate: pending]"
```

---

### Task 2: Extract the current verifier without changing compatibility behavior

**Files:**
- Create: `sovmem/src/profile/verify.rs`
- Create: `sovmem/tests/bundle_verification.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `sovmem/src/profile/golden.rs`
- Modify: `sovmem/src/cli.rs`
- Modify: `sovmem/tests/cli.rs`

**Interfaces:**
- Produces: `VerificationLevel::{One,Two,Three}` with `number() -> u8`.
- Produces: `VerificationOptions { explicit_level: Option<VerificationLevel>, require_corpus: bool }` and `VerificationOptions::compatibility(require_corpus: bool)`.
- Produces: `verify_bundle(profile: &ProfileConfig, bundle_root: &Path, options: VerificationOptions) -> BundleVerification`.
- `BundleVerification` provides `document_count()`, `bundle_hash()`, `diagnostics()`, `has_errors()`, and crate-private access to generated documents, manifest, export report, and corpus report.
- Moves: `verified_published_corpus_evidence(profile, bundle_root)` from `crate::cli` to `profile::verify` without changing golden validation behavior.

- [ ] **Step 1: Write a failing module-boundary characterization test**

Create `sovmem/tests/bundle_verification.rs` using the existing `support::GitFixture` and the same one-note canary configuration pattern as `cli.rs`. Export with `export_canary`, then assert the new module API:

```rust
#[allow(dead_code)]
mod support;

use sovmem::profile::{
    export_canary, load_profile, verify_bundle, ExportRequest,
    VerificationOptions,
};
use std::fs;

struct VerificationFixture {
    vault: GitFixture,
    root: std::path::PathBuf,
    profile: std::path::PathBuf,
    canary: std::path::PathBuf,
    bundle: std::path::PathBuf,
}

impl VerificationFixture {
    fn new(name: &str) -> Self {
        let vault = GitFixture::new(name);
        vault.write("04-Research/OKF/README.md", "# Verification fixture
");
        vault.commit_all("add verification fixture", "2026-01-02T12:00:00Z");
        let root = std::env::temp_dir().join(format!(
            "sovmem-bundle-verification-{name}-{}",
            std::process::id(),
        ));
        fs::create_dir(&root).expect("create verification area");
        let canary = root.join("canary.yaml");
        fs::write(
            &canary,
            format!(
                "mode: committed
source_commit: {}
include:
  - 04-Research/OKF/README.md
exclude: []
",
                vault.head(),
            ),
        ).expect("write canary");
        Self {
            vault,
            profile: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../profiles/audy-ssa0-v0/profile.yaml"),
            bundle: root.join("bundle"),
            canary,
            root,
        }
    }
}

impl Drop for VerificationFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn extracted_verifier_preserves_the_valid_canary_contract() {
    let fixture = VerificationFixture::new("module-compatibility");
    export_canary(ExportRequest {
        vault_root: fixture.vault.root.clone(),
        output_root: fixture.bundle.clone(),
        profile_path: fixture.profile.clone(),
        canary_path: fixture.canary.clone(),
    }).expect("fixture exports");
    let profile = load_profile(&fixture.profile).expect("profile loads");
    let root = fs::canonicalize(&fixture.bundle).expect("bundle canonicalizes");
    let result = verify_bundle(&profile, &root, VerificationOptions::compatibility(false));
    assert_eq!(result.document_count(), 1);
    assert!(!result.has_errors(), "{:?}", result.diagnostics());
    assert_eq!(result.bundle_hash().len(), 64);
}
```

In `sovmem/tests/cli.rs`, strengthen `valid_bundle_verification_prints_one_json_summary` with:

```rust
assert_eq!(summary.as_object().unwrap().len(), 6);
assert!(summary.get("level").is_none());
```

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```bash
cargo test --manifest-path sovmem/Cargo.toml --test bundle_verification
cargo test --manifest-path sovmem/Cargo.toml --test cli valid_bundle_verification_prints_one_json_summary
```

Expected: the new module test fails to compile because the verification API does not exist; the CLI characterization stays green.

- [ ] **Step 3: Move verifier internals into `profile::verify`**

Create `sovmem/src/profile/verify.rs`. Move the current `Verification`, `OutputDiagnostic`, `verify_bundle`, `verified_published_corpus_evidence`, and all verifier-only helpers from `cli.rs`:

```text
validate_manifest_structure
validate_export_report
validate_corpus_report
manifest_selects
strictly_sorted
strictly_sorted_by_source
valid_root_shape
validate_manifest_identity
source_hashes
generated_document
safe_relative_path
lowercase_hash
lowercase_hex
read_bundle_file
generated_file_paths
hash_bundle
```

Use these exact public shells around the unchanged compatibility algorithm:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationLevel { One, Two, Three }

impl VerificationLevel {
    pub const fn number(self) -> u8 {
        match self { Self::One => 1, Self::Two => 2, Self::Three => 3 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerificationOptions {
    pub explicit_level: Option<VerificationLevel>,
    pub require_corpus: bool,
}

impl VerificationOptions {
    pub const fn compatibility(require_corpus: bool) -> Self {
        Self { explicit_level: None, require_corpus }
    }
}
```

Keep redacted diagnostics serializable and ordered:

```rust
#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct VerificationDiagnostic {
    pub severity: &'static str,
    pub code: String,
    pub message: &'static str,
}

impl VerificationDiagnostic {
    pub fn error(code: impl Into<String>, message: &'static str) -> Self {
        Self { severity: "error", code: code.into(), message }
    }
}
```

`BundleVerification` keeps internal evidence private and exposes only the getters listed in Interfaces. During this extraction, `verify_bundle` must reject `explicit_level: Some(_)` with an internal assertion because Task 3 owns level behavior; no CLI path constructs that option yet.

Register `mod verify; pub use verify::*;` in `profile/mod.rs`. Change `golden.rs` to import `verified_published_corpus_evidence` from `super`, eliminating the `crate::cli` dependency.

- [ ] **Step 4: Reduce the CLI to orchestration**

In `cli.rs`, import `verify_bundle`, `VerificationOptions`, and `VerificationDiagnostic`; keep only `run_verify`, `emit_diagnostics`, and `print_json` for verification I/O. Change the call to:

```rust
let verification = verify_bundle(
    &profile,
    &bundle_root,
    VerificationOptions::compatibility(require_corpus),
);
```

Do not change `VerifyOutput`, exit codes, legacy corpus requirement, diagnostic messages, or JSON field order in this task.

- [ ] **Step 5: Run verifier, golden, and full Rust regressions**

Run:

```bash
cargo test --manifest-path sovmem/Cargo.toml --test bundle_verification
cargo test --manifest-path sovmem/Cargo.toml --test cli
cargo test --manifest-path sovmem/Cargo.toml --test golden_queries
cargo test --manifest-path sovmem/Cargo.toml
```

Expected: every command exits 0; bare verify output remains a six-field JSON object with no `level` field.

- [ ] **Step 6: Commit Task 2**

```bash
git add sovmem/src/profile/verify.rs sovmem/src/profile/mod.rs sovmem/src/profile/golden.rs sovmem/src/cli.rs sovmem/tests/bundle_verification.rs sovmem/tests/cli.rs
git commit -m "P1.1: extract offline bundle verification [gate: pending]"
```

---

### Task 3: Add cumulative verification levels and CLI compatibility routing

**Files:**
- Modify: `sovmem/src/profile/verify.rs`
- Modify: `sovmem/src/cli.rs`
- Modify: `sovmem/tests/bundle_verification.rs`
- Modify: `sovmem/tests/cli.rs`

**Interfaces:**
- Explicit Level 1 runs structure/profile/provenance checks only.
- Explicit Level 2 runs Level 1 plus raw-byte document hashes, manifest/file-set integrity, source-hash commitment shape, index, and framed bundle root.
- Explicit Level 3 runs Levels 1-2 plus link/source/report correspondence and requires complete corpus evidence.
- Compatibility mode runs the current complete applicable checks, with corpus evidence optional unless legacy `--require-corpus` is set.
- Explicit success includes `"level": 1|2|3`; compatibility success omits `level`.
- `--require-corpus` with explicit Level 1 or 2 exits 2 with `verify.level_conflict`.

- [ ] **Step 1: Write failing CLI level-routing tests**

Add helpers to `sovmem/tests/cli.rs`:

```rust
fn run_verify_level(profile: &Path, bundle: &Path, level: u8) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["verify", "--profile"]).arg(profile)
        .args(["--bundle"]).arg(bundle)
        .args(["--level", &level.to_string()])
        .output().expect("run leveled verifier")
}

fn run_verify_level_require_corpus(profile: &Path, bundle: &Path, level: u8) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["verify", "--profile"]).arg(profile)
        .args(["--bundle"]).arg(bundle)
        .args(["--level", &level.to_string(), "--require-corpus"])
        .output().expect("run conflicting verifier")
}
```

Add tests that prove:

```rust
#[test]
fn explicit_levels_are_reported_and_level_three_requires_corpus() {
    let (_repo, _area, bundle) = export_fixture("verify-levels");
    for level in [1, 2] {
        let output = run_verify_level(&profile_path(), &bundle, level);
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(stderr(&output).is_empty());
        assert_eq!(one_json_line(&output)["level"], level);
    }
    let level_three = run_verify_level(&profile_path(), &bundle, 3);
    assert_eq!(level_three.status.code(), Some(1));
    assert!(stderr(&level_three).contains("bundle.corpus_report_required"));
}

#[test]
fn legacy_and_explicit_level_contracts_do_not_conflict_silently() {
    let (_repo, _area, bundle) = export_fixture("verify-level-conflict");
    for level in [1, 2] {
        let output = run_verify_level_require_corpus(&profile_path(), &bundle, level);
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains("verify.level_conflict"));
    }
}
```

Add clap boundary cases for levels `0` and `4`, expecting exit 2 and no source sentinel. Keep the existing tests proving bare canary verification succeeds and legacy corpus-required verification fails when evidence is absent.

- [ ] **Step 2: Write failing module-level cumulative-boundary tests**

In `bundle_verification.rs`, mutate a valid generated document without repairing hashes. Assert Level 1 reaches structural success while Level 2 reports an error. Then add a corpus fixture or reuse the fixed corpus fixture helpers to assert Level 3 requires a corpus report and Compatibility remains optional-corpus.

Use this constructor in tests and implementation:

```rust
impl VerificationOptions {
    pub const fn level(level: VerificationLevel) -> Self {
        Self {
            explicit_level: Some(level),
            require_corpus: matches!(level, VerificationLevel::Three),
        }
    }
}
```

- [ ] **Step 3: Run focused tests and verify RED**

Run:

```bash
cargo test --manifest-path sovmem/Cargo.toml --test cli explicit_levels
cargo test --manifest-path sovmem/Cargo.toml --test cli legacy_and_explicit
cargo test --manifest-path sovmem/Cargo.toml --test bundle_verification
```

Expected: tests fail because `--level` is unknown and the verifier does not branch by level.

- [ ] **Step 4: Add CLI parsing and compatibility-preserving output**

Modify `Command::Verify`:

```rust
Verify {
    #[arg(long)]
    profile: PathBuf,
    #[arg(long)]
    bundle: PathBuf,
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=3))]
    level: Option<u8>,
    #[arg(long)]
    require_corpus: bool,
},
```

Add conversion and conflict handling before verification:

```rust
let explicit_level = match level {
    Some(1) => Some(VerificationLevel::One),
    Some(2) => Some(VerificationLevel::Two),
    Some(3) => Some(VerificationLevel::Three),
    Some(_) => unreachable!("clap constrains levels"),
    None => None,
};
if require_corpus && matches!(explicit_level, Some(VerificationLevel::One | VerificationLevel::Two)) {
    emit_diagnostics(vec![VerificationDiagnostic::error(
        "verify.level_conflict",
        "corpus evidence requires verification level 3",
    )]);
    return 2;
}
```

Change `VerifyOutput` to preserve compatibility output exactly:

```rust
#[derive(Serialize)]
struct VerifyOutput {
    status: &'static str,
    command: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<u8>,
    documents: usize,
    warnings: usize,
    errors: usize,
    bundle_hash: String,
}
```

Keep compatibility-mode warning emission unchanged. For explicit levels, emit diagnostics only when an error exists; a successful explicit verification reports the warning count in stdout and leaves stderr empty. This preserves the legacy surface while satisfying the formal Level 3 output contract.

- [ ] **Step 5: Split verification into cumulative stages**

In `verify.rs`, derive these policy booleans only from `VerificationOptions`:

```rust
let compatibility = options.explicit_level.is_none();
let level = options.explicit_level;
let run_integrity = compatibility || matches!(level, Some(VerificationLevel::Two | VerificationLevel::Three));
let run_semantic = compatibility || matches!(level, Some(VerificationLevel::Three));
let require_corpus = options.require_corpus || matches!(level, Some(VerificationLevel::Three));
```

Organize the existing algorithm into focused private functions:

```rust
fn verify_level_one(profile: &ProfileConfig, root: &Path, manifest: &BundleManifest, out: &mut BundleVerification);
fn verify_level_two(root: &Path, manifest: &BundleManifest, out: &mut BundleVerification);
fn verify_level_three(profile: &ProfileConfig, root: &Path, manifest: &BundleManifest, out: &mut BundleVerification);
```

Level 1 reads generated Markdown through manifest paths, parses frontmatter, decodes `GeneratedMetadata`, and runs structural/profile validation. Level 2 owns manifest structure, exact files, raw document hashes, source-hash commitment shape, `bundle/index.md`, and framed root. Level 3 owns link reconstruction, source/document set correspondence, export-report reconstruction, corpus-report reconstruction, and the 62-document Audy gate. Compatibility invokes every applicable stage but makes `corpus-report.json` optional.

Do not downgrade any current compatibility check or diagnostic. Sort and deduplicate diagnostics after all selected stages.

- [ ] **Step 6: Run focused and full Rust regressions**

Run:

```bash
cargo test --manifest-path sovmem/Cargo.toml --test bundle_verification
cargo test --manifest-path sovmem/Cargo.toml --test cli
cargo test --manifest-path sovmem/Cargo.toml --test golden_queries
cargo test --manifest-path sovmem/Cargo.toml
```

Expected: all commands exit 0; bare verify stays field-compatible, Level 1-3 are cumulative, and explicit Level 3 requires corpus evidence.

- [ ] **Step 7: Commit Task 3**

```bash
git add sovmem/src/profile/verify.rs sovmem/src/cli.rs sovmem/tests/bundle_verification.rs sovmem/tests/cli.rs
git commit -m "P1.2: formalize cumulative verification levels [gate: pending]"
```

---

### Task 4: Add scenario-specific corruption diagnostics

**Files:**
- Modify: `sovmem/src/profile/verify.rs`
- Modify: `sovmem/tests/cli.rs`

**Interfaces:**
- Level 2 modified bytes: `bundle.document_hash_mismatch`.
- Level 1 malformed frontmatter: `frontmatter.malformed`.
- Level 1 invalid provenance: `document.provenance_invalid`.
- Level 2 repaired document hashes with stale root: `bundle.root_hash_mismatch`.
- Level 3 nonexistent typed same-profile target: `links.dangling`.
- Existing umbrella diagnostics remain permitted as additional compatibility output.

- [ ] **Step 1: Add hash-repair helpers that isolate each corruption**

In `sovmem/tests/cli.rs`, keep the existing `repair_hashes` helper and add:

```rust
fn repair_all_document_hashes(bundle: &Path, manifest: &mut Value) {
    for document in manifest["documents"].as_array_mut().unwrap() {
        let output_path = document["output_path"].as_str().unwrap();
        let bytes = fs::read(bundle.join(output_path)).unwrap();
        document["hash"] = Value::String(blake3::hash(&bytes).to_hex().to_string());
    }
    write_manifest(bundle, manifest);
}

fn repair_bundle_root_claims(bundle: &Path, manifest: &mut Value) {
    let root = canonical_bundle_hash(bundle, manifest);
    manifest["bundle_hash"] = Value::String(root.clone());
    write_manifest(bundle, manifest);
    if bundle.join("corpus-report.json").is_file() {
        let mut corpus = corpus_report(bundle);
        corpus["bundle_hash"] = Value::String(root);
        write_corpus_report(bundle, &corpus);
    }
}

fn corpus_report(bundle: &Path) -> Value {
    serde_json::from_slice(&fs::read(bundle.join("corpus-report.json")).unwrap()).unwrap()
}

fn write_corpus_report(bundle: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'
');
    fs::write(bundle.join("corpus-report.json"), bytes).unwrap();
}
```

- [ ] **Step 2: Write the five failing corruption tests**

Add one test per required code. Use explicit levels so each failure proves its minimum boundary:

```rust
#[test]
fn changed_document_bytes_have_a_specific_level_two_code() {
    let (_repo, _area, bundle) = export_fixture("corrupt-document-bytes");
    let document = bundle.join(format!("bundle/{SOURCE_PATH}"));
    fs::write(&document, [fs::read(&document).unwrap(), b"
changed
".to_vec()].concat()).unwrap();
    let output = run_verify_level(&profile_path(), &bundle, 2);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("bundle.document_hash_mismatch"));
}

#[test]
fn malformed_frontmatter_has_a_specific_level_one_code() {
    let (_repo, _area, bundle) = export_fixture("corrupt-frontmatter");
    let document = bundle.join(format!("bundle/{SOURCE_PATH}"));
    fs::write(document, b"---
provenance: synthesis
").unwrap();
    let output = run_verify_level(&profile_path(), &bundle, 1);
    assert!(stderr(&output).contains("frontmatter.malformed"));
}
```

For invalid provenance, mutate `metadata["provenance"] = "forged"` and run Level 1. For stale root, mutate bytes, call `repair_all_document_hashes`, leave `manifest.bundle_hash` unchanged, and run Level 2. For dangling links, add `{rel: "references", target: "ssa-0/04-Research/Missing"}`, repair document and root claims, and run Level 3 against a corpus fixture; assert `links.dangling` even when `bundle.report_invalid` also appears.

Every test also asserts stdout is empty and `assert_no_source_content` passes.

- [ ] **Step 3: Run focused tests and verify RED**

Run each exact test name with:

```bash
cargo test --manifest-path sovmem/Cargo.toml --test cli changed_document_bytes_have_a_specific_level_two_code
cargo test --manifest-path sovmem/Cargo.toml --test cli malformed_frontmatter_has_a_specific_level_one_code
cargo test --manifest-path sovmem/Cargo.toml --test cli invalid_provenance_has_a_specific_level_one_code
cargo test --manifest-path sovmem/Cargo.toml --test cli repaired_document_hashes_do_not_repair_the_bundle_root
cargo test --manifest-path sovmem/Cargo.toml --test cli dangling_typed_link_has_a_specific_level_three_code
```

Expected: each fails because only the current umbrella code is emitted.

- [ ] **Step 4: Emit specific codes at the owning invariant**

In `verify.rs`:

```rust
// Level 1 parse failure
"frontmatter.malformed"

// Before GeneratedMetadata deserialization
match parsed.metadata.get("provenance").and_then(serde_yaml::Value::as_str) {
    Some("tier1" | "secondary" | "synthesis" | "unknown") => {}
    _ => diagnostics.push(VerificationDiagnostic::error(
        "document.provenance_invalid",
        "generated provenance is invalid",
    )),
}

// Level 2 individual document check
"bundle.document_hash_mismatch"

// Level 2 framed root check
"bundle.root_hash_mismatch"
```

For Level 3, build a `BTreeSet<&str>` of generated document IDs and inspect only typed generated links. Emit `links.dangling` when a same-profile target is absent. Do not convert existing `links.broken` or `links.ambiguous` source/body warnings into errors.

Keep `bundle.hash_mismatch`, `bundle.document_invalid`, or `links.target_invalid` where current callers depend on them; the new tests grep the scenario-specific code.

- [ ] **Step 5: Run corruption and full verifier regressions**

Run:

```bash
cargo test --manifest-path sovmem/Cargo.toml --test cli
cargo test --manifest-path sovmem/Cargo.toml --test bundle_verification
cargo test --manifest-path sovmem/Cargo.toml --test validation
cargo test --manifest-path sovmem/Cargo.toml
```

Expected: all commands exit 0; five specific codes are covered; compatibility diagnostics remain accepted.

- [ ] **Step 6: Commit Task 4**

```bash
git add sovmem/src/profile/verify.rs sovmem/tests/cli.rs
git commit -m "P1.3: cover formal corruption failures [gate: pending]"
```

---

### Task 5: Add deterministic verified-bundle statistics

**Files:**
- Create: `sovmem/src/profile/stats.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `sovmem/src/profile/verify.rs`
- Modify: `sovmem/src/cli.rs`
- Modify: `sovmem/tests/cli.rs`

**Interfaces:**
- Produces: `summarize_verified_bundle(verification: &BundleVerification) -> Option<BundleStats>`.
- `BundleStats` fields are `documents`, `by_provenance`, `by_type`, `by_layer`, `warnings`, `exclusions`, and `bundle_hash` in that serialization order.
- CLI: `sovmem stats --profile <profile.yaml> --bundle <bundle-root>`.
- Stats runs compatibility verification, writes no summary on errors, and never reads the live vault.

- [ ] **Step 1: Write failing stats CLI tests**

Add `run_stats` to `sovmem/tests/cli.rs`:

```rust
fn run_stats(profile: &Path, bundle: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["stats", "--profile"]).arg(profile)
        .args(["--bundle"]).arg(bundle)
        .output().expect("run sovmem stats")
}
```

Add tests for exact schema, sums, repeatability, and failure suppression:

```rust
#[test]
fn stats_are_complete_deterministic_and_derived_from_verified_evidence() {
    let (_repo, _area, bundle) = export_fixture("stats-success");
    let first = run_stats(&profile_path(), &bundle);
    let second = run_stats(&profile_path(), &bundle);
    assert!(first.status.success(), "{}", stderr(&first));
    assert_eq!(first.stdout, second.stdout);
    let value = one_json_line(&first);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["command"], "stats");
    assert_eq!(value["documents"], 1);
    assert_eq!(
        value["warnings"].as_u64().unwrap(),
        report(&bundle)["counts"]["warnings"].as_u64().unwrap(),
    );
    assert_eq!(value["exclusions"], 0);
    for key in ["by_provenance", "by_type", "by_layer"] {
        let sum: u64 = value[key].as_object().unwrap().values().map(|v| v.as_u64().unwrap()).sum();
        assert_eq!(sum, 1);
    }
    assert_eq!(
        value["by_provenance"].as_object().unwrap().keys().cloned().collect::<Vec<_>>(),
        vec![
            "secondary".to_owned(),
            "synthesis".to_owned(),
            "tier1".to_owned(),
            "unknown".to_owned(),
        ],
    );
}
```

Add a tampered-bundle test asserting exit 1, empty stdout, and the same redacted verification code as `verify`.

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```bash
cargo test --manifest-path sovmem/Cargo.toml --test cli stats_
```

Expected: FAIL because `stats` is not a command.

- [ ] **Step 3: Implement `BundleStats` from verified evidence**

Create `profile/stats.rs`:

```rust
use serde::Serialize;
use std::collections::BTreeMap;
use super::{BundleVerification, CoreType, Layer, Provenance};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct BundleStats {
    pub documents: usize,
    pub by_provenance: BTreeMap<String, usize>,
    pub by_type: BTreeMap<String, usize>,
    pub by_layer: BTreeMap<String, usize>,
    pub warnings: usize,
    pub exclusions: usize,
    pub bundle_hash: String,
}
```

Initialize every enum bucket at zero, then count the verified generated documents using explicit enum matches. Derive warnings from `verification.diagnostics()` where severity is `warning`, exclusions from the verified manifest, and bundle hash from the independently computed verification result. Return `None` if `verification.has_errors()`.

Add crate-private `generated_documents()` and `exclusion_count()` getters to `BundleVerification`. Register `mod stats; pub use stats::*;`.

- [ ] **Step 4: Add CLI orchestration and fixed output order**

Add `Command::Stats { profile, bundle }`, load and canonicalize inputs exactly as `run_verify`, and run `VerificationOptions::compatibility(false)`. If verification has errors, emit its diagnostics and return 1; on unreadable inputs return 2. On success, suppress warning diagnostics, place their count in the summary, and return 0.

Serialize with:

```rust
#[derive(Serialize)]
struct StatsOutput {
    status: &'static str,
    command: &'static str,
    #[serde(flatten)]
    stats: BundleStats,
}
```

Successful output is one compact JSON line with one terminal newline and no timestamp or path.

- [ ] **Step 5: Run stats and full Rust regressions**

Run:

```bash
cargo test --manifest-path sovmem/Cargo.toml --test cli stats_
cargo test --manifest-path sovmem/Cargo.toml --test cli
cargo test --manifest-path sovmem/Cargo.toml
```

Expected: every command exits 0; repeated stats bytes match exactly; invalid bundles emit no summary.

- [ ] **Step 6: Commit Task 5**

```bash
git add sovmem/src/profile/stats.rs sovmem/src/profile/mod.rs sovmem/src/profile/verify.rs sovmem/src/cli.rs sovmem/tests/cli.rs
git commit -m "P1.4: report deterministic verified bundle stats [gate: pending]"
```

---

### Task 6: Record the decisions and wire the supported commands

**Files:**
- Modify: `test/retrieval-corpus-version.test.js`
- Modify: `package.json`
- Modify: `docs/sovmem_v0.3_execution_plan.md`
- Modify: `README.md`
- Modify: `index.md`

**Interfaces:**
- `npm run verify:corpus:audy` invokes explicit `--level 3`.
- `npm run stats:audy` invokes offline stats for `var/audy-sovmem-retrieval-corpus`.
- Bare `verify:audy`, corpus export, and golden verification remain path-portable.
- The normative plan records P0 decisions and Q3 as resolved, but leaves P2 retrieval work unimplemented.
- Repository publication wording describes manual PR merge discipline and makes no enforcement claim.

- [ ] **Step 1: Write failing root-command and documentation contract tests**

Extend `test/retrieval-corpus-version.test.js`:

```js
test('wires formal level-three verification and deterministic stats', () => {
  const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
  assert.match(pkg.scripts['verify:corpus:audy'], /--level 3/);
  assert.doesNotMatch(pkg.scripts['verify:corpus:audy'], /--require-corpus/);
  assert.match(pkg.scripts['stats:audy'], /sovmem\/Cargo\.toml -- stats/);
  assert.match(pkg.scripts['stats:audy'], /var\/audy-sovmem-retrieval-corpus/);
  assert.doesNotMatch(JSON.stringify(pkg), /\/Volumes\/himura|\/Users\/khalilhimura/);
});

test('records gate closure without claiming retrieval or enforced GitHub controls', () => {
  const plan = readFileSync('docs/sovmem_v0.3_execution_plan.md', 'utf8');
  assert.match(plan, /62 accepted documents.*8 core.*54 research/s);
  assert.match(plan, /GoogleCloudPlatform\/knowledge-catalog.*d44368c15e38e7c92481c5992e4f9b5b421a801d.*okf\/SPEC\.md/s);
  assert.match(plan, /minimum_queries_with_hit.*20/);
  assert.match(plan, /manual pull-request merge discipline/);
  assert.doesNotMatch(plan, /branch protection is enforced|required merge queue is enforced/);
});
```

Add registry assertions that the new design, this plan, OKF pin, threshold artifact, verifier, stats module, threshold tests, and bundle-verification tests each appear once in their owning `index.md` sections.

- [ ] **Step 2: Run the Node contract test and verify RED**

Run:

```bash
node --test test/retrieval-corpus-version.test.js
```

Expected: FAIL because the new command and documentation contracts are not wired.

- [ ] **Step 3: Update root commands exactly**

Set these package scripts without absolute paths:

```json
"verify:corpus:audy": "cargo run --manifest-path sovmem/Cargo.toml -- verify --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus --level 3",
"stats:audy": "cargo run --manifest-path sovmem/Cargo.toml -- stats --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus"
```

Keep all existing scripts and ordering otherwise stable.

- [ ] **Step 4: Apply the approved normative-plan decisions**

Update `docs/sovmem_v0.3_execution_plan.md` with exact decisions:

```text
D3: filesystem + five-verb ledger; Git supplies snapshot identity and repository review history, not the judgment ledger.
D5: single crate.
D7: Nous repository, private, manual pull-request merge discipline.
D8: five internal verbs; ACCEPT/REJECT/REWRITE public pedagogical subset.
D10: tier1|secondary|synthesis|unknown.
Q1/Q2: Audy Vault is canonical, Git-versioned, read-only for this slice, and pinned at 43824cfd9caed7b260aa442128fd1130ddb6e2ff.
Q3: thresholds.json freezes k=5, P@5>=0.16, R@5>=0.80, and 20/25 hit queries.
Q4/Q10: resolved by D7/D8/D10.
```

Revise P0 and P1 gate text to the approved 62-document, 25-query, OKF-pin, Level 1-3, stats, and five-corruption contracts. Move the resolved questions out of the blocking list. Keep the P2 gate unpassed and retain the ban on tuning queries, thresholds, or fixtures.

Replace executor-protocol publication language with:

```text
Before manually merging a SovMem implementation pull request, update docs/sovmem-build-state.json, regenerate docs/SOVMEM-BUILD-AUDIT.md, pass npm run audit:sovmem:check and npm test at the reviewed head, and review the implementation and audit together. This private repository currently relies on manual pull-request merge discipline; documentation and audit evidence must not represent branch protection, required checks, rulesets, or merge queues as enforced unless independently verified later.
```

- [ ] **Step 5: Update README and artifact registry**

Document Levels 1-3, legacy bare verify compatibility, the formal Level 3 command, stats fields, 226 accepted warnings plus one exclusion, OKF pin, frozen thresholds, and the integer 20/25 gate. Explicitly state that no retrieval, ranking, query engine, or evaluation baseline exists.

Register every artifact listed in the Task 6 test. Describe the new design and plan as approved/implementation guidance, the OKF and threshold JSON as pinned reviewed ground truth, verifier/stats/threshold modules as bounded P0/P1 implementation, and generated output as non-authoritative.

- [ ] **Step 6: Run documentation contracts and Rust help smoke tests**

Run:

```bash
node --test test/retrieval-corpus-version.test.js
cargo run --quiet --manifest-path sovmem/Cargo.toml -- verify --help
cargo run --quiet --manifest-path sovmem/Cargo.toml -- stats --help
```

Expected: Node exits 0; both help commands exit 0 and expose only the approved arguments.

- [ ] **Step 7: Commit Task 6**

```bash
git add test/retrieval-corpus-version.test.js package.json docs/sovmem_v0.3_execution_plan.md README.md index.md
git commit -m "P0.2: record gate decisions and commands [gate: pending]"
```

---

### Task 7: Publish P0/P1 closure through audit schema v2

**Files:**
- Modify: `scripts/lib/sovmem-build-audit.mjs`
- Modify: `test/sovmem-build-audit.test.js`
- Modify: `docs/sovmem-build-state.json`
- Modify: `docs/SOVMEM-BUILD-AUDIT.md`
- Modify: `docs/superpowers/specs/2026-07-12-sovmem-build-audit-design.md`
- Modify: `index.md`

**Interfaces:**
- Audit schema becomes `sovmem-build-audit-v2` because the strict evidence object changes.
- Evidence adds corpus warnings/exclusions, golden BLAKE3, threshold identity, explicit Level 3 outcome, and `implementation_commit`.
- P0 and P1 are 100%/passed; P2 stays 10%/in-progress; overall rounds to 24%.
- Blockers are empty because Q3 is frozen; next work is P2 retrieval implementation.
- Audit wording calls tests a fresh gate-closure baseline and does not claim GitHub controls are enforced.

- [ ] **Step 1: Write failing audit v2 tests**

Update `validState()` in `test/sovmem-build-audit.test.js` to this evidence shape:

```js
evidence: {
  tests: { javascript: 43, rust: 185 },
  corpus: { accepted: 62, core: 8, research: 54, warnings: 226, exclusions: 1, errors: 0 },
  golden: {
    queries: 25,
    relevant_ids: 25,
    coverage_groups: 5,
    errors: 0,
    blake3: 'f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175',
  },
  thresholds: {
    schema_version: 'sovmem-retrieval-thresholds-v1',
    k: 5,
    minimum_queries_with_hit: 20,
    freeze_date: '2026-07-12',
  },
  verification: { level: 3, documents: 62, warnings: 226, errors: 0 },
  bundle_hash: '49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65',
  implementation_commit: 'a'.repeat(40),
},
```

Set `schema_version: 'sovmem-build-audit-v2'`, `blockers: []`, phase percentages `[100,100,10,0,0,0]`, P0/P1 gates `passed`, and test:

```js
test('calculates the approved gate-closure state as 24 percent', () => {
  const state = validateState(validState());
  assert.equal(calculateOverall(state.phases), 24);
});

test('requires frozen thresholds and explicit level-three evidence', () => {
  for (const mutate of [
    state => { state.evidence.thresholds.minimum_queries_with_hit = 19; },
    state => { state.evidence.verification.level = 2; },
    state => { state.evidence.verification.documents = 61; },
    state => { state.evidence.golden.blake3 = '0'.repeat(64); },
  ]) {
    const state = validState();
    mutate(state);
    assert.throws(() => validateState(state));
  }
});
```

Update renderer assertions to require `Overall completion: 24%`, Level 3, 226 warnings, one exclusion, threshold schema, 20 hit queries, golden BLAKE3, and `Implementation commit`. Replace the old strict-enforcement constant with the exact manual-discipline paragraph from Task 6.

- [ ] **Step 2: Run audit tests and verify RED**

Run:

```bash
node --test test/sovmem-build-audit.test.js
```

Expected: FAIL because the validator accepts only v1 and the renderer lacks gate-closure evidence.

- [ ] **Step 3: Implement audit schema v2 validation and rendering**

In `scripts/lib/sovmem-build-audit.mjs`:

```js
if (value.schema_version !== 'sovmem-build-audit-v2') {
  fail('schema_version', 'must be sovmem-build-audit-v2');
}
```

Require the exact evidence keys:

```js
['tests', 'corpus', 'golden', 'thresholds', 'verification', 'bundle_hash', 'implementation_commit']
```

Validate the approved literals and relationships, including positive JavaScript and Rust test totals, corpus `62=8+54`, one exclusion, golden BLAKE3, thresholds `k=5` and `minimum_queries_with_hit=20`, verification Level 3 with 62 documents and zero errors, and 40-character implementation commit. Render every new field and relabel the test evidence as `Gate-closure verification baseline` rather than historical corpus-milestone evidence.

Change the bundle-hash validation message from `must be a SHA-256 hash` to `must be a 64-character lowercase BLAKE3 hash`; the source-tree fingerprint remains SHA-256.

Keep generic integer, privacy, path, fingerprint, and phase-gate validation intact.

- [ ] **Step 4: Gather exact current evidence before editing reviewed state**

Run from the worktree with `AUDY_VAULT` already supplied by the environment:

```bash
test -n "$AUDY_VAULT"
npm run corpus:audy
npm run verify:corpus:audy
npm run stats:audy
npm run verify:golden:audy
node --test test/*.test.js
cargo test --manifest-path sovmem/Cargo.toml
git rev-parse HEAD
```

Record the JavaScript total from Node's final `# tests` line and sum the Rust `test result` passed counts across all integration-test targets; exclude the two zero-test unit targets and the zero-test doc-test target from neither the run nor the arithmetic, because adding zero leaves the same total. Record the exact 40-character `git rev-parse HEAD` output as `implementation_commit`; it is the last implementation commit before the audit publication commit. Assert Level 3 and stats both report 62 documents, 226 warnings, 1 exclusion, and bundle hash `49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65`. If any value differs, stop and investigate rather than editing reviewed state to the expected value.

- [ ] **Step 5: Write the reviewed gate-closure state**

Update `docs/sovmem-build-state.json` with:

```text
schema_version: sovmem-build-audit-v2
summary: P0 and P1 gates are closed and Q3 is frozen; retrieval and the sovereign write-back loop remain unbuilt.
next_step: Implement the frozen P2 Tantivy index, Petgraph adjacency, BM25 query path, evaluation baseline, and determinism gate without changing golden queries or thresholds.
blockers: []
P0: 100, passed, no remaining items
P1: 100, passed, no remaining items
P2: 10, in-progress, Q3 listed as completed, retrieval/index/eval listed as remaining
P3-P5: unchanged at 0, not-started
```

Use the exact evidence gathered in Step 4. Add one history entry dated `2026-07-12`, phase `P0/P1`, summary `Closed the ground-truth and bundle-substrate gates and froze Q3.`, and the same implementation commit.

- [ ] **Step 6: Add the audit-design supersession note and regenerate**

At the top of `docs/superpowers/specs/2026-07-12-sovmem-build-audit-design.md`, after metadata, add:

```markdown
> **2026-07-12 gate-closure supersession:** The approved P0/P1 gate-closure design moves reviewed evidence to `sovmem-build-audit-v2` and records the repository's current private, manual pull-request merge discipline. The workflow remains useful evidence, but this repository does not claim branch protection, required checks, rulesets, or merge queues are enforced.
```

Update its affected schema/evidence/publication wording without rewriting its historical rationale. Update the `index.md` audit rows to v2 and manual discipline. Then run:

```bash
npm run audit:sovmem
npm run audit:sovmem:check
node --test test/sovmem-build-audit.test.js
```

Expected: generated audit reports 24%, P0/P1 passed, P2 10%, no blockers, Level 3 evidence, Q3 freeze, and no enforcement claim; all commands exit 0.

- [ ] **Step 7: Commit Task 7**

```bash
git add scripts/lib/sovmem-build-audit.mjs test/sovmem-build-audit.test.js docs/sovmem-build-state.json docs/SOVMEM-BUILD-AUDIT.md docs/superpowers/specs/2026-07-12-sovmem-build-audit-design.md index.md
git commit -m "P1.5: publish P0 and P1 gate closure [gate: pass]"
```

---

### Task 8: Run the clean final gate and prepare the manual-review handoff

**Files:**
- Verify only: no source file may change during this task.

**Interfaces:**
- Proves the committed branch satisfies P0/P1 and freezes Q3 while P2 retrieval remains absent.
- Produces the exact branch head, test totals, Level 3 summary, stats summary, audit freshness result, and clean status for the pull-request handoff.

- [ ] **Step 1: Verify the committed scope and forbidden dependencies**

Run:

```bash
git status --short
git diff --name-status origin/main...HEAD
git diff origin/main...HEAD -- sovmem/Cargo.toml sovmem/Cargo.lock
rg -n "tantivy|petgraph|BM25|baseline\.json|Command::Query|Command::Index|Command::Eval" sovmem package.json
```

Expected: status is empty; diff contains only approved P0/P1/Q3 surfaces; dependency diff is empty; the search finds only documentation/test assertions and no retrieval implementation or dependency.

- [ ] **Step 2: Prove deterministic live corpus, Level 3, stats, and golden evidence**

Run:

```bash
test -n "$AUDY_VAULT"
npm run corpus:audy
npm run verify:corpus:audy
npm run stats:audy
cargo run --quiet --manifest-path sovmem/Cargo.toml -- stats --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus > /tmp/sovmem-stats-1.json
cargo run --quiet --manifest-path sovmem/Cargo.toml -- stats --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus > /tmp/sovmem-stats-2.json
cmp /tmp/sovmem-stats-1.json /tmp/sovmem-stats-2.json
jq -e '.documents == 62 and .warnings == 226 and .exclusions == 1 and .bundle_hash == "49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65"' /tmp/sovmem-stats-1.json
npm run verify:golden:audy
```

Expected: every command exits 0. Verify reports Level 3, 62 documents, 226 warnings, zero errors, and bundle hash `49ed4dc745cad67e2efe526b8be9b2c08d06a05856ab30bbf52d4da903144a65`. Stats reports the approved distributions, 226 warnings, 1 exclusion, and the same bundle hash. Golden validation reports 25 queries and 25 relevant IDs.

- [ ] **Step 3: Prove full regression and audit freshness**

Run:

```bash
node --test --test-reporter=dot test/*.test.js
cargo test --quiet --manifest-path sovmem/Cargo.toml
npm run audit:sovmem:check
```

Expected: all JavaScript and Rust tests pass with zero failures; audit is current.

- [ ] **Step 4: Confirm immutable evaluation bytes and final Git state**

Run:

```bash
shasum -a 256 evals/audy-sovmem-v0/golden.jsonl
shasum -a 256 evals/audy-sovmem-v0/thresholds.json
git status --short --branch
git log --oneline --decorate origin/main..HEAD
```

Expected golden SHA-256: `698e56224d8c5358948557f8e4c6965eeeab7cff76afbdaf382f2faa3091af7d`.

Expected threshold SHA-256: `33038018a4ca9fecaa960839f7bfb9e277f5306ac657a6ef30df62dbfa57f280`.

Expected Git state: clean branch `codex/sovmem-p0-p1-gate-closure`, ahead of `origin/main`, with task commits in order and no implementation beyond P0/P1/Q3.

- [ ] **Step 5: Stop for manual review and publication direction**

Report the committed branch head, worktree, exact test totals, Level 3 result, stats result, audit result, and immutable digests. Do not push, open a pull request, or merge until the user explicitly authorizes that next action.

---

## Plan self-review result

- Every approved P0 decision maps to Tasks 1 and 6.
- The official OKF pin maps to Task 1 with exact bytes and strict repository tests.
- Level 1-3 semantics, compatibility mode, exit classes, and output shape map to Tasks 2 and 3.
- All five corruption classes and exact greppable codes map to Task 4.
- Deterministic count/distribution/warning/exclusion/bundle-hash output maps to Task 5.
- Q3 schema, identity, raw-byte golden BLAKE3, human approval, freeze date, and integer 20/25 gate map to Task 1.
- Golden/threshold immutability through P2 maps to Tasks 1 and 8.
- Normative decisions, supported commands, registry, and manual merge discipline map to Task 6.
- P0=100, P1=100, P2=10, overall=24%, no blocker, and regenerated audit evidence map to Task 7.
- Live corpus, Level 3, stats determinism, golden validation, full tests, audit freshness, scope exclusion, and clean handoff map to Task 8.
- Function names and types consumed by later tasks are defined in earlier Interfaces blocks.
- No task adds a dependency, retrieval implementation, live-vault write, external model call, or GitHub enforcement claim.
