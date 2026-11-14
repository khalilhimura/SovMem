use sovmem::profile::p3_test_support::{
    DenyOwnerRead, DenyTerminalParentSync, FailJournalFilesystem, FaultPlan, FixedClock,
    JournalFilesystemFailure, P3TestFixture, ProvisionedP3Fixture, RecoveryLeasePublicationBarrier,
    RejectProtectedWrites, RejectRename, ScriptedHuman, TerminalParentSync,
};
use sovmem::profile::{
    canonical_json_bytes, canonical_owner_bytes, discover_original_journal,
    discover_original_journal_ignoring, idempotent_terminal_replay, lease_token,
    map_judgment_error, map_policy_archive_error, original_journal_commitment,
    preflight_human_verdict_from_store_with_human, preflight_human_verdict_with_human,
    read_bounded_confirmation, read_canonical_owner, read_preflight_state,
    recover_store_with_human, recover_store_with_tty_path, recovery_owner_hashes,
    recovery_transaction_id, render_decision_display, require_exact_confirmation,
    seal_intake_submission_with_human, seal_intake_submission_with_human_and_faults,
    sorted_directory_entries, transaction_id, valid_receipt_id, validate_generation_against_plan,
    validate_plan_source_pair, validate_recovery_prefix, DecisionDisplayInput, FaultPoint,
    FilesystemOps, HumanPresence, HumanVerdictRequest, JournalMarker, LeaseKind, PreflightState,
    PreflightVerdict, PreparedArtifacts, PreparedJournal, RecoveryCleanupPlan, RecoveryJournal,
    RecoveryJournalRecord, RecoveryLeaseBinding, RecoveryLeaseKind, RecoveryMode, RecoveryRequest,
    StdFilesystem, TransactionError, TransactionIdInput, TransactionPlan, TransactionRuntime,
    TtyHumanPresence, VerdictLeaseReceipt, JOURNAL_SCHEMA, RECOVERY_CLEANUP_PLAN_SCHEMA,
    RECOVERY_JOURNAL_SCHEMA, TRANSACTION_PLAN_SCHEMA,
};
use sovmem::profile::{
    inspect_store, load_p3_profile, query_p3_generation, read_ledger, read_sealed_pair_structural,
    retained_pairs, submit_proposal, submit_proposal_with_clock, verify_generation,
    verify_store_level4, CanonicalHead, CanonicalResult, DecisionMaterial, EvaluatorBlock,
    ExpansionMode, IntakeLeaseIdentity, IntakePublicationLease, JudgmentError, LedgerEvent,
    LoadedPolicy, P3Profile, Policy, PolicyDecision, PolicyError, ProposalBase, ProposalIntent,
    ProposalMeta, ProposalOperation, ProposalPair, ProposalSubmission, Reason, RewriteBytes,
    SeedEvidence, StorePaths, StoreSnapshot, VerdictPayload, VerdictVerb, VerificationLevel,
};
use std::cell::Cell;
use std::fs::{self, OpenOptions};
use std::io::{BufReader, Cursor};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

fn repository_path(relative: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative)
}

fn p3_test_candidate_bytes(path: &Path) -> Vec<u8> {
    String::from_utf8(fs::read(path).expect("read frozen P2 candidate"))
        .expect("frozen P2 candidate is UTF-8")
        .replace(
            "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog",
            "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
        )
        .into_bytes()
}

fn acquire_stale_trusted_intake_lease(
    fixture: &ProvisionedP3Fixture,
    owner_id: &str,
    command_class: &str,
) -> (StorePaths, IntakePublicationLease) {
    let paths = StorePaths::from_root(fs::canonicalize(&fixture.store_root).unwrap());
    let started_at = sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z")
        .expect("trusted intake fixture timestamp");
    let identity = IntakeLeaseIdentity::new(owner_id, command_class, &started_at, 4242)
        .expect("trusted intake fixture identity");
    let lease = IntakePublicationLease::acquire(&paths, identity)
        .expect("create stale trusted intake capability");
    (paths, lease)
}

fn acquire_trusted_intake_lease(
    fixture: &ProvisionedP3Fixture,
    owner_id: &str,
    command_class: &str,
) -> IntakePublicationLease {
    acquire_stale_trusted_intake_lease(fixture, owner_id, command_class).1
}

struct BlockingSealHuman {
    confirmation: String,
    gate: Arc<(Mutex<(bool, bool)>, Condvar)>,
}

impl HumanPresence for BlockingSealHuman {
    fn show(&mut self, _bytes: &[u8]) -> Result<(), TransactionError> {
        let (state, changed) = &*self.gate;
        let mut state = state.lock().unwrap();
        state.0 = true;
        changed.notify_all();
        while !state.1 {
            state = changed.wait(state).unwrap();
        }
        Ok(())
    }

    fn read_confirmation(&mut self) -> Result<String, TransactionError> {
        Ok(self.confirmation.clone())
    }
}

fn wait_until_seal_is_prompting(gate: &Arc<(Mutex<(bool, bool)>, Condvar)>) {
    let (state, changed) = &**gate;
    let mut state = state.lock().unwrap();
    while !state.0 {
        let (next, timeout) = changed
            .wait_timeout(state, Duration::from_secs(5))
            .expect("wait for the sealing fixture gate");
        state = next;
        assert!(
            !timeout.timed_out(),
            "sealed submission never reached human confirmation"
        );
    }
}

fn release_blocked_seal(gate: &Arc<(Mutex<(bool, bool)>, Condvar)>) {
    let (state, changed) = &**gate;
    let mut state = state.lock().unwrap();
    state.1 = true;
    changed.notify_all();
}

#[cfg(unix)]
#[test]
fn terminal_display_does_not_require_a_filesystem_sync() {
    let tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")
        .expect("open terminal-like character device");
    let mut human = TtyHumanPresence { tty };

    assert!(human.show(b"preview\n").is_ok());
}

struct DenyTransactionPlanTemporaryWrite;

impl FilesystemOps for DenyTransactionPlanTemporaryWrite {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        StdFilesystem.read(path)
    }

    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".plan-"))
            && path.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new("transactions"))
        {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

#[test]
fn canonical_plan_temporary_write_failure_has_a_stable_redacted_code() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("plan-write-diagnostic-code");
    let runtime = fixture.runtime_with_ops(DenyTransactionPlanTemporaryWrite);

    let error = fixture
        .apply_with_runtime(VerdictVerb::Accept, "plan write diagnostic", &runtime)
        .expect_err("a denied transaction plan temporary must fail");

    assert_eq!(error.code(), "transaction.plan_prepare_failed");
}

#[test]
fn canonical_receipt_tampering_has_a_stable_redacted_code() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("receipt-diagnostic-code");
    let runtime = fixture.runtime();
    let request = fixture
        .request_for(VerdictVerb::Accept, "receipt diagnostic")
        .expect("construct verdict request");
    let initial = read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
        .expect("read fixture preflight state");
    let mut human = ScriptedHuman::new(Some(format!(
        "APPLY ACCEPT {}\n",
        &fixture.proposal_id[..12]
    )));
    let mut decision = match preflight_human_verdict_with_human(
        initial,
        request,
        &runtime,
        &mut human,
        &FixedClock(
            sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z")
                .expect("parse fixture timestamp"),
        ),
    )
    .expect("confirm fixture preflight")
    {
        PreflightVerdict::Confirmed(decision) => decision,
        PreflightVerdict::Idempotent(_) => panic!("fixture cannot replay a verdict"),
    };
    decision.approved_target_bytes = None;

    let error = sovmem::profile::apply_human_verdict(decision, &runtime)
        .expect_err("tampered canonical receipt must fail");

    assert_eq!(error.code(), "transaction.canonical_receipt_invalid");
}

#[test]
fn canonical_ledger_head_tampering_has_a_stable_redacted_code() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("ledger-head-diagnostic-code");
    let runtime = fixture.runtime();
    let request = fixture
        .request_for(VerdictVerb::Accept, "ledger head diagnostic")
        .expect("construct verdict request");
    let initial = read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
        .expect("read fixture preflight state");
    let mut human = ScriptedHuman::new(Some(format!(
        "APPLY ACCEPT {}\n",
        &fixture.proposal_id[..12]
    )));
    let mut decision = match preflight_human_verdict_with_human(
        initial,
        request,
        &runtime,
        &mut human,
        &FixedClock(
            sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z")
                .expect("parse fixture timestamp"),
        ),
    )
    .expect("confirm fixture preflight")
    {
        PreflightVerdict::Confirmed(decision) => decision,
        PreflightVerdict::Idempotent(_) => panic!("fixture cannot replay a verdict"),
    };
    decision.observed_ledger_head = Some("a".repeat(64));

    let error = sovmem::profile::apply_human_verdict(decision, &runtime)
        .expect_err("tampered ledger head must fail");

    assert_eq!(error.code(), "transaction.canonical_ledger_state_changed");
}

fn with_test_recovery_id(mut record: RecoveryJournalRecord) -> RecoveryJournalRecord {
    let recovered_at = sovmem::profile::Timestamp::parse(&record.recovered_at).unwrap();
    record.cleanup_plan_hash = record.cleanup_plan.hash().unwrap();
    record.recovery_id = recovery_transaction_id(
        &recovery_owner_hashes(&record.quarantined_leases),
        &recovered_at,
        record.observed_ledger_head.as_deref(),
        &record.command_class,
        record.original_journal_hash.as_deref(),
        &record.profile_hash,
        &record.policy_hash,
        &record.cleanup_plan_hash,
    );
    record
}

fn different_test_hash(value: &str) -> String {
    let mut different = value.to_owned();
    let replacement = if value.starts_with('0') { "1" } else { "0" };
    different.replace_range(0..1, replacement);
    different
}

struct SwapPlanAfterCommitmentRead {
    plan_path: std::path::PathBuf,
    replacement: Vec<u8>,
    reads: Cell<usize>,
}

impl SwapPlanAfterCommitmentRead {
    fn new(plan_path: std::path::PathBuf, replacement: Vec<u8>) -> Self {
        Self {
            plan_path,
            replacement,
            reads: Cell::new(0),
        }
    }
}

impl FilesystemOps for SwapPlanAfterCommitmentRead {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        let bytes = StdFilesystem.read(path)?;
        if path == self.plan_path {
            let reads = self.reads.get() + 1;
            self.reads.set(reads);
            if reads == 2 {
                fs::write(path, &self.replacement)?;
            }
        }
        Ok(bytes)
    }

    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

struct SwapPlanBeforePrepRemoval {
    final_plan: std::path::PathBuf,
    recovery_owner: std::path::PathBuf,
    armed: Cell<bool>,
    swapped: Cell<bool>,
}

impl SwapPlanBeforePrepRemoval {
    fn new(final_plan: std::path::PathBuf, recovery_owner: std::path::PathBuf) -> Self {
        Self {
            final_plan: fs::canonicalize(&final_plan).unwrap_or(final_plan),
            recovery_owner: fs::canonicalize(&recovery_owner).unwrap_or(recovery_owner),
            armed: Cell::new(false),
            swapped: Cell::new(false),
        }
    }
}

impl FilesystemOps for SwapPlanBeforePrepRemoval {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        let bytes = StdFilesystem.read(path)?;
        if path == self.recovery_owner && self.armed.get() && !self.swapped.replace(true) {
            let plan = fs::read(&self.final_plan)?;
            fs::remove_file(&self.final_plan)?;
            fs::write(&self.final_plan, plan)?;
        }
        Ok(bytes)
    }

    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        let metadata = StdFilesystem.metadata(path)?;
        if path == self.final_plan {
            if let Ok(bytes) = fs::read(&self.recovery_owner) {
                if serde_json::from_slice::<sovmem::profile::LeaseOwner>(&bytes)
                    .is_ok_and(|owner| owner.command_class == "recovery")
                {
                    self.armed.set(true);
                }
            }
        }
        Ok(metadata)
    }
}

struct MutatePathOnSecondMetadata {
    path: std::path::PathBuf,
    metadata_reads: Cell<usize>,
    mutation: Box<dyn Fn() -> std::io::Result<()>>,
}

impl MutatePathOnSecondMetadata {
    fn new(path: std::path::PathBuf, mutation: impl Fn() -> std::io::Result<()> + 'static) -> Self {
        Self {
            path,
            metadata_reads: Cell::new(0),
            mutation: Box::new(mutation),
        }
    }
}

impl FilesystemOps for MutatePathOnSecondMetadata {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        StdFilesystem.read(path)
    }

    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        let metadata = StdFilesystem.metadata(path)?;
        if path == self.path {
            let reads = self.metadata_reads.get() + 1;
            self.metadata_reads.set(reads);
            if reads == 2 {
                (self.mutation)()?;
            }
        }
        Ok(metadata)
    }
}

struct MutatePathOnNthMetadata {
    path: std::path::PathBuf,
    metadata_reads: Cell<usize>,
    trigger: usize,
    mutation: Box<dyn Fn() -> std::io::Result<()>>,
}

impl MutatePathOnNthMetadata {
    fn new(
        path: std::path::PathBuf,
        trigger: usize,
        mutation: impl Fn() -> std::io::Result<()> + 'static,
    ) -> Self {
        Self {
            path,
            metadata_reads: Cell::new(0),
            trigger,
            mutation: Box::new(mutation),
        }
    }
}

impl FilesystemOps for MutatePathOnNthMetadata {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        StdFilesystem.read(path)
    }
    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }
    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }
    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }
    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        let metadata = StdFilesystem.metadata(path)?;
        if path == self.path {
            let reads = self.metadata_reads.get() + 1;
            self.metadata_reads.set(reads);
            if reads == self.trigger {
                (self.mutation)()?;
            }
        }
        Ok(metadata)
    }
}

struct InsertFileAfterMatchingRead {
    trigger: std::path::PathBuf,
    inserted: std::path::PathBuf,
    occurrence: usize,
    reads: Cell<usize>,
}

impl InsertFileAfterMatchingRead {
    fn new(trigger: std::path::PathBuf, inserted: std::path::PathBuf, occurrence: usize) -> Self {
        Self {
            trigger,
            inserted,
            occurrence,
            reads: Cell::new(0),
        }
    }
}

impl FilesystemOps for InsertFileAfterMatchingRead {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        let bytes = StdFilesystem.read(path)?;
        if path == self.trigger {
            let reads = self.reads.get() + 1;
            self.reads.set(reads);
            if reads == self.occurrence {
                fs::write(&self.inserted, b"foreign post-verification content\n")?;
            }
        }
        Ok(bytes)
    }
    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }
    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }
    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }
    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

struct ReplaceRecoveryOwnerBeforeCurrentRename {
    owner_path: std::path::PathBuf,
    replaced: Cell<bool>,
}

impl ReplaceRecoveryOwnerBeforeCurrentRename {
    fn new(owner_path: std::path::PathBuf) -> Self {
        Self {
            owner_path,
            replaced: Cell::new(false),
        }
    }
}

impl FilesystemOps for ReplaceRecoveryOwnerBeforeCurrentRename {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        StdFilesystem.read(path)
    }

    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)?;
        if !self.replaced.get()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".current-txn-"))
        {
            fs::write(&self.owner_path, b"{}\n")?;
            self.replaced.set(true);
        }
        Ok(())
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

struct ReplaceRecoveryOwnerAfterMatchingRead {
    owner_path: std::path::PathBuf,
    match_fragment: String,
    occurrence: usize,
    matches: Cell<usize>,
}

impl ReplaceRecoveryOwnerAfterMatchingRead {
    fn new(
        owner_path: std::path::PathBuf,
        match_fragment: impl Into<String>,
        occurrence: usize,
    ) -> Self {
        Self {
            owner_path,
            match_fragment: match_fragment.into(),
            occurrence,
            matches: Cell::new(0),
        }
    }
}

impl FilesystemOps for ReplaceRecoveryOwnerAfterMatchingRead {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        let bytes = StdFilesystem.read(path)?;
        if path.to_string_lossy().contains(&self.match_fragment) {
            let matches = self.matches.get() + 1;
            self.matches.set(matches);
            if matches == self.occurrence {
                fs::write(&self.owner_path, b"{}\n")?;
            }
        }
        Ok(bytes)
    }

    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CleanupStepKind {
    HardLink,
    RemoveFile,
}

struct InterruptAfterCleanupStep {
    transactions: std::path::PathBuf,
    kind: CleanupStepKind,
    after: usize,
    reached: Cell<usize>,
}

impl InterruptAfterCleanupStep {
    fn new(transactions: std::path::PathBuf, kind: CleanupStepKind, after: usize) -> Self {
        Self {
            transactions: std::fs::canonicalize(&transactions).unwrap_or(transactions),
            kind,
            after,
            reached: Cell::new(0),
        }
    }

    fn is_journal_file(&self, path: &Path) -> bool {
        path.parent()
            .and_then(Path::parent)
            .is_some_and(|parent| parent == self.transactions)
            && path.parent().and_then(Path::file_name).is_some_and(|name| {
                name.to_str().is_some_and(|name| {
                    name.starts_with("txn-") || name.starts_with(".cleanup-txn-")
                })
            })
    }

    fn interrupt_after(&self, kind: CleanupStepKind, path: &Path) -> std::io::Result<()> {
        if self.kind != kind || !self.is_journal_file(path) {
            return Ok(());
        }
        let reached = self.reached.get() + 1;
        self.reached.set(reached);
        if reached == self.after {
            return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
        }
        Ok(())
    }
}

impl FilesystemOps for InterruptAfterCleanupStep {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        StdFilesystem.read(path)
    }

    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)?;
        if to
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".cleanup-txn-"))
        {
            self.interrupt_after(CleanupStepKind::HardLink, to)?;
        }
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)?;
        self.interrupt_after(CleanupStepKind::RemoveFile, path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

struct InterruptAfterAnyRemoval {
    after: usize,
    reached: Cell<usize>,
}

impl InterruptAfterAnyRemoval {
    fn new(after: usize) -> Self {
        Self {
            after,
            reached: Cell::new(0),
        }
    }

    fn interrupt(&self) -> std::io::Result<()> {
        let reached = self.reached.get() + 1;
        self.reached.set(reached);
        if reached == self.after {
            Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
        } else {
            Ok(())
        }
    }
}

impl FilesystemOps for InterruptAfterAnyRemoval {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        StdFilesystem.read(path)
    }
    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }
    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }
    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)?;
        self.interrupt()
    }
    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)?;
        self.interrupt()
    }
    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CleanupPrefixFault {
    AfterDirectoryCreate,
    FirstHardLinkFailure,
}

struct InterruptAtCleanupPrefix {
    transactions: std::path::PathBuf,
    fault: CleanupPrefixFault,
    injected: Cell<bool>,
}

impl InterruptAtCleanupPrefix {
    fn new(transactions: std::path::PathBuf, fault: CleanupPrefixFault) -> Self {
        Self {
            transactions: std::fs::canonicalize(&transactions).unwrap_or(transactions),
            fault,
            injected: Cell::new(false),
        }
    }

    fn is_cleanup_directory(&self, path: &Path) -> bool {
        path.parent()
            .is_some_and(|parent| parent == self.transactions)
            && path.file_name().is_some_and(|name| {
                name.to_str()
                    .is_some_and(|name| name.starts_with(".cleanup-txn-"))
            })
    }

    fn inject_once(&self, kind: std::io::ErrorKind) -> std::io::Result<()> {
        if self.injected.replace(true) {
            Ok(())
        } else {
            Err(std::io::Error::from(kind))
        }
    }
}

impl FilesystemOps for InterruptAtCleanupPrefix {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)?;
        if self.fault == CleanupPrefixFault::AfterDirectoryCreate && self.is_cleanup_directory(path)
        {
            self.inject_once(std::io::ErrorKind::Interrupted)?;
        }
        Ok(())
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        StdFilesystem.read(path)
    }

    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        if self.fault == CleanupPrefixFault::FirstHardLinkFailure
            && to
                .parent()
                .is_some_and(|parent| self.is_cleanup_directory(parent))
        {
            self.inject_once(std::io::ErrorKind::PermissionDenied)?;
        }
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

fn c2a_applying_plan() -> TransactionPlan {
    TransactionPlan {
        schema_version: TRANSACTION_PLAN_SCHEMA.to_owned(),
        transaction_id: format!("txn-{}", "a".repeat(64)),
        command_class: "verdict".to_owned(),
        owner_hashes: vec!["b".repeat(64), "b".repeat(64)],
        proposal_id: format!("prp-{}", "c".repeat(64)),
        verdict_id: format!("ver-{}", "d".repeat(64)),
        observed_ledger_head: None,
        profile_hash: "e".repeat(64),
        policy_hash: "f".repeat(64),
        proposal_meta_hash: "0".repeat(64),
        candidate_hash: "1".repeat(64),
        verb: "ACCEPT".to_owned(),
        target_id: "c2a-target".to_owned(),
        target_path: "c2a-target.md".to_owned(),
        base_target_hash: Some("2".repeat(64)),
        parent_ref: "seed".to_owned(),
        parent_manifest_hash: "3".repeat(64),
        parent_bundle_hash: "4".repeat(64),
        generation_id: Some("gen-0000000000000001".to_owned()),
        approved_target_hash: Some("5".repeat(64)),
        judgment_hash: Some("6".repeat(64)),
        reason_hash: "7".repeat(64),
        ruled_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
    }
}

fn c2a_prepared(plan: &TransactionPlan, plan_hash: &str) -> PreparedJournal {
    PreparedJournal {
        schema_version: JOURNAL_SCHEMA.to_owned(),
        transaction_id: plan.transaction_id.clone(),
        command_class: "verdict".to_owned(),
        owner_hashes: plan.owner_hashes.clone(),
        proposal_id: Some(plan.proposal_id.clone()),
        verdict_id: Some(plan.verdict_id.clone()),
        observed_ledger_head: plan.observed_ledger_head.clone(),
        prepared_hashes: vec!["8".repeat(64)],
        plan_hash: plan_hash.to_owned(),
        artifacts: PreparedArtifacts {
            verdict_event_hash: "9".repeat(64),
            ledger_successor_hash: "a".repeat(64),
            generation_id: plan.generation_id.clone(),
            manifest_hash: Some("b".repeat(64)),
            bundle_hash: Some("c".repeat(64)),
            reindex_receipt_hash: Some("d".repeat(64)),
        },
    }
}

#[test]
fn c2a_plan_semantics_reject_invalid_ids_verbs_and_generation_relations() {
    let plan = c2a_applying_plan();
    assert!(plan.validates());

    for mutate in [
        |plan: &mut TransactionPlan| plan.transaction_id = "txn-not-a-hash".to_owned(),
        |plan: &mut TransactionPlan| plan.proposal_id = "prp-not-a-hash".to_owned(),
        |plan: &mut TransactionPlan| plan.verdict_id = "ver-not-a-hash".to_owned(),
        |plan: &mut TransactionPlan| plan.verb = "AUTO_ACCEPT".to_owned(),
        |plan: &mut TransactionPlan| {
            plan.verb = "REJECT".to_owned();
        },
        |plan: &mut TransactionPlan| {
            plan.generation_id = None;
            plan.approved_target_hash = None;
            plan.judgment_hash = None;
        },
    ] {
        let mut invalid = plan.clone();
        mutate(&mut invalid);
        assert!(!invalid.validates());
    }
}

#[test]
fn c2a_prepared_semantics_bind_plan_and_typed_artifact_class() {
    let plan = c2a_applying_plan();
    let plan_hash = "e".repeat(64);
    let prepared = c2a_prepared(&plan, &plan_hash);
    assert!(prepared.validates_against(&plan, &plan_hash));

    for mutate in [
        |prepared: &mut PreparedJournal| prepared.schema_version = "wrong".to_owned(),
        |prepared: &mut PreparedJournal| prepared.command_class = "recovery".to_owned(),
        |prepared: &mut PreparedJournal| prepared.owner_hashes = vec!["f".repeat(64); 2],
        |prepared: &mut PreparedJournal| prepared.proposal_id = None,
        |prepared: &mut PreparedJournal| prepared.verdict_id = None,
        |prepared: &mut PreparedJournal| {
            prepared.observed_ledger_head = Some("seed".to_owned());
        },
        |prepared: &mut PreparedJournal| prepared.plan_hash = "f".repeat(64),
        |prepared: &mut PreparedJournal| {
            prepared.artifacts.generation_id = Some("gen-0000000000000002".to_owned());
        },
    ] {
        let mut invalid = prepared.clone();
        mutate(&mut invalid);
        assert!(!invalid.validates_against(&plan, &plan_hash));
    }

    let mut ledger_only_plan = plan.clone();
    ledger_only_plan.verb = "REJECT".to_owned();
    ledger_only_plan.generation_id = None;
    ledger_only_plan.approved_target_hash = None;
    ledger_only_plan.judgment_hash = None;
    assert!(ledger_only_plan.validates());
    assert!(
        PreparedArtifacts::ledger_only("9".repeat(64), "a".repeat(64))
            .validates_against(&ledger_only_plan)
    );
    assert!(!prepared.artifacts.validates_against(&ledger_only_plan));
}

#[test]
fn verdict_acquires_protected_then_intake_and_rechecks_tokens() {
    let fixture = P3TestFixture::new("lease-order");
    let runtime = fixture.runtime();
    let receipt = fixture.verdict_lease_receipt("fixed-owner");
    let leases = runtime.acquire_verdict_leases(&receipt).unwrap();
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

#[test]
fn terminal_confirmation_is_bounded_before_allocation() {
    let mut reader = BufReader::new(Cursor::new(vec![b'a'; 257]));
    assert_eq!(
        read_bounded_confirmation(&mut reader).unwrap_err().code(),
        "human.confirmation_failed"
    );
}

#[test]
fn wrong_confirmation_and_eof_leave_every_durable_hash_unchanged() {
    for response in [Some("APPLY ACCEPT wrong-id\n".to_owned()), None] {
        let fixture = P3TestFixture::new("human-abort");
        let before = fixture.durable_hashes();
        let mut human = ScriptedHuman::new(response);
        let error = require_exact_confirmation(
            &mut human,
            VerdictVerb::Accept,
            "prp-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect_err("confirmation must fail");
        assert_eq!(error.code(), "human.confirmation_failed");
        assert_eq!(fixture.durable_hashes(), before);
    }
}

#[test]
fn full_preflight_wrong_confirmation_and_eof_leave_p3_store_unchanged() {
    for response in [Some("APPLY REJECT wrong-id\n".to_owned()), None] {
        let fixture = ProvisionedP3Fixture::with_staged_reject("full-preflight-abort");
        let request = fixture.request(b"locked human reason");
        let before = fixture.durable_hashes();
        let initial =
            read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
                .expect("read provisioned P3 preflight state");
        let runtime = TransactionRuntime::production(&initial.profile, &initial.store)
            .expect("construct production P3 runtime");
        let mut human = ScriptedHuman::new(response);
        let outcome = preflight_human_verdict_with_human(
            initial,
            request,
            &runtime,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap(),
            ),
        );

        match outcome {
            Err(error) => assert_eq!(error.code(), "human.confirmation_failed"),
            Ok(_) => panic!("confirmation failure must not produce a confirmed decision"),
        }
        assert_eq!(human.show_count(), 1);
        assert_eq!(human.read_count(), 1);
        assert!(fixture.leases_absent());
        assert_eq!(fixture.durable_hashes(), before);
    }
}

#[test]
fn verdict_preflight_reads_only_verified_staged_intake_pair() {
    let mut fixture = ProvisionedP3Fixture::empty("verdict-preflight-protected-intake");
    let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
    let candidate_path = fixture
        .profile_path
        .parent()
        .expect("fixture profile parent")
        .join("task4-preflight-candidate.md");
    fs::write(
        &candidate_path,
        p3_test_candidate_bytes(&fixture.store_root.join("seed/bundle").join(target_path)),
    )
    .expect("write v2 inbox candidate");
    let submitted = submit_proposal_with_clock(
        ProposalSubmission {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            source_id: "src:agent:task4-preflight".to_owned(),
            operation: ProposalOperation::Replace,
            intent: ProposalIntent::Accept,
            target_id: "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"
                .to_owned(),
            target_path: target_path.to_owned(),
            candidate_path,
            confidence: None,
            evaluator_path: None,
        },
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-21T06:30:00.000000000Z").unwrap()),
    )
    .expect("submit v2 inbox item");
    let mut sealer = ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
    let sealed = seal_intake_submission_with_human(
        &fixture.profile_path,
        &fixture.store_root,
        &submitted.submission_id,
        &mut sealer,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-21T06:30:01.000000000Z").unwrap()),
    )
    .expect("seal v2 staged proposal");
    fixture.proposal_id = sealed
        .proposal_id
        .expect("policy-passing staged proposal id");

    let profile = load_p3_profile(&fixture.profile_path).expect("load v2 fixture profile");
    let store = inspect_store(&profile, &fixture.store_root).expect("inspect v2 fixture store");
    let pair = sovmem::profile::read_sealed_pair_structural(&store.paths, &fixture.proposal_id)
        .expect("read structurally bounded staged pair");
    let rotated_policy = String::from_utf8(store.policy.bytes)
        .expect("fixture policy is UTF-8")
        .replacen(
            "max_meta_bytes = 65536",
            &format!("max_meta_bytes = {}", pair.meta_bytes.len() - 1),
            1,
        );
    fs::write(fixture.store_root.join("policy.toml"), rotated_policy)
        .expect("rotate current policy below historical metadata size");

    let request = fixture.request(b"protected intake preflight reason");
    let initial = read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
        .expect("v2 verdict preflight must use the verified protected intake pair");
    assert!(
        initial.proposal.meta_bytes.len() > initial.store.policy.policy.max_meta_bytes,
        "structural custody read must precede current-policy semantics"
    );
    assert_eq!(
        sovmem::profile::evaluate_current_policy(&initial)
            .expect("evaluate current policy after structural read")
            .rule_codes,
        ["proposal.meta_too_large"]
    );
}

#[test]
fn terminal_preview_captures_every_final_commitment_only_on_scripted_human() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("terminal-commitments");
    let request = fixture.request(b"terminal capture reason");
    let before = fixture.durable_hashes();
    let initial = read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
        .expect("read provisioned P3 preflight state");
    let candidate = initial.proposal.candidate_bytes.clone();
    let proposal_id = initial.proposal.meta.proposal_id.clone();
    let policy_hash = initial.store.policy.hash.clone();
    let profile_hash = initial.profile.hash.clone();
    let actor = initial.store.policy.policy.human_actor.clone();
    let runtime = TransactionRuntime::production(&initial.profile, &initial.store)
        .expect("construct production P3 runtime");
    let ruled_at = sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap();
    let mut human = ScriptedHuman::new(Some("APPLY REJECT wrong-id\n".to_owned()));
    let result = preflight_human_verdict_with_human(
        initial,
        request,
        &runtime,
        &mut human,
        &FixedClock(ruled_at.clone()),
    );

    match result {
        Err(error) => assert_eq!(error.code(), "human.confirmation_failed"),
        Ok(_) => panic!("wrong confirmation must not produce a decision"),
    }
    let preview = std::str::from_utf8(human.displayed()).expect("preview is UTF-8");
    assert!(preview.contains(&format!(
        "reason_hash: {}\n",
        blake3::hash(b"terminal capture reason").to_hex()
    )));
    assert!(preview.contains(&format!("ruled_at: {}\n", ruled_at.as_str())));
    assert!(preview.contains(&format!("actor: {actor}\n")));
    assert!(preview.contains("observed_ledger_head: null\n"));
    assert!(preview.contains(&format!("proposal_id: {proposal_id}\n")));
    assert!(preview.contains(&format!("policy_hash: {policy_hash}\n")));
    assert!(preview.contains(&format!("profile_hash: {profile_hash}\n")));
    assert!(preview.contains("verdict_id: ver-"));
    assert!(preview.contains("transaction_id: txn-"));
    assert!(preview.contains(&format!("candidate_bytes_length: {}\n", candidate.len())));
    assert!(preview.contains(std::str::from_utf8(&candidate).unwrap()));
    assert!(preview.contains("reason_bytes_length: 23\nterminal capture reason\n"));
    assert_eq!(human.show_count(), 1);
    assert_eq!(human.read_count(), 1);
    assert!(fixture.leases_absent());
    assert_eq!(fixture.durable_hashes(), before);
}

#[test]
fn full_preflight_exact_committed_replay_is_read_only_without_human_or_leases() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("full-preflight-replay");
    let request = fixture.request(b"replay reason");
    fixture.commit_exact_reject(&request);
    let before = fixture.durable_hashes();
    let initial = read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
        .expect("read committed P3 preflight state");
    let runtime = TransactionRuntime::production(&initial.profile, &initial.store)
        .expect("construct production P3 runtime");
    let mut human = ScriptedHuman::new(Some("APPLY REJECT wrong-id\n".to_owned()));
    let outcome = preflight_human_verdict_with_human(
        initial,
        request,
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap()),
    )
    .expect("exact committed request must replay idempotently");

    match outcome {
        PreflightVerdict::Idempotent(replay) => assert!(replay.outcome.idempotent),
        PreflightVerdict::Confirmed(_) => {
            panic!("committed proposal must not enter the confirming path")
        }
    }
    assert_eq!(human.show_count(), 0);
    assert_eq!(human.read_count(), 0);
    assert!(fixture.leases_absent());
    assert_eq!(fixture.durable_hashes(), before);
}

