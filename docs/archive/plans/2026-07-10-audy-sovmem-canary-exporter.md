> **ARCHIVED** — This implementation plan has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix A: Implementation Plan Index](../sovmem-master-spec.md#appendix-a-implementation-plan-index) for a summary of its scope.
>
# Audy-SovMem Canary Exporter and Validator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a read-only Rust exporter and validator that converts a pinned, committed 13-document Audy Vault snapshot into a deterministic, profile-isolated, OKF-shaped SovMem canary bundle without mutating the vault.

**Architecture:** Add the first narrow slice of the canonical `sovmem` Rust binary: profile loading, committed Git snapshot reads, deterministic metadata normalization, link conversion, bundle validation, and `export`/`verify` CLI commands. The exporter reads only objects from a pinned Audy Vault commit, rejects selected dirty or untracked paths, builds into a temporary output directory, validates before publication, and emits a bundle, manifest, and report outside the vault. Tantivy, Petgraph, MCP, proposals, verdict application, ledger mutation, trust scoring, Honcho ingestion, and live-vault writes remain outside this plan.

**Tech Stack:** Rust 1.84.0; Cargo; `clap`, `serde`, `serde_yaml`, `serde_json`, `blake3`, `anyhow`, and `thiserror`; Git CLI for immutable object reads and commit dates; Rust built-in test framework; existing Node.js 20 repository checks.

## Global Constraints

- The approved design is `docs/superpowers/specs/2026-07-10-audy-sovmem-profile-v0-design.md`.
- The reusable identifiers are `nous-ssa-context-v0` and `nous-sovmem-v0`; the reference instance is `ssa-0` / `audy-ssa0-v0` / `audy` / namespace `ssa-0`.
- Input mode is committed-only. The canary source commit is `43824cfd9caed7b260aa442128fd1130ddb6e2ff`.
- Selected modified or untracked paths are excluded and reported. Source bytes are read with `git show <commit>:<path>`, never from the working-tree file.
- The exporter MUST NOT write inside Audy Vault, mutate a source note, use filesystem modification time, call an LLM, infer a judgment, or mix profile namespaces.
- All classifications and fallbacks are deterministic. Explicit valid source metadata wins; conflicting explicit metadata is a hard failure.
- Core `type` values are exactly `concept`, `resource`, and `judgment`; `context_kind` carries profile vocabulary.
- Every generated document includes the approved required fields and records inferred classification/content fields in `inferred_fields`.
- Unknown `context_kind`, unresolved links, inferred description/provenance, weak citations, and excluded dirty paths are warnings; malformed required data, duplicate identities, restricted scope violations, source mismatch, namespace mismatch, and cross-profile references are hard failures.
- Output ordering, YAML field order, JSON object order, tag order, link order, document order, and hashes are deterministic. No wall-clock timestamp enters generated output.
- Only the dependencies named in the canonical SovMem v0.3 plan may be added. Tantivy, Petgraph, and `rmcp` are deliberately deferred.
- Generated canary output is reproducible evidence, not canon, and remains untracked by default.

## File Structure

| Path | Responsibility |
| --- | --- |
| `rust-toolchain.toml` | Pin Rust 1.84.0 for reproducible builds. |
| `sovmem/Cargo.toml` | Define the first single-binary SovMem crate and approved dependencies. |
| `sovmem/Cargo.lock` | Lock the approved dependency graph. |
| `sovmem/src/lib.rs` | Export the profile subsystem and stable public entry points. |
| `sovmem/src/main.rs` | Invoke the CLI and map failures to a non-zero exit. |
| `sovmem/src/cli.rs` | Define and dispatch `export` and `verify` commands. |
| `sovmem/src/profile/mod.rs` | Own shared profile, document, diagnostic, manifest, and report types. |
| `sovmem/src/profile/config.rs` | Load and validate profile and canary YAML. |
| `sovmem/src/profile/source.rs` | Resolve the pinned commit, reject dirty selections, read Git objects, dates, and hashes. |
| `sovmem/src/profile/frontmatter.rs` | Split source Markdown, preserve unknown fields, and serialize stable generated YAML. |
| `sovmem/src/profile/mapping.rs` | Apply ordered path mappings, sensitivity defaults, and deterministic metadata inference. |
| `sovmem/src/profile/links.rs` | Resolve Obsidian and Markdown links without inventing targets. |
| `sovmem/src/profile/validate.rs` | Enforce document, bundle, judgment, namespace, sensitivity, and duplicate rules. |
| `sovmem/src/profile/export.rs` | Orchestrate read, normalize, link, validate, hash, stage, and publish. |
| `profiles/audy-ssa0-v0/profile.yaml` | Store the approved Audy reference identity, kind map, mappings, and sensitivity policy. |
| `profiles/audy-ssa0-v0/canary.yaml` | Pin the source commit and 13 clean research-note paths. |
| `sovmem/tests/support/mod.rs` | Create disposable Git fixtures using only the standard library and Git CLI. |
| `sovmem/tests/profile_config.rs` | Cover identity, enums, mappings, and profile consistency. |
| `sovmem/tests/source_snapshot.rs` | Cover committed reads, dates, dirty exclusion, hashes, and source immutability. |
| `sovmem/tests/normalization.rs` | Cover frontmatter precedence and every deterministic fallback. |
| `sovmem/tests/link_conversion.rs` | Cover resolved, ambiguous, broken, external, and cross-profile links. |
| `sovmem/tests/validation.rs` | Cover hard failures, warnings, judgments, duplicates, and sensitivity. |
| `sovmem/tests/export_determinism.rs` | Cover byte identity, staging, output shape, and source preservation. |
| `sovmem/tests/default_profile_isolation.rs` | Prove a neutral fixture exports without SSA-0 or Audy leakage. |
| `sovmem/tests/cli.rs` | Cover CLI success, mismatch, output-inside-vault rejection, and verify failures. |
| `.gitignore` | Exclude generated profile output under `/var/`. |
| `package.json` | Add root-level Rust and combined verification scripts. |
| `README.md` | Document the canary commands and their non-canonical status. |
| `index.md` | Register the implementation artifacts and update current state. |

---

### Task 1: Bootstrap the pinned Rust crate and shared test harness

**Files:**
- Create: `rust-toolchain.toml`
- Create: `sovmem/Cargo.toml`
- Create: `sovmem/src/lib.rs`
- Create: `sovmem/src/main.rs`
- Create: `sovmem/tests/smoke.rs`
- Create: `sovmem/tests/support/mod.rs`

**Interfaces:**
- Produces: `sovmem::PROFILE_SCHEMA_VERSION: &str` and a reusable `support::GitFixture` for later integration tests.

- [ ] **Step 1: Add a failing smoke test for the crate contract**

```rust
// sovmem/tests/smoke.rs
use sovmem::PROFILE_SCHEMA_VERSION;

#[test]
fn exposes_the_approved_profile_schema() {
    assert_eq!(PROFILE_SCHEMA_VERSION, "nous-ssa-context-v0");
}
```

- [ ] **Step 2: Run the test before the crate exists**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test smoke`

Expected: FAIL because `sovmem/Cargo.toml` does not exist.

- [ ] **Step 3: Create the pinned crate and minimal library**

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.84.0"
profile = "minimal"
components = ["clippy", "rustfmt"]
```

```toml
# sovmem/Cargo.toml
[package]
name = "sovmem"
version = "0.1.0"
edition = "2021"
rust-version = "1.84"

[dependencies]
anyhow = "=1.0.95"
blake3 = "=1.5.5"
clap = { version = "=4.5.27", features = ["derive"] }
serde = { version = "=1.0.217", features = ["derive"] }
serde_json = "=1.0.138"
serde_yaml = "=0.9.34"
thiserror = "=2.0.11"

[lib]
path = "src/lib.rs"

[[bin]]
name = "sovmem"
path = "src/main.rs"
```

```rust
// sovmem/src/lib.rs
pub const PROFILE_SCHEMA_VERSION: &str = "nous-ssa-context-v0";
```

```rust
// sovmem/src/main.rs
fn main() {
    eprintln!("sovmem profile commands are not wired yet");
    std::process::exit(2);
}
```

- [ ] **Step 4: Add the standard-library Git fixture helper**

`GitFixture` MUST create a unique directory beneath `std::env::temp_dir()`, initialize Git, set local author identity, write fixture files, make dated commits by setting `GIT_AUTHOR_DATE` and `GIT_COMMITTER_DATE`, expose `head()`, and remove the directory in `Drop`. Every Git command MUST use `std::process::Command` with argument arrays; tests MUST NOT invoke a shell.

```rust
// public surface in sovmem/tests/support/mod.rs
pub struct GitFixture {
    pub root: std::path::PathBuf,
}

impl GitFixture {
    pub fn new(name: &str) -> Self;
    pub fn write(&self, path: &str, body: &str);
    pub fn commit_all(&self, message: &str, date: &str);
    pub fn head(&self) -> String;
    pub fn read(&self, path: &str) -> String;
    pub fn hash_worktree(&self) -> std::collections::BTreeMap<String, String>;
}
```

- [ ] **Step 5: Format and run the smoke test**

Run: `cargo fmt --manifest-path sovmem/Cargo.toml --check`

Expected: exit 0.

Run: `cargo test --manifest-path sovmem/Cargo.toml --test smoke`

Expected: 1 passed, 0 failed.

- [ ] **Step 6: Commit the bootstrap**

```bash
git add rust-toolchain.toml sovmem/Cargo.toml sovmem/Cargo.lock sovmem/src/lib.rs sovmem/src/main.rs sovmem/tests/smoke.rs sovmem/tests/support/mod.rs
git commit -m "build: bootstrap sovmem profile crate"
```

### Task 2: Load and validate isolated profile configuration

**Files:**
- Create: `sovmem/src/profile/mod.rs`
- Create: `sovmem/src/profile/config.rs`
- Modify: `sovmem/src/lib.rs`
- Create: `profiles/audy-ssa0-v0/profile.yaml`
- Create: `sovmem/tests/profile_config.rs`

**Interfaces:**
- Produces: `load_profile(path: &Path) -> Result<ProfileConfig, ProfileError>`.
- Produces: `load_canary(path: &Path) -> Result<CanaryConfig, ProfileError>`.
- Produces: `ProfileConfig::validate() -> Result<(), ProfileError>` and `ProfileConfig::mapping_for(path: &str) -> Option<&MappingRule>`.
- Consumes: `PROFILE_SCHEMA_VERSION` from Task 1.

- [ ] **Step 1: Write failing tests for valid identity and isolation failures**

```rust
fn profile_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../profiles/audy-ssa0-v0/profile.yaml")
}

#[test]
fn loads_the_audy_reference_profile() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    assert_eq!(profile.nous_schema, "nous-ssa-context-v0");
    assert_eq!(profile.profile, "nous-sovmem-v0");
    assert_eq!(profile.ssa_id, "ssa-0");
    assert_eq!(profile.ssa_profile, "audy-ssa0-v0");
    assert_eq!(profile.familiar_id, "audy");
    assert_eq!(profile.context_namespace, "ssa-0");
}

#[test]
fn rejects_an_identity_namespace_mismatch() {
    let mut profile = load_profile(&profile_path()).expect("profile loads");
    profile.context_namespace = "ssa-1".into();
    let error = profile.validate().expect_err("mismatch must fail");
    assert_eq!(error.code(), "profile.namespace_mismatch");
}
```

Add table-driven cases rejecting an empty identifier, an unsupported core type, a kind mapped to no core type, duplicate mapping precedence, and a `vault_id` containing `/`.

- [ ] **Step 2: Run the profile tests to verify failure**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test profile_config`

Expected: FAIL because the profile module and YAML do not exist.

- [ ] **Step 3: Define the exact shared types**

```rust
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CoreType { Concept, Resource, Judgment }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Layer { Synthesis, Analysis, Concept }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Authority { Canonical, Reviewed, Working, Raw }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Sensitivity { Public, Internal, Private, Restricted }

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MappingRule {
    pub pattern: String,
    pub core_type: CoreType,
    pub context_kind: String,
    pub layer: Layer,
    pub authority: Authority,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SensitivityRule {
    pub pattern: String,
    pub sensitivity: Sensitivity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProfileConfig {
    pub okf_version: String,
    pub profile: String,
    pub nous_schema: String,
    pub ssa_id: String,
    pub ssa_profile: String,
    pub familiar_id: String,
    pub context_namespace: String,
    pub vault_id: String,
    pub context_kinds: std::collections::BTreeMap<String, CoreType>,
    pub mapping_rules: Vec<MappingRule>,
    pub sensitivity_rules: Vec<SensitivityRule>,
    pub default_sensitivity: Sensitivity,
    pub public_allowlist: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceMode { Committed }

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CanaryConfig {
    pub mode: SourceMode,
    pub source_commit: String,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}
```

Define `ProfileError` with stable codes for `profile.parse`, `profile.schema`, `profile.namespace_mismatch`, `profile.identifier`, `profile.kind_mapping`, `profile.mapping_precedence`, `canary.mode`, `canary.commit`, and `canary.selection`.

- [ ] **Step 4: Add the approved Audy profile YAML**

The YAML MUST contain all 23 approved `context_kind` values, each mapped to `concept` or `resource`; `Judgment` is reserved for explicit human verdict records only. It MUST encode the mapping table from the approved design in most-specific-first order and reject equal-specificity overlaps.

```yaml
okf_version: "0.1"
profile: nous-sovmem-v0
nous_schema: nous-ssa-context-v0
ssa_id: ssa-0
ssa_profile: audy-ssa0-v0
familiar_id: audy
context_namespace: ssa-0
vault_id: audy-vault
context_kinds:
  Doctrine: concept
  Identity: concept
  Operating Contract: concept
  Memory Index: concept
  Health Report: concept
  Daily Log: concept
  Protocol: concept
  Person: concept
  Harness Lane: concept
  Project Brief: concept
  Research Brief: concept
  Design Brief: concept
  Source Snapshot: resource
  Concept Map: concept
  Gap Analysis: concept
  Adoption Spec: concept
  Profile Spec: concept
  Promotion Candidate Report: concept
  Loop Definition: concept
  Loop Output: concept
  Decision Log: concept
  Handoff: concept
  Runtime Note: concept
mapping_rules:
  - { pattern: "01-Soul/Audy Himura.md", core_type: concept, context_kind: Identity, layer: synthesis, authority: canonical }
  - { pattern: "AGENTS.md", core_type: concept, context_kind: "Operating Contract", layer: synthesis, authority: canonical }
  - { pattern: "OPERATIONS.md", core_type: concept, context_kind: "Operating Contract", layer: synthesis, authority: canonical }
  - { pattern: "00-Index.md", core_type: concept, context_kind: "Memory Index", layer: synthesis, authority: canonical }
  - { pattern: "02-Memory/MEMORY.md", core_type: concept, context_kind: "Memory Index", layer: synthesis, authority: canonical }
  - { pattern: "04-Research/**/source-pages/*.md", core_type: resource, context_kind: "Source Snapshot", layer: concept, authority: raw }
  - { pattern: "04-Research/**/*Concept-Map*.md", core_type: concept, context_kind: "Concept Map", layer: analysis, authority: working }
  - { pattern: "04-Research/**/*Gap-Analysis*.md", core_type: concept, context_kind: "Gap Analysis", layer: analysis, authority: working }
  - { pattern: "04-Research/**/*Adoption-Spec*.md", core_type: concept, context_kind: "Adoption Spec", layer: analysis, authority: working }
  - { pattern: "04-Research/**/*Profile*.md", core_type: concept, context_kind: "Profile Spec", layer: analysis, authority: working }
  - { pattern: "04-Research/**/README.md", core_type: concept, context_kind: "Research Brief", layer: analysis, authority: working }
  - { pattern: "01-Soul/*.md", core_type: concept, context_kind: Doctrine, layer: synthesis, authority: canonical }
  - { pattern: "03-Protocols/*.md", core_type: concept, context_kind: Protocol, layer: concept, authority: reviewed }
  - { pattern: "08-Hermes/**/*.md", core_type: concept, context_kind: "Harness Lane", layer: analysis, authority: working }
  - { pattern: "09-Claude/**/*.md", core_type: concept, context_kind: "Harness Lane", layer: analysis, authority: working }
  - { pattern: "10-Codex/**/*.md", core_type: concept, context_kind: "Harness Lane", layer: analysis, authority: working }
  - { pattern: "11-Antigravity/**/*.md", core_type: concept, context_kind: "Harness Lane", layer: analysis, authority: working }
  - { pattern: "05-Daily/*.md", core_type: concept, context_kind: "Daily Log", layer: analysis, authority: raw }
  - { pattern: "loops/**/references/*.md", core_type: concept, context_kind: "Loop Definition", layer: concept, authority: reviewed }
  - { pattern: "loops/**/archive/*.md", core_type: concept, context_kind: "Loop Output", layer: analysis, authority: raw }
sensitivity_rules:
  - { pattern: "02-Memory/**", sensitivity: private }
  - { pattern: "05-Daily/**", sensitivity: private }
  - { pattern: "08-Hermes/**", sensitivity: private }
  - { pattern: "09-Claude/**", sensitivity: private }
  - { pattern: "10-Codex/**", sensitivity: private }
  - { pattern: "11-Antigravity/**", sensitivity: private }
  - { pattern: "01-Soul/**", sensitivity: internal }
  - { pattern: "03-Protocols/**", sensitivity: internal }
  - { pattern: "04-Research/**/source-pages/**", sensitivity: internal }
  - { pattern: "04-Research/**", sensitivity: internal }
default_sensitivity: internal
public_allowlist: []
```

- [ ] **Step 5: Implement loading, validation, and deterministic glob matching**

`load_profile` MUST use `serde_yaml::from_str`, call `validate`, and never accept profile identity from source notes. The glob matcher MUST support literal segments, `*` within one segment, and `**` across segments. `mapping_for` and sensitivity matching MUST evaluate rules in stored order and fail profile loading when two rules have equal specificity and can match the same literal fixture path.

```rust
pub fn load_profile(path: &Path) -> Result<ProfileConfig, ProfileError>;
pub fn load_canary(path: &Path) -> Result<CanaryConfig, ProfileError>;
pub fn glob_matches(pattern: &str, path: &str) -> bool;
impl ProfileConfig {
    pub fn validate(&self) -> Result<(), ProfileError>;
    pub fn mapping_for(&self, path: &str) -> Option<&MappingRule>;
    pub fn sensitivity_for(&self, path: &str) -> Sensitivity;
}
```

- [ ] **Step 6: Run and commit**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test profile_config`

Expected: all profile tests pass.

```bash
git add sovmem/src/lib.rs sovmem/src/profile profiles/audy-ssa0-v0/profile.yaml sovmem/tests/profile_config.rs
git commit -m "feat: define isolated SSA profile configuration"
```

### Task 3: Read a pinned committed source snapshot without touching vault files

**Files:**
- Create: `sovmem/src/profile/source.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Create: `sovmem/tests/source_snapshot.rs`

**Interfaces:**
- Produces: `open_committed_snapshot(vault_root, expected_commit, paths) -> Result<SourceSnapshot, SourceError>`.
- Produces: `SourceDocument { source_path, bytes, source_hash, created, updated }` sorted by `source_path`.
- Consumes: `GitFixture` from Task 1.

- [ ] **Step 1: Write failing committed-snapshot tests**

Create a clean fixture with `Research/One.md` committed on `2026-01-02` and updated on `2026-01-05`. Read the first commit before making any working-tree modification and assert:

```rust
let snapshot = open_committed_snapshot(&repo.root, &first_commit, &["Research/One.md".into()])?;
assert_eq!(snapshot.commit, first_commit);
assert_eq!(snapshot.documents[0].created, "2026-01-02");
assert_eq!(snapshot.documents[0].updated, "2026-01-02");
assert_eq!(snapshot.documents[0].bytes, b"# One

Committed body.
");
```

In a separate case, modify the selected path after both commits and prove it is reported and excluded in committed canary mode. Add cases proving an untracked selection is reported and excluded, a missing commit fails with `source.commit_missing`, a missing tracked path fails with `source.path_missing`, and every read leaves the before/after worktree hash map identical.

- [ ] **Step 2: Run the source tests to verify failure**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test source_snapshot`

Expected: FAIL because `open_committed_snapshot` is undefined.

- [ ] **Step 3: Implement Git object reads and dates**

Use `Command::new("git").arg("-C").arg(vault_root)` for every call. Resolve the commit with `rev-parse --verify <commit>^{commit}`; compare the full result with `expected_commit`; obtain selected-path status with `status --porcelain=v1 --untracked-files=all -- <paths>`; read bytes with `git show <commit>:<path>`; obtain `created` from the oldest `git log --follow --diff-filter=A --format=%cs <commit> -- <path>` result and `updated` from `git log -1 --format=%cs <commit> -- <path>`. Missing dates are hard failures. Hash raw source bytes with BLAKE3.

```rust
pub struct SourceSnapshot {
    pub vault_root: PathBuf,
    pub commit: String,
    pub documents: Vec<SourceDocument>,
    pub excluded: Vec<ExcludedSource>,
}

pub fn open_committed_snapshot(
    vault_root: &Path,
    expected_commit: &str,
    paths: &[String],
) -> Result<SourceSnapshot, SourceError>;
```

- [ ] **Step 4: Run and commit**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test source_snapshot`

Expected: all snapshot tests pass and fixture worktrees remain byte-identical.

```bash
git add sovmem/src/profile/source.rs sovmem/src/profile/mod.rs sovmem/tests/source_snapshot.rs
git commit -m "feat: read immutable vault source snapshots"
```

### Task 4: Normalize Markdown and generated frontmatter deterministically

**Files:**
- Create: `sovmem/src/profile/frontmatter.rs`
- Create: `sovmem/src/profile/mapping.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Create: `sovmem/tests/normalization.rs`

**Interfaces:**
- Produces: `parse_source_document(SourceDocument) -> Result<ParsedSource, FrontmatterError>`.
- Produces: `normalize_document(profile, parsed) -> Result<GeneratedDocument, NormalizeError>`.
- Produces: `render_document(&GeneratedDocument) -> Result<Vec<u8>, FrontmatterError>`.
- Consumes: profile mappings from Task 2 and Git dates/hashes from Task 3.

- [ ] **Step 1: Write failing normalization tests**

Cover the approved example exactly: `04-Research/OKF/OKF-Concept-Map.md` maps to `type: concept`, `context_kind: Concept Map`, `layer: analysis`, `authority: working`, namespace-prefixed ID/resource, H1 title, deterministic first-sentence description, `internal` sensitivity, sorted tags and links, and the exact `inferred_fields` list.

Add cases for:

- explicit valid metadata overriding a mapping default;
- conflicting explicit `ssa_id`, `ssa_profile`, `familiar_id`, `context_namespace`, `profile`, or `nous_schema` failing instead of being overwritten;
- filename title fallback when no H1 exists;
- Markdown syntax stripping and a 240-character description cap;
- explicit and Git-derived dates, with no filesystem-time fallback;
- source snapshot mapping to `resource`;
- unmapped paths failing with `normalize.mapping_missing`;
- verdict-like prose remaining `concept`;
- unknown source extension fields surviving round-trip.

```rust
assert_eq!(generated.metadata.id, "ssa-0/04-Research/OKF/OKF-Concept-Map");
assert_eq!(generated.metadata.resource, "vault://ssa-0/04-Research/OKF/OKF-Concept-Map");
assert_eq!(generated.metadata.core_type, CoreType::Concept);
assert_eq!(generated.metadata.context_kind, "Concept Map");
```

- [ ] **Step 2: Run the tests to verify failure**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test normalization`

Expected: FAIL because parsing and normalization are undefined.

- [ ] **Step 3: Implement the generated metadata contract**

Define `GeneratedMetadata` with fields in the approved YAML order: `okf_version`, `profile`, `nous_schema`, `ssa_id`, `ssa_profile`, `familiar_id`, `context_namespace`, `id`, `type` (serialized from `core_type`), `context_kind`, `title`, `description`, `layer`, `source`, `source_path`, `resource`, `source_kind`, `provenance`, `status`, `authority`, `sensitivity`, `created`, `updated`, `tags`, `links`, `inferred_fields`, optional lifecycle/judgment fields, then a flattened `BTreeMap<String, serde_yaml::Value>` for unknown extensions.

```rust
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind { VaultNote, SourceSnapshot, GeneratedReport, ExternalReference }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provenance { Tier1, Secondary, Synthesis, Unknown }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status { Active, Review, Draft, Generated, Superseded, Archived }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum LinkRelation { Cites, DerivedFrom, Supersedes, DependsOn, References }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypedLink {
    pub rel: LinkRelation,
    pub target: String,
}
```

```rust
pub fn parse_source_document(source: SourceDocument) -> Result<ParsedSource, FrontmatterError>;
pub fn normalize_document(
    profile: &ProfileConfig,
    source: ParsedSource,
) -> Result<GeneratedDocument, NormalizeError>;
pub fn render_document(document: &GeneratedDocument) -> Result<Vec<u8>, FrontmatterError>;
```

IDs and `vault://` resources MUST percent-encode each path segment using UTF-8 bytes while leaving RFC 3986 unreserved bytes unchanged. Tags MUST be lowercase, deduplicated, and sorted. `inferred_fields` MUST be emitted in generated-field order, not alphabetically.

When explicit valid values are absent, inference MUST use these fixed rules: `source_kind=source-snapshot` and `provenance=unknown` for `Source Snapshot`; otherwise `source_kind=vault-note` and `provenance=synthesis` for the selected research syntheses. `authority=canonical` implies `status=active`, `authority=reviewed` implies `status=review`, and `authority=working|raw` implies `status=draft`. `source` and `resource` are the profile-scoped vault URI for vault-native canary notes. No provenance tier higher than `synthesis` may be inferred from prose.

- [ ] **Step 4: Implement conservative sensitivity and secret detection**

Path defaults MUST match the approved table. A public scope is allowlist-only. Mark and exclude as `restricted` any document with explicit restricted sensitivity or lines beginning, after ASCII whitespace and case-folding, with `api_key:`, `token:`, `password:`, `secret:`, or containing a PEM private-key header. Research prose containing those words without an assignment prefix MUST not trigger.

- [ ] **Step 5: Run and commit**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test normalization`

Expected: all normalization tests pass.

```bash
git add sovmem/src/profile/frontmatter.rs sovmem/src/profile/mapping.rs sovmem/src/profile/mod.rs sovmem/tests/normalization.rs
git commit -m "feat: normalize Audy notes into SovMem documents"
```

### Task 5: Convert links and reject cross-profile references

**Files:**
- Create: `sovmem/src/profile/links.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Create: `sovmem/tests/link_conversion.rs`

**Interfaces:**
- Produces: `convert_links(profile, documents) -> Result<LinkConversionResult, LinkError>`.
- Consumes: normalized profile-scoped IDs and source paths from Task 4.

- [ ] **Step 1: Write failing link-conversion tests**

```rust
assert_eq!(converted.body, "See [Map](../OKF/OKF-Concept-Map.md).
");
assert_eq!(converted.metadata.links[0].rel, LinkRelation::References);
assert_eq!(converted.metadata.links[0].target, "ssa-0/04-Research/OKF/OKF-Concept-Map");
```

Add cases for aliased wikilinks, heading fragments, relative Markdown links, HTTP(S) links as `cites`, explicit typed links preserved, duplicate links deduplicated, broken targets warning, duplicate-basename ambiguity warning, and any `ssa-1/` or `vault://ssa-1/` target failing with `links.cross_profile`.

- [ ] **Step 2: Run the tests to verify failure**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test link_conversion`

Expected: FAIL because `convert_links` is undefined.

- [ ] **Step 3: Implement deterministic link resolution**

Build exact-path and basename indexes from the selected generated documents. Resolve exact relative paths before unique basenames. Convert resolvable wikilinks to standard Markdown body links, preserve fragments, and emit sorted `{rel, target}` metadata. Leave ambiguous or broken source text unchanged and append a stable warning. Do not infer `supersedes` or `depends_on` from prose; only preserve those relations from explicit valid metadata.

```rust
pub fn convert_links(
    profile: &ProfileConfig,
    documents: Vec<GeneratedDocument>,
) -> Result<LinkConversionResult, LinkError>;
```

- [ ] **Step 4: Run and commit**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test link_conversion`

Expected: all link tests pass.

```bash
git add sovmem/src/profile/links.rs sovmem/src/profile/mod.rs sovmem/tests/link_conversion.rs
git commit -m "feat: resolve profile-scoped document links"
```

### Task 6: Validate documents and bundles before any output is published

**Files:**
- Create: `sovmem/src/profile/validate.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Create: `sovmem/tests/validation.rs`

**Interfaces:**
- Produces: `validate_document(profile, document) -> ValidationReport`.
- Produces: `validate_bundle(profile, documents) -> ValidationReport`.
- Consumes: generated documents and diagnostics from Tasks 4-5.

- [ ] **Step 1: Write failing validator tests for every hard-failure class**

Use one table-driven test that mutates a known-valid generated document and asserts stable diagnostic codes for missing fields, invalid enums, duplicate IDs/resources, namespace mismatch, cross-profile target, timestamp absence, restricted scope violation, malformed judgment, empty judgment reason, invalid verdict, and conflicting explicit metadata.

```rust
let report = validate_bundle(&profile, &[first, duplicate]);
assert!(report.has_errors());
assert_eq!(report.errors()[0].code, "bundle.duplicate_id");
```

Add warning assertions for unknown `context_kind`, unresolved links, inferred description/provenance, weak research citations, and selected dirty-file exclusion.

- [ ] **Step 2: Run the tests to verify failure**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test validation`

Expected: FAIL because validation functions are undefined.

- [ ] **Step 3: Implement stable diagnostics and validation**

```rust
#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity { Warning, Error }

#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub source_path: Option<String>,
    pub message: String,
}

pub fn validate_document(profile: &ProfileConfig, document: &GeneratedDocument) -> ValidationReport;
pub fn validate_bundle(profile: &ProfileConfig, documents: &[GeneratedDocument]) -> ValidationReport;
```

Sort diagnostics by severity, code, source path, then message. Validation MUST be side-effect-free. A judgment MUST have an explicit human-produced `verdict` in `ACCEPT|REJECT|REWRITE|TP|FP`, non-empty `reason`, and profile-scoped `subject`; no mapping rule may create a judgment.

- [ ] **Step 4: Run and commit**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test validation`

Expected: all validation tests pass.

```bash
git add sovmem/src/profile/validate.rs sovmem/src/profile/mod.rs sovmem/tests/validation.rs
git commit -m "feat: validate generated SovMem profile bundles"
```

### Task 7: Orchestrate deterministic staging, manifesting, and publication

**Files:**
- Create: `sovmem/src/profile/export.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Create: `sovmem/tests/export_determinism.rs`

**Interfaces:**
- Produces: `export_canary(request: ExportRequest) -> Result<ExportSummary, ExportError>`.
- Consumes: all profile subsystem interfaces from Tasks 2-6.

- [ ] **Step 1: Write failing end-to-end library tests**

Create a 3-document committed Git fixture and export it twice to separate roots. Assert every relative output path and byte sequence is identical; source worktree hashes match before and after; output contains `bundle/index.md`, three generated notes, `manifest.json`, and `export-report.json`; and the manifest document hashes match rendered bytes.

```rust
assert_eq!(tree_bytes(&first_output), tree_bytes(&second_output));
assert_eq!(source_before, fixture.hash_worktree());
assert_eq!(summary.exported, 3);
assert_eq!(summary.errors, 0);
```

Add cases proving validation failure publishes no output, an output root inside the vault fails, source-commit mismatch fails, and a 50-document fixture exports without schema changes.

- [ ] **Step 2: Run the tests to verify failure**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test export_determinism`

Expected: FAIL because `export_canary` is undefined.

- [ ] **Step 3: Implement the export transaction**

```rust
pub struct ExportRequest {
    pub vault_root: PathBuf,
    pub output_root: PathBuf,
    pub profile_path: PathBuf,
    pub canary_path: PathBuf,
}

pub fn export_canary(request: ExportRequest) -> Result<ExportSummary, ExportError>;
```

The transaction order MUST be: canonicalize and separate input/output roots; load profile/canary; open committed snapshot; parse; normalize; resolve links; validate in memory; create a temporary sibling directory; render sorted documents; create sorted `bundle/index.md`; calculate raw-byte BLAKE3 hashes; serialize pretty JSON with terminal newline; fsync files; replace the requested output directory only after validation succeeds. If the requested output already exists, move it to a temporary backup, publish the new tree, then remove the backup; restore the backup on publication failure.

`manifest.json` MUST include schema/profile identities, vault ID, commit, committed mode, inclusion list, exclusion list, source hashes, generated document IDs/resources/hashes, and a bundle hash. `export-report.json` MUST include deterministic counts and sorted diagnostics only; it MUST NOT include wall-clock time, machine name, mounted path, or username.

- [ ] **Step 4: Run and commit**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test export_determinism`

Expected: all export tests pass, including the 50-document fixture.

```bash
git add sovmem/src/profile/export.rs sovmem/src/profile/mod.rs sovmem/tests/export_determinism.rs
git commit -m "feat: export deterministic validated canary bundles"
```

### Task 8: Wire the CLI and pin the real 13-document Audy canary

**Files:**
- Create: `sovmem/src/cli.rs`
- Modify: `sovmem/src/main.rs`
- Modify: `sovmem/src/lib.rs`
- Create: `profiles/audy-ssa0-v0/canary.yaml`
- Create: `sovmem/tests/cli.rs`

**Interfaces:**
- Produces: `sovmem export --vault <path> --profile <path> --canary <path> --output <path>`.
- Produces: `sovmem verify --profile <path> --bundle <output-root>`.
- Consumes: `export_canary` and `validate_bundle`.

- [ ] **Step 1: Write failing CLI tests**

Invoke the compiled binary with `std::process::Command`. Assert export success prints one JSON summary to stdout and no source content; invalid profile, commit mismatch, and output-inside-vault return exit 2; valid verify returns exit 0; tampered generated Markdown returns exit 1 and `bundle.hash_mismatch` on stderr.

```rust
let output = Command::new(env!("CARGO_BIN_EXE_sovmem"))
    .args(["export", "--vault"])
    .arg(&fixture.root)
    .args(["--profile", profile_path, "--canary", canary_path, "--output"])
    .arg(&output_root)
    .output()?;
assert!(output.status.success());
```

- [ ] **Step 2: Run the CLI tests to verify failure**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test cli`

Expected: FAIL because CLI dispatch is not implemented.

- [ ] **Step 3: Define and dispatch the two commands**

```rust
#[derive(clap::Parser)]
#[command(name = "sovmem", version, about = "Sovereign memory profile tools")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand)]
pub enum Command {
    Export { vault: PathBuf, profile: PathBuf, canary: PathBuf, output: PathBuf },
    Verify { profile: PathBuf, bundle: PathBuf },
}
```

`main` MUST print deterministic JSON success summaries to stdout, diagnostics to stderr, and return 0 for success, 1 for invalid/tampered bundle verification, and 2 for configuration, source, or usage failures. It MUST never print note bodies or secrets.

- [ ] **Step 4: Pin the real canary manifest**

```yaml
mode: committed
source_commit: 43824cfd9caed7b260aa442128fd1130ddb6e2ff
include:
  - 04-Research/OKF/README.md
  - 04-Research/OKF/Audy-OKF-Profile-Draft.md
  - 04-Research/OKF/OKF-Adoption-Spec.md
  - 04-Research/OKF/OKF-Audy-Vault-Gap-Analysis.md
  - 04-Research/OKF/OKF-Concept-Map.md
  - 04-Research/AVM/README.md
  - 04-Research/AVM/AVM-Adoption-Spec.md
  - 04-Research/DSOM/README.md
  - 04-Research/DSOM/DSOM-Adoption-Spec.md
  - 04-Research/DSOM/DSOM-Concept-Map.md
  - 04-Research/DSOM/DSOM-TFIS-SSA-Gap-Analysis.md
  - 04-Research/Agent-Loop-Engineering/Loop-Engineering-Concept-Map.md
  - 04-Research/Agent-Loop-Engineering/TFIS-Daily-Court-Gap-Analysis.md
exclude:
  - "**/source-pages/**"
  - "02-Memory/**"
  - "05-Daily/**"
  - "08-Hermes/**"
  - "09-Claude/**"
  - "10-Codex/**"
  - "11-Antigravity/**"
```

Add a config test loading this file and asserting exactly 13 unique included paths, no included path matches an exclusion, and every included path has a deterministic `mapping_for` result in `profile.yaml`.

- [ ] **Step 5: Run and commit**

Run: `cargo test --manifest-path sovmem/Cargo.toml --test cli`

Expected: all CLI tests pass.

```bash
git add sovmem/src/cli.rs sovmem/src/main.rs sovmem/src/lib.rs profiles/audy-ssa0-v0/canary.yaml sovmem/tests/cli.rs
git commit -m "feat: add Audy canary export and verify commands"
```

### Task 9: Prove default-profile and cross-profile isolation

**Files:**
- Create: `sovmem/tests/fixtures/default-ssa-v0/profile.yaml`
- Create: `sovmem/tests/fixtures/default-ssa-v0/source/Research/Welcome.md`
- Create: `sovmem/tests/default_profile_isolation.rs`

**Interfaces:**
- Consumes: the same profile loader and exporter used by Audy; no default-profile-only code path is permitted.
- Produces: a reusable leak-scan assertion for future onboarding work.

- [ ] **Step 1: Write the failing isolation test**

Export a neutral fixture using `ssa_id: test-ssa`, `ssa_profile: default-ssa-v0`, `familiar_id: familiar`, and `context_namespace: test-ssa`. Concatenate all output bytes after ASCII case-folding and assert none of these forbidden tokens appear: `ssa-0`, `khalil`, `audy`, `/volumes/himura`, `honcho`, `verdict`, `trust score`, `private memory`.

```rust
for forbidden in FORBIDDEN_DEFAULT_PROFILE_TOKENS {
    assert!(!exported_text.contains(forbidden), "leaked token: {forbidden}");
}
```

Add a second test inserting `vault://ssa-0/04-Research/Foreign` into the neutral source and assert export fails with `links.cross_profile` and publishes no bundle.

- [ ] **Step 2: Run the test to verify it catches a seeded leak**

Temporarily seed `Audy` in the neutral fixture, run `cargo test --manifest-path sovmem/Cargo.toml --test default_profile_isolation`, and confirm the test fails with `leaked token: audy`. Remove the seeded value before continuing.

- [ ] **Step 3: Complete the neutral fixture and run the tests**

The neutral profile MUST contain only generic structure, empty trust/ledger/runtime fields, the same `nous-ssa-context-v0` schema, and a `Research Brief -> concept` mapping. The source note MUST contain generic onboarding prose with no TFIS private context or Audy identifiers.

Run: `cargo test --manifest-path sovmem/Cargo.toml --test default_profile_isolation`

Expected: 2 passed, 0 failed.

- [ ] **Step 4: Commit**

```bash
git add sovmem/tests/fixtures/default-ssa-v0 sovmem/tests/default_profile_isolation.rs
git commit -m "test: prove SSA profile isolation"
```

### Task 10: Run the real canary and document the development boundary

**Files:**
- Modify: `.gitignore`
- Modify: `package.json`
- Modify: `README.md`
- Modify: `index.md`

**Interfaces:**
- Produces: root commands `npm run test:js`, `npm run test:sovmem`, `npm test`, `npm run canary:audy`, and `npm run verify:audy`.
- Consumes: the mounted vault path only through the `AUDY_VAULT` environment variable.

- [ ] **Step 1: Add root verification scripts and ignore generated output**

```json
"scripts": {
  "start": "node src/index.js",
  "test:js": "node --test test/*.test.js",
  "test:sovmem": "cargo test --manifest-path sovmem/Cargo.toml",
  "test": "npm run test:js && npm run test:sovmem",
  "canary:audy": "cargo run --manifest-path sovmem/Cargo.toml -- export --vault $AUDY_VAULT --profile profiles/audy-ssa0-v0/profile.yaml --canary profiles/audy-ssa0-v0/canary.yaml --output var/audy-sovmem-canary",
  "verify:audy": "cargo run --manifest-path sovmem/Cargo.toml -- verify --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-canary"
}
```

Append `/var/` to `.gitignore`. The command MUST fail clearly when `AUDY_VAULT` is unset; it MUST NOT embed Khalil's mount path in tracked configuration or output.

- [ ] **Step 2: Run static and automated verification**

Run: `cargo fmt --manifest-path sovmem/Cargo.toml --check`

Expected: exit 0.

Run: `cargo clippy --manifest-path sovmem/Cargo.toml --all-targets -- -D warnings`

Expected: exit 0 with no warnings.

Run: `npm test`

Expected: existing 3 Node tests plus all Rust tests pass.

- [ ] **Step 3: Run the real Audy canary twice**

Run: `AUDY_VAULT=/Volumes/himura/audy-vault npm run canary:audy`

Expected: exit 0, `exported: 13`, `errors: 0`, and generated output only under `var/audy-sovmem-canary`.

Run: `cp -R var/audy-sovmem-canary /tmp/audy-sovmem-canary-first`

Run: `AUDY_VAULT=/Volumes/himura/audy-vault npm run canary:audy`

Run: `diff -ru /tmp/audy-sovmem-canary-first var/audy-sovmem-canary`

Expected: exit 0 with no differences.

Run: `npm run verify:audy`

Expected: exit 0 with `documents: 13`, `errors: 0`.

- [ ] **Step 4: Prove the vault remained unchanged**

Run before and after the canary: `git -C "$AUDY_VAULT" status --porcelain=v1`

Expected: byte-for-byte identical output. Record the BLAKE3 source hashes from `manifest.json`; do not commit live-vault status or private paths.

- [ ] **Step 5: Update documentation and artifact registry**

README MUST state that the canary is a read-only generated interface, not canon or a full SovMem implementation. Document the two commands, environment variable, pinned commit, 13-document scope, and deferred capabilities. Update `index.md` current state and register the Rust crate, profile files, tests, and generated-output exclusion without promoting P1-P5 to implemented.

- [ ] **Step 6: Run final repository checks**

Run: `npm test`

Expected: all Node and Rust tests pass.

Run: `git diff --check`

Expected: exit 0.

Run the repository local-link and artifact-registry validators documented by the root-index implementation plan.

Expected: all links resolve and every durable unignored file is registered.

- [ ] **Step 7: Commit the completed canary slice**

```bash
git add .gitignore package.json README.md index.md rust-toolchain.toml sovmem profiles
git commit -m "feat: deliver read-only Audy SovMem canary"
```

## Completion Gate

The implementation is complete only when all of the following are simultaneously true:

1. The 13 selected documents export from commit `43824cfd9caed7b260aa442128fd1130ddb6e2ff` with zero hard failures.
2. Two exports are byte-identical.
3. Source worktree status and hashes are unchanged.
4. The generated bundle validates its profile, namespace, document hashes, and bundle hash.
5. Dirty and untracked selected paths are excluded and reported.
6. Restricted content, invalid judgments, duplicate IDs/resources, source mismatch, output-inside-vault, and cross-profile references fail closed.
7. The 50-document synthetic expansion passes without a schema change.
8. The neutral default-profile fixture contains none of the forbidden SSA-0/Audy tokens.
9. `cargo fmt`, `cargo clippy -D warnings`, `npm test`, local-link validation, registry coverage, and `git diff --check` all pass.
10. No Tantivy, Petgraph, MCP, Honcho synchronization, verdict application, ledger mutation, trust scoring, or live-vault write path exists in this slice.
