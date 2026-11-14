#![allow(dead_code)]

use super::{
    append_events_with, append_intake_event_with_boundaries, archive_policy, build_judgment,
    build_p3_index_generation, completed_intake_transaction, current_base,
    discard_unreferenced_policy_archive, evaluator_commitment, intake_prefix, load_p3_profile,
    prepare_proposal, publish_sealed_pair, read_inbox_submission, read_intake, read_ledger,
    read_sealed_pair_structural, recover_incomplete_intake_transaction, stage_generation, store,
    successor_bytes_with, validate_decision, validate_rewrite, verify_generation,
    verify_preflight_transient_state, verify_proposal_pair_bytes, verify_store_level4,
    verify_store_level4_with_allowance, write_intake_committed, write_intake_prepared,
    write_intake_published, AppliedGenerationInput, CanonicalHead, CanonicalResult, Clock,
    DecisionMaterial, GenerationPlan, GenesisEvent, GenesisPayload, IntakeError, IntakeEvent,
    IntakeHead, IntakeLeaseIdentity, IntakeOutcome, IntakePayload, IntakePreparedRecord,
    IntakePublicationLease, JudgmentDocument, JudgmentError, LedgerEvent, P3IndexRequest,
    P3Profile, PolicyArchive, PolicyContext, PolicyDecision, PolicyError, PolicyEvaluator,
    ProposalDraft, ProposalIntent, ProposalPair, Reason, RewriteBytes, StoreSnapshot, SystemClock,
    Timestamp, VerdictEvent, VerdictIdentityInput, VerdictPayload, VerdictVerb,
    VerificationAllowance, VerificationLevel,
};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

const LEASE_OWNER_SCHEMA: &str = "sovmem-lease-owner-v1";
pub const JOURNAL_SCHEMA: &str = "sovmem-transaction-journal-v1";
pub const TRANSACTION_PLAN_SCHEMA: &str = "sovmem-transaction-plan-v1";
pub const RECOVERY_JOURNAL_SCHEMA: &str = "sovmem-recovery-journal-v1";
pub const RECOVERY_CLEANUP_PLAN_SCHEMA: &str = "sovmem-recovery-cleanup-plan-v2";
const LEASE_TOKEN_DOMAIN: &[u8] = b"sovmem.lease-token.v1";
const LEASE_RECEIPT_DOMAIN: &[u8] = b"sovmem.lease-receipt.v1";
const RECOVERY_TRANSACTION_DOMAIN: &[u8] = b"sovmem.recovery-transaction.v1";
const RECOVERY_CLEANUP_PLAN_DOMAIN: &[u8] = b"sovmem.recovery-cleanup-plan.v2";
const MAX_CONFIRMATION_BYTES: usize = 256;

std::thread_local! {
    static INTAKE_SEAL_POST_PRECHECK_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static INTAKE_SEAL_PRE_CLEANUP_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static INTAKE_SEAL_POST_CLEANUP_SELECTION_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static INTAKE_SEAL_AFTER_CANDIDATE_CLEANUP_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static INTAKE_SEAL_BEFORE_METADATA_CLEANUP_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static INTAKE_SEAL_AFTER_METADATA_CLEANUP_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static VERDICT_PRE_APPEND_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static VERDICT_LATE_INTAKE_CHECK_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

pub fn install_intake_seal_post_precheck_hook_for_test(hook: impl FnOnce() + 'static) {
    INTAKE_SEAL_POST_PRECHECK_HOOK.with(|slot| {
        assert!(
            slot.borrow_mut().replace(Box::new(hook)).is_none(),
            "an intake seal post-precheck hook is already installed"
        );
    });
}

pub fn install_intake_seal_pre_cleanup_hook_for_test(hook: impl FnOnce() + 'static) {
    INTAKE_SEAL_PRE_CLEANUP_HOOK.with(|slot| {
        assert!(
            slot.borrow_mut().replace(Box::new(hook)).is_none(),
            "an intake seal pre-cleanup hook is already installed"
        );
    });
}

pub fn install_intake_seal_post_cleanup_selection_hook_for_test(hook: impl FnOnce() + 'static) {
    INTAKE_SEAL_POST_CLEANUP_SELECTION_HOOK.with(|slot| {
        assert!(
            slot.borrow_mut().replace(Box::new(hook)).is_none(),
            "an intake seal post-cleanup-selection hook is already installed"
        );
    });
}

pub fn install_intake_seal_after_candidate_cleanup_hook_for_test(hook: impl FnOnce() + 'static) {
    INTAKE_SEAL_AFTER_CANDIDATE_CLEANUP_HOOK.with(|slot| {
        assert!(
            slot.borrow_mut().replace(Box::new(hook)).is_none(),
            "an intake seal after-candidate-cleanup hook is already installed"
        );
    });
}

pub fn install_intake_seal_before_metadata_cleanup_hook_for_test(hook: impl FnOnce() + 'static) {
    INTAKE_SEAL_BEFORE_METADATA_CLEANUP_HOOK.with(|slot| {
        assert!(
            slot.borrow_mut().replace(Box::new(hook)).is_none(),
            "an intake seal before-metadata-cleanup hook is already installed"
        );
    });
}

pub fn install_intake_seal_after_metadata_cleanup_hook_for_test(hook: impl FnOnce() + 'static) {
    INTAKE_SEAL_AFTER_METADATA_CLEANUP_HOOK.with(|slot| {
        assert!(
            slot.borrow_mut().replace(Box::new(hook)).is_none(),
            "an intake seal after-metadata-cleanup hook is already installed"
        );
    });
}

pub fn install_verdict_pre_append_hook_for_test(hook: impl FnOnce() + 'static) {
    VERDICT_PRE_APPEND_HOOK.with(|slot| {
        assert!(
            slot.borrow_mut().replace(Box::new(hook)).is_none(),
            "a verdict pre-append hook is already installed"
        );
    });
}

pub fn install_verdict_late_intake_check_hook_for_test(hook: impl FnOnce() + 'static) {
    VERDICT_LATE_INTAKE_CHECK_HOOK.with(|slot| {
        assert!(
            slot.borrow_mut().replace(Box::new(hook)).is_none(),
            "a verdict late intake-check hook is already installed"
        );
    });
}

#[cfg(any(test, feature = "test-support"))]
fn run_intake_seal_post_precheck_hook_for_test() {
    let hook = INTAKE_SEAL_POST_PRECHECK_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(any(test, feature = "test-support"))]
fn run_intake_seal_pre_cleanup_hook_for_test() {
    let hook = INTAKE_SEAL_PRE_CLEANUP_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(any(test, feature = "test-support"))]
fn run_intake_seal_post_cleanup_selection_hook_for_test() {
    let hook = INTAKE_SEAL_POST_CLEANUP_SELECTION_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(any(test, feature = "test-support"))]
fn run_intake_seal_after_candidate_cleanup_hook_for_test() {
    let hook = INTAKE_SEAL_AFTER_CANDIDATE_CLEANUP_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(any(test, feature = "test-support"))]
fn run_intake_seal_before_metadata_cleanup_hook_for_test() {
    let hook = INTAKE_SEAL_BEFORE_METADATA_CLEANUP_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(any(test, feature = "test-support"))]
fn run_intake_seal_after_metadata_cleanup_hook_for_test() {
    let hook = INTAKE_SEAL_AFTER_METADATA_CLEANUP_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(any(test, feature = "test-support"))]
fn run_verdict_pre_append_hook_for_test() {
    let hook = VERDICT_PRE_APPEND_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(any(test, feature = "test-support"))]
fn run_verdict_late_intake_check_hook_for_test() {
    let hook = VERDICT_LATE_INTAKE_CHECK_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HumanVerdictRequest {
    pub proposal_id: String,
    pub verb: VerdictVerb,
    pub reason: Reason,
    pub rewrite: Option<RewriteBytes>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerdictOutcome {
    pub proposal_id: String,
    pub verdict_id: String,
    pub event_id: String,
    pub applied: bool,
    pub generation: Option<String>,
    pub idempotent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntakeSealOutcome {
    pub submission_id: String,
    pub proposal_id: Option<String>,
    pub state: String,
    pub intake_sequence: u64,
    pub intake_head_hash: String,
    pub policy_hash: String,
    pub profile_hash: String,
    pub idempotent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdempotentVerdict {
    pub outcome: VerdictOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryRequest {
    pub profile_path: PathBuf,
    pub store_root: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryMode {
    VerdictTransaction,
    IntakeOnly,
    CleanFinalization,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryOutcome {
    pub rolled_forward: bool,
    pub idempotent: bool,
    pub mode: RecoveryMode,
}

pub enum PreflightVerdict {
    Idempotent(IdempotentVerdict),
    Confirmed(ConfirmedDecision),
}

pub struct ConfirmedDecision {
    pub profile: P3Profile,
    pub profile_path: PathBuf,
    pub store: StoreSnapshot,
    pub proposal: ProposalPair,
    pub request: HumanVerdictRequest,
    pub decision: DecisionMaterial,
    pub ruled_at: Timestamp,
    pub transaction_id: String,
    pub observed_ledger_head: Option<String>,
    pub intake_head: IntakeHead,
    pub generation_id: Option<String>,
    pub approved_target_bytes: Option<Vec<u8>>,
    pub approved_target_hash: Option<String>,
    pub judgment: Option<JudgmentDocument>,
    pub policy_decision: PolicyDecision,
    pub policy_archive: PolicyArchive,
    pub leases: LeaseSet,
}

impl std::fmt::Debug for ConfirmedDecision {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfirmedDecision")
            .field("proposal_id", &self.proposal.meta.proposal_id)
            .field("verb", &self.decision.verb.as_str())
            .field("verdict_id", &self.decision.verdict_id)
            .field("transaction_id", &self.transaction_id)
            .field("observed_ledger_head", &self.observed_ledger_head)
            .field("intake_head", &self.intake_head)
            .field("generation_id", &self.generation_id)
            .field("policy_hash", &self.store.policy.hash)
            .field("profile_hash", &self.profile.hash)
            .finish_non_exhaustive()
    }
}

impl ConfirmedDecision {
    pub fn leases(&self) -> &LeaseSet {
        &self.leases
    }

    pub fn proposal(&self) -> &ProposalPair {
        &self.proposal
    }

    pub fn store(&self) -> &StoreSnapshot {
        &self.store
    }

    pub fn profile(&self) -> &P3Profile {
        &self.profile
    }

    pub fn decision(&self) -> &DecisionMaterial {
        &self.decision
    }

    pub fn ruled_at(&self) -> &Timestamp {
        &self.ruled_at
    }

    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    pub fn observed_ledger_head(&self) -> Option<&str> {
        self.observed_ledger_head.as_deref()
    }

    pub fn generation_id(&self) -> Option<&str> {
        self.generation_id.as_deref()
    }

    pub fn approved_target_bytes(&self) -> Option<&[u8]> {
        self.approved_target_bytes.as_deref()
    }

    pub fn approved_target_hash(&self) -> Option<&str> {
        self.approved_target_hash.as_deref()
    }

    pub fn judgment(&self) -> Option<&JudgmentDocument> {
        self.judgment.as_ref()
    }

    pub fn policy_decision(&self) -> &PolicyDecision {
        &self.policy_decision
    }

    pub fn request(&self) -> &HumanVerdictRequest {
        &self.request
    }

    pub fn profile_path(&self) -> &Path {
        &self.profile_path
    }

    pub fn policy_archive(&self) -> &PolicyArchive {
        &self.policy_archive
    }

    pub fn runtime(&self) -> Result<TransactionRuntime, TransactionError> {
        TransactionRuntime::production(&self.profile, &self.store)
    }

    pub fn release_leases(&self) -> Result<(), TransactionError> {
        self.runtime()?.release_verdict_leases(&self.leases)
    }
}

pub trait HumanPresence {
    fn show(&mut self, bytes: &[u8]) -> Result<(), TransactionError>;
    fn read_confirmation(&mut self) -> Result<String, TransactionError>;
}

#[derive(Debug)]
pub struct TtyHumanPresence {
    pub tty: File,
}

impl TtyHumanPresence {
    pub fn open() -> Result<Self, TransactionError> {
        Self::open_at(Path::new("/dev/tty"))
    }

    pub fn open_at(path: &Path) -> Result<Self, TransactionError> {
        let tty = OpenOptions::new()
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

impl HumanPresence for TtyHumanPresence {
    fn show(&mut self, bytes: &[u8]) -> Result<(), TransactionError> {
        self.tty
            .write_all(bytes)
            .and_then(|_| self.tty.flush())
            .map_err(|_| TransactionError::Write)
    }

    fn read_confirmation(&mut self) -> Result<String, TransactionError> {
        let reader = self
            .tty
            .try_clone()
            .map_err(|_| TransactionError::ConfirmationFailed)?;
        let mut reader = BufReader::new(reader);
        read_bounded_confirmation(&mut reader)
    }
}

pub fn seal_intake_submission(
    profile_path: &Path,
    store_root: &Path,
    submission_id: &str,
) -> Result<IntakeSealOutcome, TransactionError> {
    // Production opens the controlling terminal before taking custody of any
    // untrusted submission. Custody is protected transient evidence only; it
    // cannot publish queue, canon, judgment, index, or ledger authority.
    let mut human = TtyHumanPresence::open()?;
    seal_intake_submission_with_human(
        profile_path,
        store_root,
        submission_id,
        &mut human,
        &SystemClock,
    )
}

pub fn seal_intake_submission_with_human(
    profile_path: &Path,
    store_root: &Path,
    submission_id: &str,
    human: &mut impl HumanPresence,
    clock: &impl Clock,
) -> Result<IntakeSealOutcome, TransactionError> {
    seal_intake_submission_with_human_and_faults(
        profile_path,
        store_root,
        submission_id,
        human,
        clock,
        &NoFaults,
    )
}

pub fn seal_intake_submission_with_human_and_faults(
    profile_path: &Path,
    store_root: &Path,
    submission_id: &str,
    human: &mut impl HumanPresence,
    clock: &impl Clock,
    faults: &impl FaultInjector,
) -> Result<IntakeSealOutcome, TransactionError> {
    let profile = load_p3_profile(profile_path).map_err(|_| TransactionError::Read)?;
    let store = store::inspect_store(&profile, store_root).map_err(|error| match error {
        store::StoreError::Incomplete => {
            let locked = fs::read_dir(store_root.join("locks"))
                .is_ok_and(|mut entries| entries.next().is_some());
            if locked {
                TransactionError::IntakeLocked
            } else {
                TransactionError::IntakeIncomplete
            }
        }
        _ => TransactionError::Read,
    })?;
    if let Some(completed) =
        completed_intake_transaction(&store.paths, submission_id).map_err(map_intake_error)?
    {
        let extant = store.paths.inbox.join(submission_id);
        match fs::symlink_metadata(&extant) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(TransactionError::IntakeIncomplete),
            Ok(_) => {
                let submission = read_inbox_submission(
                    &store.paths.inbox,
                    submission_id,
                    store.policy.policy.max_candidate_bytes,
                )
                .map_err(|_| TransactionError::IntakeIncomplete)?;
                if blake3::hash(&submission.candidate_bytes).to_hex().as_str()
                    != completed.submission_candidate_hash
                    || blake3::hash(&submission.meta_bytes).to_hex().as_str()
                        != completed.submission_meta_hash
                {
                    return Err(TransactionError::IntakeIncomplete);
                }
            }
        }
        return Ok(IntakeSealOutcome {
            submission_id: completed.submission_id,
            proposal_id: completed.proposal_id,
            state: completed.state.to_owned(),
            intake_sequence: completed.intake_sequence,
            intake_head_hash: completed.intake_head_hash,
            policy_hash: completed.policy_hash,
            profile_hash: completed.profile_hash,
            idempotent: true,
        });
    }
    if fs::read_dir(&store.paths.locks)
        .map_err(|_| TransactionError::Read)?
        .next()
        .is_some()
    {
        return Err(TransactionError::IntakeLocked);
    }
    #[cfg(any(test, feature = "test-support"))]
    run_intake_seal_post_precheck_hook_for_test();
    let lease_time = Timestamp::parse("1970-01-01T00:00:00.000000000Z")
        .map_err(|_| TransactionError::IntakeIncomplete)?;
    let identity = IntakeLeaseIdentity::new(
        format!("seal-{submission_id}"),
        "intake-seal",
        &lease_time,
        std::process::id(),
    )
    .map_err(map_intake_error)?;
    let mut lease =
        IntakePublicationLease::acquire(&store.paths, identity).map_err(map_intake_error)?;
    if let Err(error) = verify_exclusive_intake_seal_lease(&store.paths, &mut lease) {
        return match lease.release(&store.paths).map_err(map_intake_error) {
            Ok(()) => Err(error),
            Err(release_error) => Err(release_error),
        };
    }
    let mut claim = match ClaimedInboxSubmission::take(&store.paths, submission_id) {
        Ok(claim) => claim,
        Err(error) => {
            return match lease.release(&store.paths).map_err(map_intake_error) {
                Ok(()) => Err(error),
                Err(release_error) => Err(release_error),
            };
        }
    };
    let mut result = seal_intake_submission_under_lease(
        IntakeSealContext {
            profile_path,
            profile: &profile,
            store: &store,
        },
        &mut claim,
        &mut lease,
        human,
        clock,
        faults,
    );
    if result.is_err() && !claim.cleaned {
        result = Err(TransactionError::IntakeIncomplete);
    }
    if result == Err(TransactionError::IntakeIncomplete) {
        return result;
    }
    let released = lease.release(&store.paths).map_err(map_intake_error);
    match released {
        Ok(()) => result,
        Err(error) => Err(error),
    }
}

#[derive(Debug)]
struct ClaimedInboxSubmission {
    submission_id: String,
    custody_root: PathBuf,
    custody: PathBuf,
    cleaned: bool,
}

struct IntakeSealContext<'a> {
    profile_path: &'a Path,
    profile: &'a P3Profile,
    store: &'a StoreSnapshot,
}

impl ClaimedInboxSubmission {
    fn take(paths: &super::StorePaths, submission_id: &str) -> Result<Self, TransactionError> {
        if !valid_intake_submission_id(submission_id) {
            return Err(TransactionError::IntakeRecordInvalid);
        }
        let inbox_root = paths.inbox.join(submission_id);
        let custody_root = paths.intake_transactions.join(submission_id);
        require_regular_directory(&paths.inbox, TransactionError::IntakeRecordInvalid)?;
        require_regular_directory(
            &paths.intake_transactions,
            TransactionError::IntakeIncomplete,
        )?;
        require_regular_directory(&inbox_root, TransactionError::IntakeRecordInvalid)?;
        match fs::symlink_metadata(&custody_root) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(TransactionError::IntakeIncomplete),
        }
        fs::rename(&inbox_root, &custody_root).map_err(|_| TransactionError::IntakeIncomplete)?;
        if super::sync_directory(&paths.inbox).is_err()
            || super::sync_directory(&paths.intake_transactions).is_err()
        {
            return Err(TransactionError::IntakeIncomplete);
        }
        require_regular_directory(&custody_root, TransactionError::IntakeIncomplete)?;
        Ok(Self {
            submission_id: submission_id.to_owned(),
            custody_root,
            custody: paths.intake_transactions.clone(),
            cleaned: false,
        })
    }
}

fn valid_intake_submission_id(value: &str) -> bool {
    value.len() == 68
        && value.starts_with("sub-")
        && value[4..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn require_regular_directory(path: &Path, error: TransactionError) -> Result<(), TransactionError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(error);
    }
    Ok(())
}

fn verify_exclusive_intake_seal_lease(
    paths: &super::StorePaths,
    lease: &mut IntakePublicationLease,
) -> Result<(), TransactionError> {
    lease.verify(paths).map_err(map_intake_error)?;
    let mut locks = fs::read_dir(&paths.locks)
        .map_err(|_| TransactionError::IntakeLocked)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TransactionError::IntakeLocked)?;
    if locks.len() != 1 || locks.pop().unwrap().file_name().to_str() != Some("intake.lock") {
        return Err(TransactionError::IntakeLocked);
    }
    Ok(())
}

fn seal_intake_submission_under_lease(
    context: IntakeSealContext<'_>,
    claim: &mut ClaimedInboxSubmission,
    lease: &mut IntakePublicationLease,
    human: &mut impl HumanPresence,
    clock: &impl Clock,
    faults: &impl FaultInjector,
) -> Result<IntakeSealOutcome, TransactionError> {
    let IntakeSealContext {
        profile_path,
        profile,
        store,
    } = context;
    lease.verify(&store.paths).map_err(map_intake_error)?;
    let submission = read_inbox_submission(
        &claim.custody,
        &claim.submission_id,
        store.policy.policy.max_candidate_bytes,
    )
    .map_err(|_| TransactionError::IntakeRecordInvalid)?;
    let prepared_submission = submission.clone();
    faults.check(FaultPoint::AfterInboxRead)?;
    let occurred_at = clock.now().map_err(|_| TransactionError::Read)?;
    let current_profile = load_p3_profile(profile_path).map_err(|_| TransactionError::Read)?;
    let current_policy =
        super::load_policy(&store.paths.policy).map_err(|_| TransactionError::Read)?;
    if current_profile.bytes != profile.bytes
        || current_profile.hash != profile.hash
        || submission.meta.profile_hash != profile.hash
    {
        return Err(TransactionError::ProfileChanged);
    }
    if current_policy.bytes != store.policy.bytes
        || current_policy.hash != store.policy.hash
        || submission.meta.policy_hash != store.policy.hash
    {
        return Err(TransactionError::PolicyChanged);
    }
    let view = read_intake(&store.paths).map_err(map_intake_error)?;
    if view
        .events()
        .last()
        .is_some_and(|event| occurred_at.as_str() < event.payload().occurred_at.as_str())
    {
        return Err(TransactionError::IntakeClockRegression);
    }
    let prefix = intake_prefix(&view, view.head()).map_err(map_intake_error)?;
    let base = current_base(
        profile,
        store,
        submission.meta.operation,
        &submission.meta.target_path,
    )
    .map_err(|_| TransactionError::IntakeRecordInvalid)?;
    let pair = prepare_proposal(ProposalDraft {
        submitted_at: occurred_at.clone(),
        source_id: submission.meta.source_id.clone(),
        operation: submission.meta.operation,
        intent: submission.meta.intent,
        target_id: submission.meta.target_id.clone(),
        target_path: submission.meta.target_path.clone(),
        base,
        candidate_bytes: submission.candidate_bytes,
        policy_hash: store.policy.hash.clone(),
        profile_hash: profile.hash.clone(),
        confidence: submission.meta.confidence,
        evaluator: submission.meta.evaluator,
    })
    .map_err(|_| TransactionError::IntakeRecordInvalid)?;
    let evaluator_binding_mismatch = pair
        .meta
        .evaluator
        .as_ref()
        .is_some_and(|evaluator| evaluator.candidate_hash != pair.candidate_hash);
    let mut decision = if evaluator_binding_mismatch {
        PolicyDecision {
            passed: false,
            rule_codes: vec!["proposal.evaluator_hash_mismatch".to_owned()],
        }
    } else {
        PolicyEvaluator::evaluate(&PolicyContext {
            policy: &store.policy,
            profile,
            store,
            proposal: &pair,
            intake: &prefix,
        })
    };
    decision.rule_codes.sort();
    decision.rule_codes.dedup();
    if !evaluator_binding_mismatch {
        let preview = render_intake_preview(
            &claim.submission_id,
            &pair,
            &store.policy.hash,
            &profile.hash,
            &decision,
        );
        human.show(preview.as_bytes())?;
        let expected = format!("SEAL INTAKE {}\n", claim.submission_id);
        if human.read_confirmation()? != expected {
            return Err(TransactionError::IntakeConfirmationFailed);
        }
    }

    let sequence = view
        .head()
        .sequence
        .checked_add(1)
        .ok_or(TransactionError::IntakeIncomplete)?;
    let outcome = if decision.passed {
        IntakeOutcome::STAGED
    } else {
        IntakeOutcome::REJECTED
    };
    let event = IntakeEvent::new(IntakePayload {
        schema_version: super::INTAKE_EVENT_SCHEMA.to_owned(),
        event_type: "intake".to_owned(),
        sequence,
        event_id: format!("int-{sequence:016}"),
        occurred_at: occurred_at.as_str().to_owned(),
        previous_event_hash: view.head().event_hash.clone(),
        outcome,
        source_id: Some(pair.meta.source_id.clone()),
        proposal_id: decision.passed.then(|| pair.meta.proposal_id.clone()),
        proposal_meta_hash: decision.passed.then(|| pair.meta_hash.clone()),
        candidate_hash: decision.passed.then(|| pair.candidate_hash.clone()),
        policy_hash: store.policy.hash.clone(),
        profile_hash: profile.hash.clone(),
        rule_codes: if decision.passed {
            Vec::new()
        } else {
            decision.rule_codes.clone()
        },
    })
    .map_err(map_intake_error)?;
    let prepared = IntakePreparedRecord::new(
        &prepared_submission,
        view.head(),
        &event,
        decision.passed.then_some(&pair),
    )
    .map_err(map_intake_error)?;
    write_intake_prepared(&store.paths, &claim.custody_root, &prepared)
        .map_err(map_intake_error)?;
    faults.check(FaultPoint::AfterIntakeJournalSync)?;
    if decision.passed {
        publish_sealed_pair(&store.paths, lease, &pair).map_err(map_intake_error)?;
    }
    faults.check(FaultPoint::AfterPairSync)?;
    let committed_view = append_intake_event_with_boundaries(
        &store.paths,
        lease,
        &event,
        || {
            faults
                .check(FaultPoint::BeforeIntakeEventRename)
                .map_err(|_| IntakeError::Write)
        },
        || {
            faults
                .check(FaultPoint::AfterIntakeEventRename)
                .map_err(|_| IntakeError::Write)
        },
    )
    .map_err(map_intake_error)?;
    write_intake_committed(&store.paths, &claim.custody_root, &prepared)
        .map_err(map_intake_error)?;
    write_intake_published(&store.paths, &claim.custody_root, &prepared)
        .map_err(map_intake_error)?;
    faults.check(FaultPoint::BeforeInboxCleanup)?;
    #[cfg(any(test, feature = "test-support"))]
    run_intake_seal_pre_cleanup_hook_for_test();
    lease.verify(&store.paths).map_err(map_intake_error)?;
    remove_claimed_submission(
        claim,
        lease,
        &store.paths,
        &prepared_submission.candidate_bytes,
        &prepared_submission.meta_bytes,
    )?;
    claim.cleaned = true;
    let head_hash = committed_view
        .head()
        .event_hash
        .clone()
        .ok_or(TransactionError::IntakeIncomplete)?;
    Ok(IntakeSealOutcome {
        submission_id: claim.submission_id.clone(),
        proposal_id: decision.passed.then_some(pair.meta.proposal_id),
        state: if decision.passed {
            "staged".to_owned()
        } else {
            "rejected-at-intake".to_owned()
        },
        intake_sequence: committed_view.head().sequence,
        intake_head_hash: head_hash,
        policy_hash: store.policy.hash.clone(),
        profile_hash: profile.hash.clone(),
        idempotent: false,
    })
}

fn render_intake_preview(
    submission_id: &str,
    pair: &ProposalPair,
    policy_hash: &str,
    profile_hash: &str,
    decision: &PolicyDecision,
) -> String {
    format!(
        "SOVMEM HUMAN INTAKE PREVIEW\nsubmission_id: {submission_id}\ncandidate_hash: {}\nproposal_meta_hash: {}\npolicy_hash: {policy_hash}\nprofile_hash: {profile_hash}\npolicy_passed: {}\npolicy_rule_codes: {}\nTYPE EXACTLY: SEAL INTAKE {submission_id}\n",
        pair.candidate_hash,
        pair.meta_hash,
        decision.passed,
        decision.rule_codes.join(","),
    )
}

fn remove_claimed_submission(
    claim: &ClaimedInboxSubmission,
    lease: &IntakePublicationLease,
    paths: &super::StorePaths,
    expected_candidate: &[u8],
    expected_metadata: &[u8],
) -> Result<(), TransactionError> {
    require_regular_directory(&claim.custody_root, TransactionError::IntakeIncomplete)?;
    let mut entries = fs::read_dir(&claim.custody_root)
        .map_err(|_| TransactionError::IntakeIncomplete)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TransactionError::IntakeIncomplete)?;
    entries.sort_by_key(|entry| entry.file_name());
    if entries.len() != 5
        || entries[0].file_name().to_str() != Some("candidate.md")
        || entries[1].file_name().to_str() != Some("committed.json")
        || entries[2].file_name().to_str() != Some("prepared.json")
        || entries[3].file_name().to_str() != Some("published.json")
        || entries[4].file_name().to_str() != Some("submission.meta.json")
    {
        return Err(TransactionError::IntakeIncomplete);
    }
    for entry in &entries {
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(|_| TransactionError::IntakeIncomplete)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(TransactionError::IntakeIncomplete);
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    run_intake_seal_post_cleanup_selection_hook_for_test();
    let mut refreshed = fs::read_dir(&claim.custody_root)
        .map_err(|_| TransactionError::IntakeIncomplete)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TransactionError::IntakeIncomplete)?;
    refreshed.sort_by_key(|entry| entry.file_name());
    if refreshed.len() != 5
        || refreshed[0].file_name().to_str() != Some("candidate.md")
        || refreshed[1].file_name().to_str() != Some("committed.json")
        || refreshed[2].file_name().to_str() != Some("prepared.json")
        || refreshed[3].file_name().to_str() != Some("published.json")
        || refreshed[4].file_name().to_str() != Some("submission.meta.json")
    {
        return Err(TransactionError::IntakeIncomplete);
    }
    for entry in &refreshed {
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(|_| TransactionError::IntakeIncomplete)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(TransactionError::IntakeIncomplete);
        }
    }
    if fs::read(claim.custody_root.join("candidate.md"))
        .map_err(|_| TransactionError::IntakeIncomplete)?
        != expected_candidate
        || fs::read(claim.custody_root.join("submission.meta.json"))
            .map_err(|_| TransactionError::IntakeIncomplete)?
            != expected_metadata
    {
        return Err(TransactionError::IntakeIncomplete);
    }
    lease.verify(paths).map_err(map_intake_error)?;
    fs::remove_file(claim.custody_root.join("candidate.md"))
        .map_err(|_| TransactionError::IntakeIncomplete)?;
    #[cfg(any(test, feature = "test-support"))]
    run_intake_seal_after_candidate_cleanup_hook_for_test();
    #[cfg(any(test, feature = "test-support"))]
    run_intake_seal_before_metadata_cleanup_hook_for_test();
    lease.verify(paths).map_err(map_intake_error)?;
    fs::remove_file(claim.custody_root.join("submission.meta.json"))
        .map_err(|_| TransactionError::IntakeIncomplete)?;
    #[cfg(any(test, feature = "test-support"))]
    run_intake_seal_after_metadata_cleanup_hook_for_test();
    super::sync_directory(&claim.custody_root).map_err(|_| TransactionError::IntakeIncomplete)?;
    super::sync_directory(&claim.custody).map_err(|_| TransactionError::IntakeIncomplete)
}

fn map_intake_error(error: IntakeError) -> TransactionError {
    match error {
        IntakeError::Locked => TransactionError::IntakeLocked,
        IntakeError::ClockRegression => TransactionError::IntakeClockRegression,
        IntakeError::RecordInvalid => TransactionError::IntakeRecordInvalid,
        IntakeError::ChainInvalid => TransactionError::IntakeChainInvalid,
        IntakeError::HeadMismatch => TransactionError::IntakeHeadMismatch,
        IntakeError::PairBinding => TransactionError::IntakePairBinding,
        IntakeError::Read | IntakeError::Write => TransactionError::IntakeIncomplete,
    }
}

pub fn read_bounded_confirmation<R: BufRead>(reader: &mut R) -> Result<String, TransactionError> {
    let mut bytes = Vec::with_capacity(MAX_CONFIRMATION_BYTES + 1);
    let mut limited = <&mut R as io::Read>::take(reader, (MAX_CONFIRMATION_BYTES + 1) as u64);
    let read = limited
        .read_until(b'\n', &mut bytes)
        .map_err(|_| TransactionError::ConfirmationFailed)?;
    if read == 0 || bytes.len() > MAX_CONFIRMATION_BYTES || bytes.last() != Some(&b'\n') {
        return Err(TransactionError::ConfirmationFailed);
    }
    String::from_utf8(bytes).map_err(|_| TransactionError::ConfirmationFailed)
}

pub fn require_exact_confirmation(
    human: &mut impl HumanPresence,
    verb: VerdictVerb,
    proposal_id: &str,
) -> Result<(), TransactionError> {
    let prefix = proposal_id
        .get(..12)
        .ok_or(TransactionError::ConfirmationFailed)?;
    let expected = format!("APPLY {} {prefix}\n", verb.as_str());
    if human.read_confirmation()? != expected {
        return Err(TransactionError::ConfirmationFailed);
    }
    Ok(())
}

#[derive(Clone)]
pub struct PreflightState {
    pub profile: P3Profile,
    pub profile_path: PathBuf,
    pub store: StoreSnapshot,
    pub proposal: ProposalPair,
    pub ledger_head: Option<String>,
    pub terminal: Option<VerdictPayload>,
    pub intake_head: IntakeHead,
}

pub fn preflight_human_verdict(
    profile_path: &Path,
    store_root: &Path,
    request: HumanVerdictRequest,
) -> Result<PreflightVerdict, TransactionError> {
    validate_request_shape(&request)?;
    let profile = load_p3_profile(profile_path).map_err(|_| TransactionError::Read)?;
    let store = store::inspect_store_with_transients(&profile, store_root)
        .map_err(|_| TransactionError::Read)?;
    let runtime = TransactionRuntime::production(&profile, &store)?;
    let mut human = TtyHumanPresence::open()?;
    preflight_human_verdict_from_store_with_human(
        profile_path,
        store_root,
        request,
        &runtime,
        &mut human,
        &SystemClock,
    )
}

/// Sole production verdict entry point. Exact terminal replay is resolved from
/// verified durable state before `/dev/tty` is opened; every new decision goes
/// through the real terminal ceremony and the transaction coordinator.
pub fn commit_human_verdict(
    profile_path: &Path,
    store_root: &Path,
    request: HumanVerdictRequest,
) -> Result<VerdictOutcome, TransactionError> {
    validate_request_shape(&request)?;
    let profile = load_p3_profile(profile_path).map_err(|_| TransactionError::Read)?;
    let store = store::inspect_store_with_transients(&profile, store_root)
        .map_err(|_| TransactionError::Read)?;
    let initial = read_preflight_state(profile_path, store_root, &request, None)?;
    if let Some(terminal) = initial.terminal.as_ref() {
        return Ok(idempotent_terminal_replay(terminal, &request)?.outcome);
    }
    if matches!(
        (initial.proposal.meta.intent, request.verb),
        (ProposalIntent::Rewrite, VerdictVerb::Accept)
            | (ProposalIntent::Accept, VerdictVerb::Rewrite)
    ) {
        return Err(TransactionError::IntentConflict);
    }

    let runtime = TransactionRuntime::production(&profile, &store)?;
    let mut human = TtyHumanPresence::open()?;
    match preflight_human_verdict_with_human(initial, request, &runtime, &mut human, &SystemClock)?
    {
        PreflightVerdict::Idempotent(replay) => Ok(replay.outcome),
        PreflightVerdict::Confirmed(decision) => apply_human_verdict(decision, &runtime),
    }
}

pub fn preflight_human_verdict_with_human<O, F, H, C>(
    initial: PreflightState,
    request: HumanVerdictRequest,
    runtime: &TransactionRuntime<O, F>,
    human: &mut H,
    clock: &C,
) -> Result<PreflightVerdict, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
    H: HumanPresence,
    C: Clock,
{
    let receipt = VerdictLeaseReceipt::new(
        &initial.store.policy.policy.store_id,
        Some(initial.store.head.as_str()),
        &initial.profile.hash,
        &initial.store.policy.hash,
        &request.proposal_id,
    )?;
    let leases = runtime.acquire_verdict_leases(&receipt)?;
    preflight_human_verdict_with_held_leases(initial, request, runtime, human, clock, leases)
}

pub fn preflight_human_verdict_from_store_with_human<O, F, H, C>(
    profile_path: &Path,
    store_root: &Path,
    request: HumanVerdictRequest,
    runtime: &TransactionRuntime<O, F>,
    human: &mut H,
    clock: &C,
) -> Result<PreflightVerdict, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
    H: HumanPresence,
    C: Clock,
{
    validate_request_shape(&request)?;
    let profile = load_p3_profile(profile_path).map_err(|_| TransactionError::Read)?;
    let store = store::inspect_store_with_transients(&profile, store_root)
        .map_err(|_| TransactionError::Read)?;
    if runtime.root != store.paths.root {
        return Err(TransactionError::InvalidReceipt);
    }
    let receipt = VerdictLeaseReceipt::new(
        &store.policy.policy.store_id,
        Some(store.head.as_str()),
        &profile.hash,
        &store.policy.hash,
        &request.proposal_id,
    )?;
    let leases = runtime.acquire_verdict_leases(&receipt)?;
    let initial = match read_preflight_state(
        profile_path,
        runtime.root.as_path(),
        &request,
        Some(&leases),
    ) {
        Ok(initial) => initial,
        Err(error) => {
            let _ = runtime.release_verdict_leases(&leases);
            return Err(error);
        }
    };
    preflight_human_verdict_with_held_leases(initial, request, runtime, human, clock, leases)
}

fn preflight_human_verdict_with_held_leases<O, F, H, C>(
    initial: PreflightState,
    request: HumanVerdictRequest,
    runtime: &TransactionRuntime<O, F>,
    human: &mut H,
    clock: &C,
    held_leases: LeaseSet,
) -> Result<PreflightVerdict, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
    H: HumanPresence,
    C: Clock,
{
    let mut leases = Some(held_leases);
    let result = (|| {
        let locked = read_preflight_state(
            &initial.profile_path,
            runtime.root.as_path(),
            &request,
            leases.as_ref(),
        )?;
        assert_locked_inputs_match_initial(&initial, &locked)?;
        if let Some(terminal) = &locked.terminal {
            return Ok(PreflightVerdict::Idempotent(idempotent_terminal_replay(
                terminal, &request,
            )?));
        }
        if locked.proposal.meta_hash != initial.proposal.meta_hash
            || locked.proposal.candidate_hash != initial.proposal.candidate_hash
            || locked.proposal.meta.proposal_id != initial.proposal.meta.proposal_id
        {
            return Err(TransactionError::ProposalChanged);
        }
        let policy_decision = evaluate_current_policy(&locked)?;
        if !policy_decision.passed {
            if policy_decision
                .rule_codes
                .iter()
                .any(|code| code == "policy.changed_during_transaction")
            {
                return Err(TransactionError::PolicyChanged);
            }
            if policy_decision
                .rule_codes
                .iter()
                .any(|code| code == "profile.changed_during_transaction")
            {
                return Err(TransactionError::ProfileChanged);
            }
            return Err(TransactionError::PolicyRejected);
        }

        Reason::new(
            request.reason.bytes(),
            locked.store.policy.policy.max_reason_bytes,
        )
        .map_err(map_judgment_error)?;
        let _rewrite_document = match request.rewrite.as_ref() {
            Some(rewrite) => Some(
                validate_rewrite(
                    rewrite,
                    &locked.profile,
                    &locked.store.policy.policy,
                    &locked.proposal.meta.target_id,
                    &locked.proposal.meta.target_path,
                )
                .map_err(map_judgment_error)?,
            ),
            None => None,
        };
        let ruled_at = clock.now().map_err(|_| TransactionError::Read)?;
        let identity = VerdictIdentityInput {
            proposal_id: &locked.proposal.meta.proposal_id,
            verb: request.verb,
            reason: &request.reason,
            rewrite: request.rewrite.as_ref(),
            actor: &locked.store.policy.policy.human_actor,
            ruled_at: &ruled_at,
            prior_ledger_head: locked.ledger_head.as_deref(),
            intake_sequence: locked.intake_head.sequence,
            intake_head_hash: locked.intake_head.event_hash.as_deref(),
            policy_hash: &locked.store.policy.hash,
            profile_hash: &locked.profile.hash,
        };
        let decision = validate_decision(
            locked.proposal.meta.intent,
            request.verb,
            request.reason.clone(),
            request.rewrite.clone(),
            &identity,
        )
        .map_err(map_judgment_error)?;
        let approved_target_bytes = if decision.verb.applies() {
            Some(match decision.rewrite.as_ref() {
                Some(rewrite) => rewrite.bytes().to_vec(),
                None => locked.proposal.candidate_bytes.clone(),
            })
        } else {
            None
        };
        let approved_target_hash = approved_target_bytes
            .as_ref()
            .map(|bytes| blake3::hash(bytes).to_hex().to_string());
        let judgment = build_judgment(
            &locked.profile,
            &decision.verdict_id,
            decision.verb,
            &locked.proposal.meta.target_id,
            &decision.reason,
            &ruled_at,
        )
        .map_err(map_judgment_error)?;
        let reason_hash = blake3::hash(decision.reason.bytes()).to_hex().to_string();
        let transaction_id = transaction_id(&TransactionIdInput {
            proposal_id: &locked.proposal.meta.proposal_id,
            verb: decision.verb,
            reason_hash: reason_hash.as_bytes(),
            rewrite_hash: decision.rewrite.as_ref().map(RewriteBytes::hash),
            actor: &locked.store.policy.policy.human_actor,
            ruled_at: ruled_at.as_str(),
            observed_ledger_head: locked.ledger_head.as_deref(),
            intake_sequence: locked.intake_head.sequence,
            intake_head_hash: locked.intake_head.event_hash.as_deref(),
            policy_hash: &locked.store.policy.hash,
            profile_hash: &locked.profile.hash,
        });
        let generation_id = decision
            .verb
            .applies()
            .then(|| next_generation_id(&locked.store.head));

        human.show(&render_decision_display(DecisionDisplayInput {
            state: &locked,
            decision: &decision,
            reason_hash: &reason_hash,
            ruled_at: &ruled_at,
            actor: &locked.store.policy.policy.human_actor,
            observed_ledger_head: locked.ledger_head.as_deref(),
            approved_target: approved_target_bytes.as_deref(),
            approved_target_hash: approved_target_hash.as_deref(),
            judgment: judgment.as_ref(),
            policy_decision: &policy_decision,
            transaction_id: &transaction_id,
            generation_id: generation_id.as_deref(),
        }))?;
        require_exact_confirmation(human, request.verb, &request.proposal_id)?;

        assert_inputs_unchanged(&locked)?;
        assert_intake_head_unchanged(&locked)?;
        let archive = archive_policy(&locked.store.paths.policies, &locked.store.policy)
            .map_err(map_policy_archive_error)?;
        if let Err(error) =
            assert_inputs_unchanged(&locked).and_then(|_| assert_intake_head_unchanged(&locked))
        {
            return match discard_unreferenced_policy_archive(&archive, &locked.store.policy) {
                Ok(()) => Err(error),
                Err(cleanup) => Err(map_policy_archive_error(cleanup)),
            };
        }

        Ok(PreflightVerdict::Confirmed(ConfirmedDecision {
            profile: locked.profile,
            profile_path: locked.profile_path,
            store: locked.store,
            proposal: locked.proposal,
            request,
            decision,
            ruled_at,
            transaction_id,
            observed_ledger_head: locked.ledger_head,
            intake_head: locked.intake_head,
            generation_id,
            approved_target_bytes,
            approved_target_hash,
            judgment,
            policy_decision,
            policy_archive: archive,
            leases: leases
                .take()
                .expect("leases remain held until confirmation succeeds"),
        }))
    })();
    if result.is_err() || matches!(&result, Ok(PreflightVerdict::Idempotent(_))) {
        if let Some(leases) = leases.as_ref() {
            let _ = runtime.release_verdict_leases(leases);
        }
    }
    result
}

pub fn read_preflight_state(
    profile_path: &Path,
    store_root: &Path,
    request: &HumanVerdictRequest,
    leases: Option<&LeaseSet>,
) -> Result<PreflightState, TransactionError> {
    let profile = load_p3_profile(profile_path).map_err(|_| TransactionError::Read)?;
    let allowance = match leases {
        Some(leases) => VerificationAllowance {
            verdict_lease_token: Some(leases.verdict_token().to_owned()),
            intake_lease_token: Some(leases.intake_token().to_owned()),
            recovery_verdict_lease_token: None,
        },
        None => VerificationAllowance::default(),
    };
    let store = match leases {
        Some(_) => store::inspect_store_with_transients(&profile, store_root)
            .map_err(|_| TransactionError::Read)?,
        None => store::inspect_store(&profile, store_root).map_err(|error| match error {
            store::StoreError::Incomplete
                if fs::read_dir(store_root.join("locks"))
                    .is_ok_and(|mut entries| entries.next().is_some()) =>
            {
                TransactionError::Locked(LeaseKind::Verdict)
            }
            _ => TransactionError::Read,
        })?,
    };
    verify_preflight_transient_state(&store.paths, &allowance)
        .map_err(|_| TransactionError::Read)?;
    if store.sealed {
        if leases.is_some() {
            verify_store_level4_with_allowance(&profile, store_root, &allowance)
                .map_err(|_| TransactionError::Read)?;
        } else {
            verify_store_level4(&profile, store_root).map_err(|_| TransactionError::Read)?;
        }
    }
    let intake = read_intake(&store.paths).map_err(map_intake_error)?;
    let intake_head = intake.head().clone();
    let prefix = intake_prefix(&intake, &intake_head).map_err(map_intake_error)?;
    if !prefix
        .staged
        .iter()
        .any(|summary| summary.proposal_id == request.proposal_id)
    {
        return Err(TransactionError::ProposalNotFound);
    }
    let proposal = read_sealed_pair_structural(&store.paths, &request.proposal_id)
        .map_err(map_intake_error)?;
    let (ledger_head, terminal) = if store.sealed {
        let view = read_ledger(&store.paths.ledger).map_err(|_| TransactionError::Read)?;
        let terminal = view.events.iter().find_map(|event| match event {
            LedgerEvent::Verdict(event) if event.payload().proposal_id == request.proposal_id => {
                Some(event.payload().clone())
            }
            _ => None,
        });
        (view.final_hash, terminal)
    } else {
        (None, None)
    };
    Ok(PreflightState {
        profile,
        profile_path: profile_path.to_owned(),
        store,
        proposal,
        ledger_head,
        terminal,
        intake_head,
    })
}

pub fn idempotent_terminal_replay(
    terminal: &VerdictPayload,
    request: &HumanVerdictRequest,
) -> Result<IdempotentVerdict, TransactionError> {
    let reason_hash = blake3::hash(request.reason.bytes()).to_hex().to_string();
    let terminal_reason_hash = blake3::hash(terminal.reason.as_bytes())
        .to_hex()
        .to_string();
    let rewrite_hash = request.rewrite.as_ref().map(|rewrite| rewrite.hash());
    if terminal.proposal_id != request.proposal_id
        || terminal.verb != request.verb
        || terminal_reason_hash != reason_hash
        || terminal.rewrite_hash.as_deref() != rewrite_hash
    {
        return Err(TransactionError::AlreadyRuled);
    }
    Ok(IdempotentVerdict {
        outcome: VerdictOutcome {
            proposal_id: terminal.proposal_id.clone(),
            verdict_id: terminal.verdict_id.clone(),
            event_id: terminal.event_id.clone(),
            applied: terminal.verb.applies(),
            generation: terminal.canonical_result.generation_id.clone(),
            idempotent: true,
        },
    })
}

pub fn apply_human_verdict<O, F>(
    decision: ConfirmedDecision,
    runtime: &TransactionRuntime<O, F>,
) -> Result<VerdictOutcome, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if decision.decision.verb.applies() {
        return apply_canonical_verdict(decision, runtime);
    }
    if decision.decision.rewrite.is_some() {
        return Err(TransactionError::InvalidRequest);
    }
    if runtime.root != decision.store.paths.root {
        return Err(TransactionError::InvalidReceipt);
    }
    decision.leases.verify_all_with(&runtime.ops)?;

    let ledger_path = &decision.store.paths.ledger;
    let view = read_ledger(ledger_path).map_err(|_| TransactionError::Read)?;
    if view.final_hash.as_deref() != decision.observed_ledger_head() {
        return Err(TransactionError::Read);
    }
    if decision.store.sealed == view.events.is_empty() {
        return Err(TransactionError::Read);
    }

    #[cfg(any(test, feature = "test-support"))]
    run_verdict_pre_append_hook_for_test();
    assert_confirmed_inputs_unchanged(&decision)?;

    let mut events = Vec::new();
    let previous_event_hash = if view.events.is_empty() {
        let genesis = GenesisEvent::new(GenesisPayload {
            schema_version: super::LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "genesis".to_owned(),
            sequence: 0,
            event_id: "evt-0000000000000000".to_owned(),
            created_at: decision.ruled_at.as_str().to_owned(),
            previous_event_hash: None,
            store_id: decision.store.policy.policy.store_id.clone(),
            profile_version: decision.profile.version.clone(),
            profile_hash: decision.profile.hash.clone(),
            policy_hash: decision.store.policy.hash.clone(),
            seed_manifest_hash: decision.store.seed.manifest_hash.clone(),
            seed_corpus_report_hash: decision.store.seed.corpus_report_hash.clone(),
            seed_bundle_hash: decision.store.seed.bundle_hash.clone(),
            source_commit: decision.store.seed.source_commit.clone(),
            initial_canonical_ref: "seed".to_owned(),
        })
        .map_err(|_| TransactionError::Write)?;
        let hash = genesis.hash().to_owned();
        events.push(LedgerEvent::Genesis(Box::new(genesis)));
        Some(hash)
    } else {
        view.final_hash.clone()
    };
    let sequence =
        u64::try_from(view.events.len() + events.len()).map_err(|_| TransactionError::Write)?;
    let event_id = format!("evt-{sequence:016}");
    let evaluator_meta_hash = decision
        .proposal
        .meta
        .evaluator
        .as_ref()
        .map(evaluator_commitment)
        .transpose()
        .map_err(|_| TransactionError::Read)?;
    let verdict = VerdictEvent::new(VerdictPayload {
        schema_version: super::LEDGER_EVENT_SCHEMA.to_owned(),
        event_type: "verdict".to_owned(),
        sequence,
        event_id: event_id.clone(),
        occurred_at: decision.ruled_at.as_str().to_owned(),
        previous_event_hash,
        store_id: decision.store.policy.policy.store_id.clone(),
        transaction_id: decision.transaction_id.clone(),
        proposal_id: decision.proposal.meta.proposal_id.clone(),
        proposal_meta_hash: decision.proposal.meta_hash.clone(),
        candidate_hash: decision.proposal.candidate_hash.clone(),
        source_id: decision.proposal.meta.source_id.clone(),
        verb: decision.decision.verb,
        reason: decision.decision.reason.as_str().to_owned(),
        verdict_id: decision.decision.verdict_id.clone(),
        actor: decision.store.policy.policy.human_actor.clone(),
        policy_hash: decision.store.policy.hash.clone(),
        profile_hash: decision.profile.hash.clone(),
        confidence: decision.proposal.meta.confidence,
        evaluator_meta_hash,
        base_kind: decision.proposal.meta.base.kind.clone(),
        base_generation: decision.proposal.meta.base.generation_id.clone(),
        base_manifest_hash: decision.proposal.meta.base.manifest_hash.clone(),
        base_bundle_hash: decision.proposal.meta.base.bundle_hash.clone(),
        base_target_hash: decision.proposal.meta.base.target_hash.clone(),
        rewrite_hash: None,
        rewrite_bytes: None,
        canonical_result: CanonicalResult::none(),
        resulting_canonical_head: decision.store.head.as_str().to_owned(),
        intake_sequence: decision.intake_head.sequence,
        intake_head_hash: decision.intake_head.event_hash.clone(),
    })
    .map_err(|_| TransactionError::Write)?;
    let verdict_hash = verdict.hash().to_owned();
    events.push(LedgerEvent::Verdict(Box::new(verdict)));

    let plan = runtime
        .create_transaction_plan(transaction_plan_from_decision(&decision)?, &decision.leases)?;
    let successor = successor_bytes_with(ledger_path, &events, &runtime.ops)?;
    let journal = runtime.prepare_transaction_journal(
        plan,
        &decision.leases,
        PreparedJournalInput {
            transaction_id: &decision.transaction_id,
            proposal_id: Some(&decision.proposal.meta.proposal_id),
            verdict_id: Some(&decision.decision.verdict_id),
            observed_ledger_head: decision.observed_ledger_head(),
            prepared_hashes: prepared_hashes(&decision, &verdict_hash),
            artifacts: PreparedArtifacts::ledger_only(
                verdict_hash,
                blake3::hash(&successor).to_hex().to_string(),
            ),
        },
    )?;
    runtime.faults.check(FaultPoint::AfterPreparedMarker)?;
    assert_confirmed_inputs_unchanged(&decision)?;
    decision.leases.verify_all_with(&runtime.ops)?;
    append_events_with(
        ledger_path,
        &decision.leases,
        &events,
        &successor,
        &runtime.ops,
        &runtime.faults,
    )?;
    decision
        .leases
        .verify_all_with(&runtime.ops)
        .map_err(|_| TransactionError::JournalState)?;
    journal
        .write_ledger_committed(&runtime.ops)
        .map_err(|_| TransactionError::JournalState)?;
    decision
        .leases
        .verify_all_with(&runtime.ops)
        .map_err(|_| TransactionError::JournalState)?;
    runtime
        .faults
        .check(FaultPoint::BeforePublishedMarker)
        .map_err(|_| TransactionError::JournalState)?;
    journal
        .write_published(&runtime.ops)
        .map_err(|_| TransactionError::JournalState)?;
    decision
        .leases
        .verify_all_with(&runtime.ops)
        .map_err(|_| TransactionError::JournalState)?;
    runtime
        .faults
        .check(FaultPoint::DuringCleanup)
        .map_err(|_| TransactionError::JournalState)?;
    journal
        .cleanup_with(&runtime.ops)
        .map_err(|_| TransactionError::JournalState)?;
    runtime
        .faults
        .check(FaultPoint::AfterJournalCleanup)
        .map_err(|_| TransactionError::JournalState)?;

    let allowance = VerificationAllowance {
        verdict_lease_token: Some(decision.leases.verdict_token().to_owned()),
        intake_lease_token: Some(decision.leases.intake_token().to_owned()),
        recovery_verdict_lease_token: None,
    };
    verify_store_level4_with_allowance(&decision.profile, &decision.store.paths.root, &allowance)
        .map_err(|_| TransactionError::JournalState)?;
    runtime
        .release_verdict_leases(&decision.leases)
        .map_err(|_| TransactionError::JournalState)?;

    Ok(VerdictOutcome {
        proposal_id: decision.proposal.meta.proposal_id,
        verdict_id: decision.decision.verdict_id,
        event_id,
        applied: false,
        generation: None,
        idempotent: false,
    })
}

fn transaction_plan_from_decision(
    decision: &ConfirmedDecision,
) -> Result<TransactionPlan, TransactionError> {
    let (parent_ref, parent_manifest_hash, parent_bundle_hash) = match &decision.store.head {
        CanonicalHead::Seed => (
            "seed".to_owned(),
            decision.store.seed.manifest_hash.clone(),
            decision.store.seed.bundle_hash.clone(),
        ),
        CanonicalHead::Generation(generation_id) => {
            let parent = verify_generation(
                &decision.profile,
                &decision
                    .store
                    .paths
                    .canon
                    .join("generations")
                    .join(generation_id),
                VerificationLevel::Three,
            )
            .map_err(|_| TransactionError::Read)?;
            (
                generation_id.clone(),
                parent.manifest_hash.clone(),
                parent.bundle_hash.clone(),
            )
        }
    };
    let applying = decision.decision.verb.applies();
    let generation_id = decision.generation_id.clone();
    let approved_target_hash = decision.approved_target_hash.clone();
    let judgment_hash = decision
        .judgment
        .as_ref()
        .map(|judgment| judgment.hash.clone());
    if applying
        != (generation_id.is_some() && approved_target_hash.is_some() && judgment_hash.is_some())
    {
        return Err(TransactionError::InvalidReceipt);
    }
    let mut owner_hashes = decision.leases.owner_hashes();
    owner_hashes.sort();
    Ok(TransactionPlan {
        schema_version: TRANSACTION_PLAN_SCHEMA.to_owned(),
        transaction_id: decision.transaction_id.clone(),
        command_class: "verdict".to_owned(),
        owner_hashes,
        proposal_id: decision.proposal.meta.proposal_id.clone(),
        verdict_id: decision.decision.verdict_id.clone(),
        observed_ledger_head: decision.observed_ledger_head.clone(),
        profile_hash: decision.profile.hash.clone(),
        policy_hash: decision.store.policy.hash.clone(),
        proposal_meta_hash: decision.proposal.meta_hash.clone(),
        candidate_hash: decision.proposal.candidate_hash.clone(),
        verb: decision.decision.verb.as_str().to_owned(),
        target_id: decision.proposal.meta.target_id.clone(),
        target_path: decision.proposal.meta.target_path.clone(),
        base_target_hash: decision.proposal.meta.base.target_hash.clone(),
        parent_ref,
        parent_manifest_hash,
        parent_bundle_hash,
        generation_id,
        approved_target_hash,
        judgment_hash,
        reason_hash: blake3::hash(decision.decision.reason.as_str().as_bytes())
            .to_hex()
            .to_string(),
        ruled_at: decision.ruled_at.as_str().to_owned(),
    })
}

fn apply_canonical_verdict<O, F>(
    decision: ConfirmedDecision,
    runtime: &TransactionRuntime<O, F>,
) -> Result<VerdictOutcome, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if !decision.decision.verb.applies()
        || decision.generation_id().is_none()
        || decision.approved_target_bytes().is_none()
        || decision.approved_target_hash().is_none()
        || decision.judgment().is_none()
        || runtime.root != decision.store.paths.root
    {
        return Err(TransactionError::CanonicalReceiptInvalid);
    }
    decision.leases.verify_all_with(&runtime.ops)?;

    let ledger_path = &decision.store.paths.ledger;
    let view = read_ledger(ledger_path).map_err(|_| TransactionError::CanonicalLedgerRead)?;
    if view.final_hash.as_deref() != decision.observed_ledger_head()
        || decision.store.sealed == view.events.is_empty()
    {
        return Err(TransactionError::CanonicalLedgerStateChanged);
    }

    #[cfg(any(test, feature = "test-support"))]
    run_verdict_pre_append_hook_for_test();
    assert_confirmed_inputs_unchanged(&decision)?;

    let transaction_input =
        transaction_plan_from_decision(&decision).map_err(|error| match error {
            TransactionError::InvalidReceipt => TransactionError::CanonicalReceiptInvalid,
            other => other,
        })?;
    let transaction_plan = runtime
        .create_transaction_plan(transaction_input, &decision.leases)
        .map_err(|error| match error {
            TransactionError::Write => TransactionError::PlanPrepare,
            other => other,
        })?;
    let generation_id = decision
        .generation_id()
        .ok_or(TransactionError::InvalidReceipt)?
        .to_owned();
    let target_bytes = decision
        .approved_target_bytes()
        .ok_or(TransactionError::InvalidReceipt)?
        .to_vec();
    let target_hash = decision
        .approved_target_hash()
        .ok_or(TransactionError::InvalidReceipt)?
        .to_owned();
    let judgment = decision
        .judgment()
        .cloned()
        .ok_or(TransactionError::InvalidReceipt)?;
    let generation_input = AppliedGenerationInput {
        generation_id: generation_id.clone(),
        proposal_id: decision.proposal.meta.proposal_id.clone(),
        verdict_id: decision.decision.verdict_id.clone(),
        policy_hash: decision.store.policy.hash.clone(),
        operation: decision.proposal.meta.operation,
        target_path: decision.proposal.meta.target_path.clone(),
        target_id: decision.proposal.meta.target_id.clone(),
        target_bytes,
        judgment,
    };
    let plan = match &decision.store.head {
        CanonicalHead::Seed => GenerationPlan::from_seed(
            &decision.profile,
            &decision.store.paths.seed,
            generation_input,
        ),
        CanonicalHead::Generation(parent) => {
            let parent = verify_generation(
                &decision.profile,
                &decision.store.paths.canon.join("generations").join(parent),
                VerificationLevel::Three,
            )
            .map_err(|_| TransactionError::Read)?;
            GenerationPlan::from_generation(&decision.profile, &parent, generation_input)
        }
    }
    .map_err(|_| TransactionError::Write)?;
    let staged_generation_root = decision
        .store
        .paths
        .canon
        .join("generations")
        .join(format!(".txn-{}", decision.transaction_id));
    let mut generation = stage_generation(&decision.profile, &plan, &staged_generation_root)
        .map_err(|_| TransactionError::Write)?;
    decision.leases.verify_all_with(&runtime.ops)?;
    runtime.faults.check(FaultPoint::AfterTargetBytesStaged)?;
    runtime.faults.check(FaultPoint::AfterJudgmentBytesStaged)?;
    runtime.faults.check(FaultPoint::AfterManifestSync)?;
    runtime.faults.check(FaultPoint::AfterLevelThreeSelfCheck)?;
    runtime
        .ops
        .sync_directory(&decision.store.paths.canon.join("generations"))
        .map_err(|_| TransactionError::Write)?;

    let staged_index_root = decision
        .store
        .paths
        .derived
        .join("indexes")
        .join(format!(".txn-{}", decision.transaction_id));
    runtime.faults.check(FaultPoint::DuringIndexBuild)?;
    let index = build_p3_index_generation(P3IndexRequest {
        profile: &decision.profile,
        generation: &generation,
        index_root: &staged_index_root,
    })
    .map_err(|_| TransactionError::Write)?;
    decision.leases.verify_all_with(&runtime.ops)?;
    runtime.faults.check(FaultPoint::AfterIndexSelfCheck)?;
    runtime
        .ops
        .sync_directory(&decision.store.paths.derived.join("indexes"))
        .map_err(|_| TransactionError::Write)?;

    let receipt_path = decision
        .store
        .paths
        .evidence
        .join("reindex")
        .join(format!("{}.json", decision.decision.verdict_id));
    require_absent(&runtime.ops, &receipt_path)?;

    let canonical_result = CanonicalResult {
        generation_id: Some(generation_id.to_owned()),
        manifest_hash: Some(generation.manifest_hash.clone()),
        bundle_hash: Some(generation.bundle_hash.clone()),
        target_prior_hash: decision.proposal.meta.base.target_hash.clone(),
        target_new_hash: Some(target_hash),
        judgment_hash: Some(
            decision
                .judgment()
                .ok_or(TransactionError::InvalidReceipt)?
                .hash
                .clone(),
        ),
        reindex_receipt_hash: Some(index.receipt_hash.clone()),
    };
    let (events, event_id, verdict_hash) =
        canonical_verdict_events(&decision, &view, canonical_result, &generation_id)?;

    let final_generation_root = decision
        .store
        .paths
        .canon
        .join("generations")
        .join(&generation_id);
    let final_index_root = decision
        .store
        .paths
        .derived
        .join("indexes")
        .join(&generation_id);
    require_absent(&runtime.ops, &final_generation_root)?;
    require_absent(&runtime.ops, &final_index_root)?;

    let mut journal_hashes = prepared_hashes(&decision, &verdict_hash);
    journal_hashes.extend([
        generation.manifest_hash.clone(),
        generation.bundle_hash.clone(),
        index.receipt_hash.clone(),
    ]);
    journal_hashes.sort();
    journal_hashes.dedup();
    let successor = successor_bytes_with(ledger_path, &events, &runtime.ops)?;
    let journal = runtime.prepare_transaction_journal(
        transaction_plan,
        &decision.leases,
        PreparedJournalInput {
            transaction_id: &decision.transaction_id,
            proposal_id: Some(&decision.proposal.meta.proposal_id),
            verdict_id: Some(&decision.decision.verdict_id),
            observed_ledger_head: decision.observed_ledger_head(),
            prepared_hashes: journal_hashes,
            artifacts: PreparedArtifacts {
                verdict_event_hash: verdict_hash,
                ledger_successor_hash: blake3::hash(&successor).to_hex().to_string(),
                generation_id: Some(generation_id.clone()),
                manifest_hash: Some(generation.manifest_hash.clone()),
                bundle_hash: Some(generation.bundle_hash.clone()),
                reindex_receipt_hash: Some(index.receipt_hash.clone()),
            },
        },
    )?;
    runtime.faults.check(FaultPoint::AfterPreparedMarker)?;

    #[cfg(any(test, feature = "test-support"))]
    run_verdict_late_intake_check_hook_for_test();
    if let Err(error) = assert_confirmed_inputs_unchanged(&decision) {
        discard_unpublished_applying_artifacts(
            &decision,
            runtime,
            &staged_generation_root,
            &staged_index_root,
        )?;
        return Err(error);
    }

    runtime
        .ops
        .write_new_synced(&receipt_path, &index.receipt_bytes)
        .map_err(|_| TransactionError::Write)?;
    runtime
        .ops
        .sync_directory(&decision.store.paths.evidence.join("reindex"))
        .map_err(|_| TransactionError::Write)?;
    runtime.faults.check(FaultPoint::AfterReindexReceiptSync)?;

    require_absent(&runtime.ops, &final_generation_root)?;
    require_absent(&runtime.ops, &final_index_root)?;
    decision.leases.verify_all_with(&runtime.ops)?;
    runtime
        .ops
        .rename(&staged_generation_root, &final_generation_root)
        .map_err(|_| TransactionError::Write)?;
    runtime
        .ops
        .sync_directory(&decision.store.paths.canon.join("generations"))
        .map_err(|_| TransactionError::Write)?;
    generation.root = final_generation_root;
    runtime.faults.check(FaultPoint::AfterGenerationRename)?;
    decision.leases.verify_all_with(&runtime.ops)?;
    runtime
        .ops
        .rename(&staged_index_root, &final_index_root)
        .map_err(|_| TransactionError::Write)?;
    runtime
        .ops
        .sync_directory(&decision.store.paths.derived.join("indexes"))
        .map_err(|_| TransactionError::Write)?;
    runtime.faults.check(FaultPoint::AfterDerivedIndexRename)?;

    decision.leases.verify_all_with(&runtime.ops)?;
    append_events_with(
        ledger_path,
        &decision.leases,
        &events,
        &successor,
        &runtime.ops,
        &runtime.faults,
    )?;
    decision
        .leases
        .verify_all_with(&runtime.ops)
        .map_err(|_| TransactionError::JournalState)?;
    journal
        .write_ledger_committed(&runtime.ops)
        .map_err(|_| TransactionError::JournalState)?;
    publish_current(
        &decision.store.paths.current,
        &generation_id,
        &decision.transaction_id,
        runtime,
        None,
    )?;
    decision
        .leases
        .verify_all_with(&runtime.ops)
        .map_err(|_| TransactionError::JournalState)?;
    runtime
        .faults
        .check(FaultPoint::BeforePublishedMarker)
        .map_err(|_| TransactionError::JournalState)?;
    journal
        .write_published(&runtime.ops)
        .map_err(|_| TransactionError::JournalState)?;
    decision
        .leases
        .verify_all_with(&runtime.ops)
        .map_err(|_| TransactionError::JournalState)?;
    runtime
        .faults
        .check(FaultPoint::DuringCleanup)
        .map_err(|_| TransactionError::JournalState)?;
    journal
        .cleanup_with(&runtime.ops)
        .map_err(|_| TransactionError::JournalState)?;
    runtime
        .faults
        .check(FaultPoint::AfterJournalCleanup)
        .map_err(|_| TransactionError::JournalState)?;

    let allowance = VerificationAllowance {
        verdict_lease_token: Some(decision.leases.verdict_token().to_owned()),
        intake_lease_token: Some(decision.leases.intake_token().to_owned()),
        recovery_verdict_lease_token: None,
    };
    verify_store_level4_with_allowance(&decision.profile, &decision.store.paths.root, &allowance)
        .map_err(|_| TransactionError::JournalState)?;
    runtime
        .release_verdict_leases(&decision.leases)
        .map_err(|_| TransactionError::JournalState)?;

    Ok(VerdictOutcome {
        proposal_id: decision.proposal.meta.proposal_id,
        verdict_id: decision.decision.verdict_id,
        event_id,
        applied: true,
        generation: Some(generation_id),
        idempotent: false,
    })
}

pub fn recover_store(
    request: RecoveryRequest,
    runtime: &TransactionRuntime,
) -> Result<RecoveryOutcome, TransactionError> {
    recover_store_with_tty_path(request, runtime, Path::new("/dev/tty"))
}

pub fn recover_store_human(request: RecoveryRequest) -> Result<RecoveryOutcome, TransactionError> {
    let profile = load_p3_profile(&request.profile_path).map_err(|_| TransactionError::Read)?;
    let snapshot = store::inspect_store_with_transients(&profile, &request.store_root)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let runtime = TransactionRuntime::production(&profile, &snapshot)?;
    // Reject structurally ambiguous recovery state before opening a terminal.
    RecoveryJournal::discover(&runtime.ops, &snapshot.paths.transactions)?;
    discover_original_journal(&runtime.ops, &snapshot.paths.transactions)?;
    recover_store(request, &runtime)
}

pub fn recover_store_with_tty_path<O, F>(
    request: RecoveryRequest,
    runtime: &TransactionRuntime<O, F>,
    tty_path: &Path,
) -> Result<RecoveryOutcome, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let mut human = TtyHumanPresence::open_at(tty_path)?;
    recover_store_with_human(request, runtime, &mut human, &SystemClock)
}