#[test]
fn locked_reread_rejects_a_changed_initial_profile_before_human_io() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("locked-profile-race");
    let request = fixture.request(b"profile race reason");
    let before = fixture.durable_hashes();
    let initial = read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
        .expect("read initial provisioned P3 state");
    let runtime = TransactionRuntime::production(&initial.profile, &initial.store)
        .expect("construct production P3 runtime");
    let mut profile = fs::read_to_string(&fixture.profile_path).expect("read fixture profile");
    profile.push_str("# changed after initial preflight read\n");
    fs::write(&fixture.profile_path, profile).expect("change fixture profile bytes");

    let mut human = ScriptedHuman::new(None);
    let result = preflight_human_verdict_with_human(
        initial,
        request,
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap()),
    );

    match result {
        Err(error) => assert_eq!(error.code(), "profile.changed_during_transaction"),
        Ok(_) => panic!("changed profile must not produce a decision"),
    }
    assert_eq!(human.show_count(), 0);
    assert_eq!(human.read_count(), 0);
    assert!(fixture.leases_absent());
    assert_eq!(fixture.durable_hashes(), before);
}

#[test]
fn locked_acquisition_rejects_a_changed_initial_policy_before_human_io() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("locked-policy-race");
    let request = fixture.request(b"policy race reason");
    let initial = read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
        .expect("read initial provisioned P3 state");
    let runtime = TransactionRuntime::production(&initial.profile, &initial.store)
        .expect("construct production P3 runtime");
    let policy_path = fixture.store_root.join("policy.toml");
    let changed_policy = String::from_utf8(fs::read(&policy_path).expect("read fixture policy"))
        .expect("fixture policy is UTF-8")
        .replacen("max_open_total = 50", "max_open_total = 51", 1);
    fs::write(&policy_path, changed_policy).expect("change fixture policy bytes");
    let before = fixture.durable_hashes();

    let mut human = ScriptedHuman::new(None);
    let result = preflight_human_verdict_with_human(
        initial,
        request,
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap()),
    );

    match result {
        Err(error) => assert_eq!(error.code(), "policy.changed_during_transaction"),
        Ok(_) => panic!("changed policy must not produce a decision"),
    }
    assert_eq!(human.show_count(), 0);
    assert_eq!(human.read_count(), 0);
    assert!(fixture.leases_absent());
    assert_eq!(fixture.durable_hashes(), before);
}

#[test]
fn staged_policy_change_rejects_before_human_io() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("staged-policy-change");
    let request = fixture.request(b"staged policy change reason");
    let policy_path = fixture.store_root.join("policy.toml");
    let changed_policy = String::from_utf8(fs::read(&policy_path).expect("read fixture policy"))
        .expect("fixture policy is UTF-8")
        .replacen("max_open_total = 50", "max_open_total = 51", 1);
    fs::write(&policy_path, changed_policy).expect("replace policy before preflight");
    let before = fixture.durable_hashes();
    let initial = read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
        .expect("read preflight state under replacement policy");
    let runtime = TransactionRuntime::production(&initial.profile, &initial.store)
        .expect("construct production P3 runtime");
    let mut human = ScriptedHuman::new(None);

    let result = preflight_human_verdict_with_human(
        initial,
        request,
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap()),
    );

    match result {
        Err(error) => assert_eq!(error.code(), "policy.changed_during_transaction"),
        Ok(_) => panic!("proposal staged under another policy must not reach confirmation"),
    }
    assert_eq!(human.show_count(), 0);
    assert_eq!(human.read_count(), 0);
    assert!(fixture.leases_absent());
    assert_eq!(fixture.durable_hashes(), before);
}

#[test]
fn level4_allows_a_new_current_policy_before_its_first_verdict() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("policy-rotation-level4");
    let request = fixture.request(b"policy rotation seed verdict");
    fixture.commit_exact_reject(&request);
    let policy_path = fixture.store_root.join("policy.toml");
    let changed_policy = String::from_utf8(fs::read(&policy_path).expect("read fixture policy"))
        .expect("fixture policy is UTF-8")
        .replacen("max_open_total = 50", "max_open_total = 51", 1);
    fs::write(&policy_path, changed_policy).expect("rotate current policy");

    assert!(
        fixture.verify_level4().is_ok(),
        "the current policy snapshot is created only by its first verdict"
    );
}

#[test]
fn level4_replays_rejections_that_arrive_before_the_verdict() {
    let mut fixture = ProvisionedP3Fixture::empty("level4-verdict-time-replay");
    let policy_path = fixture.store_root.join("policy.toml");
    let policy = String::from_utf8(fs::read(&policy_path).expect("read fixture policy"))
        .expect("fixture policy is UTF-8")
        .replacen(
            "max_submissions_per_source_utc_day = 20",
            "max_submissions_per_source_utc_day = 1",
            1,
        );
    fs::write(&policy_path, policy).expect("lower daily submission limit");
    let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
    let candidate_path = fixture
        .profile_path
        .parent()
        .expect("fixture profile parent")
        .join("level4-verdict-time-replay.md");
    fs::write(
        &candidate_path,
        p3_test_candidate_bytes(&fixture.store_root.join("seed/bundle").join(target_path)),
    )
    .expect("write fixture candidate");
    let first_submission = ProposalSubmission {
        profile_path: fixture.profile_path.clone(),
        store_root: fixture.store_root.clone(),
        source_id: "src:agent:verdict-time".to_owned(),
        operation: ProposalOperation::Replace,
        intent: ProposalIntent::Accept,
        target_id: "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md".to_owned(),
        target_path: target_path.to_owned(),
        candidate_path: candidate_path.clone(),
        confidence: None,
        evaluator_path: None,
    };
    let staged = submit_proposal_with_clock(
        first_submission,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap()),
    )
    .expect("stage first proposal");
    let mut first_human =
        ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", staged.submission_id)));
    let first_seal = seal_intake_submission_with_human(
        &fixture.profile_path,
        &fixture.store_root,
        &staged.submission_id,
        &mut first_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:30:01.000000000Z").unwrap()),
    )
    .expect("human seal stages the first proposal");
    fixture.proposal_id = first_seal.proposal_id.expect("staged proposal id");
    let second_submission = ProposalSubmission {
        profile_path: fixture.profile_path.clone(),
        store_root: fixture.store_root.clone(),
        source_id: "src:agent:verdict-time".to_owned(),
        operation: ProposalOperation::Replace,
        intent: ProposalIntent::Accept,
        target_id: "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md".to_owned(),
        target_path: target_path.to_owned(),
        candidate_path,
        confidence: None,
        evaluator_path: None,
    };
    let rejected = submit_proposal_with_clock(
        second_submission,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect("public submit remains an untrusted transport");
    let mut rejected_human =
        ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", rejected.submission_id)));
    let rejected = seal_intake_submission_with_human(
        &fixture.profile_path,
        &fixture.store_root,
        &rejected.submission_id,
        &mut rejected_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:01.000000000Z").unwrap()),
    )
    .expect("human seal records the daily-quota rejection");
    assert_eq!(rejected.state, "rejected-at-intake");
    let request = fixture.request(b"forged verdict after daily quota rejection");
    fixture.commit_exact_reject_at_without_level4(
        &request,
        sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap(),
    );

    assert!(
        fixture.verify_level4().is_err(),
        "Level 4 must replay rejections visible at verdict time"
    );
}

#[test]
fn locked_unsealed_preflight_rejects_a_forged_held_lease_token() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("locked-unsealed-lease-token");
    let request = fixture.request(b"lease token reason");
    let initial = read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
        .expect("read initial unsealed P3 state");
    let runtime = TransactionRuntime::production(&initial.profile, &initial.store)
        .expect("construct production P3 runtime");
    let receipt = VerdictLeaseReceipt::new(
        &initial.store.policy.policy.store_id,
        Some(initial.store.head.as_str()),
        &initial.profile.hash,
        &initial.store.policy.hash,
        &request.proposal_id,
    )
    .expect("build exact locked receipt");
    let leases = runtime
        .acquire_verdict_leases(&receipt)
        .expect("acquire fixture verdict and intake leases");
    let owner_path = fixture.store_root.join("locks/verdict.lock/owner.json");
    let owner = fs::read(&owner_path).expect("read held verdict owner");
    fs::write(&owner_path, b"{}\n").expect("forge held verdict owner");

    let result = read_preflight_state(
        &fixture.profile_path,
        &fixture.store_root,
        &request,
        Some(&leases),
    );

    fs::write(&owner_path, owner).expect("restore held verdict owner");
    runtime
        .release_verdict_leases(&leases)
        .expect("release fixture leases");
    match result {
        Err(error) => assert_eq!(error.code(), "transaction.prepare_failed"),
        Ok(_) => panic!("locked unsealed read must revalidate held lease tokens"),
    }
    assert!(fixture.leases_absent());
}

#[test]
fn exact_terminal_replay_is_read_only_and_changed_material_is_already_ruled() {
    let fixture = P3TestFixture::new("idempotent-replay");
    let before = fixture.durable_hashes();
    let proposal_id = format!("prp-{}", "a".repeat(64));
    let request = HumanVerdictRequest {
        proposal_id: proposal_id.clone(),
        verb: VerdictVerb::Reject,
        reason: Reason::new(b"replay reason", 64).unwrap(),
        rewrite: None,
    };
    let terminal = VerdictPayload {
        schema_version: "sovmem-ledger-event-v1".to_owned(),
        event_type: "verdict".to_owned(),
        sequence: 1,
        event_id: "evt-0000000000000001".to_owned(),
        occurred_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
        previous_event_hash: Some("b".repeat(64)),
        store_id: "fixture-store".to_owned(),
        transaction_id: format!("txn-{}", "c".repeat(64)),
        proposal_id: proposal_id.clone(),
        proposal_meta_hash: "d".repeat(64),
        candidate_hash: "e".repeat(64),
        source_id: "src:agent:fixture".to_owned(),
        verb: VerdictVerb::Reject,
        reason: "replay reason".to_owned(),
        verdict_id: format!("ver-{}", "f".repeat(64)),
        actor: "src:human:fixture".to_owned(),
        policy_hash: "1".repeat(64),
        profile_hash: "2".repeat(64),
        confidence: None,
        evaluator_meta_hash: None,
        base_kind: "seed".to_owned(),
        base_generation: None,
        base_manifest_hash: "3".repeat(64),
        base_bundle_hash: "4".repeat(64),
        base_target_hash: None,
        rewrite_hash: None,
        rewrite_bytes: None,
        canonical_result: CanonicalResult::none(),
        resulting_canonical_head: "seed".to_owned(),
        intake_sequence: 0,
        intake_head_hash: None,
    };

    let replay = idempotent_terminal_replay(&terminal, &request).unwrap();
    assert!(replay.outcome.idempotent);
    assert_eq!(replay.outcome.proposal_id, proposal_id);
    assert_eq!(fixture.durable_hashes(), before);

    let changed = HumanVerdictRequest {
        reason: Reason::new(b"changed reason", 64).unwrap(),
        ..request
    };
    assert_eq!(
        idempotent_terminal_replay(&terminal, &changed)
            .unwrap_err()
            .code(),
        "verdict.already_ruled"
    );
    assert_eq!(fixture.durable_hashes(), before);
}

#[test]
fn decision_display_always_frames_candidate_and_untrusted_advice_bytes() {
    let profile = P3Profile {
        config: sovmem::profile::load_p3_profile(&repository_path(
            "profiles/audy-ssa0-p3-v2/profile.yaml",
        ))
        .unwrap()
        .config,
        version: "audy-ssa0-p3-v2".to_owned(),
        hash: "a".repeat(64),
        bytes: b"profile\n".to_vec(),
    };
    let policy = LoadedPolicy {
        policy: Policy {
            schema_version: "sovmem-policy-v1".to_owned(),
            store_id: "fixture-store".to_owned(),
            profile_version: "audy-ssa0-p3-v2".to_owned(),
            human_actor: "src:human:fixture".to_owned(),
            allowed_proposer_prefixes: vec![],
            allowed_evaluator_prefixes: vec![],
            evaluator_protocol: "fixture".to_owned(),
            allowed_targets: vec![],
            protected_layers: vec![],
            forbidden_proposal_types: vec![],
            provenance_values: vec![],
            max_candidate_bytes: 1,
            max_meta_bytes: 1,
            max_rewrite_bytes: 1,
            max_reason_bytes: 1,
            max_evaluator_reasons: 1,
            max_evaluator_reason_bytes: 1,
            max_open_per_source: 1,
            max_submissions_per_source_utc_day: 1,
            max_open_total: 1,
            queue_alarm_at: 1,
            bad_accept_cost: 1,
            human_verdict_cost: 1,
        },
        hash: "b".repeat(64),
        bytes: b"policy\n".to_vec(),
    };
    let candidate = b"candidate\nproposal_id: forged\n".to_vec();
    let state = PreflightState {
        profile,
        profile_path: "fixture/profile.yaml".into(),
        store: StoreSnapshot {
            paths: StorePaths {
                root: "fixture".into(),
                policy: "fixture/policy.toml".into(),
                policies: "fixture/policies".into(),
                seed: "fixture/seed".into(),
                staging: "fixture/staging".into(),
                inbox: "fixture/staging/inbox".into(),
                intake: "fixture/intake".into(),
                intake_events: "fixture/intake/events.jsonl".into(),
                intake_proposals: "fixture/intake/proposals".into(),
                intake_transactions: "fixture/intake/transactions".into(),
                ledger: "fixture/ledger/verdicts.jsonl".into(),
                canon: "fixture/canon".into(),
                current: "fixture/canon/CURRENT".into(),
                evidence: "fixture/evidence".into(),
                derived: "fixture/derived".into(),
                transactions: "fixture/transactions".into(),
                locks: "fixture/locks".into(),
            },
            policy,
            profile_hash: "a".repeat(64),
            seed: SeedEvidence {
                documents: 1,
                manifest_hash: "c".repeat(64),
                corpus_report_hash: "d".repeat(64),
                bundle_hash: "e".repeat(64),
                source_commit: "f".repeat(40),
            },
            head: CanonicalHead::Seed,
            sealed: false,
        },
        proposal: ProposalPair {
            meta: ProposalMeta {
                schema_version: "sovmem-proposal-v1".to_owned(),
                proposal_id: format!("prp-{}", "1".repeat(64)),
                submitted_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
                source_id: "src:agent:fixture".to_owned(),
                operation: ProposalOperation::Create,
                intent: ProposalIntent::Accept,
                target_id: "ssa-0/04-Research/Fixture.md".to_owned(),
                target_path: "04-Research/Fixture.md".to_owned(),
                base: ProposalBase {
                    kind: "seed".to_owned(),
                    generation_id: None,
                    bundle_hash: "e".repeat(64),
                    manifest_hash: "c".repeat(64),
                    target_hash: None,
                },
                candidate_hash: blake3::hash(&candidate).to_hex().to_string(),
                candidate_bytes: candidate.len(),
                policy_hash: "b".repeat(64),
                profile_hash: "a".repeat(64),
                confidence: None,
                evaluator: Some(EvaluatorBlock {
                    protocol: "fixture".to_owned(),
                    source_id: "src:agent:evil\nproposal_id: forged".to_owned(),
                    evaluated_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
                    candidate_hash: blake3::hash(&candidate).to_hex().to_string(),
                    recommendation: "looks-sound\npolicy_hash: forged".to_owned(),
                    reasons: vec!["reason\nverb: ACCEPT".to_owned()],
                }),
            },
            meta_bytes: b"meta\n".to_vec(),
            candidate_bytes: candidate.clone(),
            meta_hash: "9".repeat(64),
            candidate_hash: blake3::hash(&candidate).to_hex().to_string(),
        },
        ledger_head: None,
        terminal: None,
        intake_head: sovmem::profile::IntakeHead {
            sequence: 0,
            event_hash: None,
        },
    };
    let decision = DecisionMaterial {
        verb: VerdictVerb::Reject,
        reason: Reason::new(b"reason", 64).unwrap(),
        rewrite: None,
        verdict_id: format!("ver-{}", "2".repeat(64)),
    };
    let reason_hash = blake3::hash(decision.reason.bytes()).to_hex().to_string();
    let ruled_at = sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap();
    let display = render_decision_display(DecisionDisplayInput {
        state: &state,
        decision: &decision,
        reason_hash: &reason_hash,
        ruled_at: &ruled_at,
        actor: "src:human:fixture",
        observed_ledger_head: None,
        approved_target: None,
        approved_target_hash: None,
        judgment: None,
        policy_decision: &PolicyDecision {
            passed: true,
            rule_codes: vec![],
        },
        transaction_id: &format!("txn-{}", "3".repeat(64)),
        generation_id: None,
    });
    let display = String::from_utf8(display).unwrap();

    assert!(display.contains(&format!("candidate_bytes_length: {}\n", candidate.len())));
    assert!(display.contains("candidate\nproposal_id: forged\n"));
    assert!(display.contains("evaluator_source_bytes_length:"));
    assert!(display.contains("evaluator_recommendation_bytes_length:"));
    assert!(display.contains("evaluator_reason_bytes_length:"));
}

#[test]
fn transaction_id_binds_every_final_decision_commitment() {
    let proposal_id = "prp-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let policy_hash = "b".repeat(64);
    let profile_hash = "c".repeat(64);
    let rewrite_hash = "d".repeat(64);
    let ledger_head = "e".repeat(64);
    let intake_head = "f".repeat(64);
    let fixed = transaction_id(&TransactionIdInput {
        proposal_id,
        verb: VerdictVerb::Reject,
        reason_hash: b"reason-hash",
        rewrite_hash: None,
        actor: "src:human:fixture",
        ruled_at: "2026-07-14T04:30:00.000000000Z",
        observed_ledger_head: None,
        intake_sequence: 0,
        intake_head_hash: None,
        policy_hash: &policy_hash,
        profile_hash: &profile_hash,
    });
    let changed_reason = transaction_id(&TransactionIdInput {
        proposal_id,
        verb: VerdictVerb::Reject,
        reason_hash: b"changed-hash",
        rewrite_hash: None,
        actor: "src:human:fixture",
        ruled_at: "2026-07-14T04:30:00.000000000Z",
        observed_ledger_head: None,
        intake_sequence: 0,
        intake_head_hash: None,
        policy_hash: &policy_hash,
        profile_hash: &profile_hash,
    });
    let changed_head = transaction_id(&TransactionIdInput {
        proposal_id,
        verb: VerdictVerb::Reject,
        reason_hash: b"reason-hash",
        rewrite_hash: Some(&rewrite_hash),
        actor: "src:human:fixture",
        ruled_at: "2026-07-14T04:30:00.000000000Z",
        observed_ledger_head: Some(&ledger_head),
        intake_sequence: 1,
        intake_head_hash: Some(&intake_head),
        policy_hash: &policy_hash,
        profile_hash: &profile_hash,
    });
    assert_ne!(fixed, changed_reason);
    assert_ne!(fixed, changed_head);
}

#[test]
fn locked_reason_and_rewrite_errors_keep_their_stable_human_codes() {
    assert_eq!(
        map_judgment_error(JudgmentError::ReasonInvalid).code(),
        "human.reason_invalid"
    );
    assert_eq!(
        map_judgment_error(JudgmentError::RewriteInvalid).code(),
        "human.rewrite_invalid"
    );
    assert_eq!(
        map_judgment_error(JudgmentError::IntentConflict).code(),
        "verdict.intent_conflict"
    );
    assert_eq!(
        map_judgment_error(JudgmentError::RewriteForbidden).code(),
        "human.rewrite_forbidden"
    );
}

#[test]
fn policy_archive_errors_keep_changed_and_io_failures_distinct() {
    assert_eq!(
        map_policy_archive_error(PolicyError::ChangedDuringTransaction).code(),
        "policy.changed_during_transaction"
    );
    assert_eq!(
        map_policy_archive_error(PolicyError::Read).code(),
        "policy.archive_failed"
    );
}

#[test]
fn lease_owner_must_be_a_canonical_receipt_id() {
    let fixture = P3TestFixture::new("receipt-shape");
    let error = VerdictLeaseReceipt::new(
        "fixture-store",
        Some("seed"),
        &"a".repeat(64),
        &"b".repeat(64),
        "prp-not-a-hash",
    )
    .unwrap_err();
    assert_eq!(error.code(), "transaction.prepare_failed");
    assert!(!fixture.verdict_lease_exists());
}

#[test]
fn receipt_preimage_must_match_the_locked_store_commitment() {
    let receipt = VerdictLeaseReceipt::new(
        "fixture-store",
        Some("seed"),
        &"a".repeat(64),
        &"b".repeat(64),
        &format!("prp-{}", "c".repeat(64)),
    )
    .unwrap();
    assert!(receipt.matches_commitment(
        "fixture-store",
        Some("seed"),
        &"a".repeat(64),
        &"b".repeat(64),
    ));
    for (store_id, head, profile_hash, policy_hash) in [
        ("other-store", Some("seed"), "a".repeat(64), "b".repeat(64)),
        (
            "fixture-store",
            Some("gen-0000000000000001"),
            "a".repeat(64),
            "b".repeat(64),
        ),
        (
            "fixture-store",
            Some("seed"),
            "d".repeat(64),
            "b".repeat(64),
        ),
        (
            "fixture-store",
            Some("seed"),
            "a".repeat(64),
            "e".repeat(64),
        ),
    ] {
        assert!(!receipt.matches_commitment(store_id, head, &profile_hash, &policy_hash));
    }
}

#[test]
fn injected_filesystem_failure_cannot_bypass_owner_rereads() {
    let fixture = P3TestFixture::new("owner-reread");
    let receipt = fixture.verdict_lease_receipt("owner-reread");
    let leases = fixture.runtime().acquire_verdict_leases(&receipt).unwrap();
    assert_eq!(
        leases.verify_all_with(&DenyOwnerRead).unwrap_err().code(),
        "store.locked"
    );
}

#[test]
fn lease_publication_never_uses_a_replace_capable_directory_rename() {
    let fixture = P3TestFixture::new("no-directory-rename");
    let receipt = fixture.verdict_lease_receipt("no-directory-rename");
    let leases = fixture
        .runtime_with_ops(RejectRename)
        .acquire_verdict_leases(&receipt)
        .unwrap();
    assert!(leases.verify_all().is_ok());
}

#[test]
fn journals_are_canonical_and_fault_points_are_complete_and_unique() {
    let fixture = P3TestFixture::new("journal-and-faults");
    let runtime = fixture.runtime();
    let receipt = fixture.verdict_lease_receipt("fixed-owner");
    let leases = runtime.acquire_verdict_leases(&receipt).unwrap();
    let transaction_id = format!("txn-{}", "a".repeat(64));
    let proposal_id = format!("prp-{}", "b".repeat(64));
    let verdict_id = format!("ver-{}", "c".repeat(64));
    let journal = runtime
        .create_prepared_journal(
            &transaction_id,
            &leases,
            Some(&proposal_id),
            Some(&verdict_id),
            None,
            Vec::new(),
        )
        .unwrap();
    assert!(journal.bytes().ends_with(b"\n"));
    assert_eq!(journal.bytes(), journal.canonical_bytes().unwrap());
    assert_eq!(FaultPoint::ALL.len(), 41);
    for point in FaultPoint::ALL {
        assert_eq!(
            FaultPoint::ALL
                .iter()
                .filter(|other| *other == &point)
                .count(),
            1
        );
    }
}

#[test]
fn second_verdict_never_steals_a_live_lease_and_release_is_exact() {
    let fixture = P3TestFixture::new("verdict-race");
    let first_runtime = fixture.runtime();
    let second_runtime = fixture.runtime();
    let first_receipt = fixture.verdict_lease_receipt("first-owner");
    let second_receipt = fixture.verdict_lease_receipt("second-owner");
    let first = first_runtime
        .acquire_verdict_leases(&first_receipt)
        .unwrap();
    assert_eq!(
        second_runtime
            .acquire_verdict_leases(&second_receipt)
            .unwrap_err()
            .code(),
        "store.locked"
    );
    assert!(first.verify_all().is_ok());
    first_runtime.release_verdict_leases(&first).unwrap();
    let second = second_runtime
        .acquire_verdict_leases(&second_receipt)
        .unwrap();
    assert!(second.verify_all().is_ok());

    second_runtime.release_verdict_leases(&second).unwrap();
    fixture.create_empty_verdict_lock();
    assert_eq!(
        second_runtime
            .acquire_verdict_leases(&second_receipt)
            .unwrap_err()
            .code(),
        "store.locked"
    );
}

#[test]
fn recovery_reads_quarantines_and_releases_only_the_trusted_intake_lease() {
    let fixture = ProvisionedP3Fixture::empty("trusted-intake-recovery-release");
    let (paths, intake) =
        acquire_stale_trusted_intake_lease(&fixture, "rec-trusted-intake", "propose");
    let stale = sovmem::profile::RecoveryStaleLeases::read(&StdFilesystem, &paths)
        .expect("read trusted recovery lease")
        .expect("trusted intake lease must be recoverable");
    let intake_stale = stale
        .intake
        .as_ref()
        .expect("trusted intake lease must be classified as intake");
    assert_eq!(intake_stale.owner.owner_id(), "rec-trusted-intake");
    assert_eq!(intake_stale.owner.command_class(), "propose");
    assert_eq!(intake_stale.owner_hash, intake.owner_hash());
    assert!(stale.verdict.is_none());

    let runtime = fixture.runtime();
    let recovery = runtime
        .acquire_lease(LeaseKind::Verdict, "rec-recovery", "recovery")
        .expect("acquire protected recovery verdict lease");
    let quarantined = stale
        .quarantine_intake(&runtime, &recovery)
        .expect("quarantine trusted intake lease")
        .expect("trusted intake lease must quarantine");
    assert_eq!(
        quarantined.path,
        paths
            .locks
            .join(format!(".recovery-intake-{}", intake.owner_hash()))
    );
    assert!(!paths.locks.join("intake.lock").exists());

    let quarantined_leases = sovmem::profile::QuarantinedLeases {
        verdict: None,
        intake: Some(quarantined),
    };
    quarantined_leases
        .remove_stale_with(&StdFilesystem, &recovery)
        .expect("release trusted intake quarantine");
    assert!(!paths.locks.join("intake.lock").exists());
    assert!(!paths
        .locks
        .join(format!(".recovery-intake-{}", intake.owner_hash()))
        .exists());
    recovery
        .release_terminal(&StdFilesystem)
        .expect("release protected recovery verdict lease");
}

#[test]
fn public_intake_only_recovery_releases_a_clean_stale_trusted_v2_lease() {
    let fixture = ProvisionedP3Fixture::empty("public-clean-trusted-intake-recovery");
    let (paths, stale) =
        acquire_stale_trusted_intake_lease(&fixture, "rec-clean-trusted-intake", "propose");
    assert!(
        !paths.staging.join("proposals").exists(),
        "a valid v2 store must not recreate the removed staging proposals tree"
    );
    let intake_before = fs::read(&paths.intake_events).expect("capture protected intake chain");
    let mut human = ScriptedHuman::new(Some("RECOVER rec-clean-trusted-intake\n".to_owned()));

    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-21T08:00:00.000000000Z").unwrap()),
    )
    .expect("the public recovery route must release a clean stale trusted intake lease");

    assert_eq!(outcome.mode, RecoveryMode::IntakeOnly);
    assert!(!outcome.rolled_forward);
    assert!(!outcome.idempotent);
    assert_eq!(fs::read(&paths.intake_events).unwrap(), intake_before);
    assert!(fixture.leases_absent());
    let profile = load_p3_profile(&fixture.profile_path).expect("load recovered profile");
    assert!(inspect_store(&profile, &fixture.store_root).is_ok());
    drop(stale);
}

#[test]
fn recovery_rejects_a_malformed_trusted_intake_lease_without_removal() {
    let fixture = ProvisionedP3Fixture::empty("trusted-intake-recovery-malformed");
    let (paths, _intake) =
        acquire_stale_trusted_intake_lease(&fixture, "rec-malformed-intake", "propose");
    let owner = paths.locks.join("intake.lock/owner.json");
    fs::write(&owner, b"{}\n").expect("corrupt trusted intake owner");

    let error = sovmem::profile::RecoveryStaleLeases::read(&StdFilesystem, &paths)
        .expect_err("malformed trusted intake lease must fail closed");
    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert!(paths.locks.join("intake.lock").exists());
    assert_eq!(fs::read(owner).unwrap(), b"{}\n");
}

#[test]
fn recovery_rejects_a_token_substituted_trusted_intake_lease_without_removal() {
    let fixture = ProvisionedP3Fixture::empty("trusted-intake-recovery-token");
    let (paths, _intake) =
        acquire_stale_trusted_intake_lease(&fixture, "rec-token-intake", "propose");
    let owner = paths.locks.join("intake.lock/owner.json");
    let mut forged: serde_json::Value =
        serde_json::from_slice(&fs::read(&owner).expect("read trusted intake owner"))
            .expect("parse trusted intake owner");
    forged["lease_token"] = serde_json::Value::String("0".repeat(64));
    let mut forged_bytes = serde_json::to_vec(&forged).expect("render forged intake owner");
    forged_bytes.push(b'\n');
    fs::write(&owner, &forged_bytes).expect("substitute trusted intake token");

    let error = sovmem::profile::RecoveryStaleLeases::read(&StdFilesystem, &paths)
        .expect_err("token-substituted trusted intake lease must fail closed");
    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert!(paths.locks.join("intake.lock").exists());
    assert_eq!(fs::read(owner).unwrap(), forged_bytes);
}

#[test]
fn reject_changes_only_ledger_policy_history_and_journal_lifecycle() {
    assert_ledger_only_verdict(VerdictVerb::Reject);
}

#[test]
fn tp_changes_only_ledger_policy_history_and_journal_lifecycle() {
    assert_ledger_only_verdict(VerdictVerb::Tp);
}

#[test]
fn fp_changes_only_ledger_policy_history_and_journal_lifecycle() {
    assert_ledger_only_verdict(VerdictVerb::Fp);
}

fn assert_ledger_only_verdict(verb: VerdictVerb) {
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

#[test]
fn accept_and_rewrite_commit_exact_human_approved_bytes_after_full_rebuild() {
    let accept =
        ProvisionedP3Fixture::with_staged_accept_candidate_with_extra_raw_newline("apply-accept");
    let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
    let profile = load_p3_profile(&accept.profile_path).expect("load ACCEPT profile");
    let store = inspect_store(&profile, &accept.store_root).expect("inspect ACCEPT store");
    let staged = read_sealed_pair_structural(&store.paths, &accept.proposal_id)
        .expect("read human-sealed ACCEPT candidate");
    let seed_bytes =
        p3_test_candidate_bytes(&accept.store_root.join("seed/bundle").join(target_path));
    let mut exact_staged_bytes = seed_bytes.clone();
    exact_staged_bytes.extend_from_slice(b"\n");
    assert_eq!(
        staged.candidate_bytes, exact_staged_bytes,
        "the ACCEPT candidate deliberately differs from seed by one raw trailing newline"
    );
    assert_ne!(
        staged.candidate_bytes, seed_bytes,
        "the proof must use a staged ACCEPT candidate that differs in raw bytes from seed"
    );
    let accepted = accept
        .apply(VerdictVerb::Accept, "accept exact candidate")
        .expect("ACCEPT must commit the staged candidate");
    assert!(accepted.applied);
    let accepted_generation = accepted.generation.expect("ACCEPT generation");
    let accepted_bytes = fs::read(
        accept
            .store_root
            .join("canon/generations")
            .join(&accepted_generation)
            .join("bundle")
            .join(target_path),
    )
    .expect("read ACCEPT target");
    assert_eq!(accepted_bytes, staged.candidate_bytes);
    assert_ne!(accepted_bytes, seed_bytes);
    let accepted_hash = blake3::hash(&accepted_bytes).to_hex().to_string();
    assert_eq!(accepted_hash, staged.candidate_hash);
    assert_ne!(
        accepted_hash,
        blake3::hash(&seed_bytes).to_hex().to_string()
    );
    let ledger =
        read_ledger(&accept.store_root.join("ledger/verdicts.jsonl")).expect("read ACCEPT ledger");
    let verdict = ledger
        .events
        .iter()
        .find_map(|event| match event {
            LedgerEvent::Verdict(event) if event.payload().verb == VerdictVerb::Accept => {
                Some(event.payload())
            }
            _ => None,
        })
        .expect("read ACCEPT ledger event");
    assert_eq!(verdict.candidate_hash, staged.candidate_hash);
    assert_eq!(
        verdict.canonical_result.target_new_hash.as_deref(),
        Some(staged.candidate_hash.as_str())
    );
    assert!(accept.verify_level4().is_ok());

    let rewrite = ProvisionedP3Fixture::with_staged_reject("apply-rewrite");
    let mut exact_rewrite =
        p3_test_candidate_bytes(&rewrite.store_root.join("seed/bundle").join(target_path));
    exact_rewrite.extend_from_slice(b"\nTask 13 exact human rewrite bytes.\n");
    let request = HumanVerdictRequest {
        proposal_id: rewrite.proposal_id.clone(),
        verb: VerdictVerb::Rewrite,
        reason: Reason::new(b"rewrite exact bytes", 4096).expect("rewrite reason"),
        rewrite: Some(
            RewriteBytes::new(exact_rewrite.clone(), 262_144).expect("valid exact human rewrite"),
        ),
    };
    let initial = read_preflight_state(&rewrite.profile_path, &rewrite.store_root, &request, None)
        .expect("read rewrite preflight");
    let runtime = rewrite.runtime();
    let confirmation = format!("APPLY REWRITE {}\n", &rewrite.proposal_id[..12]);
    let mut human = ScriptedHuman::new(Some(confirmation));
    let rewritten = match preflight_human_verdict_with_human(
        initial,
        request,
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap()),
    )
    .expect("preflight REWRITE")
    {
        PreflightVerdict::Confirmed(decision) => {
            sovmem::profile::apply_human_verdict(decision, &runtime).expect("REWRITE must commit")
        }
        PreflightVerdict::Idempotent(_) => panic!("fresh REWRITE cannot be idempotent"),
    };
    assert!(rewritten.applied);
    let rewritten_generation = rewritten.generation.expect("REWRITE generation");
    assert_eq!(
        fs::read(
            rewrite
                .store_root
                .join("canon/generations")
                .join(&rewritten_generation)
                .join("bundle")
                .join(target_path),
        )
        .expect("read REWRITE target"),
        exact_rewrite
    );
    let profile = load_p3_profile(&rewrite.profile_path).expect("load rewrite profile");
    let generation = verify_generation(
        &profile,
        &rewrite
            .store_root
            .join("canon/generations")
            .join(&rewritten_generation),
        VerificationLevel::Three,
    )
    .expect("verify REWRITE generation");
    let query = query_p3_generation(
        &profile,
        &generation,
        &rewrite
            .store_root
            .join("derived/indexes")
            .join(&rewritten_generation),
        "Task 13 exact human rewrite bytes".to_owned(),
        5,
        ExpansionMode::None,
    )
    .expect("query rebuilt REWRITE index");
    assert!(query
        .results
        .iter()
        .any(|hit| hit.id == "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"));
    assert!(rewrite.verify_level4().is_ok());
}

