> **ARCHIVED** — This implementation plan has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix A: Implementation Plan Index](../sovmem-master-spec.md#appendix-a-implementation-plan-index) for a summary of its scope.
>
# Audy SovMem Retrieval Corpus and Golden Queries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **2026-07-12 v1 supersession:** The [approved v1 design](../specs/2026-07-12-audy-sovmem-retrieval-corpus-v1-design.md) and [v1 implementation plan](../../../.superpowers/sdd/task-1-brief.md) supersede the active corpus contract. Completed v0 history below remains unchanged. All pending Task 6-8 commands and durable references must use `retrieval-corpus-v1.yaml` and schema `audy-retrieval-corpus-v1`.

**Goal:** Build a deterministic corpus-first SovMem milestone that exports Audy's eight canonical-core documents plus at least forty-five reviewed research documents, emits a strict coverage report, and validates at least twenty-five human-authored golden queries without implementing retrieval.

**Architecture:** Add a corpus configuration boundary beside the existing canary configuration, then reuse the hardened committed-source and atomic export pipeline through a shared selection path. A corpus gate derives accepted core/research counts and themes from generated documents plus explicit source exclusions, writes `corpus-report.json` before atomic publication, and fails closed below the approved quotas. A separate strict JSONL validator checks human-authored golden queries against the exported manifest and corpus report; it does not execute or tune retrieval.

**Tech Stack:** Rust 1.84.0; existing `clap`, `serde`, `serde_yaml`, `serde_json`, `blake3`, `anyhow`, and `thiserror`; Git CLI for immutable source reads; Node.js 20 root scripts; Markdown and JSONL evaluation artifacts.

## Global Constraints

- The approved design is `docs/superpowers/specs/2026-07-12-audy-sovmem-retrieval-corpus-design.md`.
- The normative build authority remains `docs/sovmem_v0.3_execution_plan.md`; profile identity and isolation remain governed by `docs/superpowers/specs/2026-07-10-audy-sovmem-profile-v0-design.md`.
- Source mode is committed-only at `43824cfd9caed7b260aa442128fd1130ddb6e2ff`.
- Audy Vault is canonical and read-only. Generated output remains derived under ignored `var/`.
- Existing identifiers and schemas remain exactly `nous-ssa-context-v0`, `nous-sovmem-v0`, `ssa-0`, `audy-ssa0-v0`, `audy`, and namespace `ssa-0`.
- No new core type, context kind, profile identity, dependency, model call, working-tree fallback, filesystem timestamp, or live-vault write is allowed.
- The exact core set is eight documents. The research manifest has 55 candidates. At least 45 research documents and 53 total documents must export.
- Theme minima are `tfis-ssa: 10`, `sovereign-context: 15`, `agent-operations: 10`, and `governance-evidence: 10`; one research document has exactly one primary theme.
- Source exclusions are soft only for non-core research paths already reported as dirty or untracked by the existing committed-source boundary. Core exclusion and every content, privacy, identity, namespace, or schema error remain hard failures.
- Golden queries are authored or explicitly approved by Khalil. Agents must not generate or expand query text or relevance labels.
- Tantivy, Petgraph, BM25, graph expansion, ranking, metrics, threshold tuning, and `baseline.json` are out of scope.

## File Structure

| Path | Responsibility |
| --- | --- |
| `profiles/audy-ssa0-v0/retrieval-corpus.yaml` | Exact eight-core and 55-research selection contract, themes, quotas, exclusions, and pinned commit. |
| `profiles/audy-ssa0-v0/profile.yaml` | Add five exact mappings using only existing context kinds for selected research files not covered by current rules. |
| `sovmem/src/profile/corpus.rs` | Deserialize and validate corpus configuration; expose exact selection and theme lookup. |
| `sovmem/src/profile/corpus_report.rs` | Enforce core/count/theme gates and serialize deterministic coverage evidence. |
| `sovmem/src/profile/golden.rs` | Strictly load and validate human-authored golden JSONL against a corpus bundle. |
| `sovmem/src/profile/export.rs` | Reuse one hardened selection/export transaction for canary and corpus modes; atomically write `corpus-report.json`. |
| `sovmem/src/profile/mod.rs` | Register new modules and shared strict report structures. |
| `sovmem/src/cli.rs` | Add `corpus` and `verify-golden` commands with redacted JSON output and stable exit classes. |
| `sovmem/tests/corpus_config.rs` | Cover exact manifest, core set, quotas, themes, forbidden lanes, duplicates, and mapping coverage. |
| `sovmem/tests/corpus_report.rs` | Cover core/count/theme gates, exclusions, deterministic ordering, and redaction. |
| `sovmem/tests/corpus_export.rs` | Cover deterministic corpus export, atomic report publication, source immutability, and canary compatibility. |
| `sovmem/tests/golden_queries.rs` | Cover JSONL structure, authorship, uniqueness, privacy, reference integrity, and coverage. |
| `evals/audy-sovmem-v0/golden.jsonl` | Human-authored, human-approved query and relevance ground truth. Created only at the explicit human gate in Task 6. |
| `package.json` | Add guarded corpus export, corpus verification, and golden-query validation commands. |
| `README.md` | Document the corpus-first milestone, human query gate, and non-retrieval status. |
| `index.md` | Register every new durable artifact and update the current SovMem boundary. |

---

### Task 1: Load and validate the exact retrieval-corpus contract

**Files:**
- Create: `profiles/audy-ssa0-v0/retrieval-corpus.yaml`
- Create: `sovmem/src/profile/corpus.rs`
- Create: `sovmem/tests/corpus_config.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `profiles/audy-ssa0-v0/profile.yaml`

**Interfaces:**
- Consumes: `SourceMode`, `ProfileError`, `valid_relative_path`, and `glob_matches` from `sovmem::profile`.
- Produces: `load_corpus(path: &Path) -> Result<CorpusConfig, CorpusError>`.
- Produces: `CorpusConfig::selected_paths() -> Vec<String>` and `CorpusConfig::entry(path: &str) -> Option<CorpusEntryRef<'_>>`.
- Produces: `CorpusTheme::{TfisSsa,SovereignContext,AgentOperations,GovernanceEvidence}` and `CorpusLane::{Core,Research}`.

- [ ] **Step 1: Write failing configuration tests**

Create `sovmem/tests/corpus_config.rs` with the following contract tests:

```rust
use sovmem::profile::{load_corpus, load_profile, CorpusLane, CorpusTheme};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn corpus_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../profiles/audy-ssa0-v0/retrieval-corpus.yaml")
}

fn profile_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../profiles/audy-ssa0-v0/profile.yaml")
}

#[test]
fn loads_the_approved_exact_corpus_contract() {
    let corpus = load_corpus(&corpus_path()).expect("approved corpus loads");
    assert_eq!(corpus.schema, "audy-retrieval-corpus-v0");
    assert_eq!(corpus.source_commit, "43824cfd9caed7b260aa442128fd1130ddb6e2ff");
    assert_eq!(corpus.minimum_documents, 53);
    assert_eq!(corpus.minimum_research_documents, 45);
    assert_eq!(corpus.core.len(), 8);
    assert_eq!(corpus.research.len(), 55);
    assert_eq!(corpus.selected_paths().len(), 63);
    assert_eq!(corpus.selected_paths().into_iter().collect::<BTreeSet<_>>().len(), 63);

    let mut themes = BTreeMap::new();
    for entry in &corpus.research {
        *themes.entry(entry.theme.clone()).or_insert(0usize) += 1;
    }
    assert_eq!(themes[&CorpusTheme::TfisSsa], 12);
    assert_eq!(themes[&CorpusTheme::SovereignContext], 15);
    assert_eq!(themes[&CorpusTheme::AgentOperations], 14);
    assert_eq!(themes[&CorpusTheme::GovernanceEvidence], 14);

    assert!(corpus
        .entry("01-Soul/Audy Himura.md")
        .is_some_and(|entry| entry.lane == CorpusLane::Core));
}

#[test]
fn every_selected_path_has_an_existing_profile_mapping() {
    let corpus = load_corpus(&corpus_path()).expect("approved corpus loads");
    let profile = load_profile(&profile_path()).expect("profile loads");
    for path in corpus.selected_paths() {
        assert!(profile.mapping_for(&path).is_some(), "missing mapping for {path}");
    }
}
```

Add table-driven temporary-YAML cases asserting these stable codes:

```rust
// corpus.schema: unsupported schema or non-committed mode
// corpus.commit: abbreviated or uppercase source commit
// corpus.core: missing, duplicate, extra, or role-mismatched required core path
// corpus.research: any candidate count other than exactly 55
// corpus.theme: unknown theme or a candidate distribution below 10/15/10/10
// corpus.path: wildcard, traversal, duplicate, non-04-Research research path, or forbidden lane
// corpus.selection: selected path matched by an exclusion
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```bash
cargo test --manifest-path sovmem/Cargo.toml --test corpus_config
```

Expected: FAIL because `profile::corpus`, `load_corpus`, and the committed manifest do not exist.

- [ ] **Step 3: Define strict corpus configuration types**

Create `sovmem/src/profile/corpus.rs` with these public types and exact validation constants:

```rust
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use thiserror::Error;

use super::{glob_matches, valid_relative_path, SourceMode};

pub const CORPUS_SCHEMA: &str = "audy-retrieval-corpus-v0";
pub const MINIMUM_DOCUMENTS: usize = 53;
pub const MINIMUM_RESEARCH_DOCUMENTS: usize = 45;
pub const REQUIRED_CORE: [(&str, CorpusRole); 8] = [
    ("01-Soul/Audy Himura.md", CorpusRole::Identity),
    ("01-Soul/Identity.md", CorpusRole::Doctrine),
    ("01-Soul/SSA Archetypes.md", CorpusRole::Doctrine),
    ("01-Soul/TFIS Axioms.md", CorpusRole::Doctrine),
    ("01-Soul/TFIS Canon.md", CorpusRole::Doctrine),
    ("AGENTS.md", CorpusRole::OperatingContract),
    ("OPERATIONS.md", CorpusRole::OperatingContract),
    ("00-Index.md", CorpusRole::ContextIndex),
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusRole {
    Identity,
    Doctrine,
    OperatingContract,
    ContextIndex,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusTheme {
    TfisSsa,
    SovereignContext,
    AgentOperations,
    GovernanceEvidence,
}

impl CorpusTheme {
    pub const fn minimum_accepted(&self) -> usize {
        match self {
            Self::TfisSsa => 10,
            Self::SovereignContext => 15,
            Self::AgentOperations => 10,
            Self::GovernanceEvidence => 10,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusLane {
    Core,
    Research,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreEntry {
    pub path: String,
    pub role: CorpusRole,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchEntry {
    pub path: String,
    pub theme: CorpusTheme,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusConfig {
    pub schema: String,
    pub mode: SourceMode,
    pub source_commit: String,
    pub minimum_documents: usize,
    pub minimum_research_documents: usize,
    pub core: Vec<CoreEntry>,
    pub research: Vec<ResearchEntry>,
    pub exclude: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusEntryRef<'a> {
    pub lane: CorpusLane,
    pub path: &'a str,
    pub role: Option<&'a CorpusRole>,
    pub theme: Option<&'a CorpusTheme>,
}

#[derive(Debug, Error)]
pub enum CorpusError {
    #[error("corpus configuration could not be parsed")]
    Parse,
    #[error("corpus schema or source mode is unsupported")]
    Schema,
    #[error("corpus source commit is invalid")]
    Commit,
    #[error("canonical core contract is invalid")]
    Core,
    #[error("research candidate count is invalid")]
    Research,
    #[error("research theme distribution is invalid")]
    Theme,
    #[error("corpus path contract is invalid")]
    Path,
    #[error("corpus selection conflicts with an exclusion")]
    Selection,
}
```

Implement `CorpusError::code()` with exactly `corpus.parse`, `corpus.schema`, `corpus.commit`, `corpus.core`, `corpus.research`, `corpus.theme`, `corpus.path`, and `corpus.selection`. Implement `CorpusConfig::validate()` so `minimum_documents` equals `MINIMUM_DOCUMENTS`, `minimum_research_documents` equals `MINIMUM_RESEARCH_DOCUMENTS`, the core equals the exact `REQUIRED_CORE` path/role set, research count is exactly `55`, every theme's candidate count is at least `CorpusTheme::minimum_accepted()`, paths are unique/exact/safe, every research path begins `04-Research/`, all forbidden-lane exclusions are present, and no selected path matches an exclusion. The committed-manifest test separately freezes the reviewed distribution at `12/15/14/14`. Any later candidate-count change requires a new corpus schema version.

Implement these exact helpers:

```rust
pub fn load_corpus(path: &Path) -> Result<CorpusConfig, CorpusError>;
impl CorpusConfig {
    pub fn selected_paths(&self) -> Vec<String>;
    pub fn entry(&self, path: &str) -> Option<CorpusEntryRef<'_>>;
    pub fn research_theme(&self, path: &str) -> Option<&CorpusTheme>;
}
```

`selected_paths()` must return a sorted, deduplicated vector. `load_corpus()` must parse to `serde_yaml::Value` first and reject any mode other than the literal `committed` before typed deserialization, mirroring `load_canary()`.

- [ ] **Step 4: Add the exact reviewed candidate manifest**

Create `profiles/audy-ssa0-v0/retrieval-corpus.yaml` with this complete content. The candidate list is proposed by the plan and becomes human-approved only when Khalil approves this plan before execution.

```yaml
schema: audy-retrieval-corpus-v0
mode: committed
source_commit: 43824cfd9caed7b260aa442128fd1130ddb6e2ff
minimum_documents: 53
minimum_research_documents: 45
core:
  - { path: "01-Soul/Audy Himura.md", role: identity }
  - { path: "01-Soul/Identity.md", role: doctrine }
  - { path: "01-Soul/SSA Archetypes.md", role: doctrine }
  - { path: "01-Soul/TFIS Axioms.md", role: doctrine }
  - { path: "01-Soul/TFIS Canon.md", role: doctrine }
  - { path: "AGENTS.md", role: operating-contract }
  - { path: "OPERATIONS.md", role: operating-contract }
  - { path: "00-Index.md", role: context-index }
research:
  - { path: "04-Research/Agent-Loop-Engineering/Daily-Court-Deep-Gap-Notes.md", theme: tfis-ssa }
  - { path: "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md", theme: tfis-ssa }
  - { path: "04-Research/Agent-Loop-Engineering/Loop-Engineering-Concept-Map.md", theme: tfis-ssa }
  - { path: "04-Research/Agent-Loop-Engineering/README.md", theme: tfis-ssa }
  - { path: "04-Research/Agent-Loop-Engineering/TFIS-Daily-Court-Gap-Analysis.md", theme: tfis-ssa }
  - { path: "04-Research/DSOM/DSOM-Adoption-Spec.md", theme: tfis-ssa }
  - { path: "04-Research/DSOM/DSOM-Concept-Map.md", theme: tfis-ssa }
  - { path: "04-Research/DSOM/DSOM-TFIS-SSA-Gap-Analysis.md", theme: tfis-ssa }
  - { path: "04-Research/DSOM/README.md", theme: tfis-ssa }
  - { path: "04-Research/DSOM/source-pages/4.-ai-and-agent-protocols__ai-cognitive-twin-protocol.md", theme: tfis-ssa }
  - { path: "04-Research/DSOM/source-pages/1.-sovereign-governance__digital-sovereignty-model.md", theme: tfis-ssa }
  - { path: "04-Research/DSOM/source-pages/1.-sovereign-governance__operational-sovereignty.md", theme: tfis-ssa }
  - { path: "04-Research/AVM/AVM-Adoption-Spec.md", theme: sovereign-context }
  - { path: "04-Research/AVM/AVM-OKF-Substrate-Investigation.md", theme: sovereign-context }
  - { path: "04-Research/AVM/Enterprise-Managed-Authorization-Implications.md", theme: sovereign-context }
  - { path: "04-Research/AVM/Nous-Graph-MCP-Precursor-Note.md", theme: sovereign-context }
  - { path: "04-Research/AVM/README.md", theme: sovereign-context }
  - { path: "04-Research/AVM/source-pages/blog-enterprise-managed-auth.md", theme: sovereign-context }
  - { path: "04-Research/AVM/source-pages/mcp-extension-enterprise-managed-authorization.md", theme: sovereign-context }
  - { path: "04-Research/OKF/Audy-OKF-Profile-Draft.md", theme: sovereign-context }
  - { path: "04-Research/OKF/OKF-Adoption-Spec.md", theme: sovereign-context }
  - { path: "04-Research/OKF/OKF-Audy-Vault-Gap-Analysis.md", theme: sovereign-context }
  - { path: "04-Research/OKF/OKF-Concept-Map.md", theme: sovereign-context }
  - { path: "04-Research/OKF/README.md", theme: sovereign-context }
  - { path: "04-Research/OKF/source-pages/README.md", theme: sovereign-context }
  - { path: "04-Research/OKF/source-pages/SPEC.md", theme: sovereign-context }
  - { path: "04-Research/DSOM/source-pages/palace-rooms-wing_dsom_core__room_brain_artifacts-brain-files.md", theme: sovereign-context }
  - { path: "04-Research/DSOM/source-pages/1.-sovereign-governance__ai-master-protocol.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/1.-sovereign-governance__crisp2-operational-strategy.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/1.-sovereign-governance__gitops-aiops-ansible-strategy.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/1.-sovereign-governance__operational-guide.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/2.-operational-rituals__eod-ritual.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/2.-operational-rituals__ritual-of-transition.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/2.-operational-rituals__sod-ritual.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/4.-ai-and-agent-protocols__ai-response-template.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/4.-ai-and-agent-protocols__howto-adopt-dsom.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/4.-ai-and-agent-protocols__howto-reanimate-session.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/4.-ai-and-agent-protocols__howto-upgrade-dsom.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/4.-ai-and-agent-protocols__human-handover-context.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/4.-ai-and-agent-protocols__multi-agent-protocols.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/4.-ai-and-agent-protocols__reanimation-prompt-template.md", theme: agent-operations }
  - { path: "04-Research/DSOM/source-pages/1.-sovereign-governance__hub-and-spoke-model.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/1.-sovereign-governance__itil-alignment.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/5.-tools-and-automation-drivers__audit-pre-flight.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/5.-tools-and-automation-drivers__privacy-guardian.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/7.-ansible-playbooks-dsom-operational__audit-preflight.yml-pre-flight-audit.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/7.-ansible-playbooks-dsom-operational__privacy-scan.yml-privacy-guardian.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/dsom-automation-encyclopedia-docs-tools__howto-audit-pre-flight.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/dsom-automation-encyclopedia-docs-tools__howto-privacy-guardian.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/palace-documentation__research-reasoning-gap.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/palace-rooms-wing_dsom_core__room_clean_architecture-design-laws.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/palace-rooms-wing_dsom_core__room_crisp_strategy-ai-interaction.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/palace-rooms-wing_dsom_core__room_ledger-version-history.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/palace-rooms-wing_dsom_core__room_uncategorised-discoveries.md", theme: governance-evidence }
  - { path: "04-Research/DSOM/source-pages/table-of-contents__contributing.md", theme: governance-evidence }
exclude:
  - "02-Memory/**"
  - "05-Daily/**"
  - "08-Hermes/**"
  - "09-Claude/**"
  - "10-Codex/**"
  - "11-Antigravity/**"
  - "loops/**"
```

- [ ] **Step 5: Add exact mappings without changing the schema**

Insert these rules in `profiles/audy-ssa0-v0/profile.yaml` immediately before the generic `source-pages` and research-pattern rules so stored precedence remains explicit:

```yaml
  - { pattern: "04-Research/Agent-Loop-Engineering/Daily-Court-Deep-Gap-Notes.md", core_type: concept, context_kind: "Gap Analysis", layer: analysis, authority: working }
  - { pattern: "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md", core_type: concept, context_kind: "Research Brief", layer: analysis, authority: working }
  - { pattern: "04-Research/AVM/AVM-OKF-Substrate-Investigation.md", core_type: concept, context_kind: "Research Brief", layer: analysis, authority: working }
  - { pattern: "04-Research/AVM/Enterprise-Managed-Authorization-Implications.md", core_type: concept, context_kind: "Design Brief", layer: analysis, authority: working }
  - { pattern: "04-Research/AVM/Nous-Graph-MCP-Precursor-Note.md", core_type: concept, context_kind: "Design Brief", layer: analysis, authority: working }
```

Do not add any `context_kinds` values. The test must prove all 63 selected paths map through existing types.

- [ ] **Step 6: Register the module and run GREEN**

Add `mod corpus;` to `sovmem/src/profile/mod.rs`; its existing `pub use corpus::*;` pattern is satisfied by adding `pub use corpus::*;` beside the other module exports.

Run:

```bash
cargo fmt --manifest-path sovmem/Cargo.toml
cargo test --manifest-path sovmem/Cargo.toml --test corpus_config
cargo test --manifest-path sovmem/Cargo.toml --test profile_config
```

Expected: all focused tests pass; the existing profile tests remain green.

- [ ] **Step 7: Commit the corpus contract**

```bash
git add profiles/audy-ssa0-v0/retrieval-corpus.yaml profiles/audy-ssa0-v0/profile.yaml sovmem/src/profile/corpus.rs sovmem/src/profile/mod.rs sovmem/tests/corpus_config.rs
git commit -m "feat: define Audy retrieval corpus contract"
```

### Task 2: Enforce corpus composition and render deterministic coverage evidence

**Files:**
- Create: `sovmem/src/profile/corpus_report.rs`
- Create: `sovmem/tests/corpus_report.rs`
- Modify: `sovmem/src/profile/corpus.rs`
- Modify: `sovmem/src/profile/mod.rs`

**Interfaces:**
- Consumes: `CorpusConfig`, `CorpusLane`, `CorpusTheme`, `GeneratedDocument`, `ExcludedSource`, and `Diagnostic`.
- Produces: `evaluate_corpus(profile, config, documents, exclusions, diagnostics) -> Result<CorpusReportDraft, CorpusGateError>`.
- Produces: `CorpusReportDraft::finalize(bundle_hash: String) -> CorpusReport`.

- [ ] **Step 1: Write failing gate and report tests**

Create fixtures in `sovmem/tests/corpus_report.rs` that generate `GeneratedDocument` values for every configured path with canonical source-derived IDs. Assert:

```rust
#[test]
fn approved_composition_passes_with_deterministic_counts() {
    let config = load_corpus(&corpus_path()).unwrap();
    let documents = documents_for(&config);
    let profile = load_profile(&profile_path()).unwrap();
    let draft = evaluate_corpus(&profile, &config, &documents, &[], &[]).unwrap();
    let report = draft.finalize("a".repeat(64));
    assert_eq!(report.counts.candidates, 63);
    assert_eq!(report.counts.core, 8);
    assert_eq!(report.counts.research, 55);
    assert_eq!(report.research_by_theme[&CorpusTheme::TfisSsa], 12);
    assert!(report.gates_passed);
}
```

Add focused cases proving:

- a missing or excluded core returns `corpus.core_missing`;
- 10 excluded research candidates may still pass only if all theme minima and total `53` remain satisfied;
- an exclusion that drops a theme below its minimum returns `corpus.theme_minimum`;
- every valid passing report records at least 45 accepted research documents and at least 53 total accepted documents; these are derived invariants because the four theme minima sum to 45 and all eight core documents are mandatory, not independently reachable error branches;
- a generated path not present in the manifest returns `corpus.unmanifested_document`;
- any `Severity::Error` diagnostic returns `corpus.validation_error`;
- a uniformly self-consistent foreign profile tuple returns `corpus.profile_identity`;
- a mutated config that no longer passes `CorpusConfig::validate()` returns `corpus.config_invalid`;
- accepted entries and exclusions serialize in stable path order;
- serialized JSON contains neither a supplied note-body sentinel nor `/Volumes/himura`.

- [ ] **Step 2: Run the focused test and verify RED**

```bash
cargo test --manifest-path sovmem/Cargo.toml --test corpus_report
```

Expected: FAIL because the corpus report module does not exist.

- [ ] **Step 3: Implement strict report and gate types**

Create `sovmem/src/profile/corpus_report.rs` with these serialized structures:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CorpusCounts {
    pub candidates: usize,
    pub core: usize,
    pub research: usize,
    pub rejected: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct CorpusAccepted {
    pub source_path: String,
    pub id: String,
    pub lane: String,
    pub theme: Option<CorpusTheme>,
    pub source_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct CorpusRejected {
    pub source_path: String,
    pub lane: String,
    pub theme: Option<CorpusTheme>,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CorpusReport {
    pub schema: String,
    pub profile: String,
    pub source_commit: String,
    pub counts: CorpusCounts,
    pub required_core: Vec<String>,
    pub research_by_theme: BTreeMap<CorpusTheme, usize>,
    pub accepted: Vec<CorpusAccepted>,
    pub rejected: Vec<CorpusRejected>,
    pub diagnostic_codes: Vec<String>,
    pub bundle_hash: String,
    pub gates_passed: bool,
}

pub struct CorpusReportDraft {
    report: CorpusReport,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("corpus acceptance gate failed: {code}")]
pub struct CorpusGateError {
    code: &'static str,
}
```

Implement `CorpusGateError::code()`. Make `CorpusConfig::validate()` available as `pub(crate)` and call it at the start of `evaluate_corpus()`, mapping failure to `corpus.config_invalid`. Pass the canonical `ProfileConfig` into `evaluate_corpus()` and require every generated document's full profile tuple to equal that profile before report construction; a uniformly foreign tuple must not establish its own authority. Any `Severity::Error` diagnostic returns `corpus.validation_error`. The function must derive acceptance only from generated documents, match exclusions back to configured paths, treat only dirty/untracked non-core exclusions as `CorpusRejected`, and reject any other mismatch. Enforce actionable gates in this stable order: valid config, core, canonical profile identity and hard diagnostics, theme minima, then unmanifested/missing evidence. A passing report must assert and record the derived `research >= 45` and `total >= 53` invariants, but must not expose synthetic error branches reachable only through an invalid mutable config. Diagnostics enter the report as sorted unique codes only; messages and source bodies never enter `corpus-report.json`.

- [ ] **Step 4: Register the report module and run GREEN**

Add `mod corpus_report;` and `pub use corpus_report::*;` in `sovmem/src/profile/mod.rs`.

Run:

```bash
cargo fmt --manifest-path sovmem/Cargo.toml
cargo test --manifest-path sovmem/Cargo.toml --test corpus_report
```

Expected: all report and gate tests pass.

- [ ] **Step 5: Commit corpus gates**

```bash
git add sovmem/src/profile/corpus_report.rs sovmem/src/profile/mod.rs sovmem/tests/corpus_report.rs
git commit -m "feat: enforce retrieval corpus gates"
```

### Task 3: Reuse the hardened export transaction for corpus publication

**Files:**
- Modify: `sovmem/src/profile/export.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Create: `sovmem/tests/corpus_export.rs`
- Modify: `sovmem/tests/export_determinism.rs`

**Interfaces:**
- Consumes: `load_corpus`, `evaluate_corpus`, and the existing committed-source, normalization, link, validation, render, staging, fsync, rollback, and publication functions.
- Produces: `export_corpus(request: CorpusExportRequest) -> Result<CorpusExportSummary, ExportError>`.
- Preserves: `export_canary(request: ExportRequest) -> Result<ExportSummary, ExportError>` unchanged for callers.

- [ ] **Step 1: Write failing corpus transaction tests**

Create `sovmem/tests/corpus_export.rs` with disposable Git fixtures. The happy fixture must contain eight core paths and 45 mapped research paths distributed 10/15/10/10, then assert:

```rust
let first = export_corpus(request(&repo, first_output.clone(), corpus_path.clone())).unwrap();
let second = export_corpus(request(&repo, second_output.clone(), corpus_path)).unwrap();
assert_eq!(first, second);
assert_eq!(tree_bytes(&first_output), tree_bytes(&second_output));
assert_eq!(first.core, 8);
assert_eq!(first.research, 45);
assert_eq!(first.exported, 53);
assert!(first_output.join("corpus-report.json").is_file());
```

Add cases proving:

- corpus gate failure preserves a prior output sentinel;
- a dirty core path fails with `corpus.core_missing` and publishes nothing;
- a dirty research buffer path is reported but may pass while all minima remain met;
- `corpus-report.json` is written before the same fsync and atomic publication boundary as the manifest;
- output inside the vault fails before writes;
- source hashes and worktree bytes are unchanged before/after;
- the existing canary still exports byte-identically after the refactor.

- [ ] **Step 2: Run the focused tests and verify RED**

```bash
cargo test --manifest-path sovmem/Cargo.toml --test corpus_export
cargo test --manifest-path sovmem/Cargo.toml --test export_determinism
```

Expected: corpus tests fail because `export_corpus` does not exist; existing export tests remain green before refactoring.

- [ ] **Step 3: Add corpus request and summary types**

Add to `sovmem/src/profile/export.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusExportRequest {
    pub vault_root: PathBuf,
    pub output_root: PathBuf,
    pub profile_path: PathBuf,
    pub corpus_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusExportSummary {
    pub exported: usize,
    pub excluded: usize,
    pub core: usize,
    pub research: usize,
    pub warnings: usize,
    pub errors: usize,
    pub bundle_hash: String,
}

enum ExportFlavor<'a> {
    Canary,
    Corpus(&'a CorpusConfig),
}

struct SelectionRequest {
    vault_root: PathBuf,
    output_root: PathBuf,
    profile_path: PathBuf,
    source_commit: String,
    include: Vec<String>,
    exclude: Vec<String>,
}
```

Add `ExportError::Corpus(#[from] CorpusError)` and `ExportError::CorpusGate(#[from] CorpusGateError)`; their codes delegate to the contained errors.

- [ ] **Step 4: Extract one selection transaction**

Refactor the existing `export_canary()` body into:

```rust
fn export_selection(
    request: SelectionRequest,
    flavor: ExportFlavor<'_>,
) -> Result<(ExportSummary, Option<CorpusReport>), ExportError>;
```

The transaction order must remain exactly: canonicalize and separate roots; load profile; select exact committed paths; open snapshot; parse; normalize; resolve links; validate; evaluate corpus gate when present; create same-parent staging; render sorted bundle/manifest/export report; finalize and write `corpus-report.json` when present; fsync; re-open and compare snapshot; publish atomically; return summary. Do not duplicate the existing publication implementation.

Implement wrappers:

```rust
pub fn export_canary(request: ExportRequest) -> Result<ExportSummary, ExportError> {
    let canary = load_canary(&request.canary_path)?;
    export_selection(
        SelectionRequest {
            vault_root: request.vault_root,
            output_root: request.output_root,
            profile_path: request.profile_path,
            source_commit: canary.source_commit,
            include: canary.include,
            exclude: canary.exclude,
        },
        ExportFlavor::Canary,
    )
    .map(|(summary, _)| summary)
}

pub fn export_corpus(request: CorpusExportRequest) -> Result<CorpusExportSummary, ExportError> {
    let corpus = load_corpus(&request.corpus_path)?;
    let include = corpus.selected_paths();
    let (summary, report) = export_selection(
        SelectionRequest {
            vault_root: request.vault_root,
            output_root: request.output_root,
            profile_path: request.profile_path,
            source_commit: corpus.source_commit.clone(),
            include,
            exclude: corpus.exclude.clone(),
        },
        ExportFlavor::Corpus(&corpus),
    )?;
    let report = report.ok_or(ExportError::SourceChanged)?;
    Ok(CorpusExportSummary {
        exported: summary.exported,
        excluded: summary.excluded,
        core: report.counts.core,
        research: report.counts.research,
        warnings: summary.warnings,
        errors: 0,
        bundle_hash: summary.bundle_hash,
    })
}
```

Serialize `CorpusReport` with pretty JSON plus a terminal newline and write it inside staging before `sync_directory_tree()`. Offline bundle verification must permit exactly `corpus-report.json` as an optional root artifact, strictly deserialize it when present, require its `bundle_hash` and accepted IDs to match `manifest.json`, and reject it for canary bundles whose manifest selection does not correspond to the report.

- [ ] **Step 5: Run corpus, canary, and verifier tests GREEN**

```bash
cargo fmt --manifest-path sovmem/Cargo.toml
cargo test --manifest-path sovmem/Cargo.toml --test corpus_export
cargo test --manifest-path sovmem/Cargo.toml --test export_determinism
cargo test --manifest-path sovmem/Cargo.toml --test cli
```

Expected: corpus transaction tests pass; every existing canary and verifier test remains green.

- [ ] **Step 6: Commit the shared export transaction**

```bash
git add sovmem/src/profile/export.rs sovmem/src/profile/mod.rs sovmem/tests/corpus_export.rs sovmem/tests/export_determinism.rs sovmem/tests/cli.rs
git commit -m "feat: export gated retrieval corpora"
```

### Task 4: Add corpus and golden-query CLI contracts

**Files:**
- Modify: `sovmem/src/cli.rs`
- Modify: `sovmem/tests/cli.rs`

**Interfaces:**
- Consumes: `export_corpus`, `CorpusExportRequest`, and later `validate_golden_file`.
- Produces CLI commands `corpus` and `verify-golden` with stable JSON summaries.

- [ ] **Step 1: Write failing CLI tests for corpus export**

Add a `run_corpus()` helper beside `run_export()` in `sovmem/tests/cli.rs` and assert a valid fixture prints exactly one stdout line. Parse it with the existing `one_json_line()` helper and assert:

```rust
let summary = one_json_line(&output);
assert_eq!(summary["status"], "ok");
assert_eq!(summary["command"], "corpus");
assert_eq!(summary["exported"], 53);
assert_eq!(summary["excluded"], 10);
assert_eq!(summary["core"], 8);
assert_eq!(summary["research"], 45);
assert_eq!(summary["warnings"], 10);
assert_eq!(summary["errors"], 0);
let hash = summary["bundle_hash"].as_str().unwrap();
assert_eq!(hash.len(), 64);
assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
```

Add cases asserting configuration/source failures return exit `2`, gate failures return exit `1`, stdout remains empty on failure, stderr contains stable redacted codes, and neither source bodies nor absolute paths are emitted.

- [ ] **Step 2: Run the CLI test and verify RED**

```bash
cargo test --manifest-path sovmem/Cargo.toml --test cli corpus_
```

Expected: FAIL because the `corpus` subcommand is absent.

- [ ] **Step 3: Add the corpus command and output**

Extend `Command` with:

```rust
Corpus {
    #[arg(long)]
    vault: PathBuf,
    #[arg(long)]
    profile: PathBuf,
    #[arg(long)]
    corpus: PathBuf,
    #[arg(long)]
    output: PathBuf,
},
```

Add `CorpusOutput` with fields in this exact serialized order: `status`, `command`, `exported`, `excluded`, `core`, `research`, `warnings`, `errors`, `bundle_hash`. Implement `run_corpus()` so configuration/source errors return `2`, corpus acceptance-gate errors return `1`, and success returns `0`. Reuse `export_diagnostics()` and never emit source paths or exception text.

- [ ] **Step 4: Run CLI tests GREEN and commit**

```bash
cargo fmt --manifest-path sovmem/Cargo.toml
cargo test --manifest-path sovmem/Cargo.toml --test cli corpus_
git add sovmem/src/cli.rs sovmem/tests/cli.rs
git commit -m "feat: expose corpus export command"
```

### Task 5: Validate human-authored golden queries against corpus evidence

**Files:**
- Create: `sovmem/src/profile/golden.rs`
- Create: `sovmem/tests/golden_queries.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `sovmem/src/cli.rs`
- Modify: `sovmem/tests/cli.rs`

**Interfaces:**
- Consumes: strict `BundleManifest`, `CorpusReport`, `ProfileConfig`, canonical profile IDs, and existing conservative secret detection.
- Produces: `validate_golden_file(profile: &ProfileConfig, bundle_root: &Path, golden_path: &Path) -> Result<GoldenSummary, GoldenError>`.
- Produces CLI `verify-golden --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus --golden evals/audy-sovmem-v0/golden.jsonl`.

- [ ] **Step 1: Write failing golden-query tests**

Create `sovmem/tests/golden_queries.rs` with a valid 25-line JSONL fixture referencing a synthetic corpus report covering `core` plus all four themes. Assert:

```rust
let summary = validate_golden_file(&profile, &bundle, &golden).unwrap();
assert_eq!(summary.queries, 25);
assert_eq!(summary.relevant_ids, 25);
assert_eq!(
    summary.coverage,
    vec![
        "core".to_owned(),
        "tfis-ssa".to_owned(),
        "sovereign-context".to_owned(),
        "agent-operations".to_owned(),
        "governance-evidence".to_owned(),
    ]
);
```

Add one focused case per stable code:

- `golden.read`: missing/unreadable file;
- `golden.parse`: blank line, invalid JSON, unknown field, or trailing content;
- `golden.count`: fewer than 25 lines;
- `golden.id`: malformed or duplicate query ID;
- `golden.query`: blank, control-containing, or duplicate normalized query;
- `golden.authorship`: value other than literal `human`;
- `golden.relevance`: empty, duplicate, unsorted, noncanonical, foreign-profile, or absent relevant ID;
- `golden.privacy`: secret-like text, absolute path, or an explicitly forbidden lane/content token;
- `golden.coverage`: no query references core or one of the four themes;
- `golden.bundle`: missing/inconsistent `manifest.json` or `corpus-report.json`.

- [ ] **Step 2: Run the focused test and verify RED**

```bash
cargo test --manifest-path sovmem/Cargo.toml --test golden_queries
```

Expected: FAIL because the golden validator does not exist.

- [ ] **Step 3: Implement strict JSONL types and validation**

Create `sovmem/src/profile/golden.rs`:

```rust
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GoldenQuery {
    pub id: String,
    pub query: String,
    pub relevant_ids: Vec<String>,
    pub rationale: String,
    pub authorship: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct GoldenSummary {
    pub queries: usize,
    pub relevant_ids: usize,
    pub coverage: Vec<String>,
}

#[derive(Debug, Error)]
pub enum GoldenError {
    #[error("golden query file could not be read")]
    Read,
    #[error("golden query line could not be parsed")]
    Parse,
    #[error("golden query count is below the approved minimum")]
    Count,
    #[error("golden query identifier is invalid")]
    Id,
    #[error("golden query text is invalid")]
    Query,
    #[error("golden query authorship is invalid")]
    Authorship,
    #[error("golden query relevance is invalid")]
    Relevance,
    #[error("golden query contains unsafe content")]
    Privacy,
    #[error("golden query coverage is incomplete")]
    Coverage,
    #[error("golden query evidence bundle is invalid")]
    Bundle,
}
```

Implement `GoldenError::code()` with the codes listed above. `validate_golden_file()` must read raw bytes, require UTF-8 and a terminal newline, reject blank lines, deserialize each line independently with `deny_unknown_fields`, normalize query uniqueness with Unicode text unchanged except `trim()` and ASCII whitespace collapse, require IDs `q-001` through `q-999`, and require sorted unique `relevant_ids`.

Load and strictly validate `manifest.json` and `corpus-report.json`; require equal bundle hashes and equal accepted ID sets. Build an ID-to-lane/theme map from `CorpusReport.accepted`, then require relevance coverage for `core` and all four themes. Apply existing `contains_secret()` to query plus rationale and separately reject `/Volumes/`, `/Users/`, `02-Memory`, `05-Daily`, `08-Hermes`, `09-Claude`, `10-Codex`, `11-Antigravity`, `token:`, `password:`, and `private key` case-insensitively.

The automated validator must not infer whether free text describes a personal incident. Semantic personal-detail review remains part of Khalil's human authorship and approval gate; automated enforcement is limited to `contains_secret()` and the explicit denylist above.

- [ ] **Step 4: Add and test the CLI command**

Extend `Command`:

```rust
VerifyGolden {
    #[arg(long)]
    profile: PathBuf,
    #[arg(long)]
    bundle: PathBuf,
    #[arg(long)]
    golden: PathBuf,
},
```

Success prints exactly:

```json
{"status":"ok","command":"verify-golden","queries":25,"relevant_ids":25,"coverage":["core","tfis-ssa","sovereign-context","agent-operations","governance-evidence"],"errors":0}
```

Golden data errors return exit `1`; missing profile/bundle/file configuration returns `2`. Failures emit only stable code plus a fixed redacted message.

Run:

```bash
cargo fmt --manifest-path sovmem/Cargo.toml
cargo test --manifest-path sovmem/Cargo.toml --test golden_queries
cargo test --manifest-path sovmem/Cargo.toml --test cli verify_golden_
```

Expected: all focused golden and CLI tests pass.

- [ ] **Step 5: Commit golden validation**

```bash
git add sovmem/src/profile/golden.rs sovmem/src/profile/mod.rs sovmem/src/cli.rs sovmem/tests/golden_queries.rs sovmem/tests/cli.rs
git commit -m "feat: validate human golden queries"
```

### Task 6: Human-author and approve the golden query set

**Files:**
- Create after human authorship: `evals/audy-sovmem-v0/golden.jsonl`

**Interfaces:**
- Consumes: the corpus exporter and golden validator from Tasks 3-5 plus the readable pinned Audy Vault.
- Produces: the first accepted live corpus under ignored `var/audy-sovmem-retrieval-corpus/` and its generated ID catalog.
- Produces: at least 25 human-authored `GoldenQuery` JSON lines approved by Khalil.

- [ ] **Step 1: Run the first live corpus preflight**

Require `AUDY_VAULT` to point to the readable Git vault, then run:

```bash
test -n "$AUDY_VAULT"
git -C "$AUDY_VAULT" rev-parse HEAD
npm run corpus:audy
npm run verify:corpus:audy
```

Expected HEAD: `43824cfd9caed7b260aa442128fd1130ddb6e2ff`. Expected corpus result: `exported >= 53`, `core = 8`, `research >= 45`, `errors = 0`, followed by offline verification with the same bundle hash. If the corpus gate fails, stop and resolve the manifest, mapping, privacy, or validation failure through the normal TDD and review loop before asking for queries.

- [ ] **Step 2: Stop for the human-authorship gate**

Do not generate query text, rationales, or relevance labels. Present Khalil with the accepted generated ID catalog grouped by `core` and the four themes. The catalog may contain only `id`, generated title, lane, and theme; it must not contain source bodies, absolute paths, or secrets.

Ask Khalil to author or explicitly provide all five fields for at least 25 lines:

```json
{"id":"q-001","query":"What defines the Solo Systems Architect?","relevant_ids":["ssa-0/01-Soul/SSA%20Archetypes"],"rationale":"The canonical archetype document directly defines the role.","authorship":"human"}
```

The line above is the single query already approved in the design. It may be retained. Every additional line requires Khalil's authorship or explicit approval.

- [ ] **Step 3: Write only the supplied human data**

After Khalil supplies or approves the complete set, create `evals/audy-sovmem-v0/golden.jsonl` with exactly the approved JSON objects, one compact object per line, sorted by query ID, and one terminal newline. Do not paraphrase, expand, repair relevance, or add queries silently.

- [ ] **Step 4: Validate and return failures to the human**

Run:

```bash
cargo run --manifest-path sovmem/Cargo.toml -- verify-golden \
  --profile profiles/audy-ssa0-v0/profile.yaml \
  --bundle var/audy-sovmem-retrieval-corpus \
  --golden evals/audy-sovmem-v0/golden.jsonl
```

Expected: `queries >= 25`, coverage contains all five lanes/themes, and `errors: 0`. If validation fails, report the stable code and ask Khalil to revise the affected human record; do not auto-correct it.

- [ ] **Step 5: Commit the approved human ground truth**

```bash
git add evals/audy-sovmem-v0/golden.jsonl
git commit -m "eval: add human Audy retrieval queries"
```

### Task 7: Wire root commands and document the corpus-first boundary

**Files:**
- Modify: `package.json`
- Modify: `README.md`
- Modify: `index.md`
- Test: `test/*.test.js`

**Interfaces:**
- Consumes: `corpus`, `verify`, and `verify-golden` CLI commands.
- Produces: guarded root commands and registered durable artifacts.

- [ ] **Step 1: Add failing JavaScript assertions for command safety**

Add tests that parse `package.json` and require:

```javascript
assert.match(pkg.scripts['corpus:audy'], /AUDY_VAULT/);
assert.match(pkg.scripts['corpus:audy'], /retrieval-corpus-v1\.yaml/);
assert.match(pkg.scripts['verify:corpus:audy'], /var\/audy-sovmem-retrieval-corpus/);
assert.match(pkg.scripts['verify:golden:audy'], /evals\/audy-sovmem-v0\/golden\.jsonl/);
assert.doesNotMatch(JSON.stringify(pkg), /\/Volumes\/himura|\/Users\/khalilhimura/);
```

Run `node --test test/*.test.js` and expect RED because the scripts are absent.

- [ ] **Step 2: Add guarded package scripts**

Add:

```json
"corpus:audy": "if [ -z \"$AUDY_VAULT\" ]; then echo \"error: AUDY_VAULT must be set to a readable Git vault\" >&2; exit 1; fi; cargo run --manifest-path sovmem/Cargo.toml -- corpus --vault \"$AUDY_VAULT\" --profile profiles/audy-ssa0-v0/profile.yaml --corpus profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml --output var/audy-sovmem-retrieval-corpus",
"verify:corpus:audy": "cargo run --manifest-path sovmem/Cargo.toml -- verify --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus",
"verify:golden:audy": "cargo run --manifest-path sovmem/Cargo.toml -- verify-golden --profile profiles/audy-ssa0-v0/profile.yaml --bundle var/audy-sovmem-retrieval-corpus --golden evals/audy-sovmem-v0/golden.jsonl"
```

Keep the existing 13-document canary commands unchanged.

- [ ] **Step 3: Update README and artifact registry**

Document:

- the exact eight-core plus 55-candidate contract;
- minimum 45 research / 53 total gate;
- human-authored query requirement;
- derived internal output and unchanged vault boundary;
- commands for corpus export, offline verification, and golden validation;
- explicit non-implementation of retrieval/ranking/metrics.

Register in `index.md`: the approved design, this plan, `retrieval-corpus-v1.yaml`, `corpus.rs`, `corpus_report.rs`, `golden.rs`, four new Rust test files, and `evals/audy-sovmem-v0/golden.jsonl`. Do not register ignored `var/` output.

- [ ] **Step 4: Run documentation and root checks GREEN**

```bash
node --test test/*.test.js
npm test
```

Run the repository local-link and artifact-registry validators documented by `docs/superpowers/plans/2026-07-10-nous-root-index.md`. Expected: all links resolve and all durable unignored files are registered.

- [ ] **Step 5: Commit root integration**

```bash
git add package.json README.md index.md test
git commit -m "docs: expose retrieval corpus workflow"
```

### Task 8: Repeat the live corpus gate and run whole-branch verification

**Files:**
- Verify only; generated output remains ignored under `var/audy-sovmem-retrieval-corpus/`.

**Interfaces:**
- Consumes every prior task.
- Produces final evidence that the corpus and human golden set satisfy the approved milestone without mutating Audy Vault.

- [ ] **Step 1: Run fresh local verification before touching the vault**

```bash
cargo fmt --manifest-path sovmem/Cargo.toml --check
cargo clippy --manifest-path sovmem/Cargo.toml --all-targets -- -D warnings
npm test
git diff --check origin/main..HEAD
```

Expected: all JavaScript and Rust tests pass; formatting, lint, and diff hygiene pass.

- [ ] **Step 2: Capture source identity and working-state fingerprint**

```bash
git -C "$AUDY_VAULT" rev-parse HEAD
git -C "$AUDY_VAULT" status --porcelain=v1 | shasum -a 256
```

Expected HEAD: `43824cfd9caed7b260aa442128fd1130ddb6e2ff`. Preserve the porcelain SHA-256 for the post-run comparison.

- [ ] **Step 3: Repeat the live corpus export from the final branch state**

```bash
test -n "$AUDY_VAULT"
npm run corpus:audy
```

Expected: exit `0`; `exported >= 53`, `core = 8`, `research >= 45`, `errors = 0`; `manifest.json`, `export-report.json`, and `corpus-report.json` exist under ignored output.

If a selected committed document fails mapping, privacy, identity, or validation, stop and report the exact stable code. Do not drop or replace a candidate without a versioned manifest change and Khalil approval.

- [ ] **Step 4: Verify the corpus offline and validate human queries**

```bash
npm run verify:corpus:audy
npm run verify:golden:audy
```

Expected: bundle verification returns the same bundle hash as export; golden validation reports at least 25 queries, all five coverage labels, and zero errors.

- [ ] **Step 5: Prove deterministic regeneration**

Preserve the first successful output outside the repository, rerun `npm run corpus:audy`, and compare recursively:

```bash
diff -ru /private/tmp/audy-sovmem-retrieval-corpus-first var/audy-sovmem-retrieval-corpus
```

Expected: no diff and exit `0`.

- [ ] **Step 6: Prove the vault remained unchanged**

Repeat HEAD and porcelain hashing from Step 2. Both values must match exactly.

- [ ] **Step 7: Run final full verification and independent review**

```bash
cargo fmt --manifest-path sovmem/Cargo.toml --check
cargo clippy --manifest-path sovmem/Cargo.toml --all-targets -- -D warnings
npm test
git diff --check origin/main..HEAD
git status --short --branch
```

Re-run local-link and artifact-registry validators. Request whole-branch review against the approved design and this plan. Block completion on every Critical or Important finding; fix, retest, and re-review until clean.

- [ ] **Step 8: Commit any evidence-only documentation corrections**

If verification changes only durable documentation, stage those exact files and commit:

```bash
git commit -m "docs: record retrieval corpus gate"
```

Do not commit `var/`, source bodies, absolute vault paths, status fingerprints, or generated reports.

## Completion Gate

The plan is complete only when all eight required core documents export, at least 45 research documents survive every quota, total accepted documents are at least 53, the exact human-approved manifest remains pinned, at least 25 human-authored queries validate across core plus all four themes, repeated output is byte-identical, Audy Vault is unchanged, existing canary and default-profile isolation remain green, and no retrieval implementation has entered the branch.