pub fn recover_store_with_human<O, F, H, C>(
    request: RecoveryRequest,
    runtime: &TransactionRuntime<O, F>,
    human: &mut H,
    clock: &C,
) -> Result<RecoveryOutcome, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
    H: HumanPresence,
    C: Clock,
{
    require_supported_platform()?;
    let profile = load_p3_profile(&request.profile_path).map_err(|_| TransactionError::Read)?;
    let snapshot = store::inspect_store_with_transients(&profile, &request.store_root)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if runtime.root != snapshot.paths.root {
        return Err(TransactionError::InvalidReceipt);
    }
    if let Some(recovery_journal) =
        RecoveryJournal::discover(&runtime.ops, &snapshot.paths.transactions)?
    {
        if recovery_journal.cleanup_form {
            return resume_recovery_cleanup_witness_tail(
                &profile,
                &snapshot,
                runtime,
                recovery_journal,
            );
        }
        if recovery_journal.is_published(&runtime.ops)? {
            return finish_published_recovery_tail(&profile, &snapshot, runtime, recovery_journal);
        }
        return resume_unpublished_recovery(&profile, &snapshot, runtime, human, recovery_journal);
    }
    let stale = RecoveryStaleLeases::read(&runtime.ops, &snapshot.paths)?;
    if stale.is_none() {
        return verify_clean_recovery(&profile, &snapshot, runtime);
    }
    let stale = stale.expect("checked above");
    if let Some(verdict) = stale.verdict.as_ref() {
        if verdict.owner.command_class() == "recovery" {
            if has_lease_preparation(&snapshot.paths.locks, LeaseKind::Verdict)? {
                return Err(TransactionError::Locked(LeaseKind::Verdict));
            }
            if let Some(bootstrap) =
                in_progress_recovery_bootstrap(runtime, &profile, &snapshot, &stale)?
            {
                return resume_recovery_bootstrap(&profile, runtime, human, clock, bootstrap);
            }
            return finish_recovery_lease_tail(&profile, &snapshot, runtime, &stale);
        }
        if stale.intake.is_none()
            && discover_original_journal(&runtime.ops, &snapshot.paths.transactions)?
                .root
                .is_none()
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    human.show(&render_recovery_display(&stale))?;
    require_exact_recovery_confirmation(human, stale.confirmation_owner_id()?)?;

    let journal =
        discover_original_journal(&runtime.ops, &snapshot.paths.transactions).or_else(|_| {
            discover_unbound_terminal_original_cleanup_residue(
                &runtime.ops,
                &snapshot.paths.transactions,
            )
        })?;
    let ledger = read_ledger(&snapshot.paths.ledger).map_err(|_| TransactionError::Read)?;
    validate_recovery_stale_leases_against_journal(
        &snapshot, &profile, runtime, &stale, &journal, &ledger,
    )?;
    let recovery_owner_hashes = stale.owner_hashes();
    let mode = select_recovery_mode(&snapshot, &stale, &journal, &ledger)?;
    let command_class = recovery_source_command_class(mode, &stale)?.to_owned();
    let cleanup_plan = match mode {
        RecoveryMode::VerdictTransaction => {
            prepare_recovery_cleanup_plan(&snapshot, &profile, runtime, &journal, &ledger)?
        }
        RecoveryMode::IntakeOnly => prepare_intake_cleanup_plan(
            &runtime.ops,
            &snapshot.paths,
            stale
                .intake
                .as_ref()
                .ok_or(TransactionError::RecoveryAmbiguous)?,
        )?,
        RecoveryMode::CleanFinalization => RecoveryCleanupPlan::empty(),
    };
    let cleanup_plan_hash = cleanup_plan.hash()?;
    let mut recovery_owner_frames = recovery_owner_hashes
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    recovery_owner_frames.push(&cleanup_plan_hash);
    let recovery_owner = lease_receipt_id(
        "recovery",
        &snapshot.policy.policy.store_id,
        Some(snapshot.head.as_str()),
        &profile.hash,
        &snapshot.policy.hash,
        &recovery_owner_frames,
    );
    let quarantined_verdict = stale.quarantine_verdict(runtime)?;
    let recovery_lease = runtime.acquire_lease(LeaseKind::Verdict, &recovery_owner, "recovery")?;
    runtime
        .faults
        .check(FaultPoint::AfterRecoveryLeaseAcquisition)?;
    let quarantined = QuarantinedLeases {
        verdict: quarantined_verdict,
        intake: stale.quarantine_intake(runtime, &recovery_lease)?,
    };
    if quarantined.owner_hashes() != recovery_owner_hashes {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let quarantined =
        QuarantinedLeases::read_bound(&runtime.ops, &snapshot.paths, &quarantined.bindings())?;
    if quarantined.owner_hashes() != recovery_owner_hashes {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    recovery_lease.verify_with(&runtime.ops)?;
    let journal = normalize_plan_publication(runtime, &journal, &recovery_lease)?;
    let revalidated_ledger =
        read_ledger(&snapshot.paths.ledger).map_err(|_| TransactionError::Read)?;
    validate_recovery_stale_leases_against_journal(
        &snapshot,
        &profile,
        runtime,
        &stale,
        &journal,
        &revalidated_ledger,
    )?;
    let revalidated_mode = select_recovery_mode(&snapshot, &stale, &journal, &revalidated_ledger)?;
    let revalidated_cleanup_plan = match revalidated_mode {
        RecoveryMode::VerdictTransaction => prepare_recovery_cleanup_plan(
            &snapshot,
            &profile,
            runtime,
            &journal,
            &revalidated_ledger,
        )?,
        RecoveryMode::IntakeOnly => prepare_intake_cleanup_plan(
            &runtime.ops,
            &snapshot.paths,
            stale
                .intake
                .as_ref()
                .ok_or(TransactionError::RecoveryAmbiguous)?,
        )?,
        RecoveryMode::CleanFinalization => RecoveryCleanupPlan::empty(),
    };
    if revalidated_ledger.final_hash != ledger.final_hash
        || revalidated_mode != mode
        || revalidated_cleanup_plan != cleanup_plan
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let recovery_at = clock.now().map_err(|_| TransactionError::Read)?;
    let original_journal_hash = journal.commitment_hash();
    let recovery_id = recovery_transaction_id(
        &recovery_owner_hashes,
        &recovery_at,
        ledger.final_hash.as_deref(),
        &command_class,
        original_journal_hash.as_deref(),
        &profile.hash,
        &snapshot.policy.hash,
        &cleanup_plan_hash,
    );
    create_and_finish_recovery(
        &profile,
        &snapshot,
        runtime,
        &journal,
        &ledger,
        &quarantined,
        &recovery_lease,
        RecoveryJournalRecord {
            schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
            recovery_id,
            mode,
            command_class,
            recovered_at: recovery_at.as_str().to_owned(),
            quarantined_leases: quarantined.bindings(),
            recovery_lease_owner_hash: recovery_lease.owner_hash().to_owned(),
            original_transaction_id: journal.transaction_id().map(str::to_owned),
            original_journal_hash,
            observed_ledger_head: ledger.final_hash.clone(),
            profile_hash: profile.hash.clone(),
            policy_hash: snapshot.policy.hash.clone(),
            cleanup_plan,
            cleanup_plan_hash,
        },
    )
}

pub fn recover_store_with_human_for_test<O, F, H, C>(
    request: RecoveryRequest,
    runtime: &TransactionRuntime<O, F>,
    human: &mut H,
    clock: &C,
) -> Result<RecoveryOutcome, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
    H: HumanPresence,
    C: Clock,
{
    recover_store_with_human(request, runtime, human, clock)
}

fn has_lease_preparation(parent: &Path, kind: LeaseKind) -> Result<bool, TransactionError> {
    let prefix = format!(".lease-{}-", kind.name());
    Ok(sorted_directory_entries(parent)?.into_iter().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(&prefix))
    }))
}