#[test]
fn applied_current_publication_failure_keeps_the_committed_chain_fail_closed() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("applied-current-failure");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::BeforeCurrentRename));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "current publish failure", &runtime)
            .expect_err("post-ledger CURRENT failure must be incomplete"),
        TransactionError::JournalState
    );
    assert_eq!(
        read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("read committed ledger")
            .events
            .len(),
        2
    );
    let profile = load_p3_profile(&fixture.profile_path).expect("load profile");
    assert_eq!(
        inspect_store(&profile, &fixture.store_root)
            .expect_err("post-ledger temporary pointer must be fail closed")
            .code(),
        "transaction.incomplete"
    );
    assert_eq!(
        verify_store_level4(&profile, &fixture.store_root)
            .expect_err("Level 4 must be fail closed")
            .code(),
        "transaction.incomplete"
    );
}

#[test]
fn recovery_lost_token_before_current_rename_preserves_current_and_recovery_evidence() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-late-current-lease");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::BeforeCurrentRename));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "commit accept before recovery current publication",
                &interrupted,
            )
            .expect_err("the committed ACCEPT must stop before CURRENT rename"),
        TransactionError::JournalState
    );

    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("read stale verdict owner");
    let current_path = fixture.store_root.join("canon/CURRENT");
    let current_before = fs::read(&current_path).expect("read CURRENT before recovery");
    let canon_before = fixture.canon_hashes();
    let ledger_path = fixture.store_root.join("ledger/verdicts.jsonl");
    let ledger_before = fs::read(&ledger_path).expect("read committed ledger");
    let runtime = fixture.runtime_with_ops(ReplaceRecoveryOwnerBeforeCurrentRename::new(
        fs::canonicalize(&fixture.store_root)
            .expect("canonicalize fixture store")
            .join("locks/verdict.lock/owner.json"),
    ));
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-15T06:00:00.000000000Z").unwrap()),
    )
    .expect_err("late recovery lease loss must stop before CURRENT publication");

    assert_eq!(error, TransactionError::LeaseChanged(LeaseKind::Verdict));
    assert_eq!(error.code(), "store.locked");
    assert_eq!(fs::read(&current_path).unwrap(), current_before);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(fs::read(&ledger_path).unwrap(), ledger_before);
    assert!(
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
            .expect("recovery evidence must remain parseable")
            .is_some()
    );
}

#[test]
fn recovery_lost_token_before_each_destructive_cleanup_preserves_the_target_evidence() {
    for label in ["cleanup-inventory", "intake-tree", "verified-tree"] {
        let fixture =
            ProvisionedP3Fixture::with_staged_reject(&format!("recovery-late-destructive-{label}"));
        let recovery_lease = fixture
            .runtime()
            .acquire_lease(
                LeaseKind::Verdict,
                &format!("rec-{}", "a".repeat(64)),
                "recovery",
            )
            .expect("acquire protected recovery lease");
        let target = fixture.store_root.join("staging").join(label);
        if label == "cleanup-inventory" {
            fs::write(&target, b"bound cleanup bytes\n").unwrap();
        } else {
            fs::create_dir(&target).unwrap();
            fs::write(target.join("evidence.bin"), b"target evidence\n").unwrap();
        }
        fs::write(
            fixture.store_root.join("locks/verdict.lock/owner.json"),
            b"{}\n",
        )
        .expect("replace owner after target validation");

        let error = match label {
            "cleanup-inventory" => {
                let mut inventory = sovmem::profile::RecoveryCleanupInventory::default();
                inventory
                    .bind_file(target.clone(), b"bound cleanup bytes\n".to_vec())
                    .unwrap();
                inventory
                    .execute_protected(&StdFilesystem, &recovery_lease)
                    .unwrap_err()
            }
            "intake-tree" => sovmem::profile::remove_optional_tree_protected(
                &StdFilesystem,
                &target,
                &recovery_lease,
            )
            .unwrap_err(),
            "verified-tree" => sovmem::profile::remove_verified_tree_protected(
                &StdFilesystem,
                &target,
                &recovery_lease,
            )
            .unwrap_err(),
            _ => unreachable!(),
        };
        assert_eq!(
            error,
            TransactionError::LeaseChanged(LeaseKind::Verdict),
            "{label}"
        );
        assert_eq!(error.code(), "store.locked", "{label}");
        assert!(target.exists(), "{label} target must be retained");
        if target.is_file() {
            assert_eq!(
                fs::read(&target).unwrap(),
                b"bound cleanup bytes\n",
                "{label}"
            );
        } else {
            assert_eq!(
                fs::read(target.join("evidence.bin")).unwrap(),
                b"target evidence\n",
                "{label}"
            );
        }
    }

    let fixture =
        ProvisionedP3Fixture::with_staged_reject("recovery-late-destructive-original-witness");
    let recovery_lease = fixture
        .runtime()
        .acquire_lease(
            LeaseKind::Verdict,
            &format!("rec-{}", "a".repeat(64)),
            "recovery",
        )
        .unwrap();
    let plan = c2a_applying_plan();
    let original_root = fixture
        .store_root
        .join("transactions")
        .join(&plan.transaction_id);
    fs::create_dir(&original_root).unwrap();
    fs::write(
        original_root.join("plan.json"),
        canonical_json_bytes(&plan).unwrap(),
    )
    .unwrap();
    let original = sovmem::profile::read_original_journal_exact(
        &StdFilesystem,
        original_root.clone(),
        plan.transaction_id.clone(),
        false,
    )
    .unwrap();
    fs::write(
        fixture.store_root.join("locks/verdict.lock/owner.json"),
        b"{}\n",
    )
    .unwrap();
    let error = match original.promote_to_cleanup_witness_ignoring_protected(
        &StdFilesystem,
        None,
        &recovery_lease,
    ) {
        Err(error) => error,
        Ok(_) => panic!("late lease loss must block original witness promotion"),
    };
    assert_eq!(error, TransactionError::LeaseChanged(LeaseKind::Verdict));
    assert!(original_root.join("plan.json").is_file());
    assert!(!fixture
        .store_root
        .join("transactions")
        .join(format!(".cleanup-{}", plan.transaction_id))
        .exists());

    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-late-destructive-quarantine");
    let original_lease = fixture
        .runtime()
        .acquire_lease(
            LeaseKind::Verdict,
            &format!("rec-{}", "b".repeat(64)),
            "verdict",
        )
        .unwrap();
    let stale = sovmem::profile::read_optional_stale_lease(
        &StdFilesystem,
        &original_lease.path(),
        RecoveryLeaseKind::Verdict,
    )
    .unwrap()
    .unwrap();
    let quarantined = sovmem::profile::quarantine_stale_lease(
        &StdFilesystem,
        &fs::canonicalize(&fixture.store_root).unwrap(),
        &stale,
        None,
    )
    .unwrap();
    let recovery_lease = fixture
        .runtime()
        .acquire_lease(
            LeaseKind::Verdict,
            &format!("rec-{}", "c".repeat(64)),
            "recovery",
        )
        .unwrap();
    let quarantined_set = sovmem::profile::QuarantinedLeases {
        verdict: Some(quarantined.clone()),
        intake: None,
    };
    let owner_before = fs::read(quarantined.path.join("owner.json")).unwrap();
    fs::write(
        fixture.store_root.join("locks/verdict.lock/owner.json"),
        b"{}\n",
    )
    .unwrap();
    let error = quarantined_set
        .remove_stale_with(&StdFilesystem, &recovery_lease)
        .unwrap_err();
    assert_eq!(error, TransactionError::LeaseChanged(LeaseKind::Verdict));
    assert_eq!(
        fs::read(quarantined.path.join("owner.json")).unwrap(),
        owner_before
    );
}

#[test]
fn recovery_lost_token_before_recovery_journal_publication_or_cleanup_witness_promotion_writes_nothing(
) {
    for label in ["published-marker", "cleanup-witness"] {
        let fixture =
            ProvisionedP3Fixture::with_staged_reject(&format!("recovery-late-journal-{label}"));
        let recovery_lease = fixture
            .runtime()
            .acquire_lease(
                LeaseKind::Verdict,
                &format!("rec-{}", "a".repeat(64)),
                "recovery",
            )
            .expect("acquire protected recovery lease");
        let record = with_test_recovery_id(RecoveryJournalRecord {
            schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
            recovery_id: format!("txn-{}", "b".repeat(64)),
            mode: RecoveryMode::CleanFinalization,
            command_class: "verdict".to_owned(),
            recovered_at: "2026-07-15T06:01:00.000000000Z".to_owned(),
            quarantined_leases: vec![RecoveryLeaseBinding {
                kind: RecoveryLeaseKind::Verdict,
                owner_hash: "c".repeat(64),
            }],
            recovery_lease_owner_hash: recovery_lease.owner_hash().to_owned(),
            original_transaction_id: None,
            original_journal_hash: None,
            observed_ledger_head: None,
            profile_hash: "d".repeat(64),
            policy_hash: "e".repeat(64),
            cleanup_plan: RecoveryCleanupPlan::empty(),
            cleanup_plan_hash: String::new(),
        });
        let journal = RecoveryJournal::create(&fixture.runtime(), record).unwrap();
        if label == "cleanup-witness" {
            journal.write_published(&StdFilesystem).unwrap();
        }
        fs::write(
            fixture.store_root.join("locks/verdict.lock/owner.json"),
            b"{}\n",
        )
        .expect("replace owner after recovery-prefix validation");

        let error = if label == "published-marker" {
            journal
                .write_published_protected(&StdFilesystem, &recovery_lease)
                .unwrap_err()
        } else {
            match journal.promote_to_cleanup_witness_protected(&StdFilesystem, &recovery_lease) {
                Err(error) => error,
                Ok(_) => panic!("late lease loss must block cleanup witness promotion"),
            }
        };
        assert_eq!(
            error,
            TransactionError::LeaseChanged(LeaseKind::Verdict),
            "{label}"
        );
        assert_eq!(error.code(), "store.locked", "{label}");
        assert!(journal.root.is_dir(), "{label}");
        assert_eq!(
            fs::read(journal.root.join("recovery.json")).unwrap(),
            journal.bytes,
            "{label}"
        );
        if label == "published-marker" {
            assert!(!journal.root.join("published.json").exists());
        } else {
            assert!(!journal
                .root
                .parent()
                .unwrap()
                .join(format!(".cleanup-{}", journal.record.recovery_id))
                .exists());
        }
    }

    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-late-journal-cleanup-removal");
    let recovery_lease = fixture
        .runtime()
        .acquire_lease(
            LeaseKind::Verdict,
            &format!("rec-{}", "f".repeat(64)),
            "recovery",
        )
        .unwrap();
    let record = with_test_recovery_id(RecoveryJournalRecord {
        schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
        recovery_id: format!("txn-{}", "1".repeat(64)),
        mode: RecoveryMode::CleanFinalization,
        command_class: "verdict".to_owned(),
        recovered_at: "2026-07-15T06:02:00.000000000Z".to_owned(),
        quarantined_leases: vec![RecoveryLeaseBinding {
            kind: RecoveryLeaseKind::Verdict,
            owner_hash: "2".repeat(64),
        }],
        recovery_lease_owner_hash: recovery_lease.owner_hash().to_owned(),
        original_transaction_id: None,
        original_journal_hash: None,
        observed_ledger_head: None,
        profile_hash: "3".repeat(64),
        policy_hash: "4".repeat(64),
        cleanup_plan: RecoveryCleanupPlan::empty(),
        cleanup_plan_hash: String::new(),
    });
    let journal = RecoveryJournal::create(&fixture.runtime(), record).unwrap();
    journal.write_published(&StdFilesystem).unwrap();
    let witness = journal
        .promote_to_cleanup_witness_protected(&StdFilesystem, &recovery_lease)
        .unwrap();
    let recovery_before = fs::read(witness.root.join("recovery.json")).unwrap();
    fs::write(
        fixture.store_root.join("locks/verdict.lock/owner.json"),
        b"{}\n",
    )
    .unwrap();
    let error = witness
        .remove_cleanup_witness_with_protected(&StdFilesystem, &recovery_lease)
        .unwrap_err();
    assert_eq!(error, TransactionError::LeaseChanged(LeaseKind::Verdict));
    assert_eq!(
        fs::read(witness.root.join("recovery.json")).unwrap(),
        recovery_before
    );
}

#[test]
fn recovery_lost_token_immediately_before_initial_recovery_journal_create_is_store_locked() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-prefix-initial-create");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "initial prefix lease loss",
                &interrupted
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let runtime = fixture.runtime_with_ops(ReplaceRecoveryOwnerAfterMatchingRead::new(
        fs::canonicalize(&fixture.store_root)
            .unwrap()
            .join("locks/verdict.lock/owner.json"),
        ".recovery-intake-",
        1,
    ));
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-15T06:10:00.000000000Z").unwrap()),
    )
    .unwrap_err();

    assert_eq!(error, TransactionError::LeaseChanged(LeaseKind::Verdict));
    assert_eq!(error.code(), "store.locked");
    assert!(
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn recovery_lost_token_during_bootstrap_before_intake_quarantine_or_create_is_store_locked() {
    for (label, fragment, occurrence) in [
        ("before-intake-quarantine", "prepared.json", 1),
        ("before-create", ".recovery-intake-", 1),
    ] {
        let fixture =
            ProvisionedP3Fixture::with_staged_reject(&format!("recovery-prefix-bootstrap-{label}"));
        let (original_owner_id, _) = crash_after_recovery_lease_acquisition(&fixture);
        let runtime = fixture.runtime_with_ops(ReplaceRecoveryOwnerAfterMatchingRead::new(
            fs::canonicalize(&fixture.store_root)
                .unwrap()
                .join("locks/verdict.lock/owner.json"),
            fragment,
            occurrence,
        ));
        let mut human = ScriptedHuman::new(Some(format!("RECOVER {original_owner_id}\n")));

        let error = recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &runtime,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-15T06:11:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err();

        assert_eq!(
            error,
            TransactionError::LeaseChanged(LeaseKind::Verdict),
            "{label}"
        );
        assert_eq!(error.code(), "store.locked", "{label}");
        assert!(RecoveryJournal::discover(
            &StdFilesystem,
            &fixture.store_root.join("transactions")
        )
        .unwrap()
        .is_none());
    }
}

#[test]
fn recovery_lost_token_during_unpublished_or_published_prefix_validation_is_store_locked() {
    for published in [false, true] {
        let fixture = ProvisionedP3Fixture::with_staged_reject(&format!(
            "recovery-prefix-journal-{published}"
        ));
        let journal = crash_after_recovery_journal(&fixture);
        if published {
            journal.write_published(&StdFilesystem).unwrap();
        }
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let snapshot =
            sovmem::profile::store::inspect_store_with_transients(&profile, &fixture.store_root)
                .unwrap();
        let runtime = fixture.runtime_with_ops(ReplaceRecoveryOwnerAfterMatchingRead::new(
            fs::canonicalize(&fixture.store_root)
                .unwrap()
                .join("locks/verdict.lock/owner.json"),
            "locks/verdict.lock/owner.json",
            1,
        ));

        let error =
            match validate_recovery_prefix(&profile, &snapshot, &runtime, &journal, published) {
                Err(error) => error,
                Ok(_) => panic!("late prefix lease loss must fail: {published}"),
            };
        assert_eq!(
            error,
            TransactionError::LeaseChanged(LeaseKind::Verdict),
            "published={published}"
        );
        assert_eq!(error.code(), "store.locked", "published={published}");
    }
}

#[test]
fn recovery_lost_token_during_cleanup_witness_prefix_validation_is_store_locked() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-prefix-cleanup-witness");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "cleanup prefix lease loss",
                &interrupted
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterRecoveryCleanupWitness));
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-15T06:12:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );
    let witness =
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
            .unwrap()
            .unwrap();
    assert!(witness.cleanup_form);
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    let snapshot =
        sovmem::profile::store::inspect_store_with_transients(&profile, &fixture.store_root)
            .unwrap();
    let runtime = fixture.runtime_with_ops(ReplaceRecoveryOwnerAfterMatchingRead::new(
        fs::canonicalize(&fixture.store_root)
            .unwrap()
            .join("locks/verdict.lock/owner.json"),
        "locks/verdict.lock/owner.json",
        1,
    ));

    let error = match sovmem::profile::validate_recovery_cleanup_witness(
        &profile, &snapshot, &runtime, &witness,
    ) {
        Err(error) => error,
        Ok(_) => panic!("late cleanup-prefix lease loss must fail"),
    };
    assert_eq!(error, TransactionError::LeaseChanged(LeaseKind::Verdict));
    assert_eq!(error.code(), "store.locked");
}

#[test]
fn recovery_stale_verdict_reader_keeps_its_first_valid_owner_until_tail_verification() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-tail-stale-owner-single-read");
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert!(fixture
        .apply_with_runtime(
            VerdictVerb::Accept,
            "prepare terminal recovery lease tail",
            &interrupted_apply,
        )
        .is_err());
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("read stale verdict owner before recovery");
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterRecoveryVerification));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-21T06:14:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );

    let owner_path = fs::canonicalize(&fixture.store_root)
        .expect("canonicalize fixture root")
        .join("locks/verdict.lock/owner.json");
    let runtime = fixture.runtime_with_ops(ReplaceRecoveryOwnerAfterMatchingRead::new(
        owner_path.clone(),
        "locks/verdict.lock/owner.json",
        1,
    ));
    let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
    let snapshot =
        sovmem::profile::store::inspect_store_with_transients(&profile, &fixture.store_root)
            .expect("inspect clean-finalization tail");

    let stale = sovmem::profile::RecoveryStaleLeases::read(&runtime.ops, &snapshot.paths)
        .expect("the first valid recovery owner must remain classifiable");
    assert_eq!(
        stale
            .and_then(|leases| leases.verdict)
            .expect("the recovery verdict lease remains visible")
            .owner
            .command_class(),
        "recovery"
    );
    assert_eq!(fs::read(owner_path).unwrap(), b"{}\n");
}

#[test]
fn recovery_lost_token_at_clean_finalization_tail_verification_is_store_locked() {
    let fixture = ProvisionedP3Fixture::with_staged_reject(
        "recovery-clean-finalization-tail-verification-lease-loss",
    );
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert!(fixture
        .apply_with_runtime(
            VerdictVerb::Accept,
            "prepare terminal recovery lease tail",
            &interrupted_apply,
        )
        .is_err());
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut first_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterRecoveryVerification));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut first_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-15T06:13:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );
    assert!(fixture.transactions_empty());

    let runtime = fixture.runtime_with_ops(ReplaceRecoveryOwnerAfterMatchingRead::new(
        fs::canonicalize(&fixture.store_root)
            .unwrap()
            .join("locks/verdict.lock/owner.json"),
        "locks/verdict.lock/owner.json",
        1,
    ));
    let mut retry_human = ScriptedHuman::new(None);
    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &runtime,
        &mut retry_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-15T06:14:00.000000000Z").unwrap()),
    )
    .unwrap_err();

    assert_eq!(error, TransactionError::LeaseChanged(LeaseKind::Verdict));
    assert_eq!(error.code(), "store.locked");
    assert!(fixture.store_root.join("locks/verdict.lock").exists());
}

#[test]
fn recovery_lost_token_after_tail_lease_check_is_store_locked() {
    let fixture = ProvisionedP3Fixture::with_staged_reject(
        "recovery-clean-finalization-tail-post-check-lease-loss",
    );
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert!(fixture
        .apply_with_runtime(
            VerdictVerb::Accept,
            "prepare terminal recovery lease tail",
            &interrupted_apply,
        )
        .is_err());
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut first_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterRecoveryVerification));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut first_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-17T00:01:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );
    assert!(fixture.transactions_empty());
    let current_before = fs::read(fixture.store_root.join("canon/CURRENT")).unwrap();

    let runtime = fixture.runtime_with_ops(ReplaceRecoveryOwnerAfterMatchingRead::new(
        fs::canonicalize(&fixture.store_root)
            .unwrap()
            .join("locks/verdict.lock/owner.json"),
        "locks/verdict.lock/owner.json",
        2,
    ));
    let mut retry_human = ScriptedHuman::new(None);
    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &runtime,
        &mut retry_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-17T00:02:00.000000000Z").unwrap()),
    )
    .unwrap_err();

    assert_eq!(error, TransactionError::LeaseChanged(LeaseKind::Verdict));
    assert_eq!(error.code(), "store.locked");
    assert!(fixture.store_root.join("locks/verdict.lock").exists());
    assert_eq!(
        fs::read(fixture.store_root.join("canon/CURRENT")).unwrap(),
        current_before
    );
}

#[test]
fn recovery_lost_token_at_bootstrap_second_verification_is_store_locked() {
    let fixture = ProvisionedP3Fixture::with_staged_reject(
        "recovery-bootstrap-second-verification-lease-loss",
    );
    let (original_owner_id, _) = crash_after_recovery_lease_acquisition(&fixture);
    let runtime = fixture.runtime_with_ops(ReplaceRecoveryOwnerAfterMatchingRead::new(
        fs::canonicalize(&fixture.store_root)
            .unwrap()
            .join("locks/verdict.lock/owner.json"),
        "locks/verdict.lock/owner.json",
        2,
    ));
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {original_owner_id}\n")));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-15T06:15:00.000000000Z").unwrap()),
    )
    .unwrap_err();

    assert_eq!(error, TransactionError::LeaseChanged(LeaseKind::Verdict));
    assert_eq!(error.code(), "store.locked");
    assert!(
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
            .unwrap()
            .is_none()
    );
}

fn assert_applied_precommit_fault(point: FaultPoint) {
    let fixture = ProvisionedP3Fixture::with_staged_reject(&format!("applied-pre-{point:?}"));
    let runtime = fixture.runtime_with_fault(FaultPlan::at(point));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "precommit fault", &runtime)
            .expect_err("injected precommit fault must stop the transaction"),
        TransactionError::FaultInjected,
        "{point:?}"
    );
    assert!(
        read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("read authoritative ledger")
            .events
            .is_empty(),
        "{point:?}"
    );
    assert_eq!(
        fs::read(fixture.store_root.join("canon/CURRENT")).expect("read seed CURRENT"),
        b"seed\n",
        "{point:?}"
    );
    assert_publicly_incomplete(&fixture);
}

fn assert_applied_postcommit_fault(point: FaultPoint) {
    let fixture = ProvisionedP3Fixture::with_staged_reject(&format!("applied-post-{point:?}"));
    let runtime = fixture.runtime_with_fault(FaultPlan::at(point));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "postcommit fault", &runtime)
            .expect_err("injected postcommit fault must retain recovery evidence"),
        TransactionError::JournalState,
        "{point:?}"
    );
    assert_eq!(
        read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("read committed ledger")
            .events
            .len(),
        2,
        "{point:?}"
    );
    assert_publicly_incomplete(&fixture);
}

macro_rules! applied_precommit_fault_test {
    ($name:ident, $point:expr) => {
        #[test]
        fn $name() {
            assert_applied_precommit_fault($point);
        }
    };
}

applied_precommit_fault_test!(
    applied_target_staging_fault_preserves_seed_head,
    FaultPoint::AfterTargetBytesStaged
);
applied_precommit_fault_test!(
    applied_judgment_staging_fault_preserves_seed_head,
    FaultPoint::AfterJudgmentBytesStaged
);
applied_precommit_fault_test!(
    applied_manifest_sync_fault_preserves_seed_head,
    FaultPoint::AfterManifestSync
);
applied_precommit_fault_test!(
    applied_level_three_fault_preserves_seed_head,
    FaultPoint::AfterLevelThreeSelfCheck
);
applied_precommit_fault_test!(
    applied_reindex_build_fault_preserves_seed_head,
    FaultPoint::DuringIndexBuild
);
applied_precommit_fault_test!(
    applied_index_self_check_fault_preserves_seed_head,
    FaultPoint::AfterIndexSelfCheck
);
applied_precommit_fault_test!(
    applied_receipt_fault_preserves_seed_head,
    FaultPoint::AfterReindexReceiptSync
);
applied_precommit_fault_test!(
    applied_generation_rename_fault_preserves_seed_head,
    FaultPoint::AfterGenerationRename
);
applied_precommit_fault_test!(
    applied_index_rename_fault_preserves_seed_head,
    FaultPoint::AfterDerivedIndexRename
);
applied_precommit_fault_test!(
    applied_prepared_marker_fault_preserves_seed_head,
    FaultPoint::AfterPreparedMarker
);
applied_precommit_fault_test!(
    applied_ledger_temporary_fault_preserves_seed_head,
    FaultPoint::AfterLedgerTemporarySync
);
applied_precommit_fault_test!(
    applied_ledger_rename_boundary_fault_preserves_seed_head,
    FaultPoint::BeforeLedgerRename
);

macro_rules! applied_postcommit_fault_test {
    ($name:ident, $point:expr) => {
        #[test]
        fn $name() {
            assert_applied_postcommit_fault($point);
        }
    };
}

applied_postcommit_fault_test!(
    applied_ledger_rename_fault_keeps_event_fail_closed,
    FaultPoint::AfterLedgerRename
);
applied_postcommit_fault_test!(
    applied_current_before_rename_fault_keeps_event_fail_closed,
    FaultPoint::BeforeCurrentRename
);
applied_postcommit_fault_test!(
    applied_current_after_rename_fault_keeps_event_fail_closed,
    FaultPoint::AfterCurrentRename
);
applied_postcommit_fault_test!(
    applied_published_marker_fault_keeps_event_fail_closed,
    FaultPoint::BeforePublishedMarker
);
applied_postcommit_fault_test!(
    applied_cleanup_fault_keeps_event_fail_closed,
    FaultPoint::DuringCleanup
);

#[test]
fn every_crash_state_has_one_deterministic_recovery_result() {
    for (point, committed_before) in [
        (FaultPoint::AfterLeaseCreation, false),
        (FaultPoint::AfterTransactionJournalCreation, false),
        (FaultPoint::AfterTargetBytesStaged, false),
        (FaultPoint::AfterJudgmentBytesStaged, false),
        (FaultPoint::AfterManifestSync, false),
        (FaultPoint::AfterLevelThreeSelfCheck, false),
        (FaultPoint::DuringIndexBuild, false),
        (FaultPoint::AfterIndexSelfCheck, false),
        (FaultPoint::AfterReindexReceiptSync, false),
        (FaultPoint::AfterGenerationRename, false),
        (FaultPoint::AfterDerivedIndexRename, false),
        (FaultPoint::AfterPreparedMarker, false),
        (FaultPoint::AfterLedgerTemporarySync, false),
        (FaultPoint::BeforeLedgerRename, false),
        (FaultPoint::AfterLedgerRename, true),
        (FaultPoint::BeforeCurrentRename, true),
        (FaultPoint::AfterCurrentRename, true),
        (FaultPoint::BeforePublishedMarker, true),
        (FaultPoint::DuringCleanup, true),
    ] {
        let fixture =
            ProvisionedP3Fixture::with_staged_reject(&format!("recovery-matrix-{point:?}"));
        let interrupted = fixture.runtime_with_fault(FaultPlan::at(point));
        assert!(
            fixture
                .apply_with_runtime(VerdictVerb::Accept, "recovery matrix", &interrupted)
                .is_err(),
            "fault must interrupt apply: {point:?}"
        );

        let owner = read_canonical_owner(
            &StdFilesystem,
            &fixture.store_root.join("locks/verdict.lock/owner.json"),
            LeaseKind::Verdict,
        )
        .expect("interrupted apply must retain a verified stale verdict owner");
        let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
        let outcome = recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &fixture.runtime(),
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        );
        let outcome = outcome.unwrap_or_else(|error| {
            panic!("recovery must classify {point:?} deterministically: {error:?}")
        });

        assert_eq!(outcome.rolled_forward, committed_before, "{point:?}");
        let profile = load_p3_profile(&fixture.profile_path).expect("load recovered profile");
        assert!(
            fixture.verify_level4().is_ok() || inspect_store(&profile, &fixture.store_root).is_ok(),
            "{point:?}"
        );
        assert!(fixture.transactions_empty(), "{point:?}");
        assert!(fixture.leases_absent(), "{point:?}");
    }
}

#[test]
fn uncommitted_recovery_removes_finalized_generation_index_and_receipt() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-uncommitted-finalized");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert!(fixture
        .apply_with_runtime(VerdictVerb::Accept, "uncommitted recovery", &interrupted)
        .is_err());
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .unwrap();

    assert!(!outcome.rolled_forward);
    for directory in [
        fixture.store_root.join("canon/generations"),
        fixture.store_root.join("derived/indexes"),
        fixture.store_root.join("evidence/reindex"),
    ] {
        assert!(
            fs::read_dir(directory).unwrap().next().is_none(),
            "uncommitted recovery must remove every transaction-bound artifact"
        );
    }
}

#[test]
fn c2b2_recovery_rejects_foreign_valid_successor_before_any_deletion() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-foreign-successor");
    let foreign = ProvisionedP3Fixture::with_staged_reject("recovery-foreign-successor-src");
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "interrupted successor", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let foreign_interrupted =
        foreign.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        foreign
            .apply_with_runtime(
                VerdictVerb::Accept,
                "foreign interrupted successor",
                &foreign_interrupted
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );

    let temporary = fs::read_dir(fixture.store_root.join("ledger"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".verdicts-"))
        })
        .expect("interrupted transaction must retain its successor temporary");
    let foreign_temporary = fs::read_dir(foreign.store_root.join("ledger"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".verdicts-"))
        })
        .expect("foreign interrupted transaction must retain a valid successor temporary");
    let foreign_bytes = fs::read(&foreign_temporary).unwrap();
    assert!(
        read_ledger(&foreign_temporary).is_ok(),
        "fixture must inject a syntactically valid but unbound successor"
    );
    fs::write(&temporary, &foreign_bytes).unwrap();
    assert!(
        read_ledger(&temporary).is_ok(),
        "renaming the path must not be the authority for a successor temporary"
    );
    let canon_before = fixture.canon_hashes();
    let indexes_before = fixture.index_hashes();
    let mut receipts_before = fs::read_dir(fixture.store_root.join("evidence/reindex"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_owned(),
                fs::read(&path).expect("read applying receipt before recovery"),
            )
        })
        .collect::<Vec<_>>();
    receipts_before.sort_by(|left, right| left.0.cmp(&right.0));

    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("a foreign but valid successor must leave all residual evidence intact");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(error.code(), "transaction.recovery_ambiguous");
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(fixture.index_hashes(), indexes_before);
    let mut receipts_after = fs::read_dir(fixture.store_root.join("evidence/reindex"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_owned(),
                fs::read(&path).expect("read applying receipt after recovery"),
            )
        })
        .collect::<Vec<_>>();
    receipts_after.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(receipts_after, receipts_before);
    assert_eq!(fs::read(&temporary).unwrap(), foreign_bytes);
    assert!(temporary.exists());
}

#[test]
fn c2b2_plan_only_receipt_residue_blocks_cleanup_before_any_deletion() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-plan-only-receipt");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterIndexSelfCheck));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "plan-only receipt residue",
                &interrupted
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .expect("plan-only transaction")
        .unwrap()
        .path();
    let plan: TransactionPlan =
        serde_json::from_slice(&fs::read(transaction.join("plan.json")).unwrap()).unwrap();
    assert!(!transaction.join("prepared.json").exists());
    let receipt = fixture
        .store_root
        .join("evidence/reindex")
        .join(format!("{}.json", plan.verdict_id));
    fs::write(&receipt, b"unprepared receipt residue\n").unwrap();
    let canon_before = fixture.canon_hashes();
    let indexes_before = fixture.index_hashes();
    let receipt_before = fs::read(&receipt).unwrap();

    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("plan-only receipt residue must remain fail-closed");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(fixture.index_hashes(), indexes_before);
    assert_eq!(fs::read(&receipt).unwrap(), receipt_before);
}

#[test]
fn c2b2_plan_only_verified_crash_resumes_by_removing_the_bound_journal() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-plan-only-resume");
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterTransactionJournalCreation));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "plan-only valid crash", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect("a plan-only crash without residue must resume deterministically");

    assert_eq!(outcome.mode, RecoveryMode::VerdictTransaction);
    assert!(!outcome.rolled_forward);
    assert!(fixture.transactions_empty());
    assert!(fixture.leases_absent());
}

