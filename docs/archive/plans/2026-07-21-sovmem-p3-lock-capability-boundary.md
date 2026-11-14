> Public-copy note: workstation paths are environment placeholders; the original private historical plan is retained in Nous. Set the named variables to your own locations before adapting any archived commands.

> **ARCHIVED** — This implementation plan has been superseded by [docs/sovmem-master-spec.md](../sovmem-master-spec.md). See [Appendix A: Implementation Plan Index](../sovmem-master-spec.md#appendix-a-implementation-plan-index) for a summary of its scope.
>
# SovMem P3 Lock Capability Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the abandoned descriptor-race experiment with the approved trusted-writer intake lease, restore the frozen dependency surface atomically with that replacement, and preserve fail-closed observable corruption and incomplete-state handling.

**Architecture:** The `locks/` root is protected by the Task 9 filesystem-capability boundary; untrusted proposer, agent, evaluator, MCP, and harness identities cannot mutate it. Task 2 therefore coordinates trusted P3 writers with standard-library no-replace creation, canonical owner bytes, and visible incomplete evidence. It never claims to defeat a process with the same unrestricted trusted OS mutation capability.

**Tech Stack:** Rust 1.84; existing `std`, `serde`, `serde_json`, `blake3`, and `thiserror`; no direct `rustix` dependency; existing P3 fixture support and Cargo test runner.

## Global Constraints

- Work only in `${P3_WORKTREE}` on `codex/sovmem-p3-verified-write-back-loop`; preserve the eight pre-existing dirty audit/runtime/Task 10 files.
- Parent authority is `docs/superpowers/specs/2026-07-14-sovmem-p3-verified-write-back-loop-design.md`; this task implements only `docs/superpowers/specs/2026-07-21-sovmem-p3-lock-capability-boundary-amendment.md`.
- `locks/` is outside every untrusted write root. An unrestricted process using the trusted local OS identity is explicitly out of scope; Task 9, not this task, proves the actual untrusted capability boundary.
- Remove the experimental direct `rustix = { version = "=1.1.4", features = ["fs"] }` line from `sovmem/Cargo.toml` and only the direct `"rustix 1.1.4",` entry from the `sovmem` dependency list in `sovmem/Cargo.lock`. Keep transitive Rustix package records unchanged.
- No new dependency, `unsafe`, async runtime, network call, process helper, CLI/MCP authority, policy/profile change, P2 corpus/golden/threshold/label/baseline change, canon write, live Vault write, audit regeneration, or source-vault synchronization.
- All owner, proposal, intake-event, manifest, policy, and ledger digests remain BLAKE3 over exact raw bytes. Diagnostics remain typed, static, JSONL, and redacted.
- Retain the internal verdict grammar `ACCEPT`, `REJECT`, `REWRITE`, `TP`, `FP`; this correction adds no public command or verdict behavior.
- A focused nonzero RED fixture must precede each new production behavior. Retained regression fixtures may already be green, but must run nonzero before and after the replacement and must never be reported as RED evidence.
- Use one task-owned `CARGO_TARGET_DIR` sequentially; preserve definitive output in the ignored report, then remove exactly that target after the task review package is created. Do not bulk-delete `/private/tmp`.
- Do not force-add `.superpowers/sdd/task-2-report.md`; it is ignored local evidence. Each commit stages only listed tracked files.

---

## File Structure

| File | Responsibility after this correction |
| --- | --- |
| `sovmem/Cargo.toml` | Restores the approved direct dependency surface: no Rustix direct edge. |
| `sovmem/Cargo.lock` | Removes only the direct SovMem-to-Rustix dependency entry; retains all transitive package records. |
| `sovmem/src/profile/intake.rs` | Trusted-writer lease acquisition, owner commitment verification, release ordering, pair/event promotion checks, and test failpoints. |
| `sovmem/src/profile/store.rs` | Lease-layout validation and byte-safe temporary-name parsing. |
| `sovmem/src/profile/p3_internal/proposal.rs` | Fixture-first lease, stale-capability, collision, incomplete, corruption, and successor-cleanup tests. |
| `sovmem/src/profile/p3_internal/store.rs` | Multibyte temporary-name fail-closed fixture. |
| `.superpowers/sdd/task-2-report.md` | Ignored commands, test counts, review result, and Task 9 handoff; never staged. |

## Shared Interfaces

Keep the crate-private interface consumed by later intake seal and verdict work:

```rust
pub(crate) struct IntakePublicationLease {
    root: PathBuf,
    owner_hash: String,
    token: String,
}

impl IntakePublicationLease {
    pub(crate) fn acquire(
        paths: &StorePaths,
        identity: IntakeLeaseIdentity,
    ) -> Result<Self, IntakeError>;

    pub(crate) fn verify(&mut self, paths: &StorePaths) -> Result<(), IntakeError>;

    pub(crate) fn release(self, paths: &StorePaths) -> Result<(), IntakeError>;
}

pub(crate) fn publish_sealed_pair(
    paths: &StorePaths,
    lease: &mut IntakePublicationLease,
    pair: &ProposalPair,
) -> Result<PathBuf, IntakeError>;

pub(crate) fn append_intake_event(
    paths: &StorePaths,
    lease: &mut IntakePublicationLease,
    event: &IntakeEvent,
) -> Result<IntakeView, IntakeError>;
```

`IntakePublicationLease` must not derive `Clone`. The `IntakeLeaseOwner` acquisition nonce remains a canonical 64-lower-hex value from `fresh_acquisition_nonce`; `lease_token` remains the BLAKE3 commitment over the validated identity and nonce. `verify` must check the real `locks/`, `intake.lock`, and `owner.json` path types through `symlink_metadata`, require exactly one regular `owner.json`, parse its canonical bytes, and map a missing, wrong-type, symlinked, malformed, mismatched, or incomplete observation to `intake.locked`.

### Task 1: Atomically restore the frozen dependency surface and rebuild the trusted-writer lease

**Files:**

- Modify: `sovmem/Cargo.toml`
- Modify: `sovmem/Cargo.lock`
- Modify: `sovmem/src/profile/intake.rs`
- Modify: `sovmem/src/profile/p3_internal/proposal.rs`
- Verify: `sovmem/src/profile/store.rs`
- Verify: `sovmem/src/profile/p3_internal/store.rs`
- Test: `sovmem/src/profile/p3_internal/proposal.rs`
- Test: `sovmem/src/profile/p3_internal/store.rs`

**Consumes:** The approved lock capability-boundary amendment and current Task 2 helpers `intake_paths`, `intake_lease_identity`, `intake_pair`, `sealed_pair_root`, `staged_intake_payload`, and `ProvisionedP3Fixture`.

**Produces:** A direct-dependency-free, cross-platform standard-library lease that serializes trusted writers, rejects observable unsafe evidence, preserves incomplete visible locks on failed acquisition/release, and retains correct pair/event/temporary cleanup behavior.

- [ ] **Step 1: Add exact RED fixtures without removing the current backend yet**

  Keep the existing descriptor backend compiled during this step, so the manifest is still a valid direct dependency and the intended fixture failures are observable. Add the following tests to `p3_internal/proposal.rs` and the listed test-only failpoint declaration to `intake.rs`; do not consume the new failpoint until Step 3.

  ```rust
  #[test]
  fn intake_lease_dependency_surface_is_frozen() {
      let root = Path::new(env!("CARGO_MANIFEST_DIR"));
      let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
      let lock = fs::read_to_string(root.join("Cargo.lock")).unwrap();
      let sovmem = lock.split("[[package]]
name = \"sovmem\"").nth(1).unwrap();

      assert!(!manifest.contains("rustix ="));
      assert!(sovmem
          .lines()
          .take_while(|line| !line.starts_with("[[package]]"))
          .all(|line| !line.contains("rustix")));
  }

  #[test]
  fn intake_lease_acquisition_is_no_replace_and_preserves_existing_owner_bytes() {
      let fixture = ProvisionedP3Fixture::empty("intake-lease-no-replace");
      let paths = intake_paths(&fixture);
      let first = intake_lease(&paths, "first-trusted-writer");
      let before = fs::read(paths.locks.join("intake.lock/owner.json")).unwrap();

      assert_eq!(
          IntakePublicationLease::acquire(&paths, intake_lease_identity("second-trusted-writer"))
              .unwrap_err()
              .code(),
          "intake.locked"
      );
      assert_eq!(fs::read(paths.locks.join("intake.lock/owner.json")).unwrap(), before);
      first.release(&paths).unwrap();
  }

  #[test]
  fn intake_lease_owner_publication_failure_leaves_visible_incomplete_evidence() {
      let fixture = ProvisionedP3Fixture::empty("intake-lease-owner-failure");
      let paths = intake_paths(&fixture);
      install_intake_lease_owner_publication_failure_for_test();

      assert_eq!(
          IntakePublicationLease::acquire(&paths, intake_lease_identity("trusted-writer"))
              .unwrap_err()
              .code(),
          "intake.write"
      );
      assert!(paths.locks.join("intake.lock").is_dir());
      assert_eq!(
          inspect_store(&fixture.profile(), &fixture.store_root).unwrap_err().code(),
          "transaction.incomplete"
      );
  }

  #[test]
  fn intake_lease_release_failure_leaves_visible_incomplete_evidence() {
      let fixture = ProvisionedP3Fixture::empty("intake-lease-release-incomplete");
      let paths = intake_paths(&fixture);
      let lease = intake_lease(&paths, "trusted-writer");
      fs::write(paths.locks.join("intake.lock/extra"), b"fixture
").unwrap();

      assert_eq!(lease.release(&paths).unwrap_err().code(), "intake.write");
      assert!(paths.locks.join("intake.lock").is_dir());
      assert_eq!(
          inspect_store(&fixture.profile(), &fixture.store_root).unwrap_err().code(),
          "transaction.incomplete"
      );
  }
  ```

  ```rust
  #[cfg(test)]
  std::thread_local! {
      static FAIL_INTAKE_LEASE_OWNER_PUBLICATION: Cell<bool> = const { Cell::new(false) };
  }

  #[cfg(test)]
  pub(crate) fn install_intake_lease_owner_publication_failure_for_test() {
      FAIL_INTAKE_LEASE_OWNER_PUBLICATION.with(|fail| {
          assert!(!fail.replace(true), "owner publication failure is already installed");
      });
  }
  ```

  Add this Unix-only corruption fixture (the implementation must remain portable; only the symlink fixture is Unix-specific):

  ```rust
  #[cfg(unix)]
  #[test]
  fn intake_lease_rejects_unsafe_observed_owner_without_publication() {
      let fixture = ProvisionedP3Fixture::empty("intake-unsafe-owner");
      let paths = intake_paths(&fixture);
      let mut lease = intake_lease(&paths, "trusted-writer");
      let pair = intake_pair(&fixture, "src:agent:unsafe", "2026-07-21T06:00:00.000000000Z", "unsafe");
      let owner = paths.locks.join("intake.lock/owner.json");
      fs::remove_file(&owner).unwrap();
      std::os::unix::fs::symlink("missing-owner.json", &owner).unwrap();

      assert_eq!(publish_sealed_pair(&paths, &mut lease, &pair).unwrap_err().code(), "intake.locked");
      assert!(!sealed_pair_root(&paths, &pair.meta.proposal_id).exists());
  }
  ```

  Retain the existing stale in-process lease capability, duplicate publisher, pair/event collision, promotion recheck, and pre-rename successor-cleanup fixtures. Keep the existing `p3_v2_unicode_intake_pair_temporary_fails_closed_without_panicking` fixture in `p3_internal/store.rs`. Delete only the descriptor-specific test imports/hooks/FIFO/oversize and same-identity rename-race fixtures in Step 3, after their replacements have been added.

- [ ] **Step 2: Run and record the exact RED evidence**

  Run:

  ```bash
  CARGO_TARGET_DIR=/private/tmp/sovmem-p3-lock-boundary-task2 \
    cargo test --locked --manifest-path sovmem/Cargo.toml \
    profile::p3_internal::proposal::intake_lease_dependency_surface_is_frozen \
    -- --exact --nocapture --test-threads=1
  CARGO_TARGET_DIR=/private/tmp/sovmem-p3-lock-boundary-task2 \
    cargo test --locked --manifest-path sovmem/Cargo.toml \
    profile::p3_internal::proposal::intake_lease_owner_publication_failure_leaves_visible_incomplete_evidence \
    -- --exact --nocapture --test-threads=1
  CARGO_TARGET_DIR=/private/tmp/sovmem-p3-lock-boundary-task2 \
    cargo test --locked --manifest-path sovmem/Cargo.toml \
    profile::p3_internal::proposal::intake_lease_acquisition_is_no_replace_and_preserves_existing_owner_bytes \
    -- --exact --nocapture --test-threads=1
  ```

  Expected: the dependency fixture runs once and fails because `Cargo.toml` still directly lists Rustix; the owner-publication fixture runs once and fails because the new failpoint has not yet been consumed; neither failure may be a compile error or a zero-test filter. The no-replace fixture is a retained regression that may already pass; record it as non-RED coverage. Append each command, count, and failure reason to `.superpowers/sdd/task-2-report.md`.

- [ ] **Step 3: Replace the descriptor experiment and remove its dependency in one atomic implementation**

  In `intake.rs`, remove all Rustix imports, Unix/Redox backend branches, `OwnedFd`, descriptor identity/open/read/write helpers, `openat`/`mkdirat`/`unlinkat` code, descriptor test hooks, the unsupported-platform test bridge, and the descriptor-only tests that depend on them. `IntakePublicationLease::{acquire,verify,release}` must call the standard implementation on every supported Rust target.

  Use `fs::create_dir` as the no-replace lease acquisition. It maps `AlreadyExists` to `IntakeError::Locked` without touching the existing owner bytes. Immediately validate the non-symlink `locks/` directory, create `intake.lock`, construct `IntakeLeaseOwner::from_identity(identity, fresh_acquisition_nonce()?)`, and write canonical owner bytes with `write_new_synced`. Consume the test failpoint immediately before the owner write:

  ```rust
  fn acquire_intake_lease(
      paths: &StorePaths,
      identity: IntakeLeaseIdentity,
  ) -> Result<IntakePublicationLease, IntakeError> {
      regular_lease_directory(&paths.locks)?;
      let lock = paths.locks.join("intake.lock");
      match fs::create_dir(&lock) {
          Ok(()) => {}
          Err(error) if error.kind() == ErrorKind::AlreadyExists => return Err(IntakeError::Locked),
          Err(_) => return Err(IntakeError::Write),
      }
      let owner = IntakeLeaseOwner::from_identity(identity, fresh_acquisition_nonce()?)?;
      let owner_bytes = owner.canonical_bytes()?;
      #[cfg(test)]
      if FAIL_INTAKE_LEASE_OWNER_PUBLICATION.with(Cell::take) {
          return Err(IntakeError::Write);
      }
      write_new_synced(&lock.join("owner.json"), &owner_bytes).map_err(|_| IntakeError::Write)?;
      sync_directory(&lock).map_err(|_| IntakeError::Write)?;
      sync_directory(&paths.locks).map_err(|_| IntakeError::Write)?;
      let lease = IntakePublicationLease {
          root: paths.root.clone(),
          owner_hash: raw_hash(&owner_bytes),
          token: owner.lease_token,
      };
      lease.verify(paths)?;
      Ok(lease)
  }
  ```

  Do not clean up a directory created by a failed owner publication. It is deliberate visible incomplete evidence. Implement the path/type and ownership checks exactly as follows:

  ```rust
  fn regular_lease_directory(path: &Path) -> Result<(), IntakeError> {
      let metadata = fs::symlink_metadata(path).map_err(|_| IntakeError::Locked)?;
      if metadata.file_type().is_symlink() || !metadata.is_dir() {
          return Err(IntakeError::Locked);
      }
      Ok(())
  }

  fn verified_lease_owner_bytes(paths: &StorePaths) -> Result<Vec<u8>, IntakeError> {
      regular_lease_directory(&paths.locks)?;
      let lock = paths.locks.join("intake.lock");
      regular_lease_directory(&lock)?;
      let mut entries = fs::read_dir(&lock)
          .map_err(|_| IntakeError::Locked)?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|_| IntakeError::Locked)?;
      if entries.len() != 1 || entries[0].file_name().to_str() != Some("owner.json") {
          return Err(IntakeError::Locked);
      }
      let owner = entries.pop().unwrap().path();
      let metadata = fs::symlink_metadata(&owner).map_err(|_| IntakeError::Locked)?;
      if metadata.file_type().is_symlink() || !metadata.is_file() {
          return Err(IntakeError::Locked);
      }
      let bytes = fs::read(owner).map_err(|_| IntakeError::Locked)?;
      IntakeLeaseOwner::parse(&bytes)?;
      Ok(bytes)
  }

  fn verify_intake_lease(
      lease: &IntakePublicationLease,
      paths: &StorePaths,
  ) -> Result<(), IntakeError> {
      if lease.root != paths.root {
          return Err(IntakeError::Locked);
      }
      let owner_bytes = verified_lease_owner_bytes(paths)?;
      let owner = IntakeLeaseOwner::parse(&owner_bytes)?;
      if raw_hash(&owner_bytes) != lease.owner_hash || owner.lease_token != lease.token {
          return Err(IntakeError::Locked);
      }
      Ok(())
  }
  ```

  A changed owner, stale capability, malformed bytes, extra entry, missing entry, or type mismatch must return `intake.locked`.

  Release must first call `verify`, then sync `paths.locks` while `intake.lock` remains visible. Remove `owner.json`; on failure, retain the owner evidence and return `intake.write`. Finally call `fs::remove_dir(&lock)` as the terminal liveness-only action. Do not perform a later fallible sync; a final removal failure leaves the empty lock directory visible and returns `intake.write`:

  ```rust
  fn release_intake_lease(
      lease: &IntakePublicationLease,
      paths: &StorePaths,
  ) -> Result<(), IntakeError> {
      verify_intake_lease(lease, paths)?;
      let lock = paths.locks.join("intake.lock");
      sync_directory(&paths.locks).map_err(|_| IntakeError::Write)?;
      fs::remove_file(lock.join("owner.json")).map_err(|_| IntakeError::Write)?;
      fs::remove_dir(lock).map_err(|_| IntakeError::Write)
  }
  ```

  Retain `&mut IntakePublicationLease` in both publishers. `publish_sealed_pair` and `append_intake_event` verify before work and immediately before their replace-capable rename. Preserve pair/event collision code `intake.record_invalid`. Keep the pre-rename event parent sync inside its cleanup-protected closure, so the existing injected failure leaves the prior event head byte-identical and removes `.events-<sequence>`.

  Preserve byte-safe temporary parsing in `store.rs`:

  ```rust
  fn proposal_temporary_tail(name: &str) -> Option<&str> {
      let value = name.strip_prefix(".proposal-")?;
      let proposal_id = value.get(..68)?;
      if !valid_proposal_id(proposal_id) {
          return None;
      }
      value.get(68..)?.strip_prefix('-')
  }
  ```

  `store.rs` already implements the byte-safe `proposal_temporary_tail` form above and its public transient scan makes any remaining entry under `locks/` return `transaction.incomplete`; verify those existing protections rather than rewriting them. In the same coherent change, remove exactly the direct Rustix manifest line and the direct `"rustix 1.1.4",` entry under `[[package]] name = "sovmem"` in `Cargo.lock`. Do not regenerate the lockfile or remove the transitive Rustix package records.

- [ ] **Step 4: Prove GREEN, retained invariants, and serialized regression behavior**

  Rerun the three exact commands from Step 2, then run:

  ```bash
  CARGO_TARGET_DIR=/private/tmp/sovmem-p3-lock-boundary-task2 \
    cargo test --locked --manifest-path sovmem/Cargo.toml \
    profile::p3_internal::proposal::intake_lease_release_failure_leaves_visible_incomplete_evidence \
    -- --exact --nocapture --test-threads=1
  CARGO_TARGET_DIR=/private/tmp/sovmem-p3-lock-boundary-task2 \
    cargo test --locked --manifest-path sovmem/Cargo.toml \
    profile::p3_internal::proposal::intake_event_pre_rename_directory_sync_failure_removes_successor \
    -- --exact --nocapture --test-threads=1
  CARGO_TARGET_DIR=/private/tmp/sovmem-p3-lock-boundary-task2 \
    cargo test --locked --manifest-path sovmem/Cargo.toml \
    profile::p3_internal::store::p3_v2_unicode_intake_pair_temporary_fails_closed_without_panicking \
    -- --exact --nocapture --test-threads=1
  CARGO_TARGET_DIR=/private/tmp/sovmem-p3-lock-boundary-task2 \
    cargo test --locked --manifest-path sovmem/Cargo.toml --lib \
    profile::p3_internal::proposal::intake_ -- --nocapture --test-threads=1
  CARGO_TARGET_DIR=/private/tmp/sovmem-p3-lock-boundary-task2 \
    cargo test --locked --manifest-path sovmem/Cargo.toml --lib \
    profile::p3_internal::store::p3_v2_intake_ -- --nocapture --test-threads=1
  CARGO_TARGET_DIR=/private/tmp/sovmem-p3-lock-boundary-task2 \
    cargo clippy --locked --manifest-path sovmem/Cargo.toml --lib -- -D warnings
  cargo fmt --manifest-path sovmem/Cargo.toml --check
  git diff --check
  ```

  Expected: every named test executes once and passes; both serialized suites have nonzero passing counts and zero failures; clippy, formatting, and diff checks pass. Confirm the manifest/lock diff contains only the direct Rustix removal. Record counts and exact output paths in the ignored report.

- [ ] **Step 5: Commit the coherent task**

  ```bash
  git add sovmem/Cargo.toml sovmem/Cargo.lock sovmem/src/profile/intake.rs \
    sovmem/src/profile/p3_internal/proposal.rs
  git diff --cached --check
  git diff --cached --name-only
  git commit -m "P3.21: restore trusted intake lease boundary"
  ```

  Expected: the commit contains only the four listed tracked files. The ignored report and the eight pre-existing dirty files remain unstaged.

### Task 2: Preserve the Task 9 capability handoff and obtain fresh task-level review

**Files:**

- Modify: `.superpowers/sdd/task-2-report.md` (ignored local evidence only)
- Verify: `docs/superpowers/specs/2026-07-21-sovmem-p3-lock-capability-boundary-amendment.md`
- Verify: `docs/superpowers/plans/2026-07-21-sovmem-p3-lock-capability-boundary.md`
- Verify: the four Task 1 tracked files plus the retained `store.rs` fixtures

**Consumes:** The coherent Task 1 commit.

**Produces:** A reviewed boundary correction with a concrete Task 9 filesystem-capability proof contract, without changing audit state, publication state, or P3 completion status.

- [ ] **Step 1: Record the exact Task 9 handoff in ignored evidence**

  Append this paragraph verbatim to `.superpowers/sdd/task-2-report.md`:

  ```markdown
  Task 9 capability handoff: Task 2 coordinates trusted P3 writers only. The Task 9 privilege suite must prove that proposer, agent, evaluator, MCP, and untrusted-harness identities cannot create, rename, remove, or replace `locks/` entries; may write only granted untrusted inbox/staging inputs; and receive stable redacted diagnostics without a partial intake, canonical, manifest, index, or ledger publication. A same unrestricted trusted OS identity is outside this P3 threat model and is not represented as an untrusted fixture.
  ```

- [ ] **Step 2: Run exact scope and frozen-contract checks**

  Run:

  ```bash
  git diff --check 16e8ca6..HEAD
  git diff --name-only 16e8ca6..HEAD
  git diff 16e8ca6..HEAD -- sovmem/Cargo.toml sovmem/Cargo.lock
  rg -n 'rustix|tokio|async-std|reqwest|ureq|curl|Command::new|unsafe[[:space:]]*(\{|fn|impl|extern)' \
    sovmem/src/profile/intake.rs sovmem/src/profile/p3_internal/proposal.rs
  npm run audit:sovmem:check
  ```

  Expected: Cargo changes only remove the direct Rustix line/edge; the source scan has no match (the fixture name `unsafe_observed_owner` is allowed because it is not the Rust keyword); and audit output is recorded as a non-closure check if it is blocked by the eight pre-existing Task 10/audit edits. Do not edit frozen evidence to force it green.

- [ ] **Step 3: Build a review package and request independent review**

  Run:

  ```bash
  "${REVIEW_PACKAGE_SCRIPT}" \
    16e8ca6 HEAD
  ```

  Give the reviewer the amendment, this plan, the ignored Task 2 report, and the emitted package path. Require the reviewer to verify: no source claims same-identity hostile-rename resistance; no direct Rustix dependency remains; collisions, stale capabilities, unsafe observed lock evidence, and incomplete state fail closed; pair/event/Unicode protections persist; and Task 9—not Task 2—owns untrusted filesystem capability proof.

- [ ] **Step 4: Record review result and close only the reviewed task**

  If review is clean, append the review conclusion and exact commands/counts to the ignored report. Do not create a documentation-only commit. Then preserve the report and remove only the task-owned target after confirming that no process holds it:

  ```bash
  if lsof /private/tmp/sovmem-p3-lock-boundary-task2; then exit 1; fi
  rm -rf -- /private/tmp/sovmem-p3-lock-boundary-task2
  test ! -e /private/tmp/sovmem-p3-lock-boundary-task2
  ```

  If review finds a Critical or Important issue, add one focused RED fixture, correct one invariant, rerun its focused and related serialized checks, create a new review package, and obtain re-review before task closure. Do not advance Task 10, safe-clone proof, audit regeneration, P3 completion, publication, or merge.

## Plan Self-Review

- The approved authority boundary, non-scope, standard-library replacement, direct dependency restoration, no-replace collision behavior, owner commitment, incomplete-state retention, typed corruption rejection, raw-byte preservation, no P2/CLI/policy expansion, and Task 9 handoff are each mapped to a concrete task and command above.
- Dependency removal now occurs atomically with removal of its only source user, preventing an intermediate non-compiling commit. The test-only owner-publication failure provides the actual nonzero RED condition; retained collision/stale/corruption/event/Unicode fixtures are explicitly not mislabeled as RED.
- Interfaces and errors match the existing crate names: `IntakePublicationLease`, `IntakeLeaseOwner`, `IntakeError::{Locked,Write}`, `StorePaths`, `publish_sealed_pair`, `append_intake_event`, `write_new_synced`, `sync_directory`, and `raw_hash`.
- The plan contains no deferred implementation marker, arbitrary same-identity adversary claim, partial-write cleanup that hides incomplete evidence, nondeterministic production test ID requirement, sensitive-body diagnostic, or P4/P5/Task 10 scope creep.