#[allow(clippy::too_many_arguments)] // Recovery values are independently bound and intentionally explicit.
fn create_and_finish_recovery<O, F>(
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
    ledger: &super::LedgerView,
    quarantined: &QuarantinedLeases,
    recovery_lease: &Lease,
    record: RecoveryJournalRecord,
) -> Result<RecoveryOutcome, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let mode = record.mode;
    let recovery_journal = RecoveryJournal::create_protected(runtime, record, recovery_lease)?;
    runtime
        .faults
        .check(FaultPoint::AfterRecoveryJournalCreation)?;

    if matches!(
        mode,
        RecoveryMode::VerdictTransaction | RecoveryMode::IntakeOnly
    ) {
        let typed_directories = typed_recovery_cleanup_directories_for_execution(
            snapshot,
            profile,
            runtime,
            journal,
            ledger,
            &recovery_journal.record,
        )?;
        execute_recovery_cleanup_plan(
            &runtime.ops,
            &snapshot.paths.root,
            &recovery_journal.record.cleanup_plan,
            recovery_lease,
            &typed_directories,
        )?;
    }

    let rolled_forward = match mode {
        RecoveryMode::VerdictTransaction => recover_verdict_transaction(
            snapshot,
            profile,
            runtime,
            journal,
            ledger,
            recovery_lease,
        )?,
        RecoveryMode::IntakeOnly => {
            recover_intake_only(
                snapshot,
                runtime,
                quarantined,
                &recovery_journal,
                recovery_lease,
            )?;
            false
        }
        RecoveryMode::CleanFinalization => false,
    };

    let original_cleanup_witness = recovered_original_cleanup_witness(
        journal,
        &runtime.ops,
        Some(&recovery_journal.root),
        recovery_lease,
    )?;
    runtime
        .faults
        .check(FaultPoint::AfterOriginalCleanupWitness)?;
    recovery_journal.write_published_protected(&runtime.ops, recovery_lease)?;
    runtime
        .faults
        .check(FaultPoint::AfterRecoveryPublishedMarker)?;
    complete_recovery_tail(
        profile,
        snapshot,
        runtime,
        &recovery_journal,
        &original_cleanup_witness,
        quarantined,
        recovery_lease,
    )?;

    Ok(RecoveryOutcome {
        rolled_forward,
        idempotent: false,
        mode,
    })
}

fn normalize_plan_publication<O, F>(
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
    recovery_lease: &Lease,
) -> Result<OriginalJournal, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let Some(prep) = journal.plan_preparation.as_deref() else {
        return Ok(journal.clone());
    };
    let transaction_id = journal
        .transaction_id()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let expected_plan = journal
        .plan
        .as_ref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let transactions = runtime.root.join("transactions");
    let root = transactions.join(transaction_id);
    let final_plan = root.join("plan.json");

    let current = discover_original_journal(&runtime.ops, &transactions)?;
    if current.transaction_id() != Some(transaction_id)
        || current.plan.as_ref().map(|plan| plan.bytes.as_slice())
            != Some(expected_plan.bytes.as_slice())
        || current.plan_preparation.as_deref() != Some(prep)
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }

    match runtime.ops.metadata(&root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            recovery_lease.verify_with(&runtime.ops)?;
            runtime
                .ops
                .create_dir(&root)
                .map_err(|_| TransactionError::Write)?;
            recovery_lease.verify_with(&runtime.ops)?;
            runtime
                .ops
                .sync_directory(&transactions)
                .map_err(|_| TransactionError::Write)?;
            runtime
                .faults
                .check(FaultPoint::AfterPlanFinalDirectorySync)?;
        }
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {}
        _ => return Err(TransactionError::RecoveryAmbiguous),
    }

    let names = original_journal_entry_names(&runtime.ops, &root)?;
    if names.is_empty() {
        recovery_lease.verify_with(&runtime.ops)?;
        runtime
            .ops
            .hard_link(prep, &final_plan)
            .map_err(|_| TransactionError::Write)?;
        recovery_lease.verify_with(&runtime.ops)?;
        runtime
            .ops
            .sync_directory(&root)
            .map_err(|_| TransactionError::Write)?;
        recovery_lease.verify_with(&runtime.ops)?;
        runtime
            .ops
            .sync_directory(&transactions)
            .map_err(|_| TransactionError::Write)?;
        runtime.faults.check(FaultPoint::AfterPlanHardLinkSync)?;
    } else if names != ["plan.json"] {
        return Err(TransactionError::RecoveryAmbiguous);
    }

    validate_plan_publication_p3(&runtime.ops, prep, &root, &expected_plan.bytes)?;
    recovery_lease.verify_with(&runtime.ops)?;
    validate_plan_publication_p3(&runtime.ops, prep, &root, &expected_plan.bytes)?;
    runtime
        .ops
        .remove_file(prep)
        .map_err(|_| TransactionError::Write)?;
    recovery_lease.verify_with(&runtime.ops)?;
    runtime
        .ops
        .sync_directory(&transactions)
        .map_err(|_| TransactionError::Write)?;
    runtime.faults.check(FaultPoint::AfterPlanPrepRemoval)?;

    let normalized = discover_original_journal(&runtime.ops, &transactions)?;
    if normalized.plan_preparation.is_some()
        || normalized.transaction_id() != Some(transaction_id)
        || normalized.plan.as_ref().map(|plan| plan.bytes.as_slice())
            != Some(expected_plan.bytes.as_slice())
        || normalized.files != ["plan.json"]
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(normalized)
}

fn validate_plan_publication_p3(
    ops: &impl FilesystemOps,
    prep: &Path,
    root: &Path,
    expected_bytes: &[u8],
) -> Result<(), TransactionError> {
    if original_journal_entry_names(ops, root)? != ["plan.json"]
        || read_original_journal_regular_file(ops, prep)? != expected_bytes
        || read_original_journal_regular_file(ops, &root.join("plan.json"))? != expected_bytes
        || !same_regular_file_identity(ops, prep, &root.join("plan.json"))?
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(())
}

fn resume_recovery_bootstrap<O, F, H, C>(
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    human: &mut H,
    clock: &C,
    bootstrap: RecoveryBootstrap,
) -> Result<RecoveryOutcome, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
    H: HumanPresence,
    C: Clock,
{
    human.show(&render_recovery_display(&bootstrap.originals))?;
    require_exact_recovery_confirmation(human, bootstrap.originals.confirmation_owner_id()?)?;

    let snapshot = store::inspect_store_with_transients(profile, &runtime.root)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let recovery_presence = RecoveryStaleLeases::read(&runtime.ops, &snapshot.paths)?
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let refreshed =
        in_progress_recovery_bootstrap(runtime, profile, &snapshot, &recovery_presence)?
            .ok_or(TransactionError::RecoveryAmbiguous)?;
    if !bootstrap.matches(&refreshed) {
        return Err(TransactionError::RecoveryAmbiguous);
    }

    let journal =
        discover_original_journal(&runtime.ops, &snapshot.paths.transactions).or_else(|_| {
            discover_unbound_terminal_original_cleanup_residue(
                &runtime.ops,
                &snapshot.paths.transactions,
            )
        })?;
    let ledger = read_ledger(&snapshot.paths.ledger).map_err(|_| TransactionError::Read)?;
    validate_recovery_stale_leases_against_journal(
        &snapshot,
        profile,
        runtime,
        &refreshed.originals,
        &journal,
        &ledger,
    )?;
    let original_owner_hashes = refreshed.originals.owner_hashes();
    refreshed.recovery_lease.verify_with(&runtime.ops)?;
    let quarantined = QuarantinedLeases {
        verdict: refreshed.quarantined.verdict.clone(),
        intake: match refreshed.quarantined.intake.clone() {
            Some(intake) => Some(intake),
            None => refreshed
                .originals
                .quarantine_intake(runtime, &refreshed.recovery_lease)?,
        },
    };
    if quarantined.owner_hashes() != original_owner_hashes {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let quarantined =
        QuarantinedLeases::read_bound(&runtime.ops, &snapshot.paths, &quarantined.bindings())?;
    if quarantined.owner_hashes() != original_owner_hashes {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    refreshed.recovery_lease.verify_with(&runtime.ops)?;
    let journal = normalize_plan_publication(runtime, &journal, &refreshed.recovery_lease)?;
    let ledger = read_ledger(&snapshot.paths.ledger).map_err(|_| TransactionError::Read)?;
    validate_recovery_stale_leases_against_journal(
        &snapshot,
        profile,
        runtime,
        &refreshed.originals,
        &journal,
        &ledger,
    )?;
    let mode = select_recovery_mode(&snapshot, &refreshed.originals, &journal, &ledger)?;
    let command_class = recovery_source_command_class(mode, &refreshed.originals)?.to_owned();
    let cleanup_plan = match mode {
        RecoveryMode::VerdictTransaction => {
            prepare_recovery_cleanup_plan(&snapshot, profile, runtime, &journal, &ledger)?
        }
        RecoveryMode::IntakeOnly => prepare_intake_cleanup_plan(
            &runtime.ops,
            &snapshot.paths,
            refreshed
                .originals
                .intake
                .as_ref()
                .ok_or(TransactionError::RecoveryAmbiguous)?,
        )?,
        RecoveryMode::CleanFinalization => RecoveryCleanupPlan::empty(),
    };
    let cleanup_plan_hash = cleanup_plan.hash()?;
    let recovery_at = clock.now().map_err(|_| TransactionError::Read)?;
    let original_journal_hash = journal.commitment_hash();
    let recovery_id = recovery_transaction_id(
        &original_owner_hashes,
        &recovery_at,
        ledger.final_hash.as_deref(),
        &command_class,
        original_journal_hash.as_deref(),
        &profile.hash,
        &snapshot.policy.hash,
        &cleanup_plan_hash,
    );

    create_and_finish_recovery(
        profile,
        &snapshot,
        runtime,
        &journal,
        &ledger,
        &quarantined,
        &refreshed.recovery_lease,
        RecoveryJournalRecord {
            schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
            recovery_id,
            mode,
            command_class,
            recovered_at: recovery_at.as_str().to_owned(),
            quarantined_leases: quarantined.bindings(),
            recovery_lease_owner_hash: refreshed.recovery_lease.owner_hash().to_owned(),
            original_transaction_id: journal.transaction_id().map(str::to_owned),
            original_journal_hash,
            observed_ledger_head: ledger.final_hash.clone(),
            profile_hash: profile.hash.clone(),
            policy_hash: snapshot.policy.hash.clone(),
            cleanup_plan,
            cleanup_plan_hash,
        },
    )
}

fn finish_recovery_lease_tail<O, F>(
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    stale: &RecoveryStaleLeases,
) -> Result<RecoveryOutcome, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let recovery_owner = stale
        .verdict
        .as_ref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    if stale.intake.is_some()
        || recovery_owner.owner.command_class() != "recovery"
        || !valid_receipt_id(recovery_owner.owner.owner_id())
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    ensure_recovery_quarantines_absent(&snapshot.paths)?;
    let recovery_lease = Lease {
        root: snapshot.paths.root.clone(),
        kind: LeaseKind::Verdict,
        owner_hash: recovery_owner.owner_hash.clone(),
        lease_token: recovery_owner.owner.lease_token().to_owned(),
    };
    recovery_lease.verify_with(&runtime.ops)?;
    if let Some(residue) =
        terminal_empty_recovery_cleanup_residue(&runtime.ops, &snapshot.paths.transactions)?
    {
        // The protected recovery lease reaches this path only after an
        // authenticated recovery tail. An empty cleanup directory is accepted
        // solely as that tail's final liveness residue.
        runtime
            .ops
            .sync_directory(&snapshot.paths.transactions)
            .map_err(|_| TransactionError::Write)?;
        recovery_lease.verify_with(&runtime.ops)?;
        runtime
            .ops
            .remove_dir(&residue)
            .map_err(|_| TransactionError::Write)?;
    }
    verify_recovery_store(profile, snapshot, &recovery_lease)?;
    recovery_lease.release_terminal(&runtime.ops)?;
    Ok(RecoveryOutcome {
        rolled_forward: false,
        idempotent: false,
        mode: RecoveryMode::CleanFinalization,
    })
}

fn resume_unpublished_recovery<O, F, H>(
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    human: &mut H,
    recovery_journal: RecoveryJournal,
) -> Result<RecoveryOutcome, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
    H: HumanPresence,
{
    let bindings = validate_recovery_prefix(profile, snapshot, runtime, &recovery_journal, false)?;
    human.show(&render_recovery_display(&bindings.recovery_presence))?;
    require_exact_recovery_confirmation(
        human,
        bindings.recovery_presence.confirmation_owner_id()?,
    )?;

    if matches!(
        recovery_journal.record.mode,
        RecoveryMode::VerdictTransaction | RecoveryMode::IntakeOnly
    ) {
        let typed_directories = typed_recovery_cleanup_directories_for_execution(
            snapshot,
            profile,
            runtime,
            &bindings.journal,
            &bindings.ledger,
            &recovery_journal.record,
        )?;
        execute_recovery_cleanup_plan(
            &runtime.ops,
            &snapshot.paths.root,
            &recovery_journal.record.cleanup_plan,
            &bindings.recovery_lease,
            &typed_directories,
        )?;
    }

    let rolled_forward = match recovery_journal.record.mode {
        RecoveryMode::VerdictTransaction
            if bindings.journal.root.is_none() || bindings.journal.cleanup_form =>
        {
            cleanup_witness_rolled_forward(&recovery_journal.record, &bindings.ledger)?
        }
        RecoveryMode::VerdictTransaction => recover_verdict_transaction(
            snapshot,
            profile,
            runtime,
            &bindings.journal,
            &bindings.ledger,
            &bindings.recovery_lease,
        )?,
        RecoveryMode::IntakeOnly => {
            recover_intake_only(
                snapshot,
                runtime,
                &bindings.quarantined,
                &recovery_journal,
                &bindings.recovery_lease,
            )?;
            false
        }
        RecoveryMode::CleanFinalization => false,
    };

    let original_cleanup_witness = recovered_original_cleanup_witness(
        &bindings.journal,
        &runtime.ops,
        Some(&recovery_journal.root),
        &bindings.recovery_lease,
    )?;
    runtime
        .faults
        .check(FaultPoint::AfterOriginalCleanupWitness)?;
    recovery_journal.write_published_protected(&runtime.ops, &bindings.recovery_lease)?;
    runtime
        .faults
        .check(FaultPoint::AfterRecoveryPublishedMarker)?;
    complete_recovery_tail(
        profile,
        snapshot,
        runtime,
        &recovery_journal,
        &original_cleanup_witness,
        &bindings.quarantined,
        &bindings.recovery_lease,
    )?;

    Ok(RecoveryOutcome {
        rolled_forward,
        idempotent: false,
        mode: recovery_journal.record.mode,
    })
}

pub struct ValidatedRecoveryPrefix {
    ledger: super::LedgerView,
    recovery_lease: Lease,
    recovery_presence: RecoveryStaleLeases,
    quarantined: QuarantinedLeases,
    journal: OriginalJournal,
}

pub fn validate_recovery_prefix<O, F>(
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    recovery_journal: &RecoveryJournal,
    expected_published: bool,
) -> Result<ValidatedRecoveryPrefix, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    recovery_journal
        .verify(&runtime.ops)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if recovery_journal.cleanup_form
        || recovery_journal.is_published(&runtime.ops)? != expected_published
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }

    let record = &recovery_journal.record;
    let recorded_owner_hashes = validate_recovery_record_identity(profile, snapshot, record)?;

    let ledger = read_ledger(&snapshot.paths.ledger).map_err(|_| TransactionError::Read)?;
    if ledger.final_hash != record.observed_ledger_head {
        return Err(TransactionError::RecoveryAmbiguous);
    }

    let recovery_owner = read_optional_stale_lease(
        &runtime.ops,
        &snapshot.paths.locks.join("verdict.lock"),
        RecoveryLeaseKind::Verdict,
    )?
    .ok_or(TransactionError::RecoveryAmbiguous)?;
    let mut recovery_owner_frames = recorded_owner_hashes
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    recovery_owner_frames.push(&record.cleanup_plan_hash);
    let expected_recovery_owner_id = lease_receipt_id(
        "recovery",
        &snapshot.policy.policy.store_id,
        Some(snapshot.head.as_str()),
        &profile.hash,
        &snapshot.policy.hash,
        &recovery_owner_frames,
    );
    if recovery_owner.owner.command_class() != "recovery"
        || recovery_owner.owner.owner_id() != expected_recovery_owner_id
        || recovery_owner.owner_hash != record.recovery_lease_owner_hash
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if read_optional_stale_lease(
        &runtime.ops,
        &snapshot.paths.locks.join("intake.lock"),
        RecoveryLeaseKind::Intake,
    )?
    .is_some()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let recovery_lease = Lease {
        root: snapshot.paths.root.clone(),
        kind: LeaseKind::Verdict,
        owner_hash: recovery_owner.owner_hash.clone(),
        lease_token: recovery_owner.owner.lease_token().to_owned(),
    };
    recovery_lease.verify_with(&runtime.ops)?;
    let recovery_presence = RecoveryStaleLeases {
        verdict: Some(recovery_owner),
        intake: None,
    };

    let quarantined =
        QuarantinedLeases::read_bound(&runtime.ops, &snapshot.paths, &record.quarantined_leases)?;
    let stale = RecoveryStaleLeases {
        verdict: quarantined
            .verdict
            .as_ref()
            .map(|lease| lease.stale.clone()),
        intake: quarantined.intake.as_ref().map(|lease| lease.stale.clone()),
    };
    if recovery_source_command_class(record.mode, &stale)? != record.command_class {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let journal = discover_original_journal_with_terminal_residue(
        &runtime.ops,
        &snapshot.paths.transactions,
        Some(&recovery_journal.root),
        expected_published
            .then_some(record.original_transaction_id.as_deref())
            .flatten(),
    )?;
    if journal.root.is_some() {
        if expected_published && !journal.cleanup_form {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if journal.terminal_cleanup_residue {
            if !expected_published
                || record.mode != RecoveryMode::VerdictTransaction
                || record.original_transaction_id.as_deref() != journal.transaction_id()
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        } else if record.original_transaction_id.as_deref() != journal.transaction_id()
            || record.original_journal_hash != journal.commitment_hash()
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if journal.terminal_cleanup_residue {
            if stale.owner_hashes() != recorded_owner_hashes {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        } else {
            stale.validate_against(&journal)?;
        }
        if select_recovery_mode(snapshot, &stale, &journal, &ledger)? != record.mode {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    } else {
        if record.mode == RecoveryMode::VerdictTransaction
            && record.original_transaction_id.is_none()
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if !expected_published
            && !matches!(
                record.mode,
                RecoveryMode::VerdictTransaction | RecoveryMode::IntakeOnly
            )
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        stale.validate_against(&journal)?;
    }

    Ok(ValidatedRecoveryPrefix {
        ledger,
        recovery_lease,
        recovery_presence,
        quarantined,
        journal,
    })
}

fn finish_published_recovery_tail<O, F>(
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    recovery_journal: RecoveryJournal,
) -> Result<RecoveryOutcome, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let bindings = validate_recovery_prefix(profile, snapshot, runtime, &recovery_journal, true)?;
    let rolled_forward = published_recovery_rolled_forward(
        &recovery_journal.record,
        &bindings.journal,
        &bindings.ledger,
    )?;
    complete_recovery_tail(
        profile,
        snapshot,
        runtime,
        &recovery_journal,
        &bindings.journal,
        &bindings.quarantined,
        &bindings.recovery_lease,
    )?;
    Ok(RecoveryOutcome {
        rolled_forward,
        idempotent: false,
        mode: recovery_journal.record.mode,
    })
}

fn resume_recovery_cleanup_witness_tail<O, F>(
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    cleanup_witness: RecoveryJournal,
) -> Result<RecoveryOutcome, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let (ledger, recovery_lease) =
        validate_recovery_cleanup_witness(profile, snapshot, runtime, &cleanup_witness)?;
    let rolled_forward = cleanup_witness_rolled_forward(&cleanup_witness.record, &ledger)?;
    finish_recovery_cleanup_witness_tail(
        profile,
        snapshot,
        runtime,
        &cleanup_witness,
        &recovery_lease,
    )?;
    Ok(RecoveryOutcome {
        rolled_forward,
        idempotent: false,
        mode: cleanup_witness.record.mode,
    })
}

pub fn validate_recovery_cleanup_witness<O, F>(
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    cleanup_witness: &RecoveryJournal,
) -> Result<(super::LedgerView, Lease), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if !cleanup_witness.cleanup_form {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    cleanup_witness
        .verify(&runtime.ops)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if !cleanup_witness.is_published(&runtime.ops)?
        && !is_terminal_recovery_cleanup_residue(&runtime.ops, cleanup_witness)?
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }

    let record = &cleanup_witness.record;
    let recorded_owner_hashes = validate_recovery_record_identity(profile, snapshot, record)?;
    let ledger = read_ledger(&snapshot.paths.ledger).map_err(|_| TransactionError::Read)?;
    if ledger.final_hash != record.observed_ledger_head {
        return Err(TransactionError::RecoveryAmbiguous);
    }

    let recovery_owner = read_optional_stale_lease(
        &runtime.ops,
        &snapshot.paths.locks.join("verdict.lock"),
        RecoveryLeaseKind::Verdict,
    )?
    .ok_or(TransactionError::RecoveryAmbiguous)?;
    let mut recovery_owner_frames = recorded_owner_hashes
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    recovery_owner_frames.push(&record.cleanup_plan_hash);
    let expected_recovery_owner_id = lease_receipt_id(
        "recovery",
        &snapshot.policy.policy.store_id,
        Some(snapshot.head.as_str()),
        &profile.hash,
        &snapshot.policy.hash,
        &recovery_owner_frames,
    );
    if recovery_owner.owner.command_class() != "recovery"
        || recovery_owner.owner.owner_id() != expected_recovery_owner_id
        || recovery_owner.owner_hash != record.recovery_lease_owner_hash
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if read_optional_stale_lease(
        &runtime.ops,
        &snapshot.paths.locks.join("intake.lock"),
        RecoveryLeaseKind::Intake,
    )?
    .is_some()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let recovery_lease = Lease {
        root: snapshot.paths.root.clone(),
        kind: LeaseKind::Verdict,
        owner_hash: recovery_owner.owner_hash.clone(),
        lease_token: recovery_owner.owner.lease_token().to_owned(),
    };
    recovery_lease.verify_with(&runtime.ops)?;

    ensure_recovery_quarantines_absent(&snapshot.paths)?;
    if discover_original_journal_ignoring(
        &runtime.ops,
        &snapshot.paths.transactions,
        Some(&cleanup_witness.root),
    )?
    .root
    .is_some()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok((ledger, recovery_lease))
}

fn ensure_recovery_quarantines_absent(paths: &store::StorePaths) -> Result<(), TransactionError> {
    for parent in [&paths.locks, &paths.staging] {
        for entry in sorted_directory_entries(parent)? {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| TransactionError::RecoveryAmbiguous)?;
            if name.starts_with(".recovery-") {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }
    }
    Ok(())
}