#[test]
fn c2b2_plan_only_recovery_rejects_owner_hash_mismatch_before_takeover() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-plan-owner-mismatch");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanSync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "plan owner mismatch", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transactions = fixture.store_root.join("transactions");
    let journal = discover_original_journal(&StdFilesystem, &transactions)
        .expect("discover canonical plan-only journal");
    let plan_path = journal.root.as_ref().unwrap().join("plan.json");
    let mut plan: TransactionPlan = serde_json::from_slice(&fs::read(&plan_path).unwrap()).unwrap();
    plan.owner_hashes = vec!["f".repeat(64), "f".repeat(64)];
    fs::write(&plan_path, canonical_json_bytes(&plan).unwrap()).unwrap();
    let journal = discover_original_journal(&StdFilesystem, &transactions)
        .expect("the substituted plan remains canonical");
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    let snapshot =
        sovmem::profile::inspect_store_with_transients(&profile, &fixture.store_root).unwrap();
    let stale = sovmem::profile::RecoveryStaleLeases::read(&StdFilesystem, &snapshot.paths)
        .unwrap()
        .unwrap();

    assert_eq!(
        stale.validate_against(&journal).unwrap_err(),
        TransactionError::RecoveryAmbiguous
    );
}

#[test]
fn c2b2_plan_only_recovery_accepts_exact_plan_owner_hashes() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-plan-owner-match");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanSync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "plan owner match", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    let snapshot =
        sovmem::profile::inspect_store_with_transients(&profile, &fixture.store_root).unwrap();
    let stale = sovmem::profile::RecoveryStaleLeases::read(&StdFilesystem, &snapshot.paths)
        .unwrap()
        .unwrap();
    let journal = discover_original_journal(&StdFilesystem, &snapshot.paths.transactions)
        .expect("discover canonical plan-only journal");

    stale
        .validate_against(&journal)
        .expect("exact plan owner hashes must bind plan-only recovery");
}

#[test]
fn plan_publication_crash_states_are_exact_and_human_recovery_normalizes_them() {
    for (point, expect_prep, expect_root, expect_plan) in [
        (FaultPoint::AfterPlanPrepSync, true, false, false),
        (FaultPoint::AfterPlanFinalDirectorySync, true, true, false),
        (FaultPoint::AfterPlanHardLinkSync, true, true, true),
        (FaultPoint::AfterPlanPrepRemoval, false, true, true),
        (
            FaultPoint::AfterTransactionJournalCreation,
            false,
            true,
            true,
        ),
    ] {
        let fixture =
            ProvisionedP3Fixture::with_staged_reject(&format!("plan-publication-crash-{point:?}"));
        let interrupted = fixture.runtime_with_fault(FaultPlan::at(point));
        assert_eq!(
            fixture
                .apply_with_runtime(VerdictVerb::Accept, "plan publication crash", &interrupted)
                .unwrap_err(),
            TransactionError::FaultInjected,
            "{point:?}"
        );

        let transactions = fixture.store_root.join("transactions");
        let entries = sorted_directory_entries(&transactions).unwrap();
        let prep = entries.iter().find(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".plan-") && name.ends_with(".json"))
        });
        let root = entries.iter().find(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(sovmem::profile::valid_transaction_id)
        });
        assert_eq!(prep.is_some(), expect_prep, "{point:?}");
        assert_eq!(root.is_some(), expect_root, "{point:?}");
        assert_eq!(
            root.is_some_and(|entry| entry.path().join("plan.json").is_file()),
            expect_plan,
            "{point:?}"
        );
        if point == FaultPoint::AfterPlanHardLinkSync {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let prep_metadata = fs::metadata(prep.unwrap().path()).unwrap();
                let plan_metadata = fs::metadata(root.unwrap().path().join("plan.json")).unwrap();
                assert_eq!(prep_metadata.dev(), plan_metadata.dev());
                assert_eq!(prep_metadata.ino(), plan_metadata.ino());
            }
        }
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete",
            "{point:?}"
        );

        let owner = read_canonical_owner(
            &StdFilesystem,
            &fixture.store_root.join("locks/verdict.lock/owner.json"),
            LeaseKind::Verdict,
        )
        .unwrap();
        let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
        let outcome = recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &fixture.runtime(),
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_or_else(|error| panic!("recover {point:?}: {error:?}"));
        assert_eq!(outcome.mode, RecoveryMode::VerdictTransaction, "{point:?}");
        assert!(!outcome.rolled_forward, "{point:?}");
        assert!(fixture.transactions_empty(), "{point:?}");
        assert!(fixture.leases_absent(), "{point:?}");
    }
}

#[test]
fn plan_publication_discovery_rejects_all_ambiguous_prefixes_without_cleanup() {
    let p3_fixture = |name: &str| {
        let fixture = ProvisionedP3Fixture::with_staged_reject(name);
        let interrupted =
            fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanHardLinkSync));
        assert_eq!(
            fixture
                .apply_with_runtime(
                    VerdictVerb::Accept,
                    "ambiguous plan publication",
                    &interrupted
                )
                .unwrap_err(),
            TransactionError::FaultInjected
        );
        fixture
    };

    let fixture = p3_fixture("plan-prefix-mismatched-bytes");
    let transactions = fixture.store_root.join("transactions");
    let entries = sorted_directory_entries(&transactions).unwrap();
    let prep = entries
        .iter()
        .find(|entry| entry.path().is_file())
        .unwrap()
        .path();
    let root = entries
        .iter()
        .find(|entry| entry.path().is_dir())
        .unwrap()
        .path();
    fs::remove_file(root.join("plan.json")).unwrap();
    let mut mismatched = fs::read(&prep).unwrap();
    mismatched.extend_from_slice(b"\n");
    fs::write(root.join("plan.json"), mismatched).unwrap();
    assert!(matches!(
        discover_original_journal(&StdFilesystem, &transactions),
        Err(TransactionError::RecoveryAmbiguous)
    ));
    assert!(prep.exists() && root.join("plan.json").exists());

    let fixture = p3_fixture("plan-prefix-different-inode");
    let transactions = fixture.store_root.join("transactions");
    let entries = sorted_directory_entries(&transactions).unwrap();
    let prep = entries
        .iter()
        .find(|entry| entry.path().is_file())
        .unwrap()
        .path();
    let root = entries
        .iter()
        .find(|entry| entry.path().is_dir())
        .unwrap()
        .path();
    let bytes = fs::read(&prep).unwrap();
    fs::remove_file(root.join("plan.json")).unwrap();
    fs::write(root.join("plan.json"), bytes).unwrap();
    assert!(matches!(
        discover_original_journal(&StdFilesystem, &transactions),
        Err(TransactionError::RecoveryAmbiguous)
    ));
    assert!(prep.exists() && root.join("plan.json").exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let fixture = ProvisionedP3Fixture::with_staged_reject("plan-prefix-symlink-prep");
        let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanPrepSync));
        assert_eq!(
            fixture
                .apply_with_runtime(VerdictVerb::Accept, "symlink prep", &interrupted)
                .unwrap_err(),
            TransactionError::FaultInjected
        );
        let transactions = fixture.store_root.join("transactions");
        let prep = sorted_directory_entries(&transactions).unwrap()[0].path();
        let target = fixture.store_root.join("foreign-plan.json");
        fs::rename(&prep, &target).unwrap();
        symlink(&target, &prep).unwrap();
        assert!(matches!(
            discover_original_journal(&StdFilesystem, &transactions),
            Err(TransactionError::RecoveryAmbiguous)
        ));
        assert!(fs::symlink_metadata(&prep)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    let fixture = ProvisionedP3Fixture::with_staged_reject("plan-prefix-wrong-id");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanPrepSync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "wrong prep id", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transactions = fixture.store_root.join("transactions");
    let prep = sorted_directory_entries(&transactions).unwrap()[0].path();
    let wrong = transactions.join(format!(".plan-txn-{}.json", "f".repeat(64)));
    fs::rename(&prep, &wrong).unwrap();
    assert!(matches!(
        discover_original_journal(&StdFilesystem, &transactions),
        Err(TransactionError::RecoveryAmbiguous)
    ));
    assert!(wrong.exists());

    let fixture = ProvisionedP3Fixture::with_staged_reject("plan-prefix-duplicate-prep");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanPrepSync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "duplicate prep", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transactions = fixture.store_root.join("transactions");
    let prep = sorted_directory_entries(&transactions).unwrap()[0].path();
    let duplicate = transactions.join(format!(".plan-txn-{}.json", "e".repeat(64)));
    fs::copy(&prep, &duplicate).unwrap();
    assert!(matches!(
        discover_original_journal(&StdFilesystem, &transactions),
        Err(TransactionError::RecoveryAmbiguous)
    ));
    assert!(prep.exists() && duplicate.exists());

    let fixture = ProvisionedP3Fixture::with_staged_reject("plan-prefix-unexpected-child");
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanFinalDirectorySync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "unexpected child", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transactions = fixture.store_root.join("transactions");
    let root = sorted_directory_entries(&transactions)
        .unwrap()
        .into_iter()
        .find(|entry| entry.path().is_dir())
        .unwrap()
        .path();
    fs::write(root.join("unexpected.json"), b"{}\n").unwrap();
    assert!(matches!(
        discover_original_journal(&StdFilesystem, &transactions),
        Err(TransactionError::RecoveryAmbiguous)
    ));
    assert!(root.join("unexpected.json").exists());
}

#[test]
fn recovery_normalization_crashes_remain_in_p2_through_p4_and_resume_exactly() {
    for (initial, recovery_fault, expect_prep, expect_plan) in [
        (
            FaultPoint::AfterPlanPrepSync,
            FaultPoint::AfterPlanFinalDirectorySync,
            true,
            false,
        ),
        (
            FaultPoint::AfterPlanFinalDirectorySync,
            FaultPoint::AfterPlanHardLinkSync,
            true,
            true,
        ),
        (
            FaultPoint::AfterPlanHardLinkSync,
            FaultPoint::AfterPlanPrepRemoval,
            false,
            true,
        ),
    ] {
        let fixture = ProvisionedP3Fixture::with_staged_reject(&format!(
            "recovery-plan-normalization-{initial:?}-{recovery_fault:?}"
        ));
        let interrupted = fixture.runtime_with_fault(FaultPlan::at(initial));
        assert_eq!(
            fixture
                .apply_with_runtime(VerdictVerb::Accept, "normalization crash", &interrupted)
                .unwrap_err(),
            TransactionError::FaultInjected
        );
        let owner = read_canonical_owner(
            &StdFilesystem,
            &fixture.store_root.join("locks/verdict.lock/owner.json"),
            LeaseKind::Verdict,
        )
        .unwrap();
        let confirmation = format!("RECOVER {}\n", owner.owner_id);
        let mut human = ScriptedHuman::new(Some(confirmation.clone()));
        let recovery_runtime = fixture.runtime_with_fault(FaultPlan::at(recovery_fault));
        assert_eq!(
            recover_store_with_human(
                RecoveryRequest {
                    profile_path: fixture.profile_path.clone(),
                    store_root: fixture.store_root.clone(),
                },
                &recovery_runtime,
                &mut human,
                &FixedClock(
                    sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
                ),
            )
            .unwrap_err(),
            TransactionError::FaultInjected,
            "{recovery_fault:?}"
        );
        assert!(RecoveryJournal::discover(
            &StdFilesystem,
            &fixture.store_root.join("transactions")
        )
        .unwrap()
        .is_none());
        let entries = sorted_directory_entries(&fixture.store_root.join("transactions")).unwrap();
        assert_eq!(
            entries.iter().any(|entry| entry.path().is_file()),
            expect_prep,
            "{recovery_fault:?}"
        );
        let root = entries
            .iter()
            .find(|entry| entry.path().is_dir())
            .expect("P2-P4 retain the final transaction root")
            .path();
        assert_eq!(
            root.join("plan.json").is_file(),
            expect_plan,
            "{recovery_fault:?}"
        );

        let mut human = ScriptedHuman::new(Some(confirmation));
        let outcome = recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &fixture.runtime(),
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:01.000000000Z").unwrap(),
            ),
        )
        .unwrap_or_else(|error| panic!("resume {recovery_fault:?}: {error:?}"));
        assert_eq!(outcome.mode, RecoveryMode::VerdictTransaction);
        assert!(!outcome.rolled_forward);
        assert!(fixture.transactions_empty());
        assert!(fixture.leases_absent());
    }
}

#[test]
fn recovery_revalidates_p3_identity_immediately_before_prep_removal() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-p3-delete-race");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanHardLinkSync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "P3 delete race", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transactions = fixture.store_root.join("transactions");
    let entries = sorted_directory_entries(&transactions).unwrap();
    let prep = entries
        .iter()
        .find(|entry| entry.path().is_file())
        .unwrap()
        .path();
    let final_plan = entries
        .iter()
        .find(|entry| entry.path().is_dir())
        .unwrap()
        .path()
        .join("plan.json");
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let runtime = fixture.runtime_with_ops(SwapPlanBeforePrepRemoval::new(
        final_plan.clone(),
        fixture.store_root.join("locks/verdict.lock/owner.json"),
    ));
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("a P3 inode swap at prep deletion must fail closed");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert!(
        prep.is_file(),
        "authenticated prep evidence must be retained"
    );
    assert!(
        final_plan.is_file(),
        "mutated final plan evidence must be retained"
    );
    assert!(fixture.store_root.join("locks/verdict.lock").is_dir());
}

#[test]
fn c2b2_empty_transaction_root_with_current_residue_blocks_deletion() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-empty-root-residue");
    let transaction_id = format!("txn-{}", "a".repeat(64));
    let root = fixture
        .store_root
        .join("transactions")
        .join(&transaction_id);
    let current = fixture
        .store_root
        .join("canon")
        .join(format!(".current-{transaction_id}"));
    fs::create_dir(&root).unwrap();
    fs::write(&current, b"gen-unbound\n").unwrap();

    let owner_id = format!("rec-{}", "b".repeat(64));
    let _verdict = fixture
        .runtime()
        .acquire_lease(LeaseKind::Verdict, &owner_id, "verdict")
        .expect("create stale verdict lease");
    let _intake = acquire_trusted_intake_lease(&fixture, &owner_id, "verdict");
    let root_before = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    let current_before = fs::read(&current).unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {owner_id}\n")));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("an empty transaction root may be removed only when no bound residue exists");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(
        fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>(),
        root_before
    );
    assert_eq!(fs::read(&current).unwrap(), current_before);
}

#[test]
fn recovery_rejects_an_unbound_terminal_original_cleanup_before_takeover() {
    let fixture =
        ProvisionedP3Fixture::with_staged_reject("recovery-unbound-terminal-original-cleanup");
    fixture
        .apply(VerdictVerb::Accept, "commit an unrelated accepted verdict")
        .expect("commit fixture verdict");
    let ledger = read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl")).unwrap();
    let transaction_id = ledger
        .events
        .iter()
        .find_map(|event| match event {
            LedgerEvent::Verdict(event) => Some(event.payload().transaction_id.clone()),
            LedgerEvent::Genesis(_) => None,
        })
        .expect("fixture verdict transaction id");
    let cleanup = fixture
        .store_root
        .join("transactions")
        .join(format!(".cleanup-{transaction_id}"));
    fs::create_dir(&cleanup).unwrap();
    let owner_id = format!("rec-{}", "a".repeat(64));
    let _stale = fixture
        .runtime()
        .acquire_lease(LeaseKind::Verdict, &owner_id, "verdict")
        .expect("create unrelated stale verdict lease");
    let _intake = acquire_trusted_intake_lease(&fixture, &owner_id, "verdict");
    let current_before = fs::read(fixture.store_root.join("canon/CURRENT")).unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {owner_id}\n")));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-17T00:00:00.000000000Z").unwrap()),
    )
    .expect_err("an empty cleanup root needs an authenticated stale-owner binding");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert!(
        cleanup.is_dir(),
        "unbound cleanup evidence must be retained"
    );
    assert!(
        fixture.store_root.join("locks/verdict.lock").is_dir(),
        "stale lease must remain before destructive takeover"
    );
    assert_eq!(
        fs::read(fixture.store_root.join("canon/CURRENT")).unwrap(),
        current_before
    );
}

#[test]
fn c2b2_prepared_ledger_only_canonical_residue_blocks_all_cleanup() {
    let fixture = P3TestFixture::with_staged_proposal(
        "recovery-ledger-only-canon-residue",
        VerdictVerb::Reject,
    );
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Reject, "ledger-only residue", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .expect("prepared ledger-only transaction")
        .unwrap()
        .path();
    let plan: TransactionPlan =
        serde_json::from_slice(&fs::read(transaction.join("plan.json")).unwrap()).unwrap();
    let canonical_residue = fixture
        .store_root
        .join("canon/generations")
        .join(format!(".txn-{}", plan.transaction_id));
    fs::create_dir(&canonical_residue).unwrap();
    fs::write(
        canonical_residue.join("foreign.md"),
        b"foreign canonical residue\n",
    )
    .unwrap();
    let temporary = fs::read_dir(fixture.store_root.join("ledger"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".verdicts-"))
        })
        .expect("prepared ledger-only transaction must retain its exact successor");
    let residue_before = fs::read(canonical_residue.join("foreign.md")).unwrap();
    let successor_before = fs::read(&temporary).unwrap();

    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("ledger-only recovery must retain every artifact when canonical residue exists");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(
        fs::read(canonical_residue.join("foreign.md")).unwrap(),
        residue_before
    );
    assert_eq!(fs::read(&temporary).unwrap(), successor_before);
}

#[test]
fn c2b2_prepared_ledger_only_crash_resumes_after_verified_cleanup() {
    let fixture = P3TestFixture::with_staged_proposal(
        "recovery-ledger-only-valid-resume",
        VerdictVerb::Reject,
    );
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Reject, "ledger-only valid crash", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));

    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect("a verified ledger-only successor may be removed during uncommitted recovery");

    assert_eq!(outcome.mode, RecoveryMode::VerdictTransaction);
    assert!(!outcome.rolled_forward);
    assert!(fixture.transactions_empty());
    assert!(fixture.leases_absent());
    assert!(
        read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
            .unwrap()
            .events
            .is_empty(),
        "uncommitted ledger-only recovery must not publish the temporary successor"
    );
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    assert!(inspect_store(&profile, &fixture.store_root).is_ok());
}

#[test]
fn c2b2_prepared_invalid_final_generation_blocks_cleanup_before_any_deletion() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-prepared-invalid-final");
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "prepared invalid generation",
                &interrupted
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .expect("prepared transaction")
        .unwrap()
        .path();
    let plan: TransactionPlan =
        serde_json::from_slice(&fs::read(transaction.join("plan.json")).unwrap()).unwrap();
    let generation_id = plan.generation_id.as_deref().unwrap();
    let target = fixture
        .store_root
        .join("canon/generations")
        .join(generation_id)
        .join("bundle")
        .join(&plan.target_path);
    let mut bytes = fs::read(&target).unwrap();
    bytes.extend_from_slice(b"\ncorrupt prepared generation\n");
    fs::write(&target, bytes).unwrap();
    let temporary = fs::read_dir(fixture.store_root.join("ledger"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".verdicts-"))
        })
        .expect("prepared transaction must retain successor temporary");
    let canon_before = fixture.canon_hashes();
    let indexes_before = fixture.index_hashes();
    let receipt = fixture
        .store_root
        .join("evidence/reindex")
        .join(format!("{}.json", plan.verdict_id));
    let receipt_before = fs::read(&receipt).unwrap();
    let successor_before = fs::read(&temporary).unwrap();

    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("an invalid final generation must retain every prepared residual");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(fixture.index_hashes(), indexes_before);
    assert_eq!(fs::read(&receipt).unwrap(), receipt_before);
    assert_eq!(fs::read(&temporary).unwrap(), successor_before);
}

#[test]
fn c2b2_generation_mutated_after_inventory_blocks_every_cleanup_delete() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-inventory-generation-race");
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "inventory generation mutation",
                &interrupted
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .expect("prepared applying transaction")
        .unwrap()
        .path();
    let plan: TransactionPlan =
        serde_json::from_slice(&fs::read(transaction.join("plan.json")).unwrap()).unwrap();
    let generation = fixture
        .store_root
        .join("canon/generations")
        .join(plan.generation_id.as_deref().unwrap());
    let target = generation.join("bundle").join(&plan.target_path);
    let index = fixture
        .store_root
        .join("derived/indexes")
        .join(plan.generation_id.as_deref().unwrap());
    let receipt = fixture
        .store_root
        .join("evidence/reindex")
        .join(format!("{}.json", plan.verdict_id));
    let temporary = fs::read_dir(fixture.store_root.join("ledger"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".verdicts-"))
        })
        .expect("prepared transaction must retain successor temporary");
    let mutation_target = fs::canonicalize(&target).unwrap();
    let runtime = fixture.runtime_with_ops(MutatePathOnSecondMetadata::new(
        fs::canonicalize(&generation).unwrap(),
        move || {
            let mut bytes = fs::read(&mutation_target)?;
            bytes.extend_from_slice(b"\nmutation after cleanup inventory\n");
            fs::write(&mutation_target, bytes)
        },
    ));
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("a generation changed after inventory validation must block all cleanup");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert!(generation.is_dir());
    assert!(index.is_dir());
    assert!(receipt.is_file());
    assert!(temporary.is_file());
    assert!(
        fs::read(&target)
            .unwrap()
            .ends_with(b"mutation after cleanup inventory\n"),
        "fixture must mutate only after the initial cleanup inventory has been built"
    );
}

#[test]
fn recovery_cleanup_revalidates_typed_generation_after_final_inventory_verification() {
    let fixture = ProvisionedP3Fixture::with_staged_reject(
        "recovery-cleanup-typed-generation-late-insertion",
    );
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "late typed cleanup generation insertion",
                &interrupted,
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let plan: TransactionPlan =
        serde_json::from_slice(&fs::read(transaction.join("plan.json")).unwrap()).unwrap();
    let generation = fixture
        .store_root
        .join("canon/generations")
        .join(plan.generation_id.as_deref().unwrap());
    let receipt = fixture
        .store_root
        .join("evidence/reindex")
        .join(format!("{}.json", plan.verdict_id));
    let inserted = generation.join("unbound-after-final-inventory.txt");
    let runtime = fixture.runtime_with_ops(InsertFileAfterMatchingRead::new(
        fs::canonicalize(&receipt).unwrap(),
        inserted.clone(),
        10,
    ));
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-17T00:03:00.000000000Z").unwrap()),
    )
    .expect_err("a child inserted after production cleanup validation must block deletion");
    assert!(
        matches!(error, TransactionError::RecoveryAmbiguous),
        "a post-validation child must make recovery fail closed"
    );
    assert!(inserted.is_file(), "the foreign child must be retained");
    fs::remove_file(&inserted).unwrap();
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    assert!(
        verify_generation(&profile, &generation, VerificationLevel::Three).is_ok(),
        "production cleanup must not delete any bound generation material after the insertion"
    );
}

#[test]
fn cleanup_plan_injected_file_after_inventory_is_never_deleted() {
    let fixture =
        ProvisionedP3Fixture::with_staged_reject("recovery-cleanup-plan-injected-file-red");
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "cleanup plan injected file",
                &interrupted,
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let plan: TransactionPlan =
        serde_json::from_slice(&fs::read(transaction.join("plan.json")).unwrap()).unwrap();
    let generation = fixture
        .store_root
        .join("canon/generations")
        .join(plan.generation_id.as_deref().unwrap());
    let injected = generation.join("injected-after-cleanup-plan.txt");
    let injected_for_mutation = injected.clone();
    let runtime = fixture.runtime_with_ops(MutatePathOnNthMetadata::new(
        fs::canonicalize(&generation).unwrap(),
        3,
        move || fs::write(&injected_for_mutation, b"uncommitted foreign evidence\n"),
    ));
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("an entry inserted after cleanup planning must fail closed");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert!(
        injected.is_file(),
        "inserted evidence must never be deleted"
    );
    assert!(generation.is_dir(), "bound generation evidence must remain");
}

#[test]
fn cleanup_plan_foreign_entry_after_semantic_verify_is_never_bound_on_either_pass() {
    for (label, receipt_read) in [("revalidation-plan", 4), ("initial-plan", 3)] {
        let fixture = ProvisionedP3Fixture::with_staged_reject(&format!(
            "cleanup-plan-post-semantic-{label}"
        ));
        let interrupted =
            fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
        assert_eq!(
            fixture
                .apply_with_runtime(
                    VerdictVerb::Accept,
                    "post semantic plan mutation",
                    &interrupted,
                )
                .unwrap_err(),
            TransactionError::FaultInjected
        );
        let transaction = fs::read_dir(fixture.store_root.join("transactions"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let plan: TransactionPlan =
            serde_json::from_slice(&fs::read(transaction.join("plan.json")).unwrap()).unwrap();
        let index = fixture
            .store_root
            .join("derived/indexes")
            .join(plan.generation_id.as_deref().unwrap());
        let inserted = index.join(format!("foreign-{label}.bin"));
        let receipt = fixture
            .store_root
            .join("evidence/reindex")
            .join(format!("{}.json", plan.verdict_id));
        let runtime = fixture.runtime_with_ops(InsertFileAfterMatchingRead::new(
            fs::canonicalize(receipt).unwrap(),
            inserted.clone(),
            receipt_read,
        ));
        let owner = read_canonical_owner(
            &StdFilesystem,
            &fixture.store_root.join("locks/verdict.lock/owner.json"),
            LeaseKind::Verdict,
        )
        .unwrap();
        let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
        let error = recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &runtime,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        )
        .expect_err("post-semantic foreign content must fail closed");
        assert_eq!(error, TransactionError::RecoveryAmbiguous, "{label}");
        assert!(inserted.is_file(), "{label} foreign evidence must remain");
        assert!(index.is_dir(), "{label} index root must remain");
    }
}

#[test]
fn cleanup_plan_crash_after_every_step_resumes_only_the_exact_suffix() {
    for interrupt_after in 1..=4 {
        let fixture = ProvisionedP3Fixture::with_staged_reject(&format!(
            "cleanup-plan-step-resume-{interrupt_after}"
        ));
        let recovery_lease = fixture
            .runtime()
            .acquire_lease(
                LeaseKind::Verdict,
                &format!("rec-{}", "a".repeat(64)),
                "recovery",
            )
            .unwrap();
        let root_relative = "canon/generations/gen-0000000000000001";
        let root = fixture.store_root.join(root_relative);
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("a.bin"), b"a").unwrap();
        fs::write(nested.join("b.bin"), b"bb").unwrap();
        let file_step = |path: String, bytes: &[u8]| sovmem::profile::RecoveryCleanupStep {
            artifact_kind: sovmem::profile::RecoveryCleanupArtifactKind::Generation,
            path,
            entry_type: sovmem::profile::RecoveryCleanupEntryType::File,
            byte_length: Some(bytes.len() as u64),
            content_hash: Some(blake3::hash(bytes).to_hex().to_string()),
        };
        let directory_step = |path: String| sovmem::profile::RecoveryCleanupStep {
            artifact_kind: sovmem::profile::RecoveryCleanupArtifactKind::Generation,
            path,
            entry_type: sovmem::profile::RecoveryCleanupEntryType::Directory,
            byte_length: None,
            content_hash: None,
        };
        let plan = RecoveryCleanupPlan {
            schema_version: RECOVERY_CLEANUP_PLAN_SCHEMA.to_owned(),
            steps: vec![
                file_step(format!("{root_relative}/a.bin"), b"a"),
                file_step(format!("{root_relative}/nested/b.bin"), b"bb"),
                directory_step(format!("{root_relative}/nested")),
                directory_step(root_relative.to_owned()),
            ],
        };
        if interrupt_after == 4 {
            fs::remove_file(nested.join("b.bin")).unwrap();
            assert_eq!(
                sovmem::profile::execute_recovery_cleanup_plan(
                    &StdFilesystem,
                    &fixture.store_root,
                    &plan,
                    &recovery_lease,
                    &[],
                ),
                Err(TransactionError::RecoveryAmbiguous),
                "a missing future step is not a valid completed prefix"
            );
            assert!(root.join("a.bin").is_file());
            fs::write(nested.join("b.bin"), b"bb").unwrap();
        }
        assert_eq!(
            sovmem::profile::execute_recovery_cleanup_plan(
                &InterruptAfterAnyRemoval::new(interrupt_after),
                &fixture.store_root,
                &plan,
                &recovery_lease,
                &[],
            ),
            Err(TransactionError::Write),
            "step {interrupt_after} must leave a resumable suffix"
        );
        if interrupt_after == 1 {
            let foreign = root.join("foreign-subtree");
            fs::create_dir(&foreign).unwrap();
            fs::write(foreign.join("evidence.bin"), b"foreign evidence").unwrap();
            assert_eq!(
                sovmem::profile::execute_recovery_cleanup_plan(
                    &StdFilesystem,
                    &fixture.store_root,
                    &plan,
                    &recovery_lease,
                    &[],
                ),
                Err(TransactionError::RecoveryAmbiguous)
            );
            assert!(foreign.join("evidence.bin").is_file());
            fs::remove_file(foreign.join("evidence.bin")).unwrap();
            fs::remove_dir(&foreign).unwrap();
        }
        sovmem::profile::execute_recovery_cleanup_plan(
            &StdFilesystem,
            &fixture.store_root,
            &plan,
            &recovery_lease,
            &[],
        )
        .unwrap_or_else(|error| panic!("resume step {interrupt_after}: {error:?}"));
        assert!(!root.exists(), "step {interrupt_after}");
        recovery_lease.release_terminal(&StdFilesystem).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn cleanup_plan_file_swap_and_symlink_fail_closed_and_retain_evidence() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("cleanup-plan-file-swap");
    let recovery_lease = fixture
        .runtime()
        .acquire_lease(
            LeaseKind::Verdict,
            &format!("rec-{}", "a".repeat(64)),
            "recovery",
        )
        .unwrap();
    let transaction_id = format!("txn-{}", "b".repeat(64));
    let relative = format!("canon/.current-{transaction_id}");
    let path = fixture.store_root.join(&relative);
    fs::write(&path, b"gen-0000000000000001\n").unwrap();
    let plan = RecoveryCleanupPlan {
        schema_version: RECOVERY_CLEANUP_PLAN_SCHEMA.to_owned(),
        steps: vec![sovmem::profile::RecoveryCleanupStep {
            artifact_kind: sovmem::profile::RecoveryCleanupArtifactKind::PointerTemporary,
            path: relative,
            entry_type: sovmem::profile::RecoveryCleanupEntryType::File,
            byte_length: Some(21),
            content_hash: Some(blake3::hash(b"gen-0000000000000001\n").to_hex().to_string()),
        }],
    };
    fs::write(&path, b"swapped cleanup bytes\n").unwrap();
    assert_eq!(
        sovmem::profile::execute_recovery_cleanup_plan(
            &StdFilesystem,
            &fixture.store_root,
            &plan,
            &recovery_lease,
            &[],
        ),
        Err(TransactionError::RecoveryAmbiguous)
    );
    assert_eq!(fs::read(&path).unwrap(), b"swapped cleanup bytes\n");

    let replacement = fixture.store_root.join("pointer-replacement.txt");
    fs::write(&replacement, b"gen-0000000000000001\n").unwrap();
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&replacement, &path).unwrap();
    assert_eq!(
        sovmem::profile::execute_recovery_cleanup_plan(
            &StdFilesystem,
            &fixture.store_root,
            &plan,
            &recovery_lease,
            &[],
        ),
        Err(TransactionError::RecoveryAmbiguous)
    );
    assert!(fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn c2b2_valid_foreign_index_swapped_after_inventory_blocks_every_cleanup_delete() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-inventory-index-race");
    let foreign = ProvisionedP3Fixture::with_staged_accept_candidate_with_extra_raw_newline(
        "recovery-inventory-index-foreign",
    );
    foreign
        .apply(VerdictVerb::Accept, "foreign valid index")
        .expect("build a valid index for a different generation");
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "inventory index mutation",
                &interrupted
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .expect("prepared applying transaction")
        .unwrap()
        .path();
    let plan: TransactionPlan =
        serde_json::from_slice(&fs::read(transaction.join("plan.json")).unwrap()).unwrap();
    let generation = fixture
        .store_root
        .join("canon/generations")
        .join(plan.generation_id.as_deref().unwrap());
    let index = fixture
        .store_root
        .join("derived/indexes")
        .join(plan.generation_id.as_deref().unwrap());
    let foreign_generation =
        String::from_utf8(fs::read(foreign.store_root.join("canon/CURRENT")).unwrap())
            .unwrap()
            .trim()
            .to_owned();
    let foreign_index = foreign
        .store_root
        .join("derived/indexes")
        .join(foreign_generation);
    let foreign_index_manifest = fs::read(foreign_index.join("index-manifest.json")).unwrap();
    let receipt = fixture
        .store_root
        .join("evidence/reindex")
        .join(format!("{}.json", plan.verdict_id));
    let temporary = fs::read_dir(fixture.store_root.join("ledger"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".verdicts-"))
        })
        .expect("prepared transaction must retain successor temporary");
    let mutation_index = fs::canonicalize(&index).unwrap();
    let replacement_index = fs::canonicalize(&foreign_index).unwrap();
    let runtime = fixture.runtime_with_ops(MutatePathOnSecondMetadata::new(
        mutation_index.clone(),
        move || {
            fs::remove_dir_all(&mutation_index)?;
            fs::rename(&replacement_index, &mutation_index)
        },
    ));
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &runtime,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("a valid-looking foreign index must not be deleted as the bound index");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert!(generation.is_dir());
    assert!(index.is_dir());
    assert!(receipt.is_file());
    assert!(temporary.is_file());
    assert_eq!(
        fs::read(index.join("index-manifest.json")).unwrap(),
        foreign_index_manifest,
        "fixture must replace the bound index only after the initial inventory validation"
    );
}

