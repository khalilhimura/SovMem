> **ARCHIVED** — This implementation plan has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix A: Implementation Plan Index](../sovmem-master-spec.md#appendix-a-implementation-plan-index) for a summary of its scope.
>
# SovMem P3 Verified Write-Back Loop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and close the complete P3 gate: immutable agent proposals, deterministic policy, CLI-only human verdicts, exact verified canonical writes, full P2 rebuilds, an append-only BLAKE3 verdict chain, Level 4 verification, safe recovery, and a reviewed safe-clone proof without changing frozen P2 evidence.

**Architecture:** Add a separately named `audy-sovmem-writeback-v1` store seeded from the verified Level 3 export while leaving Audy Vault and the frozen P2 lane immutable. Keep proposal intake public and staging-only; keep the canonical transaction crate-private and reachable in production only from the interactive verdict CLI. Treat the ledger append as the commit point, make canonical generations immutable copy-on-write values, build and self-check a complete P2 index before each applied ledger commit, and derive `CURRENT` from the committed chain with fail-closed recovery.

**Tech Stack:** One Rust 1.84.0 crate; existing exact pins `anyhow =1.0.95`, `blake3 =1.5.5`, `clap =4.5.27`, `serde =1.0.219`, `serde_json =1.0.140`, `serde_yaml =0.9.34`, `tantivy =0.24.2`, `petgraph =0.8.3`, and `thiserror =2.0.11`; Node.js 20 ESM and `node:test`; deterministic JSON/JSONL, Markdown, canonical TOML subset, filesystem fsync/rename primitives, and Git only as manual outer snapshot history.

## Global Constraints

- The approved design is `docs/superpowers/specs/2026-07-14-sovmem-p3-verified-write-back-loop-design.md` at commit `fde4e15c11a7edfb6377fec53756594841cd9a5e`.
- `docs/sovmem_v0.3_execution_plan.md` remains normative. Stop on any genuine contradiction; do not silently weaken either authority.
- Work only in `.worktrees/sovmem-p3-verified-write-back-loop` on `codex/sovmem-p3-verified-write-back-loop` until the branch is manually reviewed.
- The implementation base is `origin/main` commit `73c4be50e7df7ebcd1eec8cf2f1dedf4809348ba` plus the approved design commit.
- Preserve every existing linked worktree. Do not remove, move, unlock, or repurpose another worktree.
- Preserve the single Rust crate, edition 2021, `rust-version = "1.84"`, and `rust-toolchain.toml` channel `1.84.0`.
- Add no dependency. If a parser, locking primitive, timestamp library, async runtime, signature system, or other crate appears necessary, stop for explicit approval and an exact version pin.
- Keep Tantivy exactly `0.24.2` and Petgraph exactly `0.8.3`; keep the P2 tokenizer, schema, boosts, ordering, graph rules, and full-rebuild semantics unchanged.
- Keep the binary offline: no HTTP, socket, external model, embeddings, vector search, Qdrant, or runtime network call.
- Use no `unsafe`. Use no async outside a future P4 MCP module.
- Audy Vault remains the canonical external context and is read-only to P3. The live Vault is never a P3 write target.
- Keep `profiles/audy-ssa0-v0/profile.yaml`, the 62-document P2 corpus, golden queries, thresholds, relevance labels, and `evals/audy-sovmem-v0/baseline.json` byte-identical.
- The evolving lane is exactly `store_id = "audy-sovmem-writeback-v1"` with profile version `audy-ssa0-p3-v1`; consumers must name the lane they read.
- The seed is a byte-for-byte copy of the verified P2 Level 3 export and is immutable after genesis.
- Agents may write only proposal staging and the intake lease. No agent, MCP surface, library API, confidence field, evaluator block, or trust value may commit canon.
- A new verdict is CLI-only, human-only, and interactive through the real controlling terminal. There is no non-interactive, `--yes`, `--force`, environment, stdin, library, or MCP bypass.
- Internal verdict grammar is exactly `ACCEPT|REJECT|REWRITE|TP|FP`; the public pedagogical subset remains `ACCEPT|REJECT|REWRITE`.
- Every verb requires a non-empty reason. ACCEPT applies candidate raw bytes exactly. REWRITE applies human-supplied raw bytes exactly. REJECT, TP, and FP never mutate canon or index.
- Confidence is optional, fixed to the inclusive range 0 through 1 with at most six decimal places, captured before draft by the harness, persisted as untrusted evidence, and never authoritative.
- Evaluator and skeptic material is advisory only. The skeptic stays outside the binary. Erasing confidence and evaluator fields must leave policy results byte-identical.
- Keep provenance exactly `tier1|secondary|synthesis|unknown`; every new resource contains an explicit value.
- Agents cannot propose `judgments/` or type `judgment`. Only ACCEPT and REWRITE transactions mint deterministic Human Judgment documents.
- Hash exact raw bytes. Do not normalize Markdown, rewrite YAML, append a newline, or reserialize approved target bytes.
- The ledger is logical append-only JSONL. Every successor must preserve all prior bytes as an exact prefix.
- The ledger event is the transaction commit point. `canon/CURRENT` is a recoverable cache derived from the committed chain.
- Build, reopen, and self-check a complete P2 index before an ACCEPT or REWRITE ledger commit. The derived index remains disposable and full-rebuild-only.
- Level 4 validates the seed, complete chain, archived policies, retained proposals, every applied generation, every re-index receipt, and the ledger-derived current head.
- Production filesystem semantics are macOS/Linux same-directory atomic rename plus file and directory fsync. Lease directories use the approved no-replace create-directory plus hard-linked-owner protocol documented in the design, because Rust 1.84 `std::fs` does not provide a no-replace directory rename on macOS. Unsupported semantics fail closed with exit 2.
- Stable diagnostics are sorted, deduplicated JSON Lines on stderr; failures emit no stdout and never expose bodies, reasons, advice, query text, paths, temporary names, PIDs, usernames, hostnames, source excerpts, or OS error strings.
- Git is optional manual outer snapshot history only. The binary does not call Git and Git never substitutes for the verdict ledger.
- Do not implement P4 MCP transport, an MCP verdict/write tool, P5 sentry/trust/dogfood work, TTF/outcome calibration, automatic promotion, source-vault synchronization, or live Git automation.
- The private repository uses manual PR discipline only. Never claim branch protection, required checks, rulesets, or merge queues are enforced.
- Starting baseline is 55 JavaScript tests and 285 Rust tests. Record final counts from actual output, never prediction.
- Every implementation task follows RED -> GREEN -> focused regression -> commit. Do not combine task commits.
- Do not update `docs/sovmem-build-state.json` or regenerate `docs/SOVMEM-BUILD-AUDIT.md` until every P3 implementation, red-team, 20-verdict, safe-clone, frozen-P2, formatting, lint, and full-suite gate has passed.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `profiles/audy-ssa0-p3-v1/profile.yaml` | Versioned P3 profile: exact P2 mappings plus the non-overlapping Human Judgment mapping. |
| `profiles/audy-ssa0-p3-v1/policy.toml` | Reviewed initial canonical-TOML policy template copied into an approved store during safe provisioning. |
| `sovmem/src/profile/clock.rs` | Exact UTC timestamp parsing/formatting and private production/injected test clocks without a new dependency. |
| `sovmem/src/profile/store.rs` | Store paths, root-shape/symlink checks, seed/head inspection, file fsync/rename helpers, and read-only authority snapshots. |
| `sovmem/src/profile/policy.rs` | Strict canonical-TOML parser, raw policy hash, invariant checks, ordered `PolicyEvaluator`, and stable rule codes. |
| `sovmem/src/profile/proposal.rs` | Proposal/evaluator schemas, confidence representation, identifiers, raw-byte binding, atomic staging, receipts, dedupe, quotas, lifecycle, and list/status projections. |
| `sovmem/src/profile/judgment.rs` | Five-verb grammar, reason/rewrite validation, verdict IDs, decision material, and deterministic Human Judgment bytes. |
| `sovmem/src/profile/canon.rs` | P3 manifest, copy-on-write generation construction, raw-byte rehashing, `index.md` reconstruction, and P3 Level 1-3 verification. |
| `sovmem/src/profile/ledger.rs` | Canonical genesis/verdict JSONL, prefix-preserving append, event hashing, chain parsing, and Level 4 verification. |
| `sovmem/src/profile/transaction.rs` | Private human authority, ordered leases, journals, ACCEPT/REWRITE and ledger-only transactions, fsync boundaries, faults, idempotency, and recovery. |
| `sovmem/src/profile/p3_test_support.rs` | `#[cfg(test)]` deterministic clock, scripted human, fault plan, store fixture, and transaction snapshots for private unit tests. |
| `sovmem/src/profile/config.rs` | P3 profile loader and exact Judgment-extension validation while preserving the frozen P2 loader. |
| `sovmem/src/profile/validate.rs` | Permit only the reviewed P3 Judgment mapping and keep arbitrary/agent Judgment mappings forbidden. |
| `sovmem/src/profile/verify.rs` | Reusable raw-byte helpers and `VerificationLevel::Four` dispatch without weakening Levels 1-3. |
| `sovmem/src/profile/index.rs` | Versioned frozen/P3 index inputs, P3 full generation build/open, exact input binding, and deterministic rebuild evidence. |
| `sovmem/src/profile/query.rs` | Store-mode query opening against the ledger head and exact re-index receipt; frozen bundle mode stays unchanged. |
| `sovmem/src/profile/stats.rs` | Sealed/unsealed P3 queue alarm and ledger statistics while preserving bundle-mode bytes. |
| `sovmem/src/profile/mod.rs` | Register P3 modules; keep transaction APIs crate-private and test support test-only. |
| `sovmem/src/cli/p3.rs` | Redacted handlers/renderers for propose, proposal/verdict lists, verdict, recover, Level 4, and store-mode index/query/stats. |
| `sovmem/src/cli.rs` | Clap shapes, mutually exclusive bundle/store modes, structured P3 usage errors, and command dispatch. |
| `sovmem/src/cli/p2.rs` | Preserve frozen P2 handlers while sharing versioned index/query internals. |
| `sovmem/tests/support/p3.rs` | Public/read-only integration fixture provisioning and CLI helpers; no human verdict bypass. |
| `sovmem/tests/p3_policy.rs` | P3 profile, policy parser, rule ordering, advisory erasure, quota, and poisoning fixtures. |
| `sovmem/tests/p3_store.rs` | Store shape, seed, path, symlink, timestamp, and capability-boundary fixtures. |
| `sovmem/tests/p3_proposal.rs` | Proposal schema, raw binding, atomic staging, receipts, duplicates, lifecycle, and queue fixtures. |
| `sovmem/tests/p3_canon.rs` | Judgment and manifest bytes, copy-on-write generation, P3 Level 1-3, corruption, and frozen-seed fixtures. |
| `sovmem/tests/p3_index.rs` | P3 full rebuild, versioned manifest, re-index receipt, delete/rebuild, and store-query fixtures. |
| `sovmem/tests/p3_ledger.rs` | Genesis, hash links, canonical lines, five-verb semantics, tampering, and Level 4 fixtures. |
| `sovmem/tests/p3_cli.rs` | Exact JSON/text contracts, exit classes, no-TTY failures, list/status/store modes, and P0-P2 compatibility. |
| `sovmem/tests/p3_privacy.rs` | Sentinel-driven body/reason/advice/query/path/temporary/OS-detail redaction fixtures. |
| `sovmem/tests/p3_determinism.rs` | Two-store identity, mixed 20-verdict chain, full loop, deleted-index rebuild, and frozen-P2 isolation. |
| `sovmem/examples/sovmem-p3-raw-hash.rs` | Harness-only read-only raw-file BLAKE3 helper using the crate's already pinned dependency; outputs a hash only and is not a `sovmem` command. |
| `scripts/provision-sovmem-p3-safe-clone.mjs` | Harness-only safe-store provisioning with root-overlap refusal and no Vault write/sync behavior. |
| `scripts/verify-sovmem-p3-safe-clone.mjs` | Harness-only before/after hash and Git checks plus redacted proof publication after a real human CLI verdict. |
| `test/sovmem-p3-safe-clone.test.js` | Synthetic provisioning, overlap, immutability, redaction, and evidence-schema tests for the harness scripts. |
| `docs/evidence/sovmem-p3-safe-clone-proof.json` | Reviewed redacted real-data proof, created only after separate safe-clone authorization and success. |
| `package.json` | Portable P3 safe-provision/proof and read-only store command aliases; no live path is embedded. |
| `README.md` | Exact P3 authority lanes, CLI boundaries, safe-clone use, and public three-verb teaching subset. |
| `index.md` | Durable registry entries for P3 code, tests, profiles, evidence, design, and plan. |
| `docs/sovmem_v0.3_execution_plan.md` | After the full gate only, record implemented P3 behavior without changing P4/P5 scope. |
| `scripts/lib/sovmem-build-audit.mjs` | After the full gate only, validate P3 evidence, exact phase state, and 62.96 percent weighting. |
| `test/sovmem-build-audit.test.js` | After the full gate only, reject incomplete or forged P3 closure evidence and preserve manual PR claims. |
| `docs/sovmem-build-state.json` | After the full gate only, reviewed P3=100/passed state and actual implementation evidence. |
| `docs/SOVMEM-BUILD-AUDIT.md` | After the full gate only, regenerated approximately 63 percent report. |

## Spec-to-Task Coverage

| Approved design area | Implemented and gated by |
| --- | --- |
| Authority target, versioned lane, frozen P2 isolation | Tasks 1, 2, 6, 7, 18 |
| Threat model and capability boundary | Tasks 2, 4, 11, 14, 17 |
| Exact pins and single-crate boundary | Tasks 1 and 19 |
| Proposal schema, IDs, confidence, evaluator, intake receipts | Tasks 3 and 4 |
| Proposal lifecycle, list/status, queue alarm | Tasks 9 and 15 |
| Policy schema, defaults, order, stable rule IDs | Tasks 1 and 4 |
| Five verbs, reason, REWRITE, Human Judgment | Tasks 5, 12, 13 |
| Canon manifest, raw hashes, P3 Level 1-3 | Task 6 |
| Full P2 rebuild, receipt, re-query compatibility | Tasks 7, 13, 16, 17 |
| Ledger encoding, chain, Level 4 | Tasks 8 and 9 |
| Atomic transaction and ledger commit point | Tasks 10, 12, 13 |
| Locks, concurrency, crash recovery, idempotency | Tasks 10, 12, 14 |
| CLI/output/error/privacy contracts | Tasks 15, 16, 17 |
| Deterministic timestamps/IDs/test injection | Tasks 2, 3, 5, 10, 17 |
| Red-team and 20 mixed verdicts | Task 17 |
| Real-data safe-clone proof | Task 18 |
| Audit publication and approximately 63 percent | Task 19 |

---

### Task 1: Add the versioned P3 profile and strict canonical policy

**Files:**
- Create: `profiles/audy-ssa0-p3-v1/profile.yaml`
- Create: `profiles/audy-ssa0-p3-v1/policy.toml`
- Create: `sovmem/src/profile/policy.rs`
- Create: `sovmem/tests/p3_policy.rs`
- Modify: `sovmem/src/profile/config.rs`
- Modify: `sovmem/src/profile/validate.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `sovmem/tests/profile_config.rs`
- Modify: `sovmem/tests/validation.rs`
- Modify: `test/retrieval-corpus-version.test.js`

**Interfaces:**
- Produces `P3_PROFILE_VERSION`, `P3_STORE_ID`, `P3Profile { config, version, hash, bytes }`, and `load_p3_profile(path) -> Result<P3Profile, ProfileError>`.
- Produces `Policy`, `LoadedPolicy { policy, hash, bytes }`, `PolicyError`, and `load_policy(path) -> Result<LoadedPolicy, PolicyError>`.
- Produces exact canonical-TOML helpers used later by proposal intake and Level 4.
- Keeps `load_profile` and every frozen P2 profile behavior unchanged.

- [ ] **Step 1: Write failing profile-extension and policy-contract tests**

Create `sovmem/tests/p3_policy.rs` with these first fixtures:

```rust
use sovmem::profile::{load_p3_profile, load_policy, P3_PROFILE_VERSION, P3_STORE_ID};
use std::path::Path;

fn repository_path(relative: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(relative)
}

#[test]
fn loads_the_exact_versioned_profile_and_policy() {
    let profile = load_p3_profile(&repository_path(
        "profiles/audy-ssa0-p3-v1/profile.yaml",
    ))
    .expect("load P3 profile");
    let policy = load_policy(&repository_path(
        "profiles/audy-ssa0-p3-v1/policy.toml",
    ))
    .expect("load P3 policy");

    assert_eq!(profile.version, P3_PROFILE_VERSION);
    assert_eq!(policy.policy.store_id, P3_STORE_ID);
    assert_eq!(policy.policy.profile_version, P3_PROFILE_VERSION);
    assert_eq!(profile.hash.len(), 64);
    assert_eq!(policy.hash.len(), 64);
    assert!(profile.config.context_kinds.contains_key("Human Judgment"));
    assert_eq!(policy.policy.provenance_values, [
        "tier1", "secondary", "synthesis", "unknown"
    ]);
}

#[test]
fn rejects_noncanonical_policy_bytes_before_evaluation() {
    for (name, bytes, code) in [
        ("unknown", b"unknown = true
".as_slice(), "policy.unknown_field"),
        ("crlf", b"schema_version = \"sovmem-policy-v1\"
".as_slice(), "policy.line_endings"),
        ("comment", b"schema_version = \"sovmem-policy-v1\" # no
".as_slice(), "policy.syntax"),
        ("no-final-lf", b"schema_version = \"sovmem-policy-v1\"".as_slice(), "policy.line_endings"),
    ] {
        let error = sovmem::profile::parse_policy(bytes)
            .expect_err(name);
        assert_eq!(error.code(), code, "{name}");
    }
}
```

Extend `test/retrieval-corpus-version.test.js` with a dependency/scope lock:

```js
test('keeps P3 inside the pinned offline single-crate boundary', () => {
  const cargo = readFileSync('sovmem/Cargo.toml', 'utf8');
  assert.doesNotMatch(cargo, /toml\s*=|chrono\s*=|time\s*=|tokio\s*=|reqwest\s*=|qdrant|embedding|vector/i);
  assert.match(cargo, /rust-version = "1\.84"/);
  assert.match(cargo, /tantivy = \{ version = "=0\.24\.2"/);
  assert.match(cargo, /petgraph = \{ version = "=0\.8\.3"/);
});
```

- [ ] **Step 2: Run the focused tests and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_policy
node --test test/retrieval-corpus-version.test.js
```

Expected: Rust fails because the P3 profile/policy modules and files do not exist. The Node dependency lock passes against the unchanged manifest.

- [ ] **Step 3: Create the exact profile extension**

Copy `profiles/audy-ssa0-v0/profile.yaml` byte-for-byte to `profiles/audy-ssa0-p3-v1/profile.yaml`, then make only these insertions:

```yaml
context_kinds:
  Human Judgment: judgment
```

Insert the mapping before every broader rule that could overlap it:

```yaml
mapping_rules:
  - { pattern: "judgments/*.md", core_type: judgment, context_kind: "Human Judgment", layer: concept, authority: canonical }
```

Insert the explicit sensitivity rule before the default:

```yaml
sensitivity_rules:
  - { pattern: "judgments/*.md", sensitivity: internal }
```

Do not change the identity tuple or any existing P2 mapping/sensitivity/public-allowlist byte sequence.

- [ ] **Step 4: Create the exact reviewed policy template**

Create `profiles/audy-ssa0-p3-v1/policy.toml` with exactly these bytes and one final LF:

```toml
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
```

- [ ] **Step 5: Add the P3 profile loader without weakening P2**

Add to `sovmem/src/profile/config.rs`:

```rust
pub const P3_PROFILE_VERSION: &str = "audy-ssa0-p3-v1";
pub const P3_STORE_ID: &str = "audy-sovmem-writeback-v1";

#[derive(Clone, Debug)]
pub struct P3Profile {
    pub config: ProfileConfig,
    pub version: String,
    pub hash: String,
    pub bytes: Vec<u8>,
}

pub fn load_p3_profile(path: &Path) -> Result<P3Profile, ProfileError> {
    let bytes = fs::read(path).map_err(|error| ProfileError::Parse(error.to_string()))?;
    let version = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .ok_or_else(|| ProfileError::Identifier("P3 profile version path is invalid".into()))?;
    if version != P3_PROFILE_VERSION {
        return Err(ProfileError::Identifier(
            "P3 profile path must carry the reviewed version".into(),
        ));
    }
    let source = std::str::from_utf8(&bytes)
        .map_err(|error| ProfileError::Parse(error.to_string()))?;
    let config: ProfileConfig =
        serde_yaml::from_str(source).map_err(|error| ProfileError::Parse(error.to_string()))?;
    config.validate_p3()?;
    Ok(P3Profile {
        config,
        version: version.to_owned(),
        hash: blake3::hash(&bytes).to_hex().to_string(),
        bytes,
    })
}
```

Refactor `ProfileConfig::validate` into a shared identity/mapping validator plus two exact entry points:

```rust
impl ProfileConfig {
    pub fn validate(&self) -> Result<(), ProfileError> {
        self.validate_with_judgment_mapping(false)
    }

    pub(crate) fn validate_p3(&self) -> Result<(), ProfileError> {
        self.validate_with_judgment_mapping(true)?;
        let judgment_kinds = self
            .context_kinds
            .iter()
            .filter(|(_, value)| **value == CoreType::Judgment)
            .collect::<Vec<_>>();
        let judgment_rules = self
            .mapping_rules
            .iter()
            .filter(|rule| rule.core_type == CoreType::Judgment)
            .collect::<Vec<_>>();
        if judgment_kinds.len() != 1
            || judgment_kinds[0].0 != "Human Judgment"
            || judgment_rules.len() != 1
            || judgment_rules[0].pattern != "judgments/*.md"
            || judgment_rules[0].context_kind != "Human Judgment"
            || judgment_rules[0].layer != Layer::Concept
            || judgment_rules[0].authority != Authority::Canonical
            || self.sensitivity_for("judgments/ver-example.md") != Sensitivity::Internal
        {
            return Err(ProfileError::KindMapping(
                "P3 requires the exact reviewed Human Judgment extension".into(),
            ));
        }
        Ok(())
    }
}
```

The shared validator must preserve all current checks and permit `CoreType::Judgment` only when `allow_judgment_mapping` is true. Add regression tests showing `load_profile` still rejects any Judgment mapping and `load_p3_profile` rejects extra, renamed, broader, non-canonical, or differently sensitive Judgment rules.

In `validate_judgment`, replace the unconditional mapping prohibition with an exact-P3-extension predicate; arbitrary Judgment mappings must still emit `judgment.mapping_forbidden`.

- [ ] **Step 6: Implement the canonical policy parser and invariants**

Create `sovmem/src/profile/policy.rs` with these public contracts:

```rust
use serde::Serialize;
use std::path::Path;
use thiserror::Error;

pub const POLICY_SCHEMA: &str = "sovmem-policy-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Policy {
    pub schema_version: String,
    pub store_id: String,
    pub profile_version: String,
    pub human_actor: String,
    pub allowed_proposer_prefixes: Vec<String>,
    pub allowed_evaluator_prefixes: Vec<String>,
    pub evaluator_protocol: String,
    pub allowed_targets: Vec<String>,
    pub protected_layers: Vec<String>,
    pub forbidden_proposal_types: Vec<String>,
    pub provenance_values: Vec<String>,
    pub max_candidate_bytes: usize,
    pub max_meta_bytes: usize,
    pub max_rewrite_bytes: usize,
    pub max_reason_bytes: usize,
    pub max_evaluator_reasons: usize,
    pub max_evaluator_reason_bytes: usize,
    pub max_open_per_source: usize,
    pub max_submissions_per_source_utc_day: usize,
    pub max_open_total: usize,
    pub queue_alarm_at: usize,
    pub bad_accept_cost: usize,
    pub human_verdict_cost: usize,
}

#[derive(Clone, Debug)]
pub struct LoadedPolicy {
    pub policy: Policy,
    pub hash: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("policy could not be read")]
    Read,
    #[error("policy is not UTF-8")]
    Utf8,
    #[error("policy line encoding is invalid")]
    LineEndings,
    #[error("policy syntax is invalid")]
    Syntax,
    #[error("policy key order is invalid")]
    KeyOrder,
    #[error("policy contains a duplicate key")]
    DuplicateKey,
    #[error("policy contains an unknown field")]
    UnknownField,
    #[error("policy is missing a field")]
    MissingField,
    #[error("policy schema is unsupported")]
    SchemaUnsupported,
    #[error("policy value is invalid")]
    ValueInvalid,
    #[error("policy invariant is invalid")]
    InvariantInvalid,
    #[error("policy changed during transaction")]
    ChangedDuringTransaction,
}

pub fn load_policy(path: &Path) -> Result<LoadedPolicy, PolicyError> {
    let bytes = std::fs::read(path).map_err(|_| PolicyError::Read)?;
    let policy = parse_policy(&bytes)?;
    Ok(LoadedPolicy {
        hash: blake3::hash(&bytes).to_hex().to_string(),
        policy,
        bytes,
    })
}
```

Implement `parse_policy` as a fixed 23-key state machine: require UTF-8, LF only, exactly one final LF, the exact key order from the template, exact `key = value` spacing, no comments/escapes/tables/dotted keys/floats, quoted ASCII strings, decimal unsigned integers, and arrays of quoted ASCII strings. Validate exact schema/store/profile IDs, exact provenance enum/order, `queue_alarm_at <= max_open_total`, nonzero sizes/quotas, cost values, source-prefix syntax, unique arrays, and the reviewed glob grammar (`*` inside one component, `**` only as a complete component, no other metacharacters).

Expose `PolicyError::code()` with the exact codes in design Section 13.3. Do not include parser input in any error.

- [ ] **Step 7: Run GREEN and full profile regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_policy
cargo test --locked --manifest-path sovmem/Cargo.toml --test profile_config
cargo test --locked --manifest-path sovmem/Cargo.toml --test validation
node --test test/retrieval-corpus-version.test.js
```

Expected: all focused tests pass; frozen P2 profile and Judgment-forbidden behavior remain green.

- [ ] **Step 8: Commit the profile/policy foundation**

```bash
git add profiles/audy-ssa0-p3-v1/profile.yaml profiles/audy-ssa0-p3-v1/policy.toml sovmem/src/profile/policy.rs sovmem/src/profile/config.rs sovmem/src/profile/validate.rs sovmem/src/profile/mod.rs sovmem/tests/p3_policy.rs sovmem/tests/profile_config.rs sovmem/tests/validation.rs test/retrieval-corpus-version.test.js
git commit -m "P3.1: add versioned profile and strict policy"
```

---

### Task 2: Build the strict store reader, exact clock, and synthetic P3 fixture

**Files:**
- Create: `sovmem/src/profile/clock.rs`
- Create: `sovmem/src/profile/store.rs`
- Create: `sovmem/tests/support/p3.rs`
- Create: `sovmem/tests/p3_store.rs`
- Modify: `sovmem/tests/support/mod.rs`
- Modify: `sovmem/src/profile/mod.rs`

**Interfaces:**
- Produces `Timestamp`, private `Clock`, `SystemClock`, and exact timestamp validation/formatting.
- Produces `StorePaths`, `CanonicalHead`, `StoreSnapshot`, `StoreError`, `inspect_store`, `verify_unsealed_store`, and crate-private atomic/fsync helpers.
- Produces integration `P3Fixture` that provisions only a temporary synthetic store from `P2Fixture`; it is not a production init API.
- Establishes zero-byte `ledger/verdicts.jsonl` and `canon/CURRENT = b"seed
"` as the exact unsealed state.

- [ ] **Step 1: Write failing clock and store-authority fixtures**

Create `sovmem/tests/p3_store.rs`:

```rust
#[allow(dead_code)]
mod support;

use sovmem::profile::{inspect_store, load_p3_profile, Timestamp};
use support::p3::P3Fixture;

#[test]
fn timestamp_contract_is_exact_and_calendar_valid() {
    for valid in [
        "1970-01-01T00:00:00.000000000Z",
        "2024-02-29T23:59:59.999999999Z",
        "2026-07-14T04:30:00.000000000Z",
    ] {
        assert_eq!(Timestamp::parse(valid).unwrap().as_str(), valid);
    }
    for invalid in [
        "2026-07-14T04:30:00Z",
        "2026-02-29T04:30:00.000000000Z",
        "2026-07-14T24:00:00.000000000Z",
        "2026-07-14T04:30:60.000000000Z",
        "2026-07-14T04:30:00.000000000+00:00",
    ] {
        assert!(Timestamp::parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn unsealed_store_is_a_verified_seed_not_level_four() {
    let fixture = P3Fixture::new("unsealed-store");
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    let store = inspect_store(&profile, &fixture.store_root).unwrap();

    assert_eq!(store.head.as_str(), "seed");
    assert_eq!(store.seed.documents, 62);
    assert_eq!(store.seed.bundle_hash, fixture.p2_bundle_hash());
    assert!(!store.sealed);
    assert_eq!(std::fs::read(&store.paths.current).unwrap(), b"seed
");
    assert_eq!(std::fs::read(&store.paths.ledger).unwrap(), b"");
}
```

Add table-driven corruptions for extra root entries, missing directories, symlinked files/directories, non-UTF-8 names when supported, malformed `CURRENT`, wrong policy store/profile identity, changed seed bytes, seed missing Level 3 evidence, and a store root nested in its seed. Each assertion checks only a stable code.

- [ ] **Step 2: Run focused store tests and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_store
```

Expected: compile failure because `clock`, `store`, and `P3Fixture` do not exist.

- [ ] **Step 3: Implement exact UTC timestamps without a dependency**

Create `sovmem/src/profile/clock.rs` with this interface:

```rust
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(String);

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ClockError {
    #[error("timestamp format is invalid")]
    Invalid,
    #[error("system clock is invalid")]
    System,
}

impl Timestamp {
    pub fn parse(value: &str) -> Result<Self, ClockError> {
        validate_timestamp_bytes(value.as_bytes())?;
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn utc_date(&self) -> &str {
        &self.0[..10]
    }
}

pub(crate) trait Clock {
    fn now(&self) -> Result<Timestamp, ClockError>;
}

pub(crate) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Result<Timestamp, ClockError> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ClockError::System)?;
        timestamp_from_unix(elapsed.as_secs(), elapsed.subsec_nanos())
    }
}
```

Implement fixed-position parsing for exactly `YYYY-MM-DDTHH:MM:SS.NNNNNNNNNZ`, Gregorian leap-year/day validation, hours 00-23, minutes/seconds 00-59, and nine decimal nanoseconds. Format `SystemTime` using integer day/second arithmetic and a reviewed proleptic-Gregorian `civil_from_days` helper; test Unix epoch, leap days, year boundaries, and nanosecond padding. Do not accept leap seconds or timezone offsets.

- [ ] **Step 4: Implement strict read-only store inspection**

Create `sovmem/src/profile/store.rs` around these exact types:

```rust
use super::{LoadedPolicy, P3Profile};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct StorePaths {
    pub root: PathBuf,
    pub policy: PathBuf,
    pub policies: PathBuf,
    pub seed: PathBuf,
    pub staging: PathBuf,
    pub ledger: PathBuf,
    pub canon: PathBuf,
    pub current: PathBuf,
    pub evidence: PathBuf,
    pub derived: PathBuf,
    pub transactions: PathBuf,
    pub locks: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalHead {
    Seed,
    Generation(String),
}

impl CanonicalHead {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Seed => "seed",
            Self::Generation(value) => value,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SeedEvidence {
    pub documents: usize,
    pub manifest_hash: String,
    pub corpus_report_hash: String,
    pub bundle_hash: String,
    pub source_commit: String,
}

#[derive(Clone, Debug)]
pub struct StoreSnapshot {
    pub paths: StorePaths,
    pub policy: LoadedPolicy,
    pub profile_hash: String,
    pub seed: SeedEvidence,
    pub head: CanonicalHead,
    pub sealed: bool,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("store could not be read")]
    Read,
    #[error("store layout is invalid")]
    LayoutInvalid,
    #[error("store path is unsafe")]
    PathUnsafe,
    #[error("store contains a symlink")]
    Symlink,
    #[error("store roots cross filesystems")]
    CrossFilesystem,
    #[error("store is locked")]
    Locked,
    #[error("transaction is incomplete")]
    Incomplete,
    #[error("store write failed")]
    Write,
}
```

`inspect_store` must canonicalize the existing root, require the exact durable directories, permit only the named transient lock/journal/temp forms, reject every symlink with `symlink_metadata`, validate current policy/profile IDs and hashes, verify `seed/` at explicit Level 3, derive sealed state from the zero-byte or valid-genesis ledger, and validate `CURRENT` as exactly `seed
` or `gen-` plus 16 digits and LF. It must not write.

Make existing `verify.rs` helpers for raw bundle hashing and safe relative paths `pub(crate)` rather than copying divergent implementations. Add `sync_file`, `sync_directory`, `write_new_synced`, and `atomic_replace_synced` as crate-private helpers that use only `std::fs`, same-parent temporary paths, `File::sync_all`, rename, and parent sync; no public init function is added.

- [ ] **Step 5: Add the temporary P3 fixture without creating a production init path**

Create `sovmem/tests/support/p3.rs`:

```rust
use super::p2::P2Fixture;
use std::fs;
use std::path::{Path, PathBuf};

pub struct P3Fixture {
    p2: P2Fixture,
    pub profile_path: PathBuf,
    pub policy_template: PathBuf,
    pub store_root: PathBuf,
}

impl P3Fixture {
    pub fn new(name: &str) -> Self {
        let p2 = P2Fixture::new(name);
        let store_root = p2.index_root("p3-store");
        let profile_path = repository_path("profiles/audy-ssa0-p3-v1/profile.yaml");
        let policy_template = repository_path("profiles/audy-ssa0-p3-v1/policy.toml");
        provision_test_store(&p2.bundle_root, &policy_template, &store_root);
        Self {
            p2,
            profile_path,
            policy_template,
            store_root,
        }
    }

    pub fn p2_bundle_hash(&self) -> String {
        self.p2.bundle_hash()
    }
}
```

`provision_test_store(seed, policy_template, store)` first verifies the synthetic export against the frozen P2 profile, copies it byte-for-byte to `seed/`, creates the exact durable directory set, copies the supplied policy template to root `policy.toml`, writes zero-byte `ledger/verdicts.jsonl`, writes `seed
` to `canon/CURRENT`, verifies the copy again against the P3 profile's unchanged seed mappings, and leaves every other durable directory empty. Its visibility remains inside integration-test support.

- [ ] **Step 6: Run GREEN and frozen verifier regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_store
cargo test --locked --manifest-path sovmem/Cargo.toml --test bundle_verification
cargo test --locked --manifest-path sovmem/Cargo.toml --test p2_determinism
```

Expected: all tests pass; P3 inspection verifies a 62-document seed; existing Level 1-3 and P2 determinism remain green.

- [ ] **Step 7: Commit the store/clock foundation**

```bash
git add sovmem/src/profile/clock.rs sovmem/src/profile/store.rs sovmem/src/profile/verify.rs sovmem/src/profile/mod.rs sovmem/tests/support/mod.rs sovmem/tests/support/p3.rs sovmem/tests/p3_store.rs
git commit -m "P3.2: verify store authority and exact time"
```

---

### Task 3: Define and atomically bind immutable proposal pairs

**Files:**
- Create: `sovmem/src/profile/proposal.rs`
- Create: `sovmem/tests/p3_proposal.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `sovmem/tests/support/p3.rs`

**Interfaces:**
- Produces `ProposalOperation`, `ProposalIntent`, `Confidence`, `EvaluatorBlock`, `ProposalBase`, `ProposalMeta`, `ProposalPair`, `ProposalSubmission`, `ProposalSummary`, and `ProposalError`.
- Produces crate-private `prepare_proposal`, `proposal_id`, `meta_hash`, `read_proposal_pair`, and `publish_proposal_pair`.
- Proposal ID uses BLAKE3 domain `sovmem.proposal.v1` and little-endian `u64` length frames in the design field order.
- This task publishes only structurally valid pairs under a fixture-held intake lease; Task 4 adds full policy evaluation and the public submit path.

- [ ] **Step 1: Write failing schema, hash, and atomic-publication tests**

Create `sovmem/tests/p3_proposal.rs` with exact format assertions:

```rust
#[allow(dead_code)]
mod support;

use sovmem::profile::{Confidence, ProposalIntent, ProposalOperation, Timestamp};

#[test]
fn confidence_accepts_only_canonical_millionths() {
    for (source, millionths) in [
        ("0", 0),
        ("0.000001", 1),
        ("0.75", 750_000),
        ("1", 1_000_000),
    ] {
        assert_eq!(Confidence::parse(source).unwrap().millionths(), millionths);
    }
    for invalid in ["-0.1", "1.000001", ".5", "0.", "0.1234567", "NaN", "inf"] {
        assert!(Confidence::parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn proposal_enums_are_frozen() {
    assert_eq!(ProposalOperation::Create.as_str(), "create");
    assert_eq!(ProposalOperation::Replace.as_str(), "replace");
    assert_eq!(ProposalIntent::Accept.as_str(), "accept");
    assert_eq!(ProposalIntent::Rewrite.as_str(), "rewrite");
    assert_eq!(
        Timestamp::parse("2026-07-14T04:30:00.000000000Z")
            .unwrap()
            .as_str(),
        "2026-07-14T04:30:00.000000000Z"
    );
}
```

Add fixture-only tests that prepare the same semantic submission twice and require the same `prp-` plus 64 lowercase hex ID and identical canonical `proposal.meta.json` bytes; changing any timestamp, source, operation, intent, target, base hash, candidate byte, confidence presence/value, or evaluator commitment must change the ID. Add failpoint tests after candidate sync, metadata sync, and before/after directory rename, asserting either no visible proposal or one complete immutable pair.

- [ ] **Step 2: Run focused proposal tests and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_proposal
```

Expected: compile failure because the proposal types and pair publisher do not exist.

- [ ] **Step 3: Implement exact proposal values and canonical serialization**

Create `sovmem/src/profile/proposal.rs` with the exact wire types:

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProposalOperation {
    Create,
    Replace,
}

impl ProposalOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Replace => "replace",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProposalIntent {
    Accept,
    Rewrite,
}

impl ProposalIntent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Rewrite => "rewrite",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Confidence(u32);

impl Confidence {
    pub fn parse(source: &str) -> Result<Self, ProposalError> {
        let (whole, fractional) = source.split_once('.').unwrap_or((source, ""));
        if source.ends_with('.')
            || !matches!(whole, "0" | "1")
            || fractional.len() > 6
            || !fractional.bytes().all(|byte| byte.is_ascii_digit())
            || (whole == "1" && fractional.bytes().any(|byte| byte != b'0'))
        {
            return Err(ProposalError::ConfidenceInvalid);
        }
        let mut digits = fractional.to_owned();
        while digits.len() < 6 {
            digits.push('0');
        }
        let fraction = digits
            .parse::<u32>()
            .map_err(|_| ProposalError::ConfidenceInvalid)?;
        Ok(Self(if whole == "1" { 1_000_000 } else { fraction }))
    }

    pub const fn millionths(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvaluatorBlock {
    pub protocol: String,
    pub source_id: String,
    pub evaluated_at: String,
    pub candidate_hash: String,
    pub recommendation: String,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProposalBase {
    pub kind: String,
    pub generation_id: Option<String>,
    pub bundle_hash: String,
    pub manifest_hash: String,
    pub target_hash: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProposalMeta {
    pub schema_version: String,
    pub proposal_id: String,
    pub submitted_at: String,
    pub source_id: String,
    pub operation: ProposalOperation,
    pub intent: ProposalIntent,
    pub target_id: String,
    pub target_path: String,
    pub base: ProposalBase,
    pub candidate_hash: String,
    pub candidate_bytes: usize,
    pub policy_hash: String,
    pub profile_hash: String,
    pub confidence: Option<Confidence>,
    pub evaluator: Option<EvaluatorBlock>,
}

pub struct ProposalSubmission {
    pub profile_path: PathBuf,
    pub store_root: PathBuf,
    pub source_id: String,
    pub operation: ProposalOperation,
    pub intent: ProposalIntent,
    pub target_id: String,
    pub target_path: String,
    pub candidate_path: PathBuf,
    pub confidence: Option<Confidence>,
    pub evaluator_path: Option<PathBuf>,
}
```

Implement custom `Serialize`/`Deserialize` for `Confidence` so it is a JSON number, never a string or binary float. Canonical metadata is `serde_json::to_vec(&meta)` plus exactly one LF; strict loading parses with `deny_unknown_fields`, reserializes, and requires byte equality.

- [ ] **Step 4: Implement domain-separated IDs and raw pair binding**

Use one helper local to `proposal.rs`:

```rust
fn frame(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}
```

`proposal_id` frames, in order: domain, submitted timestamp, source ID, operation, intent, target ID, target path, base kind, generation/null marker, bundle hash, manifest hash, target/null marker, candidate hash, candidate byte count as `u64` LE, policy hash, profile hash, confidence presence plus millionths `u32` LE, and evaluator commitment/null marker. Prefix the final lowercase hash with `prp-`.

`read_proposal_pair` must reject non-regular/symlink files, over-limit metadata before parse, noncanonical JSON, ID mismatch, candidate size/hash mismatch, unsafe shard/path, and any extra file. It returns exact candidate/meta bytes and their raw hashes without normalization.

- [ ] **Step 5: Implement fail-atomic immutable pair publication**

`publish_proposal_pair` must require a caller-provided verified intake lease token, create a hidden same-parent directory, create-new and sync `candidate.md`, create-new and sync `proposal.meta.json`, sync the directory, atomically rename to `staging/proposals/<first-two-hash-hex>/<proposal-id>/`, and sync both shard and proposals parents. Recheck the lease immediately before rename. A destination collision is `proposal.duplicate_id`; no existing pair is overwritten, moved, or removed.

Add exact `ProposalError::code()` values for pair structure, UTF-8, ID/hash/size, duplicate ID, read, write, and lock failures. Messages stay static and redacted.

- [ ] **Step 6: Run GREEN, raw-binding, and crash-boundary tests**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_proposal
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_store
```

Expected: all proposal format/hash/atomicity tests pass; no corruption leaves a partially visible pair.

- [ ] **Step 7: Commit immutable proposal pairs**

```bash
git add sovmem/src/profile/proposal.rs sovmem/src/profile/mod.rs sovmem/tests/support/p3.rs sovmem/tests/p3_proposal.rs
git commit -m "P3.3: bind immutable proposal pairs"
```

---
### Task 4: Enforce ordered policy, advisory non-authority, receipts, quotas, and dedupe

**Files:**
- Modify: `sovmem/src/profile/policy.rs`
- Modify: `sovmem/src/profile/proposal.rs`
- Modify: `sovmem/tests/p3_policy.rs`
- Modify: `sovmem/tests/p3_proposal.rs`
- Modify: `sovmem/tests/support/p3.rs`

**Interfaces:**
- Produces `PolicyContext`, `PolicyDecision { passed, rule_codes }`, and `PolicyEvaluator::evaluate`.
- Produces `IntakeRejection`, `SubmitProposalResult`, and public `submit_proposal(request) -> Result<SubmitProposalResult, ProposalError>`.
- Produces stable stage ordering, sorted/deduplicated codes within the first failing stage, and an explicit advisory-erasure invariant.
- Failed intake persists only a redacted receipt when timestamp/source are safe; candidate bytes are never staged on failure.

- [ ] **Step 1: Write the complete failing rule-order and poisoning table**

Extend `sovmem/tests/p3_policy.rs` with a table whose expected first-stage codes are exact:

```rust
#[test]
fn evaluator_stops_at_the_first_failing_stage_and_sorts_codes() {
    let fixture = support::p3::P3Fixture::new("policy-order");
    let cases = [
        ("pair", fixture.case_pair_and_path_invalid(), vec!["proposal.meta_schema"]),
        ("source", fixture.case_source_and_path_invalid(), vec!["proposal.source_invalid"]),
        ("quota", fixture.case_quota_and_path_invalid(), vec!["proposal.quota_source_open"]),
        ("base", fixture.case_stale_base_and_bad_document(), vec!["proposal.base_bundle_stale"]),
        ("document", fixture.case_missing_provenance(), vec!["proposal.provenance_required"]),
    ];
    for (name, context, expected) in cases {
        let result = fixture.evaluate(context);
        assert_eq!(result.rule_codes, expected, "{name}");
    }
}

#[test]
fn confidence_and_evaluator_erasure_cannot_change_authority() {
    let fixture = support::p3::P3Fixture::new("advisory-erasure");
    for context in [
        fixture.valid_context_with_confidence("1"),
        fixture.valid_context_with_recommendation("looks-sound"),
        fixture.invalid_context_with_recommendation("looks-sound"),
        fixture.valid_context_with_recommendation("reject"),
    ] {
        let full = fixture.evaluate(context.clone());
        let erased = fixture.evaluate(context.without_advisory());
        assert_eq!(full.passed, erased.passed);
        assert_eq!(full.rule_codes, erased.rule_codes);
    }
}
```

Add exact fixtures for malformed/oversized metadata, source identity, confidence, missing/invalid/self/unallowlisted/wrong-hash evaluator blocks, duplicate ID, duplicate `(target_id,candidate_hash)`, source-open/daily/total quotas, unsafe/forbidden paths, create/replace mismatch, stale generation/bundle/manifest/target hashes, size boundaries, invalid frontmatter, missing provenance, synthesis accept intent, and forged Judgment type/path. Include favorable advice plus a policy violation and confidence `1` plus a policy violation; both must fail for the underlying rule.

- [ ] **Step 2: Run policy/proposal tests and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_policy
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_proposal
```

Expected: new tests fail because ordered evaluation, quotas, advisory erasure, receipts, and the public submit coordinator do not exist.

- [ ] **Step 3: Implement the fixed seven-stage evaluator**

Add to `policy.rs`:

```rust
#[derive(Clone, Debug)]
pub struct PolicyContext<'a> {
    pub policy: &'a LoadedPolicy,
    pub profile: &'a P3Profile,
    pub store: &'a StoreSnapshot,
    pub proposal: &'a ProposalPair,
    pub retained: &'a [ProposalSummary],
    pub rejected_today: &'a [IntakeRejection],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyDecision {
    pub passed: bool,
    pub rule_codes: Vec<String>,
}

pub struct PolicyEvaluator;

impl PolicyEvaluator {
    pub fn evaluate(context: &PolicyContext<'_>) -> PolicyDecision {
        for stage in [
            evaluate_store_profile_policy,
            evaluate_pair_structure,
            evaluate_source_confidence_evaluator,
            evaluate_uniqueness_and_quotas,
            evaluate_path_and_base,
            evaluate_document,
            evaluate_advisory_invariant,
        ] {
            let mut codes = stage(context);
            codes.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
            codes.dedup();
            if !codes.is_empty() {
                return PolicyDecision { passed: false, rule_codes: codes };
            }
        }
        PolicyDecision { passed: true, rule_codes: Vec::new() }
    }
}
```

Each stage implements exactly the codes/order in design Section 17. Use the reviewed glob matcher, strict relative-path analysis, P3 profile mapping, current raw head commitments, retained immutable pairs, and validated timestamps. `evaluate_advisory_invariant` reruns stages 1-6 against a cloned proposal with `confidence = None` and `evaluator = None`; any difference returns only `policy.advisory_authority_violation`.

Parse source IDs as exactly `src:<kind>:<name>`: kind is one of `human|agent|harness|fixture`; name is 1 through 64 lowercase ASCII bytes matching `[a-z0-9][a-z0-9._-]{0,63}`. Production policy permits only the configured agent/harness prefixes; fixture is accepted only by a test policy. Never echo a malformed source value.

Daily quotas count both staged pairs and receipts with a validated source whose binary-assigned UTC date matches the new timestamp. Open quotas count only retained pairs with no terminal ledger event; before the ledger module exists, the fixture supplies an empty terminal set. Task 9 replaces that fixture projection with the verified ledger view.

- [ ] **Step 4: Implement redacted rejection receipts**

Add to `proposal.rs`:

```rust
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IntakeRejection {
    pub schema_version: String,
    pub receipt_id: String,
    pub received_at: String,
    pub source_id: Option<String>,
    pub rule_codes: Vec<String>,
}

fn receipt_id(received_at: &Timestamp, source_id: Option<&str>, codes: &[String]) -> String {
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, b"sovmem.intake-rejection.v1");
    frame(&mut hasher, received_at.as_str().as_bytes());
    frame(&mut hasher, source_id.unwrap_or("").as_bytes());
    for code in codes {
        frame(&mut hasher, code.as_bytes());
    }
    format!("rec-{}", hasher.finalize().to_hex())
}
```

Serialize receipts as strict pretty two-space JSON plus one LF, create-new and fsync, and never persist candidate/meta bytes, content/target/policy hashes, IDs/paths, reasons/advice, or parser excerpts. If the receipt write fails, preserve the rejection and return both the original rule code(s) and static `intake.receipt_write` without candidate persistence.

- [ ] **Step 5: Wire the public staging-only submit path**

`submit_proposal` performs this fixed sequence:

1. load P3 profile, current policy, and read-only store snapshot;
2. require no protected verdict/quarantine lease;
3. acquire the staging intake lease through Task 2 atomic helpers;
4. recheck no protected verdict/quarantine lease;
5. sample `SystemClock` once;
6. read candidate/evaluator from regular non-symlink files within byte limits;
7. derive the current base and prepare the immutable pair;
8. load retained pairs/receipts and run `PolicyEvaluator`;
9. on failure, publish only the redacted receipt when possible;
10. on success, publish the pair atomically;
11. verify and sync the exact intake lease parent while the lease is visible, then release it; its final directory deletion has no post-delete parent sync;
12. return a redacted summary.

Expose only this staging path publicly. No function in `proposal.rs` accepts a verdict, canonical destination, ledger writer, or index destination.

- [ ] **Step 6: Run GREEN plus poisoning and quota regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_policy
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_proposal
cargo test --locked --manifest-path sovmem/Cargo.toml --test validation
```

Expected: every named intake poison is blocked at its stable first stage, valid pairs publish once, receipts are redacted, and advisory erasure is identical.

- [ ] **Step 7: Commit deterministic intake policy**

```bash
git add sovmem/src/profile/policy.rs sovmem/src/profile/proposal.rs sovmem/tests/p3_policy.rs sovmem/tests/p3_proposal.rs sovmem/tests/support/p3.rs
git commit -m "P3.4: enforce deterministic proposal policy"
```

---

### Task 5: Freeze the five-verb grammar, reason/rewrite binding, and Human Judgment bytes

**Files:**
- Create: `sovmem/src/profile/judgment.rs`
- Create: `sovmem/tests/p3_canon.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `sovmem/src/profile/validate.rs`
- Modify: `sovmem/tests/support/p3.rs`

**Interfaces:**
- Produces `VerdictVerb`, `Reason`, `RewriteBytes`, `VerdictIdentityInput`, `DecisionMaterial`, `JudgmentDocument`, and `JudgmentError`.
- Produces `verdict_id`, `validate_decision`, and `build_judgment` as pure functions.
- Locks exact target/rewrite/Judgment hashes before a later human prompt; no filesystem mutation occurs in this task.

- [ ] **Step 1: Write failing five-verb/reason/rewrite/Judgment snapshots**

Create `sovmem/tests/p3_canon.rs` with the pure contract first:

```rust
#[allow(dead_code)]
mod support;

use sovmem::profile::{Reason, VerdictVerb};

#[test]
fn internal_verdict_grammar_and_reason_rules_are_exact() {
    for (source, verb) in [
        ("ACCEPT", VerdictVerb::Accept),
        ("REJECT", VerdictVerb::Reject),
        ("REWRITE", VerdictVerb::Rewrite),
        ("TP", VerdictVerb::Tp),
        ("FP", VerdictVerb::Fp),
    ] {
        assert_eq!(source.parse::<VerdictVerb>().unwrap(), verb);
        assert_eq!(verb.as_str(), source);
    }
    for invalid in ["accept", "", "TRUE_POSITIVE", "UNKNOWN"] {
        assert!(invalid.parse::<VerdictVerb>().is_err(), "{invalid}");
    }
    assert!(Reason::new(b"human reviewed evidence", 4096).is_ok());
    for invalid in [b"".as_slice(), b"   
	", b"contains nul"] {
        assert!(Reason::new(invalid, 4096).is_err());
    }
}
```

Add a deterministic snapshot test using fixed proposal/policy/profile/head/timestamp inputs. It must assert: verdict ID `ver-` plus 64 lowercase hex; Judgment path `judgments/<verdict-id>.md`; ID `ssa-0/judgments/<verdict-id>.md`; type judgment; context kind Human Judgment; layer concept; provenance synthesis; canonical/internal/active; generated-report; one references link; exact reason; empty inferred fields; raw bytes and hash identical across two constructions. REJECT/TP/FP return no Judgment.

- [ ] **Step 2: Run the focused test and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_canon internal_verdict_grammar_and_reason_rules_are_exact -- --exact
```

Expected: compile failure because `judgment.rs` does not exist.

- [ ] **Step 3: Implement exact verbs, reasons, rewrite bytes, and verdict IDs**

Create `sovmem/src/profile/judgment.rs`:

```rust
use super::{Timestamp, TypedLink};
use std::str::FromStr;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerdictVerb {
    Accept,
    Reject,
    Rewrite,
    Tp,
    Fp,
}

impl VerdictVerb {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "ACCEPT",
            Self::Reject => "REJECT",
            Self::Rewrite => "REWRITE",
            Self::Tp => "TP",
            Self::Fp => "FP",
        }
    }

    pub const fn applies(self) -> bool {
        matches!(self, Self::Accept | Self::Rewrite)
    }
}

impl FromStr for VerdictVerb {
    type Err = JudgmentError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        match source {
            "ACCEPT" => Ok(Self::Accept),
            "REJECT" => Ok(Self::Reject),
            "REWRITE" => Ok(Self::Rewrite),
            "TP" => Ok(Self::Tp),
            "FP" => Ok(Self::Fp),
            _ => Err(JudgmentError::VerbInvalid),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reason(Vec<u8>);

impl Reason {
    pub fn new(bytes: &[u8], maximum: usize) -> Result<Self, JudgmentError> {
        let text = std::str::from_utf8(bytes).map_err(|_| JudgmentError::ReasonInvalid)?;
        if bytes.len() > maximum
            || text.chars().all(char::is_whitespace)
            || text.chars().any(|value| value == ' ' || (value.is_control() && value != '
' && value != '	'))
        {
            return Err(JudgmentError::ReasonInvalid);
        }
        Ok(Self(bytes.to_vec()))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}
```

Add `RewriteBytes` with regular/non-symlink file loading, UTF-8, maximum byte, NUL/control, full-document, profile, path/ID/type/provenance, secret, and policy validation. `validate_decision` requires ACCEPT only for accept intent, REWRITE plus exact rewrite bytes for either intent, and forbids rewrite bytes for every other verb. A synthesis target can pass only through human REWRITE.

`verdict_id` frames domain, proposal ID, verb, exact reason bytes, rewrite hash/null, policy actor, ruled timestamp, prior ledger head/null, policy hash, and profile hash in the approved order.

- [ ] **Step 4: Construct exact deterministic Judgment bytes**

Build a `GeneratedDocument` and use the pinned renderer. Construct the body with exactly:

```rust
let prefix = format!(
    "# Human Judgment: {verb}

Subject: `{target_id}`
Verdict: `{verb}`

"
);
let mut body = prefix.into_bytes();
body.extend_from_slice(reason.bytes());
```

The builder adds no trailing byte after the exact UTF-8 reason. The frontmatter title is formed by `format!("Human Judgment: {verb} {target_id}")`, the description by `format!("Human {verb} verdict for {target_id}.")`, `created`/`updated` use `ruled_at.utc_date()`, source/resource use the verdict ID under `vault://ssa-0/judgments/`, and the sole link is `references` to the exact target ID under `vault://`. Show and hash the exact rendered bytes; never infer a field.

Add validation regression proving only the reviewed P3 profile accepts this generated Judgment and an agent-supplied equivalent remains forbidden by proposal policy.

- [ ] **Step 5: Run GREEN and validation regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_canon
cargo test --locked --manifest-path sovmem/Cargo.toml --test validation
```

Expected: exact grammar/reason/rewrite rules and deterministic Judgment snapshot pass; frozen Judgment validation remains fail-closed.

- [ ] **Step 6: Commit pure human decision material**

```bash
git add sovmem/src/profile/judgment.rs sovmem/src/profile/mod.rs sovmem/src/profile/validate.rs sovmem/tests/p3_canon.rs sovmem/tests/support/p3.rs
git commit -m "P3.5: freeze verdict and judgment bytes"
```

---

### Task 6: Build immutable canonical generations and P3 Level 1-3 verification

**Files:**
- Create: `sovmem/src/profile/canon.rs`
- Modify: `sovmem/src/profile/export.rs`
- Modify: `sovmem/src/profile/verify.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `sovmem/tests/p3_canon.rs`
- Modify: `sovmem/tests/support/p3.rs`

**Interfaces:**
- Produces `CanonManifest`, `CanonDocument`, `OriginKind`, `ParentKind`, `GenerationPlan`, `VerifiedGeneration`, `CanonError`, `stage_generation`, and `verify_generation`.
- `stage_generation` is crate-private and writes only to a caller-owned hidden directory; it cannot publish a generation or `CURRENT`.
- P3 levels are cumulative: structure/profile at 1, raw hashes/file set/root at 2, links/lineage/parent semantics at 3.

- [ ] **Step 1: Add failing generation construction and corruption fixtures**

Extend `p3_canon.rs` with:

```rust
#[test]
fn create_and_replace_reconstruct_exact_generation_evidence() {
    let fixture = support::p3::P3Fixture::new("canon-generation");
    let create = fixture.stage_accept_create();
    assert_eq!(create.manifest.schema_version, "sovmem-canon-manifest-v1");
    assert_eq!(create.manifest.store_id, "audy-sovmem-writeback-v1");
    assert_eq!(create.manifest.parent_kind, "seed");
    assert_eq!(create.documents, 64);
    assert!(create.target_bytes_equal_approved);
    assert!(create.judgment_bytes_equal_approved);

    let replace = fixture.stage_rewrite_replace(&create);
    assert_eq!(replace.manifest.parent_kind, "generation");
    assert_eq!(replace.documents, 65);
    assert!(replace.target_bytes_equal_approved);
    assert!(replace.seed_lineage_preserved);
}
```

Add corruption fixtures for manifest unknown/missing/reordered fields, duplicate canonical path/ID/resource/output, extra/missing Markdown, `index.md` mismatch, raw hash mismatch, bundle-root mismatch, parent/seed/policy/profile/proposal/verdict binding mismatch, invalid origin semantics, target/Judgment mismatch, symlink, malformed profile document, typed-link failure, and forbidden `corpus-report.json`. Each asserts the stable P3 code and no private bytes/path.

- [ ] **Step 2: Run the canonical fixture and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_canon create_and_replace_reconstruct_exact_generation_evidence -- --exact
```

Expected: failure because generation schemas/build/verification do not exist.

- [ ] **Step 3: Define the strict canonical manifest**

Create `canon.rs` with fixed-field serde structs:

```rust
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanonManifest {
    pub schema_version: String,
    pub store_id: String,
    pub profile_version: String,
    pub profile_hash: String,
    pub generation_id: String,
    pub parent_kind: String,
    pub parent_generation_id: Option<String>,
    pub parent_manifest_hash: String,
    pub seed_bundle_hash: String,
    pub seed_manifest_hash: String,
    pub policy_hash: String,
    pub applied_proposal_id: String,
    pub applied_verdict_id: String,
    pub documents: Vec<CanonDocument>,
    pub bundle_hash: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanonDocument {
    pub canonical_path: String,
    pub seed_source_path: Option<String>,
    pub output_path: String,
    pub id: String,
    pub resource: String,
    pub hash: String,
    pub origin_kind: String,
    pub seed_source_hash: Option<String>,
    pub proposal_id: Option<String>,
    pub verdict_id: Option<String>,
    pub previous_hash: Option<String>,
}
```

Serialize manifest bytes with `serde_json::to_vec_pretty` plus one LF and require strict round-trip byte equality on read. Sort documents by raw UTF-8 `canonical_path`. Seed-inherited documents retain original seed source path/hash. Verdict replacements preserve seed lineage when present, record current previous raw hash, and replace proposal/verdict IDs. Creates and Judgments have null seed lineage and previous hash.

- [ ] **Step 4: Build a hidden generation from exact approved bytes**

`GenerationPlan` contains the verified parent snapshot, next 16-digit applied generation ID, proposal/verdict/policy/profile commitments, exact target bytes, and exact Judgment bytes. `stage_generation`:

1. creates only the supplied hidden directory;
2. copies every parent document as raw bytes, never hard links;
3. installs target raw bytes and Judgment raw bytes with create-new writes;
4. parses every resulting document under the P3 profile;
5. reconstructs `bundle/index.md` from complete titles/paths;
6. reconstructs sorted document lineage entries;
7. hashes every raw document and the complete bundle with the existing P1 algorithm;
8. writes/syncs strict `manifest.json`;
9. syncs every file and directory;
10. calls explicit P3 Level 3 and returns `VerifiedGeneration`.

Make existing `export::render_index`, Markdown label/path encoding, and `verify::hash_bundle` crate-private reusable helpers so P1 and P3 use identical bytes. Do not change P1 outputs.

- [ ] **Step 5: Implement cumulative P3 Level 1-3 verification**

```rust
#[derive(Clone, Debug)]
pub struct VerifiedGeneration {
    pub root: std::path::PathBuf,
    pub manifest: CanonManifest,
    pub manifest_hash: String,
    pub bundle_hash: String,
    pub documents_hash: String,
    pub generated_documents: Vec<GeneratedDocument>,
}

pub fn verify_generation(
    profile: &P3Profile,
    root: &Path,
    level: VerificationLevel,
) -> Result<VerifiedGeneration, CanonError> {
    if !matches!(level, VerificationLevel::One | VerificationLevel::Two | VerificationLevel::Three) {
        return Err(CanonError::LevelInvalid);
    }
    verify_generation_cumulative(profile, root, level)
}
```

Level 1 checks exact root shape, strict manifest/profile/store bindings, regular files/no symlinks, canonical paths, P3 frontmatter/mappings, and deterministic Judgment fields. Level 2 adds raw hashes, exact file/document set, bundle root, seed/parent/policy/proposal/verdict commitments, and exact `index.md`. Level 3 adds typed-link rules, seed/verdict origin semantics, parent validity, applied target/Judgment bindings, and rejection of corpus evidence inside generations. It never asserts 62 documents.

- [ ] **Step 6: Run GREEN and prove frozen P1/P2 bytes did not move**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_canon
cargo test --locked --manifest-path sovmem/Cargo.toml --test export_determinism
cargo test --locked --manifest-path sovmem/Cargo.toml --test bundle_verification
cargo test --locked --manifest-path sovmem/Cargo.toml --test p2_determinism
```

Expected: P3 generation/corruption tests pass and all existing export/Level 1-3/P2 determinism tests remain green.

- [ ] **Step 7: Commit canonical generation verification**

```bash
git add sovmem/src/profile/canon.rs sovmem/src/profile/export.rs sovmem/src/profile/verify.rs sovmem/src/profile/mod.rs sovmem/tests/p3_canon.rs sovmem/tests/support/p3.rs
git commit -m "P3.6: verify immutable canonical generations"
```

---

### Task 7: Extend the disposable P2 index to verified P3 generations

**Files:**
- Modify: `sovmem/src/profile/index.rs`
- Modify: `sovmem/src/profile/query.rs`
- Create: `sovmem/tests/p3_index.rs`
- Modify: `sovmem/tests/support/p3.rs`
- Modify: `sovmem/tests/index_build.rs`
- Modify: `sovmem/tests/query.rs`
- Modify: `sovmem/tests/p2_determinism.rs`

**Interfaces:**
- Produces `IndexInputKind`, versioned internal index manifests, `P3IndexRequest`, `P3IndexEvidence`, `build_p3_index_generation`, `open_p3_index_generation`, and `query_p3_generation`.
- Produces deterministic `ReindexReceipt` bytes and hash bound to the P3 generation; transaction publication comes later.
- Keeps current frozen P2 CLI schema and v1 index manifests openable; new internal manifests explicitly distinguish `frozen-corpus` and `p3-canon`.

- [ ] **Step 1: Write failing P3 rebuild, receipt, and query fixtures**

Create `sovmem/tests/p3_index.rs`:

```rust
#[allow(dead_code)]
mod support;

#[test]
fn full_p3_rebuild_binds_generation_and_is_queryable() {
    let fixture = support::p3::P3Fixture::new("p3-index");
    let generation = fixture.stage_generation_with_phrase("p3 unique retrieval phrase");
    let built = fixture.build_p3_index(&generation);

    assert_eq!(built.input_kind, "p3-canon");
    assert_eq!(built.store_id.as_deref(), Some("audy-sovmem-writeback-v1"));
    assert_eq!(built.generation_id.as_deref(), Some(generation.generation_id.as_str()));
    assert_eq!(built.manifest_hash.as_deref(), Some(generation.manifest_hash.as_str()));
    assert_eq!(built.receipt.schema_version, "sovmem-reindex-receipt-v1");
    assert!(fixture.query_p3(&generation, "p3 unique retrieval phrase")
        .results
        .iter()
        .any(|hit| hit.id == generation.target_id));
}
```

Add tests for complete-from-zero build, strict reopen, wrong generation/manifest/bundle/documents/graph/config binding, corrupt or missing receipt, no corpus-count assertion, old frozen v1 manifest reopen, frozen v2 input identity, delete/rebuild semantic equality, and unchanged frozen 25-query results.

- [ ] **Step 2: Run P3 index and frozen P2 tests and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_index
cargo test --locked --manifest-path sovmem/Cargo.toml --test p2_determinism
```

Expected: P3 test fails because generation input/receipt APIs do not exist; frozen P2 remains green.

- [ ] **Step 3: Refactor verified index input without weakening P2**

Add an explicit input descriptor to the in-memory verified corpus:

```rust
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum IndexInputKind {
    FrozenCorpus,
    P3Canon,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexInputBinding {
    pub kind: IndexInputKind,
    pub store_id: Option<String>,
    pub generation_id: Option<String>,
    pub manifest_hash: String,
}
```

Keep frozen `load_verified_corpus` and its exact 62/8/54 Level 3 gate. Add a sibling adapter from `VerifiedGeneration` that constructs the same sorted `IndexDocument`/graph/documents hash but never checks frozen corpus counts. Both feed the existing schema/tokenizer/ranking builder.

Split public CLI schema `sovmem-derived-index-v1` from an internal versioned manifest reader. New builds write strict `sovmem-derived-index-v2` with `input_kind`, `store_id`, `generation_id`, and `input_manifest_hash`. The opener continues accepting exact v1 frozen manifests as implicit `frozen-corpus`; it never accepts v1 for P3. Keep existing P2 CLI JSON/text bytes unchanged.

- [ ] **Step 4: Implement direct P3 generation build/open**

```rust
pub(crate) struct P3IndexRequest<'a> {
    pub profile: &'a P3Profile,
    pub generation: &'a VerifiedGeneration,
    pub index_root: &'a Path,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct P3IndexEvidence {
    pub input_kind: &'static str,
    pub store_id: Option<String>,
    pub generation_id: Option<String>,
    pub manifest_hash: Option<String>,
    pub documents_hash: String,
    pub graph_hash: String,
    pub documents: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub external_typed_links: usize,
    pub config_hash: String,
}
```

`build_p3_index_generation` builds into the caller-supplied hidden derived directory, creates one Tantivy generation from zero, writes graph/internal manifest, reopens/self-checks every document/metadata/graph/config/input binding, syncs the complete tree, and returns evidence. It does not rename, select, write a receipt, or touch canon/ledger.

- [ ] **Step 5: Construct and verify deterministic re-index receipts**

Add strict pretty JSON plus one LF:

```rust
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReindexReceipt {
    pub schema_version: String,
    pub verdict_id: String,
    pub generation_id: String,
    pub profile_hash: String,
    pub manifest_hash: String,
    pub bundle_hash: String,
    pub documents_hash: String,
    pub documents: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub external_typed_links: usize,
    pub graph_hash: String,
    pub index_schema: String,
    pub tokenizer: String,
    pub ranking: String,
    pub tantivy_version: String,
    pub petgraph_version: String,
    pub index_config_hash: String,
    pub semantic_manifest_hash: String,
}
```

The receipt contains no timestamp, duration, path, slot, PID, hostname, username, body, reason, advice, or query. Strict verification reserializes, compares exact bytes, and rechecks every field against `VerifiedGeneration` plus the opened index evidence. The event later stores only the raw receipt hash.

- [ ] **Step 6: Add P3 query opening while preserving frozen query mode**

`query_p3_generation` accepts a verified generation, exact opened derived root, verified receipt, query, `top_k`, and expansion. It invokes the existing `search_opened_index`; it changes no tokenizer, boost, sort, graph expansion, result field, or query validation. Any generation/receipt/index disagreement fails before search. Existing `query_index(QueryRequest)` stays byte-compatible.

- [ ] **Step 7: Run GREEN and frozen Q3 regression**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_index
cargo test --locked --manifest-path sovmem/Cargo.toml --test index_build
cargo test --locked --manifest-path sovmem/Cargo.toml --test query
cargo test --locked --manifest-path sovmem/Cargo.toml --test p2_determinism
cargo test --locked --manifest-path sovmem/Cargo.toml --test eval
```

Expected: P3 full rebuild/query/receipt tests pass; frozen P2 deterministic builds and evaluation remain identical.

- [ ] **Step 8: Commit versioned P3 index input**

```bash
git add sovmem/src/profile/index.rs sovmem/src/profile/query.rs sovmem/tests/p3_index.rs sovmem/tests/support/p3.rs sovmem/tests/index_build.rs sovmem/tests/query.rs sovmem/tests/p2_determinism.rs
git commit -m "P3.7: rebuild index from verified canon"
```

---

### Task 8: Encode the append-only BLAKE3 verdict ledger

**Files:**
- Create: `sovmem/src/profile/ledger.rs`
- Create: `sovmem/tests/p3_ledger.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `sovmem/tests/support/p3.rs`

**Interfaces:**
- Produces strict `GenesisEvent`, `VerdictEvent`, `CanonicalResult`, `LedgerEvent`, `LedgerView`, `LedgerError`, `read_ledger`, `event_hash`, and `append_events`.
- Event zero is genesis; the first verdict is event one; event IDs are `evt-` plus a 16-digit zero-based sequence.
- `append_events` requires verified lease tokens supplied by the transaction layer and preserves the complete prior file as an exact prefix.

- [ ] **Step 1: Write failing genesis, line, hash, prefix, and tamper fixtures**

Create `sovmem/tests/p3_ledger.rs`:

```rust
#[allow(dead_code)]
mod support;

#[test]
fn genesis_and_first_verdict_are_one_atomic_prefix_append() {
    let fixture = support::p3::P3Fixture::new("ledger-genesis");
    let events = fixture.fixture_genesis_and_reject();
    let bytes = fixture.append_fixture_events(&events);
    let lines = bytes.split_inclusive(|byte| *byte == b'
').collect::<Vec<_>>();

    assert_eq!(lines.len(), 2);
    assert!(std::str::from_utf8(lines[0]).unwrap().contains("\"event_id\":\"evt-0000000000000000\""));
    assert!(std::str::from_utf8(lines[1]).unwrap().contains("\"event_id\":\"evt-0000000000000001\""));
    assert!(events[0].previous_hash().is_none());
    assert_eq!(events[1].previous_hash(), Some(events[0].hash()));
}

#[test]
fn every_successor_preserves_the_prior_ledger_as_an_exact_prefix() {
    let fixture = support::p3::P3Fixture::new("ledger-prefix");
    let first = fixture.append_fixture_events(&fixture.fixture_genesis_and_reject());
    let second = fixture.append_fixture_events(&fixture.fixture_tp());
    assert!(second.starts_with(&first));
    assert!(second.len() > first.len());
}
```

Add table-driven corruption fixtures for invalid UTF-8, missing/final extra LF, pretty JSON, unknown/reordered/missing fields, truncated line, duplicate/reordered/deleted/substituted event, sequence/event-ID mismatch, previous-hash mismatch, event-hash mismatch, duplicate proposal/verdict/transaction IDs, empty reason, confidence/evaluator mismatch, invalid verb/result semantics, and non-applying head change.

- [ ] **Step 2: Run focused ledger tests and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_ledger
```

Expected: compile failure because ledger events and append/parse functions do not exist.

- [ ] **Step 3: Define fixed-field genesis and verdict payloads**

Create `ledger.rs` with deny-unknown-field structs and no maps in hash-critical payloads. The genesis payload fields are exactly:

```rust
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GenesisPayload {
    pub schema_version: String,
    pub event_type: String,
    pub sequence: u64,
    pub event_id: String,
    pub created_at: String,
    pub previous_event_hash: Option<String>,
    pub store_id: String,
    pub profile_version: String,
    pub profile_hash: String,
    pub policy_hash: String,
    pub seed_manifest_hash: String,
    pub seed_corpus_report_hash: String,
    pub seed_bundle_hash: String,
    pub source_commit: String,
    pub initial_canonical_ref: String,
}
```

The verdict payload fixes sequence/event/timestamp/previous/store/transaction/proposal/meta/candidate/source/verb/reason/verdict/actor/policy/profile/confidence/evaluator/base/rewrite/canonical-result/resulting-head fields in the design order. Store exact reason text, rewrite raw hash and byte count only, and null applied fields for REJECT/TP/FP. Candidate/rewrite bodies and paths never enter either payload.

Represent applied result with a strict nested struct containing generation, manifest, bundle, prior/new target, Judgment, and re-index receipt hashes. Represent non-applied result with the same fixed fields serialized as null so line shape is deterministic across verbs.

- [ ] **Step 4: Implement canonical event hashing and strict line parsing**

For each event type, serialize the payload without `event_hash` to compact JSON. Hash exactly:

```rust
fn hash_payload(payload: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sovmem.ledger.event.v1");
    hasher.update(&(payload.len() as u64).to_le_bytes());
    hasher.update(payload);
    hasher.finalize().to_hex().to_string()
}
```

Serialize the line struct with `event_hash` as the final field, then add one LF. Strict parsing reads one line at a time, rejects empty lines, dispatches only after a minimal event-type probe, deserializes the exact struct, reserializes, requires byte equality, recomputes the hash, and checks contiguous sequence/IDs/previous links. `LedgerView` contains ordered events, terminal proposal map, verdict/verb counts, applied heads, final hash, and duplicate-ID sets derived only after full chain validation.

- [ ] **Step 5: Implement prefix-preserving atomic append**

`append_events`:

1. validates exact verdict and intake lease tokens;
2. reads and fully verifies the current ledger;
3. validates each new event against the evolving view;
4. constructs old bytes plus complete new canonical lines in memory;
5. writes a create-new same-directory temporary and syncs it;
6. rechecks the current ledger raw hash;
7. atomically renames the temporary over `verdicts.jsonl`;
8. syncs `ledger/`;
9. rereads and verifies the successor;
10. asserts old bytes are an exact prefix before returning.

The first verdict passes two events in one call: genesis then verdict. No public function can truncate, delete, rewrite, or reorder a committed line.

- [ ] **Step 6: Run GREEN and all tamper fixtures**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_ledger
```

Expected: exact genesis/prefix fixtures pass and every named corruption returns its stable ledger code without private output.

- [ ] **Step 7: Commit the hash-chained ledger**

```bash
git add sovmem/src/profile/ledger.rs sovmem/src/profile/mod.rs sovmem/tests/p3_ledger.rs sovmem/tests/support/p3.rs
git commit -m "P3.8: add append-only verdict chain"
```

---

### Task 9: Implement Level 4, proposal lifecycle, verdict views, and queue statistics

**Files:**
- Modify: `sovmem/src/profile/ledger.rs`
- Modify: `sovmem/src/profile/proposal.rs`
- Modify: `sovmem/src/profile/stats.rs`
- Modify: `sovmem/src/profile/verify.rs`
- Modify: `sovmem/tests/p3_ledger.rs`
- Modify: `sovmem/tests/p3_proposal.rs`
- Modify: `sovmem/tests/p3_store.rs`
- Modify: `sovmem/tests/support/p3.rs`

**Interfaces:**
- Produces public `verify_store_level4(profile, store) -> Result<Level4Summary, Level4Failure>` and crate-private `verify_store_level4_with_allowance`.
- Produces `ProposalState`, `ProposalRecord`, `ProposalFilter`, `proposal_list`, `proposal_status`, `VerdictRecord`, `verdict_list`, `StoreStats`, and `store_stats`.
- Public Level 4 rejects any lease/transient/incomplete state; transaction internals may allow the exact two held verdict/intake lease tokens after journal cleanup, and recovery may allow only its exact protected recovery-lease token after recovery cleanup.

- [ ] **Step 1: Write failing complete-chain and lifecycle fixtures**

Extend `p3_ledger.rs`:

```rust
#[test]
fn level_four_walks_every_artifact_to_the_ledger_head() {
    let fixture = support::p3::P3Fixture::new("level-four");
    fixture.install_fixture_chain_with_one_applied_generation();
    let summary = fixture.verify_level4().expect("verify fixture Level 4");
    assert_eq!(summary.level, 4);
    assert_eq!(summary.events, 3);
    assert_eq!(summary.verdicts, 2);
    assert_eq!(summary.applied_generations, 1);
    assert_eq!(summary.current_generation, "gen-0000000000000001");
    assert_eq!(summary.errors, 0);
}
```

Extend `p3_proposal.rs` with staged -> ruled -> applied -> superseded projections, retained orthogonality, rejected-at-intake receipts, exact ordering, filters, default redactions, and a duplicate terminal event failure.

Extend `p3_store.rs` with unsealed stats: verified seed, sealed false, events zero, ledger head null, generation seed, open queue and count-based alarm. Add sealed stats with event/verdict/applied counts and current generation.

- [ ] **Step 2: Run focused Level 4/lifecycle/stats tests and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_ledger level_four_walks_every_artifact_to_the_ledger_head -- --exact
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_proposal
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_store
```

Expected: tests fail because cross-artifact Level 4 and lifecycle/stat projections do not exist.

- [ ] **Step 3: Add `VerificationLevel::Four` without weakening lower levels**

Extend the enum and number mapping:

```rust
pub enum VerificationLevel {
    One,
    Two,
    Three,
    Four,
}

impl VerificationLevel {
    pub const fn number(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
        }
    }
}
```

`verify_bundle` must reject Four as an invalid bundle-level call. Store Level 4 dispatches only through `verify_store_level4`. Existing compatibility/bare/explicit Level 1-3 behavior and outputs remain byte-identical.

- [ ] **Step 4: Implement the exact Level 4 walk**

```rust
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct Level4Summary {
    pub status: &'static str,
    pub command: &'static str,
    pub level: u8,
    pub events: usize,
    pub verdicts: usize,
    pub accept: usize,
    pub reject: usize,
    pub rewrite: usize,
    pub tp: usize,
    pub fp: usize,
    pub applied_generations: usize,
    pub current_generation: String,
    pub bundle_hash: String,
    pub ledger_head: String,
    pub errors: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct VerificationAllowance {
    pub verdict_lease_token: Option<String>,
    pub intake_lease_token: Option<String>,
}
```

Implement all 22 algorithm steps in design Section 28.2 in order: store/profile/current-policy/history/seed; genesis and canonical lines; sequence/hash/ID uniqueness; retained proposal/meta/candidate binding; confidence/evaluator equality; historical head reconstruction; archived-policy evaluator rerun with advisory erasure; reason/verb semantics; generation Level 3; target/Judgment/parent/seed/policy/manifest/bundle/receipt bindings; derived authoritative head; exact `CURRENT`; and transient/journal/lease rejection. P3 index directories are optional to Level 4.

Unknown or unsupported schema/fields fail closed. Public verification always passes an empty allowance. Internal verdict verification accepts only its exact current verdict/intake lease tokens after its journal is gone; internal recovery verification accepts only its exact protected recovery-lease token after recovery cleanup. Any other lease or transient remains `transaction.incomplete`.

- [ ] **Step 5: Implement lifecycle and list projections from immutable evidence**

Add exact public values:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProposalState {
    RejectedAtIntake,
    Staged,
    Ruled,
    Applied,
    Superseded,
    Retained,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalFilter {
    pub state: Option<ProposalState>,
}
```

Load pairs/receipts in raw-byte path order, validate every pair, then join only to a fully verified ledger. Staged order is timestamp then proposal ID; ruled/applied/superseded order is ledger sequence; receipts order is received timestamp then receipt ID. `retained` includes every published pair regardless of decision. Default projections expose IDs, timestamps, valid source, operation/intent, target ID, candidate hash, confidence, evaluator summary, terminal verb/verdict ID, and current/superseded state; they omit bodies, paths, reasons, and evaluator reasons.

Verdict records are ledger order and expose default hash/count metadata only. Explicit booleans control reason/advice joins after the retained proposal and ledger commitments are reverified.

- [ ] **Step 6: Implement sealed and pre-genesis store stats**

`store_stats` uses public Level 4 for sealed stores. Before genesis it explicitly verifies seed Level 3, current policy/profile, zero-byte ledger, `CURRENT=seed`, and every staged pair/receipt, then reports `sealed=false`, zero events, null head, seed generation, and the queue block without claiming Level 4.

The queue alarm is `open >= queue_alarm_at`; oldest is the minimum persisted staged timestamp; no current-time calculation is permitted. Existing `summarize_verified_bundle` and bundle stats JSON remain unchanged.

- [ ] **Step 7: Run GREEN plus corruption and compatibility regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_ledger
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_proposal
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_store
cargo test --locked --manifest-path sovmem/Cargo.toml --test bundle_verification
cargo test --locked --manifest-path sovmem/Cargo.toml --test cli
```

Expected: Level 4/lifecycle/stats tests pass; every corruption code is stable; Levels 1-3 and legacy CLI remain green.

- [ ] **Step 8: Commit Level 4 and read models**

```bash
git add sovmem/src/profile/ledger.rs sovmem/src/profile/proposal.rs sovmem/src/profile/stats.rs sovmem/src/profile/verify.rs sovmem/tests/p3_ledger.rs sovmem/tests/p3_proposal.rs sovmem/tests/p3_store.rs sovmem/tests/support/p3.rs
git commit -m "P3.9: verify chain and queue state"
```

---

### Task 10: Add human presence, ordered leases, journals, and injected crash boundaries

**Files:**
- Create: `sovmem/src/profile/transaction.rs`
- Create: `sovmem/src/profile/p3_test_support.rs`
- Modify: `sovmem/src/profile/store.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `sovmem/tests/p3_store.rs`

**Interfaces:**
- Produces crate-private `HumanVerdictRequest`, `VerdictOutcome`, `HumanPresence`, `TtyHumanPresence`, `FilesystemOps`, `StdFilesystem`, `FaultInjector`, `NoFaults`, `FaultPoint`, `Lease`, `LeaseSet`, `TransactionJournal`, and `TransactionRuntime`.
- Production transaction entry points remain crate-private; only `cli/p3.rs` may call them later.
- Verdict/recovery lock order is protected `locks/verdict.lock/` then `staging/.intake-lock/`. Proposal never acquires the protected lease and checks it before/after intake acquisition.

- [ ] **Step 1: Write failing private unit fixtures for authority and lock order**

Add `#[cfg(test)] mod tests` to the new transaction module via `p3_test_support` and require:

```rust
#[test]
fn verdict_acquires_protected_then_intake_and_rechecks_tokens() {
    let fixture = P3TestFixture::new("lease-order");
    let runtime = fixture.runtime();
    let leases = runtime.acquire_verdict_leases("rec-fixed-owner").unwrap();
    assert_eq!(leases.acquisition_order(), ["verdict", "intake"]);
    assert!(leases.verify_all().is_ok());
    fixture.replace_verdict_owner();
    assert_eq!(leases.verify_all().unwrap_err().code(), "store.locked");
}

#[test]
fn production_human_path_requires_a_real_controlling_terminal() {
    let error = TtyHumanPresence::open_at(Path::new("/definitely/not/a/tty"))
        .expect_err("non-TTY path must fail");
    assert_eq!(error.code(), "human.tty_required");
}
```

Add races for two proposals, two verdicts, proposal versus verdict, reverse-order prohibition, missing owner, replaced token, live lease never stolen, and unsupported platform. Add journal canonical-byte tests and every `FaultPoint` name from design Section 27.

- [ ] **Step 2: Run the private unit tests and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::transaction::tests -- --nocapture
```

Expected: compile failure because transaction infrastructure does not exist.

- [ ] **Step 3: Implement real controlling-terminal human presence**

```rust
pub(crate) trait HumanPresence {
    fn show(&mut self, bytes: &[u8]) -> Result<(), TransactionError>;
    fn read_confirmation(&mut self) -> Result<String, TransactionError>;
}

pub(crate) struct TtyHumanPresence {
    tty: std::fs::File,
}

impl TtyHumanPresence {
    pub(crate) fn open() -> Result<Self, TransactionError> {
        Self::open_at(std::path::Path::new("/dev/tty"))
    }

    fn open_at(path: &std::path::Path) -> Result<Self, TransactionError> {
        use std::io::IsTerminal;
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| TransactionError::TtyRequired)?;
        if !tty.is_terminal() {
            return Err(TransactionError::TtyRequired);
        }
        Ok(Self { tty })
    }
}
```

The implementation writes exact decision material only to this file and reads one line from a cloned handle. EOF, non-UTF-8, overlong input, or mismatch is `human.confirmation_failed`. It never reads stdin, never returns displayed private bytes, and has no environment/config/test-selector branch.

- [ ] **Step 4: Implement atomic lease directories and exact owner binding**

```rust
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LeaseOwner {
    schema_version: String,
    owner_id: String,
    command_class: String,
    started_at: String,
    process_id: u32,
    lease_token: String,
}
```

`started_at` and `process_id` are transient local-inspection metadata only. Production captures them through a private `LeaseInspection` value at lease creation; tests inject fixed inspection metadata. They are included in the lease token but excluded from proposal/verdict/recovery IDs, journals, ledger events, and final deterministic snapshots. This does not consume the single injected durable `Clock` sample taken later for proposal, verdict, or recovery evidence.

Use one domain-separated framed helper for owner receipts and final transaction IDs. Each frame is `u64` little-endian byte length followed by bytes; optional values use an explicit null/present marker.

Before lease creation, write an owner receipt ID `rec-` plus 64 lowercase BLAKE3 hex. Its preimage is domain `sovmem.lease-receipt.v1`, command class, store ID, observed ledger/canonical head/null, profile hash, policy hash, and: proposal ID for verdict; validated source ID/null plus operation/intent/target ID for propose; or sorted stale-owner hashes plus original journal hash/null for recovery. It excludes timestamp, candidate/meta/rewrite/reason bytes and hashes, paths, PID, user, and host. This receipt exists only in `owner.json`; it is not an intake-rejection receipt or ledger event.

After locked revalidation, the single sampled clock value enters the final `txn-` ID:

- verdict transaction preimage: `sovmem.transaction.v1`, command `verdict`, `ruled_at`, proposal ID, verb, reason raw hash, rewrite raw hash/null, policy actor, observed ledger head/null, policy hash, and profile hash;
- recovery transaction preimage: the same domain, command `recover`, `recovered_at`, sorted quarantined owner hashes, observed ledger head/null, original journal hash/null, policy hash, and profile hash.

Proposal intake has no durable transaction ID: success produces its proposal ID and failure may produce the separate `sovmem.intake-rejection.v1` receipt from Task 4. Journals bind the final transaction ID to both exact lease-owner hashes. Locked revalidation must still match every preimage commitment.

Acquire by building a hidden same-parent directory, writing/syncing canonical `owner.json`, and syncing it and its parent. Atomically create the fixed lease directory with `create_dir`, hard-link the synced hidden owner file to the previously absent fixed `owner.json`, sync the fixed directory and parent, then remove/sync the hidden directory. Both destination-creation operations fail if a destination already exists, so neither an empty lock directory nor an owner file can be replaced. A crash before linking leaves explicit incomplete evidence and public reads fail closed. The token is BLAKE3 over domain plus all owner fields except token. Every publication boundary rereads exact owner bytes and token. A standalone intake or final protected-lease release verifies exact ownership and syncs its parent while the lease is visible, then removes only the exact owned directory with no post-delete parent sync. A verdict/recovery transaction instead releases its intake or quarantined lease first and syncs its parent while the protected lease remains, then verifies, pre-delete-syncs, and terminally removes that last protected lease. PID/age are display evidence only; no decision uses liveness or age.

Verdict acquires protected then intake. Proposal checks protected/quarantine absence before and after intake acquisition and releases its intact lease on the second-check failure. Lock acquisition never waits or auto-steals.

- [ ] **Step 5: Add filesystem and fault injection with no production selector**

`FilesystemOps` exposes only the exact read/create/write/sync/rename/remove/metadata operations used by P3. `StdFilesystem` delegates to `std`. `FaultInjector::check(FaultPoint)` returns success in production; `NoFaults` is the only production value. `P3TestFixture` injects `ScriptedHuman`, `FixedClock`, and a deterministic fault plan directly into crate-private constructors; no CLI flag, environment variable, public API, or serialized field can select them.

Define all 22 crash boundaries in exact transaction order, plus proposal pair/receipt and recovery cleanup boundaries. Unit tests assert each name is unique and is reached only after the prior boundary.

- [ ] **Step 6: Implement strict transaction journals**

```rust
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PreparedJournal {
    schema_version: String,
    transaction_id: String,
    command_class: String,
    owner_hashes: Vec<String>,
    proposal_id: Option<String>,
    verdict_id: Option<String>,
    observed_ledger_head: Option<String>,
    prepared_hashes: Vec<String>,
}
```

Use strict pretty JSON plus one LF for `prepared.json`, `ledger-committed.json`, and `published.json`. Create-new/sync every marker. Successful cleanup writes/syncs published, creates `transactions/.cleanup-<transaction-id>` with no-replace semantics, hard-links the complete verified marker set into it, syncs it and the parent, removes the original journal only after the cleanup form is durable, then removes the cleanup directory as a terminal liveness-only operation with no post-delete parent sync. An existing cleanup destination fails closed and is never replaced. A crash can retain stale cleanup or lease evidence; public reads reject it until human recovery resolves the liveness state. Any visible journal/temp/lease makes public reads fail `transaction.incomplete`.

- [ ] **Step 7: Run GREEN and concurrency/authority regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::transaction::tests -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_store
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_proposal
```

Expected: ordered leases, TTY refusal, journal bytes, faults, and races pass; proposal still cannot write any protected path.

- [ ] **Step 8: Commit transaction infrastructure**

```bash
git add sovmem/src/profile/transaction.rs sovmem/src/profile/p3_test_support.rs sovmem/src/profile/store.rs sovmem/src/profile/mod.rs sovmem/tests/p3_store.rs
git commit -m "P3.10: add human transaction infrastructure"
```

---

### Task 11: Implement locked human preflight, idempotency, and policy archival

**Files:**
- Modify: `sovmem/src/profile/transaction.rs`
- Modify: `sovmem/src/profile/p3_test_support.rs`
- Modify: `sovmem/src/profile/policy.rs`
- Modify: `sovmem/tests/p3_ledger.rs`

**Interfaces:**
- Produces crate-private `preflight_human_verdict`, `ConfirmedDecision`, `IdempotentVerdict`, and `archive_policy`.
- Performs the exact read-only replay check before terminal/lease work and repeats it under both leases.
- Samples one timestamp only after locked revalidation; displays and binds all final decision bytes before exact confirmation.

- [ ] **Step 1: Write failing preflight/TTY/idempotency tests**

Add transaction unit tests:

```rust
#[test]
fn wrong_confirmation_and_eof_leave_every_durable_hash_unchanged() {
    for response in [Some("APPLY ACCEPT wrong-id
"), None] {
        let fixture = P3TestFixture::with_staged_accept("human-abort", response);
        let before = fixture.durable_hashes();
        let error = fixture.preflight().expect_err("confirmation must fail");
        assert_eq!(error.code(), "human.confirmation_failed");
        assert_eq!(fixture.durable_hashes(), before);
    }
}

#[test]
fn exact_committed_replay_is_read_only_and_does_not_open_the_terminal() {
    let fixture = P3TestFixture::with_committed_reject("idempotent-replay");
    let before = fixture.durable_hashes();
    let outcome = fixture.replay_exact_reject().unwrap();
    assert!(outcome.idempotent);
    assert_eq!(fixture.human_open_count(), 0);
    assert_eq!(fixture.durable_hashes(), before);
    assert_eq!(fixture.replay_changed_reason().unwrap_err().code(), "verdict.already_ruled");
}
```

Add tests that stale base, changed policy/profile, protected synthesis ACCEPT, missing provenance, forged evaluator, already-ruled race, invalid reason/rewrite combination, and a candidate/meta hash change all fail before a ledger/canon write. Add a terminal-capture fixture asserting exact candidate or rewrite bytes, exact generated Judgment bytes when applying, reason, confidence, evaluator recommendation/reasons, policy result, and every relevant hash are displayed on the scripted human channel only.

- [ ] **Step 2: Run private transaction tests and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::transaction::tests::wrong_confirmation_and_eof_leave_every_durable_hash_unchanged -- --exact
cargo test --locked --manifest-path sovmem/Cargo.toml profile::transaction::tests::exact_committed_replay_is_read_only_and_does_not_open_the_terminal -- --exact
```

Expected: failures because preflight/replay/confirmation logic is absent.

- [ ] **Step 3: Implement the read-only replay branch before authority acquisition**

Before opening `/dev/tty` or acquiring a lease:

1. load profile/store/verified chain and proposal pair;
2. hash the supplied exact reason and optional rewrite file without emitting either;
3. if a terminal event exists, compare proposal ID, verb, reason hash, and rewrite hash;
4. return its existing redacted summary with `idempotent=true` and exit-success semantics on exact match;
5. return `verdict.already_ruled` on any mismatch.

No replay branch writes, opens the terminal, archives policy, or changes a lease. The same comparison runs after both leases are held to close the race.

- [ ] **Step 4: Implement the exact locked preflight sequence**

`preflight_human_verdict` performs design Section 24.1 in order:

1. validate static request shape without echoing input;
2. complete the read-only replay branch;
3. open real controlling terminal;
4. acquire protected verdict lease;
5. acquire intake lease;
6. reject incomplete state;
7. verify sealed Level 4 or unsealed seed/profile/policy/staging state;
8. load exact proposal pair;
9. rerun `PolicyEvaluator` against current head/current policy;
10. reject stale base or now-ruled proposal, with locked idempotency handling;
11. validate exact reason and optional rewrite;
12. sample the clock once;
13. derive verdict/transaction/generation identifiers and exact target/Judgment bytes/hashes;
14. write the full decision display to `HumanPresence` only;
15. require exact `APPLY <VERB> <FIRST_12_PROPOSAL_ID_CHARS>`;
16. archive policy bytes;
17. reread policy/profile and require exact byte equality;
18. return `ConfirmedDecision` while both leases remain held.

The display uses length headings followed by exact bytes, so multiline candidate/rewrite/reason/advice cannot forge labels. No private display byte enters stdout/stderr or `ConfirmedDecision` debug output.

- [ ] **Step 5: Archive exact policy history fail-closed**

`archive_policy` writes current raw bytes to `policies/<policy-hash>.toml` with create-new and fsync before ledger publication. An existing file must be a regular non-symlink file with byte-identical content. Historical snapshots are never modified or deleted. After archival, reread current policy and P3 profile and compare exact preflight bytes; return `policy.changed_during_transaction` or `profile.changed_during_transaction` before any ledger commit.

Level 4 permits only hashes referenced by genesis/verdict events plus the current policy hash awaiting its first verdict. Unknown extra policy snapshots fail store layout/history verification.

- [ ] **Step 6: Run GREEN and abort-path regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::transaction::tests -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_policy
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_ledger
```

Expected: wrong/EOF/stale/tampered paths write nothing, exact replay is read-only, policy snapshots are immutable, and all private decision bytes stay on the scripted terminal.

- [ ] **Step 7: Commit human preflight**

```bash
git add sovmem/src/profile/transaction.rs sovmem/src/profile/p3_test_support.rs sovmem/src/profile/policy.rs sovmem/tests/p3_ledger.rs
git commit -m "P3.11: require locked human preflight"
```

---

### Task 12: Commit REJECT, TP, and FP as ledger-only verdicts

**Files:**
- Modify: `sovmem/src/profile/transaction.rs`
- Modify: `sovmem/src/profile/ledger.rs`
- Modify: `sovmem/src/profile/p3_test_support.rs`
- Modify: `sovmem/tests/p3_ledger.rs`
- Modify: `sovmem/tests/p3_canon.rs`

**Interfaces:**
- Produces crate-private `apply_human_verdict(request, runtime) -> Result<VerdictOutcome, TransactionError>` for the three non-applying verbs first.
- Commits genesis plus first verdict in one ledger replacement when unsealed.
- Returns only after journal cleanup, internal Level 4 with exact lease allowance, and lease release.

- [ ] **Step 1: Write failing ledger-only transaction tests**

Add exact unit tests:

```rust
#[test]
fn reject_tp_and_fp_change_only_ledger_policy_history_and_journal_lifecycle() {
    for verb in [VerdictVerb::Reject, VerdictVerb::Tp, VerdictVerb::Fp] {
        let fixture = P3TestFixture::with_staged_proposal("ledger-only", verb);
        let canon_before = fixture.canon_hashes();
        let index_before = fixture.index_hashes();
        let outcome = fixture.apply(verb, "human reason").unwrap();

        assert!(!outcome.applied);
        assert!(outcome.generation.is_none());
        assert_eq!(fixture.canon_hashes(), canon_before);
        assert_eq!(fixture.index_hashes(), index_before);
        assert_eq!(fixture.verdict_count(verb), 1);
        assert!(fixture.verify_level4().is_ok());
        assert!(fixture.transactions_empty());
        assert!(fixture.leases_absent());
    }
}
```

Add first-verdict genesis atomicity, empty/whitespace reason, rewrite-file forbidden, policy actor source, unchanged canonical head/result fields, exact replay, changed replay, ledger write failure, marker failure, cleanup failure, and no index/canon/receipt open-for-write assertions.

Add real delegating `FilesystemOps` wrappers for recovery-tail parents. Final cleanup and final verdict lease paths must complete their required pre-delete parent sync, then succeed when the wrapper denies only a post-delete `transactions/` or `locks/` sync because no such sync is attempted. Denying `staging/` after intermediate `.intake-lock` removal must instead return `transaction.incomplete`, retain committed ledger evidence and `verdict.lock`, and keep public inspection and Level 4 rejected. A separate standalone proposal wrapper must observe the required pre-delete staging sync, deny only a post-delete attempt, and leave no intake lock on success.

- [ ] **Step 2: Run the ledger-only fixture and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::transaction::tests::reject_tp_and_fp_change_only_ledger_policy_history_and_journal_lifecycle -- --exact
```

Expected: failure because verdict commit orchestration is not implemented.

- [ ] **Step 3: Construct exact non-applying verdict events**

From `ConfirmedDecision`, create a transaction journal and `prepared.json`, derive the next event sequence/ID, bind exact proposal/meta/candidate/source/verb/reason/verdict/actor/policy/profile/confidence/evaluator/base values, set rewrite and all canonical-result fields null, and set resulting head equal to the verified prior head.

If unsealed, construct genesis from the exact seed/profile/policy/store evidence and prepend it to the same `append_events` call. The first verdict previous hash is the newly computed genesis hash.

- [ ] **Step 4: Publish with the ledger as the commit point**

Implement this exact order:

1. sync transaction `prepared.json`;
2. fault boundary before ledger temporary;
3. atomically append genesis when needed plus verdict;
4. fault boundary immediately after ledger rename;
5. write/sync `ledger-committed.json`;
6. write/sync `published.json`;
7. create the no-replace hard-linked `.cleanup-<transaction-id>` form, sync it and `transactions/`, remove the original only after it is durable, then remove the cleanup form as a terminal liveness-only deletion with no post-delete parent sync;
8. run internal Level 4 allowing only the two exact lease tokens;
9. release intake, then perform its fallible staging/ sync while verdict.lock remains; verify and sync locks/ while final verdict.lock remains, then delete that last lease with no post-delete locks/ sync;
10. emit redacted `VerdictOutcome`.

Before ledger rename, any failure leaves old ledger/head authoritative and recovery later discards only uncommitted journal/temp artifacts. After ledger rename, the verdict is committed; public reads fail `transaction.incomplete` until recovery/finalization. A committed event is never removed.

- [ ] **Step 5: Run GREEN across all three non-applying verbs**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::transaction::tests -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_ledger
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_canon
```

Expected: all three verbs are ledger-only, genesis is atomic, Level 4 passes after cleanup, and injected failures classify pre/post commit correctly.

- [ ] **Step 6: Commit ledger-only verdicts**

```bash
git add sovmem/src/profile/transaction.rs sovmem/src/profile/ledger.rs sovmem/src/profile/p3_test_support.rs sovmem/tests/p3_ledger.rs sovmem/tests/p3_canon.rs
git commit -m "P3.12: commit ledger-only verdicts"
```

---

### Task 13: Commit ACCEPT and REWRITE through verified canon and full re-index

**Files:**
- Modify: `sovmem/src/profile/transaction.rs`
- Modify: `sovmem/src/profile/canon.rs`
- Modify: `sovmem/src/profile/index.rs`
- Modify: `sovmem/src/profile/ledger.rs`
- Modify: `sovmem/src/profile/p3_test_support.rs`
- Modify: `sovmem/tests/p3_canon.rs`
- Modify: `sovmem/tests/p3_index.rs`
- Modify: `sovmem/tests/p3_ledger.rs`

**Interfaces:**
- Extends `apply_human_verdict` to ACCEPT and REWRITE.
- Publishes immutable generation, disposable index, deterministic receipt, ledger event, and recoverable `CURRENT` in the approved order.
- No generation becomes authoritative until the full index has built/reopened and the ledger event commits.

- [ ] **Step 1: Write failing exact-byte ACCEPT/REWRITE transaction tests**

Add private tests:

```rust
#[test]
fn accept_and_rewrite_commit_exact_human_approved_bytes_after_full_rebuild() {
    let accept = P3TestFixture::with_staged_accept("apply-accept");
    let accepted = accept.apply(VerdictVerb::Accept, "accept reason").unwrap();
    assert!(accepted.applied);
    assert_eq!(accept.current_target_bytes(), accept.candidate_bytes());
    assert_eq!(accept.current_target_hash(), accept.candidate_hash());
    assert!(accept.current_judgment_contains_exact_reason(b"accept reason"));
    assert!(accept.receipt_matches_opened_index());
    assert!(accept.verify_level4().is_ok());

    let rewrite = P3TestFixture::with_staged_rewrite("apply-rewrite");
    let rewritten = rewrite.apply_with_rewrite(b"---
profile: complete fixture
---
# rewritten
");
    assert!(rewritten.applied);
    assert_eq!(rewrite.current_target_bytes(), rewrite.approved_rewrite_bytes());
    assert_ne!(rewrite.current_target_bytes(), rewrite.candidate_bytes());
    assert!(rewrite.receipt_matches_opened_index());
    assert!(rewrite.verify_level4().is_ok());
}
```

Add create/replace counts, synthesis ACCEPT failure/REWRITE success, policy/profile last-moment changes, raw byte/newline preservation, full target/Judgment queryability, re-index failure, receipt failure, generation/index rename failure, prepared marker failure, ledger failure, post-ledger `CURRENT` failure, cleanup failure, and first applied verdict genesis fixtures.

- [ ] **Step 2: Run the applied transaction test and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::transaction::tests::accept_and_rewrite_commit_exact_human_approved_bytes_after_full_rebuild -- --exact
```

Expected: failure because applying verbs are not implemented.

- [ ] **Step 3: Implement the exact pre-commit build sequence**

After common preflight:

1. allocate next applied `gen-` plus 16 digits and content-bound `txn-` ID;
2. create-new transaction journal;
3. stage a complete canonical generation from exact parent bytes;
4. install exact ACCEPT candidate or REWRITE human bytes;
5. install exact deterministic Judgment;
6. reconstruct `index.md` and manifest from raw bytes;
7. verify staged generation at P3 Level 3;
8. build a complete P2 index from zero into a hidden same-generation derived root;
9. reopen/self-check index against that generation;
10. create/sync deterministic receipt `evidence/reindex/<verdict-id>.json`;
11. sync generation, receipt, derived tree, and parents;
12. atomically rename staged generation to `canon/generations/<generation-id>` and sync;
13. atomically rename staged index to `derived/indexes/<generation-id>` and sync;
14. write/sync `prepared.json` with every event hash input;
15. reread policy/profile and require exact preflight bytes.

Any failure here leaves the old ledger/head authoritative. New generation/index/receipt are uncommitted and ignored until recovery removes them. No query or list surface treats them as current.

- [ ] **Step 4: Commit event, then publish `CURRENT` as a recoverable cache**

Build the applied canonical result with generation/manifest/bundle/prior/new target/Judgment/receipt hashes. Append genesis when needed plus verdict; this rename is the commit point. Then:

1. write/sync `ledger-committed.json`;
2. atomically replace `canon/CURRENT` with `<generation-id>
` and sync `canon/`;
3. write/sync `published.json`;
4. cleanup the journal through the no-replace hard-linked `.cleanup-` form, syncing while evidence remains; the final cleanup-directory deletion is liveness-only with no post-delete parent sync;
5. run internal Level 4 with only exact lease allowance;
6. optionally run the fixture query proof against the selected full index;
7. release intake, then perform its fallible staging/ sync while verdict.lock remains; verify and sync locks/ while final verdict.lock remains, then delete that last lease with no post-delete locks/ sync;
8. emit success.

If any post-ledger step fails, public reads return `transaction.incomplete`; the event remains committed and recovery rolls forward. Never roll back or delete a committed event/generation.

- [ ] **Step 5: Run GREEN through every applied crash boundary**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::transaction::tests -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_canon
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_index
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_ledger
```

Expected: ACCEPT/REWRITE exact bytes, Judgment, manifest, full rebuild, receipt, ledger, and current pointer all bind; every pre/post commit fault has the expected authority state.

- [ ] **Step 6: Commit applied verdict transactions**

```bash
git add sovmem/src/profile/transaction.rs sovmem/src/profile/canon.rs sovmem/src/profile/index.rs sovmem/src/profile/ledger.rs sovmem/src/profile/p3_test_support.rs sovmem/tests/p3_canon.rs sovmem/tests/p3_index.rs sovmem/tests/p3_ledger.rs
git commit -m "P3.13: apply verified canonical verdicts"
```

---

### Task 14: Recover crashes and prove concurrent operations fail closed

**Files:**
- Modify: `sovmem/src/profile/transaction.rs`
- Modify: `sovmem/src/profile/ledger.rs`
- Modify: `sovmem/src/profile/p3_test_support.rs`
- Modify: `sovmem/tests/p3_store.rs`
- Modify: `sovmem/tests/p3_ledger.rs`
- Modify: `sovmem/tests/p3_proposal.rs`

**Interfaces:**
- Produces crate-private `recover_store(request, runtime) -> Result<RecoveryOutcome, TransactionError>`.
- Supports exactly verdict-transaction, intake-only, and verified clean-finalization recovery.
- Recovery is itself journaled, human-only, idempotent, and never edits committed ledger/canonical bytes.

- [ ] **Step 1: Write failing recovery matrix and concurrency races**

Add one test per design boundary plus a table assertion:

```rust
#[test]
fn every_crash_state_has_one_deterministic_recovery_result() {
    for point in FaultPoint::ALL {
        let fixture = P3TestFixture::crash_at("recovery-matrix", point);
        let committed_before = fixture.ledger_contains_pending_verdict();
        fixture.terminate_simulated_owner();
        let outcome = fixture.recover_exact_owner().unwrap();
        assert_eq!(outcome.rolled_forward, committed_before, "{point:?}");
        assert!(fixture.verify_level4_or_unsealed().is_ok(), "{point:?}");
        assert!(fixture.transactions_empty(), "{point:?}");
        assert!(fixture.leases_absent(), "{point:?}");
    }
}
```

Add intake-only candidate/meta/receipt temp cleanup, completely published pair/receipt preservation, post-journal-cleanup stale locks as clean-finalization, crash-during-recovery resume, mismatched owner/hash, missing owner, multiple original/recovery journals, unexpected temp, live lease, wrong recovery phrase, recovery/no-TTY, two recovery processes, two proposals, two verdicts, and proposal/verdict barrier races.

- [ ] **Step 2: Run recovery tests and verify RED**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::transaction::tests::every_crash_state_has_one_deterministic_recovery_result -- --exact --nocapture
```

Expected: failure because recovery modes and resumption are absent.

- [ ] **Step 3: Implement explicit human stale-lease takeover**

Recovery:

1. opens the real controlling terminal;
2. instructs the operator to terminate/verify absence of the prior process;
3. displays only owner/transaction identifiers and hashes;
4. requires exact `RECOVER <owner-id>`;
5. atomically quarantines a stale verdict lease if present;
6. atomically creates protected `verdict.lock` as recovery lease, failing safely if another process wins;
7. atomically quarantines stale intake when present;
8. verifies the ledger through its last complete committed event;
9. selects exactly one recovery mode;
10. samples the recovery clock exactly once and creates/syncs a recovery journal bound to that timestamp, quarantine owner hashes, mode, ledger head, and original journal hash/null; every later recovery marker reuses the same sample.

No command automatically breaks a lease. PID/age never authorize takeover. If another process wins the protected lease after quarantine, leave quarantine as explicit incomplete evidence and return `store.locked` without touching ledger/canon.

- [ ] **Step 4: Implement the three exact recovery modes**

- Verdict transaction: require one matching original journal or recognized `.cleanup` form. If its event is committed, verify generation/receipt and roll `CURRENT` forward. If uncommitted, remove only that transaction's generation, derived index, exact transaction-bound re-index receipt, ledger temp, pointer temp, and journal.
- Intake-only: require no verdict journal. Preserve every completely published proposal pair/receipt; remove only unpublished candidate/meta/receipt/directory temps bound to the quarantined intake owner.
- Clean-finalization: allow only a verdict/recovery owner left after journals disappeared and only when ledger, `CURRENT`, generations, receipts, policies, and protected paths already verify exactly. Change no content.

Recovery never edits `verdicts.jsonl`, seed, committed generation, policy, a published proposal pair or intake-rejection receipt, a committed re-index receipt, or any frozen P2 artifact.

- [ ] **Step 5: Journal and finalize recovery idempotently**

Write/sync recovery `published.json`; cleanup the original journal if applicable through its no-replace hard-linked cleanup form; remove quarantined intake/verdict paths with their parent syncs while the protected recovery lease remains visible; then run the appropriate internal verifier while allowing only the exact protected recovery-lease token bound to the recovery journal. Only after that verification succeeds, verify and sync the protected recovery-lease parent while that lease remains visible, then remove it last. Sync each parent while recovery evidence remains, but treat only that final cleanup or final protected-lease deletion as liveness-only with no post-delete parent sync. A crash can retain stale cleanup or lease evidence, which is fail-closed human-recovery liveness state. The same ceremony resumes it; ambiguous owner/journal/temp state is `transaction.recovery_ambiguous` and never guessed.

A subsequent clean read or idempotent recovery invocation runs public Level 4 if sealed or the explicit unsealed seed/policy/profile/staging verifier. A second recovery of a clean store returns a redacted idempotent summary and writes nothing.

- [ ] **Step 6: Run GREEN across crash, race, and idempotency tests**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::transaction::tests -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_store
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_ledger
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_proposal
```

Expected: every crash state recovers deterministically, no live lease is stolen, races serialize/fail closed, and committed authority never rolls back.

- [ ] **Step 7: Commit recovery/concurrency**

```bash
git add sovmem/src/profile/transaction.rs sovmem/src/profile/ledger.rs sovmem/src/profile/p3_test_support.rs sovmem/tests/p3_store.rs sovmem/tests/p3_ledger.rs sovmem/tests/p3_proposal.rs
git commit -m "P3.14: recover write-back transactions"
```

---

### Task 15: Expose redacted proposal, queue, status, and stats CLI surfaces

**Files:**
- Create: `sovmem/src/cli/p3.rs`
- Modify: `sovmem/src/cli.rs`
- Modify: `sovmem/src/profile/proposal.rs`
- Modify: `sovmem/src/profile/stats.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Create: `sovmem/tests/p3_cli.rs`
- Modify: `sovmem/tests/cli.rs`
- Modify: `sovmem/tests/support/p3.rs`

**Interfaces:**
- Adds `propose`, nested `proposals list`, nested `proposals status`, and `stats --store` exactly as approved in design section 30.
- Adds crate-private P3 render structs with fixed declaration order and one compact JSON object plus LF, or stable line-oriented text.
- Preserves the current `stats --bundle` JSON byte shape and every P0-P2 command shape.

- [ ] **Step 1: Write failing CLI shape and compatibility tests**

In `p3_cli.rs`, use a provisioned unsealed fixture and assert:

```rust
#[test]
fn propose_derives_authoritative_fields_and_never_echoes_input_paths() {
    let fixture = P3Fixture::new("cli-propose");
    let output = fixture.command()
        .args([
            "propose", "--profile", fixture.profile_arg(),
            "--store", fixture.store_arg(),
            "--source", "src:agent:fixture",
            "--operation", "create", "--intent", "accept",
            "--target-id", "ssa-0/04-Research/CLI-Proof.md",
            "--target-path", "04-Research/CLI-Proof.md",
            "--candidate", fixture.candidate_arg(),
            "--confidence", "0.750000", "--format", "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let value = fixture.parse_single_json_stdout(&output);
    assert_eq!(value["command"], "propose");
    assert_eq!(value["state"], "staged");
    assert_eq!(value["base_kind"], "seed");
    assert_eq!(value["base_generation"], serde_json::Value::Null);
    fixture.assert_machine_streams_redacted(&output);
}
```

Also assert:

- the caller has no flags for proposal ID, timestamp, candidate hash, policy hash, profile hash, or any base hash;
- malformed confidence, unsafe target path, duplicate proposal, and quota failure emit no stdout, one sorted/deduplicated diagnostic JSONL stream, and the approved exit class;
- `proposals list` orders staged pairs by `(submitted_at, proposal_id)` and intake receipts by `(received_at, receipt_id)`;
- `proposals status` defaults to commitments only; `--show-advice` adds evaluator reasons and still omits candidate bytes and every filesystem path;
- `stats --store` before genesis reports `sealed:false`, `events:0`, `head:null`, `current_generation:"seed"`, and deterministic queue alarm evidence;
- existing `stats --bundle` output is byte-identical to the checked P2 fixture.

Run RED:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli propose_derives_authoritative_fields_and_never_echoes_input_paths -- --exact
```

Expected: FAIL because the P3 commands do not exist.

- [ ] **Step 2: Add exact Clap shapes without adding authority**

Add these command models to `cli.rs`:

```rust
Propose {
    #[arg(long)] profile: PathBuf,
    #[arg(long)] store: PathBuf,
    #[arg(long)] source: String,
    #[arg(long, value_enum)] operation: ProposalOperationArg,
    #[arg(long, value_enum)] intent: ProposalIntentArg,
    #[arg(long)] target_id: String,
    #[arg(long)] target_path: String,
    #[arg(long)] candidate: PathBuf,
    #[arg(long)] confidence: Option<String>,
    #[arg(long)] evaluator: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "json")] format: OutputFormat,
},
Proposals {
    #[command(subcommand)] command: ProposalCommand,
},
```

`ProposalCommand::List` has `--profile`, `--store`, optional `--state`, and `--format`. `ProposalCommand::Status` has positional proposal ID plus `--profile`, `--store`, `--show-advice`, and `--format`. Do not add any flag that supplies a derived commitment or bypasses policy. Register P3 commands in the structured parse-error path so invalid usage emits `cli.usage` and exit 2, while help/version retain Clap behavior.

- [ ] **Step 3: Implement redacted handlers and fixed renderers**

In `cli/p3.rs`, map CLI arguments to `submit_proposal`, `list_proposals`, and `proposal_status`. Use dedicated serialize-only output structs whose field order matches design sections 30.1 through 30.3. JSON is compact plus LF; text is stable key/value lines. Never serialize candidate/evaluator file paths, target paths, candidate bytes, rewrite bytes, default reasons, evaluator reasons, or OS details.

Map all failures through one `P3CommandError { code, message, exit }` adapter. Every message is the static approved message owned by its error variant. Sort diagnostics by `(severity, code, message)`, deduplicate exact triples, write them only to stderr, and return no partial success.

- [ ] **Step 4: Add store-mode statistics without changing bundle mode**

Replace only the Clap storage shape with mutually exclusive `Option<PathBuf>` values for `--bundle` and `--store`, then validate exactly one. Dispatch bundle mode to the unchanged `run_stats`; dispatch store mode to `summarize_store` and its new P3 renderer.

Before genesis, `summarize_store` verifies seed Level 3 plus profile, policy, and staged pairs. After genesis, it requires Level 4. It appends exactly the approved `queue`, `ledger`, `store_id`, `profile_version`, and `current_generation` fields. `oldest_submitted_at` is null when the queue is empty. `alarm` is `open >= alarm_at`; it never reads the wall clock.

- [ ] **Step 5: Run GREEN and P0-P2 CLI regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli
cargo test --locked --manifest-path sovmem/Cargo.toml --test cli
cargo test --locked --manifest-path sovmem/Cargo.toml --test stats
```

Expected: P3 proposal/list/status/store-stats contracts pass and every existing bundle command remains compatible.

- [ ] **Step 6: Commit the read/staging CLI boundary**

```bash
git add sovmem/src/cli.rs sovmem/src/cli/p3.rs sovmem/src/profile/proposal.rs sovmem/src/profile/stats.rs sovmem/src/profile/mod.rs sovmem/tests/p3_cli.rs sovmem/tests/cli.rs sovmem/tests/support/p3.rs
git commit -m "P3.15: expose proposal queue CLI"
```

---

### Task 16: Expose human verdict, recovery, Level 4, verdict list, and evolved index/query CLI surfaces

**Files:**
- Modify: `sovmem/src/cli.rs`
- Modify: `sovmem/src/cli/p3.rs`
- Modify: `sovmem/src/cli/p2.rs`
- Modify: `sovmem/src/profile/index.rs`
- Modify: `sovmem/src/profile/query.rs`
- Modify: `sovmem/src/profile/verify.rs`
- Modify: `sovmem/tests/p3_cli.rs`
- Create: `sovmem/tests/p3_privacy.rs`
- Modify: `sovmem/tests/cli.rs`
- Modify: `sovmem/tests/query.rs`
- Modify: `sovmem/tests/index_build.rs`

**Interfaces:**
- Adds `verdict`, nested `verdicts list`, `recover`, and `verify --store --level 4` exactly as approved.
- Adds store mode to existing `index` and `query` commands for the required evolved-lane rebuild/re-query proof; store mode derives `derived/indexes/<current-generation>` and never accepts a caller-selected index root.
- Keeps bundle-mode index/query/verify outputs and semantics byte-compatible.

- [ ] **Step 1: Write failing human-boundary, list, Level 4, and mode tests**

Add `p3_cli.rs` cases for:

```rust
#[test]
fn verdict_without_a_real_controlling_terminal_fails_before_mutation() {
    let fixture = P3Fixture::with_staged_accept("cli-no-tty");
    let before = fixture.durable_hashes();
    let output = fixture.command()
        .args([
            "verdict", fixture.proposal_id(), "ACCEPT",
            "--profile", fixture.profile_arg(),
            "--store", fixture.store_arg(),
            "--reason", "Reviewed fixture reason.",
            "--format", "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    fixture.assert_single_diagnostic(&output, "human.tty_required");
    assert_eq!(fixture.durable_hashes(), before);
}
```

Cover every forbidden combination:

- missing/blank reason; both/neither reason sources; REWRITE without exactly one rewrite file; rewrite file on another verb;
- lowercase/unknown verb; intent conflicts; missing proposal; already-ruled changed material;
- `verify --level 4` without `--store`; `--bundle` with Level 4; `--store` with `--require-corpus`; both/neither bundle/store;
- bundle index/query without `--index`; store index/query with `--index`; store eval; unsupported Level 1-3 store verification;
- deleted current index rebuild, changed-head-before-index-rename, and concurrent same-generation rebuild outcomes;
- recovery without a real TTY and recovery on an ambiguous journal;
- `verdicts list` exact sequence order, filters, null fields for ledger-only verbs, default reason/advice omission, and explicit `--show-reasons`/`--show-advice` exposure only on stdout.

Run RED:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli verdict_without_a_real_controlling_terminal_fails_before_mutation -- --exact
```

Expected: FAIL because the human verdict command does not exist.

- [ ] **Step 2: Add exact human and read-only Clap shapes**

Add:

```text
sovmem verdict <proposal-id> <ACCEPT|REJECT|REWRITE|TP|FP> --profile <p3> --store <store> (--reason <text> | --reason-file <file>) [--rewrite-file <file>] [--format json|text]
sovmem verdicts list --profile <p3> --store <store> [--verb <verb>] [--source <source-id>] [--show-reasons] [--show-advice] [--format json|text]
sovmem recover --profile <p3> --store <store> [--format json|text]
sovmem verify --profile <p3> --store <store> --level 4
```

Keep `--reason` and `--reason-file` in a required Clap group. `--rewrite-file` is validated against the verb after parse. Do not add `--yes`, `--force`, `--non-interactive`, an approval environment variable, a stdin approval route, an actor override, a test clock flag, or a fault flag.

Change shared command storage arguments as follows:

- verify/stats: exactly one of `--bundle` or `--store`;
- index/query: exactly one of `--bundle` or `--store`; `--index` is required only with bundle and forbidden with store;
- eval remains frozen bundle-only.

- [ ] **Step 3: Wire the sole production mutation path**

`run_verdict` first performs the read-only idempotency lookup. A new decision then calls the one crate-private transaction entry point with `TtyHumanPresence`; no public library re-export exposes that function. The ceremony writes its decision display and reads the exact confirmation through `/dev/tty`, never stdout, stderr, stdin, or an inherited pipe.

Render the exact design-section-30.4 object after the transaction returns and Level 4 has passed. Ledger-only fields are explicit nulls and `applied:false`; applied fields are exact committed commitments. Exact replay returns the same summary with `idempotent:true`, exit 0, no TTY open, and no write.

`run_recover` calls only the human recovery transaction. It cannot accept decision material, cannot synthesize an event, and emits success only after a clean Level 4 or verified pre-genesis state.

- [ ] **Step 4: Implement verdict list and store Level 4 renderers**

`run_verdicts_list` walks the already verified chain in sequence order, joins only retained proposal commitments, and emits the approved default fields. Reasons and evaluator reasons are loaded only for their explicit show flag. Candidate and rewrite bodies are impossible fields in the output structs.

`run_verify_store` accepts only Level 4, calls `verify_store_level4`, and emits one compact JSON object plus LF. Its fixed field order is `status`, `command`, `level`, `events`, `verdicts`, `accept`, `reject`, `rewrite`, `tp`, `fp`, `applied_generations`, `current_generation`, `bundle_hash`, `ledger_head`, `errors`. For the 20-verdict fixture the counts are exactly 21 events, 20 verdicts, four of each verb, and eight applied generations; `current_generation` is `gen-0000000000000008`; both hash fields are 64 lowercase hexadecimal characters; errors is zero. Do not add store/profile/manifest/warning fields to this approved output.

Before genesis, Level 4 fails `ledger.genesis_missing`; callers use `stats --store` for the verified unsealed view.

- [ ] **Step 5: Add evolved-lane index/query handlers**

Store index mode verifies and snapshots the ledger head, rebuilds from zero in a hidden sibling of `<store>/derived/indexes/<current-generation>`, reopens/self-checks it, recomputes the expected receipt in memory, and requires byte equality with the immutable receipt already committed by the current verdict. Immediately before the same-parent rename it re-verifies that the ledger-selected generation is unchanged; a changed head removes only its hidden tree and fails `index.input_stale`. It then atomically replaces only that disposable generation index, syncs/reopens it, and does not rewrite the receipt, append a verdict, or mutate canon. Concurrent same-generation rebuilds may race but must either publish a receipt-equivalent index or return a static infrastructure failure; neither may alter authority. Its output extends the existing index summary with `store_id`, `generation`, `manifest_hash`, and the existing `receipt_hash`.

Store query mode verifies Level 4 and the exact current re-index receipt before search, derives `<store>/derived/indexes/<current-generation>`, and extends the current query summary with `store_id`, `generation`, and `manifest_hash`. It keeps the P2 metadata-only hit shape and never prints the query. Bundle handlers and outputs remain unchanged.

- [ ] **Step 6: Centralize exit classes and privacy assertions**

The P3 adapter maps approved failures to:

- exit 0 for complete success or exact replay;
- exit 1 for readable data/policy/chain/stale/quota/lock/authority/deterministic verification failure;
- exit 2 for usage/configuration/unsupported filesystem/unreadable input/unsafe path/missing TTY/write infrastructure failure before a valid data decision.

Create `p3_privacy.rs` with distinct sentinel values in candidate, rewrite, reason, advice, query, source excerpt, absolute roots, temporary names, OS error text, PID, username, and hostname. Exercise every error family and assert none appears on either machine stream. The only tests that opt into `--show-reasons` or `--show-advice` assert the requested field appears on success stdout and never on stderr.

- [ ] **Step 7: Run GREEN and all frozen CLI/index/query regressions**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_privacy
cargo test --locked --manifest-path sovmem/Cargo.toml --test cli
cargo test --locked --manifest-path sovmem/Cargo.toml --test index_build
cargo test --locked --manifest-path sovmem/Cargo.toml --test query
cargo test --locked --manifest-path sovmem/Cargo.toml --test eval
```

Expected: new P3 surfaces obey the authority, output, mode, and privacy contracts; existing P2 commands remain green.

- [ ] **Step 8: Commit the complete P3 CLI**

```bash
git add sovmem/src/cli.rs sovmem/src/cli/p3.rs sovmem/src/cli/p2.rs sovmem/src/profile/index.rs sovmem/src/profile/query.rs sovmem/src/profile/verify.rs sovmem/tests/p3_cli.rs sovmem/tests/p3_privacy.rs sovmem/tests/cli.rs sovmem/tests/query.rs sovmem/tests/index_build.rs
git commit -m "P3.16: expose human write-back CLI"
```

---

### Task 17: Close poisoning, privilege, mixed-chain, full-loop, determinism, and frozen-P2 fixtures

**Files:**
- Modify: `sovmem/tests/p3_policy.rs`
- Modify: `sovmem/tests/p3_store.rs`
- Modify: `sovmem/tests/p3_proposal.rs`
- Modify: `sovmem/tests/p3_canon.rs`
- Modify: `sovmem/tests/p3_index.rs`
- Modify: `sovmem/tests/p3_ledger.rs`
- Modify: `sovmem/tests/p3_cli.rs`
- Modify: `sovmem/tests/p3_privacy.rs`
- Create: `sovmem/tests/p3_determinism.rs`
- Modify: `sovmem/tests/support/p3.rs`
- Modify: `sovmem/tests/p2_determinism.rs`
- Create: `sovmem/tests/source_contract.rs`

**Interfaces:**
- Adds no production surface. This task proves the complete approved boundary through fixture-first black-box, integration, and private unit tests.
- Produces exact two-store determinism snapshots and a 21-event chain consisting of genesis plus 20 mixed verdicts.

- [ ] **Step 1: Add a table-driven poisoning and policy matrix before production changes**

For each case, record expected stable code, exit class, durable hash snapshot, and forbidden sentinels. Include at least:

- malformed/noncanonical/unknown-field profile, policy, proposal, evaluator, manifest, receipt, journal, and ledger data;
- policy/profile raw-byte changes, invalid invariants, unknown rule fields, unsupported versions, stale target/base/bundle/manifest/generation hashes;
- symlinks, hard links where link count is observable, special files, unsafe path components, backslashes, controls, platform prefixes, case collisions, and forbidden namespaces;
- protected synthesis edits, invalid create/update intent, missing/invalid provenance, missing frontmatter, ID/path/mapping mismatch, duplicate ID, duplicate `(target, candidate-hash)`, source/open/daily quotas, and size limits;
- confidence underflow/overflow/extra precision and forged evaluator source/recommendation/reasons/hash data;
- agent Judgment create/update, forged Judgment frontmatter, candidate `type: judgment`, and attempts to target `judgments/`;
- empty/whitespace/NUL/over-limit reasons, rewrite misuse, intent conflicts, stale verdict bases, and changed exact replay material;
- target/candidate/rewrite/Judgment/manifest/index/receipt/policy-history/ledger hash corruption and all Level 4 corruption codes;
- injected write/fsync/rename failures and every named crash boundary;
- concurrent proposal, verdict, and recovery races plus stale leases and ambiguous recovery.

Add a Unix capability fixture that makes only `staging/` and its intake lease writable to the proposer process while seed, canon, ledger, policies, evidence, derived, transactions, verdict lock, and `policy.toml` are read-only. A valid proposal must stage successfully; direct/proposer-path writes to every protected root must fail; environment/user/actor spoofing and a no-TTY verdict invocation must not expand capability. On an unsupported permission model, the fixture reports the approved unsupported-platform code rather than skipping authority assertions silently.

Run each owning focused integration test and confirm RED for any missing case before modifying code.

- [ ] **Step 2: Make only the minimum implementation corrections exposed by the matrix**

Fix production code one error family at a time. Preserve policy evaluation order: the asserted code must always be the first failing stage from the approved design. Re-run the owning test after each correction. Do not introduce a fallback parser, normalization, automatic repair, authority bypass, sensitive diagnostic, or weakened Level 4 rule.

- [ ] **Step 3: Prove confidence and evaluator erasure invariance**

Create otherwise identical proposals with absent/present/forged confidence and structurally valid evaluator blocks. Assert:

- after structural validation, policy decision and ordered rule trace bytes are identical when advisory blocks are erased;
- recommendation labels never change ACCEPT/REJECT/REWRITE/TP/FP availability;
- neither block enters target bytes, Judgment bytes, manifest bytes, index behavior, or human authority checks;
- evaluator commitments are preserved only as evidence and any forgery fails before policy.

Add a harness-order fixture whose event trace records confidence capture before the candidate file is created, then assert the persisted millionths equal that pre-draft value. Also assert the binary accepts an absent confidence and cannot infer or manufacture one; the timing claim remains harness evidence, never binary authority.

- [ ] **Step 4: Build and verify exactly 20 mixed verdicts**

Use deterministic fixtures to stage and rule, in sequence, four ACCEPT, four REWRITE, four REJECT, four TP, and four FP proposals. Interleave verbs rather than grouping them. Assert:

```rust
let report = verify_store_level4(fixture.profile(), fixture.store()).unwrap();
assert_eq!(report.events, 21);
assert_eq!(report.verdicts, 20);
assert_eq!(report.applied_generations, 8);
assert_eq!(report.current_generation.as_deref(), Some("gen-0000000000000008"));
assert_eq!(fixture.verdict_counts(), [4, 4, 4, 4, 4]);
```

Check every prior ledger byte remains an exact prefix after each event, every event ID/previous hash/event hash is exact, only eight generations/Judgments/re-index receipts exist, and all 20 retained proposals resolve to the correct terminal lifecycle state.

- [ ] **Step 5: Prove the complete propose to human decision to re-query loop**

In a private transaction fixture with scripted human presence:

1. verify the unique phrase returns no seed hit;
2. submit a create proposal with the phrase and explicit provenance;
3. inspect and bind exact candidate/reason/advice inputs;
4. apply ACCEPT, then Level 4 verify;
5. assert applied target bytes equal candidate raw bytes exactly;
6. assert deterministic Judgment and reason are queryable;
7. remove only the disposable current P3 index;
8. full-rebuild from the ledger-selected generation;
9. repeat both target and Judgment query assertions;
10. apply a REWRITE in a second fixture and assert rewrite raw-byte equality through the same cycle.

The unique phrase and reason are fixture sentinels and never appear in captured command diagnostics.

- [ ] **Step 6: Prove byte-identical two-store determinism**

Run the same injected clock, proposal inputs, human decisions, and fault-free transaction sequence in two separately rooted stores. Compare relative-path file maps for seed commitments, policy history, retained proposals, canonical generations, manifests, Judgments, re-index receipts, ledger versions/head, lifecycle projections, and rebuilt index semantic snapshots. Exclude only disposable lease/journal absence checks; absolute roots and filesystem metadata never enter durable bytes.

- [ ] **Step 7: Prove frozen P2 isolation and production authority reachability**

Snapshot raw BLAKE3 for tracked P2 profile, corpus contract, golden, thresholds, relevance data, and baseline before P3 fixtures. Re-run after them and require exact equality. Re-run canonical P2 Level 3, two-build determinism, Q3 query, and eval tests.

Extend `source_contract.rs` to reject:

- any public re-export of the transaction writer or production test doubles;
- any MCP, server, socket, network, model, embedding, vector, Qdrant, async, `unsafe`, trust-gated write, auto-accept, or Git invocation in the P3 production modules;
- any canonical write call outside `transaction.rs` and the provisioning harness boundary;
- any binary route from proposer/list/status/stats/query code to the transaction writer.

- [ ] **Step 8: Run the complete Rust regression suite**

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_policy
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_store
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_proposal
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_canon
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_index
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_ledger
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_privacy
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_determinism
cargo test --locked --manifest-path sovmem/Cargo.toml
```

Expected: the red-team matrix, 20-verdict chain, full loop, determinism, and all P0-P2 tests pass.

- [ ] **Step 9: Commit the complete synthetic P3 gate**

```bash
git add sovmem/tests/p3_policy.rs sovmem/tests/p3_store.rs sovmem/tests/p3_proposal.rs sovmem/tests/p3_canon.rs sovmem/tests/p3_index.rs sovmem/tests/p3_ledger.rs sovmem/tests/p3_cli.rs sovmem/tests/p3_privacy.rs sovmem/tests/p3_determinism.rs sovmem/tests/support/p3.rs sovmem/tests/p2_determinism.rs sovmem/tests/source_contract.rs sovmem/src/profile sovmem/src/cli.rs sovmem/src/cli
git commit -m "P3.17: close synthetic write-back gate"
```

---

### Task 18: Build the safe-clone harness and capture the separately authorized real-data proof

**Files:**
- Create: `sovmem/examples/sovmem-p3-raw-hash.rs`
- Create: `scripts/provision-sovmem-p3-safe-clone.mjs`
- Create: `scripts/verify-sovmem-p3-safe-clone.mjs`
- Create: `test/sovmem-p3-safe-clone.test.js`
- Modify: `package.json`
- Modify: `README.md`
- Modify: `index.md`
- Modify: `sovmem/tests/source_contract.rs`
- Create after separate authorization and successful proof only: `docs/evidence/sovmem-p3-safe-clone-proof.json`

**Interfaces:**
- Adds harness-only provisioning and proof commands; neither script synchronizes a source vault, promotes canon, supplies a verdict, bypasses the real TTY, or calls the network.
- Publishes only the strict redacted schema `sovmem.p3.safe-clone-proof.v1` after all proof assertions pass.

- [ ] **Step 1: Write failing safe-root and evidence-schema tests**

Create `test/sovmem-p3-safe-clone.test.js` with temporary synthetic roots. Test exported pure helpers and subprocess behavior for:

- canonical equality, seed/store equality, and every ancestor/descendant overlap permutation;
- symlink resolution into a protected root;
- existing non-empty store, sealed store, unsupported store shape, and cross-filesystem publication refusal;
- verified seed copy preserving every relative raw hash;
- no write under seed, frozen artifact, or protected Vault roots;
- proof refusal when Level 3, Level 4, applied raw bytes, Judgment, receipt, re-query, rebuilt query, Vault snapshot, frozen hashes, or Q3 reproduction disagrees;
- evidence containing no absolute path, candidate/rewrite/reason/advice/query text, temporary name, user, host, PID, or OS error.

Run RED:

```bash
node --test test/sovmem-p3-safe-clone.test.js
```

Expected: FAIL because the harness modules do not exist.

- [ ] **Step 2: Implement fail-closed provisioning**

The provision command is:

```text
node scripts/provision-sovmem-p3-safe-clone.mjs --binary <sovmem> --seed <verified-level3-export> --p2-profile <frozen-profile> --p3-profile <p3-profile> --policy <policy-template> --store <approved-empty-root> --protected-root <live-vault> [--protected-root <other-read-only-root>]
```

Resolve all existing roots through realpath and the new store through its existing real parent plus basename. Reject equality, ancestor, or descendant overlap among store, seed, each protected root, repository frozen inputs, and their resolved symlink targets. Require the approved store root to be absent or empty and its parent to support the required same-directory fsync/rename semantics. Run frozen `verify --level 3 --require-corpus` before copying; copy exact bytes into `seed/`; fsync files/directories; verify the copied seed again; copy only the exact policy template to root `policy.toml`; keep the reviewed P3 profile outside the store and bind its raw hash through later proposal/genesis evidence; create exactly `policies/`, `staging/proposals/`, `staging/intake-rejections/`, `ledger/`, `canon/generations/`, `evidence/reindex/`, `derived/indexes/`, `transactions/`, and `locks/`; write the zero-byte ledger and `CURRENT=seed
`; and finish the verified unsealed pre-genesis shape. On any failure, remove only the incomplete newly created store after proving its resolved identity still matches the owned transaction marker.

Do not accept a Vault-to-store import flag. Do not call Git, except the later read-only proof script. Do not embed any live path in `package.json`, source, logs, or evidence.

- [ ] **Step 3: Add the harness-only raw-byte hash helper**

Create `sovmem/examples/sovmem-p3-raw-hash.rs` using only `std` and the already pinned `blake3 = 1.5.5`. It accepts exactly one positional regular non-symlink file, rejects extra arguments and non-regular/symlink inputs with one static stderr line and exit 2, reads raw bytes, and writes exactly the 64-character lowercase BLAKE3 plus LF to stdout. It never prints or persists the path. It has no directory traversal, write, Git, network, policy, proposal, verdict, or canonical capability.

The Node tests compile the example, hash a zero-byte regular file, require the standard BLAKE3 value `af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262`, and assert every error path is redacted. The scripts require its explicit executable path as `--hash-binary`; no ambient `b3sum`, package dependency, or network install is permitted.

Extend `source_contract.rs` to require the example to remain read-only and outside the production `sovmem` command dispatch, and to reject filesystem writes, Git/network/process spawning, proposal/verdict calls, or canonical/ledger symbols in that source.

- [ ] **Step 4: Implement all-or-nothing redacted proof publication**

The verification command shape is:

```text
node scripts/verify-sovmem-p3-safe-clone.mjs --binary <sovmem-binary> --hash-binary <compiled-hash-helper> --store <approved-store> --seed <verified-level3-export> --p2-profile <frozen-profile> --p3-profile <p3-profile> --golden <golden-jsonl> --thresholds <thresholds-json> --baseline <baseline-json> --frozen-index <approved-disposable-index> --vault <read-only-live-vault> --target-query-file <private-query-file> --judgment-query-file <private-query-file> --output docs/evidence/sovmem-p3-safe-clone-proof.json
```

The script parses machine JSON, uses the explicit pinned helper only for the frozen tracked artifacts and seed-copy equality, and writes no evidence until every check succeeds. It never passes candidate, rewrite, reason, advice, or query files to the hash helper; target/Judgment/receipt commitments come from cross-verified P3 artifacts. Its output object has this exact field order and validation:

| Field | Exact value or constraint |
| --- | --- |
| `schema_version` | `sovmem.p3.safe-clone-proof.v1` |
| `status` | `passed` |
| `store_id` | `audy-sovmem-writeback-v1` |
| `profile_version` | `audy-ssa0-p3-v1` |
| `seed_bundle_hash` | 64 lowercase hexadecimal characters, equal to the verified copied seed |
| `proposal_id` | `prp-` plus 64 lowercase hexadecimal characters |
| `verdict_id` | `ver-` plus 64 lowercase hexadecimal characters |
| `ledger_head` | 64 lowercase hexadecimal characters, equal to Level 4 |
| `generation` | `gen-0000000000000001` |
| `target_hash` | 64 lowercase hexadecimal characters, equal to the applied raw bytes |
| `judgment_hash` | 64 lowercase hexadecimal characters, equal to the deterministic Judgment |
| `reindex_receipt_hash` | 64 lowercase hexadecimal characters, equal to the committed receipt |
| `level4` | Exact nested keys `events:2`, `verdicts:1`, `applied_generations:1`, `errors:0` |
| `query_assertions` | Exact nested booleans `seed_hit:false`, `target_hit:true`, `judgment_hit:true`, `rebuilt_target_hit:true`, `rebuilt_judgment_hit:true` |
| `vault_unchanged` | `true` |
| `frozen_artifacts_unchanged` | `true` |
| `frozen_q3` | Exact nested evidence: 25 queries, 23 hits, `k=5`, precision `23/125`, recall `23/25`, misses `q-005`,`q-011`, status `GO` |

The script rejects unknown/missing fields in all consumed command objects, uses a hidden same-directory temporary plus fsync/rename for the final evidence file, and deletes the temporary on failure. It never serializes its input roots or sensitive strings.

- [ ] **Step 5: Add portable commands and authority documentation**

Add package scripts that require caller-supplied arguments and contain no machine-specific root. Update README and `index.md` to state:

- Audy Vault is external canonical context and remains read-only;
- the frozen 62-document Level 3 export remains the immutable P2/Q3 lane;
- only the separately provisioned `audy-sovmem-writeback-v1` lane evolves;
- promotion/synchronization back to Vault is out of P3 scope and always a future reviewed human workflow;
- agents may stage only, and only the real interactive CLI verdict transaction commits canon;
- Git is optional outer snapshot history, never judgment authority;
- public teaching uses ACCEPT/REJECT/REWRITE while the internal ledger keeps all five verbs.

- [ ] **Step 6: Run harness GREEN and commit harness-only changes**

```bash
cargo build --locked --manifest-path sovmem/Cargo.toml --example sovmem-p3-raw-hash
node --test test/sovmem-p3-safe-clone.test.js
npm test
git add sovmem/examples/sovmem-p3-raw-hash.rs sovmem/tests/source_contract.rs scripts/provision-sovmem-p3-safe-clone.mjs scripts/verify-sovmem-p3-safe-clone.mjs test/sovmem-p3-safe-clone.test.js package.json README.md index.md
git commit -m "P3.18: add safe-clone proof harness"
```

Expected: synthetic harness tests and the existing JavaScript suite pass. The evidence file does not exist yet.

- [ ] **Step 7: STOP and obtain separate safe-clone authorization**

Report the proposed sandbox root, read-only seed/export root, read-only live Vault root, expected bounded create target ID, exact candidate hash, exact human reason hash, confidence/advice presence, and cleanup/retention behavior to Khalil. Do not provision or write any real-data sandbox until he explicitly authorizes this live-data proof and location. Never request authority to write the live Vault or frozen export.

If no safe location is approved, mark the implementation gate blocked and do not advance to Task 19.

- [ ] **Step 8: Execute the authorized real-data ceremony**

After authorization only:

1. record frozen artifact and complete 62-document export raw hashes;
2. record the live Vault Git commit and clean/dirty fingerprint read-only;
3. provision the disjoint store and verify copied seed at Level 3;
4. build a fresh frozen P2 index at the separately approved disposable index root, query the unique phrase against the verified seed/export in bundle mode, and require zero hits;
5. submit the bounded proposal through `sovmem propose`;
6. inspect candidate, reason, confidence, and optional advice on the real terminal;
7. apply one human ACCEPT or REWRITE with the exact confirmation phrase;
8. verify target/Judgment/manifest/receipt and Level 4;
9. query target plus Judgment;
10. remove only the current `<approved-store>/derived/indexes/<generation-id>` directory, run the full store rebuild, and repeat both queries;
11. recheck Vault commit/fingerprint and every frozen raw hash;
12. rerun frozen Level 3, index, query, and eval and reproduce 23/25, P@5 0.1840, R@5 0.9200, misses `q-005` and `q-011`;
13. publish only the reviewed redacted proof object.

Any disagreement stops evidence publication. Synthetic evidence cannot substitute for this step.

- [ ] **Step 9: Verify and commit only the reviewed proof**

```bash
node scripts/verify-sovmem-p3-safe-clone.mjs --binary <sovmem-binary> --hash-binary <compiled-hash-helper> --store <approved-store> --seed <verified-level3-export> --p2-profile profiles/audy-ssa0-v0/profile.yaml --p3-profile profiles/audy-ssa0-p3-v1/profile.yaml --golden evals/audy-sovmem-v0/golden.jsonl --thresholds evals/audy-sovmem-v0/thresholds.json --baseline evals/audy-sovmem-v0/baseline.json --frozen-index <approved-disposable-index> --vault <read-only-live-vault> --target-query-file <private-target-query-file> --judgment-query-file <private-judgment-query-file> --output docs/evidence/sovmem-p3-safe-clone-proof.json
npm test
cargo test --locked --manifest-path sovmem/Cargo.toml
git add docs/evidence/sovmem-p3-safe-clone-proof.json index.md
git commit -m "P3.18: record verified safe-clone proof"
```

Expected: proof schema passes, Vault/frozen evidence is unchanged, full P2 reproduction is GO, and no private path or sensitive body appears in tracked evidence.

---

### Task 19: Close the reviewed P3 audit state only after every gate passes

**Files:**
- Modify: `test/sovmem-build-audit.test.js`
- Modify: `scripts/lib/sovmem-build-audit.mjs`
- Modify: `docs/sovmem-build-state.json`
- Regenerate: `docs/SOVMEM-BUILD-AUDIT.md`
- Modify: `docs/sovmem_v0.3_execution_plan.md`
- Modify: `index.md`

**Interfaces:**
- Advances the reviewed state schema to `sovmem-build-audit-v4` and accepts P3=100 only when strict P3 evidence and the tracked safe-clone proof agree.
- Keeps P0, P1, and P2 canonical evidence exact; computes `(1 + 2 + 2 + 3.5) / 13.5 = 62.96%` and renders the existing reviewed whole-percent rule as 63%.

- [ ] **Step 1: Write failing audit-v4 and forged-closure tests**

Extend `validState()` with a strict `evidence.write_back` object and require exact fields:

```text
schema_version, store_id, profile_version, implementation_commit,
level4_events, level4_verdicts, level4_applied_generations, ledger_head,
mixed_chain_events, mixed_chain_verdicts, mixed_chain_applied_generations,
mixed_chain_counts, mixed_chain_ledger_head, full_loop_passed,
deleted_index_rebuild_passed, red_team_passed, privacy_passed,
concurrency_passed, crash_recovery_passed, advisory_erasure_passed,
safe_clone_proof_path, safe_clone_proof_blake3, frozen_p2_unchanged
```

Use exact strings, lowercase 64-character hashes, booleans, and nonzero counts. The safe-clone `level4_*` fields and `ledger_head` must exactly match its tracked proof. The synthetic chain must have `mixed_chain_events = mixed_chain_verdicts + 1`, at least 20 verdicts, every internal verb count greater than zero, applied generations equal ACCEPT plus REWRITE counts, and its own nonempty `mixed_chain_ledger_head`. Require `safe_clone_proof_path` to equal `docs/evidence/sovmem-p3-safe-clone-proof.json`; parse that file and cross-check store/profile/head/event/verdict/generation evidence.

Add one mutation test per field plus tests that reject:

- P3 passed with missing/failed proof, fewer than 20 mixed verdicts, absent verb, zero applied generations, or false full-loop/rebuild/red-team/privacy/concurrency/recovery/advisory/frozen-P2 flags;
- changed P2 bundle/golden/threshold/baseline/retrieval evidence;
- P4/P5 progress, automatic promotion, source sync, MCP write/verdict, trust routing, or enforced-GitHub-control claims;
- missing, non-integer, zero, or below-merged-baseline test counts;
- overall rounding other than the generator's approved 63%.

Run RED:

```bash
node --test test/sovmem-build-audit.test.js
```

Expected: FAIL because v3 does not validate P3 closure evidence.

- [ ] **Step 2: Implement strict v4 validation and rendering**

Update the audit validator, not the phase weights. Validate exact object keys, evidence hashes, safe-proof schema/cross-links, implementation commit, phase descriptions, manual-PR disclaimer, and P4/P5 zero state. Keep the existing source fingerprint and freshness behavior, adding the P3 modules/profile/policy/tests/harness/proof to required registry evidence. Initially require positive test counts; after Step 3, replace that temporary predicate with exact constants copied from the measured output and add mutation tests for both constants.

Render P3 completed evidence with no private paths, candidate/rewrite/reason/advice/query text, or claim of automatic Vault promotion. State explicitly that the evolving store is distinct from Audy Vault and frozen P2, and that the safe proof used an approved clone.

- [ ] **Step 3: Measure the complete suites after implementation and before reviewed state**

From the worktree root, record actual counts and outputs:

```bash
npm test
cargo test --locked --manifest-path sovmem/Cargo.toml
cargo fmt --manifest-path sovmem/Cargo.toml --all -- --check
cargo clippy --locked --manifest-path sovmem/Cargo.toml --all-targets --all-features -- -D warnings
```

Also rerun the explicit frozen Q3 command sequence and require 23/25, P@5 0.1840, R@5 0.9200, and misses `q-005`, `q-011`. Confirm:

```bash
git diff --exit-code origin/main -- profiles/audy-ssa0-v0/profile.yaml profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml evals/audy-sovmem-v0/golden.jsonl evals/audy-sovmem-v0/thresholds.json evals/audy-sovmem-v0/baseline.json
```

Expected: Rust, formatting, lint, frozen Q3, and every non-audit JavaScript test pass. The JavaScript runner may report only the deliberate live-state/freshness failures caused by strict v4 code reading the still-v3 reviewed state; any other JavaScript failure stops the task. Record the runner's complete JavaScript count and the complete Rust count. `audit:sovmem:check` is intentionally deferred until state/report move together in Step 5.

- [ ] **Step 4: Record only measured reviewed evidence**

Set:

- P0/P1/P2 to their existing exact 100% evidence;
- P3 to `completion_percent:100`, `gate:"passed"`, with complete artifacts and proof;
- P4/P5 to exactly 0%, `not-started`;
- overall audit to 63% by the existing rounding rule;
- `evidence.tests` to the actual JavaScript and Rust counts from Step 3;
- `evidence.implementation_commit` and `evidence.write_back.implementation_commit` to the reviewed implementation/proof head, never a predicted hash;
- `as_of`, summary, next step, history, completed, remaining, and evidence paths to truthful reviewed values.

Update the normative execution plan only with achieved P3 facts and unchanged P4/P5 scope. Do not describe source promotion, MCP, sentry, trust, TTF, or automatic behavior as delivered.

In the same change, pin the Step 3 counts as exact named constants in `sovmem-build-audit.mjs` and update the audit tests to reject either count plus or minus one. Do not estimate the constants from the number of source files.

Do this substep test-first: add the two measured-count mutation tests and run them by exact test-name pattern against the generic positive-count validator to see RED; then pin the measured constants and rerun those focused tests GREEN before editing the reviewed state. If the complete Step 5 runner reports a different count, update both measured constants and state to that new observed output and repeat Step 5; never guess.

- [ ] **Step 5: Regenerate and validate the audit**

```bash
npm run audit:sovmem
npm run audit:sovmem:check
npm test
cargo test --locked --manifest-path sovmem/Cargo.toml
cargo fmt --manifest-path sovmem/Cargo.toml --all -- --check
cargo clippy --locked --manifest-path sovmem/Cargo.toml --all-targets --all-features -- -D warnings
git diff --check
```

Read the generated report and confirm P0/P1/P2/P3 = 100%, P4/P5 = 0%, overall = 63%, actual test counts, safe proof, Level 4 chain, manual PR discipline, and no private/sensitive text.

- [ ] **Step 6: Commit audit closure and stop for branch review**

```bash
git add test/sovmem-build-audit.test.js scripts/lib/sovmem-build-audit.mjs docs/sovmem-build-state.json docs/SOVMEM-BUILD-AUDIT.md docs/sovmem_v0.3_execution_plan.md index.md
git commit -m "P3.19: close verified write-back gate"
git status --short --branch
```

Expected: clean worktree on `codex/sovmem-p3-verified-write-back-loop`. Do not push, open a PR, merge, remove the worktree, or claim repository enforcement. Report the reviewed head and verification evidence for Khalil's manual branch review.

---

## Plan Exit Gate

This plan is ready to execute only after Khalil reviews and approves this committed document. Approval authorizes the fixture-first implementation tasks, not a live canonical or safe-clone write. Task 18 still requires separate explicit authorization for the sandbox location and real-data ceremony.

During execution, stop rather than weaken the design if authority mapping, atomic publication, Level 4 chain validity, frozen-P2 isolation, human-only TTY control, safe-root separation, or dependency constraints cannot be satisfied. Do not advance the reviewed audit state until every implementation and proof gate is green.