fn cleanup_witness_rolled_forward(
    record: &RecoveryJournalRecord,
    ledger: &super::LedgerView,
) -> Result<bool, TransactionError> {
    if record.mode != RecoveryMode::VerdictTransaction {
        return Ok(false);
    }
    let transaction_id = record
        .original_transaction_id
        .as_deref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let committed = ledger
        .events
        .iter()
        .filter(|event| {
            matches!(event, LedgerEvent::Verdict(event) if event.payload().transaction_id == transaction_id)
        })
        .count();
    if committed > 1 {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(committed == 1)
}

fn published_recovery_rolled_forward(
    record: &RecoveryJournalRecord,
    journal: &OriginalJournal,
    ledger: &super::LedgerView,
) -> Result<bool, TransactionError> {
    if record.mode != RecoveryMode::VerdictTransaction {
        return Ok(false);
    }
    let transaction_id = journal
        .transaction_id()
        .or(record.original_transaction_id.as_deref())
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let committed = ledger
        .events
        .iter()
        .filter_map(|event| match event {
            LedgerEvent::Verdict(event) if event.payload().transaction_id == transaction_id => {
                Some(event.as_ref())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if committed.len() > 1 {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let Some(event) = committed.first() else {
        return Ok(false);
    };
    if let Some(prepared) = journal.prepared() {
        if prepared.verdict_id.as_deref() != Some(event.payload().verdict_id.as_str())
            || prepared.proposal_id.as_deref() != Some(event.payload().proposal_id.as_str())
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    } else if journal.root.is_some() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(true)
}

fn complete_recovery_tail<O, F>(
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    recovery_journal: &RecoveryJournal,
    journal: &OriginalJournal,
    quarantined: &QuarantinedLeases,
    recovery_lease: &Lease,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    recovery_journal.verify(&runtime.ops)?;
    journal.remove_cleanup_witness_with_ignoring_protected(
        &runtime.ops,
        &snapshot.paths.transactions,
        Some(&recovery_journal.root),
        recovery_lease,
    )?;
    runtime.faults.check(FaultPoint::AfterOriginalCleanup)?;
    quarantined.remove_stale_with(&runtime.ops, recovery_lease)?;
    let cleanup_witness =
        recovery_journal.promote_to_cleanup_witness_protected(&runtime.ops, recovery_lease)?;
    runtime
        .faults
        .check(FaultPoint::AfterRecoveryCleanupWitness)?;
    finish_recovery_cleanup_witness_tail(
        profile,
        snapshot,
        runtime,
        &cleanup_witness,
        recovery_lease,
    )
}

fn finish_recovery_cleanup_witness_tail<O, F>(
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    cleanup_witness: &RecoveryJournal,
    recovery_lease: &Lease,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    cleanup_witness.remove_cleanup_witness_with_protected(&runtime.ops, recovery_lease)?;
    if let Some(residue) =
        terminal_empty_recovery_cleanup_residue(&runtime.ops, &snapshot.paths.transactions)?
    {
        runtime
            .ops
            .sync_directory(&snapshot.paths.transactions)
            .map_err(|_| TransactionError::Write)?;
        recovery_lease.verify_with(&runtime.ops)?;
        runtime
            .ops
            .remove_dir(&residue)
            .map_err(|_| TransactionError::Write)?;
    }
    verify_recovery_store(profile, snapshot, recovery_lease)?;
    runtime
        .faults
        .check(FaultPoint::AfterRecoveryVerification)?;
    recovery_lease.release_terminal(&runtime.ops)
}

pub fn recovery_owner_hashes(bindings: &[RecoveryLeaseBinding]) -> Vec<String> {
    let mut hashes = bindings
        .iter()
        .map(|binding| binding.owner_hash.clone())
        .collect::<Vec<_>>();
    hashes.sort();
    hashes
}

fn verify_clean_recovery<O, F>(
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
) -> Result<RecoveryOutcome, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if store::has_public_transient_state(&snapshot.paths).map_err(|_| TransactionError::Read)? {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if snapshot.sealed {
        verify_store_level4(profile, &runtime.root).map_err(|_| TransactionError::Read)?;
    } else {
        store::inspect_store(profile, &runtime.root).map_err(|_| TransactionError::Read)?;
    }
    Ok(RecoveryOutcome {
        rolled_forward: false,
        idempotent: true,
        mode: RecoveryMode::CleanFinalization,
    })
}

fn require_exact_recovery_confirmation(
    human: &mut impl HumanPresence,
    owner_id: &str,
) -> Result<(), TransactionError> {
    if human.read_confirmation()? != format!("RECOVER {owner_id}\n") {
        return Err(TransactionError::ConfirmationFailed);
    }
    Ok(())
}

fn render_recovery_display(stale: &RecoveryStaleLeases) -> Vec<u8> {
    let mut display = b"SovMem recovery requires a verified stale owner.\n".to_vec();
    for owner in stale.owners() {
        display_field(&mut display, "owner_id", owner.owner.owner_id());
        display_field(&mut display, "owner_hash", &owner.owner_hash);
    }
    display.extend_from_slice(
        b"Terminate and verify absence of the prior process before continuing.\nType RECOVER <owner-id> exactly.\n",
    );
    display
}

#[derive(Clone, Debug)]
pub struct StaleLease {
    pub kind: RecoveryLeaseKind,
    pub owner: StaleLeaseOwner,
    pub owner_hash: String,
}

#[derive(Clone, Debug)]
pub enum StaleLeaseOwner {
    Verdict(LeaseOwner),
    Intake(super::intake::IntakeLeaseOwner),
}

impl StaleLeaseOwner {
    pub fn owner_id(&self) -> &str {
        match self {
            Self::Verdict(owner) => &owner.owner_id,
            Self::Intake(owner) => &owner.owner_id,
        }
    }

    pub fn command_class(&self) -> &str {
        match self {
            Self::Verdict(owner) => &owner.command_class,
            Self::Intake(owner) => &owner.command_class,
        }
    }

    fn lease_token(&self) -> &str {
        match self {
            Self::Verdict(owner) => &owner.lease_token,
            Self::Intake(owner) => &owner.lease_token,
        }
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, TransactionError> {
        match self {
            Self::Verdict(owner) => canonical_owner_bytes(owner),
            Self::Intake(owner) => owner
                .canonical_bytes()
                .map_err(|_| TransactionError::RecoveryAmbiguous),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RecoveryStaleLeases {
    pub verdict: Option<StaleLease>,
    pub intake: Option<StaleLease>,
}

impl RecoveryStaleLeases {
    pub fn read(
        ops: &impl FilesystemOps,
        paths: &store::StorePaths,
    ) -> Result<Option<Self>, TransactionError> {
        let verdict = read_optional_stale_lease(
            ops,
            &paths.locks.join("verdict.lock"),
            RecoveryLeaseKind::Verdict,
        )?;
        let intake = read_optional_stale_lease(
            ops,
            &paths.locks.join("intake.lock"),
            RecoveryLeaseKind::Intake,
        )?;
        let leases = Self { verdict, intake };
        Ok((leases.verdict.is_some() || leases.intake.is_some()).then_some(leases))
    }

    pub fn owners(&self) -> impl Iterator<Item = &StaleLease> {
        self.verdict.iter().chain(self.intake.iter())
    }

    pub fn confirmation_owner_id(&self) -> Result<&str, TransactionError> {
        let mut owners = self.owners();
        let first = owners.next().ok_or(TransactionError::RecoveryAmbiguous)?;
        if owners.any(|owner| owner.owner.owner_id() != first.owner.owner_id()) {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        Ok(first.owner.owner_id())
    }

    pub fn owner_hashes(&self) -> Vec<String> {
        let mut hashes = self
            .owners()
            .map(|owner| owner.owner_hash.clone())
            .collect::<Vec<_>>();
        hashes.sort();
        hashes
    }

    pub fn validate_against(&self, journal: &OriginalJournal) -> Result<(), TransactionError> {
        if journal.terminal_cleanup_residue || (journal.empty_root && journal.plan.is_none()) {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if let Some(plan) = journal.plan.as_ref() {
            if plan.plan.owner_hashes != self.owner_hashes() {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }
        if let Some(prepared) = journal.prepared() {
            if prepared.owner_hashes != self.owner_hashes() {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }
        Ok(())
    }

    fn quarantine_verdict<O, F>(
        &self,
        runtime: &TransactionRuntime<O, F>,
    ) -> Result<Option<QuarantinedLease>, TransactionError>
    where
        O: FilesystemOps,
        F: FaultInjector,
    {
        self.verdict
            .as_ref()
            .map(|lease| quarantine_stale_lease(&runtime.ops, &runtime.root, lease, None))
            .transpose()
    }

    pub fn quarantine_intake<O, F>(
        &self,
        runtime: &TransactionRuntime<O, F>,
        recovery_lease: &Lease,
    ) -> Result<Option<QuarantinedLease>, TransactionError>
    where
        O: FilesystemOps,
        F: FaultInjector,
    {
        self.intake
            .as_ref()
            .map(|lease| {
                quarantine_stale_lease(&runtime.ops, &runtime.root, lease, Some(recovery_lease))
            })
            .transpose()
    }
}

fn validate_recovery_stale_leases_against_journal<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    stale: &RecoveryStaleLeases,
    journal: &OriginalJournal,
    ledger: &super::LedgerView,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if !journal.terminal_cleanup_residue {
        return stale.validate_against(journal);
    }

    let (Some(verdict), Some(intake)) = (stale.verdict.as_ref(), stale.intake.as_ref()) else {
        return Err(TransactionError::RecoveryAmbiguous);
    };
    let transaction_id = journal
        .transaction_id()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let mut committed = ledger.events.iter().filter_map(|event| match event {
        LedgerEvent::Verdict(event) if event.payload().transaction_id == transaction_id => {
            Some(event.as_ref())
        }
        _ => None,
    });
    let event = committed
        .next()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    if committed.next().is_some() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    validate_terminal_empty_event_state(snapshot, profile, runtime, event)?;

    let payload = event.payload();
    let observed_head = match (
        payload.base_kind.as_str(),
        payload.base_generation.as_deref(),
    ) {
        ("seed", None) => "seed",
        ("generation", Some(generation_id)) if valid_generation_id(generation_id) => generation_id,
        _ => return Err(TransactionError::RecoveryAmbiguous),
    };
    let expected_owner_id = lease_receipt_id(
        "verdict",
        &snapshot.policy.policy.store_id,
        Some(observed_head),
        &profile.hash,
        &snapshot.policy.hash,
        &[&payload.proposal_id],
    );
    if verdict.owner.command_class() != "verdict"
        || intake.owner.command_class() != "verdict"
        || verdict.owner.owner_id() != expected_owner_id
        || intake.owner.owner_id() != expected_owner_id
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct RecoveryBootstrap {
    recovery_lease: Lease,
    originals: RecoveryStaleLeases,
    quarantined: QuarantinedLeases,
}

impl RecoveryBootstrap {
    fn matches(&self, other: &Self) -> bool {
        self.recovery_lease.owner_hash == other.recovery_lease.owner_hash
            && self.recovery_lease.lease_token == other.recovery_lease.lease_token
            && self.originals.owner_hashes() == other.originals.owner_hashes()
            && self.quarantined.bindings() == other.quarantined.bindings()
    }
}

fn in_progress_recovery_bootstrap<O, F>(
    runtime: &TransactionRuntime<O, F>,
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    recovery_presence: &RecoveryStaleLeases,
) -> Result<Option<RecoveryBootstrap>, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let ops = &runtime.ops;
    let Some(recovery_owner) = recovery_presence.verdict.as_ref() else {
        return Ok(None);
    };
    if recovery_owner.owner.command_class() != "recovery" {
        return Ok(None);
    }

    let mut bindings = Vec::new();
    for entry in sorted_directory_entries(&snapshot.paths.locks)? {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if !name.starts_with(".recovery-") {
            continue;
        }
        let (kind, owner_hash) = [RecoveryLeaseKind::Verdict, RecoveryLeaseKind::Intake]
            .into_iter()
            .find_map(|kind| {
                name.strip_prefix(&format!(".recovery-{}-", kind.name()))
                    .map(|hash| (kind, hash))
            })
            .ok_or(TransactionError::RecoveryAmbiguous)?;
        if !valid_hash(owner_hash) {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        bindings.push(RecoveryLeaseBinding {
            kind,
            owner_hash: owner_hash.to_owned(),
        });
    }
    bindings.sort();
    if bindings.is_empty() {
        return Ok(None);
    }
    if bindings.windows(2).any(|pair| pair[0].kind == pair[1].kind)
        || (recovery_presence.intake.is_some()
            && bindings
                .iter()
                .any(|binding| binding.kind == RecoveryLeaseKind::Intake))
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let quarantined = QuarantinedLeases::read_bound(ops, &snapshot.paths, &bindings)?;
    let originals = RecoveryStaleLeases {
        verdict: quarantined
            .verdict
            .as_ref()
            .map(|lease| lease.stale.clone()),
        intake: quarantined
            .intake
            .as_ref()
            .map(|lease| lease.stale.clone())
            .or_else(|| recovery_presence.intake.clone()),
    };
    let owner_hashes = originals.owner_hashes();
    if owner_hashes.is_empty() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let recovery_lease = Lease {
        root: snapshot.paths.root.clone(),
        kind: LeaseKind::Verdict,
        owner_hash: recovery_owner.owner_hash.clone(),
        lease_token: recovery_owner.owner.lease_token().to_owned(),
    };
    let journal = discover_original_journal(ops, &snapshot.paths.transactions).or_else(|_| {
        discover_unbound_terminal_original_cleanup_residue(ops, &snapshot.paths.transactions)
    })?;
    let ledger = read_ledger(&snapshot.paths.ledger).map_err(|_| TransactionError::Read)?;
    validate_recovery_stale_leases_against_journal(
        snapshot, profile, runtime, &originals, &journal, &ledger,
    )?;
    let mode = select_recovery_mode(snapshot, &originals, &journal, &ledger)?;
    let cleanup_plan = match mode {
        RecoveryMode::VerdictTransaction => {
            prepare_recovery_cleanup_plan(snapshot, profile, runtime, &journal, &ledger)?
        }
        RecoveryMode::IntakeOnly => prepare_intake_cleanup_plan(
            ops,
            &snapshot.paths,
            originals
                .intake
                .as_ref()
                .ok_or(TransactionError::RecoveryAmbiguous)?,
        )?,
        RecoveryMode::CleanFinalization => RecoveryCleanupPlan::empty(),
    };
    let cleanup_plan_hash = cleanup_plan.hash()?;
    let mut owner_frames = owner_hashes.iter().map(String::as_str).collect::<Vec<_>>();
    owner_frames.push(&cleanup_plan_hash);
    let expected_owner_id = lease_receipt_id(
        "recovery",
        &snapshot.policy.policy.store_id,
        Some(snapshot.head.as_str()),
        &profile.hash,
        &snapshot.policy.hash,
        &owner_frames,
    );
    recovery_lease.verify_with(ops)?;
    if recovery_owner.owner.owner_id() != expected_owner_id {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    recovery_lease.verify_with(ops)?;
    Ok(Some(RecoveryBootstrap {
        recovery_lease,
        originals,
        quarantined,
    }))
}

pub fn read_optional_stale_lease(
    ops: &impl FilesystemOps,
    root: &Path,
    kind: RecoveryLeaseKind,
) -> Result<Option<StaleLease>, TransactionError> {
    match ops.metadata(root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(TransactionError::RecoveryAmbiguous)
        }
        Ok(_) => {
            let owner = read_stale_lease_owner(ops, &root.join("owner.json"), kind)?;
            let owner_bytes = owner.canonical_bytes()?;
            Ok(Some(StaleLease {
                kind,
                owner,
                owner_hash: blake3::hash(&owner_bytes).to_hex().to_string(),
            }))
        }
        Err(_) => Err(TransactionError::Read),
    }
}

fn read_stale_lease_owner(
    ops: &impl FilesystemOps,
    path: &Path,
    kind: RecoveryLeaseKind,
) -> Result<StaleLeaseOwner, TransactionError> {
    let metadata = ops
        .metadata(path)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    match kind {
        RecoveryLeaseKind::Verdict => read_canonical_owner(ops, path, LeaseKind::Verdict)
            .map(StaleLeaseOwner::Verdict)
            .map_err(|_| TransactionError::RecoveryAmbiguous),
        RecoveryLeaseKind::Intake => {
            let bytes = ops
                .read(path)
                .map_err(|_| TransactionError::RecoveryAmbiguous)?;
            super::intake::IntakeLeaseOwner::parse(&bytes)
                .map(StaleLeaseOwner::Intake)
                .map_err(|_| TransactionError::RecoveryAmbiguous)
        }
    }
}

#[derive(Clone, Debug)]
pub struct QuarantinedLease {
    pub stale: StaleLease,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct QuarantinedLeases {
    pub verdict: Option<QuarantinedLease>,
    pub intake: Option<QuarantinedLease>,
}

impl QuarantinedLeases {
    fn read_bound(
        ops: &impl FilesystemOps,
        paths: &store::StorePaths,
        expected: &[RecoveryLeaseBinding],
    ) -> Result<Self, TransactionError> {
        let mut verdict = None;
        let mut intake = None;
        for binding in expected {
            let slot = match binding.kind {
                RecoveryLeaseKind::Verdict => &mut verdict,
                RecoveryLeaseKind::Intake => &mut intake,
            };
            let path = paths.locks.join(format!(
                ".recovery-{}-{}",
                binding.kind.name(),
                binding.owner_hash
            ));
            let metadata = ops
                .metadata(&path)
                .map_err(|_| TransactionError::RecoveryAmbiguous)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() || slot.is_some() {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            let stale = read_optional_stale_lease(ops, &path, binding.kind)?
                .ok_or(TransactionError::RecoveryAmbiguous)?;
            let owner_bytes = stale.owner.canonical_bytes()?;
            if blake3::hash(&owner_bytes).to_hex().as_str() != binding.owner_hash {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            *slot = Some(QuarantinedLease { stale, path });
        }
        let quarantined = Self { verdict, intake };
        if quarantined.bindings() != expected {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        for entry in sorted_directory_entries(&paths.locks)? {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| TransactionError::RecoveryAmbiguous)?;
            if !name.starts_with(".recovery-") {
                continue;
            }
            let (kind, hash) = [RecoveryLeaseKind::Verdict, RecoveryLeaseKind::Intake]
                .into_iter()
                .find_map(|kind| {
                    name.strip_prefix(&format!(".recovery-{}-", kind.name()))
                        .map(|hash| (kind, hash))
                })
                .ok_or(TransactionError::RecoveryAmbiguous)?;
            let binding = RecoveryLeaseBinding {
                kind,
                owner_hash: hash.to_owned(),
            };
            if !binding.validates() || !expected.contains(&binding) {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }
        Ok(quarantined)
    }

    fn bindings(&self) -> Vec<RecoveryLeaseBinding> {
        let mut bindings = self
            .verdict
            .iter()
            .chain(self.intake.iter())
            .map(|lease| RecoveryLeaseBinding {
                kind: lease.stale.kind,
                owner_hash: lease.stale.owner_hash.clone(),
            })
            .collect::<Vec<_>>();
        bindings.sort();
        bindings
    }

    pub fn owner_hashes(&self) -> Vec<String> {
        let mut hashes = self
            .verdict
            .iter()
            .chain(self.intake.iter())
            .map(|lease| lease.stale.owner_hash.clone())
            .collect::<Vec<_>>();
        hashes.sort();
        hashes
    }

    pub fn remove_stale_with(
        &self,
        ops: &impl FilesystemOps,
        recovery_lease: &Lease,
    ) -> Result<(), TransactionError> {
        for lease in [&self.intake, &self.verdict].into_iter().flatten() {
            let owner =
                read_stale_lease_owner(ops, &lease.path.join("owner.json"), lease.stale.kind)?;
            let bytes = owner.canonical_bytes()?;
            if blake3::hash(&bytes).to_hex().as_str() != lease.stale.owner_hash {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            recovery_lease.verify_with(ops)?;
            ops.remove_file(&lease.path.join("owner.json"))
                .map_err(|_| TransactionError::Write)?;
            recovery_lease.verify_with(ops)?;
            ops.remove_dir(&lease.path)
                .map_err(|_| TransactionError::Write)?;
            let parent = lease.path.parent().ok_or(TransactionError::Write)?;
            ops.sync_directory(parent)
                .map_err(|_| TransactionError::Write)?;
        }
        Ok(())
    }
}

pub fn quarantine_stale_lease(
    ops: &impl FilesystemOps,
    root: &Path,
    stale: &StaleLease,
    recovery_lease: Option<&Lease>,
) -> Result<QuarantinedLease, TransactionError> {
    let parent = root.join("locks");
    let source = match stale.kind {
        RecoveryLeaseKind::Verdict => parent.join("verdict.lock"),
        RecoveryLeaseKind::Intake => parent.join("intake.lock"),
    };
    let destination = parent.join(format!(
        ".recovery-{}-{}",
        stale.kind.name(),
        stale.owner_hash
    ));
    match ops.metadata(&destination) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        _ => return Err(TransactionError::RecoveryAmbiguous),
    }
    if let Some(lease) = recovery_lease {
        lease.verify_with(ops)?;
    }
    ops.rename(&source, &destination)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    ops.sync_directory(&parent)
        .map_err(|_| TransactionError::Write)?;
    Ok(QuarantinedLease {
        stale: stale.clone(),
        path: destination,
    })
}

#[derive(Clone, Debug)]
pub struct OriginalJournal {
    pub root: Option<PathBuf>,
    pub plan_preparation: Option<PathBuf>,
    pub transaction_id: Option<String>,
    pub plan: Option<TransactionPlanJournal>,
    pub prepared: Option<TransactionJournal>,
    pub files: Vec<String>,
    pub cleanup_form: bool,
    pub empty_root: bool,
    pub terminal_cleanup_residue: bool,
    pub commitment_hash: Option<String>,
}

const ORIGINAL_CLEANUP_LINK_ORDER: [&str; 4] = [
    "plan.json",
    "prepared.json",
    "ledger-committed.json",
    "published.json",
];
const ORIGINAL_CLEANUP_TERMINAL_DELETE_ORDER: [&str; 4] = [
    "published.json",
    "ledger-committed.json",
    "prepared.json",
    "plan.json",
];

impl OriginalJournal {
    fn none() -> Self {
        Self {
            root: None,
            plan_preparation: None,
            transaction_id: None,
            plan: None,
            prepared: None,
            files: Vec::new(),
            cleanup_form: false,
            empty_root: false,
            terminal_cleanup_residue: false,
            commitment_hash: None,
        }
    }

    fn empty_root(root: PathBuf, transaction_id: String) -> Self {
        Self {
            root: Some(root),
            plan_preparation: None,
            transaction_id: Some(transaction_id),
            plan: None,
            prepared: None,
            files: Vec::new(),
            cleanup_form: false,
            empty_root: true,
            terminal_cleanup_residue: false,
            commitment_hash: None,
        }
    }

    fn terminal_cleanup_residue(root: PathBuf, transaction_id: String) -> Self {
        Self {
            root: Some(root),
            plan_preparation: None,
            transaction_id: Some(transaction_id),
            plan: None,
            prepared: None,
            files: Vec::new(),
            cleanup_form: true,
            empty_root: false,
            terminal_cleanup_residue: true,
            commitment_hash: None,
        }
    }

    pub fn transaction_id(&self) -> Option<&str> {
        self.transaction_id.as_deref()
    }

    pub fn prepared(&self) -> Option<&PreparedJournal> {
        self.prepared.as_ref().map(|journal| &journal.prepared)
    }

    pub fn commitment_hash(&self) -> Option<String> {
        self.commitment_hash.clone()
    }

    pub fn promote_to_cleanup_witness(
        &self,
        ops: &impl FilesystemOps,
    ) -> Result<Self, TransactionError> {
        self.promote_to_cleanup_witness_ignoring(ops, None)
    }

    fn promote_to_cleanup_witness_ignoring(
        &self,
        ops: &impl FilesystemOps,
        ignored_root: Option<&Path>,
    ) -> Result<Self, TransactionError> {
        self.promote_to_cleanup_witness_ignoring_with_lease(ops, ignored_root, None)
    }

    pub fn promote_to_cleanup_witness_ignoring_protected(
        &self,
        ops: &impl FilesystemOps,
        ignored_root: Option<&Path>,
        recovery_lease: &Lease,
    ) -> Result<Self, TransactionError> {
        self.promote_to_cleanup_witness_ignoring_with_lease(ops, ignored_root, Some(recovery_lease))
    }

    fn promote_to_cleanup_witness_ignoring_with_lease(
        &self,
        ops: &impl FilesystemOps,
        ignored_root: Option<&Path>,
        recovery_lease: Option<&Lease>,
    ) -> Result<Self, TransactionError> {
        let Some(root) = &self.root else {
            return Ok(Self::none());
        };
        let transaction_id = self
            .transaction_id
            .as_deref()
            .ok_or(TransactionError::RecoveryAmbiguous)?;
        let parent = root.parent().ok_or(TransactionError::Write)?;
        let current = discover_original_journal_ignoring(ops, parent, ignored_root)?;
        if current.files != self.files || current.commitment_hash() != self.commitment_hash() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if current.empty_root {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if current.terminal_cleanup_residue {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if current.cleanup_form {
            return finish_original_source_removal(ops, &current, recovery_lease);
        }

        let cleanup = parent.join(format!(".cleanup-{transaction_id}"));
        match ops.metadata(&cleanup) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Some(lease) = recovery_lease {
                    lease.verify_with(ops)?;
                }
                ops.create_dir(&cleanup)
                    .map_err(|_| TransactionError::RecoveryAmbiguous)?;
                ops.sync_directory(parent)
                    .map_err(|_| TransactionError::Write)?;
            }
            Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {}
            _ => return Err(TransactionError::RecoveryAmbiguous),
        }
        for name in original_cleanup_link_order(&current.files)? {
            let source = root.join(name);
            let destination = cleanup.join(name);
            match ops.metadata(&destination) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    if let Some(lease) = recovery_lease {
                        lease.verify_with(ops)?;
                    }
                    ops.hard_link(&source, &destination)
                        .map_err(|_| TransactionError::Write)?
                }
                Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_file() => {
                    if read_original_journal_regular_file(ops, &source)?
                        != read_original_journal_regular_file(ops, &destination)?
                    {
                        return Err(TransactionError::RecoveryAmbiguous);
                    }
                }
                _ => return Err(TransactionError::RecoveryAmbiguous),
            }
        }
        ops.sync_directory(&cleanup)
            .map_err(|_| TransactionError::Write)?;
        ops.sync_directory(parent)
            .map_err(|_| TransactionError::Write)?;

        for name in original_cleanup_link_order(&current.files)? {
            let source = root.join(name);
            if read_original_journal_regular_file(ops, &source)?
                != read_original_journal_regular_file(ops, &cleanup.join(name))?
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            if let Some(lease) = recovery_lease {
                lease.verify_with(ops)?;
            }
            ops.remove_file(&source)
                .map_err(|_| TransactionError::Write)?;
        }
        if let Some(lease) = recovery_lease {
            lease.verify_with(ops)?;
        }
        ops.remove_dir(root).map_err(|_| TransactionError::Write)?;
        ops.sync_directory(parent)
            .map_err(|_| TransactionError::Write)?;
        read_original_journal_exact(ops, cleanup, transaction_id.to_owned(), true)
    }

    pub fn remove_cleanup_witness_with(
        &self,
        ops: &impl FilesystemOps,
        transactions: &Path,
    ) -> Result<(), TransactionError> {
        self.remove_cleanup_witness_with_ignoring(ops, transactions, None)
    }

    fn remove_cleanup_witness_with_ignoring(
        &self,
        ops: &impl FilesystemOps,
        transactions: &Path,
        ignored_root: Option<&Path>,
    ) -> Result<(), TransactionError> {
        self.remove_cleanup_witness_with_ignoring_with_lease(ops, transactions, ignored_root, None)
    }

    fn remove_cleanup_witness_with_ignoring_protected(
        &self,
        ops: &impl FilesystemOps,
        transactions: &Path,
        ignored_root: Option<&Path>,
        recovery_lease: &Lease,
    ) -> Result<(), TransactionError> {
        self.remove_cleanup_witness_with_ignoring_with_lease(
            ops,
            transactions,
            ignored_root,
            Some(recovery_lease),
        )
    }

    fn remove_cleanup_witness_with_ignoring_with_lease(
        &self,
        ops: &impl FilesystemOps,
        transactions: &Path,
        ignored_root: Option<&Path>,
        recovery_lease: Option<&Lease>,
    ) -> Result<(), TransactionError> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        if !self.cleanup_form {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if self.terminal_cleanup_residue {
            // This form is constructed only by the published recovery-tail validator,
            // after the recovery record and protected lease have been authenticated.
            ops.sync_directory(transactions)
                .map_err(|_| TransactionError::Write)?;
            if let Some(lease) = recovery_lease {
                lease.verify_with(ops)?;
            }
            return ops.remove_dir(root).map_err(|_| TransactionError::Write);
        }
        let current = discover_original_journal_ignoring(ops, transactions, ignored_root)?;
        if current.files != self.files || current.commitment_hash() != self.commitment_hash() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        let witness = if current.cleanup_form {
            finish_original_source_removal(ops, &current, recovery_lease)?
        } else {
            current.promote_to_cleanup_witness_ignoring_with_lease(
                ops,
                ignored_root,
                recovery_lease,
            )?
        };
        remove_original_cleanup_witness_terminally(ops, &witness, transactions, recovery_lease)
    }
}

pub fn discover_original_journal(
    ops: &impl FilesystemOps,
    transactions: &Path,
) -> Result<OriginalJournal, TransactionError> {
    discover_original_journal_ignoring(ops, transactions, None)
}

pub fn discover_original_journal_ignoring(
    ops: &impl FilesystemOps,
    transactions: &Path,
    ignored_root: Option<&Path>,
) -> Result<OriginalJournal, TransactionError> {
    discover_original_journal_with_terminal_residue(ops, transactions, ignored_root, None)
}

fn same_regular_directory_identity(entry: &Path, ignored_root: &Path) -> bool {
    entry == ignored_root
        || matches!(
            (fs::canonicalize(entry), fs::canonicalize(ignored_root)),
            (Ok(entry), Ok(ignored_root)) if entry == ignored_root
        )
}

fn discover_original_journal_with_terminal_residue(
    ops: &impl FilesystemOps,
    transactions: &Path,
    ignored_root: Option<&Path>,
    terminal_residue_id: Option<&str>,
) -> Result<OriginalJournal, TransactionError> {
    if let Some(root) = ignored_root {
        let metadata = ops
            .metadata(root)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    let mut regular = Vec::new();
    let mut cleanup = Vec::new();
    let mut plan_preparations = Vec::new();
    for entry in sorted_directory_entries(transactions)? {
        let entry_path = entry.path();
        let metadata = ops
            .metadata(&entry_path)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if let Some(id) = plan_preparation_id(&name) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            plan_preparations.push((id.to_owned(), entry_path));
            continue;
        }
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if ignored_root.is_some_and(|root| same_regular_directory_identity(&entry_path, root)) {
            continue;
        }
        if valid_transaction_id(&name) {
            regular.push((name, entry_path));
        } else if let Some(id) = name.strip_prefix(".cleanup-") {
            if valid_transaction_id(id) {
                cleanup.push((id.to_owned(), entry_path));
            } else {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        } else {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    if plan_preparations.len() > 1 {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if let Some((transaction_id, prep)) = plan_preparations.pop() {
        if !cleanup.is_empty() || regular.len() > 1 {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        let regular_root = match regular.pop() {
            Some((regular_id, root)) if regular_id == transaction_id => Some(root),
            Some(_) => return Err(TransactionError::RecoveryAmbiguous),
            None => None,
        };
        return read_plan_publication_prefix(ops, transactions, transaction_id, prep, regular_root);
    }
    if regular.is_empty() && cleanup.is_empty() {
        return Ok(OriginalJournal::none());
    }
    if regular.len() > 1 || cleanup.len() > 1 {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    match (regular.pop(), cleanup.pop()) {
        (None, None) => Ok(OriginalJournal::none()),
        (Some((transaction_id, root)), None) => {
            read_original_journal_exact(ops, root, transaction_id, false)
        }
        (None, Some((transaction_id, root))) => {
            let names = original_journal_entry_names(ops, &root)?;
            if names.is_empty() {
                if terminal_residue_id == Some(transaction_id.as_str()) {
                    return Ok(OriginalJournal::terminal_cleanup_residue(
                        root,
                        transaction_id,
                    ));
                }
                return Err(TransactionError::RecoveryAmbiguous);
            }
            read_original_journal_exact(ops, root, transaction_id, true)
        }
        (Some((regular_id, regular_root)), Some((cleanup_id, cleanup_root))) => {
            if regular_id != cleanup_id {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            read_original_journal_transition(ops, regular_root, cleanup_root, regular_id)
        }
    }
}

fn plan_preparation_id(name: &str) -> Option<&str> {
    name.strip_prefix(".plan-")
        .and_then(|name| name.strip_suffix(".json"))
        .filter(|id| valid_transaction_id(id))
}

fn read_plan_publication_prefix(
    ops: &impl FilesystemOps,
    transactions: &Path,
    transaction_id: String,
    prep: PathBuf,
    regular_root: Option<PathBuf>,
) -> Result<OriginalJournal, TransactionError> {
    let bytes = read_original_journal_regular_file(ops, &prep)?;
    let plan: TransactionPlan =
        serde_json::from_slice(&bytes).map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if plan.transaction_id != transaction_id || plan.plan_hash(&bytes).is_err() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let root = regular_root.unwrap_or_else(|| transactions.join(&transaction_id));
    let mut empty_root = false;
    match ops.metadata(&root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {
            let names = original_journal_entry_names(ops, &root)?;
            match names.as_slice() {
                [] => empty_root = true,
                [name] if name == "plan.json" => {
                    let final_path = root.join("plan.json");
                    if read_original_journal_regular_file(ops, &final_path)? != bytes
                        || !same_regular_file_identity(ops, &prep, &final_path)?
                    {
                        return Err(TransactionError::RecoveryAmbiguous);
                    }
                }
                _ => return Err(TransactionError::RecoveryAmbiguous),
            }
        }
        _ => return Err(TransactionError::RecoveryAmbiguous),
    }
    let plan = TransactionPlanJournal {
        root: root.clone(),
        path: prep.clone(),
        plan,
        bytes,
    };
    plan.verify(ops)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    Ok(OriginalJournal {
        root: Some(root),
        plan_preparation: Some(prep),
        transaction_id: Some(transaction_id),
        plan: Some(plan),
        prepared: None,
        files: vec!["plan.json".to_owned()],
        cleanup_form: false,
        empty_root,
        terminal_cleanup_residue: false,
        commitment_hash: None,
    })
}

#[cfg(unix)]
fn same_regular_file_identity(
    ops: &impl FilesystemOps,
    left: &Path,
    right: &Path,
) -> Result<bool, TransactionError> {
    use std::os::unix::fs::MetadataExt;
    let left = ops
        .metadata(left)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let right = ops
        .metadata(right)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    Ok(left.is_file() && right.is_file() && left.dev() == right.dev() && left.ino() == right.ino())
}

#[cfg(not(unix))]
fn same_regular_file_identity(
    _ops: &impl FilesystemOps,
    _left: &Path,
    _right: &Path,
) -> Result<bool, TransactionError> {
    Ok(false)
}

fn discover_unbound_terminal_original_cleanup_residue(
    ops: &impl FilesystemOps,
    transactions: &Path,
) -> Result<OriginalJournal, TransactionError> {
    let entries = sorted_directory_entries(transactions)?;
    if entries.len() != 1 {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let entry = entries.into_iter().next().expect("checked one entry");
    let name = entry
        .file_name()
        .into_string()
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let transaction_id = name
        .strip_prefix(".cleanup-")
        .filter(|id| valid_transaction_id(id))
        .ok_or(TransactionError::RecoveryAmbiguous)?
        .to_owned();
    let metadata = ops
        .metadata(&entry.path())
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || !sorted_directory_entries(&entry.path())?.is_empty()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(OriginalJournal::terminal_cleanup_residue(
        entry.path(),
        transaction_id,
    ))
}

pub fn read_original_journal_exact(
    ops: &impl FilesystemOps,
    root: PathBuf,
    transaction_id: String,
    cleanup_form: bool,
) -> Result<OriginalJournal, TransactionError> {
    let names = original_journal_entry_names(ops, &root)?;
    let plan_only = ["plan.json"];
    let prepared_prefix = ["plan.json", "prepared.json"];
    let committed_prefix = ["ledger-committed.json", "plan.json", "prepared.json"];
    let published_prefix = [
        "ledger-committed.json",
        "plan.json",
        "prepared.json",
        "published.json",
    ];
    if names.is_empty() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if names != plan_only
        && names != prepared_prefix
        && names != committed_prefix
        && names != published_prefix
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let plan_bytes = ops
        .read(&root.join("plan.json"))
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let plan: TransactionPlan =
        serde_json::from_slice(&plan_bytes).map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if plan.transaction_id != transaction_id || plan.plan_hash(&plan_bytes).is_err() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let plan = TransactionPlanJournal {
        root: root.clone(),
        path: root.join("plan.json"),
        plan,
        bytes: plan_bytes,
    };
    let commitment_hash = original_journal_commitment(ops, &root, &names)?;
    plan.verify(ops)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if names == plan_only {
        return Ok(OriginalJournal {
            root: Some(root),
            plan_preparation: None,
            transaction_id: Some(transaction_id),
            plan: Some(plan),
            prepared: None,
            files: names,
            cleanup_form,
            empty_root: false,
            terminal_cleanup_residue: false,
            commitment_hash: Some(commitment_hash),
        });
    }
    let prepared_bytes = ops
        .read(&root.join("prepared.json"))
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let prepared: PreparedJournal =
        serde_json::from_slice(&prepared_bytes).map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if prepared.schema_version != JOURNAL_SCHEMA
        || prepared.command_class != "verdict"
        || prepared.transaction_id != transaction_id
        || canonical_json_bytes(&prepared)? != prepared_bytes
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let journal = TransactionJournal {
        root: root.clone(),
        plan: plan.clone(),
        prepared,
        bytes: prepared_bytes,
    };
    journal
        .verify_prepared(ops)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if names == committed_prefix || names == published_prefix {
        journal
            .verify_marker(ops, "ledger-committed.json", "ledger-committed")
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    }
    if names == published_prefix {
        journal
            .verify_marker(ops, "published.json", "published")
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    }
    Ok(OriginalJournal {
        root: Some(root),
        plan_preparation: None,
        transaction_id: Some(transaction_id),
        plan: Some(plan),
        prepared: Some(journal),
        files: names,
        cleanup_form,
        empty_root: false,
        terminal_cleanup_residue: false,
        commitment_hash: Some(commitment_hash),
    })
}

fn original_journal_entry_names(
    ops: &impl FilesystemOps,
    root: &Path,
) -> Result<Vec<String>, TransactionError> {
    let metadata = ops
        .metadata(root)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let names = sorted_directory_entries(root)?
        .into_iter()
        .map(|entry| {
            entry
                .file_name()
                .into_string()
                .map_err(|_| TransactionError::RecoveryAmbiguous)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for name in &names {
        let child = ops
            .metadata(&root.join(name))
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if child.file_type().is_symlink() || !child.is_file() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    Ok(names)
}

fn original_cleanup_link_order(files: &[String]) -> Result<Vec<&'static str>, TransactionError> {
    if files
        .iter()
        .any(|name| !ORIGINAL_CLEANUP_LINK_ORDER.contains(&name.as_str()))
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(ORIGINAL_CLEANUP_LINK_ORDER
        .into_iter()
        .filter(|name| files.iter().any(|file| file == name))
        .collect())
}

fn original_cleanup_names_match(actual: &[String], expected_ordered: &[&str]) -> bool {
    let mut expected = expected_ordered
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    expected.sort();
    actual == expected
}

fn original_cleanup_is_link_prefix(cleanup: &[String], full: &[String]) -> bool {
    if cleanup.is_empty() {
        return false;
    }
    let Ok(order) = original_cleanup_link_order(full) else {
        return false;
    };
    cleanup.len() <= order.len() && original_cleanup_names_match(cleanup, &order[..cleanup.len()])
}

fn original_cleanup_is_remaining_suffix(regular: &[String], full: &[String]) -> bool {
    let Ok(order) = original_cleanup_link_order(full) else {
        return false;
    };
    regular.len() <= order.len()
        && original_cleanup_names_match(regular, &order[order.len() - regular.len()..])
}

fn assert_original_counterpart_bytes(
    ops: &impl FilesystemOps,
    first_root: &Path,
    first_names: &[String],
    second_root: &Path,
    second_names: &[String],
) -> Result<(), TransactionError> {
    for name in first_names {
        if second_names.iter().any(|candidate| candidate == name)
            && read_original_journal_regular_file(ops, &first_root.join(name))?
                != read_original_journal_regular_file(ops, &second_root.join(name))?
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    Ok(())
}

fn read_original_journal_transition(
    ops: &impl FilesystemOps,
    regular_root: PathBuf,
    cleanup_root: PathBuf,
    transaction_id: String,
) -> Result<OriginalJournal, TransactionError> {
    let regular_names = original_journal_entry_names(ops, &regular_root)?;
    let cleanup_names = original_journal_entry_names(ops, &cleanup_root)?;
    if let Ok(regular) =
        read_original_journal_exact(ops, regular_root.clone(), transaction_id.clone(), false)
    {
        if !regular.empty_root {
            let authenticated_empty_prefix = cleanup_names.is_empty() && !regular.files.is_empty();
            if !authenticated_empty_prefix
                && !original_cleanup_is_link_prefix(&cleanup_names, &regular.files)
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            if !cleanup_names.is_empty() {
                let cleanup =
                    read_original_journal_exact(ops, cleanup_root.clone(), transaction_id, true)?;
                if cleanup.files != cleanup_names {
                    return Err(TransactionError::RecoveryAmbiguous);
                }
            }
            assert_original_counterpart_bytes(
                ops,
                &regular_root,
                &regular_names,
                &cleanup_root,
                &cleanup_names,
            )?;
            return Ok(regular);
        }
    }

    let cleanup = read_original_journal_exact(ops, cleanup_root.clone(), transaction_id, true)?;
    if !original_cleanup_is_remaining_suffix(&regular_names, &cleanup.files) {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    assert_original_counterpart_bytes(
        ops,
        &cleanup_root,
        &cleanup.files,
        &regular_root,
        &regular_names,
    )?;
    Ok(cleanup)
}

fn finish_original_source_removal(
    ops: &impl FilesystemOps,
    cleanup_witness: &OriginalJournal,
    recovery_lease: Option<&Lease>,
) -> Result<OriginalJournal, TransactionError> {
    if !cleanup_witness.cleanup_form || cleanup_witness.terminal_cleanup_residue {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let root = cleanup_witness
        .root
        .as_deref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let transactions = root.parent().ok_or(TransactionError::Write)?;
    let transaction_id = cleanup_witness
        .transaction_id()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let regular = transactions.join(transaction_id);
    match ops.metadata(&regular) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return read_original_journal_exact(
                ops,
                root.to_owned(),
                transaction_id.to_owned(),
                true,
            )
        }
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {}
        _ => return Err(TransactionError::RecoveryAmbiguous),
    }
    let regular_names = original_journal_entry_names(ops, &regular)?;
    if !original_cleanup_is_remaining_suffix(&regular_names, &cleanup_witness.files) {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    assert_original_counterpart_bytes(ops, root, &cleanup_witness.files, &regular, &regular_names)?;
    for name in original_cleanup_link_order(&cleanup_witness.files)? {
        if regular_names.iter().any(|candidate| candidate == name) {
            let source = regular.join(name);
            if read_original_journal_regular_file(ops, &source)?
                != read_original_journal_regular_file(ops, &root.join(name))?
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            if let Some(lease) = recovery_lease {
                lease.verify_with(ops)?;
            }
            ops.remove_file(&source)
                .map_err(|_| TransactionError::Write)?;
        }
    }
    if let Some(lease) = recovery_lease {
        lease.verify_with(ops)?;
    }
    ops.remove_dir(&regular)
        .map_err(|_| TransactionError::Write)?;
    ops.sync_directory(transactions)
        .map_err(|_| TransactionError::Write)?;
    read_original_journal_exact(ops, root.to_owned(), transaction_id.to_owned(), true)
}

fn remove_original_cleanup_witness_terminally(
    ops: &impl FilesystemOps,
    cleanup_witness: &OriginalJournal,
    transactions: &Path,
    recovery_lease: Option<&Lease>,
) -> Result<(), TransactionError> {
    if !cleanup_witness.cleanup_form || cleanup_witness.terminal_cleanup_residue {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let root = cleanup_witness
        .root
        .as_deref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let transaction_id = cleanup_witness
        .transaction_id()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    ops.sync_directory(transactions)
        .map_err(|_| TransactionError::Write)?;
    for name in ORIGINAL_CLEANUP_TERMINAL_DELETE_ORDER {
        let current =
            read_original_journal_exact(ops, root.to_owned(), transaction_id.to_owned(), true)?;
        if current.files.iter().any(|candidate| candidate == name) {
            if let Some(lease) = recovery_lease {
                lease.verify_with(ops)?;
            }
            ops.remove_file(&root.join(name))
                .map_err(|_| TransactionError::Write)?;
        }
    }
    if let Some(lease) = recovery_lease {
        lease.verify_with(ops)?;
    }
    ops.remove_dir(root).map_err(|_| TransactionError::Write)
}

fn read_original_journal_regular_file(
    ops: &impl FilesystemOps,
    path: &Path,
) -> Result<Vec<u8>, TransactionError> {
    let metadata = ops
        .metadata(path)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    ops.read(path)
        .map_err(|_| TransactionError::RecoveryAmbiguous)
}

pub fn original_journal_commitment(
    ops: &impl FilesystemOps,
    root: &Path,
    names: &[String],
) -> Result<String, TransactionError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sovmem.original-journal-commitment.v1\0");
    for name in names {
        let path = root.join(name);
        let metadata = ops
            .metadata(&path)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        let bytes = ops
            .read(&path)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        let name_length = u64::try_from(name.len()).map_err(|_| TransactionError::Write)?;
        let byte_length = u64::try_from(bytes.len()).map_err(|_| TransactionError::Write)?;
        hasher.update(&name_length.to_be_bytes());
        hasher.update(name.as_bytes());
        hasher.update(&byte_length.to_be_bytes());
        hasher.update(&bytes);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn select_recovery_mode(
    snapshot: &StoreSnapshot,
    stale: &RecoveryStaleLeases,
    journal: &OriginalJournal,
    ledger: &super::LedgerView,
) -> Result<RecoveryMode, TransactionError> {
    if journal.empty_root {
        return Ok(RecoveryMode::VerdictTransaction);
    }
    if journal.root.is_some() {
        return Ok(RecoveryMode::VerdictTransaction);
    }
    if stale.verdict.is_none() && stale.intake.is_some() {
        return Ok(RecoveryMode::IntakeOnly);
    }
    if store::has_ledger_temporary(&snapshot.paths).map_err(|_| TransactionError::Read)?
        || store::has_current_temporary(&snapshot.paths).map_err(|_| TransactionError::Read)?
        || store::has_generation_or_index_staging(&snapshot.paths)
            .map_err(|_| TransactionError::Read)?
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if stale.verdict.is_some() && stale.intake.is_some() {
        return Ok(RecoveryMode::CleanFinalization);
    }
    if ledger_has_unbound_transaction(ledger, journal.transaction_id()) {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(RecoveryMode::CleanFinalization)
}

fn ledger_has_unbound_transaction(
    ledger: &super::LedgerView,
    transaction_id: Option<&str>,
) -> bool {
    transaction_id.is_none()
        && ledger
            .events
            .iter()
            .any(|event| matches!(event, LedgerEvent::Verdict(_)))
}

fn recover_verdict_transaction<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
    ledger: &super::LedgerView,
    recovery_lease: &Lease,
) -> Result<bool, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if journal.terminal_cleanup_residue {
        return recover_terminal_empty_cleanup(snapshot, profile, runtime, journal, ledger);
    }
    if journal.empty_root {
        validate_empty_original_root(snapshot, runtime, journal)?;
        remove_empty_original_root(snapshot, runtime, journal, recovery_lease)?;
        return Ok(false);
    }
    let transaction_id = journal
        .transaction_id()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let committed = ledger
        .events
        .iter()
        .filter_map(|event| match event {
            LedgerEvent::Verdict(event) if event.payload().transaction_id == transaction_id => {
                Some(event.as_ref())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if committed.len() > 1 {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if let Some(event) = committed.first() {
        if let Some(prepared) = journal.prepared() {
            if prepared.verdict_id.as_deref() != Some(event.payload().verdict_id.as_str())
                || prepared.proposal_id.as_deref() != Some(event.payload().proposal_id.as_str())
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            recover_committed_event(snapshot, profile, runtime, journal, event, recovery_lease)?;
        } else if journal.cleanup_form && journal.files == ["plan.json"] {
            recover_committed_terminal_plan(
                snapshot,
                profile,
                runtime,
                journal,
                event,
                recovery_lease,
            )?;
        } else {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        return Ok(true);
    }
    recover_uncommitted_transaction(snapshot, profile, runtime, journal, recovery_lease)?;
    Ok(false)
}

fn prepare_recovery_cleanup_plan<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
    ledger: &super::LedgerView,
) -> Result<RecoveryCleanupPlan, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let cleanup = prepare_recovery_cleanup_inventory(snapshot, profile, runtime, journal, ledger)?;
    if journal.terminal_cleanup_residue || journal.empty_root {
        return Ok(RecoveryCleanupPlan::empty());
    }
    let transaction = &journal
        .plan
        .as_ref()
        .ok_or(TransactionError::RecoveryAmbiguous)?
        .plan;
    cleanup.immutable_plan(&runtime.ops, &snapshot.paths.root, transaction)
}

fn prepare_recovery_cleanup_inventory<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
    ledger: &super::LedgerView,
) -> Result<RecoveryCleanupInventory, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if journal.terminal_cleanup_residue || journal.empty_root {
        return Ok(RecoveryCleanupInventory::default());
    }
    let transaction_id = journal
        .transaction_id()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let committed = ledger
        .events
        .iter()
        .filter_map(|event| match event {
            LedgerEvent::Verdict(event) if event.payload().transaction_id == transaction_id => {
                Some(event.as_ref())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if committed.len() > 1 {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if let Some(event) = committed.first() {
        let cleanup = if journal.prepared().is_some() {
            validate_committed_event_cleanup(snapshot, profile, runtime, journal, event)?
        } else {
            RecoveryCleanupInventory::default()
        };
        return Ok(cleanup);
    }
    validate_uncommitted_cleanup(snapshot, profile, runtime, journal)
}

fn typed_recovery_cleanup_directories_for_execution<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
    ledger: &super::LedgerView,
    record: &RecoveryJournalRecord,
) -> Result<Vec<RecoveryCleanupDirectory>, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if record.mode != RecoveryMode::VerdictTransaction
        || !recovery_cleanup_plan_is_pristine(
            &runtime.ops,
            &snapshot.paths.root,
            &record.cleanup_plan,
        )?
    {
        return Ok(Vec::new());
    }
    let cleanup = prepare_recovery_cleanup_inventory(snapshot, profile, runtime, journal, ledger)?;
    if journal.terminal_cleanup_residue || journal.empty_root {
        return Ok(Vec::new());
    }
    let transaction = &journal
        .plan
        .as_ref()
        .ok_or(TransactionError::RecoveryAmbiguous)?
        .plan;
    if cleanup.immutable_plan(&runtime.ops, &snapshot.paths.root, transaction)?
        != record.cleanup_plan
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(cleanup.directories)
}

fn recovery_cleanup_plan_is_pristine(
    ops: &impl FilesystemOps,
    store_root: &Path,
    plan: &RecoveryCleanupPlan,
) -> Result<bool, TransactionError> {
    for step in &plan.steps {
        match ops.metadata(&store_root.join(&step.path)) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(TransactionError::RecoveryAmbiguous),
        }
    }
    Ok(true)
}

fn validate_empty_original_root<O, F>(
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if !journal.empty_root || journal.plan.is_some() || journal.prepared.is_some() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let transaction_id = journal
        .transaction_id()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let root = journal
        .root
        .as_deref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let metadata = runtime
        .ops
        .metadata(root)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || !sorted_directory_entries(root)?.is_empty()
        || !recovery_ledger_temporaries(&snapshot.paths.ledger)?.is_empty()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let next_generation = next_generation_id(&snapshot.head);
    for path in [
        snapshot
            .paths
            .canon
            .join("generations")
            .join(format!(".txn-{transaction_id}")),
        snapshot
            .paths
            .derived
            .join("indexes")
            .join(format!(".txn-{transaction_id}")),
        snapshot
            .paths
            .canon
            .join(format!(".current-{transaction_id}")),
        snapshot
            .paths
            .canon
            .join("generations")
            .join(&next_generation),
        snapshot
            .paths
            .derived
            .join("indexes")
            .join(&next_generation),
    ] {
        require_recovery_absent(&runtime.ops, &path)?;
    }
    Ok(())
}

fn remove_empty_original_root<O, F>(
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
    recovery_lease: &Lease,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    validate_empty_original_root(snapshot, runtime, journal)?;
    let root = journal
        .root
        .as_deref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    recovery_lease.verify_with(&runtime.ops)?;
    runtime
        .ops
        .remove_dir(root)
        .map_err(|_| TransactionError::Write)?;
    runtime
        .ops
        .sync_directory(&snapshot.paths.transactions)
        .map_err(|_| TransactionError::Write)
}

fn recovered_original_cleanup_witness(
    journal: &OriginalJournal,
    ops: &impl FilesystemOps,
    ignored_root: Option<&Path>,
    recovery_lease: &Lease,
) -> Result<OriginalJournal, TransactionError> {
    if journal.terminal_cleanup_residue {
        return Ok(journal.clone());
    }
    let Some(root) = journal.root.as_deref() else {
        return Ok(OriginalJournal::none());
    };
    match ops.metadata(root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(OriginalJournal::none()),
        Ok(_) if journal.empty_root => Err(TransactionError::RecoveryAmbiguous),
        Ok(_) => {
            journal.promote_to_cleanup_witness_ignoring_protected(ops, ignored_root, recovery_lease)
        }
        Err(_) => Err(TransactionError::RecoveryAmbiguous),
    }
}

fn validate_bound_verdict_event(
    snapshot: &StoreSnapshot,
    plan: &TransactionPlan,
    prepared: &PreparedJournal,
    event: &VerdictEvent,
) -> Result<(), TransactionError> {
    if !prepared.validates_against(plan, &blake3::hash(&canonical_json_bytes(plan)?).to_hex()) {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let payload = event.payload();
    if event.hash() != prepared.artifacts.verdict_event_hash
        || payload.transaction_id != plan.transaction_id
        || payload.proposal_id != plan.proposal_id
        || payload.verdict_id != plan.verdict_id
        || payload.verb.as_str() != plan.verb
        || payload.occurred_at != plan.ruled_at
        || blake3::hash(payload.reason.as_bytes()).to_hex().as_str() != plan.reason_hash
        || payload.profile_hash != plan.profile_hash
        || payload.policy_hash != plan.policy_hash
        || payload.proposal_meta_hash != plan.proposal_meta_hash
        || payload.candidate_hash != plan.candidate_hash
        || payload.base_manifest_hash != plan.parent_manifest_hash
        || payload.base_bundle_hash != plan.parent_bundle_hash
        || payload.base_target_hash != plan.base_target_hash
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let expected_base_kind = if plan.parent_ref == "seed" {
        "seed"
    } else {
        "generation"
    };
    if payload.base_kind != expected_base_kind
        || payload.base_generation.as_deref()
            != (plan.parent_ref != "seed").then_some(plan.parent_ref.as_str())
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if matches!(plan.verb.as_str(), "ACCEPT" | "REWRITE") {
        if payload.resulting_canonical_head != plan.generation_id.as_deref().unwrap_or_default()
            || payload.canonical_result.generation_id.as_deref() != plan.generation_id.as_deref()
            || payload.canonical_result.manifest_hash.as_deref()
                != prepared.artifacts.manifest_hash.as_deref()
            || payload.canonical_result.bundle_hash.as_deref()
                != prepared.artifacts.bundle_hash.as_deref()
            || payload.canonical_result.target_prior_hash != plan.base_target_hash
            || payload.canonical_result.target_new_hash.as_deref()
                != plan.approved_target_hash.as_deref()
            || payload.canonical_result.judgment_hash.as_deref() != plan.judgment_hash.as_deref()
            || payload.canonical_result.reindex_receipt_hash.as_deref()
                != prepared.artifacts.reindex_receipt_hash.as_deref()
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    } else if payload.resulting_canonical_head != snapshot.head.as_str()
        || payload.canonical_result != CanonicalResult::none()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(())
}

fn validate_ledger_only_residue<O, F>(
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    plan: &TransactionPlan,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let next_generation = next_generation_id(&snapshot.head);
    for path in [
        snapshot
            .paths
            .canon
            .join("generations")
            .join(format!(".txn-{}", plan.transaction_id)),
        snapshot
            .paths
            .derived
            .join("indexes")
            .join(format!(".txn-{}", plan.transaction_id)),
        snapshot
            .paths
            .canon
            .join(format!(".current-{}", plan.transaction_id)),
        snapshot
            .paths
            .canon
            .join("generations")
            .join(&next_generation),
        snapshot
            .paths
            .derived
            .join("indexes")
            .join(&next_generation),
        snapshot
            .paths
            .evidence
            .join("reindex")
            .join(format!("{}.json", plan.verdict_id)),
    ] {
        require_recovery_absent(&runtime.ops, &path)?;
    }
    Ok(())
}

fn validate_committed_current_temporary<O, F>(
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    plan: &TransactionPlan,
) -> Result<RecoveryCleanupInventory, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let generation_id = plan
        .generation_id
        .as_deref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    if snapshot.head.as_str() != plan.parent_ref && snapshot.head.as_str() != generation_id {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let temporary = snapshot
        .paths
        .canon
        .join(format!(".current-{}", plan.transaction_id));
    let Some(bytes) = read_optional_recovery_regular_file(&runtime.ops, &temporary)? else {
        return Ok(RecoveryCleanupInventory::default());
    };
    if snapshot.head.as_str() != plan.parent_ref || bytes != format!("{generation_id}\n").as_bytes()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let mut cleanup = RecoveryCleanupInventory::default();
    cleanup.bind_file(temporary, bytes)?;
    Ok(cleanup)
}

fn validate_committed_event_cleanup<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
    verdict_event: &VerdictEvent,
) -> Result<RecoveryCleanupInventory, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let plan = journal
        .plan
        .as_ref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let prepared = journal
        .prepared()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    validate_committed_plan_source_pair(snapshot, &plan.plan)?;
    validate_bound_verdict_event(snapshot, &plan.plan, prepared, verdict_event)?;
    let event = verdict_event.payload();
    if !event.verb.applies() {
        validate_ledger_only_residue(snapshot, runtime, &plan.plan)?;
        return Ok(RecoveryCleanupInventory::default());
    }
    let generation_id = event
        .canonical_result
        .generation_id
        .as_deref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    if event.resulting_canonical_head != generation_id {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let generation = verify_generation(
        profile,
        &snapshot.paths.canon.join("generations").join(generation_id),
        VerificationLevel::Three,
    )
    .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    validate_committed_generation_against_plan(snapshot, profile, &plan.plan, &generation)?;
    if generation.manifest_hash
        != prepared
            .artifacts
            .manifest_hash
            .as_deref()
            .unwrap_or_default()
        || generation.bundle_hash
            != prepared
                .artifacts
                .bundle_hash
                .as_deref()
                .unwrap_or_default()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let receipt_path = snapshot
        .paths
        .evidence
        .join("reindex")
        .join(format!("{}.json", event.verdict_id));
    let receipt_bytes = runtime
        .ops
        .read(&receipt_path)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let receipt =
        serde_json::from_slice(&receipt_bytes).map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if blake3::hash(&receipt_bytes).to_hex().as_str()
        != event
            .canonical_result
            .reindex_receipt_hash
            .as_deref()
            .ok_or(TransactionError::RecoveryAmbiguous)?
        || prepared.artifacts.reindex_receipt_hash.as_deref()
            != event.canonical_result.reindex_receipt_hash.as_deref()
        || super::verify_reindex_receipt(
            profile,
            &generation,
            &snapshot.paths.derived.join("indexes").join(generation_id),
            &receipt,
            &receipt_bytes,
        )
        .is_err()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let cleanup = validate_committed_current_temporary(snapshot, runtime, &plan.plan)?;
    Ok(cleanup)
}

fn recover_committed_event<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
    verdict_event: &VerdictEvent,
    recovery_lease: &Lease,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let cleanup =
        validate_committed_event_cleanup(snapshot, profile, runtime, journal, verdict_event)?;
    if !cleanup.directories.is_empty() || !cleanup.files.is_empty() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let plan = &journal
        .plan
        .as_ref()
        .ok_or(TransactionError::RecoveryAmbiguous)?
        .plan;
    let Some(generation_id) = plan.generation_id.as_deref() else {
        return Ok(());
    };
    if snapshot.head.as_str() != generation_id {
        publish_current(
            &snapshot.paths.current,
            generation_id,
            &plan.transaction_id,
            runtime,
            Some(recovery_lease),
        )?;
    }
    Ok(())
}

fn recover_committed_terminal_plan<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
    verdict_event: &VerdictEvent,
    recovery_lease: &Lease,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if !journal.cleanup_form || journal.files != ["plan.json"] {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let plan = journal
        .plan
        .as_ref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    validate_committed_plan_source_pair(snapshot, &plan.plan)?;
    validate_terminal_plan_event(snapshot, &plan.plan, verdict_event)?;
    let event = verdict_event.payload();
    if !event.verb.applies() {
        return validate_ledger_only_residue(snapshot, runtime, &plan.plan);
    }
    let generation_id = event
        .canonical_result
        .generation_id
        .as_deref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let generation = verify_generation(
        profile,
        &snapshot.paths.canon.join("generations").join(generation_id),
        VerificationLevel::Three,
    )
    .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    validate_committed_generation_against_plan(snapshot, profile, &plan.plan, &generation)?;
    if generation.manifest_hash
        != event
            .canonical_result
            .manifest_hash
            .as_deref()
            .ok_or(TransactionError::RecoveryAmbiguous)?
        || generation.bundle_hash
            != event
                .canonical_result
                .bundle_hash
                .as_deref()
                .ok_or(TransactionError::RecoveryAmbiguous)?
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let receipt_path = snapshot
        .paths
        .evidence
        .join("reindex")
        .join(format!("{}.json", event.verdict_id));
    let receipt_bytes = runtime
        .ops
        .read(&receipt_path)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let receipt =
        serde_json::from_slice(&receipt_bytes).map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if blake3::hash(&receipt_bytes).to_hex().as_str()
        != event
            .canonical_result
            .reindex_receipt_hash
            .as_deref()
            .ok_or(TransactionError::RecoveryAmbiguous)?
        || super::verify_reindex_receipt(
            profile,
            &generation,
            &snapshot.paths.derived.join("indexes").join(generation_id),
            &receipt,
            &receipt_bytes,
        )
        .is_err()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let cleanup = validate_committed_current_temporary(snapshot, runtime, &plan.plan)?;
    if !cleanup.directories.is_empty() || !cleanup.files.is_empty() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if snapshot.head.as_str() != generation_id {
        publish_current(
            &snapshot.paths.current,
            generation_id,
            &plan.plan.transaction_id,
            runtime,
            Some(recovery_lease),
        )?;
    }
    Ok(())
}

fn validate_terminal_plan_event(
    snapshot: &StoreSnapshot,
    plan: &TransactionPlan,
    event: &VerdictEvent,
) -> Result<(), TransactionError> {
    let payload = event.payload();
    if payload.transaction_id != plan.transaction_id
        || payload.proposal_id != plan.proposal_id
        || payload.verdict_id != plan.verdict_id
        || payload.verb.as_str() != plan.verb
        || payload.occurred_at != plan.ruled_at
        || blake3::hash(payload.reason.as_bytes()).to_hex().as_str() != plan.reason_hash
        || payload.profile_hash != plan.profile_hash
        || payload.policy_hash != plan.policy_hash
        || payload.proposal_meta_hash != plan.proposal_meta_hash
        || payload.candidate_hash != plan.candidate_hash
        || payload.base_manifest_hash != plan.parent_manifest_hash
        || payload.base_bundle_hash != plan.parent_bundle_hash
        || payload.base_target_hash != plan.base_target_hash
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let expected_base_kind = if plan.parent_ref == "seed" {
        "seed"
    } else {
        "generation"
    };
    if payload.base_kind != expected_base_kind
        || payload.base_generation.as_deref()
            != (plan.parent_ref != "seed").then_some(plan.parent_ref.as_str())
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if payload.verb.applies() {
        if payload.resulting_canonical_head != plan.generation_id.as_deref().unwrap_or_default()
            || payload.canonical_result.generation_id.as_deref() != plan.generation_id.as_deref()
            || payload.canonical_result.target_prior_hash != plan.base_target_hash
            || payload.canonical_result.target_new_hash.as_deref()
                != plan.approved_target_hash.as_deref()
            || payload.canonical_result.judgment_hash.as_deref() != plan.judgment_hash.as_deref()
            || payload
                .canonical_result
                .manifest_hash
                .as_deref()
                .is_none_or(|hash| !valid_hash(hash))
            || payload
                .canonical_result
                .bundle_hash
                .as_deref()
                .is_none_or(|hash| !valid_hash(hash))
            || payload
                .canonical_result
                .reindex_receipt_hash
                .as_deref()
                .is_none_or(|hash| !valid_hash(hash))
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    } else if payload.resulting_canonical_head != snapshot.head.as_str()
        || payload.canonical_result != CanonicalResult::none()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(())
}

fn recover_terminal_empty_cleanup<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
    ledger: &super::LedgerView,
) -> Result<bool, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if !journal.terminal_cleanup_residue || !journal.cleanup_form || !journal.files.is_empty() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let transaction_id = journal
        .transaction_id()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let committed = ledger
        .events
        .iter()
        .filter_map(|event| match event {
            LedgerEvent::Verdict(event) if event.payload().transaction_id == transaction_id => {
                Some(event.as_ref())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if committed.len() != 1 {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    validate_terminal_empty_event_state(snapshot, profile, runtime, committed[0])?;
    Ok(true)
}

fn validate_terminal_empty_event_state<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    event: &VerdictEvent,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let payload = event.payload();
    if payload.profile_hash != snapshot.profile_hash
        || payload.policy_hash != snapshot.policy.hash
        || payload.store_id != snapshot.policy.policy.store_id
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let pair = read_sealed_pair_structural(&snapshot.paths, &payload.proposal_id)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if pair.meta_hash != payload.proposal_meta_hash
        || pair.candidate_hash != payload.candidate_hash
        || pair.meta.profile_hash != payload.profile_hash
        || pair.meta.policy_hash != payload.policy_hash
        || pair.meta.source_id != payload.source_id
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if !payload.verb.applies() {
        if payload.canonical_result != CanonicalResult::none()
            || payload.resulting_canonical_head != snapshot.head.as_str()
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        return Ok(());
    }
    let generation_id = payload
        .canonical_result
        .generation_id
        .as_deref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    if payload.resulting_canonical_head != generation_id || snapshot.head.as_str() != generation_id
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let generation = verify_generation(
        profile,
        &snapshot.paths.canon.join("generations").join(generation_id),
        VerificationLevel::Three,
    )
    .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if generation.manifest_hash
        != payload
            .canonical_result
            .manifest_hash
            .as_deref()
            .ok_or(TransactionError::RecoveryAmbiguous)?
        || generation.bundle_hash
            != payload
                .canonical_result
                .bundle_hash
                .as_deref()
                .ok_or(TransactionError::RecoveryAmbiguous)?
        || generation.manifest.applied_proposal_id != payload.proposal_id
        || generation.manifest.applied_verdict_id != payload.verdict_id
        || generation.manifest.policy_hash != payload.policy_hash
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let receipt_path = snapshot
        .paths
        .evidence
        .join("reindex")
        .join(format!("{}.json", payload.verdict_id));
    let receipt_bytes = runtime
        .ops
        .read(&receipt_path)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let receipt =
        serde_json::from_slice(&receipt_bytes).map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if blake3::hash(&receipt_bytes).to_hex().as_str()
        != payload
            .canonical_result
            .reindex_receipt_hash
            .as_deref()
            .ok_or(TransactionError::RecoveryAmbiguous)?
        || super::verify_reindex_receipt(
            profile,
            &generation,
            &snapshot.paths.derived.join("indexes").join(generation_id),
            &receipt,
            &receipt_bytes,
        )
        .is_err()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryCleanupArtifactKind {
    Generation,
    Index,
    Receipt,
    LedgerTemporary,
    PointerTemporary,
    ProposalTemporary,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryCleanupEntryType {
    File,
    Directory,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCleanupStep {
    pub artifact_kind: RecoveryCleanupArtifactKind,
    pub path: String,
    pub entry_type: RecoveryCleanupEntryType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCleanupPlan {
    pub schema_version: String,
    pub steps: Vec<RecoveryCleanupStep>,
}

impl RecoveryCleanupPlan {
    pub fn empty() -> Self {
        Self {
            schema_version: RECOVERY_CLEANUP_PLAN_SCHEMA.to_owned(),
            steps: Vec::new(),
        }
    }

    pub fn hash(&self) -> Result<String, TransactionError> {
        let bytes = canonical_json_bytes(self)?;
        let mut hasher = blake3::Hasher::new();
        frame(&mut hasher, RECOVERY_CLEANUP_PLAN_DOMAIN);
        frame(&mut hasher, &bytes);
        Ok(hasher.finalize().to_hex().to_string())
    }

    pub fn validate(&self) -> Result<(), TransactionError> {
        if self.schema_version != RECOVERY_CLEANUP_PLAN_SCHEMA {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        let mut previous = None;
        for step in &self.steps {
            let path = Path::new(&step.path);
            if path.is_absolute()
                || path.components().next().is_none()
                || step
                    .path
                    .chars()
                    .any(|character| character == '\\' || character.is_control())
                || path
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
                || previous == Some(step.path.as_str())
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            let components = path
                .components()
                .filter_map(|component| match component {
                    std::path::Component::Normal(value) => value.to_str(),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if components.join("/") != step.path {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            let kind_matches = match step.artifact_kind {
                RecoveryCleanupArtifactKind::Generation => {
                    components.len() >= 3
                        && components.starts_with(&["canon", "generations"])
                        && (valid_generation_id(components[2])
                            || components[2]
                                .strip_prefix(".txn-")
                                .is_some_and(valid_transaction_id))
                }
                RecoveryCleanupArtifactKind::Index => {
                    components.len() >= 3
                        && components.starts_with(&["derived", "indexes"])
                        && (valid_generation_id(components[2])
                            || components[2]
                                .strip_prefix(".txn-")
                                .is_some_and(valid_transaction_id))
                }
                RecoveryCleanupArtifactKind::Receipt => {
                    components.len() == 3
                        && components[..2] == ["evidence", "reindex"]
                        && components[2]
                            .strip_suffix(".json")
                            .is_some_and(valid_verdict_id)
                        && step.entry_type == RecoveryCleanupEntryType::File
                }
                RecoveryCleanupArtifactKind::LedgerTemporary => {
                    components.len() == 2
                        && components[0] == "ledger"
                        && components[1]
                            .strip_prefix(".verdicts-")
                            .and_then(|suffix| {
                                suffix.parse::<u64>().ok().map(|value| (suffix, value))
                            })
                            .is_some_and(|(suffix, value)| suffix == value.to_string())
                        && step.entry_type == RecoveryCleanupEntryType::File
                }
                RecoveryCleanupArtifactKind::PointerTemporary => {
                    components.len() == 2
                        && components[0] == "canon"
                        && components[1]
                            .strip_prefix(".current-")
                            .is_some_and(valid_transaction_id)
                        && step.entry_type == RecoveryCleanupEntryType::File
                }
                RecoveryCleanupArtifactKind::ProposalTemporary => {
                    proposal_temporary_cleanup_components_match(&components, step.entry_type)
                }
            };
            if !kind_matches
                || (step.entry_type == RecoveryCleanupEntryType::Directory
                    && !matches!(
                        step.artifact_kind,
                        RecoveryCleanupArtifactKind::Generation
                            | RecoveryCleanupArtifactKind::Index
                            | RecoveryCleanupArtifactKind::ProposalTemporary
                    ))
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            match step.entry_type {
                RecoveryCleanupEntryType::File => {
                    if step.byte_length.is_none()
                        || step
                            .content_hash
                            .as_deref()
                            .is_none_or(|hash| !valid_hash(hash))
                    {
                        return Err(TransactionError::RecoveryAmbiguous);
                    }
                }
                RecoveryCleanupEntryType::Directory => {
                    if step.byte_length.is_some() || step.content_hash.is_some() {
                        return Err(TransactionError::RecoveryAmbiguous);
                    }
                }
            }
            previous = Some(step.path.as_str());
        }
        for (index, step) in self.steps.iter().enumerate() {
            if self.steps[..index]
                .iter()
                .any(|earlier| earlier.path == step.path)
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            if step.entry_type == RecoveryCleanupEntryType::Directory
                && self.steps[index + 1..]
                    .iter()
                    .any(|later| Path::new(&later.path).starts_with(Path::new(&step.path)))
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }
        Ok(())
    }

    fn validate_intake_owner(&self, owner_hash: &str) -> Result<(), TransactionError> {
        self.validate()?;
        if !valid_hash(owner_hash)
            || self
                .steps
                .iter()
                .any(|step| step.artifact_kind != RecoveryCleanupArtifactKind::ProposalTemporary)
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        let mut previous_root = None;
        let mut roots = Vec::new();
        for step in &self.steps {
            let components = Path::new(&step.path)
                .components()
                .filter_map(|component| match component {
                    std::path::Component::Normal(value) => value.to_str(),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let temporary_name = components
                .get(2)
                .copied()
                .ok_or(TransactionError::RecoveryAmbiguous)?;
            if store::proposal_temporary_owner_hash(temporary_name) != Some(owner_hash) {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            if let Some(previous) = previous_root {
                if temporary_name < previous {
                    return Err(TransactionError::RecoveryAmbiguous);
                }
            }
            if roots.last().copied() != Some(temporary_name) {
                roots.push(temporary_name);
            }
            previous_root = Some(temporary_name);
        }
        for root in roots {
            let prefix = format!("staging/proposals/{root}");
            let paths = self
                .steps
                .iter()
                .filter(|step| Path::new(&step.path).starts_with(Path::new(&prefix)))
                .map(|step| step.path.as_str())
                .collect::<Vec<_>>();
            let candidate = format!("{prefix}/candidate.md");
            let meta = format!("{prefix}/proposal.meta.json");
            let valid = paths == [prefix.as_str()]
                || paths == [candidate.as_str(), prefix.as_str()]
                || paths == [candidate.as_str(), meta.as_str(), prefix.as_str()];
            if !valid {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }
        Ok(())
    }

    fn validate_for_recovery(
        &self,
        mode: RecoveryMode,
        quarantined_leases: &[RecoveryLeaseBinding],
    ) -> Result<(), TransactionError> {
        match mode {
            RecoveryMode::VerdictTransaction => {
                self.validate()?;
                if self.steps.iter().any(|step| {
                    step.artifact_kind == RecoveryCleanupArtifactKind::ProposalTemporary
                }) {
                    return Err(TransactionError::RecoveryAmbiguous);
                }
                Ok(())
            }
            RecoveryMode::IntakeOnly => {
                let owner_hash = quarantined_leases
                    .iter()
                    .find(|binding| binding.kind == RecoveryLeaseKind::Intake)
                    .map(|binding| binding.owner_hash.as_str())
                    .ok_or(TransactionError::RecoveryAmbiguous)?;
                self.validate_intake_owner(owner_hash)
            }
            RecoveryMode::CleanFinalization if self.steps.is_empty() => self.validate(),
            RecoveryMode::CleanFinalization => Err(TransactionError::RecoveryAmbiguous),
        }
    }
}

fn proposal_temporary_cleanup_components_match(
    components: &[&str],
    entry_type: RecoveryCleanupEntryType,
) -> bool {
    if components.len() < 3
        || components[..2] != ["staging", "proposals"]
        || !store::valid_proposal_temporary(components[2])
        || store::proposal_temporary_owner_hash(components[2]).is_none()
    {
        return false;
    }
    match (components.len(), entry_type) {
        (3, RecoveryCleanupEntryType::Directory) => true,
        (4, RecoveryCleanupEntryType::File) => {
            matches!(components[3], "candidate.md" | "proposal.meta.json")
        }
        _ => false,
    }
}

#[derive(Default)]
pub struct RecoveryCleanupInventory {
    pub directories: Vec<RecoveryCleanupDirectory>,
    pub files: Vec<RecoveryCleanupFile>,
}

#[derive(Clone)]
pub struct RecoveryCleanupFile {
    path: PathBuf,
    bytes: Vec<u8>,
}

pub enum RecoveryCleanupDirectory {
    Generation(RecoveryGenerationBinding),
    Index(RecoveryIndexBinding),
}

#[derive(Clone)]
pub struct RecoveryGenerationBinding {
    path: PathBuf,
    profile: P3Profile,
    snapshot: StoreSnapshot,
    plan: TransactionPlan,
    artifacts: Option<PreparedArtifacts>,
}

pub struct RecoveryIndexBinding {
    path: PathBuf,
    generation: RecoveryGenerationBinding,
    receipt: Option<RecoveryCleanupFile>,
}

impl RecoveryGenerationBinding {
    pub fn new(
        path: PathBuf,
        profile: &P3Profile,
        snapshot: &StoreSnapshot,
        plan: &TransactionPlan,
        prepared: Option<&PreparedJournal>,
    ) -> Self {
        Self {
            path,
            profile: profile.clone(),
            snapshot: snapshot.clone(),
            plan: plan.clone(),
            artifacts: prepared.map(|prepared| prepared.artifacts.clone()),
        }
    }

    pub fn verify(&self) -> Result<super::VerifiedGeneration, TransactionError> {
        validate_plan_source_pair(&self.snapshot, &self.plan)?;
        let generation = verify_generation(&self.profile, &self.path, VerificationLevel::Three)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        validate_generation_against_plan(&self.snapshot, &self.profile, &self.plan, &generation)?;
        if let Some(artifacts) = &self.artifacts {
            if generation.manifest_hash != artifacts.manifest_hash.as_deref().unwrap_or_default()
                || generation.bundle_hash != artifacts.bundle_hash.as_deref().unwrap_or_default()
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }
        Ok(generation)
    }
}

impl RecoveryCleanupDirectory {
    pub fn path(&self) -> &Path {
        match self {
            Self::Generation(binding) => &binding.path,
            Self::Index(binding) => &binding.path,
        }
    }

    pub fn verify(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        match self {
            Self::Generation(binding) => {
                binding.verify()?;
            }
            Self::Index(binding) => {
                let generation = binding.generation.verify()?;
                super::open_p3_index_generation(
                    &binding.generation.profile,
                    &generation,
                    &binding.path,
                )
                .map_err(|_| TransactionError::RecoveryAmbiguous)?;
                if let Some(receipt) = &binding.receipt {
                    let bytes = read_optional_recovery_regular_file(ops, &receipt.path)?
                        .filter(|bytes| bytes == &receipt.bytes)
                        .ok_or(TransactionError::RecoveryAmbiguous)?;
                    let parsed = serde_json::from_slice(&bytes)
                        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
                    super::verify_reindex_receipt(
                        &binding.generation.profile,
                        &generation,
                        &binding.path,
                        &parsed,
                        &bytes,
                    )
                    .map_err(|_| TransactionError::RecoveryAmbiguous)?;
                }
            }
        }
        Ok(())
    }
}

impl RecoveryCleanupInventory {
    fn bind_directory(
        &mut self,
        directory: RecoveryCleanupDirectory,
    ) -> Result<(), TransactionError> {
        let path = directory.path();
        if self
            .directories
            .iter()
            .any(|existing| existing.path() == path)
            || self
                .files
                .iter()
                .any(|existing| existing.path.as_path() == path)
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        self.directories.push(directory);
        Ok(())
    }

    pub fn bind_file(&mut self, path: PathBuf, bytes: Vec<u8>) -> Result<(), TransactionError> {
        if self
            .directories
            .iter()
            .any(|existing| existing.path() == path)
            || self.files.iter().any(|existing| existing.path == path)
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        self.files.push(RecoveryCleanupFile { path, bytes });
        Ok(())
    }

    fn extend(&mut self, other: Self) -> Result<(), TransactionError> {
        for directory in other.directories {
            self.bind_directory(directory)?;
        }
        for file in other.files {
            self.bind_file(file.path, file.bytes)?;
        }
        Ok(())
    }

    pub fn immutable_plan(
        &self,
        ops: &impl FilesystemOps,
        store_root: &Path,
        transaction: &TransactionPlan,
    ) -> Result<RecoveryCleanupPlan, TransactionError> {
        for file in &self.files {
            if read_optional_recovery_regular_file(ops, &file.path)?.as_deref()
                != Some(file.bytes.as_slice())
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }

        let mut steps = Vec::new();
        for directory in &self.directories {
            require_recovery_regular_directory(ops, directory.path())?;
            let kind = cleanup_artifact_kind(store_root, transaction, directory.path())?;
            let mut before = Vec::new();
            enumerate_cleanup_tree(ops, store_root, directory.path(), kind, &mut before)?;
            directory.verify(ops)?;
            let mut after = Vec::new();
            enumerate_cleanup_tree(ops, store_root, directory.path(), kind, &mut after)?;
            if before != after {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            steps.extend(before);
        }
        for file in &self.files {
            let kind = cleanup_artifact_kind(store_root, transaction, &file.path)?;
            steps.push(cleanup_file_step(
                store_root,
                &file.path,
                kind,
                &file.bytes,
            )?);
        }
        let plan = RecoveryCleanupPlan {
            schema_version: RECOVERY_CLEANUP_PLAN_SCHEMA.to_owned(),
            steps,
        };
        plan.validate()?;
        Ok(plan)
    }

    fn execute(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        for directory in &self.directories {
            require_recovery_regular_directory(ops, directory.path())?;
        }
        for directory in &self.directories {
            directory.verify(ops)?;
        }
        for file in &self.files {
            if read_optional_recovery_regular_file(ops, &file.path)?.as_deref()
                != Some(file.bytes.as_slice())
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }
        for directory in &self.directories {
            remove_verified_tree(ops, directory.path())?;
            ops.sync_directory(directory.path().parent().ok_or(TransactionError::Write)?)
                .map_err(|_| TransactionError::Write)?;
        }
        for file in &self.files {
            if read_optional_recovery_regular_file(ops, &file.path)?.as_deref()
                != Some(file.bytes.as_slice())
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            ops.remove_file(&file.path)
                .map_err(|_| TransactionError::Write)?;
            ops.sync_directory(file.path.parent().ok_or(TransactionError::Write)?)
                .map_err(|_| TransactionError::Write)?;
        }
        Ok(())
    }

    pub fn execute_protected(
        &self,
        ops: &impl FilesystemOps,
        recovery_lease: &Lease,
    ) -> Result<(), TransactionError> {
        for directory in &self.directories {
            require_recovery_regular_directory(ops, directory.path())?;
        }
        for directory in &self.directories {
            directory.verify(ops)?;
        }
        for file in &self.files {
            if read_optional_recovery_regular_file(ops, &file.path)?.as_deref()
                != Some(file.bytes.as_slice())
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }
        for directory in &self.directories {
            remove_verified_tree_protected(ops, directory.path(), recovery_lease)?;
            ops.sync_directory(directory.path().parent().ok_or(TransactionError::Write)?)
                .map_err(|_| TransactionError::Write)?;
        }
        for file in &self.files {
            if read_optional_recovery_regular_file(ops, &file.path)?.as_deref()
                != Some(file.bytes.as_slice())
            {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            recovery_lease.verify_with(ops)?;
            ops.remove_file(&file.path)
                .map_err(|_| TransactionError::Write)?;
            ops.sync_directory(file.path.parent().ok_or(TransactionError::Write)?)
                .map_err(|_| TransactionError::Write)?;
        }
        Ok(())
    }
}

fn cleanup_relative_path(store_root: &Path, path: &Path) -> Result<String, TransactionError> {
    let relative = path
        .strip_prefix(store_root)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if relative.components().next().is_none()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    relative
        .to_str()
        .map(str::to_owned)
        .ok_or(TransactionError::RecoveryAmbiguous)
}

fn cleanup_artifact_kind(
    store_root: &Path,
    transaction: &TransactionPlan,
    path: &Path,
) -> Result<RecoveryCleanupArtifactKind, TransactionError> {
    let relative = cleanup_relative_path(store_root, path)?;
    let staged = format!(".txn-{}", transaction.transaction_id);
    let mut exact = vec![
        (
            format!("canon/generations/{staged}"),
            RecoveryCleanupArtifactKind::Generation,
        ),
        (
            format!("derived/indexes/{staged}"),
            RecoveryCleanupArtifactKind::Index,
        ),
        (
            format!("evidence/reindex/{}.json", transaction.verdict_id),
            RecoveryCleanupArtifactKind::Receipt,
        ),
        (
            format!("canon/.current-{}", transaction.transaction_id),
            RecoveryCleanupArtifactKind::PointerTemporary,
        ),
    ];
    if let Some(generation) = transaction.generation_id.as_deref() {
        exact.extend([
            (
                format!("canon/generations/{generation}"),
                RecoveryCleanupArtifactKind::Generation,
            ),
            (
                format!("derived/indexes/{generation}"),
                RecoveryCleanupArtifactKind::Index,
            ),
        ]);
    }
    for (root, kind) in exact {
        if relative == root || Path::new(&relative).starts_with(Path::new(&root)) {
            return Ok(kind);
        }
    }
    if Path::new(&relative)
        .parent()
        .is_some_and(|parent| parent == Path::new("ledger"))
        && Path::new(&relative)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.strip_prefix(".verdicts-")
                    .is_some_and(|suffix| suffix.parse::<u64>().is_ok())
            })
    {
        return Ok(RecoveryCleanupArtifactKind::LedgerTemporary);
    }
    Err(TransactionError::RecoveryAmbiguous)
}

fn cleanup_file_step(
    store_root: &Path,
    path: &Path,
    artifact_kind: RecoveryCleanupArtifactKind,
    bytes: &[u8],
) -> Result<RecoveryCleanupStep, TransactionError> {
    Ok(RecoveryCleanupStep {
        artifact_kind,
        path: cleanup_relative_path(store_root, path)?,
        entry_type: RecoveryCleanupEntryType::File,
        byte_length: Some(
            u64::try_from(bytes.len()).map_err(|_| TransactionError::RecoveryAmbiguous)?,
        ),
        content_hash: Some(blake3::hash(bytes).to_hex().to_string()),
    })
}

fn enumerate_cleanup_tree(
    ops: &impl FilesystemOps,
    store_root: &Path,
    path: &Path,
    artifact_kind: RecoveryCleanupArtifactKind,
    steps: &mut Vec<RecoveryCleanupStep>,
) -> Result<(), TransactionError> {
    for entry in sorted_directory_entries(path)? {
        let metadata = ops
            .metadata(&entry.path())
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if metadata.file_type().is_symlink() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if metadata.is_dir() {
            enumerate_cleanup_tree(ops, store_root, &entry.path(), artifact_kind, steps)?;
        } else if metadata.is_file() {
            let bytes = ops
                .read(&entry.path())
                .map_err(|_| TransactionError::RecoveryAmbiguous)?;
            steps.push(cleanup_file_step(
                store_root,
                &entry.path(),
                artifact_kind,
                &bytes,
            )?);
        } else {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    steps.push(RecoveryCleanupStep {
        artifact_kind,
        path: cleanup_relative_path(store_root, path)?,
        entry_type: RecoveryCleanupEntryType::Directory,
        byte_length: None,
        content_hash: None,
    });
    Ok(())
}

pub fn execute_recovery_cleanup_plan(
    ops: &impl FilesystemOps,
    store_root: &Path,
    plan: &RecoveryCleanupPlan,
    recovery_lease: &Lease,
    typed_directories: &[RecoveryCleanupDirectory],
) -> Result<(), TransactionError> {
    plan.validate()?;
    loop {
        let mut existing = Vec::new();
        for (index, step) in plan.steps.iter().enumerate() {
            let path = store_root.join(&step.path);
            match ops.metadata(&path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Ok(_) => existing.push(index),
                Err(_) => return Err(TransactionError::RecoveryAmbiguous),
            }
        }
        let Some(first) = existing.first().copied() else {
            return Ok(());
        };
        if existing != (first..plan.steps.len()).collect::<Vec<_>>() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        validate_cleanup_suffix(ops, store_root, &plan.steps[first..])?;
        revalidate_typed_cleanup_before_first_tree_delete(ops, first, typed_directories)?;
        recovery_lease.verify_with(ops)?;
        let next = &plan.steps[first];
        let path = store_root.join(&next.path);
        match next.entry_type {
            RecoveryCleanupEntryType::File => ops
                .remove_file(&path)
                .map_err(|_| TransactionError::Write)?,
            RecoveryCleanupEntryType::Directory => {
                ops.remove_dir(&path).map_err(|_| TransactionError::Write)?
            }
        }
        ops.sync_directory(path.parent().ok_or(TransactionError::Write)?)
            .map_err(|_| TransactionError::Write)?;
    }
}

fn revalidate_typed_cleanup_before_first_tree_delete(
    ops: &impl FilesystemOps,
    first: usize,
    typed_directories: &[RecoveryCleanupDirectory],
) -> Result<(), TransactionError> {
    if first != 0 {
        return Ok(());
    }
    for directory in typed_directories {
        directory.verify(ops)?;
    }
    Ok(())
}

fn validate_cleanup_suffix(
    ops: &impl FilesystemOps,
    store_root: &Path,
    suffix: &[RecoveryCleanupStep],
) -> Result<(), TransactionError> {
    for step in suffix {
        let path = store_root.join(&step.path);
        let metadata = ops
            .metadata(&path)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if metadata.file_type().is_symlink() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        match step.entry_type {
            RecoveryCleanupEntryType::File if metadata.is_file() => {
                let bytes = ops
                    .read(&path)
                    .map_err(|_| TransactionError::RecoveryAmbiguous)?;
                if u64::try_from(bytes.len()).ok() != step.byte_length
                    || step.content_hash.as_deref() != Some(blake3::hash(&bytes).to_hex().as_str())
                {
                    return Err(TransactionError::RecoveryAmbiguous);
                }
            }
            RecoveryCleanupEntryType::Directory if metadata.is_dir() => {
                let mut allowed_children = suffix
                    .iter()
                    .filter_map(|candidate| {
                        let candidate = Path::new(&candidate.path);
                        (candidate.parent() == Some(Path::new(&step.path)))
                            .then(|| candidate.file_name().map(|name| name.to_owned()))
                            .flatten()
                    })
                    .collect::<Vec<_>>();
                allowed_children.sort();
                let actual_children = sorted_directory_entries(&path)?
                    .into_iter()
                    .map(|entry| entry.file_name())
                    .collect::<Vec<_>>();
                if actual_children != allowed_children {
                    return Err(TransactionError::RecoveryAmbiguous);
                }
            }
            _ => return Err(TransactionError::RecoveryAmbiguous),
        }
    }
    Ok(())
}

fn recover_uncommitted_transaction<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
    _recovery_lease: &Lease,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let cleanup = validate_uncommitted_cleanup(snapshot, profile, runtime, journal)?;
    if cleanup.directories.is_empty() && cleanup.files.is_empty() {
        Ok(())
    } else {
        Err(TransactionError::RecoveryAmbiguous)
    }
}

fn validate_uncommitted_cleanup<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    journal: &OriginalJournal,
) -> Result<RecoveryCleanupInventory, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let plan = journal
        .plan
        .as_ref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    validate_plan_source_pair(snapshot, &plan.plan)?;
    let mut cleanup = match journal.prepared() {
        Some(prepared) => {
            validate_prepared_successor_temporary(snapshot, runtime, &plan.plan, prepared)?
        }
        None => {
            if !recovery_ledger_temporaries(&snapshot.paths.ledger)?.is_empty() {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            RecoveryCleanupInventory::default()
        }
    };
    if journal.prepared().is_none() {
        cleanup.extend(validate_plan_only_residue(
            snapshot, profile, runtime, &plan.plan,
        )?)?;
    } else {
        cleanup.extend(validate_prepared_residue(
            snapshot,
            profile,
            runtime,
            &plan.plan,
            journal.prepared().expect("checked prepared journal"),
        )?)?;
    }
    Ok(cleanup)
}

pub fn validate_plan_source_pair(
    snapshot: &StoreSnapshot,
    plan: &TransactionPlan,
) -> Result<ProposalPair, TransactionError> {
    validate_plan_source_pair_for_committed_head(snapshot, plan, false)
}

fn validate_committed_plan_source_pair(
    snapshot: &StoreSnapshot,
    plan: &TransactionPlan,
) -> Result<ProposalPair, TransactionError> {
    validate_plan_source_pair_for_committed_head(snapshot, plan, true)
}

fn validate_plan_source_pair_for_committed_head(
    snapshot: &StoreSnapshot,
    plan: &TransactionPlan,
    allow_resulting_head: bool,
) -> Result<ProposalPair, TransactionError> {
    let head_matches_plan = plan.parent_ref == snapshot.head.as_str()
        || (allow_resulting_head && plan.generation_id.as_deref() == Some(snapshot.head.as_str()));
    if !head_matches_plan
        || plan.profile_hash != snapshot.profile_hash
        || plan.policy_hash != snapshot.policy.hash
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let pair = read_sealed_pair_structural(&snapshot.paths, &plan.proposal_id)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let expected_base_kind = if plan.parent_ref == "seed" {
        "seed"
    } else {
        "generation"
    };
    if pair.meta.proposal_id != plan.proposal_id
        || pair.meta_hash != plan.proposal_meta_hash
        || pair.candidate_hash != plan.candidate_hash
        || pair.meta.candidate_hash != plan.candidate_hash
        || pair.meta.profile_hash != plan.profile_hash
        || pair.meta.policy_hash != plan.policy_hash
        || pair.meta.target_id != plan.target_id
        || pair.meta.target_path != plan.target_path
        || pair.meta.base.kind != expected_base_kind
        || pair.meta.base.generation_id.as_deref()
            != (plan.parent_ref != "seed").then_some(plan.parent_ref.as_str())
        || pair.meta.base.manifest_hash != plan.parent_manifest_hash
        || pair.meta.base.bundle_hash != plan.parent_bundle_hash
        || pair.meta.base.target_hash != plan.base_target_hash
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(pair)
}

fn validate_plan_only_residue<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    plan: &TransactionPlan,
) -> Result<RecoveryCleanupInventory, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let receipt = snapshot
        .paths
        .evidence
        .join("reindex")
        .join(format!("{}.json", plan.verdict_id));
    require_recovery_absent(&runtime.ops, &receipt)?;
    let current = snapshot
        .paths
        .canon
        .join(format!(".current-{}", plan.transaction_id));
    require_recovery_absent(&runtime.ops, &current)?;

    let Some(generation_id) = plan.generation_id.as_deref() else {
        for path in [
            snapshot
                .paths
                .canon
                .join("generations")
                .join(format!(".txn-{}", plan.transaction_id)),
            snapshot
                .paths
                .derived
                .join("indexes")
                .join(format!(".txn-{}", plan.transaction_id)),
        ] {
            require_recovery_absent(&runtime.ops, &path)?;
        }
        return Ok(RecoveryCleanupInventory::default());
    };
    let final_generation = snapshot.paths.canon.join("generations").join(generation_id);
    let final_index = snapshot.paths.derived.join("indexes").join(generation_id);
    require_recovery_absent(&runtime.ops, &final_generation)?;
    require_recovery_absent(&runtime.ops, &final_index)?;

    let staged_generation = snapshot
        .paths
        .canon
        .join("generations")
        .join(format!(".txn-{}", plan.transaction_id));
    let staged_index = snapshot
        .paths
        .derived
        .join("indexes")
        .join(format!(".txn-{}", plan.transaction_id));
    let mut cleanup = RecoveryCleanupInventory::default();
    let generation = match runtime.ops.metadata(&staged_generation) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(TransactionError::RecoveryAmbiguous)
        }
        Ok(_) => Some(
            verify_generation(profile, &staged_generation, VerificationLevel::Three)
                .map_err(|_| TransactionError::RecoveryAmbiguous)?,
        ),
        Err(_) => return Err(TransactionError::RecoveryAmbiguous),
    };
    if let Some(generation) = generation {
        validate_generation_against_plan(snapshot, profile, plan, &generation)?;
        let binding = RecoveryGenerationBinding::new(
            staged_generation.clone(),
            profile,
            snapshot,
            plan,
            None,
        );
        cleanup.bind_directory(RecoveryCleanupDirectory::Generation(binding.clone()))?;
        match runtime.ops.metadata(&staged_index) {
            Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {
                super::open_p3_index_generation(profile, &generation, &staged_index)
                    .map_err(|_| TransactionError::RecoveryAmbiguous)?;
                cleanup.bind_directory(RecoveryCleanupDirectory::Index(RecoveryIndexBinding {
                    path: staged_index.clone(),
                    generation: binding,
                    receipt: None,
                }))?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(TransactionError::RecoveryAmbiguous),
        }
    } else if runtime.ops.metadata(&staged_index).is_ok() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(cleanup)
}

fn validate_prepared_residue<O, F>(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    runtime: &TransactionRuntime<O, F>,
    plan: &TransactionPlan,
    prepared: &PreparedJournal,
) -> Result<RecoveryCleanupInventory, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let staged_generation = snapshot
        .paths
        .canon
        .join("generations")
        .join(format!(".txn-{}", plan.transaction_id));
    let staged_index = snapshot
        .paths
        .derived
        .join("indexes")
        .join(format!(".txn-{}", plan.transaction_id));
    let current = snapshot
        .paths
        .canon
        .join(format!(".current-{}", plan.transaction_id));
    require_recovery_absent(&runtime.ops, &current)?;

    let receipt_path = snapshot
        .paths
        .evidence
        .join("reindex")
        .join(format!("{}.json", plan.verdict_id));
    if !matches!(plan.verb.as_str(), "ACCEPT" | "REWRITE") {
        for path in [staged_generation, staged_index, receipt_path] {
            require_recovery_absent(&runtime.ops, &path)?;
        }
        return Ok(RecoveryCleanupInventory::default());
    }

    let generation_id = plan
        .generation_id
        .as_deref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let final_generation = snapshot.paths.canon.join("generations").join(generation_id);
    let final_index = snapshot.paths.derived.join("indexes").join(generation_id);
    let receipt = read_optional_recovery_regular_file(&runtime.ops, &receipt_path)?;
    let temporary_ledger_present = !recovery_ledger_temporaries(&snapshot.paths.ledger)?.is_empty();

    let (generation_path, index_path) = if receipt.is_none() {
        if temporary_ledger_present {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        require_recovery_absent(&runtime.ops, &final_generation)?;
        require_recovery_absent(&runtime.ops, &final_index)?;
        let staged_generation_present = match runtime.ops.metadata(&staged_generation) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => true,
            _ => return Err(TransactionError::RecoveryAmbiguous),
        };
        let staged_index_present = match runtime.ops.metadata(&staged_index) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => true,
            _ => return Err(TransactionError::RecoveryAmbiguous),
        };
        if !staged_generation_present && !staged_index_present {
            return Ok(RecoveryCleanupInventory::default());
        }
        if !staged_generation_present || !staged_index_present {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        (staged_generation, staged_index)
    } else {
        (
            exactly_one_recovery_directory(&runtime.ops, &staged_generation, &final_generation)?,
            exactly_one_recovery_directory(&runtime.ops, &staged_index, &final_index)?,
        )
    };
    let generation = verify_generation(profile, &generation_path, VerificationLevel::Three)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    validate_generation_against_plan(snapshot, profile, plan, &generation)?;
    if generation.manifest_hash
        != prepared
            .artifacts
            .manifest_hash
            .as_deref()
            .unwrap_or_default()
        || generation.bundle_hash
            != prepared
                .artifacts
                .bundle_hash
                .as_deref()
                .unwrap_or_default()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    super::open_p3_index_generation(profile, &generation, &index_path)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;

    let binding = RecoveryGenerationBinding::new(
        generation_path.clone(),
        profile,
        snapshot,
        plan,
        Some(prepared),
    );
    let receipt_binding = receipt.as_ref().map(|bytes| RecoveryCleanupFile {
        path: receipt_path.clone(),
        bytes: bytes.clone(),
    });
    if let Some(receipt_bytes) = receipt.as_ref() {
        if blake3::hash(receipt_bytes).to_hex().as_str()
            != prepared
                .artifacts
                .reindex_receipt_hash
                .as_deref()
                .unwrap_or_default()
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        let receipt = serde_json::from_slice(receipt_bytes)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        super::verify_reindex_receipt(profile, &generation, &index_path, &receipt, receipt_bytes)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    }
    let mut cleanup = RecoveryCleanupInventory::default();
    cleanup.bind_directory(RecoveryCleanupDirectory::Generation(binding.clone()))?;
    cleanup.bind_directory(RecoveryCleanupDirectory::Index(RecoveryIndexBinding {
        path: index_path,
        generation: binding,
        receipt: receipt_binding,
    }))?;
    if let Some(receipt_bytes) = receipt {
        cleanup.bind_file(receipt_path, receipt_bytes)?;
    }
    Ok(cleanup)
}

pub fn validate_generation_against_plan(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    plan: &TransactionPlan,
    generation: &super::VerifiedGeneration,
) -> Result<(), TransactionError> {
    validate_generation_against_plan_for_committed_head(snapshot, profile, plan, generation, false)
}

fn validate_committed_generation_against_plan(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    plan: &TransactionPlan,
    generation: &super::VerifiedGeneration,
) -> Result<(), TransactionError> {
    validate_generation_against_plan_for_committed_head(snapshot, profile, plan, generation, true)
}

fn validate_generation_against_plan_for_committed_head(
    snapshot: &StoreSnapshot,
    profile: &P3Profile,
    plan: &TransactionPlan,
    generation: &super::VerifiedGeneration,
    allow_resulting_head: bool,
) -> Result<(), TransactionError> {
    let generation_id = plan
        .generation_id
        .as_deref()
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let head_matches_plan = plan.parent_ref == snapshot.head.as_str()
        || (allow_resulting_head && generation_id == snapshot.head.as_str());
    if !head_matches_plan
        || generation.manifest.generation_id != generation_id
        || generation.manifest.profile_hash != plan.profile_hash
        || generation.manifest.policy_hash != plan.policy_hash
        || generation.manifest.applied_proposal_id != plan.proposal_id
        || generation.manifest.applied_verdict_id != plan.verdict_id
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if plan.parent_ref == "seed" {
        if generation.manifest.parent_kind != super::ParentKind::Seed
            || generation.manifest.parent_generation_id.is_some()
            || generation.manifest.parent_manifest_hash != plan.parent_manifest_hash
            || generation.manifest.seed_manifest_hash != snapshot.seed.manifest_hash
            || generation.manifest.seed_bundle_hash != plan.parent_bundle_hash
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    } else {
        let parent = verify_generation(
            profile,
            &snapshot
                .paths
                .canon
                .join("generations")
                .join(&plan.parent_ref),
            VerificationLevel::Three,
        )
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if generation.manifest.parent_kind != super::ParentKind::Generation
            || generation.manifest.parent_generation_id.as_deref() != Some(&plan.parent_ref)
            || generation.manifest.parent_manifest_hash != plan.parent_manifest_hash
            || parent.manifest_hash != plan.parent_manifest_hash
            || parent.bundle_hash != plan.parent_bundle_hash
            || generation.manifest.seed_manifest_hash != parent.manifest.seed_manifest_hash
            || generation.manifest.seed_bundle_hash != parent.manifest.seed_bundle_hash
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    let judgment_path = format!("judgments/{}.md", plan.verdict_id);
    let entries = generation
        .manifest
        .documents
        .iter()
        .filter(|entry| {
            entry.proposal_id.as_deref() == Some(plan.proposal_id.as_str())
                && entry.verdict_id.as_deref() == Some(plan.verdict_id.as_str())
        })
        .collect::<Vec<_>>();
    let judgment = entries
        .iter()
        .copied()
        .find(|entry| entry.canonical_path == judgment_path)
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let targets = entries
        .into_iter()
        .filter(|entry| entry.canonical_path != judgment_path)
        .collect::<Vec<_>>();
    if targets.len() != 1
        || judgment.hash != plan.judgment_hash.as_deref().unwrap_or_default()
        || targets[0].canonical_path != plan.target_path
        || targets[0].id != plan.target_id
        || targets[0].hash != plan.approved_target_hash.as_deref().unwrap_or_default()
        || targets[0].previous_hash != plan.base_target_hash
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let document = generation
        .generated_documents
        .iter()
        .find(|document| document.metadata.source_path == judgment_path)
        .ok_or(TransactionError::RecoveryAmbiguous)?;
    let ruled_at =
        Timestamp::parse(&plan.ruled_at).map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if document.source_hash != plan.judgment_hash.as_deref().unwrap_or_default()
        || document.metadata.verdict.as_deref() != Some(plan.verb.as_str())
        || document.metadata.subject.as_deref() != Some(plan.target_id.as_str())
        || document
            .metadata
            .reason
            .as_deref()
            .map(|reason| blake3::hash(reason.as_bytes()).to_hex().to_string())
            .as_deref()
            != Some(plan.reason_hash.as_str())
        || document.metadata.created != ruled_at.utc_date()
        || document.metadata.updated != ruled_at.utc_date()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(())
}

fn require_recovery_absent(ops: &impl FilesystemOps, path: &Path) -> Result<(), TransactionError> {
    match ops.metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(TransactionError::RecoveryAmbiguous),
    }
}

fn read_optional_recovery_regular_file(
    ops: &impl FilesystemOps,
    path: &Path,
) -> Result<Option<Vec<u8>>, TransactionError> {
    match ops.metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(TransactionError::RecoveryAmbiguous)
        }
        Ok(_) => ops
            .read(path)
            .map(Some)
            .map_err(|_| TransactionError::RecoveryAmbiguous),
        Err(_) => Err(TransactionError::RecoveryAmbiguous),
    }
}

fn require_recovery_regular_directory(
    ops: &impl FilesystemOps,
    path: &Path,
) -> Result<(), TransactionError> {
    let metadata = ops
        .metadata(path)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(())
}

fn exactly_one_recovery_directory(
    ops: &impl FilesystemOps,
    first: &Path,
    second: &Path,
) -> Result<PathBuf, TransactionError> {
    let mut present = Vec::new();
    for path in [first, second] {
        match ops.metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {
                present.push(path.to_owned())
            }
            _ => return Err(TransactionError::RecoveryAmbiguous),
        }
    }
    if present.len() == 1 {
        Ok(present.remove(0))
    } else {
        Err(TransactionError::RecoveryAmbiguous)
    }
}

fn validate_prepared_successor_temporary<O, F>(
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    plan: &TransactionPlan,
    prepared: &PreparedJournal,
) -> Result<RecoveryCleanupInventory, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if !prepared.validates_against(plan, &blake3::hash(&canonical_json_bytes(plan)?).to_hex()) {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let temporaries = recovery_ledger_temporaries(&snapshot.paths.ledger)?;
    if temporaries.len() > 1 {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let Some(temporary) = temporaries.into_iter().next() else {
        return Ok(RecoveryCleanupInventory::default());
    };
    let metadata = runtime
        .ops
        .metadata(&temporary)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let successor = runtime
        .ops
        .read(&temporary)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if blake3::hash(&successor).to_hex().as_str() != prepared.artifacts.ledger_successor_hash {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let current_metadata = runtime
        .ops
        .metadata(&snapshot.paths.ledger)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if current_metadata.file_type().is_symlink() || !current_metadata.is_file() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let current = runtime
        .ops
        .read(&snapshot.paths.ledger)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if !successor.starts_with(&current) {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let current_view =
        read_ledger(&snapshot.paths.ledger).map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let successor_view =
        read_ledger(&temporary).map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let additional_events = usize::from(current_view.events.is_empty()) + 1;
    if successor_view.events.len() != current_view.events.len() + additional_events {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let event = match successor_view.events.last() {
        Some(LedgerEvent::Verdict(event)) => event,
        _ => return Err(TransactionError::RecoveryAmbiguous),
    };
    let payload = event.payload();
    let expected_sequence = u64::try_from(successor_view.events.len() - 1)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if payload.sequence != expected_sequence
        || payload.event_id != format!("evt-{expected_sequence:016}")
        || event.hash() != prepared.artifacts.verdict_event_hash
        || payload.transaction_id != plan.transaction_id
        || payload.proposal_id != plan.proposal_id
        || payload.verdict_id != plan.verdict_id
        || payload.verb.as_str() != plan.verb
        || payload.occurred_at != plan.ruled_at
        || blake3::hash(payload.reason.as_bytes()).to_hex().as_str() != plan.reason_hash
        || payload.profile_hash != plan.profile_hash
        || payload.policy_hash != plan.policy_hash
        || payload.proposal_meta_hash != plan.proposal_meta_hash
        || payload.candidate_hash != plan.candidate_hash
        || payload.base_manifest_hash != plan.parent_manifest_hash
        || payload.base_bundle_hash != plan.parent_bundle_hash
        || payload.base_target_hash != plan.base_target_hash
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let applying = matches!(plan.verb.as_str(), "ACCEPT" | "REWRITE");
    if applying {
        if payload.base_kind
            != if plan.parent_ref == "seed" {
                "seed"
            } else {
                "generation"
            }
            || payload.base_generation.as_deref()
                != (plan.parent_ref != "seed").then_some(plan.parent_ref.as_str())
            || payload.resulting_canonical_head != plan.generation_id.as_deref().unwrap_or_default()
            || payload.canonical_result.generation_id.as_deref() != plan.generation_id.as_deref()
            || payload.canonical_result.manifest_hash.as_deref()
                != prepared.artifacts.manifest_hash.as_deref()
            || payload.canonical_result.bundle_hash.as_deref()
                != prepared.artifacts.bundle_hash.as_deref()
            || payload.canonical_result.target_prior_hash != plan.base_target_hash
            || payload.canonical_result.target_new_hash.as_deref()
                != plan.approved_target_hash.as_deref()
            || payload.canonical_result.judgment_hash.as_deref() != plan.judgment_hash.as_deref()
            || payload.canonical_result.reindex_receipt_hash.as_deref()
                != prepared.artifacts.reindex_receipt_hash.as_deref()
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    } else if payload.base_kind
        != if plan.parent_ref == "seed" {
            "seed"
        } else {
            "generation"
        }
        || payload.base_generation.as_deref()
            != (plan.parent_ref != "seed").then_some(plan.parent_ref.as_str())
        || payload.resulting_canonical_head != snapshot.head.as_str()
        || payload.canonical_result != CanonicalResult::none()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let mut cleanup = RecoveryCleanupInventory::default();
    cleanup.bind_file(temporary, successor)?;
    Ok(cleanup)
}

fn recover_intake_only<O, F>(
    snapshot: &StoreSnapshot,
    runtime: &TransactionRuntime<O, F>,
    quarantined: &QuarantinedLeases,
    recovery_journal: &RecoveryJournal,
    recovery_lease: &Lease,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if quarantined.verdict.is_some() || quarantined.intake.is_none() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    recovery_journal.verify(&runtime.ops)?;
    let transaction_entries = sorted_directory_entries(&snapshot.paths.transactions)?;
    if transaction_entries.len() != 1 || transaction_entries[0].path() != recovery_journal.root {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let intake_owner_hash = &quarantined
        .intake
        .as_ref()
        .expect("checked above")
        .stale
        .owner_hash;
    recovery_journal
        .record
        .cleanup_plan
        .validate_intake_owner(intake_owner_hash)?;
    recover_incomplete_intake_transaction(
        &snapshot.paths,
        snapshot.policy.policy.max_candidate_bytes,
        || {
            recovery_lease
                .verify_with(&runtime.ops)
                .map_err(|_| IntakeError::Locked)
        },
    )
    .map_err(|_| TransactionError::IntakeIncomplete)?;
    Ok(())
}

fn prepare_intake_cleanup_plan(
    _ops: &impl FilesystemOps,
    _paths: &store::StorePaths,
    intake: &StaleLease,
) -> Result<RecoveryCleanupPlan, TransactionError> {
    let plan = RecoveryCleanupPlan::empty();
    plan.validate_intake_owner(&intake.owner_hash)?;
    Ok(plan)
}

fn validate_intake_temporary(
    ops: &impl FilesystemOps,
    temporary: &Path,
    name: &str,
) -> Result<(), TransactionError> {
    require_recovery_regular_directory(ops, temporary)?;
    let entries = sorted_directory_entries(temporary)?;
    let names = entries
        .iter()
        .map(|entry| {
            entry
                .file_name()
                .into_string()
                .map_err(|_| TransactionError::RecoveryAmbiguous)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let valid_prefix = names.is_empty()
        || names == ["candidate.md"]
        || names == ["candidate.md", "proposal.meta.json"];
    if !valid_prefix {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    for entry in &entries {
        let metadata = ops
            .metadata(&entry.path())
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    let Some(candidate) = entries.first() else {
        return Ok(());
    };
    let candidate_bytes = ops
        .read(&candidate.path())
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if std::str::from_utf8(&candidate_bytes).is_err() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    if entries.len() == 2 {
        let meta_bytes = ops
            .read(&entries[1].path())
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        let proposal_id = name
            .strip_prefix(".proposal-")
            .and_then(|value| value.get(..68))
            .ok_or(TransactionError::RecoveryAmbiguous)?;
        verify_proposal_pair_bytes(proposal_id, candidate_bytes, meta_bytes)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    }
    Ok(())
}

fn verify_recovery_store(
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    recovery_lease: &Lease,
) -> Result<(), TransactionError> {
    recovery_lease.verify()?;
    let allowance = VerificationAllowance {
        verdict_lease_token: None,
        intake_lease_token: None,
        recovery_verdict_lease_token: Some(recovery_lease.token().to_owned()),
    };
    if snapshot.sealed {
        if verify_store_level4_with_allowance(profile, &snapshot.paths.root, &allowance).is_err() {
            recovery_lease.verify()?;
            return Err(TransactionError::RecoveryAmbiguous);
        }
    } else {
        let refreshed = match store::inspect_store_with_transients(profile, &snapshot.paths.root) {
            Ok(refreshed) => refreshed,
            Err(_) => {
                recovery_lease.verify()?;
                return Err(TransactionError::RecoveryAmbiguous);
            }
        };
        if verify_preflight_transient_state(&refreshed.paths, &allowance).is_err() {
            recovery_lease.verify()?;
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Each hash is framed separately in the transaction identity.
pub fn recovery_transaction_id(
    owner_hashes: &[String],
    recovered_at: &Timestamp,
    observed_ledger_head: Option<&str>,
    command_class: &str,
    original_journal_hash: Option<&str>,
    profile_hash: &str,
    policy_hash: &str,
    cleanup_plan_hash: &str,
) -> String {
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, RECOVERY_TRANSACTION_DOMAIN);
    frame(&mut hasher, command_class.as_bytes());
    for hash in owner_hashes {
        frame(&mut hasher, hash.as_bytes());
    }
    frame(&mut hasher, recovered_at.as_str().as_bytes());
    frame_optional(&mut hasher, observed_ledger_head.map(str::as_bytes));
    frame_optional(&mut hasher, original_journal_hash.map(str::as_bytes));
    frame(&mut hasher, profile_hash.as_bytes());
    frame(&mut hasher, policy_hash.as_bytes());
    frame(&mut hasher, cleanup_plan_hash.as_bytes());
    format!("txn-{}", hasher.finalize().to_hex())
}

fn recovery_source_command_class(
    mode: RecoveryMode,
    stale: &RecoveryStaleLeases,
) -> Result<&str, TransactionError> {
    let mut owners = stale.owners();
    let first = owners.next().ok_or(TransactionError::RecoveryAmbiguous)?;
    let command_class = first.owner.command_class();
    if owners.any(|owner| owner.owner.command_class() != command_class) {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let valid = match mode {
        RecoveryMode::IntakeOnly => matches!(command_class, "propose" | "intake-seal"),
        RecoveryMode::VerdictTransaction | RecoveryMode::CleanFinalization => {
            command_class == "verdict"
        }
    };
    if !valid {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(command_class)
}

fn validate_recovery_record_identity(
    profile: &P3Profile,
    snapshot: &StoreSnapshot,
    record: &RecoveryJournalRecord,
) -> Result<Vec<String>, TransactionError> {
    if record.profile_hash != profile.hash
        || record.profile_hash != snapshot.profile_hash
        || record.policy_hash != snapshot.policy.hash
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let recovered_at =
        Timestamp::parse(&record.recovered_at).map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let recorded_owner_hashes = recovery_owner_hashes(&record.quarantined_leases);
    if recovery_transaction_id(
        &recorded_owner_hashes,
        &recovered_at,
        record.observed_ledger_head.as_deref(),
        &record.command_class,
        record.original_journal_hash.as_deref(),
        &record.profile_hash,
        &record.policy_hash,
        &record.cleanup_plan_hash,
    ) != record.recovery_id
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(recorded_owner_hashes)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct RecoveryLeaseBinding {
    pub kind: RecoveryLeaseKind,
    pub owner_hash: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryLeaseKind {
    Intake,
    Verdict,
}

impl RecoveryLeaseKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Verdict => "verdict",
            Self::Intake => "intake",
        }
    }
}

impl RecoveryLeaseBinding {
    pub fn validates(&self) -> bool {
        valid_hash(&self.owner_hash)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryJournalRecord {
    pub schema_version: String,
    pub recovery_id: String,
    pub mode: RecoveryMode,
    pub command_class: String,
    pub recovered_at: String,
    pub quarantined_leases: Vec<RecoveryLeaseBinding>,
    pub recovery_lease_owner_hash: String,
    pub original_transaction_id: Option<String>,
    pub original_journal_hash: Option<String>,
    pub observed_ledger_head: Option<String>,
    pub profile_hash: String,
    pub policy_hash: String,
    pub cleanup_plan: RecoveryCleanupPlan,
    pub cleanup_plan_hash: String,
}

impl RecoveryJournalRecord {
    pub fn validate(&self) -> Result<(), TransactionError> {
        if !valid_transaction_id(&self.recovery_id)
            || self.schema_version != RECOVERY_JOURNAL_SCHEMA
            || Timestamp::parse(&self.recovered_at).is_err()
            || !matches!(
                self.command_class.as_str(),
                "verdict" | "propose" | "intake-seal"
            )
            || (self.mode == RecoveryMode::IntakeOnly)
                != matches!(self.command_class.as_str(), "propose" | "intake-seal")
            || self.quarantined_leases.is_empty()
            || self
                .quarantined_leases
                .iter()
                .any(|binding| !binding.validates())
            || self
                .quarantined_leases
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self
                .quarantined_leases
                .windows(2)
                .any(|pair| pair[0].kind == pair[1].kind)
            || !valid_hash(&self.recovery_lease_owner_hash)
            || self
                .original_transaction_id
                .as_deref()
                .is_some_and(|id| !valid_transaction_id(id))
            || self
                .original_journal_hash
                .as_deref()
                .is_some_and(|hash| !valid_hash(hash))
            || self
                .observed_ledger_head
                .as_deref()
                .is_some_and(|hash| !valid_hash(hash))
            || !valid_hash(&self.profile_hash)
            || !valid_hash(&self.policy_hash)
            || self
                .cleanup_plan
                .validate_for_recovery(self.mode, &self.quarantined_leases)
                .is_err()
            || self.cleanup_plan.hash().ok().as_deref() != Some(&self.cleanup_plan_hash)
            || self.recovery_id
                != recovery_transaction_id(
                    &recovery_owner_hashes(&self.quarantined_leases),
                    &Timestamp::parse(&self.recovered_at)
                        .map_err(|_| TransactionError::RecoveryAmbiguous)?,
                    self.observed_ledger_head.as_deref(),
                    &self.command_class,
                    self.original_journal_hash.as_deref(),
                    &self.profile_hash,
                    &self.policy_hash,
                    &self.cleanup_plan_hash,
                )
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        Ok(())
    }
}

pub struct RecoveryJournal {
    pub root: PathBuf,
    pub record: RecoveryJournalRecord,
    pub bytes: Vec<u8>,
    pub cleanup_form: bool,
}

impl RecoveryJournal {
    pub fn discover(
        ops: &impl FilesystemOps,
        transactions: &Path,
    ) -> Result<Option<Self>, TransactionError> {
        let mut entries = Vec::new();
        for entry in sorted_directory_entries(transactions)? {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| TransactionError::RecoveryAmbiguous)?;
            let (transaction_id, cleanup_form) = if valid_transaction_id(&name) {
                (name, false)
            } else if let Some(id) = name.strip_prefix(".cleanup-") {
                if !valid_transaction_id(id) {
                    return Err(TransactionError::RecoveryAmbiguous);
                }
                (id.to_owned(), true)
            } else {
                continue;
            };
            let metadata = ops
                .metadata(&entry.path())
                .map_err(|_| TransactionError::RecoveryAmbiguous)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(TransactionError::RecoveryAmbiguous);
            }
            let names = recovery_journal_entry_names(ops, &entry.path())?;
            entries.push((transaction_id, entry.path(), cleanup_form, names));
        }
        let mut recovery_ids = entries
            .iter()
            .filter(|(_, _, _, names)| names.iter().any(|name| name == "recovery.json"))
            .map(|(id, _, _, _)| id.clone())
            .collect::<Vec<_>>();
        recovery_ids.sort();
        recovery_ids.dedup();
        if recovery_ids.len() > 1 {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        let Some(recovery_id) = recovery_ids.first() else {
            return Ok(None);
        };
        let mut regular = None;
        let mut cleanup = None;
        for (id, root, cleanup_form, names) in entries {
            if id != *recovery_id {
                continue;
            }
            if cleanup_form {
                if cleanup.replace((root, names)).is_some() {
                    return Err(TransactionError::RecoveryAmbiguous);
                }
            } else if regular.replace((root, names)).is_some() {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }
        match (regular, cleanup) {
            (Some((root, names)), None) => {
                Self::read_exact(ops, root, recovery_id.clone(), false, names).map(Some)
            }
            (None, Some((root, names))) => {
                Self::read_exact(ops, root, recovery_id.clone(), true, names).map(Some)
            }
            (Some((regular_root, regular_names)), Some((cleanup_root, cleanup_names))) => {
                read_recovery_journal_transition(
                    ops,
                    recovery_id.clone(),
                    regular_root,
                    regular_names,
                    cleanup_root,
                    cleanup_names,
                )
                .map(Some)
            }
            (None, None) => Ok(None),
        }
    }

    fn read_exact(
        ops: &impl FilesystemOps,
        root: PathBuf,
        transaction_id: String,
        cleanup_form: bool,
        names: Vec<String>,
    ) -> Result<Self, TransactionError> {
        if names != ["recovery.json"] && names != ["published.json", "recovery.json"] {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        for name in &names {
            let metadata = ops
                .metadata(&root.join(name))
                .map_err(|_| TransactionError::RecoveryAmbiguous)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }
        let bytes = read_recovery_journal_regular_file(
            ops,
            &root.join("recovery.json"),
            TransactionError::RecoveryAmbiguous,
        )?;
        let record: RecoveryJournalRecord =
            serde_json::from_slice(&bytes).map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if record.recovery_id != transaction_id
            || canonical_json_bytes(&record)? != bytes
            || record.validate().is_err()
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        let journal = Self {
            root,
            record,
            bytes,
            cleanup_form,
        };
        if names.len() == 2 {
            journal.verify_published_marker(ops)?;
        }
        Ok(journal)
    }

    pub fn verify_published_marker(
        &self,
        ops: &impl FilesystemOps,
    ) -> Result<(), TransactionError> {
        let marker_bytes = read_recovery_journal_regular_file(
            ops,
            &self.root.join("published.json"),
            TransactionError::RecoveryAmbiguous,
        )?;
        let marker: JournalMarker = serde_json::from_slice(&marker_bytes)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        let expected = JournalMarker {
            schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
            transaction_id: self.record.recovery_id.clone(),
            stage: "published".to_owned(),
        };
        if marker != expected || canonical_json_bytes(&marker)? != marker_bytes {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        Ok(())
    }

    pub fn is_published(&self, ops: &impl FilesystemOps) -> Result<bool, TransactionError> {
        match ops.metadata(&self.root.join("published.json")) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                Err(TransactionError::RecoveryAmbiguous)
            }
            Ok(_) => {
                self.verify_published_marker(ops)?;
                Ok(true)
            }
            Err(_) => Err(TransactionError::RecoveryAmbiguous),
        }
    }

    pub fn create<O, F>(
        runtime: &TransactionRuntime<O, F>,
        record: RecoveryJournalRecord,
    ) -> Result<Self, TransactionError>
    where
        O: FilesystemOps,
        F: FaultInjector,
    {
        record.validate()?;
        let root = runtime.root.join("transactions").join(&record.recovery_id);
        runtime
            .ops
            .create_dir(&root)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        runtime
            .ops
            .sync_directory(&root)
            .map_err(|_| TransactionError::Write)?;
        runtime
            .ops
            .sync_directory(&runtime.root.join("transactions"))
            .map_err(|_| TransactionError::Write)?;
        let bytes = canonical_json_bytes(&record)?;
        runtime
            .ops
            .write_new_synced(&root.join("recovery.json"), &bytes)
            .map_err(|_| TransactionError::Write)?;
        runtime
            .ops
            .sync_directory(&root)
            .map_err(|_| TransactionError::Write)?;
        runtime
            .ops
            .sync_directory(&runtime.root.join("transactions"))
            .map_err(|_| TransactionError::Write)?;
        Ok(Self {
            root,
            record,
            bytes,
            cleanup_form: false,
        })
    }

    pub fn write_published(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        self.verify(ops)?;
        let marker = canonical_json_bytes(&JournalMarker {
            schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
            transaction_id: self.record.recovery_id.clone(),
            stage: "published".to_owned(),
        })?;
        ops.write_new_synced(&self.root.join("published.json"), &marker)
            .map_err(|_| TransactionError::Write)?;
        ops.sync_directory(&self.root)
            .map_err(|_| TransactionError::Write)?;
        ops.sync_directory(self.root.parent().ok_or(TransactionError::Write)?)
            .map_err(|_| TransactionError::Write)
    }

    fn create_protected<O, F>(
        runtime: &TransactionRuntime<O, F>,
        record: RecoveryJournalRecord,
        recovery_lease: &Lease,
    ) -> Result<Self, TransactionError>
    where
        O: FilesystemOps,
        F: FaultInjector,
    {
        record.validate()?;
        let root = runtime.root.join("transactions").join(&record.recovery_id);
        recovery_lease.verify_with(&runtime.ops)?;
        runtime
            .ops
            .create_dir(&root)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        runtime
            .ops
            .sync_directory(&root)
            .map_err(|_| TransactionError::Write)?;
        runtime
            .ops
            .sync_directory(&runtime.root.join("transactions"))
            .map_err(|_| TransactionError::Write)?;
        let bytes = canonical_json_bytes(&record)?;
        recovery_lease.verify_with(&runtime.ops)?;
        runtime
            .ops
            .write_new_synced(&root.join("recovery.json"), &bytes)
            .map_err(|_| TransactionError::Write)?;
        runtime
            .ops
            .sync_directory(&root)
            .map_err(|_| TransactionError::Write)?;
        runtime
            .ops
            .sync_directory(&runtime.root.join("transactions"))
            .map_err(|_| TransactionError::Write)?;
        Ok(Self {
            root,
            record,
            bytes,
            cleanup_form: false,
        })
    }

    pub fn write_published_protected(
        &self,
        ops: &impl FilesystemOps,
        recovery_lease: &Lease,
    ) -> Result<(), TransactionError> {
        self.verify(ops)?;
        let marker = canonical_json_bytes(&JournalMarker {
            schema_version: RECOVERY_JOURNAL_SCHEMA.to_owned(),
            transaction_id: self.record.recovery_id.clone(),
            stage: "published".to_owned(),
        })?;
        recovery_lease.verify_with(ops)?;
        ops.write_new_synced(&self.root.join("published.json"), &marker)
            .map_err(|_| TransactionError::Write)?;
        ops.sync_directory(&self.root)
            .map_err(|_| TransactionError::Write)?;
        ops.sync_directory(self.root.parent().ok_or(TransactionError::Write)?)
            .map_err(|_| TransactionError::Write)
    }

    pub fn cleanup_with(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        let parent = self.root.parent().ok_or(TransactionError::Write)?;
        let current = Self::discover(ops, parent)
            .map_err(|_| TransactionError::JournalState)?
            .ok_or(TransactionError::JournalState)?;
        if current.record != self.record || current.bytes != self.bytes {
            return Err(TransactionError::JournalState);
        }
        let cleanup_witness = current.promote_to_cleanup_witness(ops)?;
        cleanup_witness.remove_cleanup_witness_with(ops)
    }

    pub fn promote_to_cleanup_witness(
        &self,
        ops: &impl FilesystemOps,
    ) -> Result<Self, TransactionError> {
        self.promote_to_cleanup_witness_with_lease(ops, None)
    }

    pub fn promote_to_cleanup_witness_protected(
        &self,
        ops: &impl FilesystemOps,
        recovery_lease: &Lease,
    ) -> Result<Self, TransactionError> {
        self.promote_to_cleanup_witness_with_lease(ops, Some(recovery_lease))
    }

    fn promote_to_cleanup_witness_with_lease(
        &self,
        ops: &impl FilesystemOps,
        recovery_lease: Option<&Lease>,
    ) -> Result<Self, TransactionError> {
        let parent = self.root.parent().ok_or(TransactionError::Write)?;
        let current = Self::discover(ops, parent)
            .map_err(|_| TransactionError::JournalState)?
            .ok_or(TransactionError::JournalState)?;
        if current.record != self.record || current.bytes != self.bytes {
            return Err(TransactionError::JournalState);
        }
        if current.cleanup_form {
            return finish_recovery_source_removal(ops, &current, recovery_lease);
        }
        current.verify(ops)?;
        current
            .verify_published_marker(ops)
            .map_err(|_| TransactionError::JournalState)?;
        let cleanup = parent.join(format!(".cleanup-{}", current.record.recovery_id));
        match ops.metadata(&cleanup) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Some(lease) = recovery_lease {
                    lease.verify_with(ops)?;
                }
                ops.create_dir(&cleanup)
                    .map_err(|_| TransactionError::JournalState)?;
                ops.sync_directory(parent)
                    .map_err(|_| TransactionError::Write)?;
            }
            Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {}
            _ => return Err(TransactionError::JournalState),
        }
        for name in ["recovery.json", "published.json"] {
            let source = current.root.join(name);
            let destination = cleanup.join(name);
            match ops.metadata(&destination) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    if let Some(lease) = recovery_lease {
                        lease.verify_with(ops)?;
                    }
                    ops.hard_link(&source, &destination)
                        .map_err(|_| TransactionError::JournalState)?
                }
                Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_file() => {
                    if read_recovery_journal_regular_file(
                        ops,
                        &source,
                        TransactionError::JournalState,
                    )? != read_recovery_journal_regular_file(
                        ops,
                        &destination,
                        TransactionError::JournalState,
                    )? {
                        return Err(TransactionError::JournalState);
                    }
                }
                _ => return Err(TransactionError::JournalState),
            }
        }
        ops.sync_directory(&cleanup)
            .map_err(|_| TransactionError::Write)?;
        ops.sync_directory(parent)
            .map_err(|_| TransactionError::Write)?;
        for name in ["recovery.json", "published.json"] {
            let source = current.root.join(name);
            if read_recovery_journal_regular_file(ops, &source, TransactionError::JournalState)?
                != read_recovery_journal_regular_file(
                    ops,
                    &cleanup.join(name),
                    TransactionError::JournalState,
                )?
            {
                return Err(TransactionError::JournalState);
            }
            if let Some(lease) = recovery_lease {
                lease.verify_with(ops)?;
            }
            ops.remove_file(&source)
                .map_err(|_| TransactionError::Write)?;
        }
        if let Some(lease) = recovery_lease {
            lease.verify_with(ops)?;
        }
        ops.remove_dir(&current.root)
            .map_err(|_| TransactionError::Write)?;
        ops.sync_directory(parent)
            .map_err(|_| TransactionError::Write)?;
        Self::read_exact(
            ops,
            cleanup,
            current.record.recovery_id.clone(),
            true,
            vec!["published.json".to_owned(), "recovery.json".to_owned()],
        )
        .map_err(|_| TransactionError::JournalState)
    }

    pub fn remove_cleanup_witness_with(
        &self,
        ops: &impl FilesystemOps,
    ) -> Result<(), TransactionError> {
        self.remove_cleanup_witness_with_lease(ops, None)
    }

    pub fn remove_cleanup_witness_with_protected(
        &self,
        ops: &impl FilesystemOps,
        recovery_lease: &Lease,
    ) -> Result<(), TransactionError> {
        self.remove_cleanup_witness_with_lease(ops, Some(recovery_lease))
    }

    fn remove_cleanup_witness_with_lease(
        &self,
        ops: &impl FilesystemOps,
        recovery_lease: Option<&Lease>,
    ) -> Result<(), TransactionError> {
        let parent = self.root.parent().ok_or(TransactionError::Write)?;
        let current = Self::discover(ops, parent)
            .map_err(|_| TransactionError::JournalState)?
            .ok_or(TransactionError::JournalState)?;
        if current.record != self.record || current.bytes != self.bytes || !current.cleanup_form {
            return Err(TransactionError::JournalState);
        }
        let current = finish_recovery_source_removal(ops, &current, recovery_lease)?;
        current.verify(ops)?;
        let terminal_residue = is_terminal_recovery_cleanup_residue(ops, &current)
            .map_err(|_| TransactionError::JournalState)?;
        if !terminal_residue {
            current
                .verify_published_marker(ops)
                .map_err(|_| TransactionError::JournalState)?;
        }
        ops.sync_directory(parent)
            .map_err(|_| TransactionError::Write)?;
        let removal_order: &[&str] = if terminal_residue {
            &["recovery.json"]
        } else {
            &["published.json", "recovery.json"]
        };
        for name in removal_order {
            if let Some(lease) = recovery_lease {
                lease.verify_with(ops)?;
            }
            ops.remove_file(&current.root.join(name))
                .map_err(|_| TransactionError::Write)?;
        }
        if let Some(lease) = recovery_lease {
            lease.verify_with(ops)?;
        }
        ops.remove_dir(&current.root)
            .map_err(|_| TransactionError::Write)
    }

    pub fn verify(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        let bytes = read_recovery_journal_regular_file(
            ops,
            &self.root.join("recovery.json"),
            TransactionError::JournalState,
        )?;
        if bytes != self.bytes || canonical_json_bytes(&self.record)? != bytes {
            return Err(TransactionError::JournalState);
        }
        Ok(())
    }
}

fn recovery_journal_entry_names(
    ops: &impl FilesystemOps,
    root: &Path,
) -> Result<Vec<String>, TransactionError> {
    let metadata = ops
        .metadata(root)
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let names = sorted_directory_entries(root)?
        .into_iter()
        .map(|entry| {
            entry
                .file_name()
                .into_string()
                .map_err(|_| TransactionError::RecoveryAmbiguous)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for name in &names {
        let child = ops
            .metadata(&root.join(name))
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if child.file_type().is_symlink() || !child.is_file() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    Ok(names)
}

fn is_terminal_recovery_cleanup_residue(
    ops: &impl FilesystemOps,
    journal: &RecoveryJournal,
) -> Result<bool, TransactionError> {
    if !journal.cleanup_form {
        return Ok(false);
    }
    Ok(recovery_journal_entry_names(ops, &journal.root)? == ["recovery.json"])
}

fn terminal_empty_recovery_cleanup_residue(
    ops: &impl FilesystemOps,
    transactions: &Path,
) -> Result<Option<PathBuf>, TransactionError> {
    let entries = sorted_directory_entries(transactions)?;
    if entries.is_empty() {
        return Ok(None);
    }
    if entries.len() != 1 {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let entry = entries.into_iter().next().expect("checked one entry");
    let name = entry
        .file_name()
        .into_string()
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    let Some(transaction_id) = name.strip_prefix(".cleanup-") else {
        return Err(TransactionError::RecoveryAmbiguous);
    };
    if !valid_transaction_id(transaction_id) {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    let metadata = ops
        .metadata(&entry.path())
        .map_err(|_| TransactionError::RecoveryAmbiguous)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || !sorted_directory_entries(&entry.path())?.is_empty()
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    Ok(Some(entry.path()))
}

fn recovery_cleanup_link_prefix(names: &[String]) -> bool {
    names.is_empty() || names == ["recovery.json"] || names == ["published.json", "recovery.json"]
}

fn recovery_cleanup_remaining_suffix(names: &[String]) -> bool {
    names.is_empty() || names == ["published.json"] || names == ["published.json", "recovery.json"]
}

fn assert_recovery_counterpart_bytes(
    ops: &impl FilesystemOps,
    first_root: &Path,
    first_names: &[String],
    second_root: &Path,
    second_names: &[String],
) -> Result<(), TransactionError> {
    for name in first_names {
        if second_names.iter().any(|candidate| candidate == name)
            && read_recovery_journal_regular_file(
                ops,
                &first_root.join(name),
                TransactionError::RecoveryAmbiguous,
            )? != read_recovery_journal_regular_file(
                ops,
                &second_root.join(name),
                TransactionError::RecoveryAmbiguous,
            )?
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    Ok(())
}

fn read_recovery_journal_transition(
    ops: &impl FilesystemOps,
    recovery_id: String,
    regular_root: PathBuf,
    regular_names: Vec<String>,
    cleanup_root: PathBuf,
    cleanup_names: Vec<String>,
) -> Result<RecoveryJournal, TransactionError> {
    if let Ok(regular) = RecoveryJournal::read_exact(
        ops,
        regular_root.clone(),
        recovery_id.clone(),
        false,
        regular_names.clone(),
    ) {
        if regular_names != ["published.json", "recovery.json"]
            || !recovery_cleanup_link_prefix(&cleanup_names)
        {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if !cleanup_names.is_empty() {
            let cleanup = RecoveryJournal::read_exact(
                ops,
                cleanup_root.clone(),
                recovery_id,
                true,
                cleanup_names.clone(),
            )?;
            if cleanup.record != regular.record || cleanup.bytes != regular.bytes {
                return Err(TransactionError::RecoveryAmbiguous);
            }
        }
        assert_recovery_counterpart_bytes(
            ops,
            &regular_root,
            &regular_names,
            &cleanup_root,
            &cleanup_names,
        )?;
        return Ok(regular);
    }

    let cleanup = RecoveryJournal::read_exact(
        ops,
        cleanup_root.clone(),
        recovery_id,
        true,
        cleanup_names.clone(),
    )?;
    if cleanup_names != ["published.json", "recovery.json"]
        || !recovery_cleanup_remaining_suffix(&regular_names)
    {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    assert_recovery_counterpart_bytes(
        ops,
        &cleanup_root,
        &cleanup_names,
        &regular_root,
        &regular_names,
    )?;
    Ok(cleanup)
}

fn finish_recovery_source_removal(
    ops: &impl FilesystemOps,
    cleanup_witness: &RecoveryJournal,
    recovery_lease: Option<&Lease>,
) -> Result<RecoveryJournal, TransactionError> {
    if !cleanup_witness.cleanup_form {
        return Err(TransactionError::JournalState);
    }
    let parent = cleanup_witness
        .root
        .parent()
        .ok_or(TransactionError::Write)?;
    let regular = parent.join(&cleanup_witness.record.recovery_id);
    match ops.metadata(&regular) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RecoveryJournal {
                root: cleanup_witness.root.clone(),
                record: cleanup_witness.record.clone(),
                bytes: cleanup_witness.bytes.clone(),
                cleanup_form: true,
            })
        }
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {}
        _ => return Err(TransactionError::JournalState),
    }
    let regular_names = recovery_journal_entry_names(ops, &regular)?;
    if !recovery_cleanup_remaining_suffix(&regular_names) {
        return Err(TransactionError::JournalState);
    }
    assert_recovery_counterpart_bytes(
        ops,
        &cleanup_witness.root,
        &["published.json".to_owned(), "recovery.json".to_owned()],
        &regular,
        &regular_names,
    )?;
    for name in ["recovery.json", "published.json"] {
        if regular_names.iter().any(|candidate| candidate == name) {
            if read_recovery_journal_regular_file(
                ops,
                &regular.join(name),
                TransactionError::JournalState,
            )? != read_recovery_journal_regular_file(
                ops,
                &cleanup_witness.root.join(name),
                TransactionError::JournalState,
            )? {
                return Err(TransactionError::JournalState);
            }
            if let Some(lease) = recovery_lease {
                lease.verify_with(ops)?;
            }
            ops.remove_file(&regular.join(name))
                .map_err(|_| TransactionError::Write)?;
        }
    }
    if let Some(lease) = recovery_lease {
        lease.verify_with(ops)?;
    }
    ops.remove_dir(&regular)
        .map_err(|_| TransactionError::Write)?;
    ops.sync_directory(parent)
        .map_err(|_| TransactionError::Write)?;
    RecoveryJournal::read_exact(
        ops,
        cleanup_witness.root.clone(),
        cleanup_witness.record.recovery_id.clone(),
        true,
        recovery_journal_entry_names(ops, &cleanup_witness.root)?,
    )
    .map_err(|_| TransactionError::JournalState)
}

fn read_recovery_journal_regular_file(
    ops: &impl FilesystemOps,
    path: &Path,
    error: TransactionError,
) -> Result<Vec<u8>, TransactionError> {
    let metadata = ops.metadata(path).map_err(|_| error.clone())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(error);
    }
    ops.read(path).map_err(|_| error)
}

pub fn sorted_directory_entries(path: &Path) -> Result<Vec<fs::DirEntry>, TransactionError> {
    let mut entries = fs::read_dir(path)
        .map_err(|_| TransactionError::Read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TransactionError::Read)?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

fn remove_exact_directory(
    ops: &impl FilesystemOps,
    path: &Path,
    expected: &[&str],
) -> Result<(), TransactionError> {
    let entries = sorted_directory_entries(path)?;
    let names = entries
        .iter()
        .map(|entry| {
            entry
                .file_name()
                .into_string()
                .map_err(|_| TransactionError::RecoveryAmbiguous)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut wanted = expected
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    wanted.sort();
    if names != wanted {
        return Err(TransactionError::RecoveryAmbiguous);
    }
    for entry in entries {
        let metadata = ops
            .metadata(&entry.path())
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        ops.remove_file(&entry.path())
            .map_err(|_| TransactionError::Write)?;
    }
    ops.remove_dir(path).map_err(|_| TransactionError::Write)
}

pub fn remove_verified_tree(ops: &impl FilesystemOps, path: &Path) -> Result<(), TransactionError> {
    require_recovery_regular_directory(ops, path)?;
    for entry in sorted_directory_entries(path)? {
        let metadata = ops
            .metadata(&entry.path())
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if metadata.file_type().is_symlink() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if metadata.is_dir() {
            remove_verified_tree(ops, &entry.path())?;
        } else if metadata.is_file() {
            ops.remove_file(&entry.path())
                .map_err(|_| TransactionError::Write)?;
        } else {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    ops.remove_dir(path).map_err(|_| TransactionError::Write)
}

pub fn remove_verified_tree_protected(
    ops: &impl FilesystemOps,
    path: &Path,
    recovery_lease: &Lease,
) -> Result<(), TransactionError> {
    require_recovery_regular_directory(ops, path)?;
    for entry in sorted_directory_entries(path)? {
        let metadata = ops
            .metadata(&entry.path())
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if metadata.file_type().is_symlink() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if metadata.is_dir() {
            remove_verified_tree_protected(ops, &entry.path(), recovery_lease)?;
        } else if metadata.is_file() {
            recovery_lease.verify_with(ops)?;
            ops.remove_file(&entry.path())
                .map_err(|_| TransactionError::Write)?;
        } else {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    recovery_lease.verify_with(ops)?;
    ops.remove_dir(path).map_err(|_| TransactionError::Write)
}

pub fn remove_optional_tree(ops: &impl FilesystemOps, path: &Path) -> Result<(), TransactionError> {
    match ops.metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(TransactionError::RecoveryAmbiguous)
        }
        Ok(_) => {}
        Err(_) => return Err(TransactionError::Read),
    }
    for entry in sorted_directory_entries(path)? {
        let metadata = ops
            .metadata(&entry.path())
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if metadata.file_type().is_symlink() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if metadata.is_dir() {
            remove_optional_tree(ops, &entry.path())?;
        } else if metadata.is_file() {
            ops.remove_file(&entry.path())
                .map_err(|_| TransactionError::Write)?;
        } else {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    ops.remove_dir(path).map_err(|_| TransactionError::Write)
}

pub fn remove_optional_tree_protected(
    ops: &impl FilesystemOps,
    path: &Path,
    recovery_lease: &Lease,
) -> Result<(), TransactionError> {
    match ops.metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(TransactionError::RecoveryAmbiguous)
        }
        Ok(_) => {}
        Err(_) => return Err(TransactionError::Read),
    }
    for entry in sorted_directory_entries(path)? {
        let metadata = ops
            .metadata(&entry.path())
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if metadata.file_type().is_symlink() {
            return Err(TransactionError::RecoveryAmbiguous);
        }
        if metadata.is_dir() {
            remove_optional_tree_protected(ops, &entry.path(), recovery_lease)?;
        } else if metadata.is_file() {
            recovery_lease.verify_with(ops)?;
            ops.remove_file(&entry.path())
                .map_err(|_| TransactionError::Write)?;
        } else {
            return Err(TransactionError::RecoveryAmbiguous);
        }
    }
    recovery_lease.verify_with(ops)?;
    ops.remove_dir(path).map_err(|_| TransactionError::Write)
}

pub fn remove_optional_file(ops: &impl FilesystemOps, path: &Path) -> Result<(), TransactionError> {
    match ops.metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(TransactionError::RecoveryAmbiguous)
        }
        Ok(_) => ops.remove_file(path).map_err(|_| TransactionError::Write),
        Err(_) => Err(TransactionError::Read),
    }
}

fn recovery_ledger_temporaries(ledger: &Path) -> Result<Vec<PathBuf>, TransactionError> {
    let parent = ledger.parent().ok_or(TransactionError::Read)?;
    let mut paths = Vec::new();
    for entry in sorted_directory_entries(parent)? {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        if name
            .strip_prefix(".verdicts-")
            .and_then(|value| value.parse::<u64>().ok().map(|parsed| (value, parsed)))
            .is_some_and(|(value, parsed)| parsed.to_string() == value)
        {
            paths.push(entry.path());
        }
    }
    Ok(paths)
}

fn canonical_verdict_events(
    decision: &ConfirmedDecision,
    view: &super::LedgerView,
    canonical_result: CanonicalResult,
    generation_id: &str,
) -> Result<(Vec<LedgerEvent>, String, String), TransactionError> {
    let mut events = Vec::new();
    let previous_event_hash = if view.events.is_empty() {
        let genesis = GenesisEvent::new(GenesisPayload {
            schema_version: super::LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "genesis".to_owned(),
            sequence: 0,
            event_id: "evt-0000000000000000".to_owned(),
            created_at: decision.ruled_at.as_str().to_owned(),
            previous_event_hash: None,
            store_id: decision.store.policy.policy.store_id.clone(),
            profile_version: decision.profile.version.clone(),
            profile_hash: decision.profile.hash.clone(),
            policy_hash: decision.store.policy.hash.clone(),
            seed_manifest_hash: decision.store.seed.manifest_hash.clone(),
            seed_corpus_report_hash: decision.store.seed.corpus_report_hash.clone(),
            seed_bundle_hash: decision.store.seed.bundle_hash.clone(),
            source_commit: decision.store.seed.source_commit.clone(),
            initial_canonical_ref: "seed".to_owned(),
        })
        .map_err(|_| TransactionError::Write)?;
        let hash = genesis.hash().to_owned();
        events.push(LedgerEvent::Genesis(Box::new(genesis)));
        Some(hash)
    } else {
        view.final_hash.clone()
    };
    let sequence =
        u64::try_from(view.events.len() + events.len()).map_err(|_| TransactionError::Write)?;
    let event_id = format!("evt-{sequence:016}");
    let evaluator_meta_hash = decision
        .proposal
        .meta
        .evaluator
        .as_ref()
        .map(evaluator_commitment)
        .transpose()
        .map_err(|_| TransactionError::Read)?;
    let rewrite_hash = decision
        .decision
        .rewrite
        .as_ref()
        .map(|rewrite| rewrite.hash().to_owned());
    let rewrite_bytes = decision
        .decision
        .rewrite
        .as_ref()
        .map(|rewrite| u64::try_from(rewrite.bytes().len()).map_err(|_| TransactionError::Write))
        .transpose()?;
    let verdict = VerdictEvent::new(VerdictPayload {
        schema_version: super::LEDGER_EVENT_SCHEMA.to_owned(),
        event_type: "verdict".to_owned(),
        sequence,
        event_id: event_id.clone(),
        occurred_at: decision.ruled_at.as_str().to_owned(),
        previous_event_hash,
        store_id: decision.store.policy.policy.store_id.clone(),
        transaction_id: decision.transaction_id.clone(),
        proposal_id: decision.proposal.meta.proposal_id.clone(),
        proposal_meta_hash: decision.proposal.meta_hash.clone(),
        candidate_hash: decision.proposal.candidate_hash.clone(),
        source_id: decision.proposal.meta.source_id.clone(),
        verb: decision.decision.verb,
        reason: decision.decision.reason.as_str().to_owned(),
        verdict_id: decision.decision.verdict_id.clone(),
        actor: decision.store.policy.policy.human_actor.clone(),
        policy_hash: decision.store.policy.hash.clone(),
        profile_hash: decision.profile.hash.clone(),
        confidence: decision.proposal.meta.confidence,
        evaluator_meta_hash,
        base_kind: decision.proposal.meta.base.kind.clone(),
        base_generation: decision.proposal.meta.base.generation_id.clone(),
        base_manifest_hash: decision.proposal.meta.base.manifest_hash.clone(),
        base_bundle_hash: decision.proposal.meta.base.bundle_hash.clone(),
        base_target_hash: decision.proposal.meta.base.target_hash.clone(),
        rewrite_hash,
        rewrite_bytes,
        canonical_result,
        resulting_canonical_head: generation_id.to_owned(),
        intake_sequence: decision.intake_head.sequence,
        intake_head_hash: decision.intake_head.event_hash.clone(),
    })
    .map_err(|_| TransactionError::Write)?;
    let verdict_hash = verdict.hash().to_owned();
    events.push(LedgerEvent::Verdict(Box::new(verdict)));
    Ok((events, event_id, verdict_hash))
}

fn require_absent(ops: &impl FilesystemOps, path: &Path) -> Result<(), TransactionError> {
    match ops.metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(TransactionError::Write),
    }
}

fn assert_confirmed_inputs_unchanged(decision: &ConfirmedDecision) -> Result<(), TransactionError> {
    let policy = super::load_policy(&decision.store.paths.policy)
        .map_err(|_| TransactionError::PolicyChanged)?;
    if policy.bytes != decision.store.policy.bytes || policy.hash != decision.store.policy.hash {
        return Err(TransactionError::PolicyChanged);
    }
    let profile =
        load_p3_profile(&decision.profile_path).map_err(|_| TransactionError::ProfileChanged)?;
    if profile.bytes != decision.profile.bytes || profile.hash != decision.profile.hash {
        return Err(TransactionError::ProfileChanged);
    }
    let intake = read_intake(&decision.store.paths).map_err(map_intake_error)?;
    if intake.head() != &decision.intake_head {
        return Err(TransactionError::IntakeHeadMismatch);
    }
    Ok(())
}

fn discard_unpublished_applying_artifacts<O, F>(
    decision: &ConfirmedDecision,
    runtime: &TransactionRuntime<O, F>,
    staged_generation_root: &Path,
    staged_index_root: &Path,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    decision.leases.verify_all_with(&runtime.ops)?;
    remove_verified_tree(&runtime.ops, staged_index_root)?;
    runtime
        .ops
        .sync_directory(&decision.store.paths.derived.join("indexes"))
        .map_err(|_| TransactionError::Write)?;
    decision.leases.verify_all_with(&runtime.ops)?;
    remove_verified_tree(&runtime.ops, staged_generation_root)?;
    runtime
        .ops
        .sync_directory(&decision.store.paths.canon.join("generations"))
        .map_err(|_| TransactionError::Write)?;
    decision.leases.verify_all_with(&runtime.ops)
}

fn publish_current<O, F>(
    current: &Path,
    generation_id: &str,
    transaction_id: &str,
    runtime: &TransactionRuntime<O, F>,
    recovery_lease: Option<&Lease>,
) -> Result<(), TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    let parent = current.parent().ok_or(TransactionError::Write)?;
    let temporary = parent.join(format!(".current-{transaction_id}"));
    require_absent(&runtime.ops, &temporary)?;
    runtime
        .ops
        .write_new_synced(&temporary, format!("{generation_id}\n").as_bytes())
        .map_err(|_| TransactionError::JournalState)?;
    runtime
        .ops
        .sync_directory(parent)
        .map_err(|_| TransactionError::JournalState)?;
    runtime
        .faults
        .check(FaultPoint::BeforeCurrentRename)
        .map_err(|_| TransactionError::JournalState)?;
    if let Some(lease) = recovery_lease {
        lease.verify_with(&runtime.ops)?;
    }
    runtime
        .ops
        .rename(&temporary, current)
        .map_err(|_| TransactionError::JournalState)?;
    runtime
        .ops
        .sync_directory(parent)
        .map_err(|_| TransactionError::JournalState)?;
    runtime
        .faults
        .check(FaultPoint::AfterCurrentRename)
        .map_err(|_| TransactionError::JournalState)
}

fn prepared_hashes(decision: &ConfirmedDecision, verdict_hash: &str) -> Vec<String> {
    let mut hashes = vec![
        decision.profile.hash.clone(),
        decision.store.policy.hash.clone(),
        decision.proposal.meta_hash.clone(),
        decision.proposal.candidate_hash.clone(),
        decision.proposal.meta.base.manifest_hash.clone(),
        decision.proposal.meta.base.bundle_hash.clone(),
        verdict_hash.to_owned(),
    ];
    if let Some(hash) = &decision.proposal.meta.base.target_hash {
        hashes.push(hash.clone());
    }
    hashes.sort();
    hashes.dedup();
    hashes
}

fn validate_request_shape(request: &HumanVerdictRequest) -> Result<(), TransactionError> {
    if !valid_proposal_id(&request.proposal_id)
        || request.reason.bytes().is_empty()
        || request.reason.bytes().len() > 1024 * 1024
    {
        return Err(TransactionError::InvalidRequest);
    }
    Ok(())
}

pub fn map_judgment_error(error: JudgmentError) -> TransactionError {
    match error {
        JudgmentError::ReasonInvalid => TransactionError::ReasonInvalid,
        JudgmentError::RewriteInvalid => TransactionError::RewriteInvalid,
        JudgmentError::RewriteRead => TransactionError::Read,
        JudgmentError::IntentConflict => TransactionError::IntentConflict,
        JudgmentError::RewriteRequired => TransactionError::RewriteRequired,
        JudgmentError::RewriteForbidden => TransactionError::RewriteForbidden,
        JudgmentError::VerbInvalid => TransactionError::InvalidRequest,
        JudgmentError::Render => TransactionError::JudgmentRender,
    }
}

pub fn map_policy_archive_error(error: PolicyError) -> TransactionError {
    match error {
        PolicyError::ChangedDuringTransaction => TransactionError::PolicyChanged,
        PolicyError::Read
        | PolicyError::Utf8
        | PolicyError::LineEndings
        | PolicyError::Syntax
        | PolicyError::KeyOrder
        | PolicyError::DuplicateKey
        | PolicyError::UnknownField
        | PolicyError::MissingField
        | PolicyError::SchemaUnsupported
        | PolicyError::ValueInvalid
        | PolicyError::InvariantInvalid => TransactionError::PolicyArchive,
    }
}

pub fn evaluate_current_policy(state: &PreflightState) -> Result<PolicyDecision, TransactionError> {
    let intake = read_intake(&state.store.paths).map_err(map_intake_error)?;
    let mut prefix = intake_prefix(&intake, &state.intake_head).map_err(map_intake_error)?;
    let terminal_ids = if state.store.sealed {
        read_ledger(&state.store.paths.ledger)
            .map_err(|_| TransactionError::Read)?
            .terminal_proposals
    } else {
        Default::default()
    };
    for retained in &mut prefix.staged {
        if terminal_ids.contains_key(&retained.proposal_id) {
            retained.state = "ruled".to_owned();
        }
    }
    Ok(PolicyEvaluator::evaluate(&PolicyContext {
        policy: &state.store.policy,
        profile: &state.profile,
        store: &state.store,
        proposal: &state.proposal,
        intake: &prefix,
    }))
}

fn assert_inputs_unchanged(state: &PreflightState) -> Result<(), TransactionError> {
    let policy = super::load_policy(&state.store.paths.policy)
        .map_err(|_| TransactionError::PolicyChanged)?;
    if policy.bytes != state.store.policy.bytes || policy.hash != state.store.policy.hash {
        return Err(TransactionError::PolicyChanged);
    }
    let profile =
        load_p3_profile(&state.profile_path).map_err(|_| TransactionError::ProfileChanged)?;
    if profile.bytes != state.profile.bytes || profile.hash != state.profile.hash {
        return Err(TransactionError::ProfileChanged);
    }
    Ok(())
}

fn assert_intake_head_unchanged(state: &PreflightState) -> Result<(), TransactionError> {
    let intake = read_intake(&state.store.paths).map_err(map_intake_error)?;
    if intake.head() != &state.intake_head {
        return Err(TransactionError::IntakeHeadMismatch);
    }
    Ok(())
}

fn assert_locked_inputs_match_initial(
    initial: &PreflightState,
    locked: &PreflightState,
) -> Result<(), TransactionError> {
    if locked.store.policy.bytes != initial.store.policy.bytes
        || locked.store.policy.hash != initial.store.policy.hash
    {
        return Err(TransactionError::PolicyChanged);
    }
    if locked.profile.bytes != initial.profile.bytes || locked.profile.hash != initial.profile.hash
    {
        return Err(TransactionError::ProfileChanged);
    }
    Ok(())
}

pub struct TransactionIdInput<'a> {
    pub proposal_id: &'a str,
    pub verb: VerdictVerb,
    pub reason_hash: &'a [u8],
    pub rewrite_hash: Option<&'a str>,
    pub actor: &'a str,
    pub ruled_at: &'a str,
    pub observed_ledger_head: Option<&'a str>,
    pub intake_sequence: u64,
    pub intake_head_hash: Option<&'a str>,
    pub policy_hash: &'a str,
    pub profile_hash: &'a str,
}

pub fn transaction_id(input: &TransactionIdInput<'_>) -> String {
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, b"sovmem.transaction.v2");
    frame(&mut hasher, b"verdict");
    frame(&mut hasher, input.ruled_at.as_bytes());
    frame(&mut hasher, input.proposal_id.as_bytes());
    frame(&mut hasher, input.verb.as_str().as_bytes());
    frame(&mut hasher, input.reason_hash);
    frame_optional(&mut hasher, input.rewrite_hash.map(str::as_bytes));
    frame(&mut hasher, input.actor.as_bytes());
    frame_optional(&mut hasher, input.observed_ledger_head.map(str::as_bytes));
    frame(&mut hasher, &input.intake_sequence.to_le_bytes());
    frame_optional(&mut hasher, input.intake_head_hash.map(str::as_bytes));
    frame(&mut hasher, input.policy_hash.as_bytes());
    frame(&mut hasher, input.profile_hash.as_bytes());
    format!("txn-{}", hasher.finalize().to_hex())
}

fn next_generation_id(head: &CanonicalHead) -> String {
    let number = match head {
        CanonicalHead::Seed => 1,
        CanonicalHead::Generation(value) => value
            .get(4..)
            .and_then(|value| value.parse::<u64>().ok())
            .and_then(|value| value.checked_add(1))
            .unwrap_or(0),
    };
    format!("gen-{number:016}")
}

pub struct DecisionDisplayInput<'a> {
    pub state: &'a PreflightState,
    pub decision: &'a DecisionMaterial,
    pub reason_hash: &'a str,
    pub ruled_at: &'a Timestamp,
    pub actor: &'a str,
    pub observed_ledger_head: Option<&'a str>,
    pub approved_target: Option<&'a [u8]>,
    pub approved_target_hash: Option<&'a str>,
    pub judgment: Option<&'a JudgmentDocument>,
    pub policy_decision: &'a PolicyDecision,
    pub transaction_id: &'a str,
    pub generation_id: Option<&'a str>,
}

pub fn render_decision_display(input: DecisionDisplayInput<'_>) -> Vec<u8> {
    let DecisionDisplayInput {
        state,
        decision,
        reason_hash,
        ruled_at,
        actor,
        observed_ledger_head,
        approved_target,
        approved_target_hash,
        judgment,
        policy_decision,
        transaction_id,
        generation_id,
    } = input;
    let mut display = Vec::new();
    display.extend_from_slice(b"SOVMEM HUMAN VERDICT PREVIEW\n");
    display_field(
        &mut display,
        "proposal_id",
        &state.proposal.meta.proposal_id,
    );
    display_field(&mut display, "verb", decision.verb.as_str());
    display_field(
        &mut display,
        "proposal_meta_hash",
        &state.proposal.meta_hash,
    );
    display_field(
        &mut display,
        "candidate_hash",
        &state.proposal.candidate_hash,
    );
    display_field(&mut display, "policy_hash", &state.store.policy.hash);
    display_field(&mut display, "profile_hash", &state.profile.hash);
    display_field(&mut display, "verdict_id", &decision.verdict_id);
    display_field(&mut display, "transaction_id", transaction_id);
    display_field(&mut display, "reason_hash", reason_hash);
    display_field(&mut display, "ruled_at", ruled_at.as_str());
    display_field(&mut display, "actor", actor);
    display_field(
        &mut display,
        "observed_ledger_head",
        observed_ledger_head.unwrap_or("null"),
    );
    display_field(
        &mut display,
        "intake_sequence",
        &state.intake_head.sequence.to_string(),
    );
    display_field(
        &mut display,
        "intake_head_hash",
        state.intake_head.event_hash.as_deref().unwrap_or("null"),
    );
    display_field(&mut display, "base_kind", &state.proposal.meta.base.kind);
    display_field(
        &mut display,
        "base_generation",
        state
            .proposal
            .meta
            .base
            .generation_id
            .as_deref()
            .unwrap_or(""),
    );
    display_field(
        &mut display,
        "base_manifest_hash",
        &state.proposal.meta.base.manifest_hash,
    );
    display_field(
        &mut display,
        "base_bundle_hash",
        &state.proposal.meta.base.bundle_hash,
    );
    display_field(
        &mut display,
        "base_target_hash",
        state
            .proposal
            .meta
            .base
            .target_hash
            .as_deref()
            .unwrap_or(""),
    );
    display_field(
        &mut display,
        "approved_target_hash",
        approved_target_hash.unwrap_or(""),
    );
    display_field(&mut display, "generation_id", generation_id.unwrap_or(""));
    display_field(
        &mut display,
        "judgment_hash",
        judgment.map(|value| value.hash.as_str()).unwrap_or(""),
    );
    display_field(
        &mut display,
        "policy_passed",
        if policy_decision.passed {
            "true"
        } else {
            "false"
        },
    );
    display_field(
        &mut display,
        "policy_rule_codes",
        &policy_decision.rule_codes.join(","),
    );
    display_field(
        &mut display,
        "confidence",
        &state
            .proposal
            .meta
            .confidence
            .map(|value| value.millionths().to_string())
            .unwrap_or_default(),
    );
    if let Some(evaluator) = &state.proposal.meta.evaluator {
        display_bytes(
            &mut display,
            "evaluator_source",
            evaluator.source_id.as_bytes(),
        );
        display_bytes(
            &mut display,
            "evaluator_recommendation",
            evaluator.recommendation.as_bytes(),
        );
        for reason in &evaluator.reasons {
            display_bytes(&mut display, "evaluator_reason", reason.as_bytes());
        }
        display_field(
            &mut display,
            "evaluator_meta_hash",
            &evaluator_commitment(evaluator).unwrap_or_default(),
        );
    }
    display_bytes(&mut display, "candidate", &state.proposal.candidate_bytes);
    display_bytes(&mut display, "reason", decision.reason.bytes());
    if let Some(rewrite) = &decision.rewrite {
        display_field(&mut display, "rewrite_hash", rewrite.hash());
        display_bytes(&mut display, "rewrite", rewrite.bytes());
    }
    if let Some(bytes) = approved_target {
        display_bytes(&mut display, "approved_target", bytes);
    }
    if let Some(judgment) = judgment {
        display_bytes(&mut display, "judgment", &judgment.bytes);
    }
    display.extend_from_slice(b"TYPE EXACTLY: APPLY ");
    display.extend_from_slice(decision.verb.as_str().as_bytes());
    display.extend_from_slice(b" ");
    display.extend_from_slice(&state.proposal.meta.proposal_id.as_bytes()[..12]);
    display.extend_from_slice(b"\n");
    display
}

fn display_field(display: &mut Vec<u8>, label: &str, value: &str) {
    display.extend_from_slice(label.as_bytes());
    display.extend_from_slice(b": ");
    display.extend_from_slice(value.as_bytes());
    display.push(b'\n');
}

fn display_bytes(display: &mut Vec<u8>, label: &str, bytes: &[u8]) {
    display.extend_from_slice(label.as_bytes());
    display.extend_from_slice(format!("_bytes_length: {}\n", bytes.len()).as_bytes());
    display.extend_from_slice(bytes);
    display.push(b'\n');
}

pub trait FilesystemOps {
    fn create_dir(&self, path: &Path) -> io::Result<()>;
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn sync_directory(&self, path: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn remove_dir(&self, path: &Path) -> io::Result<()>;
    fn metadata(&self, path: &Path) -> io::Result<fs::Metadata>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StdFilesystem;

impl FilesystemOps for StdFilesystem {
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        fs::create_dir(path)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let result = file.write_all(bytes).and_then(|_| file.sync_all());
        drop(file);
        if result.is_err() {
            let _ = fs::remove_file(path);
        }
        result
    }

    fn sync_directory(&self, path: &Path) -> io::Result<()> {
        File::open(path)?.sync_all()
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::hard_link(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir(path)
    }

    fn metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        fs::symlink_metadata(path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaultPoint {
    AfterInboxRead,
    AfterPairSync,
    AfterIntakeJournalSync,
    BeforeIntakeEventRename,
    AfterIntakeEventRename,
    BeforeInboxCleanup,
    AfterLeaseCreation,
    AfterPlanPrepSync,
    AfterPlanFinalDirectorySync,
    AfterPlanHardLinkSync,
    AfterPlanPrepRemoval,
    AfterPlanSync,
    AfterProposalCandidateSync,
    AfterProposalMetadataSync,
    ProposalDirectoryRename,
    AfterTransactionJournalCreation,
    AfterTargetBytesStaged,
    AfterJudgmentBytesStaged,
    AfterManifestSync,
    AfterLevelThreeSelfCheck,
    DuringIndexBuild,
    AfterIndexSelfCheck,
    AfterReindexReceiptSync,
    AfterGenerationRename,
    AfterDerivedIndexRename,
    AfterPreparedMarker,
    AfterLedgerTemporarySync,
    BeforeLedgerRename,
    AfterLedgerRename,
    BeforeCurrentRename,
    AfterCurrentRename,
    BeforePublishedMarker,
    DuringCleanup,
    AfterJournalCleanup,
    AfterRecoveryLeaseAcquisition,
    AfterRecoveryJournalCreation,
    AfterOriginalCleanupWitness,
    AfterRecoveryPublishedMarker,
    AfterOriginalCleanup,
    AfterRecoveryCleanupWitness,
    AfterRecoveryVerification,
}

impl FaultPoint {
    pub const ALL: [Self; 41] = [
        Self::AfterInboxRead,
        Self::AfterPairSync,
        Self::AfterIntakeJournalSync,
        Self::BeforeIntakeEventRename,
        Self::AfterIntakeEventRename,
        Self::BeforeInboxCleanup,
        Self::AfterLeaseCreation,
        Self::AfterPlanPrepSync,
        Self::AfterPlanFinalDirectorySync,
        Self::AfterPlanHardLinkSync,
        Self::AfterPlanPrepRemoval,
        Self::AfterPlanSync,
        Self::AfterProposalCandidateSync,
        Self::AfterProposalMetadataSync,
        Self::ProposalDirectoryRename,
        Self::AfterTransactionJournalCreation,
        Self::AfterTargetBytesStaged,
        Self::AfterJudgmentBytesStaged,
        Self::AfterManifestSync,
        Self::AfterLevelThreeSelfCheck,
        Self::DuringIndexBuild,
        Self::AfterIndexSelfCheck,
        Self::AfterReindexReceiptSync,
        Self::AfterGenerationRename,
        Self::AfterDerivedIndexRename,
        Self::AfterPreparedMarker,
        Self::AfterLedgerTemporarySync,
        Self::BeforeLedgerRename,
        Self::AfterLedgerRename,
        Self::BeforeCurrentRename,
        Self::AfterCurrentRename,
        Self::BeforePublishedMarker,
        Self::DuringCleanup,
        Self::AfterJournalCleanup,
        Self::AfterRecoveryLeaseAcquisition,
        Self::AfterRecoveryJournalCreation,
        Self::AfterOriginalCleanupWitness,
        Self::AfterRecoveryPublishedMarker,
        Self::AfterOriginalCleanup,
        Self::AfterRecoveryCleanupWitness,
        Self::AfterRecoveryVerification,
    ];
}

pub trait FaultInjector {
    fn check(&self, point: FaultPoint) -> Result<(), TransactionError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoFaults;

impl FaultInjector for NoFaults {
    fn check(&self, _point: FaultPoint) -> Result<(), TransactionError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseKind {
    Verdict,
}

impl LeaseKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Verdict => "verdict",
        }
    }

    const fn locked_code(self) -> &'static str {
        match self {
            Self::Verdict => "store.locked",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseInspection {
    started_at: Timestamp,
    process_id: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerdictLeaseReceipt {
    id: String,
    store_id: String,
    observed_head: Option<String>,
    profile_hash: String,
    policy_hash: String,
    proposal_id: String,
}

impl VerdictLeaseReceipt {
    pub fn new(
        store_id: &str,
        observed_head: Option<&str>,
        profile_hash: &str,
        policy_hash: &str,
        proposal_id: &str,
    ) -> Result<Self, TransactionError> {
        if store_id.is_empty()
            || !valid_observed_head(observed_head)
            || !valid_hash(profile_hash)
            || !valid_hash(policy_hash)
            || !valid_proposal_id(proposal_id)
        {
            return Err(TransactionError::InvalidReceipt);
        }
        let id = lease_receipt_id(
            "verdict",
            store_id,
            observed_head,
            profile_hash,
            policy_hash,
            &[proposal_id],
        );
        Ok(Self {
            id,
            store_id: store_id.to_owned(),
            observed_head: observed_head.map(str::to_owned),
            profile_hash: profile_hash.to_owned(),
            policy_hash: policy_hash.to_owned(),
            proposal_id: proposal_id.to_owned(),
        })
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn matches_store(&self, store: &StoreSnapshot) -> bool {
        self.matches_commitment(
            &store.policy.policy.store_id,
            Some(store.head.as_str()),
            &store.profile_hash,
            &store.policy.hash,
        )
    }

    fn store_mismatch_error(&self, store: &StoreSnapshot) -> TransactionError {
        if self.policy_hash != store.policy.hash {
            TransactionError::PolicyChanged
        } else if self.profile_hash != store.profile_hash {
            TransactionError::ProfileChanged
        } else {
            TransactionError::InvalidReceipt
        }
    }

    pub fn matches_commitment(
        &self,
        store_id: &str,
        observed_head: Option<&str>,
        profile_hash: &str,
        policy_hash: &str,
    ) -> bool {
        self.store_id == store_id
            && self.observed_head.as_deref() == observed_head
            && self.profile_hash == profile_hash
            && self.policy_hash == policy_hash
            && self.id
                == lease_receipt_id(
                    "verdict",
                    &self.store_id,
                    self.observed_head.as_deref(),
                    &self.profile_hash,
                    &self.policy_hash,
                    &[&self.proposal_id],
                )
    }
}

impl LeaseInspection {
    pub fn local() -> Result<Self, TransactionError> {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| TransactionError::Read)?;
        let started_at = super::timestamp_from_unix(seconds.as_secs(), seconds.subsec_nanos())
            .map_err(|_| TransactionError::Read)?;
        Ok(Self {
            started_at,
            process_id: std::process::id(),
        })
    }

    pub fn fixed(started_at: Timestamp, process_id: u32) -> Self {
        Self {
            started_at,
            process_id,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LeaseOwner {
    pub schema_version: String,
    pub owner_id: String,
    pub command_class: String,
    pub started_at: String,
    pub process_id: u32,
    pub lease_token: String,
}

#[derive(Clone, Debug)]
pub struct Lease {
    pub root: PathBuf,
    pub kind: LeaseKind,
    pub owner_hash: String,
    pub lease_token: String,
}

impl Lease {
    pub fn path(&self) -> PathBuf {
        self.root.join("locks/verdict.lock")
    }

    pub fn verify(&self) -> Result<(), TransactionError> {
        self.verify_with(&StdFilesystem)
    }

    pub fn verify_with(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        let owner = read_canonical_owner(ops, &self.path().join("owner.json"), self.kind)?;
        let owner_bytes = canonical_owner_bytes(&owner)?;
        if blake3::hash(&owner_bytes).to_hex().as_str() != self.owner_hash
            || owner.lease_token != self.lease_token
        {
            return Err(TransactionError::LeaseChanged(self.kind));
        }
        Ok(())
    }

    pub fn release_terminal(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        self.verify_with(ops)?;
        let path = self.path();
        let parent = path.parent().ok_or(TransactionError::Write)?;
        ops.sync_directory(parent)
            .map_err(|_| TransactionError::Write)?;
        ops.remove_file(&path.join("owner.json"))
            .map_err(|_| TransactionError::Write)?;
        ops.remove_dir(&path).map_err(|_| TransactionError::Write)?;
        Ok(())
    }

    fn release_durable_intermediate(
        &self,
        ops: &impl FilesystemOps,
    ) -> Result<(), TransactionError> {
        self.verify_with(ops)?;
        let path = self.path();
        ops.remove_file(&path.join("owner.json"))
            .map_err(|_| TransactionError::Write)?;
        ops.remove_dir(&path).map_err(|_| TransactionError::Write)?;
        let parent = path.parent().ok_or(TransactionError::Write)?;
        ops.sync_directory(parent)
            .map_err(|_| TransactionError::Write)
    }

    pub fn owner_hash(&self) -> &str {
        &self.owner_hash
    }

    pub fn token(&self) -> &str {
        &self.lease_token
    }
}

#[derive(Debug)]
pub struct LeaseSet {
    verdict: Lease,
    intake: IntakePublicationLease,
}

impl LeaseSet {
    pub const fn acquisition_order(&self) -> [&'static str; 2] {
        ["verdict", "intake"]
    }

    pub fn verify_all(&self) -> Result<(), TransactionError> {
        self.verdict.verify()?;
        let paths = store::StorePaths::from_root(self.verdict.root.clone());
        self.intake.verify(&paths).map_err(map_intake_error)
    }

    pub fn verify_all_with(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        self.verdict.verify_with(ops)?;
        let paths = store::StorePaths::from_root(self.verdict.root.clone());
        self.intake.verify(&paths).map_err(map_intake_error)
    }

    pub fn owner_hashes(&self) -> Vec<String> {
        let mut hashes = vec![
            self.verdict.owner_hash().to_owned(),
            self.intake.owner_hash().to_owned(),
        ];
        hashes.sort();
        hashes
    }

    pub fn store_root(&self) -> &Path {
        &self.verdict.root
    }

    pub fn release_with(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        let paths = store::StorePaths::from_root(self.verdict.root.clone());
        self.intake.release(&paths).map_err(map_intake_error)?;
        self.verdict.release_terminal(ops)
    }

    pub fn verdict_token(&self) -> &str {
        self.verdict.token()
    }

    pub fn intake_token(&self) -> &str {
        self.intake.token()
    }
}

pub fn verify_held_lease_tokens(
    paths: &store::StorePaths,
    verdict_token: &str,
    intake_token: &str,
) -> Result<(), TransactionError> {
    let verdict = read_canonical_owner(
        &StdFilesystem,
        &paths.locks.join("verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )?;
    if verdict.lease_token != verdict_token {
        return Err(TransactionError::LeaseChanged(LeaseKind::Verdict));
    }
    super::verify_intake_lease_token(paths, intake_token).map_err(map_intake_error)?;
    Ok(())
}

pub fn verify_held_verdict_lease_token(
    paths: &store::StorePaths,
    verdict_token: &str,
) -> Result<(), TransactionError> {
    let verdict = read_canonical_owner(
        &StdFilesystem,
        &paths.locks.join("verdict.lock/owner.json"),
        LeaseKind::Verdict,
    )?;
    if verdict.lease_token != verdict_token || verdict.command_class != "recovery" {
        return Err(TransactionError::LeaseChanged(LeaseKind::Verdict));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransactionPlan {
    pub schema_version: String,
    pub transaction_id: String,
    pub command_class: String,
    pub owner_hashes: Vec<String>,
    pub proposal_id: String,
    pub verdict_id: String,
    pub observed_ledger_head: Option<String>,
    pub profile_hash: String,
    pub policy_hash: String,
    pub proposal_meta_hash: String,
    pub candidate_hash: String,
    pub verb: String,
    pub target_id: String,
    pub target_path: String,
    pub base_target_hash: Option<String>,
    pub parent_ref: String,
    pub parent_manifest_hash: String,
    pub parent_bundle_hash: String,
    pub generation_id: Option<String>,
    pub approved_target_hash: Option<String>,
    pub judgment_hash: Option<String>,
    pub reason_hash: String,
    pub ruled_at: String,
}

impl TransactionPlan {
    pub fn validates(&self) -> bool {
        let hashes = [
            &self.profile_hash,
            &self.policy_hash,
            &self.proposal_meta_hash,
            &self.candidate_hash,
            &self.parent_manifest_hash,
            &self.parent_bundle_hash,
            &self.reason_hash,
        ];
        let applying = matches!(self.verb.as_str(), "ACCEPT" | "REWRITE");
        self.schema_version == TRANSACTION_PLAN_SCHEMA
            && self.command_class == "verdict"
            && valid_transaction_id(&self.transaction_id)
            && valid_proposal_id(&self.proposal_id)
            && valid_verdict_id(&self.verdict_id)
            && valid_ledger_head(self.observed_ledger_head.as_deref())
            && hashes.iter().all(|hash| valid_hash(hash))
            && self.base_target_hash.as_deref().is_none_or(valid_hash)
            && valid_plan_verb(&self.verb)
            && !self.target_id.is_empty()
            && !self.target_path.is_empty()
            && (self.parent_ref == "seed" || valid_generation_id(&self.parent_ref))
            && Timestamp::parse(&self.ruled_at).is_ok()
            && self.owner_hashes.len() == 2
            && self.owner_hashes.windows(2).all(|pair| pair[0] <= pair[1])
            && self.owner_hashes.iter().all(|hash| valid_hash(hash))
            && if applying {
                self.generation_id
                    .as_deref()
                    .is_some_and(valid_generation_id)
                    && self.approved_target_hash.as_deref().is_some_and(valid_hash)
                    && self.judgment_hash.as_deref().is_some_and(valid_hash)
            } else {
                self.generation_id.is_none()
                    && self.approved_target_hash.is_none()
                    && self.judgment_hash.is_none()
            }
    }

    pub fn plan_hash(&self, bytes: &[u8]) -> Result<String, TransactionError> {
        if canonical_json_bytes(self)? != bytes || !self.validates() {
            return Err(TransactionError::JournalState);
        }
        Ok(blake3::hash(bytes).to_hex().to_string())
    }
}

#[derive(Clone, Debug)]
pub struct TransactionPlanJournal {
    pub root: PathBuf,
    pub path: PathBuf,
    pub plan: TransactionPlan,
    pub bytes: Vec<u8>,
}

impl TransactionPlanJournal {
    pub fn hash(&self) -> String {
        blake3::hash(&self.bytes).to_hex().to_string()
    }

    pub fn verify(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        let metadata = ops
            .metadata(&self.path)
            .map_err(|_| TransactionError::JournalState)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(TransactionError::JournalState);
        }
        let bytes = ops
            .read(&self.path)
            .map_err(|_| TransactionError::JournalState)?;
        if bytes != self.bytes || self.plan.plan_hash(&bytes)? != self.hash() {
            return Err(TransactionError::JournalState);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PreparedArtifacts {
    pub verdict_event_hash: String,
    pub ledger_successor_hash: String,
    pub generation_id: Option<String>,
    pub manifest_hash: Option<String>,
    pub bundle_hash: Option<String>,
    pub reindex_receipt_hash: Option<String>,
}

impl PreparedArtifacts {
    pub fn ledger_only(verdict_event_hash: String, ledger_successor_hash: String) -> Self {
        Self {
            verdict_event_hash,
            ledger_successor_hash,
            generation_id: None,
            manifest_hash: None,
            bundle_hash: None,
            reindex_receipt_hash: None,
        }
    }

    pub fn validates_against(&self, plan: &TransactionPlan) -> bool {
        valid_hash(&self.verdict_event_hash)
            && valid_hash(&self.ledger_successor_hash)
            && if matches!(plan.verb.as_str(), "ACCEPT" | "REWRITE") {
                self.generation_id.as_deref() == plan.generation_id.as_deref()
                    && self.manifest_hash.as_deref().is_some_and(valid_hash)
                    && self.bundle_hash.as_deref().is_some_and(valid_hash)
                    && self.reindex_receipt_hash.as_deref().is_some_and(valid_hash)
            } else {
                self.generation_id.is_none()
                    && self.manifest_hash.is_none()
                    && self.bundle_hash.is_none()
                    && self.reindex_receipt_hash.is_none()
            }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PreparedJournal {
    pub schema_version: String,
    pub transaction_id: String,
    pub command_class: String,
    pub owner_hashes: Vec<String>,
    pub proposal_id: Option<String>,
    pub verdict_id: Option<String>,
    pub observed_ledger_head: Option<String>,
    pub prepared_hashes: Vec<String>,
    pub plan_hash: String,
    pub artifacts: PreparedArtifacts,
}

impl PreparedJournal {
    pub fn validates_against(&self, plan: &TransactionPlan, plan_hash: &str) -> bool {
        plan.validates()
            && self.schema_version == JOURNAL_SCHEMA
            && self.command_class == "verdict"
            && self.transaction_id == plan.transaction_id
            && self.owner_hashes == plan.owner_hashes
            && self.proposal_id.as_deref() == Some(plan.proposal_id.as_str())
            && self.verdict_id.as_deref() == Some(plan.verdict_id.as_str())
            && self.observed_ledger_head == plan.observed_ledger_head
            && self.plan_hash == plan_hash
            && valid_hash(&self.plan_hash)
            && self.prepared_hashes.iter().all(|hash| valid_hash(hash))
            && self
                .prepared_hashes
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            && self.artifacts.validates_against(plan)
    }
}

struct PreparedJournalInput<'a> {
    pub transaction_id: &'a str,
    pub proposal_id: Option<&'a str>,
    pub verdict_id: Option<&'a str>,
    pub observed_ledger_head: Option<&'a str>,
    pub prepared_hashes: Vec<String>,
    pub artifacts: PreparedArtifacts,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JournalMarker {
    pub schema_version: String,
    pub transaction_id: String,
    pub stage: String,
}

#[derive(Clone, Debug)]
pub struct TransactionJournal {
    pub root: PathBuf,
    plan: TransactionPlanJournal,
    pub prepared: PreparedJournal,
    pub bytes: Vec<u8>,
}

impl TransactionJournal {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, TransactionError> {
        canonical_json_bytes(&self.prepared)
    }

    pub fn write_ledger_committed(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        self.verify_prepared(ops)?;
        self.write_marker(ops, "ledger-committed.json", "ledger-committed")
    }

    pub fn write_published(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        self.verify_prepared(ops)?;
        self.verify_marker(ops, "ledger-committed.json", "ledger-committed")?;
        self.write_marker(ops, "published.json", "published")
    }

    fn verify_prepared(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        self.plan.verify(ops)?;
        let path = self.root.join("prepared.json");
        let metadata = ops
            .metadata(&path)
            .map_err(|_| TransactionError::JournalState)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(TransactionError::JournalState);
        }
        let bytes = ops
            .read(&path)
            .map_err(|_| TransactionError::JournalState)?;
        let prepared: PreparedJournal =
            serde_json::from_slice(&bytes).map_err(|_| TransactionError::JournalState)?;
        if prepared != self.prepared
            || !prepared.validates_against(&self.plan.plan, &self.plan.hash())
            || canonical_json_bytes(&prepared)? != bytes
        {
            return Err(TransactionError::JournalState);
        }
        Ok(())
    }

    fn verify_marker(
        &self,
        ops: &impl FilesystemOps,
        name: &str,
        stage: &str,
    ) -> Result<(), TransactionError> {
        let path = self.root.join(name);
        let metadata = ops
            .metadata(&path)
            .map_err(|_| TransactionError::JournalState)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(TransactionError::JournalState);
        }
        let bytes = ops
            .read(&path)
            .map_err(|_| TransactionError::JournalState)?;
        let marker: JournalMarker =
            serde_json::from_slice(&bytes).map_err(|_| TransactionError::JournalState)?;
        let expected = JournalMarker {
            schema_version: JOURNAL_SCHEMA.to_owned(),
            transaction_id: self.prepared.transaction_id.clone(),
            stage: stage.to_owned(),
        };
        if marker != expected || canonical_json_bytes(&marker)? != bytes {
            return Err(TransactionError::JournalState);
        }
        Ok(())
    }

    fn write_marker(
        &self,
        ops: &impl FilesystemOps,
        name: &str,
        stage: &str,
    ) -> Result<(), TransactionError> {
        let bytes = canonical_json_bytes(&JournalMarker {
            schema_version: JOURNAL_SCHEMA.to_owned(),
            transaction_id: self.prepared.transaction_id.clone(),
            stage: stage.to_owned(),
        })?;
        ops.write_new_synced(&self.root.join(name), &bytes)
            .map_err(|_| TransactionError::Write)?;
        ops.sync_directory(&self.root)
            .map_err(|_| TransactionError::Write)
    }

    pub fn cleanup_with(&self, ops: &impl FilesystemOps) -> Result<(), TransactionError> {
        self.plan.verify(ops)?;
        self.verify_prepared(ops)?;
        self.verify_marker(ops, "ledger-committed.json", "ledger-committed")?;
        match ops.metadata(&self.root.join("published.json")) {
            Ok(_) => self.verify_marker(ops, "published.json", "published")?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => self.write_published(ops)?,
            Err(_) => return Err(TransactionError::JournalState),
        }
        let parent = self.root.parent().ok_or(TransactionError::Write)?;
        let journal =
            discover_original_journal(ops, parent).map_err(|_| TransactionError::JournalState)?;
        if journal.transaction_id() != Some(self.prepared.transaction_id.as_str())
            || journal
                .prepared()
                .map(|prepared| prepared == &self.prepared)
                != Some(true)
        {
            return Err(TransactionError::JournalState);
        }
        let witness = journal
            .promote_to_cleanup_witness(ops)
            .map_err(|_| TransactionError::JournalState)?;
        remove_original_cleanup_witness_terminally(ops, &witness, parent, None)
            .map_err(|_| TransactionError::JournalState)
    }
}

pub struct TransactionRuntime<O = StdFilesystem, F = NoFaults>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    pub root: PathBuf,
    pub profile: Option<P3Profile>,
    pub inspection: LeaseInspection,
    pub ops: O,
    faults: F,
}

impl TransactionRuntime<StdFilesystem, NoFaults> {
    pub fn production(
        profile: &P3Profile,
        snapshot: &StoreSnapshot,
    ) -> Result<Self, TransactionError> {
        if profile.hash != snapshot.profile_hash {
            return Err(TransactionError::InvalidReceipt);
        }
        Ok(Self {
            root: snapshot.paths.root.clone(),
            profile: Some(profile.clone()),
            inspection: LeaseInspection::local()?,
            ops: StdFilesystem,
            faults: NoFaults,
        })
    }
}

impl<O, F> TransactionRuntime<O, F>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    pub fn with_parts(root: PathBuf, inspection: LeaseInspection, ops: O, faults: F) -> Self {
        Self {
            root,
            profile: None,
            inspection,
            ops,
            faults,
        }
    }

    pub fn acquire_verdict_leases(
        &self,
        receipt: &VerdictLeaseReceipt,
    ) -> Result<LeaseSet, TransactionError> {
        require_supported_platform()?;
        let owner_id = receipt.id();
        let verdict = self.acquire_lease(LeaseKind::Verdict, owner_id, "verdict")?;
        let paths = store::StorePaths::from_root(self.root.clone());
        let identity = IntakeLeaseIdentity::new(
            owner_id,
            "verdict",
            &self.inspection.started_at,
            self.inspection.process_id,
        )
        .map_err(map_intake_error)?;
        let intake =
            match IntakePublicationLease::acquire(&paths, identity).map_err(map_intake_error) {
                Ok(lease) => lease,
                Err(error) => {
                    let _ = verdict.release_terminal(&self.ops);
                    return Err(error);
                }
            };
        let leases = LeaseSet { verdict, intake };
        self.faults.check(FaultPoint::AfterLeaseCreation)?;
        leases.verify_all_with(&self.ops)?;
        if let Some(profile) = &self.profile {
            let receipt_error = store::inspect_store_with_transients(profile, &self.root)
                .map(|locked_store| {
                    if receipt.matches_store(&locked_store) {
                        None
                    } else {
                        Some(receipt.store_mismatch_error(&locked_store))
                    }
                })
                .unwrap_or(Some(TransactionError::InvalidReceipt));
            if let Some(error) = receipt_error {
                let _ = self.release_verdict_leases(&leases);
                return Err(error);
            }
        }
        Ok(leases)
    }

    pub fn create_prepared_journal(
        &self,
        transaction_id: &str,
        leases: &LeaseSet,
        proposal_id: Option<&str>,
        verdict_id: Option<&str>,
        observed_ledger_head: Option<&str>,
        prepared_hashes: Vec<String>,
    ) -> Result<TransactionJournal, TransactionError> {
        let proposal_id = proposal_id
            .unwrap_or("prp-0000000000000000000000000000000000000000000000000000000000000000");
        let verdict_id = verdict_id
            .unwrap_or("ver-0000000000000000000000000000000000000000000000000000000000000000");
        let plan = TransactionPlan {
            schema_version: TRANSACTION_PLAN_SCHEMA.to_owned(),
            transaction_id: transaction_id.to_owned(),
            command_class: "verdict".to_owned(),
            owner_hashes: leases.owner_hashes(),
            proposal_id: proposal_id.to_owned(),
            verdict_id: verdict_id.to_owned(),
            observed_ledger_head: observed_ledger_head.map(str::to_owned),
            profile_hash: "a".repeat(64),
            policy_hash: "b".repeat(64),
            proposal_meta_hash: "c".repeat(64),
            candidate_hash: "d".repeat(64),
            verb: "REJECT".to_owned(),
            target_id: "plan-test-target".to_owned(),
            target_path: "plan-test.md".to_owned(),
            base_target_hash: None,
            parent_ref: "seed".to_owned(),
            parent_manifest_hash: "e".repeat(64),
            parent_bundle_hash: "f".repeat(64),
            generation_id: None,
            approved_target_hash: None,
            judgment_hash: None,
            reason_hash: "0".repeat(64),
            ruled_at: "2026-07-14T00:00:00.000000000Z".to_owned(),
        };
        let plan = self.create_transaction_plan(plan, leases)?;
        self.prepare_transaction_journal(
            plan,
            leases,
            PreparedJournalInput {
                transaction_id,
                proposal_id: Some(proposal_id),
                verdict_id: Some(verdict_id),
                observed_ledger_head,
                prepared_hashes,
                artifacts: PreparedArtifacts::ledger_only("1".repeat(64), "2".repeat(64)),
            },
        )
    }

    fn create_transaction_plan(
        &self,
        plan: TransactionPlan,
        leases: &LeaseSet,
    ) -> Result<TransactionPlanJournal, TransactionError> {
        if !plan.validates() {
            return Err(TransactionError::JournalState);
        }
        leases.verify_all_with(&self.ops)?;
        if plan.owner_hashes != leases.owner_hashes() {
            return Err(TransactionError::JournalState);
        }
        let transactions = self.root.join("transactions");
        let root = transactions.join(&plan.transaction_id);
        let plan_path = root.join("plan.json");
        let prep = transactions.join(format!(".plan-{}.json", plan.transaction_id));
        let bytes = canonical_json_bytes(&plan)?;

        self.ops
            .write_new_synced(&prep, &bytes)
            .map_err(|_| TransactionError::Write)?;
        leases.verify_all_with(&self.ops)?;
        self.ops
            .sync_directory(&transactions)
            .map_err(|_| TransactionError::Write)?;
        self.faults.check(FaultPoint::AfterPlanPrepSync)?;

        leases.verify_all_with(&self.ops)?;
        self.ops
            .create_dir(&root)
            .map_err(|_| TransactionError::Write)?;
        leases.verify_all_with(&self.ops)?;
        self.ops
            .sync_directory(&transactions)
            .map_err(|_| TransactionError::Write)?;
        self.faults.check(FaultPoint::AfterPlanFinalDirectorySync)?;

        leases.verify_all_with(&self.ops)?;
        self.ops
            .hard_link(&prep, &plan_path)
            .map_err(|_| TransactionError::Write)?;
        leases.verify_all_with(&self.ops)?;
        self.ops
            .sync_directory(&root)
            .map_err(|_| TransactionError::Write)?;
        leases.verify_all_with(&self.ops)?;
        self.ops
            .sync_directory(&transactions)
            .map_err(|_| TransactionError::Write)?;
        self.faults.check(FaultPoint::AfterPlanHardLinkSync)?;

        leases.verify_all_with(&self.ops)?;
        self.ops
            .remove_file(&prep)
            .map_err(|_| TransactionError::Write)?;
        leases.verify_all_with(&self.ops)?;
        self.ops
            .sync_directory(&transactions)
            .map_err(|_| TransactionError::Write)?;
        self.faults.check(FaultPoint::AfterPlanPrepRemoval)?;
        self.faults
            .check(FaultPoint::AfterTransactionJournalCreation)?;
        self.faults.check(FaultPoint::AfterPlanSync)?;
        leases.verify_all_with(&self.ops)?;
        Ok(TransactionPlanJournal {
            path: plan_path,
            root,
            plan,
            bytes,
        })
    }

    fn prepare_transaction_journal(
        &self,
        plan: TransactionPlanJournal,
        leases: &LeaseSet,
        input: PreparedJournalInput<'_>,
    ) -> Result<TransactionJournal, TransactionError> {
        let PreparedJournalInput {
            transaction_id,
            proposal_id,
            verdict_id,
            observed_ledger_head,
            prepared_hashes,
            artifacts,
        } = input;
        let root = &plan.root;
        if *root != self.root.join("transactions").join(transaction_id) {
            return Err(TransactionError::JournalState);
        }
        let root_metadata = self
            .ops
            .metadata(root)
            .map_err(|_| TransactionError::JournalState)?;
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            return Err(TransactionError::JournalState);
        }
        leases.verify_all_with(&self.ops)?;
        plan.verify(&self.ops)?;
        let mut owner_hashes = leases.owner_hashes();
        owner_hashes.sort();
        let prepared = PreparedJournal {
            schema_version: JOURNAL_SCHEMA.to_owned(),
            transaction_id: transaction_id.to_owned(),
            command_class: "verdict".to_owned(),
            owner_hashes,
            proposal_id: proposal_id.map(str::to_owned),
            verdict_id: verdict_id.map(str::to_owned),
            observed_ledger_head: observed_ledger_head.map(str::to_owned),
            prepared_hashes,
            plan_hash: plan.hash(),
            artifacts,
        };
        if !prepared.validates_against(&plan.plan, &plan.hash()) {
            return Err(TransactionError::JournalState);
        }
        let bytes = canonical_json_bytes(&prepared)?;
        self.ops
            .write_new_synced(&root.join("prepared.json"), &bytes)
            .map_err(|_| TransactionError::Write)?;
        self.ops
            .sync_directory(root)
            .map_err(|_| TransactionError::Write)?;
        self.ops
            .sync_directory(&self.root.join("transactions"))
            .map_err(|_| TransactionError::Write)?;
        leases.verify_all_with(&self.ops)?;
        Ok(TransactionJournal {
            root: root.clone(),
            plan,
            prepared,
            bytes,
        })
    }

    pub fn release_verdict_leases(&self, leases: &LeaseSet) -> Result<(), TransactionError> {
        leases.release_with(&self.ops)
    }

    pub fn acquire_lease(
        &self,
        kind: LeaseKind,
        owner_id: &str,
        command_class: &str,
    ) -> Result<Lease, TransactionError> {
        let path = self.root.join("locks/verdict.lock");
        let parent = path.parent().ok_or(TransactionError::Write)?;
        let mut owner = LeaseOwner {
            schema_version: LEASE_OWNER_SCHEMA.to_owned(),
            owner_id: owner_id.to_owned(),
            command_class: command_class.to_owned(),
            started_at: self.inspection.started_at.as_str().to_owned(),
            process_id: self.inspection.process_id,
            lease_token: String::new(),
        };
        owner.lease_token = lease_token(&owner);
        let owner_bytes = canonical_owner_bytes(&owner)?;
        let temporary = parent.join(format!(".lease-{}-{}", kind.name(), owner.lease_token));
        self.ops
            .create_dir(&temporary)
            .map_err(|error| map_acquire_error(kind, error))?;
        let mut final_directory_created = false;
        let result = (|| {
            self.ops
                .write_new_synced(&temporary.join("owner.json"), &owner_bytes)
                .map_err(|_| TransactionError::Write)?;
            self.ops
                .sync_directory(&temporary)
                .map_err(|_| TransactionError::Write)?;
            self.ops
                .sync_directory(parent)
                .map_err(|_| TransactionError::Write)?;
            self.ops
                .create_dir(&path)
                .map_err(|error| map_acquire_error(kind, error))?;
            final_directory_created = true;
            self.ops
                .sync_directory(parent)
                .map_err(|_| TransactionError::Write)?;
            self.ops
                .hard_link(&temporary.join("owner.json"), &path.join("owner.json"))
                .map_err(|error| map_owner_link_error(kind, error))?;
            self.ops
                .sync_directory(&path)
                .map_err(|_| TransactionError::Write)?;
            self.ops
                .sync_directory(parent)
                .map_err(|_| TransactionError::Write)?;
            self.ops
                .remove_file(&temporary.join("owner.json"))
                .map_err(|_| TransactionError::Write)?;
            self.ops
                .remove_dir(&temporary)
                .map_err(|_| TransactionError::Write)?;
            self.ops
                .sync_directory(parent)
                .map_err(|_| TransactionError::Write)
        })();
        if let Err(error) = result {
            if !final_directory_created {
                let _ = self.ops.remove_file(&temporary.join("owner.json"));
                let _ = self.ops.remove_dir(&temporary);
                let _ = self.ops.sync_directory(parent);
            }
            return Err(error);
        }
        let lease = Lease {
            root: self.root.clone(),
            kind,
            owner_hash: blake3::hash(&owner_bytes).to_hex().to_string(),
            lease_token: owner.lease_token,
        };
        lease.verify_with(&self.ops)?;
        Ok(lease)
    }
}

pub fn lease_receipt_id(
    command_class: &str,
    store_id: &str,
    observed_head: Option<&str>,
    profile_hash: &str,
    policy_hash: &str,
    subject_frames: &[&str],
) -> String {
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, LEASE_RECEIPT_DOMAIN);
    frame(&mut hasher, command_class.as_bytes());
    frame(&mut hasher, store_id.as_bytes());
    frame_optional(&mut hasher, observed_head.map(str::as_bytes));
    frame(&mut hasher, profile_hash.as_bytes());
    frame(&mut hasher, policy_hash.as_bytes());
    for subject in subject_frames {
        frame(&mut hasher, subject.as_bytes());
    }
    format!("rec-{}", hasher.finalize().to_hex())
}

pub fn canonical_owner_bytes(owner: &LeaseOwner) -> Result<Vec<u8>, TransactionError> {
    canonical_json_bytes(owner)
}

pub fn canonical_json_bytes(value: &impl Serialize) -> Result<Vec<u8>, TransactionError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|_| TransactionError::Write)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn read_canonical_owner(
    ops: &impl FilesystemOps,
    path: &Path,
    kind: LeaseKind,
) -> Result<LeaseOwner, TransactionError> {
    let metadata = ops
        .metadata(path)
        .map_err(|_| TransactionError::LeaseChanged(kind))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TransactionError::LeaseChanged(kind));
    }
    let bytes = ops
        .read(path)
        .map_err(|_| TransactionError::LeaseChanged(kind))?;
    let owner: LeaseOwner =
        serde_json::from_slice(&bytes).map_err(|_| TransactionError::LeaseChanged(kind))?;
    if owner.schema_version != LEASE_OWNER_SCHEMA
        || owner.owner_id.is_empty()
        || owner.command_class.is_empty()
        || Timestamp::parse(&owner.started_at).is_err()
        || owner.lease_token != lease_token(&owner)
        || canonical_owner_bytes(&owner).map_err(|_| TransactionError::LeaseChanged(kind))? != bytes
    {
        return Err(TransactionError::LeaseChanged(kind));
    }
    Ok(owner)
}

pub fn lease_token(owner: &LeaseOwner) -> String {
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, LEASE_TOKEN_DOMAIN);
    frame(&mut hasher, owner.schema_version.as_bytes());
    frame(&mut hasher, owner.owner_id.as_bytes());
    frame(&mut hasher, owner.command_class.as_bytes());
    frame(&mut hasher, owner.started_at.as_bytes());
    frame(&mut hasher, &owner.process_id.to_le_bytes());
    hasher.finalize().to_hex().to_string()
}

fn frame(hasher: &mut blake3::Hasher, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn frame_optional(hasher: &mut blake3::Hasher, value: Option<&[u8]>) {
    match value {
        Some(value) => {
            hasher.update(&[1]);
            frame(hasher, value);
        }
        None => {
            hasher.update(&[0]);
        }
    }
}

fn map_acquire_error(kind: LeaseKind, error: io::Error) -> TransactionError {
    if error.kind() == io::ErrorKind::AlreadyExists {
        TransactionError::Locked(kind)
    } else {
        TransactionError::Write
    }
}

fn map_owner_link_error(kind: LeaseKind, error: io::Error) -> TransactionError {
    if error.kind() == io::ErrorKind::AlreadyExists {
        TransactionError::Locked(kind)
    } else {
        TransactionError::Write
    }
}

pub fn valid_receipt_id(value: &str) -> bool {
    value.len() == 68
        && value.starts_with("rec-")
        && value[4..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn valid_transaction_id(value: &str) -> bool {
    value.len() == 68 && value.starts_with("txn-") && valid_hash(&value[4..])
}

fn valid_proposal_id(value: &str) -> bool {
    value.len() == 68 && value.starts_with("prp-") && valid_hash(&value[4..])
}

fn valid_verdict_id(value: &str) -> bool {
    value.len() == 68 && value.starts_with("ver-") && valid_hash(&value[4..])
}

fn valid_plan_verb(value: &str) -> bool {
    matches!(value, "ACCEPT" | "REJECT" | "REWRITE" | "TP" | "FP")
}

fn valid_generation_id(value: &str) -> bool {
    value.len() == 20
        && value.starts_with("gen-")
        && value[4..].bytes().all(|byte| byte.is_ascii_digit())
        && &value[4..] != "0000000000000000"
}

fn valid_observed_head(value: Option<&str>) -> bool {
    value.is_none_or(|value| value == "seed" || valid_generation_id(value))
}

fn valid_ledger_head(value: Option<&str>) -> bool {
    value.is_none_or(valid_hash)
}

fn require_supported_platform() -> Result<(), TransactionError> {
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        Ok(())
    } else {
        Err(TransactionError::UnsupportedPlatform)
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TransactionError {
    #[error("a controlling terminal is required")]
    TtyRequired,
    #[error("human confirmation failed")]
    ConfirmationFailed,
    #[error("intake confirmation failed")]
    IntakeConfirmationFailed,
    #[error("intake publication lease is unavailable")]
    IntakeLocked,
    #[error("intake transaction is incomplete")]
    IntakeIncomplete,
    #[error("intake clock moved backward")]
    IntakeClockRegression,
    #[error("intake record is invalid")]
    IntakeRecordInvalid,
    #[error("intake chain is invalid")]
    IntakeChainInvalid,
    #[error("intake head does not match")]
    IntakeHeadMismatch,
    #[error("intake sealed pair binding is invalid")]
    IntakePairBinding,
    #[error("protected lease is held")]
    Locked(LeaseKind),
    #[error("lease owner changed")]
    LeaseChanged(LeaseKind),
    #[error("transaction journal state is invalid")]
    JournalState,
    #[error("transaction recovery state is ambiguous")]
    RecoveryAmbiguous,
    #[error("lease owner receipt is invalid")]
    InvalidReceipt,
    #[error("transaction write failed")]
    Write,
    #[error("transaction plan could not be prepared")]
    PlanPrepare,
    #[error("canonical receipt is invalid")]
    CanonicalReceiptInvalid,
    #[error("canonical ledger could not be reread")]
    CanonicalLedgerRead,
    #[error("canonical ledger state changed")]
    CanonicalLedgerStateChanged,
    #[error("transaction read failed")]
    Read,
    #[error("verdict request is invalid")]
    InvalidRequest,
    #[error("proposal bytes changed during preflight")]
    ProposalChanged,
    #[error("proposal was not found")]
    ProposalNotFound,
    #[error("proposal is already ruled")]
    AlreadyRuled,
    #[error("proposal is rejected by current policy")]
    PolicyRejected,
    #[error("human reason is invalid")]
    ReasonInvalid,
    #[error("human rewrite is invalid")]
    RewriteInvalid,
    #[error("verdict conflicts with proposal intent")]
    IntentConflict,
    #[error("human rewrite is required")]
    RewriteRequired,
    #[error("human rewrite is forbidden")]
    RewriteForbidden,
    #[error("Judgment document could not be rendered")]
    JudgmentRender,
    #[error("policy changed during transaction")]
    PolicyChanged,
    #[error("profile changed during transaction")]
    ProfileChanged,
    #[error("policy archive could not be made durable")]
    PolicyArchive,
    #[error("platform is unsupported")]
    UnsupportedPlatform,
    #[error("transaction fault injected")]
    FaultInjected,
}

impl TransactionError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::TtyRequired => "human.tty_required",
            Self::ConfirmationFailed => "human.confirmation_failed",
            Self::IntakeConfirmationFailed => "intake.confirmation_failed",
            Self::IntakeLocked => "intake.locked",
            Self::IntakeIncomplete => "intake.incomplete",
            Self::IntakeClockRegression => "intake.clock_regression",
            Self::IntakeRecordInvalid => "intake.record_invalid",
            Self::IntakeChainInvalid => "intake.chain_invalid",
            Self::IntakeHeadMismatch => "intake.head_mismatch",
            Self::IntakePairBinding => "intake.pair_binding",
            Self::Locked(kind) | Self::LeaseChanged(kind) => kind.locked_code(),
            Self::JournalState => "transaction.incomplete",
            Self::RecoveryAmbiguous => "transaction.recovery_ambiguous",
            Self::InvalidReceipt => "transaction.prepare_failed",
            Self::Write => "transaction.prepare_failed",
            Self::PlanPrepare => "transaction.plan_prepare_failed",
            Self::CanonicalReceiptInvalid => "transaction.canonical_receipt_invalid",
            Self::CanonicalLedgerRead => "transaction.canonical_ledger_read_failed",
            Self::CanonicalLedgerStateChanged => "transaction.canonical_ledger_state_changed",
            Self::Read => "transaction.prepare_failed",
            Self::InvalidRequest => "verdict.request_invalid",
            Self::ProposalChanged => "proposal.changed_during_transaction",
            Self::ProposalNotFound => "proposal.not_found",
            Self::AlreadyRuled => "verdict.already_ruled",
            Self::PolicyRejected => "policy.rejected",
            Self::ReasonInvalid => "human.reason_invalid",
            Self::RewriteInvalid => "human.rewrite_invalid",
            Self::IntentConflict => "verdict.intent_conflict",
            Self::RewriteRequired => "human.rewrite_required",
            Self::RewriteForbidden => "human.rewrite_forbidden",
            Self::JudgmentRender => "judgment.render",
            Self::PolicyChanged => "policy.changed_during_transaction",
            Self::ProfileChanged => "profile.changed_during_transaction",
            Self::PolicyArchive => "policy.archive_failed",
            Self::UnsupportedPlatform => "store.cross_filesystem",
            Self::FaultInjected => "transaction.prepare_failed",
        }
    }
}