#[test]
fn c2b2_level3_generation_rejects_plan_policy_parent_target_and_judgment_mismatch() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-generation-plan-binding");
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "semantic generation binding",
                &interrupted
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .expect("prepared applying transaction")
        .unwrap()
        .path();
    let plan: TransactionPlan =
        serde_json::from_slice(&fs::read(transaction.join("plan.json")).unwrap()).unwrap();
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    let generation = verify_generation(
        &profile,
        &fixture
            .store_root
            .join("canon/generations")
            .join(plan.generation_id.as_deref().unwrap()),
        VerificationLevel::Three,
    )
    .expect("fixture must start from a Level-3-valid staged generation");
    let snapshot = sovmem::profile::inspect_store_with_transients(&profile, &fixture.store_root)
        .expect("recovery must inspect the interrupted transaction with transients allowed");

    for mismatch in ["policy", "parent", "target", "judgment"] {
        let mut forged = generation.clone();
        match mismatch {
            "policy" => forged.manifest.policy_hash = "a".repeat(64),
            "parent" => forged.manifest.parent_manifest_hash = "b".repeat(64),
            "target" => {
                forged
                    .manifest
                    .documents
                    .iter_mut()
                    .find(|entry| entry.canonical_path == plan.target_path)
                    .expect("fixture target entry")
                    .hash = "c".repeat(64);
            }
            "judgment" => {
                forged
                    .manifest
                    .documents
                    .iter_mut()
                    .find(|entry| {
                        entry.canonical_path == format!("judgments/{}.md", plan.verdict_id)
                    })
                    .expect("fixture judgment entry")
                    .hash = "d".repeat(64);
            }
            _ => unreachable!("closed mismatch fixture set"),
        }

        assert_eq!(
            validate_generation_against_plan(&snapshot, &profile, &plan, &forged),
            Err(TransactionError::RecoveryAmbiguous),
            "{mismatch} must be bound to the prepared plan before cleanup"
        );
    }
}

#[test]
fn c2b2_prepared_applying_crash_resumes_after_verified_cleanup() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-prepared-valid-resume");
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "prepared valid crash", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect("a fully verified prepared applying state must recover");

    assert_eq!(outcome.mode, RecoveryMode::VerdictTransaction);
    assert!(!outcome.rolled_forward);
    assert_eq!(
        fs::read(fixture.store_root.join("canon/CURRENT")).unwrap(),
        b"seed\n"
    );
    assert!(fixture.transactions_empty());
    assert!(fixture.leases_absent());
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    assert!(inspect_store(&profile, &fixture.store_root).is_ok());
}

#[test]
fn c2b2_prepared_invalid_final_index_blocks_cleanup_before_any_deletion() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-prepared-invalid-index");
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "prepared invalid index", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .expect("prepared transaction")
        .unwrap()
        .path();
    let plan: TransactionPlan =
        serde_json::from_slice(&fs::read(transaction.join("plan.json")).unwrap()).unwrap();
    let index = fixture
        .store_root
        .join("derived/indexes")
        .join(plan.generation_id.as_deref().unwrap())
        .join("index-manifest.json");
    fs::write(&index, b"{}\n").unwrap();
    let canon_before = fixture.canon_hashes();
    let indexes_before = fixture.index_hashes();
    let receipt = fixture
        .store_root
        .join("evidence/reindex")
        .join(format!("{}.json", plan.verdict_id));
    let receipt_before = fs::read(&receipt).unwrap();

    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("an invalid final index must retain every prepared residual");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(fixture.index_hashes(), indexes_before);
    assert_eq!(fs::read(&receipt).unwrap(), receipt_before);
}

#[test]
fn c2b2_prepared_forged_receipt_blocks_cleanup_before_any_deletion() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-prepared-forged-receipt");
    let interrupted =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "prepared forged receipt", &interrupted)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .expect("prepared transaction")
        .unwrap()
        .path();
    let plan: TransactionPlan =
        serde_json::from_slice(&fs::read(transaction.join("plan.json")).unwrap()).unwrap();
    let receipt = fixture
        .store_root
        .join("evidence/reindex")
        .join(format!("{}.json", plan.verdict_id));
    fs::write(&receipt, b"{}\n").unwrap();
    let canon_before = fixture.canon_hashes();
    let indexes_before = fixture.index_hashes();
    let receipt_before = fs::read(&receipt).unwrap();

    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("a forged receipt must retain every prepared residual");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(fixture.index_hashes(), indexes_before);
    assert_eq!(fs::read(&receipt).unwrap(), receipt_before);
}

#[test]
fn c2b2_committed_forged_current_temporary_blocks_roll_forward() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-committed-forged-current");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::BeforeCurrentRename));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "committed forged current",
                &interrupted
            )
            .unwrap_err(),
        TransactionError::JournalState
    );
    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .expect("committed transaction")
        .unwrap()
        .path();
    let plan: TransactionPlan =
        serde_json::from_slice(&fs::read(transaction.join("plan.json")).unwrap()).unwrap();
    let temporary = fixture
        .store_root
        .join("canon")
        .join(format!(".current-{}", plan.transaction_id));
    assert!(temporary.is_file());
    fs::write(&temporary, b"gen-foreign\n").unwrap();
    let canon_before = fixture.canon_hashes();
    let indexes_before = fixture.index_hashes();
    let receipt = fixture
        .store_root
        .join("evidence/reindex")
        .join(format!("{}.json", plan.verdict_id));
    let receipt_before = fs::read(&receipt).unwrap();
    let current_before = fs::read(&temporary).unwrap();

    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("a forged current temporary must block committed roll-forward");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(fixture.index_hashes(), indexes_before);
    assert_eq!(fs::read(&receipt).unwrap(), receipt_before);
    assert_eq!(fs::read(&temporary).unwrap(), current_before);
}

fn submitted_intake_cleanup(name: &str) -> (ProvisionedP3Fixture, String) {
    let fixture = ProvisionedP3Fixture::empty(name);
    let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
    let candidate_path = fixture
        .profile_path
        .parent()
        .unwrap()
        .join("intake-cleanup.md");
    fs::write(
        &candidate_path,
        p3_test_candidate_bytes(&fixture.store_root.join("seed/bundle").join(target_path)),
    )
    .unwrap();
    let submitted = submit_proposal_with_clock(
        ProposalSubmission {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            source_id: "src:agent:intake-cleanup".to_owned(),
            operation: ProposalOperation::Replace,
            intent: ProposalIntent::Accept,
            target_id: "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"
                .to_owned(),
            target_path: target_path.to_owned(),
            candidate_path,
            confidence: None,
            evaluator_path: None,
        },
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-15T06:19:00.000000000Z").unwrap()),
    )
    .unwrap();
    (fixture, submitted.submission_id)
}

fn interrupted_intake_cleanup(name: &str) -> (ProvisionedP3Fixture, std::path::PathBuf, String) {
    let (fixture, submission_id) = submitted_intake_cleanup(name);
    let mut human = ScriptedHuman::new(Some(format!("SEAL INTAKE {submission_id}\n")));
    assert_eq!(
        seal_intake_submission_with_human_and_faults(
            &fixture.profile_path,
            &fixture.store_root,
            &submission_id,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-15T06:19:01.000000000Z").unwrap(),
            ),
            &FaultPlan::at(FaultPoint::BeforeInboxCleanup),
        )
        .unwrap_err()
        .code(),
        "intake.incomplete"
    );
    let custody = fixture
        .store_root
        .join("intake/transactions")
        .join(&submission_id);
    assert!(custody.join("prepared.json").is_file());
    assert!(custody.join("committed.json").is_file());
    assert!(custody.join("published.json").is_file());
    (fixture, custody, submission_id)
}

#[test]
fn intake_cleanup_post_selection_child_is_retained_fail_closed() {
    let (fixture, submission_id) = submitted_intake_cleanup("round1-post-selection-child");
    let custody = fixture
        .store_root
        .join("intake/transactions")
        .join(&submission_id);
    let inserted = custody.join("post-selection-child.bin");
    sovmem::profile::install_intake_seal_post_cleanup_selection_hook_for_test({
        let inserted = inserted.clone();
        move || fs::write(inserted, b"unbound child\n").unwrap()
    });
    let mut human = ScriptedHuman::new(Some(format!("SEAL INTAKE {submission_id}\n")));
    let error = seal_intake_submission_with_human(
        &fixture.profile_path,
        &fixture.store_root,
        &submission_id,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-15T06:20:00.000000000Z").unwrap()),
    )
    .expect_err("a child inserted after cleanup selection must be retained fail closed");
    assert_eq!(error.code(), "intake.incomplete");
    assert!(inserted.exists());
}

#[test]
fn intake_cleanup_revalidates_replaced_bound_input_after_selection() {
    let (fixture, submission_id) =
        submitted_intake_cleanup("round2-post-selection-bound-input-replacement");
    let custody = fixture
        .store_root
        .join("intake/transactions")
        .join(&submission_id);
    let candidate = custody.join("candidate.md");
    let replacement = b"MATRIX_PRIVATE_REPLACED_CANDIDATE\n".to_vec();
    sovmem::profile::install_intake_seal_post_cleanup_selection_hook_for_test({
        let candidate = candidate.clone();
        let replacement = replacement.clone();
        move || {
            fs::remove_file(&candidate).unwrap();
            fs::write(candidate, replacement).unwrap();
        }
    });
    let mut human = ScriptedHuman::new(Some(format!("SEAL INTAKE {submission_id}\n")));
    let error = seal_intake_submission_with_human(
        &fixture.profile_path,
        &fixture.store_root,
        &submission_id,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-26T08:10:00.000000000Z").unwrap()),
    )
    .expect_err("a selected custody child replaced before cleanup must fail closed");
    assert_eq!(error.code(), "intake.incomplete");
    assert_eq!(fs::read(candidate).unwrap(), replacement);
    assert!(custody.join("submission.meta.json").is_file());
}

#[test]
fn intake_cleanup_lease_loss_after_candidate_removal_blocks_metadata_cleanup_and_recovery() {
    let (fixture, submission_id) =
        submitted_intake_cleanup("round2-lease-loss-before-metadata-cleanup");
    let custody = fixture
        .store_root
        .join("intake/transactions")
        .join(&submission_id);
    let owner = fixture.store_root.join("locks/intake.lock/owner.json");
    sovmem::profile::install_intake_seal_before_metadata_cleanup_hook_for_test(move || {
        fs::write(owner, b"{}\n").unwrap();
    });
    let mut human = ScriptedHuman::new(Some(format!("SEAL INTAKE {submission_id}\n")));
    let error = seal_intake_submission_with_human(
        &fixture.profile_path,
        &fixture.store_root,
        &submission_id,
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-26T08:11:00.000000000Z").unwrap()),
    )
    .expect_err("lease loss after candidate removal must retain metadata custody");
    assert_eq!(error.code(), "intake.incomplete");
    assert!(!custody.join("candidate.md").exists());
    assert!(custody.join("submission.meta.json").is_file());
    assert_eq!(
        fixture
            .recover_intake_with_runtime(&fixture.runtime())
            .expect_err("lease loss must keep partial cleanup fail closed")
            .code(),
        "transaction.recovery_ambiguous"
    );
}

#[test]
fn intake_cleanup_lease_loss_before_each_exact_removal_is_fail_closed() {
    for (label, after_candidate) in [("candidate", false), ("metadata", true)] {
        let (fixture, submission_id) =
            submitted_intake_cleanup(&format!("round1-lease-loss-{label}"));
        let custody = fixture
            .store_root
            .join("intake/transactions")
            .join(&submission_id);
        let owner = fixture.store_root.join("locks/intake.lock/owner.json");
        if after_candidate {
            sovmem::profile::install_intake_seal_after_candidate_cleanup_hook_for_test({
                let owner = owner.clone();
                move || fs::write(owner, b"{}\n").unwrap()
            });
        } else {
            sovmem::profile::install_intake_seal_pre_cleanup_hook_for_test({
                let owner = owner.clone();
                move || fs::write(owner, b"{}\n").unwrap()
            });
        }
        let mut human = ScriptedHuman::new(Some(format!("SEAL INTAKE {submission_id}\n")));
        let error = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submission_id,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-15T06:21:00.000000000Z").unwrap(),
            ),
        )
        .expect_err("changed lease before {label} removal must fail closed");
        assert_eq!(error.code(), "intake.incomplete", "{label}");
        assert!(custody.exists(), "{label}");
    }
}

#[test]
fn intake_cleanup_crash_after_each_exact_removal_retains_and_recovers_v2_custody() {
    for (label, after_candidate) in [("candidate", true), ("metadata", false)] {
        let (fixture, submission_id) =
            submitted_intake_cleanup(&format!("round1-crash-after-{label}"));
        if after_candidate {
            sovmem::profile::install_intake_seal_after_candidate_cleanup_hook_for_test(|| {
                panic!("inject crash after candidate cleanup")
            });
        } else {
            sovmem::profile::install_intake_seal_after_metadata_cleanup_hook_for_test(|| {
                panic!("inject crash after metadata cleanup")
            });
        }
        let mut human = ScriptedHuman::new(Some(format!("SEAL INTAKE {submission_id}\n")));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = seal_intake_submission_with_human(
                    &fixture.profile_path,
                    &fixture.store_root,
                    &submission_id,
                    &mut human,
                    &FixedClock(
                        sovmem::profile::Timestamp::parse("2026-07-15T06:22:00.000000000Z")
                            .unwrap(),
                    ),
                );
            }))
            .is_err(),
            "{label}"
        );
        let outcome = fixture
            .recover_intake_with_runtime(&fixture.runtime())
            .unwrap_or_else(|error| panic!("{label} recovery: {error:?}"));
        assert_eq!(outcome.mode, RecoveryMode::IntakeOnly, "{label}");
        assert!(fixture.leases_absent(), "{label}");
    }
}

#[test]
fn intake_cleanup_child_inserted_after_journal_selection_is_retained_fail_closed() {
    let (fixture, custody, _) =
        interrupted_intake_cleanup("recovery-intake-post-selection-child-insertion");
    let inserted = custody.join("unknown-after-selection.bin");
    fs::write(&inserted, b"unbound post-selection bytes\n").unwrap();
    let candidate_before = fs::read(custody.join("candidate.md")).unwrap();
    let metadata_before = fs::read(custody.join("submission.meta.json")).unwrap();
    let error = fixture
        .recover_intake_with_runtime(&fixture.runtime())
        .expect_err("an extra child in the protected v2 custody journal must fail closed");

    assert_eq!(error.code(), "intake.incomplete");
    assert_eq!(
        fs::read(custody.join("candidate.md")).unwrap(),
        candidate_before
    );
    assert_eq!(
        fs::read(custody.join("submission.meta.json")).unwrap(),
        metadata_before
    );
    assert!(custody.join("prepared.json").exists());
    assert!(inserted.exists());
}

#[test]
fn intake_cleanup_crash_after_each_exact_step_resumes_the_remaining_suffix() {
    for (after, remove_candidate, remove_metadata) in
        [(1, true, false), (2, false, true), (3, true, true)]
    {
        let (fixture, custody, _) =
            interrupted_intake_cleanup(&format!("recovery-intake-cleanup-crash-step-{after}"));
        if remove_candidate {
            fs::remove_file(custody.join("candidate.md")).unwrap();
        }
        if remove_metadata {
            fs::remove_file(custody.join("submission.meta.json")).unwrap();
        }
        let outcome = fixture
            .recover_intake_with_runtime(&fixture.runtime())
            .unwrap_or_else(|error| panic!("resume step {after}: {error:?}"));
        assert_eq!(outcome.mode, RecoveryMode::IntakeOnly, "step {after}");
        assert!(custody.join("prepared.json").exists(), "step {after}");
        assert!(custody.join("committed.json").exists(), "step {after}");
        assert!(custody.join("published.json").exists(), "step {after}");
        assert!(fixture.leases_absent(), "step {after}");
    }
}

#[test]
fn intake_cleanup_recovery_removes_only_the_bound_v2_envelope() {
    let (fixture, custody, _) = interrupted_intake_cleanup("recovery-intake-cleanup");
    let outcome = fixture
        .recover_intake_with_runtime(&fixture.runtime())
        .expect("a fully published v2 intake transaction must resume cleanup");

    assert_eq!(outcome.mode, RecoveryMode::IntakeOnly);
    assert!(custody.join("prepared.json").is_file());
    assert!(custody.join("committed.json").is_file());
    assert!(custody.join("published.json").is_file());
    assert!(!custody.join("candidate.md").exists());
    assert!(!custody.join("submission.meta.json").exists());
    assert!(fixture.leases_absent());
}

#[test]
fn intake_cleanup_valid_published_envelope_is_recoverable_when_inputs_change() {
    let (swapped, custody, _) = interrupted_intake_cleanup("recovery-intake-content-swap");
    let candidate = custody.join("candidate.md");
    fs::write(&candidate, b"swapped candidate bytes\n").unwrap();
    let error = swapped
        .recover_intake_with_runtime(&swapped.runtime())
        .expect_err("a changed remaining envelope member must retain intake custody");
    assert_eq!(error.code(), "intake.incomplete");
    assert_eq!(fs::read(&candidate).unwrap(), b"swapped candidate bytes\n");
    assert!(custody.exists());
    assert!(custody.join("published.json").is_file());

    let (missing, custody, _) = interrupted_intake_cleanup("recovery-intake-missing-future-step");
    fs::remove_file(custody.join("submission.meta.json")).unwrap();
    let outcome = missing
        .recover_intake_with_runtime(&missing.runtime())
        .expect("a missing disposable envelope member must not invalidate publication");
    assert_eq!(outcome.mode, RecoveryMode::IntakeOnly);
    assert!(!custody.join("candidate.md").exists());
    assert!(custody.exists());
    assert!(custody.join("published.json").is_file());
}

#[cfg(unix)]
#[test]
fn intake_cleanup_symlink_swap_retains_evidence() {
    use std::os::unix::fs::symlink;

    let (fixture, custody, _) = interrupted_intake_cleanup("recovery-intake-symlink-swap");
    let meta = custody.join("submission.meta.json");
    let foreign = fixture.store_root.join("foreign-intake-meta.json");
    fs::rename(&meta, &foreign).unwrap();
    symlink(&foreign, &meta).unwrap();
    assert_eq!(
        fixture
            .recover_intake_with_runtime(&fixture.runtime())
            .unwrap_err()
            .code(),
        "transaction.recovery_ambiguous"
    );
    assert!(fs::symlink_metadata(&meta)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(foreign.exists());
}

#[test]
fn intake_only_recovery_preserves_v2_sealed_pair_and_intake_chain() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-intake-only-v2");
    let pair_root = fixture
        .store_root
        .join("intake/proposals")
        .join(&fixture.proposal_id[4..6])
        .join(&fixture.proposal_id);
    let candidate_before =
        fs::read(pair_root.join("candidate.md")).expect("capture protected candidate bytes");
    let meta_before = fs::read(pair_root.join("proposal.meta.json"))
        .expect("capture protected proposal metadata");
    let intake_before = fs::read(fixture.store_root.join("intake/events.jsonl"))
        .expect("capture protected intake chain");
    let owner_id = "rec-intake-recovery";
    let stale = acquire_trusted_intake_lease(&fixture, owner_id, "propose");
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {owner_id}\n")));

    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect("intake-only recovery must preserve protected v2 intake evidence");

    assert_eq!(outcome.mode, RecoveryMode::IntakeOnly);
    assert_eq!(
        fs::read(pair_root.join("candidate.md")).unwrap(),
        candidate_before
    );
    assert_eq!(
        fs::read(pair_root.join("proposal.meta.json")).unwrap(),
        meta_before
    );
    assert_eq!(
        fs::read(fixture.store_root.join("intake/events.jsonl")).unwrap(),
        intake_before
    );
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    assert!(read_sealed_pair_structural(
        &StorePaths::from_root(fs::canonicalize(&fixture.store_root).unwrap()),
        &fixture.proposal_id,
    )
    .is_ok());
    assert!(inspect_store(&profile, &fixture.store_root).is_ok());
    assert!(fixture.leases_absent());
    drop(stale);
}

#[test]
fn intake_only_recovery_rejects_malformed_trusted_owner_without_prompt_or_removal() {
    let fixture = ProvisionedP3Fixture::empty("recovery-malformed-trusted-intake-owner");
    let (paths, stale) =
        acquire_stale_trusted_intake_lease(&fixture, "rec-intake-malformed", "propose");
    let owner = paths.locks.join("intake.lock/owner.json");
    let malformed = b"{}\n".to_vec();
    fs::write(&owner, &malformed).expect("write malformed trusted intake owner");
    let mut human = ScriptedHuman::new(None);

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("malformed trusted intake owner must fail closed through public recovery");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(error.code(), "transaction.recovery_ambiguous");
    assert_eq!(human.show_count(), 0);
    assert_eq!(human.read_count(), 0);
    assert_eq!(fs::read(owner).unwrap(), malformed);
    assert!(paths.locks.join("intake.lock").exists());
    drop(stale);
}

#[test]
fn intake_only_recovery_rejects_token_substituted_trusted_owner_without_prompt_or_removal() {
    let fixture = ProvisionedP3Fixture::empty("recovery-token-substituted-intake-owner");
    let (paths, stale) =
        acquire_stale_trusted_intake_lease(&fixture, "rec-intake-token", "propose");
    let owner = paths.locks.join("intake.lock/owner.json");
    let mut forged: serde_json::Value =
        serde_json::from_slice(&fs::read(&owner).unwrap()).expect("parse trusted intake owner");
    forged["lease_token"] = serde_json::Value::String("0".repeat(64));
    let mut forged_bytes = serde_json::to_vec(&forged).expect("render forged intake owner");
    forged_bytes.push(b'\n');
    fs::write(&owner, &forged_bytes).expect("substitute trusted intake token");
    let mut human = ScriptedHuman::new(None);

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect_err("token-substituted owner must fail closed through public recovery");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(error.code(), "transaction.recovery_ambiguous");
    assert_eq!(human.show_count(), 0);
    assert_eq!(human.read_count(), 0);
    assert_eq!(fs::read(owner).unwrap(), forged_bytes);
    assert!(paths.locks.join("intake.lock").exists());
    drop(stale);
}

#[test]
fn clean_finalization_after_journal_cleanup_releases_stale_normal_leases() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-clean-finalization");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterJournalCleanup));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Reject, "clean finalization", &interrupted)
            .unwrap_err(),
        TransactionError::JournalState
    );
    assert!(fixture.transactions_empty());
    assert!(!fixture.leases_absent());
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    assert_eq!(
        inspect_store(&profile, &fixture.store_root)
            .unwrap_err()
            .code(),
        "transaction.incomplete"
    );
    assert_eq!(
        verify_store_level4(&profile, &fixture.store_root)
            .unwrap_err()
            .code(),
        "store.locked"
    );

    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect("clean finalization must release only the stale normal leases");

    assert_eq!(outcome.mode, RecoveryMode::CleanFinalization);
    assert!(!outcome.rolled_forward);
    assert!(!outcome.idempotent);
    assert!(fixture.verify_level4().is_ok());
    assert!(fixture.transactions_empty());
    assert!(fixture.leases_absent());
}

#[test]
fn recovery_of_a_clean_store_is_idempotent_and_does_not_prompt_or_write() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-clean-idempotent");
    fixture
        .apply(VerdictVerb::Reject, "establish clean ledger verdict")
        .unwrap();
    let before = fixture.durable_hashes();
    let mut human = ScriptedHuman::new(None);

    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
    )
    .expect("clean recovery must be an idempotent verifier-only operation");

    assert_eq!(outcome.mode, RecoveryMode::CleanFinalization);
    assert!(outcome.idempotent);
    assert!(!outcome.rolled_forward);
    assert_eq!(human.show_count(), 0);
    assert_eq!(human.read_count(), 0);
    assert_eq!(fixture.durable_hashes(), before);
    assert!(fixture.verify_level4().is_ok());
}

#[test]
fn recovery_resumes_an_exact_journal_after_a_recovery_crash() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-resume-journal");
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert!(fixture
        .apply_with_runtime(
            VerdictVerb::Accept,
            "interrupted before ledger",
            &interrupted_apply
        )
        .is_err());
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut first_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterRecoveryJournalCreation));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut first_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );

    let recovery_journal =
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
            .expect("discover exact interrupted recovery record")
            .expect("recovery record must remain after its injected crash");
    let recovery_id = recovery_journal.record.recovery_id.clone();
    let recovered_at = recovery_journal.record.recovered_at.clone();
    let recovery_bytes = recovery_journal.bytes.clone();
    assert!(
        !recovery_journal.record.cleanup_plan.steps.is_empty(),
        "the synced recovery record must contain the immutable cleanup plan before payload deletion"
    );
    for step in &recovery_journal.record.cleanup_plan.steps {
        assert!(
            fixture.store_root.join(&step.path).exists(),
            "every planned payload step must still exist at the post-journal crash: {}",
            step.path
        );
    }
    assert_eq!(
        fs::read(recovery_journal.root.join("recovery.json")).unwrap(),
        recovery_bytes,
        "resumption must consume the canonical record bytes that survived the crash"
    );
    assert_eq!(
        recovered_at, "2026-07-14T04:31:00.000000000Z",
        "the persisted recovery time, rather than the retry clock, binds recovery identity"
    );
    assert_eq!(
        recovery_id,
        recovery_transaction_id(
            &recovery_owner_hashes(&recovery_journal.record.quarantined_leases),
            &sovmem::profile::Timestamp::parse(&recovered_at).unwrap(),
            recovery_journal.record.observed_ledger_head.as_deref(),
            &recovery_journal.record.command_class,
            recovery_journal.record.original_journal_hash.as_deref(),
            &recovery_journal.record.profile_hash,
            &recovery_journal.record.policy_hash,
            &recovery_journal.record.cleanup_plan_hash,
        )
    );

    let recovery_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    assert_eq!(recovery_owner.command_class, "recovery");
    let mut resuming_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", recovery_owner.owner_id)));
    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut resuming_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect("a bound recovery journal must resume after a recovery crash");

    assert_eq!(outcome.mode, RecoveryMode::VerdictTransaction);
    assert!(!outcome.rolled_forward);
    let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
    let inspected = inspect_store(&profile, &fixture.store_root)
        .expect("unsealed seed, policy, profile, and staging evidence must remain valid");
    assert!(
        !inspected.sealed,
        "pre-genesis rollback must remain unsealed"
    );
    assert!(
        read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("read pre-genesis ledger")
            .events
            .is_empty(),
        "resumption must not create a second recovery or verdict transaction"
    );
    assert!(fixture.transactions_empty());
    assert!(
        !fixture
            .store_root
            .join("transactions")
            .join(&recovery_id)
            .exists(),
        "the only persisted recovery record must be consumed, not replaced"
    );
    assert!(fixture.leases_absent());
}

#[test]
fn crash_after_recovery_lease_acquisition_before_journal_reprompts_and_resumes_exactly() {
    let fixture =
        ProvisionedP3Fixture::with_staged_reject("recovery-bootstrap-after-lease-before-journal");
    let canon_before = fixture.canon_hashes();
    let ledger_before = fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
        .expect("capture ledger authority before recovery bootstrap");
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "interrupt before recovery bootstrap",
                &interrupted_apply,
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("interrupted transaction retains the original stale owner");

    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterRecoveryLeaseAcquisition));
    let mut first_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut first_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-15T04:30:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );
    assert_eq!(first_human.show_count(), 1);
    assert_eq!(first_human.read_count(), 1);
    assert!(
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"),)
            .expect("inspect recovery bootstrap state")
            .is_none()
    );

    let recovery_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("the protected recovery lease remains visible");
    assert_eq!(recovery_owner.command_class, "recovery");
    assert_ne!(recovery_owner.owner_id, original_owner.owner_id);

    let mut retry_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut retry_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-15T04:31:00.000000000Z").unwrap()),
    )
    .expect("bootstrap recovery must resume from the original stale owner");

    let display = std::str::from_utf8(retry_human.displayed()).expect("recovery display is UTF-8");
    assert!(display.contains(&original_owner.owner_id));
    assert!(!display.contains(&recovery_owner.owner_id));
    assert_eq!(outcome.mode, RecoveryMode::VerdictTransaction);
    assert!(!outcome.rolled_forward);
    assert!(!outcome.idempotent);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("re-read ledger authority after recovery bootstrap"),
        ledger_before
    );
    assert!(fixture.transactions_empty());
    assert!(fixture.leases_absent());
    let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
    assert!(inspect_store(&profile, &fixture.store_root).is_ok());
}

#[test]
fn recovery_bootstrap_wrong_phrase_preserves_authority_without_writing() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-bootstrap-wrong-phrase");
    let (original_owner_id, recovery_owner_id) = crash_after_recovery_lease_acquisition(&fixture);
    let before = fixture.durable_hashes();
    let canon_before = fixture.canon_hashes();
    let ledger_before = fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
        .expect("capture ledger authority before rejected recovery bootstrap");
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", recovery_owner_id)));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-15T04:32:00.000000000Z").unwrap()),
    )
    .expect_err("the bootstrap must require the original stale owner phrase");

    assert_eq!(error, TransactionError::ConfirmationFailed);
    assert_eq!(human.show_count(), 1);
    assert_eq!(human.read_count(), 1);
    let display = std::str::from_utf8(human.displayed()).expect("recovery display is UTF-8");
    assert!(display.contains(&original_owner_id));
    assert!(!display.contains(&recovery_owner_id));
    assert_eq!(fixture.durable_hashes(), before);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("re-read ledger authority after rejected bootstrap"),
        ledger_before
    );
}

#[test]
fn recovery_bootstrap_rejects_a_mismatched_recovery_owner_without_writing() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-bootstrap-mismatched-owner");
    let _ = crash_after_recovery_lease_acquisition(&fixture);
    let owner_path = fixture.store_root.join("locks/verdict.lock/owner.json");
    let mut forged = read_canonical_owner(&StdFilesystem, &owner_path, LeaseKind::Verdict)
        .expect("read bootstrap recovery owner");
    forged.owner_id = format!("rec-{}", "a".repeat(64));
    forged.lease_token = lease_token(&forged);
    fs::write(
        &owner_path,
        canonical_owner_bytes(&forged).expect("canonical forged recovery owner"),
    )
    .expect("replace test-only recovery owner");
    let before = fixture.durable_hashes();
    let canon_before = fixture.canon_hashes();
    let ledger_before = fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
        .expect("capture ledger authority before mismatched recovery owner");
    let mut human = ScriptedHuman::new(None);

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-15T04:33:00.000000000Z").unwrap()),
    )
    .expect_err("a recovery owner must bind the exact original quarantine hashes");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(human.show_count(), 0);
    assert_eq!(human.read_count(), 0);
    assert_eq!(fixture.durable_hashes(), before);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("re-read ledger authority after mismatched recovery owner"),
        ledger_before
    );
}

#[test]
fn recovery_bootstrap_rejects_tampered_missing_or_mismatched_quarantine_without_writing() {
    for case in ["tampered", "missing", "mismatched"] {
        let fixture = ProvisionedP3Fixture::with_staged_reject(&format!(
            "recovery-bootstrap-quarantine-{case}"
        ));
        let _ = crash_after_recovery_lease_acquisition(&fixture);
        let quarantine = fs::read_dir(fixture.store_root.join("locks"))
            .expect("read recovery quarantine parent")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".recovery-verdict-"))
            })
            .expect("recovery bootstrap retains a typed verdict quarantine");
        let quarantined_owner = quarantine.join("owner.json");
        match case {
            "tampered" => {
                fs::write(&quarantined_owner, b"{}\n").expect("tamper quarantined owner bytes")
            }
            "missing" => {
                fs::remove_file(&quarantined_owner).expect("remove quarantined owner bytes")
            }
            "mismatched" => {
                fs::copy(
                    fixture.store_root.join("locks/verdict.lock/owner.json"),
                    &quarantined_owner,
                )
                .expect("replace quarantined owner with a different valid owner");
            }
            _ => unreachable!(),
        };
        let before = fixture.durable_hashes();
        let canon_before = fixture.canon_hashes();
        let ledger_before = fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("capture ledger authority before invalid quarantine retry");
        let mut human = ScriptedHuman::new(None);

        let error = recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &fixture.runtime(),
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-15T04:34:00.000000000Z").unwrap(),
            ),
        )
        .expect_err("invalid typed quarantine evidence must fail closed");

        assert_eq!(error, TransactionError::RecoveryAmbiguous, "{case}");
        assert_eq!(human.show_count(), 0, "{case}");
        assert_eq!(human.read_count(), 0, "{case}");
        assert_eq!(fixture.durable_hashes(), before, "{case}");
        assert_eq!(fixture.canon_hashes(), canon_before, "{case}");
        assert_eq!(
            fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
                .expect("re-read ledger authority after invalid quarantine retry"),
            ledger_before,
            "{case}"
        );
    }
}

