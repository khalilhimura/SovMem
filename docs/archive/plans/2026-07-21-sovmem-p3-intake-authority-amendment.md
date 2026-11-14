> Public-copy note: workstation paths are environment placeholders; the original private historical plan is retained in Nous. Set the named variables to your own locations before adapting any archived commands.

> **ARCHIVED** — This implementation plan has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix A: Implementation Plan Index](../sovmem-master-spec.md#appendix-a-implementation-plan-index) for a summary of its scope.
>
# SovMem P3 Intake Authority Amendment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace proposer-writable P3 intake evidence with a protected, human-sealed, append-only intake authority that makes quota enforcement and Level 4 historical replay deterministic and tamper-evident.

**Architecture:** P3 v2 separates an untrusted proposal inbox from an immutable intake log and sealed proposal pairs. `propose` only creates a framed inbox submission; an interactive `intake seal` ceremony deterministically evaluates it, publishes a `STAGED` or redacted `REJECTED` intake event, and becomes the sole source for queue policy. Verdicts bind the observed intake sequence and event hash; Level 4 verifies the complete intake chain and replays the exact prefix recorded by each verdict.

**Tech Stack:** Rust 1.84; existing `serde`, `serde_json`, `toml`, `blake3`, `clap`, Tantivy 0.24.2, Petgraph 0.8.3; Node audit scripts. No new dependency.

## Global Constraints

- Work only in `${P3_WORKTREE}` on `codex/sovmem-p3-verified-write-back-loop`; preserve every other worktree and user-owned file.
- Use Rust 1.84, no `unsafe`, no async, no MCP, no network call, and no dependency addition or version change.
- Keep the frozen P2 corpus, 25 golden queries, thresholds, relevance labels, and canonical `baseline.json` byte-identical.
- Raw candidate, metadata, manifest, policy, ledger, and intake hashes are BLAKE3 over their exact raw bytes; never normalize line endings before a contract hash.
- Agents may submit only untrusted inbox requests. The local human-only `intake seal` command creates intake evidence; the existing human-only verdict ceremony remains the only canon commit path.
- The only internal verdict verbs are `ACCEPT`, `REJECT`, `REWRITE`, `TP`, and `FP`; empty verdict reasons fail closed; confidence and evaluator data remain advisory.
- Do not mutate Audy Vault, an exported canonical bundle, live canon, or frozen P2 artifacts during tests. Real-data proof uses only an approved safe clone.
- A v1 P3 profile/store must be rejected as unsupported by v2 and must not be migrated implicitly. No migration is in this amendment.
- All new diagnostics are stable, static, JSONL errors on stderr, and redact bodies, paths, source values, evaluator content, reasons, OS identities, and temporary names.
- Run every new behavior through a nonzero, focused RED fixture before implementation; then focused GREEN, related subsystem suite, serialized Rust suite (`--jobs 1` and `--test-threads=1`), JavaScript suite, frozen-artifact guard, safe-clone proof, and audit freshness check.

---

## File Structure

| File | Responsibility |
|---|---|
| `profiles/audy-ssa0-p3-v2/profile.yaml` | Immutable P3 v2 profile source; its raw BLAKE3 is the profile commitment. |
| `profiles/audy-ssa0-p3-v2/policy.toml` | Deterministic P3 v2 policy source with the existing exact policy schema and pinned v2 store/profile IDs. |
| `sovmem/src/profile/config.rs` | Accept only the reviewed P3 v2 profile directory and expose the structural metadata ceiling. |
| `sovmem/src/profile/store.rs` | Store-v2 paths, exact layout, safe-file traversal, intake transaction visibility, and lock layout validation. |
| `sovmem/src/profile/intake.rs` (new) | Canonical `IntakeEvent` schema, BLAKE3 intake-chain reader/appender, sealed-pair loading, and intake-prefix projection. |
| `sovmem/src/profile/proposal.rs` | Untrusted inbox submission only, proposal framing, pair parsing, queue projections, and no authority-bearing staging receipts. |
| `sovmem/src/profile/policy.rs` | Deterministic policy evaluation over an explicit `IntakePrefix`, including structural-then-policy metadata limits. |
| `sovmem/src/profile/ledger.rs` | Versioned verdict payload with `(intake_sequence, intake_head_hash)` commitment and v2 verdict hash validation. |
| `sovmem/src/profile/transaction.rs` | Human intake-seal ceremony, combined lock protocol, verdict preflight intake binding, and recovery routing. |
| `sovmem/src/profile/verify.rs` | Level 4 intake-chain walk, exact-prefix replay, historical policy loading, and stable corruption-code mapping. |
| `sovmem/src/profile/stats.rs` and `sovmem/src/cli/p3.rs` | Protected queue/list/status/stats projections and redacted CLI contracts. |
| `sovmem/src/cli.rs` | `intake seal` command grammar and its no-batch/no-MCP surface. |
| `sovmem/src/profile/p3_test_support.rs` | P3 v2 bootstrap fixture, deterministic clock, scripted intake human, sealed-pair helpers, and exact durable snapshots. |
| `sovmem/src/profile/p3_internal/{proposal,policy,ledger,store,poisoning_matrix,fixture_concurrency,full_loop,capability}.rs` | Focused internal RED/GREEN, corruption, crash, concurrency, and full-loop fixtures. |
| `sovmem/tests/{p3_cli,p3_privacy,default_profile_isolation}.rs` | Public compiled-CLI, privacy, profile-isolation, queue, and exit-code regressions. |
| `sovmem/tests/fixtures/p3-cli-store-v2/` (new) | Minimal checked-in v2 CLI stores; keep v1 fixtures unchanged as unsupported-legacy inputs. |
| `scripts/lib/sovmem-build-audit.mjs`, `test/sovmem-build-audit.test.js`, `docs/sovmem-build-state.json`, `docs/SOVMEM-BUILD-AUDIT.md` | Final-only audit identity and P3 completion evidence. |

## Shared Interfaces

All later tasks use these exact names and fields. Keep their serialization `#[serde(deny_unknown_fields)]` and canonical JSON line encoding (one JSON object followed by exactly one LF).

```rust
pub const P3_PROFILE_VERSION: &str = "audy-ssa0-p3-v2";
pub const P3_STORE_ID: &str = "audy-sovmem-writeback-v2";
pub const P3_STRUCTURAL_META_BYTES: usize = 262_144;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntakeHead {
    pub sequence: u64,
    pub event_hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntakePrefix {
    pub head: IntakeHead,
    pub staged: Vec<ProposalSummary>,
    pub rejected: Vec<IntakeRejection>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IntakePayload {
    pub schema_version: String,
    pub event_type: String,
    pub sequence: u64,
    pub event_id: String,
    pub occurred_at: String,
    pub previous_event_hash: Option<String>,
    pub outcome: IntakeOutcome,
    pub source_id: Option<String>,
    pub proposal_id: Option<String>,
    pub proposal_meta_hash: Option<String>,
    pub candidate_hash: Option<String>,
    pub policy_hash: String,
    pub profile_hash: String,
    pub rule_codes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum IntakeOutcome { STAGED, REJECTED }

pub fn read_intake(paths: &StorePaths) -> Result<IntakeView, IntakeError>;
pub fn intake_prefix(view: &IntakeView, head: &IntakeHead) -> Result<IntakePrefix, IntakeError>;
pub fn seal_intake_submission(
    profile_path: &Path,
    store_root: &Path,
    submission_id: &str,
) -> Result<IntakeSealOutcome, TransactionError>;
```

### Task 1: Establish the explicit v2 boundary and exact store layout

**Files:**
- Create: `profiles/audy-ssa0-p3-v2/profile.yaml`
- Create: `profiles/audy-ssa0-p3-v2/policy.toml`
- Modify: `sovmem/src/profile/config.rs`
- Modify: `sovmem/src/profile/store.rs`
- Modify: `sovmem/src/profile/p3_test_support.rs`
- Modify: `sovmem/tests/support/p3.rs`
- Test: `sovmem/src/profile/p3_internal/store.rs`
- Test: `sovmem/tests/default_profile_isolation.rs`

**Consumes:** the committed P3 v1 implementation and the amendment design.

**Produces:** a P3 v2-only loader and an empty, valid store layout containing `staging/inbox`, `intake/events.jsonl`, `intake/proposals`, `intake/transactions`, `ledger`, `canon`, `evidence`, `derived`, `transactions`, and `locks`.

- [ ] **Step 1: Write the v2 boundary RED fixtures**

Add `fn p3_v2_store_rejects_v1_layout_and_removed_staging_roots()` that copies a valid v1 store/profile unchanged, then asserts that a v2 invocation rejects it before reading a proposal or opening a TTY. In the same test, assert a valid v2 empty store accepts exactly the following root names and rejects both removed v1 authority paths:

```rust
assert_eq!(inspect_store(&v2_profile, &v1_store).unwrap_err().code(), "store.layout_invalid");
for forbidden in ["staging/proposals", "staging/intake-rejections"] {
    fs::create_dir_all(v2_store.join(forbidden)).unwrap();
    assert_eq!(inspect_store(&v2_profile, &v2_store).unwrap_err().code(), "store.layout_invalid");
    fs::remove_dir_all(v2_store.join(forbidden)).unwrap();
}
```

Add `fn p3_v2_profile_isolated_from_v1()` that passes `profiles/audy-ssa0-p3-v1/profile.yaml` and expects `profile.identifier`, then loads `profiles/audy-ssa0-p3-v2/profile.yaml` and asserts the exact `P3_PROFILE_VERSION` and raw profile hash.

- [ ] **Step 2: Run the focused RED tests**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::store::p3_v2_store_rejects_v1_layout_and_removed_staging_roots -- --exact --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::store::p3_v2_profile_isolated_from_v1 -- --exact --nocapture
```

Expected: at least one named test fails because v1 is still the accepted profile/store and the intake directories are absent.

- [ ] **Step 3: Create the v2 profile/policy and validate exact layout**

Copy the v1 profile/policy bytes into the new v2 directory, changing only these policy values:

```toml
store_id = "audy-sovmem-writeback-v2"
profile_version = "audy-ssa0-p3-v2"
```

Set the constants shown in **Shared Interfaces**. In `StorePaths`, add `inbox`, `intake`, `intake_events`, `intake_proposals`, and `intake_transactions`. Replace the v1 staging validation with exact checks equivalent to:

```rust
exact_directory(&paths.root, &[
    "policy.toml", "policies", "seed", "staging", "intake", "ledger", "canon",
    "evidence", "derived", "transactions", "locks",
])?;
exact_directory(&paths.staging, &["inbox"])?;
exact_directory(&paths.intake, &["events.jsonl", "proposals", "transactions"])?;
regular_file(&paths.intake_events)?;
regular_directory(&paths.intake_proposals)?;
regular_directory(&paths.intake_transactions)?;
```

Include every new root in same-filesystem and unsafe-tree checks. Treat any entry in `intake/transactions`, `locks/intake.lock`, or an intake temporary as `transaction.incomplete` on public reads.

- [ ] **Step 4: Convert the fixture bootstrap only**

Update `provision_store` in `p3_test_support.rs` and `tests/support/p3.rs` to create the new layout and source its profile/policy from `audy-ssa0-p3-v2`. Do not edit frozen P2 corpus/golden/baseline files or rewrite the checked-in v1 CLI fixture.

- [ ] **Step 5: Run GREEN and commit the v2 boundary**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::store::p3_v2_store_rejects_v1_layout_and_removed_staging_roots -- --exact --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::store::p3_v2_profile_isolated_from_v1 -- --exact --nocapture
git add profiles/audy-ssa0-p3-v2 sovmem/src/profile/config.rs sovmem/src/profile/store.rs sovmem/src/profile/p3_test_support.rs sovmem/tests/support/p3.rs sovmem/src/profile/p3_internal/store.rs sovmem/tests/default_profile_isolation.rs
git commit -m "P3.20: establish intake-authority v2 boundary"
```

Expected: named fixtures pass, v1 is never silently upgraded, and only the listed v2 files are committed.

### Task 2: Implement immutable intake records and sealed-pair custody

**Files:**
- Create: `sovmem/src/profile/intake.rs`
- Modify: `sovmem/src/profile/mod.rs`
- Modify: `sovmem/src/profile/store.rs`
- Test: `sovmem/src/profile/p3_internal/proposal.rs`
- Test: `sovmem/src/profile/p3_internal/store.rs`

**Consumes:** Task 1 `StorePaths` and v2 layout.

**Produces:** strict intake record parsing, canonical BLAKE3 chaining, immutable pair readers, and exact prefix materialization.

- [ ] **Step 1: Write the intake-chain RED cases**

Add `fn intake_chain_rejects_corruption_and_prefix_mismatches()` which constructs canonical v2 intake lines and then independently corrupts: sequence, `int-` ID, previous hash, event hash, schema, newline, unknown field, outcome field shape, non-null source on an invalid source, unsorted rule codes, a `REJECTED` candidate/meta/target field, a missing staged pair, an extra staged-pair file, and a pair hash mismatch. Assert the exact error codes below:

```rust
assert_eq!(read_intake(&paths).unwrap_err().code(), "intake.chain_invalid");
assert_eq!(read_intake(&paths).unwrap_err().code(), "intake.record_invalid");
assert_eq!(intake_prefix(&view, &forged_head).unwrap_err().code(), "intake.head_mismatch");
assert_eq!(read_sealed_pair(&paths, &proposal_id).unwrap_err().code(), "intake.pair_binding");
```

Add `fn intake_chain_orders_equal_timestamps_and_rejects_clock_regression()` with two equal-time events that verifies prefix order is `sequence`, not lexical time, and a backward-time append that expects `intake.clock_regression` without a new line.

- [ ] **Step 2: Run the intake RED tests**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::proposal::intake_chain_rejects_corruption_and_prefix_mismatches -- --exact --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::proposal::intake_chain_orders_equal_timestamps_and_rejects_clock_regression -- --exact --nocapture
```

Expected: the tests execute and fail because `intake.rs` and its chain reader do not exist.

- [ ] **Step 3: Implement canonical `IntakeEvent` and pair binding**

Use the Shared Interfaces exactly. Define `INTAKE_EVENT_SCHEMA = "sovmem-intake-event-v1"` and hash an event with a distinct domain:

```rust
fn intake_event_hash(payload: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sovmem.intake-event.v1");
    hasher.update(&(payload.len() as u64).to_le_bytes());
    hasher.update(payload);
    hasher.finalize().to_hex().to_string()
}
```

For `STAGED`, require all proposal/meta/candidate fields, empty `rule_codes`, an existing pair at `intake/proposals/<two hex>/<proposal-id>/`, exact raw hashes, and no extra file. For `REJECTED`, require all three pair fields `None`, `source_id` either valid or `None`, and nonempty sorted/deduplicated policy code strings. Serialize a line with `serde_json::to_vec`, append one LF, reparse, and require byte equality.

`read_intake` must verify every line before returning `IntakeView`. `intake_prefix` accepts only `(0, None)` or an exact event `(sequence, hash)`, returns all earlier records inclusively, and projects only sealed pair summaries plus redacted rejection summaries.

- [ ] **Step 4: Implement append-only publication helpers**

Create-new and fsync candidate and metadata files under a hidden same-parent directory; fsync it; rename it only into the immutable intake shard; fsync shard and `intake/proposals`. Create a temporary `events.jsonl` successor from verified previous bytes, re-read the prior head before rename, rename without replacement, and fsync `intake/`. A collision returns `intake.record_invalid`; no helper may remove or overwrite an intake event or sealed pair.

- [ ] **Step 5: Run GREEN and commit intake custody**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::proposal::intake_chain_rejects_corruption_and_prefix_mismatches -- --exact --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::proposal::intake_chain_orders_equal_timestamps_and_rejects_clock_regression -- --exact --nocapture
git add sovmem/src/profile/intake.rs sovmem/src/profile/mod.rs sovmem/src/profile/store.rs sovmem/src/profile/p3_internal/proposal.rs sovmem/src/profile/p3_internal/store.rs
git commit -m "P3.21: add protected intake evidence"
```

Expected: a retained record can be verified from raw bytes, and every corruption fails before a public projection is returned.

### Task 3: Make `propose` untrusted transport and add human-only `intake seal`

**Files:**
- Modify: `sovmem/src/profile/proposal.rs`
- Modify: `sovmem/src/profile/transaction.rs`
- Modify: `sovmem/src/profile/p3_test_support.rs`
- Modify: `sovmem/src/cli.rs`
- Modify: `sovmem/src/cli/p3.rs`
- Test: `sovmem/src/profile/p3_internal/proposal.rs`
- Test: `sovmem/tests/p3_cli.rs`
- Test: `sovmem/tests/p3_privacy.rs`

**Consumes:** Tasks 1-2 store and intake interfaces.

**Produces:** `propose -> submitted` and `intake seal <submission-id> -> staged|rejected-at-intake`, with TTY confirmation and no canonical mutation.

- [ ] **Step 1: Write public CLI RED cases**

Add compiled CLI cases that prove:

```rust
assert_eq!(propose_output["state"], "submitted");
assert!(fs::read(store.join("intake/events.jsonl"))?.is_empty());
assert_eq!(seal_without_tty.code(), "human.tty_required");
assert_eq!(seal_wrong_phrase.code(), "intake.confirmation_failed");
assert_eq!(seal_rejected["state"], "rejected-at-intake");
assert!(canon_hashes_before == canon_hashes_after);
assert!(ledger_before == ledger_after);
```

Use a real `/dev/tty` child fixture for the successful seal, pass exactly `SEAL INTAKE <submission-id>
`, and assert `intake seal` rejects `--yes`, `--format text` body leakage, batch IDs, and all unknown options through Clap exit code 2.

- [ ] **Step 2: Run the public RED tests**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli propose_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli intake_seal_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_privacy intake_seal_ -- --nocapture
```

Expected: new named tests fail because proposal submission still stages authority-bearing pairs and no `intake` command exists.

- [ ] **Step 3: Restrict submission to a framed inbox item**

Replace `submit_proposal` publication with create-new publication to `staging/inbox/<submission-id>/`. The request contains raw `candidate.md`, a strict canonical `submission.meta.json`, and no policy decision. `submission_id` is `sub-` plus the BLAKE3 of a framed domain, timestamp, source, operation, intent, target identity, candidate raw hash/length, profile hash, and policy hash. `SubmitProposalResult.state` is exactly `"submitted"`.

The inbox reader accepts only the two exact files, rejects symlinks/special files/extra entries, validates byte caps and raw binding, and never treats a proposal in the inbox as queue evidence. Delete no inbox item from `propose`.

- [ ] **Step 4: Implement the interactive seal transaction**

Add the `Command::Intake { command: IntakeCommand::Seal { submission_id, profile, store, format } }` grammar. `run_intake_seal` calls only `seal_intake_submission`. It opens `/dev/tty`, renders commitments and redacted policy codes only, then requires:

```rust
let expected = format!("SEAL INTAKE {submission_id}
");
if human.read_confirmation()? != expected {
    return Err(TransactionError::IntakeConfirmationFailed);
}
```

Under `intake.lock`, read inbox bytes once, sample `Clock` once, verify the current intake chain, construct a pair, evaluate policy against `intake_prefix(current_head)`, publish a pair only on pass, append exactly one `STAGED` or `REJECTED` intake event, then remove the corresponding inbox directory after the event fsync. Return a redacted `IntakeSealOutcome { submission_id, proposal_id: Option<String>, state, intake_sequence, intake_head_hash, policy_hash, profile_hash }`. The function must not call generation, index, judgment, or ledger append code.

- [ ] **Step 5: Run GREEN and commit the two-stage authority path**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli propose_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli intake_seal_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_privacy intake_seal_ -- --nocapture
git add sovmem/src/profile/proposal.rs sovmem/src/profile/transaction.rs sovmem/src/profile/p3_test_support.rs sovmem/src/cli.rs sovmem/src/cli/p3.rs sovmem/src/profile/p3_internal/proposal.rs sovmem/tests/p3_cli.rs sovmem/tests/p3_privacy.rs
git commit -m "P3.22: separate submission from intake sealing"
```

Expected: a proposer can create only an untrusted request, only a human seal creates queue evidence, and sealing cannot mutate canon.

### Task 4: Rebind policy evaluation to protected prefixes and structural metadata parsing

**Files:**
- Modify: `sovmem/src/profile/policy.rs`
- Modify: `sovmem/src/profile/proposal.rs`
- Modify: `sovmem/src/profile/intake.rs`
- Test: `sovmem/src/profile/p3_internal/policy.rs`
- Test: `sovmem/src/profile/p3_internal/advisory_erasure.rs`

**Consumes:** Tasks 2-3 `IntakePrefix` and sealed-pair reader.

**Produces:** policy decisions that use only protected records, exclude the candidate itself, preserve advisory non-authority, and survive a lower current metadata limit during Level 4 parsing.

- [ ] **Step 1: Write the policy/replay RED fixtures**

Add tests for all of these named conditions: deleting a v1-style receipt cannot affect quotas; a forged/mutated intake `REJECTED` line fails before policy; a later sealed item never changes an earlier prefix decision; equal timestamps use sequence; backward time creates no event; and a lower rotated `max_meta_bytes` still lets a historical sealed pair parse structurally and replay under its archived policy.

The policy-rotation assertion must be exact:

```rust
let historical = read_sealed_pair_structural(&paths, &proposal_id).unwrap();
assert!(historical.meta_bytes.len() > current.policy.max_meta_bytes);
assert!(verify_store_level4(&profile, &store).is_ok());
let record = proposal_status(&profile, &store, &open_id, false).unwrap();
assert_eq!(record.state, ProposalState::Staged);
assert!(record.policy_stale);
```

Use an archived policy whose `max_meta_bytes` permits the pair and a current policy whose value is lower. Do not change a P2 artifact to create this case.

- [ ] **Step 2: Run the focused RED tests**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::policy::intake_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::advisory_erasure -- --nocapture
```

Expected: the named tests fail because policy still reads staging receipts/pairs and enforces the mutable current metadata limit at read time.

- [ ] **Step 3: Replace mutable evaluator inputs with `IntakePrefix`**

Change the policy context to:

```rust
pub struct PolicyContext<'a> {
    pub policy: &'a LoadedPolicy,
    pub profile: &'a P3Profile,
    pub store: &'a StoreSnapshot,
    pub proposal: &'a ProposalPair,
    pub intake: &'a IntakePrefix,
}
```

Build retained/open and rejected-day inputs only from `intake`, omitting `proposal.meta.proposal_id`. Keep the existing stage order and code sorting. Delete `rejection_receipts`, `publish_rejection`, and every policy use of `staging/proposals`/`staging/intake-rejections`.

Use `P3_STRUCTURAL_META_BYTES` solely in `read_sealed_pair_structural`. Once parsed and canonically bound, apply the appropriate policy limit: current policy at seal, archived event policy at Level 4. Add `ProposalRecord.policy_stale: bool`; an open pair that exceeds the current policy remains `ProposalState::Staged` with `policy_stale = true`, never a parser error or an invented lifecycle state.

- [ ] **Step 4: Preserve advisory non-authority**

Keep confidence/evaluator validation and commitment intact, but test policy output twice with `confidence = None` and `evaluator = None`. Both passes must produce exactly the same `passed` flag and rule-code vector. No evaluator field may be consulted by `PolicyEvaluator`, intake seal outcome selection, verdict selection, or canonical apply logic.

- [ ] **Step 5: Run GREEN and commit prefix-bound policy**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::policy::intake_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::advisory_erasure -- --nocapture
git add sovmem/src/profile/policy.rs sovmem/src/profile/proposal.rs sovmem/src/profile/intake.rs sovmem/src/profile/p3_internal/policy.rs sovmem/src/profile/p3_internal/advisory_erasure.rs
git commit -m "P3.23: evaluate policy from protected intake prefixes"
```

Expected: policy no longer reads mutable receipts and policy rotation cannot deadlock Level 4 parsing.

### Task 5: Bind every human verdict to its observed intake high-water

**Files:**
- Modify: `sovmem/src/profile/ledger.rs`
- Modify: `sovmem/src/profile/transaction.rs`
- Modify: `sovmem/src/profile/judgment.rs`
- Modify: `sovmem/src/profile/p3_test_support.rs`
- Test: `sovmem/src/profile/p3_internal/ledger.rs`
- Test: `sovmem/src/profile/p3_internal/canon.rs`

**Consumes:** `IntakeHead`, `IntakePrefix`, and human seal data from Tasks 2-4.

**Produces:** a versioned verdict hash domain that commits the exact protected intake prefix under the combined verdict/intake lease.

- [ ] **Step 1: Write verdict-binding RED tests**

Extend ledger fixtures to assert a valid event contains and hashes these exact fields:

```rust
assert_eq!(event.payload().intake_sequence, 7);
assert_eq!(event.payload().intake_head_hash.as_deref(), Some("<64 lowercase hex>"));
```

Then forge each independently: wrong sequence, wrong hash at a valid sequence, `(0, Some(_))`, nonzero sequence with `None`, and a high-water that omits a visible earlier rejection. Each Level 4 failure must equal `ledger.intake_binding`; the fixture must assert `canon`, `ledger`, and `intake` durable hashes are unchanged after failed preflight.

- [ ] **Step 2: Run the ledger RED tests**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::ledger::intake_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::canon::intake_ -- --nocapture
```

Expected: the tests fail because `VerdictPayload` lacks the intake commitment and the v1 ledger event domain still validates it.

- [ ] **Step 3: Version the event payload and identity material**

Add `intake_sequence: u64` and `intake_head_hash: Option<String>` to `VerdictPayload`, `VerdictLine`, canonical serialization, `VerdictIdentityInput`, transaction IDs, and verdict display. Set `LEDGER_EVENT_SCHEMA` to `sovmem-ledger-event-v2` and prefix its BLAKE3 event hash with `sovmem.ledger-event.v2`; retain no parser that accepts v1 events under v2.

Before preflight renders the verdict prompt, hold both leases in the fixed order `verdict.lock` then `intake.lock`, read and verify the intake chain, set `ConfirmedDecision.intake_head`, build policy input from that prefix, and re-read the same head immediately before policy archive and before ledger append. Any change returns `intake.head_mismatch` without a verdict event or canon mutation.

- [ ] **Step 4: Keep nonapplying verbs ledger-only**

For `REJECT`, `TP`, and `FP`, write the new intake-bound verdict event but leave `canon/`, `evidence/reindex/`, `derived/indexes/`, and `canon/CURRENT` byte-identical. For `ACCEPT`/`REWRITE`, retain exact human-approved candidate/rewrite bytes and existing canonical result binding. Continue rejecting empty reasons before TTY output or any lock publication.

- [ ] **Step 5: Run GREEN and commit the verdict commitment**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::ledger::intake_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::canon::intake_ -- --nocapture
git add sovmem/src/profile/ledger.rs sovmem/src/profile/transaction.rs sovmem/src/profile/judgment.rs sovmem/src/profile/p3_test_support.rs sovmem/src/profile/p3_internal/ledger.rs sovmem/src/profile/p3_internal/canon.rs
git commit -m "P3.24: bind verdicts to intake high-water"
```

Expected: every v2 verdict cryptographically commits the only intake history it may rely on.

### Task 6: Make Level 4 walk the complete intake chain and replay exact prefixes

**Files:**
- Modify: `sovmem/src/profile/verify.rs`
- Modify: `sovmem/src/profile/store.rs`
- Modify: `sovmem/src/profile/intake.rs`
- Test: `sovmem/src/profile/p3_internal/ledger.rs`
- Test: `sovmem/src/profile/p3_internal/poisoning_matrix.rs`

**Consumes:** Tasks 2, 4, and 5 chain, prefix, and archived-policy interfaces.

**Produces:** Level 4 verification that checks intake before ledger replay and never depends on timestamp visibility or current metadata policy limits.

- [ ] **Step 1: Write Level 4 RED corruption matrix rows**

Add one test row per durable tamper: intake event deletion, insertion, reordering, duplicate ID, previous/event hash alteration, sealed pair content/meta hash mismatch, missing pair, extra pair, event-to-pair cross-binding mismatch, noncanonical line, rule-code disorder, backward time, high-water mismatch, and archived-policy lower-current-meta case. Each row must assert one stable code and no repair:

```rust
assert_eq!(verify_store_level4(&profile, &store).unwrap_err().code(), expected_code);
assert_eq!(durable_snapshot(&store), before);
```

Use `intake.chain_invalid`, `intake.record_invalid`, `intake.pair_binding`, `intake.clock_regression`, or `ledger.intake_binding` only as specified by the first invalid layer; do not assert generic `is_err()`.

- [ ] **Step 2: Run the Level 4 RED matrix**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::ledger::level_four_intake_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::poisoning_matrix::level4_intake_ -- --nocapture
```

Expected: rows fail because current Level 4 reconstructs policy inputs by mutable staging paths and timestamp comparison.

- [ ] **Step 3: Verify intake before ledger and replay only committed prefixes**

At the start of `verify_store_level4_with_allowance`, call `read_intake(&snapshot.paths)` after transient-state verification. For each verdict event:

```rust
let head = IntakeHead {
    sequence: event.intake_sequence,
    event_hash: event.intake_head_hash.clone(),
};
let prefix = intake_prefix(&intake_view, &head)
    .map_err(|_| level4_failure("ledger.intake_binding"))?;
let pair = read_sealed_pair_structural(&snapshot.paths, &event.proposal_id)
    .map_err(|_| level4_failure("ledger.proposal_binding"))?;
let policy = load_historical_policy(&snapshot.paths, &event.policy_hash)?;
verify_historical_policy(profile, &snapshot, &policy, &head, &pair, &prefix)?;
```

The verifier must compare the event pair's `STAGED` record to `proposal_meta_hash`, `candidate_hash`, `policy_hash`, and `profile_hash`; check the proposal is inside the prefix; and exclude only that proposal from its own policy counts. Events after the high-water are never materialized for that verdict. Replace all comparisons of `submitted_at <= occurred_at` and all `rejection_receipts` input with prefix order.

- [ ] **Step 4: Preserve Level 1-3 and transient fail-closed behavior**

Leave bundle verification unchanged. Add intake transaction journals and lease temporaries to `verify_preflight_transient_state`; a public Level 4 call must return `transaction.incomplete` while an intake seal is interrupted. Do not permit verifier repair or cleanup.

- [ ] **Step 5: Run GREEN and commit deterministic Level 4 replay**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::ledger::level_four_intake_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::poisoning_matrix::level4_intake_ -- --nocapture
git add sovmem/src/profile/verify.rs sovmem/src/profile/store.rs sovmem/src/profile/intake.rs sovmem/src/profile/p3_internal/ledger.rs sovmem/src/profile/p3_internal/poisoning_matrix.rs
git commit -m "P3.25: replay Level 4 from committed intake prefixes"
```

Expected: exact historical policy replay remains deterministic after later intake activity or a current-policy rotation.

### Task 7: Rebuild protected queue, proposal, verdict, and stats projections

**Files:**
- Modify: `sovmem/src/profile/proposal.rs`
- Modify: `sovmem/src/profile/stats.rs`
- Modify: `sovmem/src/cli/p3.rs`
- Modify: `sovmem/src/cli.rs`
- Test: `sovmem/tests/p3_cli.rs`
- Test: `sovmem/tests/p3_privacy.rs`

**Consumes:** verified intake/ledger views from Tasks 2 and 6.

**Produces:** queues/list/status/stats sourced solely from protected evidence and a public surface that distinguishes `submitted`, `staged`, and `rejected-at-intake` without leaking bodies.

- [ ] **Step 1: Write exact projection RED tests**

Create a fixture with one complete inbox submission, one sealed `STAGED`, one sealed `REJECTED`, and an intake-bound verdict. Assert ordering by intake sequence then IDs, exact status state, and queue alarm counts:

```rust
assert_eq!(list[0]["state"], "submitted");
assert_eq!(list[1]["state"], "staged");
assert_eq!(list[2]["state"], "rejected-at-intake");
assert_eq!(stats["queue"]["submitted"], 1);
assert_eq!(stats["queue"]["staged_open"], 1);
assert_eq!(stats["queue"]["rejected_at_intake"], 1);
assert_eq!(stats["queue"]["alarm"], true);
```

Run every projection with sentinel candidate/reason/evaluator/path values and assert they are absent from stdout and stderr unless existing explicit `--show-reasons`/`--show-advice` succeeds on an already verified verdict.

- [ ] **Step 2: Run the projection RED tests**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli proposal_list_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli store_stats_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_privacy queue_ -- --nocapture
```

Expected: named tests fail because views still read `staging/proposals` and receipt files.

- [ ] **Step 3: Project states from verified custody only**

Make `proposal_list`, `proposal_status`, `verdict_list`, and `store_stats` begin with Level 4 for sealed stores and `read_intake` for unsealed stores. Map complete inbox directories to `submitted`; map staged intake records to `staged` until their proposal has a terminal verdict; map rejection records to `rejected-at-intake`; derive `ruled`, `applied`, `superseded`, and `retained` from the verified ledger/canonical head. An incomplete inbox or intake transaction is an error, never a list row.

`stats` must expose only counts, hashes, state names, and alarm booleans. Preserve legacy P2 bundle stats bytes exactly when `--bundle` is used.

- [ ] **Step 4: Keep CLI exits and redaction stable**

Map intake errors to nonzero exit code 1 and usage/invalid arguments to 2. Emit exactly one static error object per failure. Do not add a `verdict` MCP path, noninteractive intake flag, a path-output field, or a candidate-body success field.

- [ ] **Step 5: Run GREEN and commit protected projections**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli proposal_list_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_cli store_stats_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_privacy queue_ -- --nocapture
git add sovmem/src/profile/proposal.rs sovmem/src/profile/stats.rs sovmem/src/cli/p3.rs sovmem/src/cli.rs sovmem/tests/p3_cli.rs sovmem/tests/p3_privacy.rs
git commit -m "P3.26: project queues from protected intake evidence"
```

Expected: queue alarm evidence is trustworthy and public output stays redacted.

### Task 8: Add intake crash recovery, locking, concurrency, and idempotency proofs

**Files:**
- Modify: `sovmem/src/profile/transaction.rs`
- Modify: `sovmem/src/profile/store.rs`
- Modify: `sovmem/src/profile/intake.rs`
- Modify: `sovmem/src/profile/p3_test_support.rs`
- Test: `sovmem/src/profile/p3_internal/fixture_concurrency.rs`
- Test: `sovmem/src/profile/p3_internal/poisoning_matrix.rs`

**Consumes:** Tasks 2-7 core mutation path.

**Produces:** a recoverable intake journal with deterministic lock order and no ambiguous partial authority state.

- [ ] **Step 1: Write fault-boundary and race RED cases**

Add injected faults at `AfterInboxRead`, `AfterPairSync`, `AfterIntakeJournalSync`, `BeforeIntakeEventRename`, `AfterIntakeEventRename`, and `BeforeInboxCleanup`. For each, assert exactly one of: no protected event/pair and retained inbox; durable event/pair and cleanup tail; or `transaction.incomplete` before public reads. Add concurrent seal/seal, seal/verdict, propose/seal, and recovery/seal tests that assert one succeeds or one receives `intake.locked`/`store.locked`, never two events for the same submission.

```rust
assert_eq!(event_ids(&store).len(), 1);
assert_eq!(proposal_ids(&store).len(), 1);
assert!(matches!(second, Err(error) if error.code() == "intake.locked"));
```

- [ ] **Step 2: Run the recovery/concurrency RED tests serially**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --jobs 1 profile::p3_internal::fixture_concurrency::intake_ -- --test-threads=1 --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --jobs 1 profile::p3_internal::poisoning_matrix::intake_crash_ -- --test-threads=1 --nocapture
```

Expected: tests fail because the v1 journal/recovery code has no authenticated intake transaction mode.

- [ ] **Step 3: Implement intake journals and recovery state**

Add `RecoveryMode::IntakeOnly` handling that validates `prepared.json`, `committed.json`, and `published.json` against selected inbox hashes, expected prior intake head, output pair hashes, and output event hash. The append/rename of the intake event is the intake commit point. Recovery may only complete a fully bound published event or remove a fully bound unpublished temporary; any mismatch returns `intake.incomplete` and leaves all durable evidence untouched.

Preserve the lock order: verdict obtains `verdict.lock`, then `intake.lock`; seal obtains only `intake.lock` and refuses if verdict/recovery state exists. Every release verifies its token before removal and syncs the parent while a durable lock state remains.

- [ ] **Step 4: Add idempotent seal behavior**

If a `submission_id` has already produced exactly one matching protected intake event, return its same `IntakeSealOutcome` with `idempotent: true`; do not append another line or create another pair. If inbox and journal disagree with the event, return `intake.incomplete`. Never use a retry timestamp to create a new event identity.

- [ ] **Step 5: Run GREEN and commit recovery proof**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml --jobs 1 profile::p3_internal::fixture_concurrency::intake_ -- --test-threads=1 --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --jobs 1 profile::p3_internal::poisoning_matrix::intake_crash_ -- --test-threads=1 --nocapture
git add sovmem/src/profile/transaction.rs sovmem/src/profile/store.rs sovmem/src/profile/intake.rs sovmem/src/profile/p3_test_support.rs sovmem/src/profile/p3_internal/fixture_concurrency.rs sovmem/src/profile/p3_internal/poisoning_matrix.rs
git commit -m "P3.27: recover sealed intake transactions safely"
```

Expected: every crash/race has one classifiable state and no unsafe retry can duplicate authority evidence.

### Task 9: Enforce capability boundaries and complete the intake poisoning matrix

**Files:**
- Modify: `sovmem/src/profile/p3_internal/capability.rs`
- Modify: `sovmem/src/profile/p3_internal/poisoning_matrix.rs`
- Modify: `sovmem/tests/p3_cli.rs`
- Modify: `sovmem/tests/p3_privacy.rs`

**Consumes:** completed protected paths and CLI from Tasks 1-8.

**Produces:** tested evidence that a proposer cannot modify protected intake authority and that every adversarial intake route emits an exact redacted error.

- [ ] **Step 1: Write the privilege/poisoning RED rows**

Replace the capability allowlist with `staging/inbox/<submission-id>/` only. Add child-process actions that attempt create/write/rename/remove/hard-link/symlink of `intake/events.jsonl`, `intake/proposals`, `intake/transactions`, `locks/intake.lock`, `ledger`, `canon`, `policies`, `evidence`, `derived`, `transactions`, and `policy.toml`. The restricted child must either be blocked by the real sandbox or report the existing approved unsupported permission model; same-user chmod is not sufficient.

Add matrix rows for forged source, evaluator, Judgment proposal, duplicate submission/proposal, duplicate `(target, candidate_hash)`, unsafe path, missing provenance, protected synthesis edit, size limits, malformed receipt-equivalent fields, quota breach, forged high-water, ledger tamper, and partial intake journal. Assert static code plus absent sentinels.

- [ ] **Step 2: Run the capability/poisoning RED tests**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::capability -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::poisoning_matrix::intake_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_privacy intake_ -- --nocapture
```

Expected: the tests fail because current capability scope still grants recursive staging writes and poisoning does not yet cover intake v2.

- [ ] **Step 3: Restrict the modeled proposer capability**

Update the real sandbox fixture and its child environment so the proposer can create input files and invoke `propose`, but has no write permission to every protected root. Verify child failures before invoking a human ceremony. Do not model protection as a process-local boolean, an environment claim, a filesystem mode check by the same user, or a test-only bypass.

- [ ] **Step 4: Make every red-team assertion exact and private**

For each matrix row, retain a before/after durable snapshot, inspect the machine-readable error, assert the exact code, and assert absence of candidate bytes, reason bytes, evaluator reason, source string, target path, OS username, temp directory, and safe-clone root. A test that merely calls `is_err()` is incomplete; convert it to exact-code coverage.

- [ ] **Step 5: Run GREEN and commit the authority regression suite**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::capability -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::poisoning_matrix::intake_ -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml --test p3_privacy intake_ -- --nocapture
git add sovmem/src/profile/p3_internal/capability.rs sovmem/src/profile/p3_internal/poisoning_matrix.rs sovmem/tests/p3_cli.rs sovmem/tests/p3_privacy.rs
git commit -m "P3.28: lock down intake authority fixtures"
```

Expected: no proposer capability can mutate protected evidence, and all attack diagnostics are stable and redacted.

### Task 10: Regenerate v2 fixtures and prove the complete P3 behavior

**Files:**
- Create: `sovmem/tests/fixtures/p3-cli-store-v2/one-applied/fixture.json`
- Create: `sovmem/tests/fixtures/p3-cli-store-v2/one-applied/store/**`
- Create: `sovmem/tests/fixtures/p3-cli-store-v2/two-applied/fixture.json`
- Create: `sovmem/tests/fixtures/p3-cli-store-v2/two-applied/store/**`
- Modify: `sovmem/src/profile/p3_internal/full_loop.rs`
- Modify: `sovmem/src/profile/p3_internal/ledger.rs`
- Modify: `sovmem/tests/p3_cli.rs`
- Test: `sovmem/src/profile/p3_internal/advisory_erasure.rs`

**Consumes:** all v2 mutation and verification behavior.

**Produces:** checked-in minimal v2 stores, a 20-mixed-verdict Level 4 proof, and an end-to-end propose → seal → human verdict → write → re-index → re-query proof.

- [ ] **Step 1: Write RED full-loop and mixed-chain assertions**

Update the full loop to invoke `propose`, a scripted human `intake seal`, and the human verdict. Assert an `ACCEPT` and a `REWRITE` apply exact approved raw bytes, reconstruct manifest/bundle hashes, produce a new index generation/receipt, and return the new retrieval marker at top five. Assert `REJECT`, `TP`, and `FP` leave canon/index hash maps byte-identical.

Extend the mixed-chain test to seal every proposal before its verdict, create at least 20 verdicts containing all five verbs, verify every intake prefix and the final ledger head, and assert the final level is 4.

- [ ] **Step 2: Run the full-loop RED tests**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::full_loop -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::ledger::twenty_mixed_verdict_chain -- --nocapture
```

Expected: tests fail until fixtures and helpers use sealed v2 pairs and v2 ledger payloads.

- [ ] **Step 3: Build fixtures only with the public v2 transaction path**

Create v2 fixtures by provisioning the P2 seed, using `propose`, sealing via the test human boundary, and applying verdicts through the supported transaction coordinator. Record only final canonical fixture files; do not copy an old v1 ledger or alter P2 source artifacts. Check every fixture with `verify --level 4` and verify its raw event/manifest hashes before adding it.

- [ ] **Step 4: Prove deterministic rebuild and advisory invariance**

Run the full loop twice with fixed clocks/IDs in separate v2 fixture roots and compare canonical manifest, intake, ledger, reindex receipt, and query result hashes. Run the same candidate with confidence absent/zero/one and favorable/adverse evaluator recommendations; hold human verdict constant and assert all policy/canon/ledger availability is identical except advisory commitments.

- [ ] **Step 5: Run GREEN and commit v2 behavioral proof**

Run:

```bash
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::full_loop -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::ledger::twenty_mixed_verdict_chain -- --nocapture
cargo test --locked --manifest-path sovmem/Cargo.toml profile::p3_internal::advisory_erasure -- --nocapture
git add sovmem/tests/fixtures/p3-cli-store-v2 sovmem/src/profile/p3_internal/full_loop.rs sovmem/src/profile/p3_internal/ledger.rs sovmem/src/profile/p3_internal/advisory_erasure.rs sovmem/tests/p3_cli.rs
git commit -m "P3.29: prove v2 write-back and mixed-chain behavior"
```

Expected: both applying verbs become queryable after full rebuild, nonapplying verbs do not mutate canon, and Level 4 verifies a 20-verdict mixed chain.

### Task 11: Execute the final P3 gates, safe-clone proof, and audit closure

**Files:**
- Modify: `scripts/lib/sovmem-build-audit.mjs`
- Modify: `test/sovmem-build-audit.test.js`
- Modify: `scripts/verify-sovmem-p3-safe-clone.mjs`
- Modify: `test/sovmem-p3-safe-clone.test.js`
- Modify: `docs/evidence/sovmem-p3-safe-clone-proof.json`
- Modify: `docs/sovmem-build-state.json`
- Modify: `docs/SOVMEM-BUILD-AUDIT.md`
- Modify: `docs/superpowers/specs/2026-07-21-sovmem-p3-intake-authority-amendment-design.md` only if an implementation fact contradicts its approved contract; otherwise leave it unchanged.

**Consumes:** a clean reviewed implementation head after Tasks 1-10.

**Produces:** verified P3 audit evidence that identifies the final implementation, safe-clone proof, and audit-only source commits separately.

- [ ] **Step 1: Add audit identity RED tests**

Change the audit test input to require all three nonempty full lowercase SHA fields whenever P3 is complete:

```js
assert.match(state.evidence.write_back.implementation_commit, /^[0-9a-f]{40}$/);
assert.match(state.evidence.write_back.safe_clone_proof_commit, /^[0-9a-f]{40}$/);
assert.match(state.evidence.write_back.audit_commit, /^[0-9a-f]{40}$/);
assert.notEqual(
  state.evidence.write_back.implementation_commit,
  state.evidence.write_back.audit_commit,
);
```

Also assert the audit refuses P3 `100%` if its source fingerprint, test counts, safe-clone report, or `verify --level 4` evidence is missing or names a different implementation SHA.

- [ ] **Step 2: Run audit RED**

Run:

```bash
node --test test/sovmem-build-audit.test.js
```

Expected: the updated test fails until the audit generator models distinct final identities and refreshed evidence.

- [ ] **Step 3: Implement v2 audit identity validation and commit it**

Change the audit-state schema to `sovmem-build-audit-v5` and write-back evidence schema to `sovmem-write-back-evidence-v2`. Require these three fields under `evidence.write_back`:

```js
assertKeys(writeBack, new Set([
  'schema_version', 'store_id', 'profile_version', 'implementation_commit',
  'safe_clone_proof_commit', 'audit_commit', 'level4_events', 'level4_verdicts',
  'level4_applied_generations', 'ledger_head', 'mixed_chain_events',
  'mixed_chain_verdicts', 'mixed_chain_applied_generations', 'mixed_chain_counts',
  'mixed_chain_ledger_head', 'full_loop_passed', 'deleted_index_rebuild_passed',
  'red_team_passed', 'privacy_passed', 'concurrency_passed', 'crash_recovery_passed',
  'advisory_erasure_passed', 'safe_clone_proof_path', 'safe_clone_proof_blake3',
  'frozen_p2_unchanged',
]), 'evidence.write_back');
```

Validate all three with `assertCommit`. Parse the redacted safe-clone proof, require its declared profile/store/version and proof source commit to equal the state fields, and require its raw BLAKE3 to equal `safe_clone_proof_blake3`. Do not hard-code a v1 or future v2 proof hash in the generator; state/proof cross-binding is the contract.

In the safe-clone verifier/test, require `audy-sovmem-writeback-v2`, `audy-ssa0-p3-v2`, a protected intake event/head, a verdict payload binding that same head, Level 4 success, and the unique retrieval marker. Its test fixture must use synthetic redacted proof data; it must not write an actual safe clone. Commit this source-only audit/proof verifier change:

```bash
git add scripts/lib/sovmem-build-audit.mjs test/sovmem-build-audit.test.js scripts/verify-sovmem-p3-safe-clone.mjs test/sovmem-p3-safe-clone.test.js
git commit -m "test: bind P3 audit evidence identities"
```

- [ ] **Step 4: Run the final source gates from the reviewed implementation head**

Run and retain complete logs outside Cargo target directories:

```bash
cargo fmt --check --manifest-path sovmem/Cargo.toml
cargo clippy --locked --manifest-path sovmem/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path sovmem/Cargo.toml --jobs 1 -- --test-threads=1
npm test
npm run audit:sovmem:check
```

Before each command, record `pwd -P`, branch, `git rev-parse HEAD`, exact command, test count, exit code, and report path. Stop on any failure; do not update audit state.

- [ ] **Step 5: Run the approved safe-clone ceremony**

Create a new safe clone from the approved read-only source, not Audy Vault or the frozen P2 corpus. Confirm its source and target paths are distinct; record source hashes before/after. Use the public CLI to propose a bounded new research document, `intake seal` it at a real TTY, apply a human `ACCEPT` with a nonempty reason at a real TTY, run `verify --level 4`, re-index via the verdict transaction, and query the unique retrieval marker. Record only redacted IDs/hashes/commands/results in `docs/evidence/sovmem-p3-safe-clone-proof.json`. Run:

```bash
CARGO_TARGET_DIR=<reused-task-target> node --test test/sovmem-p3-safe-clone.test.js
node scripts/verify-sovmem-p3-safe-clone.mjs <approved-exact-ceremony-arguments>
```

If a live canonical write is requested instead, stop for Khalil’s separate written authorization.

- [ ] **Step 6: Regenerate audit after every gate passes, review, and commit audit-only evidence**

The audit-generator/test source was committed in Step 3 and verified in Step 4. Its SHA is the value of `audit_commit`; it is the immutable audited input, not a self-referential SHA inside the generated report. Set `implementation_commit` to the committed Task 10-or-later source head actually tested and `safe_clone_proof_commit` to the exact source head used for the ceremony. Write these three already-recorded SHAs into `docs/sovmem-build-state.json`, regenerate the report, and commit only generated evidence/docs as a subsequent docs-only artifact. Run:

```bash
git rev-parse HEAD
npm run audit:sovmem
npm run audit:sovmem:check
node --test test/sovmem-build-audit.test.js
git add docs/sovmem-build-state.json docs/SOVMEM-BUILD-AUDIT.md
git commit -m "docs: close P3 intake-authority audit"
```

Then obtain a fresh whole-branch review with no Critical or Important finding. Only after that review, current complete gates, a clean scoped diff, live `origin/main` comparison, and explicit user authorization may the branch be pushed and a PR opened or merged. Do not claim branch protection, required checks, rulesets, or merge queues.

## Final Verification Checklist

- [ ] `git diff --check` passes and all P2 frozen artifacts remain byte-identical to their approved baseline.
- [ ] Every new focused test has recorded nonzero RED then GREEN evidence.
- [ ] `cargo fmt --check`, Clippy with `-D warnings`, serialized complete Rust suite, and complete JavaScript suite pass at the final implementation head.
- [ ] Level 4 validates the complete intake chain, complete verdict chain, each committed high-water prefix, all five verbs, and at least 20 mixed verdicts.
- [ ] The safe-clone propose → seal → human verdict → verified write → re-index → re-query ceremony passes without touching Audy Vault or frozen P2 evaluation inputs.
- [ ] Audit state reports P3 100% and approximately 63% overall only after all above evidence and a fresh whole-branch review.