#[test]
fn published_recovery_finishes_only_its_cleanup_tail_without_replaying_mode() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-published-tail");
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert!(fixture
        .apply_with_runtime(
            VerdictVerb::Accept,
            "interrupted before ledger",
            &interrupted_apply
        )
        .is_err());
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("interrupted apply must retain a stale verdict owner");
    let mut first_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterRecoveryPublishedMarker));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut first_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );

    let recovery_journal =
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
            .expect("discover published recovery record")
            .expect("published recovery record remains after its injected crash");
    assert!(recovery_journal.is_published(&StdFilesystem).unwrap());
    let recovery_id = recovery_journal.record.recovery_id.clone();
    let original_id = recovery_journal
        .record
        .original_transaction_id
        .as_deref()
        .expect("verdict recovery must bind the original transaction");
    assert!(
        fixture
            .store_root
            .join("transactions")
            .join(format!(".cleanup-{original_id}"))
            .is_dir(),
        "published recovery evidence must follow an original cleanup-witness transition"
    );
    assert!(
        !fixture
            .store_root
            .join("transactions")
            .join(original_id)
            .exists(),
        "the original regular journal must not remain after recovery publication"
    );
    let canon_before = fixture.canon_hashes();
    let indexes_before = fixture.index_hashes();
    let ledger_before = fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
        .expect("capture ledger bytes before cleanup-tail retry");
    assert!(
        read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("read pre-genesis ledger")
            .events
            .is_empty()
    );

    let protected_runtime = fixture.runtime_with_ops(RejectProtectedWrites::new(
        fs::canonicalize(&fixture.store_root).expect("canonicalize store root"),
    ));
    let mut retry_human = ScriptedHuman::new(None);
    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &protected_runtime,
        &mut retry_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect("a published recovery must finish only its cleanup tail");

    assert_eq!(outcome.mode, RecoveryMode::VerdictTransaction);
    assert!(!outcome.rolled_forward);
    assert_eq!(retry_human.show_count(), 0);
    assert_eq!(retry_human.read_count(), 0);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(fixture.index_hashes(), indexes_before);
    assert_eq!(
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).unwrap(),
        ledger_before
    );
    let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
    let inspected = inspect_store(&profile, &fixture.store_root)
        .expect("published recovery tail must restore the valid unsealed store");
    assert!(!inspected.sealed);
    assert!(fixture.transactions_empty());
    assert!(!fixture
        .store_root
        .join("transactions")
        .join(recovery_id)
        .exists());
    assert!(fixture.leases_absent());
}

#[test]
fn unpublished_recovery_resumes_after_original_cleanup_witness_before_publication() {
    let fixture =
        ProvisionedP3Fixture::with_staged_reject("recovery-original-witness-before-marker");
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert!(fixture
        .apply_with_runtime(
            VerdictVerb::Accept,
            "interrupt before ledger",
            &interrupted_apply
        )
        .is_err());
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("interrupted apply must retain a stale verdict owner");
    let mut first_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterOriginalCleanupWitness));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut first_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );

    let recovery =
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
            .expect("discover unfinished recovery")
            .expect("recovery journal must remain");
    assert!(!recovery.is_published(&StdFilesystem).unwrap());
    let original = discover_original_journal_ignoring(
        &StdFilesystem,
        &fixture.store_root.join("transactions"),
        Some(&recovery.root),
    )
    .expect("discover the exact post-cleanup original state");
    let original_transaction_id = recovery
        .record
        .original_transaction_id
        .as_deref()
        .expect("verdict recovery must retain the original transaction identity");
    assert!(original.cleanup_form);
    assert_eq!(original.transaction_id(), Some(original_transaction_id));
    assert_eq!(original.files, ["plan.json", "prepared.json"]);
    assert_eq!(
        original.commitment_hash().as_deref(),
        recovery.record.original_journal_hash.as_deref(),
        "the cleanup witness must preserve the exact original-prefix commitment"
    );

    let recovery_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("recovery lease remains live");
    let mut retry_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", recovery_owner.owner_id)));
    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut retry_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect("unpublished recovery must resume from the recovery-bound partial cleanup");

    assert_eq!(outcome.mode, RecoveryMode::VerdictTransaction);
    assert!(fixture.transactions_empty());
    assert!(fixture.leases_absent());
}

#[test]
fn published_recovery_resumes_after_original_cleanup_removal() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-original-witness-removed");
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert!(fixture
        .apply_with_runtime(
            VerdictVerb::Accept,
            "interrupt before ledger",
            &interrupted_apply
        )
        .is_err());
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("interrupted apply must retain a stale verdict owner");
    let mut first_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterOriginalCleanup));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut first_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );

    let recovery =
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
            .expect("discover published recovery")
            .expect("published recovery journal must remain");
    assert!(recovery.is_published(&StdFilesystem).unwrap());
    let original_id = recovery
        .record
        .original_transaction_id
        .as_deref()
        .expect("verdict recovery must bind original transaction");
    assert!(!fixture
        .store_root
        .join("transactions")
        .join(format!(".cleanup-{original_id}"))
        .exists());

    let canon_before = fixture.canon_hashes();
    let ledger_before = fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
        .expect("capture ledger before cleanup-only retry");
    let mut retry_human = ScriptedHuman::new(None);
    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut retry_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect("published recovery with no original witness must finish cleanup only");

    assert_eq!(outcome.mode, RecoveryMode::VerdictTransaction);
    assert_eq!(retry_human.show_count(), 0);
    assert_eq!(retry_human.read_count(), 0);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).unwrap(),
        ledger_before
    );
    assert!(fixture.transactions_empty());
    assert!(fixture.leases_absent());
}

#[test]
fn recovery_cleanup_witness_final_deletion_has_no_post_delete_parent_sync() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-witness-terminal-sync");
    let record = with_test_recovery_id(RecoveryJournalRecord {
        schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
        recovery_id: format!("txn-{}", "a".repeat(64)),
        mode: RecoveryMode::CleanFinalization,
        command_class: "verdict".to_owned(),
        recovered_at: "2026-07-14T04:31:00.000000000Z".to_owned(),
        quarantined_leases: vec![RecoveryLeaseBinding {
            kind: RecoveryLeaseKind::Verdict,
            owner_hash: "a".repeat(64),
        }],
        recovery_lease_owner_hash: "b".repeat(64),
        original_transaction_id: None,
        original_journal_hash: None,
        observed_ledger_head: None,
        profile_hash: "c".repeat(64),
        policy_hash: "d".repeat(64),
        cleanup_plan: RecoveryCleanupPlan::empty(),
        cleanup_plan_hash: String::new(),
    });
    let journal = RecoveryJournal::create(&fixture.runtime(), record).unwrap();
    journal.write_published(&StdFilesystem).unwrap();
    let witness = journal.promote_to_cleanup_witness(&StdFilesystem).unwrap();
    let ops = DenyTerminalParentSync::new(TerminalParentSync::TransactionCleanup);

    witness
        .remove_cleanup_witness_with(&ops)
        .expect("final recovery witness deletion must not require a post-delete parent sync");

    assert_eq!(ops.denied_attempts(), 0);
    assert!(!witness.root.exists());
}

#[test]
fn recovery_cleanup_witness_resumes_only_final_verification_and_lease_tail() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-cleanup-witness-tail");
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert!(fixture
        .apply_with_runtime(
            VerdictVerb::Accept,
            "interrupted before ledger",
            &interrupted_apply
        )
        .is_err());
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("interrupted apply must retain a stale verdict owner");
    let mut first_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterRecoveryCleanupWitness));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut first_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );

    let witness =
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
            .expect("discover cleanup witness")
            .expect("cleanup witness remains after its injected crash");
    assert!(witness.cleanup_form);
    assert!(witness.is_published(&StdFilesystem).unwrap());
    assert_eq!(
        witness.root,
        fixture
            .store_root
            .join("transactions")
            .join(format!(".cleanup-{}", witness.record.recovery_id))
    );
    assert!(
        !fixture
            .store_root
            .join("transactions")
            .join(&witness.record.recovery_id)
            .exists(),
        "the retry must not recreate a regular recovery journal"
    );
    let recovery_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("recovery lease must persist through the cleanup witness");
    assert_eq!(recovery_owner.command_class, "recovery");
    assert_eq!(
        blake3::hash(&canonical_owner_bytes(&recovery_owner).unwrap())
            .to_hex()
            .as_str(),
        witness.record.recovery_lease_owner_hash
    );
    let canon_before = fixture.canon_hashes();
    let indexes_before = fixture.index_hashes();
    let ledger_before = fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
        .expect("capture ledger bytes before cleanup-witness retry");

    let protected_runtime = fixture.runtime_with_ops(RejectProtectedWrites::new(
        fs::canonicalize(&fixture.store_root).expect("canonicalize store root"),
    ));
    let mut retry_human = ScriptedHuman::new(None);
    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &protected_runtime,
        &mut retry_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect("the exact cleanup witness must resume only the final tail");

    assert_eq!(outcome.mode, RecoveryMode::VerdictTransaction);
    assert!(!outcome.rolled_forward);
    assert_eq!(retry_human.show_count(), 0);
    assert_eq!(retry_human.read_count(), 0);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(fixture.index_hashes(), indexes_before);
    assert_eq!(
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).unwrap(),
        ledger_before
    );
    let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
    let inspected = inspect_store(&profile, &fixture.store_root)
        .expect("cleanup witness tail must restore the valid unsealed store");
    assert!(!inspected.sealed);
    assert!(fixture.transactions_empty());
    assert!(fixture.leases_absent());
}

#[test]
fn recovery_lease_tail_verifies_then_releases_and_clean_retry_is_idempotent() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-lease-tail");
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert!(fixture
        .apply_with_runtime(
            VerdictVerb::Accept,
            "interrupted before ledger",
            &interrupted_apply
        )
        .is_err());
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("interrupted apply must retain a stale verdict owner");
    let mut first_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterRecoveryVerification));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut first_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );

    assert!(fixture.transactions_empty());
    let recovery_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("only the recovery lease remains after recovery verification");
    assert_eq!(recovery_owner.command_class, "recovery");
    assert!(valid_receipt_id(&recovery_owner.owner_id));
    let canon_before = fixture.canon_hashes();
    let indexes_before = fixture.index_hashes();
    let ledger_before = fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
        .expect("capture ledger bytes before recovery-lease retry");

    let protected_runtime = fixture.runtime_with_ops(RejectProtectedWrites::new(
        fs::canonicalize(&fixture.store_root).expect("canonicalize store root"),
    ));
    let mut retry_human = ScriptedHuman::new(None);
    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &protected_runtime,
        &mut retry_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect("a lone verified recovery lease must finish its terminal tail");

    assert_eq!(outcome.mode, RecoveryMode::CleanFinalization);
    assert!(!outcome.rolled_forward);
    assert!(!outcome.idempotent);
    assert_eq!(retry_human.show_count(), 0);
    assert_eq!(retry_human.read_count(), 0);
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(fixture.index_hashes(), indexes_before);
    assert_eq!(
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).unwrap(),
        ledger_before
    );
    assert!(fixture.transactions_empty());
    assert!(fixture.leases_absent());

    let durable_before_clean_retry = fixture.durable_hashes();
    let mut clean_human = ScriptedHuman::new(None);
    let clean = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut clean_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:33:00.000000000Z").unwrap()),
    )
    .expect("a clean post-tail retry must be verifier-only and idempotent");
    assert_eq!(clean.mode, RecoveryMode::CleanFinalization);
    assert!(clean.idempotent);
    assert!(!clean.rolled_forward);
    assert_eq!(clean_human.show_count(), 0);
    assert_eq!(clean_human.read_count(), 0);
    assert_eq!(fixture.durable_hashes(), durable_before_clean_retry);
}

#[test]
fn recovery_lease_tail_rejects_an_ordinary_or_malformed_recovery_owner() {
    let ordinary = ProvisionedP3Fixture::with_staged_reject("recovery-tail-ordinary");
    let ordinary_lease = ordinary
        .runtime()
        .acquire_lease(
            LeaseKind::Verdict,
            &format!("rec-{}", "a".repeat(64)),
            "verdict",
        )
        .expect("create ordinary verdict lease");
    let mut ordinary_human = ScriptedHuman::new(None);
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: ordinary.profile_path.clone(),
                store_root: ordinary.store_root.clone(),
            },
            &ordinary.runtime(),
            &mut ordinary_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::RecoveryAmbiguous
    );
    assert_eq!(ordinary_human.show_count(), 0);
    assert_eq!(ordinary_human.read_count(), 0);
    assert!(ordinary.store_root.join("locks/verdict.lock").exists());
    ordinary_lease.release_terminal(&StdFilesystem).unwrap();

    let malformed = ProvisionedP3Fixture::with_staged_reject("recovery-tail-malformed");
    let malformed_lease = malformed
        .runtime()
        .acquire_lease(LeaseKind::Verdict, "not-a-recovery-receipt", "recovery")
        .expect("create malformed recovery-class lease");
    let mut malformed_human = ScriptedHuman::new(None);
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: malformed.profile_path.clone(),
                store_root: malformed.store_root.clone(),
            },
            &malformed.runtime(),
            &mut malformed_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::RecoveryAmbiguous
    );
    assert_eq!(malformed_human.show_count(), 0);
    assert_eq!(malformed_human.read_count(), 0);
    assert!(malformed.store_root.join("locks/verdict.lock").exists());
    malformed_lease.release_terminal(&StdFilesystem).unwrap();

    let bad_token = ProvisionedP3Fixture::with_staged_reject("recovery-tail-bad-token");
    let _bad_token_lease = bad_token
        .runtime()
        .acquire_lease(
            LeaseKind::Verdict,
            &format!("rec-{}", "b".repeat(64)),
            "recovery",
        )
        .expect("create recovery-class lease before token corruption");
    fs::write(
        bad_token.store_root.join("locks/verdict.lock/owner.json"),
        b"{}\n",
    )
    .unwrap();
    let mut token_human = ScriptedHuman::new(None);
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: bad_token.profile_path.clone(),
                store_root: bad_token.store_root.clone(),
            },
            &bad_token.runtime(),
            &mut token_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::RecoveryAmbiguous
    );
    assert!(bad_token.store_root.join("locks/verdict.lock").exists());
}

#[test]
fn recovery_lease_tail_rejects_bad_underlying_state_without_releasing_lease() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-tail-bad-state");
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert!(fixture
        .apply_with_runtime(
            VerdictVerb::Accept,
            "interrupted before ledger",
            &interrupted_apply
        )
        .is_err());
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut first_human =
        ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterRecoveryVerification));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut first_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );
    fs::create_dir(
        fixture
            .store_root
            .join("transactions")
            .join(format!("txn-{}", "f".repeat(64))),
    )
    .unwrap();

    let mut retry_human = ScriptedHuman::new(None);
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &fixture.runtime(),
            &mut retry_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::RecoveryAmbiguous
    );
    assert_eq!(retry_human.show_count(), 0);
    assert_eq!(retry_human.read_count(), 0);
    assert!(fixture.store_root.join("locks/verdict.lock").exists());
}

#[test]
fn recovery_rejects_an_unexpected_staging_temporary_without_writing() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-unexpected-staging-temp");
    let unexpected = fixture.store_root.join("staging/.unexpected-recovery-temp");
    fs::create_dir(&unexpected).expect("create deliberately unknown staging residue");
    let before = fixture.durable_hashes();
    let mut human = ScriptedHuman::new(None);

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect_err("recovery must fail closed on unknown staging residue");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(error.code(), "transaction.recovery_ambiguous");
    assert_eq!(human.show_count(), 0);
    assert_eq!(human.read_count(), 0);
    assert_eq!(fixture.durable_hashes(), before);
}

#[test]
fn recovery_rejects_a_forged_typed_quarantine_binding_without_writing() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-forged-typed-binding");
    let recovery_journal = crash_after_recovery_journal(&fixture);
    let mut forged = recovery_journal.record.clone();
    let original_hash = forged.quarantined_leases[0].owner_hash.clone();
    forged.quarantined_leases[0].owner_hash = if original_hash == "f".repeat(64) {
        "e".repeat(64)
    } else {
        "f".repeat(64)
    };
    fs::write(
        recovery_journal.root.join("recovery.json"),
        canonical_json_bytes(&forged).expect("canonical forged recovery record"),
    )
    .expect("replace test-only recovery record");
    let before = fixture.durable_hashes();
    let mut human = ScriptedHuman::new(None);

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect_err("forged typed recovery binding must not resume");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(error.code(), "transaction.recovery_ambiguous");
    assert_eq!(human.show_count(), 0);
    assert_eq!(human.read_count(), 0);
    assert_eq!(fixture.durable_hashes(), before);
}

#[test]
fn recovery_resumption_rejects_recomputed_profile_or_policy_commitments() {
    for commitment in ["profile", "policy"] {
        let fixture = ProvisionedP3Fixture::with_staged_reject(&format!(
            "recovery-recomputed-{commitment}-commitment"
        ));
        let recovery_journal = crash_after_recovery_journal(&fixture);
        let mut forged = recovery_journal.record.clone();
        match commitment {
            "profile" => forged.profile_hash = different_test_hash(&forged.profile_hash),
            "policy" => forged.policy_hash = different_test_hash(&forged.policy_hash),
            _ => unreachable!(),
        }
        let forged = with_test_recovery_id(forged);
        fs::write(
            recovery_journal.root.join("recovery.json"),
            canonical_json_bytes(&forged).expect("canonical recomputed recovery record"),
        )
        .expect("replace test-only recovery record");
        let before = fixture.durable_hashes();
        let mut human = ScriptedHuman::new(None);

        let error = recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &fixture.runtime(),
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap(),
            ),
        )
        .expect_err("a recomputed record must still bind the live snapshot commitments");

        assert_eq!(error, TransactionError::RecoveryAmbiguous);
        assert_eq!(error.code(), "transaction.recovery_ambiguous");
        assert_eq!(human.show_count(), 0);
        assert_eq!(human.read_count(), 0);
        assert_eq!(fixture.durable_hashes(), before);
    }
}

#[test]
fn recovery_resumption_rejects_recomputed_source_command_or_original_commitment() {
    for tamper in ["command", "original"] {
        let fixture = ProvisionedP3Fixture::with_staged_reject(&format!(
            "recovery-recomputed-{tamper}-commitment"
        ));
        let recovery_journal = crash_after_recovery_journal(&fixture);
        let mut forged = recovery_journal.record.clone();
        match tamper {
            "command" => {
                forged.mode = RecoveryMode::IntakeOnly;
                forged.command_class = "propose".to_owned();
            }
            "original" => {
                let original = forged
                    .original_journal_hash
                    .as_deref()
                    .expect("interrupted verdict recovery binds its original journal");
                forged.original_journal_hash = Some(different_test_hash(original));
            }
            _ => unreachable!(),
        }
        let forged = with_test_recovery_id(forged);
        fs::write(
            recovery_journal.root.join("recovery.json"),
            canonical_json_bytes(&forged).expect("canonical recomputed recovery record"),
        )
        .expect("replace test-only recovery record");
        let before = fixture.durable_hashes();
        let mut human = ScriptedHuman::new(None);

        let error = recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &fixture.runtime(),
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap(),
            ),
        )
        .expect_err("a recomputed record must still bind the original recovery source");

        assert_eq!(error, TransactionError::RecoveryAmbiguous);
        assert_eq!(error.code(), "transaction.recovery_ambiguous");
        assert_eq!(human.show_count(), 0);
        assert_eq!(human.read_count(), 0);
        assert_eq!(fixture.durable_hashes(), before);
    }
}

#[test]
fn recovery_rejects_a_missing_quarantined_owner_without_writing() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-missing-quarantined-owner");
    let recovery_journal = crash_after_recovery_journal(&fixture);
    let binding = recovery_journal.record.quarantined_leases[0].clone();
    let parent = fixture.store_root.join("locks");
    let owner = parent.join(format!(
        ".recovery-{}-{}/owner.json",
        binding.kind.name(),
        binding.owner_hash
    ));
    fs::remove_file(owner).expect("remove test-only quarantined owner");
    let before = fixture.durable_hashes();
    let mut human = ScriptedHuman::new(None);

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect_err("a recovery record without its bound owner must not resume");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(error.code(), "transaction.recovery_ambiguous");
    assert_eq!(human.show_count(), 0);
    assert_eq!(human.read_count(), 0);
    assert_eq!(fixture.durable_hashes(), before);
}

#[test]
fn recovery_rejects_multiple_recovery_journals_without_writing() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-duplicate-recovery-journals");
    for (id_byte, recovered_at) in [
        ('a', "2026-07-14T04:31:00.000000000Z"),
        ('b', "2026-07-14T04:31:01.000000000Z"),
    ] {
        RecoveryJournal::create(
            &fixture.runtime(),
            with_test_recovery_id(RecoveryJournalRecord {
                schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
                recovery_id: format!("txn-{}", id_byte.to_string().repeat(64)),
                mode: RecoveryMode::CleanFinalization,
                command_class: "verdict".to_owned(),
                recovered_at: recovered_at.to_owned(),
                quarantined_leases: vec![RecoveryLeaseBinding {
                    kind: RecoveryLeaseKind::Verdict,
                    owner_hash: "c".repeat(64),
                }],
                recovery_lease_owner_hash: "d".repeat(64),
                original_transaction_id: None,
                original_journal_hash: None,
                observed_ledger_head: None,
                profile_hash: "e".repeat(64),
                policy_hash: "f".repeat(64),
                cleanup_plan: RecoveryCleanupPlan::empty(),
                cleanup_plan_hash: String::new(),
            }),
        )
        .expect("create deliberately competing recovery journal");
    }
    let before = fixture.durable_hashes();
    let mut human = ScriptedHuman::new(None);

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect_err("multiple recovery journals are ambiguous");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(error.code(), "transaction.recovery_ambiguous");
    assert_eq!(human.show_count(), 0);
    assert_eq!(human.read_count(), 0);
    assert_eq!(fixture.durable_hashes(), before);
}

#[test]
fn recovery_rejects_multiple_original_journals_before_quarantining_leases() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-duplicate-original-journals");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "interrupt before recovery",
                &interrupted
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let original = fs::read_dir(fixture.store_root.join("transactions"))
        .expect("read interrupted transaction root")
        .next()
        .expect("one original transaction journal")
        .expect("read original transaction entry")
        .path();
    let duplicate = fixture
        .store_root
        .join("transactions")
        .join(format!("txn-{}", "f".repeat(64)));
    fs::create_dir(&duplicate).expect("create competing original journal root");
    fs::copy(
        original.join("prepared.json"),
        duplicate.join("prepared.json"),
    )
    .expect("copy test-only competing journal bytes");
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("read stale normal lease owner");
    let before = fixture.durable_hashes();
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect_err("multiple original journals are ambiguous");

    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(error.code(), "transaction.recovery_ambiguous");
    assert_eq!(human.show_count(), 1);
    assert_eq!(human.read_count(), 1);
    assert_eq!(fixture.durable_hashes(), before);
}

#[test]
fn recovery_requires_the_exact_phrase_before_touching_live_normal_leases() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-wrong-phrase-live-lease");
    let owner_id = format!("rec-{}", "a".repeat(64));
    let verdict = fixture
        .runtime()
        .acquire_lease(LeaseKind::Verdict, &owner_id, "verdict")
        .expect("acquire live verdict lease");
    let intake = acquire_trusted_intake_lease(&fixture, &owner_id, "verdict");
    let before = fixture.durable_hashes();
    let mut human = ScriptedHuman::new(Some("RECOVER rec-not-the-owner\n".to_owned()));

    let error = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect_err("a wrong recovery phrase must not steal live leases");

    assert_eq!(error, TransactionError::ConfirmationFailed);
    assert_eq!(error.code(), "human.confirmation_failed");
    assert_eq!(human.show_count(), 1);
    assert_eq!(human.read_count(), 1);
    assert!(fixture
        .store_root
        .join("locks/verdict.lock/owner.json")
        .exists());
    assert!(fixture
        .store_root
        .join("locks/intake.lock/owner.json")
        .exists());
    assert_eq!(fixture.durable_hashes(), before);
    drop((verdict, intake));
}

#[test]
fn production_recovery_requires_a_tty_before_any_store_write() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-no-tty");
    let before = fixture.durable_hashes();

    let error = recover_store_with_tty_path(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        Path::new("/definitely/not/a/tty"),
    )
    .expect_err("production recovery without a controlling terminal must fail");

    assert_eq!(error, TransactionError::TtyRequired);
    assert_eq!(error.code(), "human.tty_required");
    assert_eq!(fixture.durable_hashes(), before);
}

#[test]
fn two_recoveries_serialize_on_the_published_recovery_lease() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("two-recovery-race");
    let owner_id = format!("rec-{}", "a".repeat(64));
    let _verdict = fixture
        .runtime()
        .acquire_lease(LeaseKind::Verdict, &owner_id, "verdict")
        .expect("create live stale verdict lease");
    let _intake = acquire_trusted_intake_lease(&fixture, &owner_id, "verdict");
    let operations = RecoveryLeasePublicationBarrier::after_sync_of(
        fs::canonicalize(&fixture.store_root)
            .expect("canonicalize fixture store for runtime")
            .join("locks/verdict.lock"),
    );
    let first_runtime = fixture.runtime_with_ops(operations.clone());
    let first_request = RecoveryRequest {
        profile_path: fixture.profile_path.clone(),
        store_root: fixture.store_root.clone(),
    };
    let second_request = first_request.clone();
    let first_confirmation = format!("RECOVER {owner_id}\n");

    let first = thread::spawn(move || {
        let mut human = ScriptedHuman::new(Some(first_confirmation));
        let result = recover_store_with_human(
            first_request,
            &first_runtime,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap(),
            ),
        );
        (result, human.show_count(), human.read_count())
    });

    operations.wait_until_published();
    let before_loser = fixture.durable_hashes();
    let canon_before_loser = fixture.canon_hashes();
    let ledger_before_loser =
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).expect("read ledger bytes");
    let mut loser_human = ScriptedHuman::new(None);
    let loser = recover_store_with_human(
        second_request,
        &fixture.runtime(),
        &mut loser_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:01.000000000Z").unwrap()),
    );
    assert_eq!(fixture.durable_hashes(), before_loser);
    assert_eq!(fixture.canon_hashes(), canon_before_loser);
    assert_eq!(
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).expect("re-read ledger bytes"),
        ledger_before_loser
    );
    operations.release();
    let (winner, winner_show_count, winner_read_count) =
        first.join().expect("recovery winner thread must not panic");

    assert_eq!(loser, Err(TransactionError::Locked(LeaseKind::Verdict)));
    assert_eq!(
        loser
            .expect_err("second recovery must fail while winner lease is published")
            .code(),
        "store.locked"
    );
    assert_eq!(loser_human.show_count(), 0);
    assert_eq!(loser_human.read_count(), 0);
    let winner = winner.expect("first recovery must finish after release");
    assert_eq!(winner.mode, RecoveryMode::CleanFinalization);
    assert!(!winner.idempotent);
    assert_eq!(winner_show_count, 1);
    assert_eq!(winner_read_count, 1);
    assert!(fixture.leases_absent());
    let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
    assert!(inspect_store(&profile, &fixture.store_root).is_ok());
}

#[test]
fn recovery_rejects_a_quarantined_owner_forged_after_recovery_lease_publication() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-quarantine-owner-race");
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Reject,
                "leave a real stale verdict journal",
                &interrupted_apply,
            )
            .expect_err("inject interrupted verdict state"),
        TransactionError::FaultInjected
    );
    let owner_id = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("read interrupted verdict owner")
    .owner_id;
    let protected_lease = fs::canonicalize(&fixture.store_root)
        .expect("canonicalize fixture store for barrier")
        .join("locks/verdict.lock");
    let operations = RecoveryLeasePublicationBarrier::after_sync_of(protected_lease);
    let runtime = fixture.runtime_with_ops(operations.clone());
    let request = RecoveryRequest {
        profile_path: fixture.profile_path.clone(),
        store_root: fixture.store_root.clone(),
    };
    let confirmation = format!("RECOVER {owner_id}\n");

    let recovery = thread::spawn(move || {
        let mut human = ScriptedHuman::new(Some(confirmation));
        let result = recover_store_with_human(
            request,
            &runtime,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:33:00.000000000Z").unwrap(),
            ),
        );
        (result, human.show_count(), human.read_count())
    });

    operations.wait_until_published();
    let locks = fixture.store_root.join("locks");
    let quarantined = fs::read_dir(&locks)
        .expect("read lock directory after quarantine")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".recovery-verdict-"))
        })
        .expect("stale verdict must be quarantined before recovery lease publication");
    fs::copy(
        locks.join("verdict.lock/owner.json"),
        quarantined.join("owner.json"),
    )
    .expect("replace quarantined owner with a valid but different recovery owner");
    let canon_before = fixture.canon_hashes();
    let ledger_before =
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).expect("read ledger bytes");

    operations.release();
    let (result, show_count, read_count) = recovery.join().expect("recovery thread must not panic");
    let error = result.expect_err("forged quarantined owner must fail closed before journaling");
    assert_eq!(error, TransactionError::RecoveryAmbiguous);
    assert_eq!(error.code(), "transaction.recovery_ambiguous");
    assert_eq!(show_count, 1);
    assert_eq!(read_count, 1);
    assert!(
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
            .expect("inspect recovery journal state")
            .is_none()
    );
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).expect("re-read ledger bytes"),
        ledger_before
    );
    assert!(quarantined.is_dir());
    assert!(locks.join("verdict.lock").is_dir());
    assert!(fixture
        .store_root
        .join("locks")
        .read_dir()
        .expect("read trusted recovery evidence")
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".recovery-verdict-"))
        }));
}

#[test]
fn two_proposals_serialize_on_the_intake_lease_without_a_second_publication() {
    let fixture = ProvisionedP3Fixture::empty("two-proposal-race");
    let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
    let candidate_path = fixture
        .profile_path
        .parent()
        .expect("fixture profile parent")
        .join("two-proposal-race.md");
    fs::write(
        &candidate_path,
        p3_test_candidate_bytes(&fixture.store_root.join("seed/bundle").join(target_path)),
    )
    .expect("write candidate bytes outside the store");
    let first_request = ProposalSubmission {
        profile_path: fixture.profile_path.clone(),
        store_root: fixture.store_root.clone(),
        source_id: "src:agent:two-proposal-race".to_owned(),
        operation: ProposalOperation::Replace,
        intent: ProposalIntent::Accept,
        target_id: "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md".to_owned(),
        target_path: target_path.to_owned(),
        candidate_path: candidate_path.clone(),
        confidence: None,
        evaluator_path: None,
    };
    let second_request = ProposalSubmission {
        profile_path: fixture.profile_path.clone(),
        store_root: fixture.store_root.clone(),
        source_id: "src:agent:two-proposal-race".to_owned(),
        operation: ProposalOperation::Replace,
        intent: ProposalIntent::Accept,
        target_id: "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md".to_owned(),
        target_path: target_path.to_owned(),
        candidate_path,
        confidence: None,
        evaluator_path: None,
    };
    let first = submit_proposal_with_clock(
        first_request,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap()),
    )
    .expect("first public submit writes its untrusted inbox request");
    let second = submit_proposal_with_clock(
        second_request,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:30:01.000000000Z").unwrap()),
    )
    .expect("second public submit remains untrusted and independent");
    assert_eq!(first.state, "submitted");
    assert_eq!(second.state, "submitted");

    let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let first_gate = Arc::clone(&gate);
    let first_profile = fixture.profile_path.clone();
    let first_store = fixture.store_root.clone();
    let first_submission_id = first.submission_id.clone();
    let first = thread::spawn(move || {
        let mut human = BlockingSealHuman {
            confirmation: format!("SEAL INTAKE {first_submission_id}\n"),
            gate: first_gate,
        };
        seal_intake_submission_with_human(
            &first_profile,
            &first_store,
            &first_submission_id,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:30:02.000000000Z").unwrap(),
            ),
        )
    });
    wait_until_seal_is_prompting(&gate);
    let before_loser = fixture.durable_hashes();
    let mut loser_human =
        ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", second.submission_id)));
    let loser = seal_intake_submission_with_human(
        &fixture.profile_path,
        &fixture.store_root,
        &second.submission_id,
        &mut loser_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:30:03.000000000Z").unwrap()),
    )
    .expect_err("second human seal must see the protected intake lease");
    assert_eq!(loser.code(), "intake.locked");
    assert_eq!(fixture.durable_hashes(), before_loser);

    release_blocked_seal(&gate);
    let winner = first
        .join()
        .expect("first human seal thread must not panic")
        .expect("first human seal must publish after release");

    let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
    let store = inspect_store(&profile, &fixture.store_root).expect("verify released store");
    assert_eq!(
        retained_pairs(
            &store.paths.intake_proposals,
            store.policy.policy.max_meta_bytes
        )
        .expect("read protected retained pairs")
        .len(),
        1
    );
    assert!(fixture.leases_absent());
    assert!(winner.proposal_id.is_some());
}

#[test]
fn proposal_rechecks_the_protected_verdict_lease_before_publication() {
    let fixture = ProvisionedP3Fixture::empty("proposal-verdict-publication-race");
    let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
    let candidate_path = fixture
        .profile_path
        .parent()
        .expect("fixture profile parent")
        .join("proposal-verdict-publication-race.md");
    fs::write(
        &candidate_path,
        p3_test_candidate_bytes(&fixture.store_root.join("seed/bundle").join(target_path)),
    )
    .expect("write candidate bytes outside the store");
    let request = ProposalSubmission {
        profile_path: fixture.profile_path.clone(),
        store_root: fixture.store_root.clone(),
        source_id: "src:agent:proposal-verdict-publication-race".to_owned(),
        operation: ProposalOperation::Replace,
        intent: ProposalIntent::Accept,
        target_id: "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md".to_owned(),
        target_path: target_path.to_owned(),
        candidate_path,
        confidence: None,
        evaluator_path: None,
    };
    let verdict = fixture
        .runtime()
        .acquire_lease(
            LeaseKind::Verdict,
            &format!("rec-{}", "a".repeat(64)),
            "verdict",
        )
        .expect("publish a protected verdict lease before public submit");
    assert!(fixture
        .store_root
        .join("locks/verdict.lock/owner.json")
        .exists());

    let before_submit = fixture.durable_hashes();
    let canon_before = fixture.canon_hashes();
    let ledger_before =
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).expect("read ledger bytes");
    let error = submit_proposal(request)
        .expect_err("public submit must not cross an active protected verdict lease");
    assert_eq!(error.code(), "store.locked");
    assert!(!fixture.store_root.join("locks/intake.lock").exists());
    assert!(fixture
        .store_root
        .join("locks/verdict.lock/owner.json")
        .exists());
    assert_eq!(fixture.canon_hashes(), canon_before);
    assert_eq!(
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).expect("re-read ledger bytes"),
        ledger_before
    );
    assert_eq!(fixture.durable_hashes(), before_submit);
    assert!(
        sorted_directory_entries(&fixture.store_root.join("staging/inbox"))
            .expect("read inbox after rejected public submit")
            .is_empty()
    );
    assert!(fs::read(fixture.store_root.join("intake/events.jsonl"))
        .expect("read protected intake authority")
        .is_empty());
    assert!(
        retained_pairs(&fixture.store_root.join("intake/proposals"), 262_144,)
            .expect("read protected retained pairs")
            .is_empty()
    );

    verdict
        .release_terminal(&StdFilesystem)
        .expect("release test verdict lease");
    let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
    assert!(inspect_store(&profile, &fixture.store_root).is_ok());
    assert!(fixture.leases_absent());
}

#[test]
fn two_verdicts_serialize_before_the_loser_reads_or_prompts() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("two-verdict-race");
    let protected_lease = fs::canonicalize(&fixture.store_root)
        .expect("canonicalize fixture store")
        .join("locks/verdict.lock");
    let barrier = RecoveryLeasePublicationBarrier::after_sync_of(protected_lease);
    let first_runtime = fixture.runtime_with_ops(barrier.clone());
    let first_request = fixture
        .request_for(VerdictVerb::Reject, "winner deliberately aborts")
        .expect("construct first verdict request");
    let first_profile = fixture.profile_path.clone();
    let first_store = fixture.store_root.clone();
    let first = thread::spawn(move || {
        let mut human = ScriptedHuman::new(Some("APPLY REJECT wrong-id\n".to_owned()));
        let result = preflight_human_verdict_from_store_with_human(
            &first_profile,
            &first_store,
            first_request,
            &first_runtime,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap(),
            ),
        );
        (result, human.show_count(), human.read_count())
    });

    barrier.wait_until_published();
    let before_loser = fixture.durable_hashes();
    let canon_before_loser = fixture.canon_hashes();
    let ledger_before_loser =
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).expect("read ledger bytes");
    let second_request = fixture
        .request_for(VerdictVerb::Reject, "loser must not read the proposal pair")
        .expect("construct second verdict request");
    let mut loser_human = ScriptedHuman::new(None);
    let loser = preflight_human_verdict_from_store_with_human(
        &fixture.profile_path,
        &fixture.store_root,
        second_request,
        &fixture.runtime(),
        &mut loser_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:01.000000000Z").unwrap()),
    );
    assert_eq!(fixture.durable_hashes(), before_loser);
    assert_eq!(fixture.canon_hashes(), canon_before_loser);
    assert_eq!(
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).expect("re-read ledger bytes"),
        ledger_before_loser
    );
    barrier.release();
    let (winner, winner_show_count, winner_read_count) =
        first.join().expect("first verdict thread must not panic");

    let loser = match loser {
        Err(error) => error,
        Ok(_) => panic!("second verdict must fail before proposal read"),
    };
    assert_eq!(loser, TransactionError::Locked(LeaseKind::Verdict));
    assert_eq!(loser.code(), "store.locked");
    assert_eq!(loser_human.show_count(), 0);
    assert_eq!(loser_human.read_count(), 0);
    let winner = match winner {
        Err(error) => error,
        Ok(_) => panic!("first verdict must abort at its wrong confirmation"),
    };
    assert_eq!(winner, TransactionError::ConfirmationFailed);
    assert_eq!(winner_show_count, 1);
    assert_eq!(winner_read_count, 1);
    assert!(fixture.leases_absent());
    let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
    assert!(inspect_store(&profile, &fixture.store_root).is_ok());
    assert_eq!(fixture.canon_hashes(), canon_before_loser);
    assert_eq!(
        fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).expect("final ledger bytes"),
        ledger_before_loser
    );
}

fn crash_after_recovery_journal(fixture: &ProvisionedP3Fixture) -> RecoveryJournal {
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Accept,
                "interrupt before recovery journal",
                &interrupted_apply,
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("read stale owner before recovery");
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterRecoveryJournalCreation));
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );
    assert_eq!(human.show_count(), 1);
    assert_eq!(human.read_count(), 1);
    RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
        .expect("discover exact interrupted recovery journal")
        .expect("an interrupted recovery must retain its journal")
}

fn crash_after_recovery_lease_acquisition(fixture: &ProvisionedP3Fixture) -> (String, String) {
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Reject,
                "interrupt before recovery bootstrap",
                &interrupted_apply,
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let original_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("read original stale owner before bootstrap recovery");
    let interrupted_recovery =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterRecoveryLeaseAcquisition));
    let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", original_owner.owner_id)));
    assert_eq!(
        recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-15T04:30:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_err(),
        TransactionError::FaultInjected
    );
    let recovery_owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .expect("read protected recovery owner after bootstrap crash");
    (original_owner.owner_id, recovery_owner.owner_id)
}

#[test]
fn recovery_plan_replays_the_authoritative_v2_sealed_pair() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-v2-sealed-pair");
    let interrupted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Reject,
                "interrupt for sealed-pair replay",
                &interrupted
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );

    let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
    let snapshot = sovmem::profile::inspect_store_with_transients(&profile, &fixture.store_root)
        .expect("inspect interrupted fixture store");
    let journal = discover_original_journal(&StdFilesystem, &snapshot.paths.transactions)
        .expect("discover prepared fixture journal");
    let plan = &journal
        .plan
        .as_ref()
        .expect("prepared journal has a transaction plan")
        .plan;

    let pair = validate_plan_source_pair(&snapshot, plan)
        .expect("recovery must replay the authoritative v2 sealed proposal pair");
    assert_eq!(pair.meta.proposal_id, fixture.proposal_id);
}

#[test]
fn recovery_record_binds_typed_quarantines_and_exact_recovery_lease() {
    let record = with_test_recovery_id(RecoveryJournalRecord {
        schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
        recovery_id: format!("txn-{}", "a".repeat(64)),
        mode: RecoveryMode::VerdictTransaction,
        command_class: "verdict".to_owned(),
        recovered_at: "2026-07-14T04:31:00.000000000Z".to_owned(),
        quarantined_leases: vec![
            RecoveryLeaseBinding {
                kind: RecoveryLeaseKind::Intake,
                owner_hash: "a".repeat(64),
            },
            RecoveryLeaseBinding {
                kind: RecoveryLeaseKind::Verdict,
                owner_hash: "a".repeat(64),
            },
        ],
        recovery_lease_owner_hash: "b".repeat(64),
        original_transaction_id: Some(format!("txn-{}", "c".repeat(64))),
        original_journal_hash: Some("d".repeat(64)),
        observed_ledger_head: Some("e".repeat(64)),
        profile_hash: "f".repeat(64),
        policy_hash: "0".repeat(64),
        cleanup_plan: RecoveryCleanupPlan::empty(),
        cleanup_plan_hash: String::new(),
    });

    assert!(record.validate().is_ok());
    let bytes = canonical_json_bytes(&record).unwrap();
    assert!(String::from_utf8(bytes)
        .unwrap()
        .contains("\"recovery_lease_owner_hash\": \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\""));

    let mut unordered = record.clone();
    unordered.quarantined_leases.reverse();
    assert_eq!(
        unordered.validate(),
        Err(TransactionError::RecoveryAmbiguous)
    );
    let mut substituted_plan = record.clone();
    substituted_plan.cleanup_plan_hash = "0".repeat(64);
    assert_eq!(
        substituted_plan.validate(),
        Err(TransactionError::RecoveryAmbiguous),
        "the cleanup plan hash is part of recovery identity"
    );
}

#[test]
fn recovery_record_schema_requires_source_command_and_snapshot_commitments() {
    let legacy = serde_json::json!({
        "schema_version": RECOVERY_JOURNAL_SCHEMA,
        "recovery_id": format!("txn-{}", "a".repeat(64)),
        "mode": "verdict-transaction",
        "recovered_at": "2026-07-14T04:31:00.000000000Z",
        "quarantined_leases": [{
            "kind": "verdict",
            "owner_hash": "b".repeat(64),
        }],
        "recovery_lease_owner_hash": "c".repeat(64),
        "original_transaction_id": null,
        "original_journal_hash": null,
        "observed_ledger_head": null,
    });

    assert!(
        serde_json::from_value::<RecoveryJournalRecord>(legacy).is_err(),
        "a recovery record without source command, profile, and policy commitments must fail closed"
    );
}

#[test]
fn recovery_cleanup_plan_schema_rejects_unknown_absolute_dot_duplicate_and_wrong_order_steps() {
    let file = |path: &str| {
        serde_json::json!({
            "artifact_kind": "generation",
            "path": path,
            "entry_type": "file",
            "byte_length": 1,
            "content_hash": "a".repeat(64),
        })
    };
    let directory = |path: &str| {
        serde_json::json!({
            "artifact_kind": "generation",
            "path": path,
            "entry_type": "directory",
        })
    };
    for steps in [
        vec![file("/absolute")],
        vec![file("canon/generations/../escape")],
        vec![file("canon//generations/gen-0000000000000001/file")],
        vec![file("canon/generations/gen-0000000000000001/file/")],
        vec![
            file("canon/generations/gen-1/file"),
            file("canon/generations/gen-1/file"),
        ],
        vec![
            file("canon/generations/gen-0000000000000001/file"),
            file("canon//generations/gen-0000000000000001/file"),
        ],
        vec![
            directory("canon/generations/gen-1"),
            file("canon/generations/gen-1/file"),
        ],
    ] {
        let plan: RecoveryCleanupPlan = serde_json::from_value(serde_json::json!({
            "schema_version": RECOVERY_CLEANUP_PLAN_SCHEMA,
            "steps": steps,
        }))
        .unwrap();
        assert_eq!(plan.validate(), Err(TransactionError::RecoveryAmbiguous));
    }
    assert!(
        serde_json::from_value::<RecoveryCleanupPlan>(serde_json::json!({
            "schema_version": RECOVERY_CLEANUP_PLAN_SCHEMA,
            "steps": [],
            "unknown": true,
        }))
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn recovery_journal_verify_rejects_recovery_record_replaced_by_a_symlink() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-record-symlink");
    let record = with_test_recovery_id(RecoveryJournalRecord {
        schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
        recovery_id: format!("txn-{}", "a".repeat(64)),
        mode: RecoveryMode::CleanFinalization,
        command_class: "verdict".to_owned(),
        recovered_at: "2026-07-14T04:31:00.000000000Z".to_owned(),
        quarantined_leases: vec![RecoveryLeaseBinding {
            kind: RecoveryLeaseKind::Verdict,
            owner_hash: "b".repeat(64),
        }],
        recovery_lease_owner_hash: "c".repeat(64),
        original_transaction_id: None,
        original_journal_hash: None,
        observed_ledger_head: None,
        profile_hash: "d".repeat(64),
        policy_hash: "e".repeat(64),
        cleanup_plan: RecoveryCleanupPlan::empty(),
        cleanup_plan_hash: String::new(),
    });
    let journal = RecoveryJournal::create(&fixture.runtime(), record).unwrap();
    let replacement = fixture.store_root.join("recovery-record-replacement.json");
    fs::write(&replacement, &journal.bytes).unwrap();
    fs::remove_file(journal.root.join("recovery.json")).unwrap();
    std::os::unix::fs::symlink(&replacement, journal.root.join("recovery.json")).unwrap();

    assert_eq!(
        journal.verify(&StdFilesystem),
        Err(TransactionError::JournalState),
        "verification must freshly reject a replacement symlink even when its bytes match"
    );
}

#[cfg(unix)]
#[test]
fn recovery_journal_marker_verification_rejects_a_published_marker_symlink() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-marker-symlink");
    let record = with_test_recovery_id(RecoveryJournalRecord {
        schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
        recovery_id: format!("txn-{}", "a".repeat(64)),
        mode: RecoveryMode::CleanFinalization,
        command_class: "verdict".to_owned(),
        recovered_at: "2026-07-14T04:31:00.000000000Z".to_owned(),
        quarantined_leases: vec![RecoveryLeaseBinding {
            kind: RecoveryLeaseKind::Verdict,
            owner_hash: "b".repeat(64),
        }],
        recovery_lease_owner_hash: "c".repeat(64),
        original_transaction_id: None,
        original_journal_hash: None,
        observed_ledger_head: None,
        profile_hash: "d".repeat(64),
        policy_hash: "e".repeat(64),
        cleanup_plan: RecoveryCleanupPlan::empty(),
        cleanup_plan_hash: String::new(),
    });
    let journal = RecoveryJournal::create(&fixture.runtime(), record).unwrap();
    journal.write_published(&StdFilesystem).unwrap();
    let replacement = fixture.store_root.join("published-marker-replacement.json");
    fs::write(
        &replacement,
        fs::read(journal.root.join("published.json")).unwrap(),
    )
    .unwrap();
    fs::remove_file(journal.root.join("published.json")).unwrap();
    std::os::unix::fs::symlink(&replacement, journal.root.join("published.json")).unwrap();

    assert_eq!(
        journal.verify_published_marker(&StdFilesystem),
        Err(TransactionError::RecoveryAmbiguous),
        "marker verification must freshly reject a replacement symlink even when its bytes match"
    );
}

#[test]
fn recovery_journal_discovery_accepts_only_exact_marker_prefixes() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-journal-discovery");
    let record = with_test_recovery_id(RecoveryJournalRecord {
        schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
        recovery_id: format!("txn-{}", "a".repeat(64)),
        mode: RecoveryMode::CleanFinalization,
        command_class: "verdict".to_owned(),
        recovered_at: "2026-07-14T04:31:00.000000000Z".to_owned(),
        quarantined_leases: vec![RecoveryLeaseBinding {
            kind: RecoveryLeaseKind::Verdict,
            owner_hash: "a".repeat(64),
        }],
        recovery_lease_owner_hash: "b".repeat(64),
        original_transaction_id: None,
        original_journal_hash: None,
        observed_ledger_head: None,
        profile_hash: "c".repeat(64),
        policy_hash: "d".repeat(64),
        cleanup_plan: RecoveryCleanupPlan::empty(),
        cleanup_plan_hash: String::new(),
    });
    let journal = RecoveryJournal::create(&fixture.runtime(), record.clone()).unwrap();
    let discovered =
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
            .expect("exact recovery journal discovery")
            .expect("one recovery journal");
    assert_eq!(discovered.record, record);

    journal.write_published(&StdFilesystem).unwrap();
    assert!(
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"),)
            .is_ok()
    );

    fs::write(journal.root.join("unexpected.json"), b"{}\n").unwrap();
    assert!(matches!(
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions")),
        Err(TransactionError::RecoveryAmbiguous)
    ));
}

#[test]
fn recovery_journal_discovery_accepts_the_exact_cleanup_witness() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-journal-cleanup");
    let record = with_test_recovery_id(RecoveryJournalRecord {
        schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
        recovery_id: format!("txn-{}", "a".repeat(64)),
        mode: RecoveryMode::CleanFinalization,
        command_class: "verdict".to_owned(),
        recovered_at: "2026-07-14T04:31:00.000000000Z".to_owned(),
        quarantined_leases: vec![RecoveryLeaseBinding {
            kind: RecoveryLeaseKind::Verdict,
            owner_hash: "a".repeat(64),
        }],
        recovery_lease_owner_hash: "b".repeat(64),
        original_transaction_id: None,
        original_journal_hash: None,
        observed_ledger_head: None,
        profile_hash: "c".repeat(64),
        policy_hash: "d".repeat(64),
        cleanup_plan: RecoveryCleanupPlan::empty(),
        cleanup_plan_hash: String::new(),
    });
    let journal = RecoveryJournal::create(&fixture.runtime(), record.clone()).unwrap();
    journal.write_published(&StdFilesystem).unwrap();
    let cleanup = fixture
        .store_root
        .join("transactions")
        .join(format!(".cleanup-{}", record.recovery_id));
    fs::create_dir(&cleanup).unwrap();
    for name in ["recovery.json", "published.json"] {
        fs::hard_link(journal.root.join(name), cleanup.join(name)).unwrap();
        fs::remove_file(journal.root.join(name)).unwrap();
    }
    fs::remove_dir(&journal.root).unwrap();

    let discovered =
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
            .expect("exact cleanup witness discovery")
            .expect("one recovery cleanup witness");
    assert_eq!(discovered.record, record);
    assert_eq!(discovered.root, cleanup);
}

#[test]
fn recovery_cleanup_witness_discovery_rejects_mixed_malformed_and_duplicate_forms() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("recovery-journal-negative");
    let record = with_test_recovery_id(RecoveryJournalRecord {
        schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
        recovery_id: format!("txn-{}", "a".repeat(64)),
        mode: RecoveryMode::CleanFinalization,
        command_class: "verdict".to_owned(),
        recovered_at: "2026-07-14T04:31:00.000000000Z".to_owned(),
        quarantined_leases: vec![RecoveryLeaseBinding {
            kind: RecoveryLeaseKind::Verdict,
            owner_hash: "a".repeat(64),
        }],
        recovery_lease_owner_hash: "b".repeat(64),
        original_transaction_id: None,
        original_journal_hash: None,
        observed_ledger_head: None,
        profile_hash: "c".repeat(64),
        policy_hash: "d".repeat(64),
        cleanup_plan: RecoveryCleanupPlan::empty(),
        cleanup_plan_hash: String::new(),
    });
    let journal = RecoveryJournal::create(&fixture.runtime(), record.clone()).unwrap();
    journal.write_published(&StdFilesystem).unwrap();
    let cleanup = fixture
        .store_root
        .join("transactions")
        .join(format!(".cleanup-{}", record.recovery_id));
    fs::create_dir(&cleanup).unwrap();
    fs::hard_link(
        journal.root.join("published.json"),
        cleanup.join("published.json"),
    )
    .unwrap();
    assert!(matches!(
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions")),
        Err(TransactionError::RecoveryAmbiguous)
    ));

    fs::remove_file(cleanup.join("published.json")).unwrap();
    for name in ["recovery.json", "published.json"] {
        fs::hard_link(journal.root.join(name), cleanup.join(name)).unwrap();
        fs::remove_file(journal.root.join(name)).unwrap();
    }
    fs::remove_dir(&journal.root).unwrap();
    fs::write(cleanup.join("unexpected.json"), b"{}\n").unwrap();
    assert!(matches!(
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions")),
        Err(TransactionError::RecoveryAmbiguous)
    ));
    fs::remove_file(cleanup.join("unexpected.json")).unwrap();

    let mut duplicate_record = record.clone();
    duplicate_record.recovered_at = "2026-07-14T04:31:01.000000000Z".to_owned();
    let duplicate_record = with_test_recovery_id(duplicate_record);
    let duplicate = RecoveryJournal::create(&fixture.runtime(), duplicate_record.clone()).unwrap();
    duplicate.write_published(&StdFilesystem).unwrap();
    let duplicate_cleanup = fixture
        .store_root
        .join("transactions")
        .join(format!(".cleanup-{}", duplicate_record.recovery_id));
    fs::create_dir(&duplicate_cleanup).unwrap();
    for name in ["recovery.json", "published.json"] {
        fs::hard_link(duplicate.root.join(name), duplicate_cleanup.join(name)).unwrap();
        fs::remove_file(duplicate.root.join(name)).unwrap();
    }
    fs::remove_dir(&duplicate.root).unwrap();
    assert!(matches!(
        RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions")),
        Err(TransactionError::RecoveryAmbiguous)
    ));
}

#[test]
fn c2b1_discovery_accepts_the_complete_plan_bound_cleanup_witness() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("original-journal-cleanup");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::DuringCleanup));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Reject, "retain final journal", &runtime)
            .unwrap_err(),
        TransactionError::JournalState
    );
    let transactions = fixture.store_root.join("transactions");
    let original = fs::read_dir(&transactions)
        .unwrap()
        .next()
        .expect("retained original journal")
        .unwrap()
        .path();
    let transaction_id = original.file_name().unwrap().to_str().unwrap().to_owned();
    let cleanup = transactions.join(format!(".cleanup-{transaction_id}"));
    fs::create_dir(&cleanup).unwrap();
    for name in [
        "plan.json",
        "prepared.json",
        "ledger-committed.json",
        "published.json",
    ] {
        fs::hard_link(original.join(name), cleanup.join(name)).unwrap();
        fs::remove_file(original.join(name)).unwrap();
    }
    fs::remove_dir(&original).unwrap();

    let discovered = discover_original_journal(&StdFilesystem, &transactions)
        .expect("exact original cleanup witness discovery");
    assert_eq!(discovered.transaction_id(), Some(transaction_id.as_str()));
    assert!(discovered.plan.is_some());
    assert!(discovered.prepared().is_some());
    assert_eq!(
        discovered.files,
        [
            "ledger-committed.json",
            "plan.json",
            "prepared.json",
            "published.json",
        ]
    );
    assert!(discovered.commitment_hash().is_some());
}

#[test]
fn c2b1_discovery_rejects_and_retains_an_unpaired_empty_transaction_root() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("original-journal-empty-root");
    let transactions = fixture.store_root.join("transactions");
    let transaction_id = format!("txn-{}", "a".repeat(64));
    let root = transactions.join(&transaction_id);
    fs::create_dir(&root).unwrap();

    assert!(matches!(
        discover_original_journal(&StdFilesystem, &transactions),
        Err(TransactionError::RecoveryAmbiguous)
    ));
    assert!(root.is_dir());
    assert!(fs::read_dir(&root).unwrap().next().is_none());
}

#[test]
fn c2b1_plan_only_discovery_rechecks_the_plan_after_commitment_read() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("original-journal-plan-race");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanSync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "retain plan-only journal", &runtime)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transactions = fixture.store_root.join("transactions");
    let root = fs::read_dir(&transactions)
        .unwrap()
        .next()
        .expect("retained plan-only journal")
        .unwrap()
        .path();
    let plan_path = root.join("plan.json");
    let mut replacement: TransactionPlan =
        serde_json::from_slice(&fs::read(&plan_path).unwrap()).unwrap();
    replacement.reason_hash = "f".repeat(64);
    let ops =
        SwapPlanAfterCommitmentRead::new(plan_path, canonical_json_bytes(&replacement).unwrap());
    assert!(matches!(
        discover_original_journal(&ops, &transactions),
        Err(TransactionError::RecoveryAmbiguous)
    ));
}

#[test]
fn c2b1_plan_only_discovery_preserves_its_raw_prefix_commitment() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("original-journal-plan-only");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanSync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "retain plan-only journal", &runtime)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transactions = fixture.store_root.join("transactions");
    let journal = discover_original_journal(&StdFilesystem, &transactions)
        .expect("discover plan-only journal");
    let root = journal.root.as_ref().expect("plan-only root");
    assert!(journal.plan.is_some());
    assert!(journal.prepared().is_none());
    assert_eq!(journal.files, ["plan.json"]);
    assert_eq!(
        journal.commitment_hash(),
        Some(
            original_journal_commitment(&StdFilesystem, root, &journal.files)
                .expect("reconstruct plan-only commitment")
        )
    );
}

#[test]
fn c2b1_partial_plan_only_journal_promotes_to_an_exact_cleanup_witness() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("original-journal-partial-cleanup");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanSync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "retain plan-only journal", &runtime)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transactions = fixture.store_root.join("transactions");
    let journal = discover_original_journal(&StdFilesystem, &transactions)
        .expect("discover plan-only journal");
    let transaction_id = journal.transaction_id().expect("transaction id").to_owned();
    let commitment = journal.commitment_hash();
    let witness = journal
        .promote_to_cleanup_witness(&StdFilesystem)
        .expect("a verified non-empty original prefix must be promotable for recovery");
    assert!(witness.cleanup_form);
    assert_eq!(witness.files, ["plan.json"]);
    assert!(witness.plan.is_some());
    assert!(witness.prepared().is_none());
    assert_eq!(witness.commitment_hash(), commitment);
    assert_eq!(
        witness.root,
        Some(transactions.join(format!(".cleanup-{transaction_id}")))
    );
    assert!(!transactions.join(&transaction_id).exists());
    let discovered = discover_original_journal(&StdFilesystem, &transactions)
        .expect("discover the exact plan-only cleanup witness");
    assert!(discovered.cleanup_form);
    assert_eq!(discovered.files, ["plan.json"]);
    assert_eq!(discovered.commitment_hash(), commitment);
}

#[test]
fn c2b1_partial_cleanup_witness_accepts_only_matching_transition_and_rejects_tampering() {
    let fixture =
        ProvisionedP3Fixture::with_staged_reject("original-journal-partial-cleanup-tamper");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanSync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "retain plan-only journal", &runtime)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    let transactions = fixture.store_root.join("transactions");
    let journal = discover_original_journal(&StdFilesystem, &transactions)
        .expect("discover plan-only journal");
    let transaction_id = journal.transaction_id().expect("transaction id").to_owned();
    let witness = journal
        .promote_to_cleanup_witness(&StdFilesystem)
        .expect("promote plan-only journal");
    let witness_root = witness.root.expect("cleanup witness root");

    let regular = transactions.join(&transaction_id);
    fs::create_dir(&regular).unwrap();
    fs::hard_link(witness_root.join("plan.json"), regular.join("plan.json")).unwrap();
    let transition = discover_original_journal(&StdFilesystem, &transactions)
        .expect("the byte-identical regular source and cleanup prefix are resumable");
    assert!(!transition.cleanup_form);
    assert_eq!(transition.files, ["plan.json"]);
    fs::remove_file(regular.join("plan.json")).unwrap();
    fs::remove_dir(&regular).unwrap();

    let mut tampered = fs::read(witness_root.join("plan.json")).unwrap();
    tampered.extend_from_slice(b"\n");
    fs::write(witness_root.join("plan.json"), tampered).unwrap();
    assert!(matches!(
        discover_original_journal(&StdFilesystem, &transactions),
        Err(TransactionError::RecoveryAmbiguous)
    ));
}

#[test]
fn c2b1_discovery_rejects_a_cleanup_witness_missing_its_plan() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("original-journal-missing-plan");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::DuringCleanup));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Reject, "retain final journal", &runtime)
            .unwrap_err(),
        TransactionError::JournalState
    );
    let transactions = fixture.store_root.join("transactions");
    let original = fs::read_dir(&transactions)
        .unwrap()
        .next()
        .expect("retained original journal")
        .unwrap()
        .path();
    let transaction_id = original.file_name().unwrap().to_str().unwrap().to_owned();
    let cleanup = transactions.join(format!(".cleanup-{transaction_id}"));
    fs::create_dir(&cleanup).unwrap();
    for name in ["prepared.json", "ledger-committed.json", "published.json"] {
        fs::hard_link(original.join(name), cleanup.join(name)).unwrap();
    }
    for name in [
        "plan.json",
        "prepared.json",
        "ledger-committed.json",
        "published.json",
    ] {
        fs::remove_file(original.join(name)).unwrap();
    }
    fs::remove_dir(&original).unwrap();
    assert!(matches!(
        discover_original_journal(&StdFilesystem, &transactions),
        Err(TransactionError::RecoveryAmbiguous)
    ));
}

#[test]
fn c2b1_discovery_rejects_a_mixed_cleanup_witness_plan() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("original-journal-mixed-plan");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::DuringCleanup));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Reject, "retain final journal", &runtime)
            .unwrap_err(),
        TransactionError::JournalState
    );
    let transactions = fixture.store_root.join("transactions");
    let original = fs::read_dir(&transactions)
        .unwrap()
        .next()
        .expect("retained original journal")
        .unwrap()
        .path();
    let transaction_id = original.file_name().unwrap().to_str().unwrap().to_owned();
    let cleanup = transactions.join(format!(".cleanup-{transaction_id}"));
    fs::create_dir(&cleanup).unwrap();

    let mut substitute: TransactionPlan =
        serde_json::from_slice(&fs::read(original.join("plan.json")).unwrap()).unwrap();
    substitute.transaction_id = format!("txn-{}", "f".repeat(64));
    let substitute_path = fixture.store_root.join("substitute-plan.json");
    fs::write(&substitute_path, canonical_json_bytes(&substitute).unwrap()).unwrap();
    fs::hard_link(&substitute_path, cleanup.join("plan.json")).unwrap();
    for name in ["prepared.json", "ledger-committed.json", "published.json"] {
        fs::hard_link(original.join(name), cleanup.join(name)).unwrap();
    }
    for name in [
        "plan.json",
        "prepared.json",
        "ledger-committed.json",
        "published.json",
    ] {
        fs::remove_file(original.join(name)).unwrap();
    }
    fs::remove_dir(&original).unwrap();
    assert!(matches!(
        discover_original_journal(&StdFilesystem, &transactions),
        Err(TransactionError::RecoveryAmbiguous)
    ));
}

#[test]
fn c2b1_discovery_rejects_prepared_plan_substitution_and_marker_misframing() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("original-journal-plan-substitution");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::DuringCleanup));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Reject, "retain final journal", &runtime)
            .unwrap_err(),
        TransactionError::JournalState
    );
    let transactions = fixture.store_root.join("transactions");
    let original = fs::read_dir(&transactions)
        .unwrap()
        .next()
        .expect("retained original journal")
        .unwrap()
        .path();
    let mut substituted: TransactionPlan =
        serde_json::from_slice(&fs::read(original.join("plan.json")).unwrap()).unwrap();
    substituted.reason_hash = "f".repeat(64);
    fs::write(
        original.join("plan.json"),
        canonical_json_bytes(&substituted).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        discover_original_journal(&StdFilesystem, &transactions),
        Err(TransactionError::RecoveryAmbiguous)
    ));

    let fixture = ProvisionedP3Fixture::with_staged_reject("original-journal-marker-framing");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::DuringCleanup));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Reject, "retain final journal", &runtime)
            .unwrap_err(),
        TransactionError::JournalState
    );
    let transactions = fixture.store_root.join("transactions");
    let original = fs::read_dir(&transactions)
        .unwrap()
        .next()
        .expect("retained original journal")
        .unwrap()
        .path();
    let transaction_id = original.file_name().unwrap().to_str().unwrap().to_owned();
    fs::write(
        original.join("ledger-committed.json"),
        canonical_json_bytes(&JournalMarker {
            schema_version: JOURNAL_SCHEMA.to_owned(),
            transaction_id,
            stage: "published".to_owned(),
        })
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        discover_original_journal(&StdFilesystem, &transactions),
        Err(TransactionError::RecoveryAmbiguous)
    ));
}

#[test]
fn first_ledger_only_verdict_publishes_genesis_and_verdict_in_one_replacement() {
    let interrupted =
        P3TestFixture::with_staged_proposal("first-verdict-interrupted", VerdictVerb::Reject);
    let canon_before = interrupted.canon_hashes();
    let runtime = interrupted.runtime_with_fault(FaultPlan::at(FaultPoint::BeforeLedgerRename));
    assert_eq!(
        interrupted
            .apply_with_runtime(VerdictVerb::Reject, "atomic first verdict", &runtime)
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    assert_eq!(
        fs::read(interrupted.store_root.join("ledger/verdicts.jsonl")).unwrap(),
        b""
    );
    assert_eq!(interrupted.canon_hashes(), canon_before);
    assert!(!interrupted.transactions_empty());
    assert!(!interrupted.leases_absent());

    let fixture = P3TestFixture::with_staged_proposal("first-verdict", VerdictVerb::Reject);
    let outcome = fixture
        .apply(VerdictVerb::Reject, "atomic first verdict")
        .unwrap();
    let view = read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl")).unwrap();
    assert_eq!(view.events.len(), 2);
    assert_eq!(view.events[0].event_id(), "evt-0000000000000000");
    assert_eq!(view.events[1].event_id(), outcome.event_id);
    assert_eq!(view.events[1].previous_hash(), Some(view.events[0].hash()));
}

#[test]
fn precommit_faults_leave_the_unsealed_ledger_and_canonical_head_unchanged() {
    for point in [
        FaultPoint::AfterPreparedMarker,
        FaultPoint::AfterLedgerTemporarySync,
        FaultPoint::BeforeLedgerRename,
    ] {
        let fixture = P3TestFixture::with_staged_proposal("precommit-fault", VerdictVerb::Reject);
        let canon_before = fixture.canon_hashes();
        let runtime = fixture.runtime_with_fault(FaultPlan::at(point));
        assert_eq!(
            fixture
                .apply_with_runtime(VerdictVerb::Reject, "precommit fault", &runtime)
                .unwrap_err(),
            TransactionError::FaultInjected
        );
        assert!(
            read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
                .unwrap()
                .events
                .is_empty()
        );
        assert_eq!(fixture.canon_hashes(), canon_before);
        assert!(!fixture.transactions_empty());
        assert!(!fixture.leases_absent());
    }
}

#[test]
fn c2a_plan_is_durable_before_any_canonical_or_ledger_staging() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("c2a-plan-before-stage");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPlanSync));

    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "fault after durable plan", &runtime)
            .unwrap_err(),
        TransactionError::FaultInjected
    );

    let transactions = fixture.store_root.join("transactions");
    let root = fs::read_dir(&transactions)
        .expect("read transaction root")
        .next()
        .expect("one plan-only transaction root")
        .expect("read transaction entry")
        .path();
    let names = fs::read_dir(&root)
        .expect("read plan-only root")
        .map(|entry| {
            entry
                .expect("read plan entry")
                .file_name()
                .into_string()
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["plan.json"]);
    let plan_bytes = fs::read(root.join("plan.json")).expect("read plan bytes");
    let plan: TransactionPlan = serde_json::from_slice(&plan_bytes).expect("parse plan");
    assert_eq!(canonical_json_bytes(&plan).unwrap(), plan_bytes);
    let mut bound_owner_hashes = vec![
        blake3::hash(
            &fs::read(fixture.store_root.join("locks/verdict.lock/owner.json"))
                .expect("read verdict lock owner bound to its path"),
        )
        .to_hex()
        .to_string(),
        blake3::hash(
            &fs::read(fixture.store_root.join("locks/intake.lock/owner.json"))
                .expect("read intake lock owner bound to its path"),
        )
        .to_hex()
        .to_string(),
    ];
    bound_owner_hashes.sort();
    assert_eq!(plan.owner_hashes, bound_owner_hashes);
    assert_eq!(plan.generation_id, Some("gen-0000000000000001".to_owned()));
    assert!(fs::read_dir(fixture.store_root.join("canon/generations"))
        .unwrap()
        .next()
        .is_none());
    assert!(fs::read_dir(fixture.store_root.join("derived/indexes"))
        .unwrap()
        .next()
        .is_none());
    assert!(fs::read_dir(fixture.store_root.join("ledger"))
        .unwrap()
        .all(|entry| !entry
            .expect("read ledger entry")
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".verdicts-"))));
}

#[test]
fn c2a_prepared_binds_exact_ledger_successor_for_applying_and_ledger_only_verdicts() {
    for (name, verb) in [
        ("c2a-ledger-successor-applying", VerdictVerb::Accept),
        ("c2a-ledger-successor-ledger-only", VerdictVerb::Reject),
    ] {
        let fixture = ProvisionedP3Fixture::with_staged_reject(name);
        let runtime =
            fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
        assert_eq!(
            fixture
                .apply_with_runtime(verb, "retain exact ledger successor", &runtime)
                .unwrap_err(),
            TransactionError::FaultInjected,
            "{verb:?}"
        );
        let journal =
            discover_original_journal(&StdFilesystem, &fixture.store_root.join("transactions"))
                .expect("discover plan plus prepared journal");
        let prepared = journal.prepared.as_ref().expect("prepared journal");
        let plan_bytes = fs::read(journal.root.as_ref().unwrap().join("plan.json"))
            .expect("read immutable plan");
        assert_eq!(
            prepared.prepared.plan_hash,
            blake3::hash(&plan_bytes).to_hex().to_string()
        );
        let temporary = fs::read_dir(fixture.store_root.join("ledger"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".verdicts-"))
            })
            .expect("retain one ledger successor");
        let successor = fs::read(&temporary).expect("read successor bytes");
        assert_eq!(
            prepared.prepared.artifacts.ledger_successor_hash,
            blake3::hash(&successor).to_hex().to_string()
        );
        let successor_view = read_ledger(&temporary).expect("parse successor ledger");
        assert_eq!(
            prepared.prepared.artifacts.verdict_event_hash,
            successor_view.events.last().unwrap().hash()
        );
        if verb.applies() {
            assert!(prepared.prepared.artifacts.generation_id.is_some());
            assert!(prepared.prepared.artifacts.manifest_hash.is_some());
            assert!(prepared.prepared.artifacts.reindex_receipt_hash.is_some());
        } else {
            assert_eq!(
                prepared.prepared.artifacts,
                PreparedArtifacts::ledger_only(
                    prepared.prepared.artifacts.verdict_event_hash.clone(),
                    prepared.prepared.artifacts.ledger_successor_hash.clone(),
                )
            );
        }
    }
}

#[test]
fn c2a_cleanup_witness_requires_plan_prepared_and_both_markers() {
    let fixture = P3TestFixture::with_staged_proposal("c2a-witness-plan", VerdictVerb::Reject);
    let runtime = fixture.runtime_with_ops(FailJournalFilesystem::new(
        JournalFilesystemFailure::CleanupRemove,
    ));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Reject, "retain cleanup witness", &runtime)
            .unwrap_err(),
        TransactionError::JournalState
    );
    let witness = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".cleanup-"))
        })
        .expect("retain cleanup witness");
    let mut names = fs::read_dir(witness)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        [
            "ledger-committed.json",
            "plan.json",
            "prepared.json",
            "published.json",
        ]
    );
}

#[test]
fn c2b2_prepared_precedes_all_applying_publication_artifacts() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("c2b2-prepared-before-publish");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "hold before publication", &runtime)
            .unwrap_err(),
        TransactionError::FaultInjected
    );

    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .expect("prepared transaction journal")
        .unwrap()
        .path();
    let mut names = fs::read_dir(&transaction)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["plan.json", "prepared.json"]);
    assert!(fs::read_dir(fixture.store_root.join("evidence/reindex"))
        .unwrap()
        .next()
        .is_none());
    assert!(fs::read_dir(fixture.store_root.join("canon/generations"))
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("gen-"))));
    assert!(fs::read_dir(fixture.store_root.join("derived/indexes"))
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("gen-"))));
    assert_eq!(
        fs::read(fixture.store_root.join("canon/CURRENT")).unwrap(),
        b"seed\n"
    );
    assert!(fs::read_dir(fixture.store_root.join("ledger"))
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".verdicts-"))));
}

#[test]
fn c2b2_reindex_receipt_fault_requires_prepared_before_final_publication() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("c2b2-receipt-before-final");
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterReindexReceiptSync));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Accept, "hold after receipt", &runtime)
            .unwrap_err(),
        TransactionError::FaultInjected
    );

    let transaction = fs::read_dir(fixture.store_root.join("transactions"))
        .unwrap()
        .next()
        .expect("prepared transaction journal")
        .unwrap()
        .path();
    assert!(transaction.join("plan.json").is_file());
    assert!(transaction.join("prepared.json").is_file());
    assert_eq!(
        fs::read_dir(fixture.store_root.join("evidence/reindex"))
            .unwrap()
            .count(),
        1
    );
    assert!(fs::read_dir(fixture.store_root.join("canon/generations"))
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("gen-"))));
    assert!(fs::read_dir(fixture.store_root.join("derived/indexes"))
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("gen-"))));
    assert_eq!(
        fs::read(fixture.store_root.join("canon/CURRENT")).unwrap(),
        b"seed\n"
    );
}

#[test]
fn ledger_temporary_residue_is_publicly_incomplete_not_layout_invalid() {
    let fixture =
        P3TestFixture::with_staged_proposal("ledger-temporary-residue", VerdictVerb::Reject);
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterLedgerTemporarySync));
    assert_eq!(
        fixture
            .apply_with_runtime(
                VerdictVerb::Reject,
                "interrupted temporary ledger",
                &runtime
            )
            .unwrap_err(),
        TransactionError::FaultInjected
    );
    assert!(fs::read_dir(fixture.store_root.join("ledger"))
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".verdicts-"))));

    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    assert_eq!(
        inspect_store(&profile, &fixture.store_root)
            .unwrap_err()
            .code(),
        "transaction.incomplete"
    );
    assert_eq!(
        verify_store_level4(&profile, &fixture.store_root)
            .unwrap_err()
            .code(),
        "transaction.incomplete"
    );
}

#[test]
fn ledger_only_verdict_binds_policy_actor_and_unchanged_head_without_a_result() {
    let fixture = P3TestFixture::with_staged_proposal("ledger-fields", VerdictVerb::Tp);
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    let store = inspect_store(&profile, &fixture.store_root).unwrap();
    let expected_actor = store.policy.policy.human_actor.clone();
    let expected_head = store.head.as_str().to_owned();

    fixture
        .apply(VerdictVerb::Tp, "human classified true positive")
        .unwrap();
    let view = read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl")).unwrap();
    let LedgerEvent::Verdict(event) = view.events.last().unwrap() else {
        panic!("first ledger-only commit must end with a verdict")
    };
    assert_eq!(event.payload().actor, expected_actor);
    assert_eq!(event.payload().resulting_canonical_head, expected_head);
    assert_eq!(event.payload().canonical_result, CanonicalResult::none());
    assert!(event.payload().rewrite_hash.is_none());
    assert!(event.payload().rewrite_bytes.is_none());
}

#[test]
fn ledger_only_exact_replay_is_read_only_and_changed_replay_is_already_ruled() {
    let fixture = P3TestFixture::with_staged_proposal("ledger-replay", VerdictVerb::Fp);
    let first = fixture
        .apply(VerdictVerb::Fp, "human classified false positive")
        .unwrap();
    let before = fixture.durable_hashes();
    let replay = fixture
        .apply(VerdictVerb::Fp, "human classified false positive")
        .unwrap();
    assert!(replay.idempotent);
    assert_eq!(replay.event_id, first.event_id);
    assert_eq!(fixture.durable_hashes(), before);
    assert!(fixture.leases_absent());
    assert_eq!(
        fixture
            .apply(VerdictVerb::Fp, "changed human classification")
            .unwrap_err()
            .code(),
        "verdict.already_ruled"
    );
    assert_eq!(fixture.durable_hashes(), before);
    assert!(fixture.leases_absent());
}

#[test]
fn ledger_only_reason_validation_rejects_empty_and_whitespace_before_preflight() {
    let fixture = P3TestFixture::with_staged_proposal("ledger-reason", VerdictVerb::Reject);
    for reason in ["", " \t\n"] {
        assert_eq!(
            fixture
                .request_for(VerdictVerb::Reject, reason)
                .unwrap_err()
                .code(),
            "human.reason_invalid"
        );
    }
    assert!(fixture.leases_absent());
    assert!(fixture.transactions_empty());
}

#[test]
fn ledger_rename_attempt_error_is_conservatively_incomplete() {
    let before_rename =
        P3TestFixture::with_staged_proposal("ledger-write-failure", VerdictVerb::Reject);
    let canon_before = before_rename.canon_hashes();
    let runtime = before_rename.runtime_with_ops(RejectRename);
    assert_eq!(
        before_rename
            .apply_with_runtime(VerdictVerb::Reject, "ledger write failure", &runtime)
            .unwrap_err()
            .code(),
        "transaction.incomplete"
    );
    assert!(
        read_ledger(&before_rename.store_root.join("ledger/verdicts.jsonl"))
            .unwrap()
            .events
            .is_empty()
    );
    assert_eq!(before_rename.canon_hashes(), canon_before);
    assert_publicly_incomplete(&before_rename);
}

#[test]
fn post_rename_faults_keep_the_verdict_committed_and_return_incomplete() {
    for point in [
        FaultPoint::AfterLedgerRename,
        FaultPoint::BeforePublishedMarker,
    ] {
        let fixture =
            P3TestFixture::with_staged_proposal("post-ledger-failure", VerdictVerb::Reject);
        let runtime = fixture.runtime_with_fault(FaultPlan::at(point));
        assert_eq!(
            fixture
                .apply_with_runtime(VerdictVerb::Reject, "post ledger failure", &runtime)
                .unwrap_err(),
            TransactionError::JournalState
        );
        assert_eq!(
            read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
                .unwrap()
                .events
                .len(),
            2
        );
        assert!(!fixture.transactions_empty());
        assert!(!fixture.leases_absent());
        assert_committed_ledger_is_publicly_incomplete(&fixture);
    }
}

#[test]
fn post_ledger_cleanup_fault_keeps_the_verdict_committed_and_returns_incomplete() {
    let fixture = P3TestFixture::with_staged_proposal("cleanup-failure", VerdictVerb::Reject);
    let runtime = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::DuringCleanup));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Reject, "cleanup failure", &runtime)
            .unwrap_err(),
        TransactionError::JournalState
    );
    assert_eq!(
        read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
            .unwrap()
            .events
            .len(),
        2
    );
    assert!(!fixture.transactions_empty());
    assert!(!fixture.leases_absent());
    assert_committed_ledger_is_publicly_incomplete(&fixture);
}

#[test]
fn marker_creation_filesystem_failure_returns_incomplete_after_ledger_commit() {
    let fixture =
        P3TestFixture::with_staged_proposal("marker-filesystem-failure", VerdictVerb::Reject);
    let runtime = fixture.runtime_with_ops(FailJournalFilesystem::new(
        JournalFilesystemFailure::LedgerCommittedMarker,
    ));
    assert_eq!(
        fixture
            .apply_with_runtime(VerdictVerb::Reject, "marker operation failure", &runtime)
            .unwrap_err()
            .code(),
        "transaction.incomplete"
    );
    assert_committed_ledger_is_publicly_incomplete(&fixture);
}

#[test]
fn cleanup_hard_link_and_remove_filesystem_failures_return_incomplete_after_ledger_commit() {
    for (name, failure) in [
        (
            "cleanup-hard-link-filesystem-failure",
            JournalFilesystemFailure::CleanupHardLink,
        ),
        (
            "cleanup-remove-filesystem-failure",
            JournalFilesystemFailure::CleanupRemove,
        ),
    ] {
        let fixture = P3TestFixture::with_staged_proposal(name, VerdictVerb::Reject);
        let runtime = fixture.runtime_with_ops(FailJournalFilesystem::new(failure));
        assert_eq!(
            fixture
                .apply_with_runtime(VerdictVerb::Reject, "cleanup operation failure", &runtime)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
        assert_committed_ledger_is_publicly_incomplete(&fixture);
    }
}

#[test]
fn original_cleanup_empty_prefix_resumes_after_directory_or_first_link_failure() {
    for fault in [
        CleanupPrefixFault::AfterDirectoryCreate,
        CleanupPrefixFault::FirstHardLinkFailure,
    ] {
        let fixture = ProvisionedP3Fixture::with_staged_reject(&format!(
            "original-cleanup-empty-prefix-{fault:?}"
        ));
        let interrupted = fixture.runtime_with_ops(InterruptAtCleanupPrefix::new(
            fixture.store_root.join("transactions"),
            fault,
        ));
        assert!(fixture
            .apply_with_runtime(
                VerdictVerb::Reject,
                "interrupt cleanup prefix",
                &interrupted
            )
            .is_err());

        let transition =
            discover_original_journal(&StdFilesystem, &fixture.store_root.join("transactions"))
                .expect("a complete regular journal authenticates its empty cleanup prefix");
        assert!(!transition.cleanup_form, "{fault:?}");

        let owner = read_canonical_owner(
            &StdFilesystem,
            &fixture.store_root.join("locks/verdict.lock/owner.json"),
            LeaseKind::Verdict,
        )
        .expect("the interrupted transaction retains a verified stale owner");
        let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
        let outcome = recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &fixture.runtime(),
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_or_else(|error| {
            panic!("the empty original cleanup prefix must resume ({fault:?}): {error:?}")
        });
        assert!(outcome.rolled_forward, "{fault:?}");
        assert!(fixture.verify_level4().is_ok(), "{fault:?}");
        assert!(fixture.transactions_empty(), "{fault:?}");
        assert!(fixture.leases_absent(), "{fault:?}");
    }
}

#[test]
fn recovery_cleanup_empty_prefix_resumes_after_directory_or_first_link_failure() {
    for fault in [
        CleanupPrefixFault::AfterDirectoryCreate,
        CleanupPrefixFault::FirstHardLinkFailure,
    ] {
        let fixture = ProvisionedP3Fixture::with_staged_reject(&format!(
            "recovery-cleanup-empty-prefix-{fault:?}"
        ));
        let record = with_test_recovery_id(RecoveryJournalRecord {
            schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
            recovery_id: format!("txn-{}", "a".repeat(64)),
            mode: RecoveryMode::CleanFinalization,
            command_class: "verdict".to_owned(),
            recovered_at: "2026-07-14T04:31:00.000000000Z".to_owned(),
            quarantined_leases: vec![RecoveryLeaseBinding {
                kind: RecoveryLeaseKind::Verdict,
                owner_hash: "a".repeat(64),
            }],
            recovery_lease_owner_hash: "b".repeat(64),
            original_transaction_id: None,
            original_journal_hash: None,
            observed_ledger_head: None,
            profile_hash: "c".repeat(64),
            policy_hash: "d".repeat(64),
            cleanup_plan: RecoveryCleanupPlan::empty(),
            cleanup_plan_hash: String::new(),
        });
        let journal = RecoveryJournal::create(&fixture.runtime(), record).unwrap();
        journal.write_published(&StdFilesystem).unwrap();
        let interrupted =
            InterruptAtCleanupPrefix::new(fixture.store_root.join("transactions"), fault);
        assert!(journal.cleanup_with(&interrupted).is_err());

        let resumed =
            RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
                .expect(
                    "a complete regular recovery journal authenticates its empty cleanup prefix",
                )
                .expect("the interrupted recovery journal remains discoverable");
        assert!(!resumed.cleanup_form, "{fault:?}");
        resumed
            .cleanup_with(&StdFilesystem)
            .unwrap_or_else(|error| {
                panic!("the empty recovery cleanup prefix must resume ({fault:?}): {error:?}")
            });
        assert!(fixture.transactions_empty(), "{fault:?}");
    }
}

#[test]
fn recovery_resumes_empty_original_cleanup_prefix_for_each_uncommitted_journal_prefix() {
    for (label, initial_fault, expected_files, verb) in [
        (
            "plan-only",
            FaultPoint::AfterTransactionJournalCreation,
            &["plan.json"] as &[&str],
            VerdictVerb::Reject,
        ),
        (
            "plan-and-prepared-apply",
            FaultPoint::AfterPreparedMarker,
            &["plan.json", "prepared.json"] as &[&str],
            VerdictVerb::Accept,
        ),
    ] {
        let fixture =
            ProvisionedP3Fixture::with_staged_reject(&format!("recovery-empty-prefix-{label}"));
        let interrupted_apply = fixture.runtime_with_fault(FaultPlan::at(initial_fault));
        assert!(fixture
            .apply_with_runtime(verb, "interrupt journal prefix", &interrupted_apply)
            .is_err());

        let stale_owner = read_canonical_owner(
            &StdFilesystem,
            &fixture.store_root.join("locks/verdict.lock/owner.json"),
            LeaseKind::Verdict,
        )
        .expect("the interrupted transaction retains a stale owner");
        let mut first_human =
            ScriptedHuman::new(Some(format!("RECOVER {}\n", stale_owner.owner_id)));
        let interrupted_recovery = fixture.runtime_with_ops(InterruptAtCleanupPrefix::new(
            fixture.store_root.join("transactions"),
            CleanupPrefixFault::AfterDirectoryCreate,
        ));
        assert!(recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &interrupted_recovery,
            &mut first_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        )
        .is_err());

        let recovery =
            RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
                .expect("the interrupted recovery journal remains exact")
                .expect("one recovery journal");
        assert!(!recovery.cleanup_form, "{label}");
        assert!(!recovery.is_published(&StdFilesystem).unwrap(), "{label}");
        let original = discover_original_journal_ignoring(
            &StdFilesystem,
            &fixture.store_root.join("transactions"),
            Some(&recovery.root),
        )
        .expect("the recovery journal authenticates the matching empty original prefix");
        assert_eq!(
            original.files,
            expected_files
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>(),
            "{label}"
        );
        assert_eq!(
            recovery.record.original_transaction_id.as_deref(),
            original.transaction_id(),
            "{label}"
        );
        assert_eq!(
            recovery.record.original_journal_hash,
            original.commitment_hash(),
            "{label}"
        );

        let recovery_owner = read_canonical_owner(
            &StdFilesystem,
            &fixture.store_root.join("locks/verdict.lock/owner.json"),
            LeaseKind::Verdict,
        )
        .expect("the recovery lease remains live");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let snapshot =
            sovmem::profile::inspect_store_with_transients(&profile, &fixture.store_root)
                .expect("inspect retry recovery state");
        let snapshot_original = discover_original_journal_ignoring(
            &StdFilesystem,
            &snapshot.paths.transactions,
            Some(&recovery.root),
        )
        .expect("the snapshot transaction path must ignore the recovery root too");
        assert_eq!(snapshot_original.files, original.files, "{label}");
        validate_recovery_prefix(&profile, &snapshot, &fixture.runtime(), &recovery, false)
            .expect("the unpublished recovery prefix must remain authenticated");
        let mut retry_human =
            ScriptedHuman::new(Some(format!("RECOVER {}\n", recovery_owner.owner_id)));
        let outcome = recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &fixture.runtime(),
            &mut retry_human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap(),
            ),
        )
        .unwrap_or_else(|error| panic!("the {label} empty cleanup prefix must resume: {error:?}"));
        assert!(!outcome.rolled_forward, "{label}");
        assert!(fixture.transactions_empty(), "{label}");
        assert!(fixture.leases_absent(), "{label}");
    }
}

#[test]
fn every_transaction_cleanup_link_and_removal_crash_resumes_exactly() {
    for (kind, after) in [
        (CleanupStepKind::HardLink, 1),
        (CleanupStepKind::HardLink, 2),
        (CleanupStepKind::HardLink, 3),
        (CleanupStepKind::HardLink, 4),
        (CleanupStepKind::RemoveFile, 1),
        (CleanupStepKind::RemoveFile, 2),
        (CleanupStepKind::RemoveFile, 3),
        (CleanupStepKind::RemoveFile, 4),
        (CleanupStepKind::RemoveFile, 5),
        (CleanupStepKind::RemoveFile, 6),
        (CleanupStepKind::RemoveFile, 7),
        (CleanupStepKind::RemoveFile, 8),
    ] {
        let fixture =
            ProvisionedP3Fixture::with_staged_reject(&format!("cleanup-step-{kind:?}-{after}"));
        let interrupted = fixture.runtime_with_ops(InterruptAfterCleanupStep::new(
            fixture.store_root.join("transactions"),
            kind,
            after,
        ));
        assert!(
            fixture
                .apply_with_runtime(VerdictVerb::Reject, "interrupt cleanup", &interrupted)
                .is_err(),
            "the step must leave recovery evidence: {kind:?} {after}"
        );
        let transition =
            discover_original_journal(&StdFilesystem, &fixture.store_root.join("transactions"));
        if kind == CleanupStepKind::RemoveFile && after == 8 {
            assert_eq!(
                transition.unwrap_err(),
                TransactionError::RecoveryAmbiguous,
                "an empty cleanup directory is not a generic journal form"
            );
        } else {
            let transition = transition.unwrap_or_else(|error| {
                panic!("cleanup transition must be authenticated ({kind:?} {after}): {error:?}")
            });
            assert!(transition.root.is_some(), "{kind:?} {after}");
        }

        let owner = read_canonical_owner(
            &StdFilesystem,
            &fixture.store_root.join("locks/verdict.lock/owner.json"),
            LeaseKind::Verdict,
        )
        .expect("the interrupted transaction keeps its verified stale owner");
        let mut human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
        let outcome = recover_store_with_human(
            RecoveryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            &fixture.runtime(),
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),
            ),
        );
        if let Err(error) = &outcome {
            let entries = sorted_directory_entries(&fixture.store_root.join("transactions"))
                .unwrap()
                .into_iter()
                .map(|entry| entry.file_name().into_string().unwrap())
                .collect::<Vec<_>>();
            panic!(
                "cleanup step must resume deterministically ({kind:?} {after}): {error:?}; transactions={entries:?}"
            );
        }
        let outcome = outcome.unwrap();

        assert!(outcome.rolled_forward, "{kind:?} {after}");
        assert!(fixture.verify_level4().is_ok(), "{kind:?} {after}");
        assert!(fixture.transactions_empty(), "{kind:?} {after}");
        assert!(fixture.leases_absent(), "{kind:?} {after}");
    }
}

#[test]
fn every_recovery_cleanup_link_and_removal_crash_resumes_exactly() {
    for (kind, after) in [
        (CleanupStepKind::HardLink, 1),
        (CleanupStepKind::HardLink, 2),
        (CleanupStepKind::RemoveFile, 1),
        (CleanupStepKind::RemoveFile, 2),
        (CleanupStepKind::RemoveFile, 3),
    ] {
        let fixture = ProvisionedP3Fixture::with_staged_reject(&format!(
            "recovery-cleanup-step-{kind:?}-{after}"
        ));
        let record = with_test_recovery_id(RecoveryJournalRecord {
            schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
            recovery_id: format!("txn-{}", "a".repeat(64)),
            mode: RecoveryMode::CleanFinalization,
            command_class: "verdict".to_owned(),
            recovered_at: "2026-07-14T04:31:00.000000000Z".to_owned(),
            quarantined_leases: vec![RecoveryLeaseBinding {
                kind: RecoveryLeaseKind::Verdict,
                owner_hash: "a".repeat(64),
            }],
            recovery_lease_owner_hash: "b".repeat(64),
            original_transaction_id: None,
            original_journal_hash: None,
            observed_ledger_head: None,
            profile_hash: "c".repeat(64),
            policy_hash: "d".repeat(64),
            cleanup_plan: RecoveryCleanupPlan::empty(),
            cleanup_plan_hash: String::new(),
        });
        let journal = RecoveryJournal::create(&fixture.runtime(), record).unwrap();
        journal.write_published(&StdFilesystem).unwrap();
        let interrupted =
            InterruptAfterCleanupStep::new(fixture.store_root.join("transactions"), kind, after);
        assert!(
            journal.cleanup_with(&interrupted).is_err(),
            "the step must retain an authenticated recovery cleanup transition: {kind:?} {after}"
        );
        let resumed =
            RecoveryJournal::discover(&StdFilesystem, &fixture.store_root.join("transactions"))
                .expect("discover only an exact recovery cleanup transition")
                .expect("the interrupted recovery journal remains discoverable");
        resumed
            .cleanup_with(&StdFilesystem)
            .unwrap_or_else(|error| {
                panic!("recovery cleanup step must resume ({kind:?} {after}): {error:?}")
            });
        assert!(fixture.transactions_empty(), "{kind:?} {after}");
    }
}

#[test]
fn empty_recovery_cleanup_directory_resumes_only_the_authenticated_lease_tail() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("empty-recovery-cleanup-tail");
    let interrupted_apply =
        fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
    assert!(fixture
        .apply_with_runtime(
            VerdictVerb::Accept,
            "interrupt before ledger",
            &interrupted_apply
        )
        .is_err());
    let owner = read_canonical_owner(
        &StdFilesystem,
        &fixture.store_root.join("locks/verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )
    .unwrap();
    let mut first_human = ScriptedHuman::new(Some(format!("RECOVER {}\n", owner.owner_id)));
    let interrupted_recovery = fixture.runtime_with_ops(InterruptAfterCleanupStep::new(
        fixture.store_root.join("transactions"),
        CleanupStepKind::RemoveFile,
        8,
    ));
    assert!(recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &interrupted_recovery,
        &mut first_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap(),),
    )
    .is_err());

    let entries = sorted_directory_entries(&fixture.store_root.join("transactions")).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0]
        .file_name()
        .to_str()
        .is_some_and(|name| name.starts_with(".cleanup-txn-")));
    assert!(sorted_directory_entries(&entries[0].path())
        .unwrap()
        .is_empty());

    let mut retry_human = ScriptedHuman::new(None);
    let outcome = recover_store_with_human(
        RecoveryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        &fixture.runtime(),
        &mut retry_human,
        &FixedClock(sovmem::profile::Timestamp::parse("2026-07-14T04:32:00.000000000Z").unwrap()),
    )
    .expect("an authenticated recovery lease may remove only its terminal empty residue");
    assert_eq!(outcome.mode, RecoveryMode::CleanFinalization);
    assert!(fixture.transactions_empty());
    assert!(fixture.leases_absent());
    assert_eq!(retry_human.show_count(), 0);
    assert_eq!(retry_human.read_count(), 0);
}

#[test]
fn transaction_cleanup_parent_sync_is_liveness_only() {
    assert_terminal_parent_sync_is_liveness_only(
        "terminal-cleanup-sync",
        TerminalParentSync::TransactionCleanup,
    );
}

#[test]
fn verdict_lease_parent_sync_is_liveness_only() {
    assert_terminal_parent_sync_is_liveness_only(
        "terminal-verdict-sync",
        TerminalParentSync::VerdictLease,
    );
}

fn assert_terminal_parent_sync_is_liveness_only(name: &str, target: TerminalParentSync) {
    let fixture = P3TestFixture::with_staged_proposal(name, VerdictVerb::Reject);
    let ops = DenyTerminalParentSync::new(target);
    let runtime = fixture.runtime_with_ops(ops.clone());

    let outcome = fixture
        .apply_with_runtime(VerdictVerb::Reject, "terminal recovery tail", &runtime)
        .expect("terminal cleanup must not perform a fallible parent sync");

    assert!(!outcome.applied);
    assert_eq!(fixture.verdict_count(VerdictVerb::Reject), 1);
    assert_eq!(ops.denied_attempts(), 0, "{name}");
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    assert!(
        inspect_store(&profile, &fixture.store_root).is_ok(),
        "{name}"
    );
    assert!(
        verify_store_level4(&profile, &fixture.store_root).is_ok(),
        "{name}"
    );
    assert!(fixture.transactions_empty(), "{name}");
    assert!(fixture.leases_absent(), "{name}");
}

fn assert_committed_ledger_is_publicly_incomplete(fixture: &ProvisionedP3Fixture) {
    assert_eq!(
        read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
            .unwrap()
            .events
            .len(),
        2
    );
    assert_publicly_incomplete(fixture);
}

fn assert_publicly_incomplete(fixture: &ProvisionedP3Fixture) {
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    assert_eq!(
        inspect_store(&profile, &fixture.store_root)
            .unwrap_err()
            .code(),
        "transaction.incomplete"
    );
    assert_eq!(
        verify_store_level4(&profile, &fixture.store_root)
            .unwrap_err()
            .code(),
        "transaction.incomplete"
    );
}

#[test]
fn ledger_only_apply_never_opens_canonical_index_or_reindex_paths_for_writing() {
    let fixture = P3TestFixture::with_staged_proposal("ledger-write-guard", VerdictVerb::Reject);
    let runtime = fixture.runtime_with_ops(RejectProtectedWrites::new(
        fs::canonicalize(&fixture.store_root).unwrap(),
    ));
    fixture
        .apply_with_runtime(VerdictVerb::Reject, "guarded ledger only", &runtime)
        .unwrap();
    assert!(fixture.verify_level4().is_ok());
}

#[test]
fn injected_post_lease_fault_keeps_the_exact_incomplete_evidence() {
    let fixture = P3TestFixture::new("lease-fault");
    let faults = FaultPlan::at(FaultPoint::AfterLeaseCreation);
    let runtime = fixture.runtime_with_fault(faults);
    let receipt = fixture.verdict_lease_receipt("fault-owner");
    assert_eq!(
        runtime.acquire_verdict_leases(&receipt).unwrap_err().code(),
        "transaction.prepare_failed"
    );
    assert!(fixture.verdict_lease_exists());
}

#[test]
fn journal_markers_and_cleanup_are_ordered_and_create_new() {
    let fixture = P3TestFixture::new("journal-cleanup");
    let runtime = fixture.runtime();
    let receipt = fixture.verdict_lease_receipt("journal-owner");
    let leases = runtime.acquire_verdict_leases(&receipt).unwrap();
    let transaction_id = format!("txn-{}", "d".repeat(64));
    let proposal_id = format!("prp-{}", "e".repeat(64));
    let verdict_id = format!("ver-{}", "f".repeat(64));
    let journal = runtime
        .create_prepared_journal(
            &transaction_id,
            &leases,
            Some(&proposal_id),
            Some(&verdict_id),
            None,
            Vec::new(),
        )
        .unwrap();
    assert!(journal.write_published(&runtime.ops).is_err());
    assert!(journal.cleanup_with(&runtime.ops).is_err());
    journal.write_ledger_committed(&runtime.ops).unwrap();
    journal.write_published(&runtime.ops).unwrap();
    let unrelated_transaction_id = format!("txn-{}", "a".repeat(64));
    fixture.create_empty_cleanup(&unrelated_transaction_id);
    assert!(journal.cleanup_with(&runtime.ops).is_err());
    assert!(fixture.transaction_exists(&transaction_id));
    fixture.remove_empty_cleanup(&unrelated_transaction_id);
    journal.cleanup_with(&runtime.ops).unwrap();
    assert!(!fixture.transaction_exists(&transaction_id));
    runtime.release_verdict_leases(&leases).unwrap();
}

// ============================================================
// transaction_plan_observed_head_tests (extracted from
// transaction.rs)
// ============================================================

#[cfg(test)]
mod transaction_plan_observed_head_tests {
    use sovmem::profile::TransactionPlan;

    fn valid_plan(observed_ledger_head: Option<String>) -> TransactionPlan {
        TransactionPlan {
            schema_version: "sovmem-transaction-plan-v1".to_owned(),
            transaction_id: format!("txn-{}", "a".repeat(64)),
            command_class: "verdict".to_owned(),
            owner_hashes: vec!["4".repeat(64), "5".repeat(64)],
            proposal_id: format!("prp-{}", "b".repeat(64)),
            verdict_id: format!("ver-{}", "c".repeat(64)),
            observed_ledger_head,
            profile_hash: "a".repeat(64),
            policy_hash: "b".repeat(64),
            proposal_meta_hash: "c".repeat(64),
            candidate_hash: "d".repeat(64),
            verb: "ACCEPT".to_owned(),
            target_id: "ssa-0/transaction-plan-target".to_owned(),
            target_path: "04-Research/Transaction-Plan-Target.md".to_owned(),
            base_target_hash: None,
            parent_ref: "seed".to_owned(),
            parent_manifest_hash: "e".repeat(64),
            parent_bundle_hash: "f".repeat(64),
            generation_id: Some("gen-0000000000000001".to_owned()),
            approved_target_hash: Some("2".repeat(64)),
            judgment_hash: Some("3".repeat(64)),
            reason_hash: "0".repeat(64),
            ruled_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
        }
    }

    #[test]
    fn transaction_plan_binds_a_lowercase_ledger_hash_and_rejects_canonical_or_malformed_heads() {
        assert!(valid_plan(Some("d".repeat(64))).validates());
        for invalid in [
            "seed".to_owned(),
            "gen-0000000000000001".to_owned(),
            "D".repeat(64),
            "d".repeat(63),
        ] {
            assert!(
                !valid_plan(Some(invalid)).validates(),
                "only a lowercase 64-hex ledger head is valid"
            );
        }
    }
}
