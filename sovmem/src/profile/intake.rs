use super::{
    read_proposal_pair, sync_directory, valid_source_id, verify_proposal_pair_bytes,
    write_new_synced, InboxSubmission, IntakeRejection, ProposalPair, ProposalSummary, StorePaths,
    SubmissionMeta, Timestamp, P3_STRUCTURAL_META_BYTES,
};
use serde::{Deserialize, Serialize};
#[cfg(any(test, feature = "test-support"))]
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const INTAKE_EVENT_SCHEMA: &str = "sovmem-intake-event-v1";
const INTAKE_EVENT_TYPE: &str = "intake";
const INTAKE_LEASE_OWNER_SCHEMA: &str = "sovmem-intake-lease-owner-v1";
const INTAKE_PREPARED_SCHEMA: &str = "sovmem-intake-transaction-prepared-v1";
const INTAKE_COMMITTED_SCHEMA: &str = "sovmem-intake-transaction-committed-v1";
const INTAKE_PUBLISHED_SCHEMA: &str = "sovmem-intake-transaction-published-v1";
const CANDIDATE_FILE: &str = "candidate.md";
const META_FILE: &str = "proposal.meta.json";
const SUBMISSION_META_FILE: &str = "submission.meta.json";
const PREPARED_FILE: &str = "prepared.json";
const COMMITTED_FILE: &str = "committed.json";
const PUBLISHED_FILE: &str = "published.json";
static NEXT_PAIR_TEMPORARY: AtomicU64 = AtomicU64::new(0);
static NEXT_INTAKE_LEASE_NONCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(any(test, feature = "test-support"))]
std::thread_local! {
    static INTAKE_PROMOTION_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static FAIL_INTAKE_LEASE_OWNER_PUBLICATION: Cell<bool> = const { Cell::new(false) };
    static FAIL_INTAKE_LEASE_RELEASE_FINALIZATION: Cell<bool> = const { Cell::new(false) };
    static FAIL_INTAKE_PAIR_POST_RENAME_SYNC: Cell<bool> = const { Cell::new(false) };
    static FAIL_INTAKE_EVENT_PRE_RENAME_SYNC: Cell<bool> = const { Cell::new(false) };
    static FAIL_INTAKE_EVENT_POST_RENAME_SYNC: Cell<bool> = const { Cell::new(false) };
}

#[cfg(any(test, feature = "test-support"))]
pub fn install_intake_promotion_hook_for_test(hook: impl FnOnce() + 'static) {
    INTAKE_PROMOTION_HOOK.with(|slot| {
        assert!(
            slot.borrow_mut().replace(Box::new(hook)).is_none(),
            "an intake promotion hook is already installed"
        );
    });
}

#[cfg(any(test, feature = "test-support"))]
pub fn install_intake_event_pre_rename_sync_failure_for_test() {
    FAIL_INTAKE_EVENT_PRE_RENAME_SYNC.with(|fail| {
        assert!(
            !fail.replace(true),
            "intake event sync failure is already installed"
        );
    });
}

#[cfg(any(test, feature = "test-support"))]
pub fn install_intake_pair_post_rename_sync_failure_for_test() {
    FAIL_INTAKE_PAIR_POST_RENAME_SYNC.with(|fail| {
        assert!(
            !fail.replace(true),
            "intake pair post-rename sync failure is already installed"
        );
    });
}

#[cfg(any(test, feature = "test-support"))]
pub fn install_intake_event_post_rename_sync_failure_for_test() {
    FAIL_INTAKE_EVENT_POST_RENAME_SYNC.with(|fail| {
        assert!(
            !fail.replace(true),
            "intake event post-rename sync failure is already installed"
        );
    });
}

#[cfg(any(test, feature = "test-support"))]
pub fn install_intake_lease_owner_publication_failure_for_test() {
    FAIL_INTAKE_LEASE_OWNER_PUBLICATION.with(|fail| {
        assert!(
            !fail.replace(true),
            "owner publication failure is already installed"
        );
    });
}

#[cfg(any(test, feature = "test-support"))]
pub fn install_intake_lease_release_finalization_failure_for_test() {
    FAIL_INTAKE_LEASE_RELEASE_FINALIZATION.with(|fail| {
        assert!(
            !fail.replace(true),
            "release finalization failure is already installed"
        );
    });
}

#[cfg(any(test, feature = "test-support"))]
fn run_intake_promotion_hook_for_test() {
    let hook = INTAKE_PROMOTION_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
#[allow(clippy::upper_case_acronyms)]
pub enum IntakeOutcome {
    STAGED,
    REJECTED,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntakeEvent {
    payload: IntakePayload,
    event_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IntakePreparedRecord {
    schema_version: String,
    pub submission_id: String,
    submission_candidate_hash: String,
    submission_meta_hash: String,
    prior_sequence: u64,
    prior_event_hash: Option<String>,
    pub outcome: IntakeOutcome,
    pub proposal_id: Option<String>,
    proposal_meta_hash: Option<String>,
    candidate_hash: Option<String>,
    pub event_hash: String,
    event_line_hash: String,
    pub policy_hash: String,
    pub profile_hash: String,
    event: IntakePayload,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct IntakeCommittedRecord {
    schema_version: String,
    submission_id: String,
    prepared_hash: String,
    event_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct IntakePublishedRecord {
    schema_version: String,
    submission_id: String,
    prepared_hash: String,
    committed_hash: String,
    event_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletedIntakeTransaction {
    pub submission_id: String,
    pub submission_candidate_hash: String,
    pub submission_meta_hash: String,
    pub proposal_id: Option<String>,
    pub state: &'static str,
    pub intake_sequence: u64,
    pub intake_head_hash: String,
    pub policy_hash: String,
    pub profile_hash: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct IntakeLeaseIdentity {
    owner_id: String,
    command_class: String,
    started_at: String,
    process_id: u32,
}

impl IntakeLeaseIdentity {
    pub fn new(
        owner_id: impl Into<String>,
        command_class: impl Into<String>,
        started_at: &Timestamp,
        process_id: u32,
    ) -> Result<Self, IntakeError> {
        let identity = Self {
            owner_id: owner_id.into(),
            command_class: command_class.into(),
            started_at: started_at.as_str().to_owned(),
            process_id,
        };
        if !identity.validates() {
            return Err(IntakeError::RecordInvalid);
        }
        Ok(identity)
    }

    fn validates(&self) -> bool {
        !self.owner_id.is_empty()
            && self.owner_id.len() <= 128
            && !self.command_class.is_empty()
            && self.command_class.len() <= 128
            && Timestamp::parse(&self.started_at).is_ok()
            && self.process_id != 0
    }
}

#[derive(Debug)]
pub struct IntakePublicationLease {
    root: PathBuf,
    owner_hash: String,
    token: String,
}

impl IntakePublicationLease {
    pub fn acquire(paths: &StorePaths, identity: IntakeLeaseIdentity) -> Result<Self, IntakeError> {
        if !identity.validates() {
            return Err(IntakeError::RecordInvalid);
        }
        acquire_intake_lease(paths, identity)
    }

    pub fn verify(&self, paths: &StorePaths) -> Result<(), IntakeError> {
        verify_intake_lease(self, paths)
    }

    fn verify_for_promotion(&self, paths: &StorePaths) -> Result<(), IntakeError> {
        self.verify(paths)?;
        #[cfg(any(test, feature = "test-support"))]
        run_intake_promotion_hook_for_test();
        self.verify(paths)
    }

    pub fn owner_hash(&self) -> &str {
        &self.owner_hash
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn forged_stale_capability_for_test(&self) -> Self {
        Self {
            root: self.root.clone(),
            owner_hash: self.owner_hash.clone(),
            token: self.token.clone(),
        }
    }

    pub fn release(&self, paths: &StorePaths) -> Result<(), IntakeError> {
        release_intake_lease(self, paths)
    }
}

pub fn verify_intake_lease_token(paths: &StorePaths, token: &str) -> Result<(), IntakeError> {
    let bytes = verified_lease_owner_bytes(paths)?;
    let owner = IntakeLeaseOwner::parse(&bytes)?;
    if owner.lease_token != token {
        return Err(IntakeError::Locked);
    }
    Ok(())
}

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
    #[cfg(any(test, feature = "test-support"))]
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

fn release_intake_lease(
    lease: &IntakePublicationLease,
    paths: &StorePaths,
) -> Result<(), IntakeError> {
    verify_intake_lease(lease, paths)?;
    let lock = paths.locks.join("intake.lock");
    sync_directory(&paths.locks).map_err(|_| IntakeError::Write)?;
    fs::remove_file(lock.join("owner.json")).map_err(|_| IntakeError::Write)?;
    #[cfg(any(test, feature = "test-support"))]
    if FAIL_INTAKE_LEASE_RELEASE_FINALIZATION.with(Cell::take) {
        return Err(IntakeError::Write);
    }
    fs::remove_dir(lock).map_err(|_| IntakeError::Write)
}

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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IntakeLeaseOwner {
    pub schema_version: String,
    pub owner_id: String,
    pub command_class: String,
    pub started_at: String,
    pub process_id: u32,
    pub acquisition_nonce: String,
    pub lease_token: String,
}

impl IntakeLeaseOwner {
    fn from_identity(
        identity: IntakeLeaseIdentity,
        acquisition_nonce: String,
    ) -> Result<Self, IntakeError> {
        let mut owner = Self {
            schema_version: INTAKE_LEASE_OWNER_SCHEMA.to_owned(),
            owner_id: identity.owner_id,
            command_class: identity.command_class,
            started_at: identity.started_at,
            process_id: identity.process_id,
            acquisition_nonce,
            lease_token: String::new(),
        };
        owner.lease_token = owner.token()?;
        Ok(owner)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, IntakeError> {
        let owner: Self = serde_json::from_slice(bytes).map_err(|_| IntakeError::Locked)?;
        if !owner.validates() || owner.canonical_bytes()? != bytes {
            return Err(IntakeError::Locked);
        }
        Ok(owner)
    }

    fn validates(&self) -> bool {
        self.schema_version == INTAKE_LEASE_OWNER_SCHEMA
            && IntakeLeaseIdentity {
                owner_id: self.owner_id.clone(),
                command_class: self.command_class.clone(),
                started_at: self.started_at.clone(),
                process_id: self.process_id,
            }
            .validates()
            && self.acquisition_nonce.len() == 64
            && valid_hash(&self.acquisition_nonce)
            && self.lease_token.len() == 64
            && valid_hash(&self.lease_token)
            && self.token().is_ok_and(|token| token == self.lease_token)
    }

    fn token(&self) -> Result<String, IntakeError> {
        let identity = IntakeLeaseIdentity {
            owner_id: self.owner_id.clone(),
            command_class: self.command_class.clone(),
            started_at: self.started_at.clone(),
            process_id: self.process_id,
        };
        if !identity.validates() {
            return Err(IntakeError::RecordInvalid);
        }
        let bytes = serde_json::to_vec(&identity).map_err(|_| IntakeError::RecordInvalid)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sovmem.intake-lease-token.v1");
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
        hasher.update(&(self.acquisition_nonce.len() as u64).to_le_bytes());
        hasher.update(self.acquisition_nonce.as_bytes());
        Ok(hasher.finalize().to_hex().to_string())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, IntakeError> {
        let mut bytes = serde_json::to_vec(self).map_err(|_| IntakeError::RecordInvalid)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

fn fresh_acquisition_nonce() -> Result<String, IntakeError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| IntakeError::Write)?;
    let sequence = NEXT_INTAKE_LEASE_NONCE_SEQUENCE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| IntakeError::Write)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sovmem.intake-lease-acquisition-nonce.v1");
    hasher.update(&elapsed.as_secs().to_le_bytes());
    hasher.update(&elapsed.subsec_nanos().to_le_bytes());
    hasher.update(&std::process::id().to_le_bytes());
    hasher.update(&sequence.to_le_bytes());
    Ok(hasher.finalize().to_hex().to_string())
}

impl IntakeEvent {
    pub fn new(payload: IntakePayload) -> Result<Self, IntakeError> {
        validate_payload(&payload)?;
        let event_hash = intake_event_hash(&payload_bytes(&payload)?);
        Ok(Self {
            payload,
            event_hash,
        })
    }

    pub fn payload(&self) -> &IntakePayload {
        &self.payload
    }

    pub fn hash(&self) -> &str {
        &self.event_hash
    }

    pub fn canonical_line_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(&IntakeLine::from(self))
            .expect("validated intake events always serialize");
        bytes.push(b'\n');
        bytes
    }
}

impl IntakePreparedRecord {
    pub fn new(
        submission: &InboxSubmission,
        prior: &IntakeHead,
        event: &IntakeEvent,
        pair: Option<&ProposalPair>,
    ) -> Result<Self, IntakeError> {
        let record = Self {
            schema_version: INTAKE_PREPARED_SCHEMA.to_owned(),
            submission_id: submission.meta.submission_id.clone(),
            submission_candidate_hash: raw_hash(&submission.candidate_bytes),
            submission_meta_hash: raw_hash(&submission.meta_bytes),
            prior_sequence: prior.sequence,
            prior_event_hash: prior.event_hash.clone(),
            outcome: event.payload().outcome,
            proposal_id: pair.map(|value| value.meta.proposal_id.clone()),
            proposal_meta_hash: pair.map(|value| value.meta_hash.clone()),
            candidate_hash: pair.map(|value| value.candidate_hash.clone()),
            event_hash: event.hash().to_owned(),
            event_line_hash: raw_hash(&event.canonical_line_bytes()),
            policy_hash: event.payload().policy_hash.clone(),
            profile_hash: event.payload().profile_hash.clone(),
            event: event.payload().clone(),
        };
        if !record.validates(event, pair) {
            return Err(IntakeError::RecordInvalid);
        }
        Ok(record)
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, IntakeError> {
        canonical_json_line(self)
    }

    fn validates(&self, event: &IntakeEvent, pair: Option<&ProposalPair>) -> bool {
        self.validates_event(event)
            && match (self.outcome, pair) {
                (IntakeOutcome::STAGED, Some(pair)) => {
                    self.proposal_id.as_deref() == Some(pair.meta.proposal_id.as_str())
                        && self.proposal_meta_hash.as_deref() == Some(pair.meta_hash.as_str())
                        && self.candidate_hash.as_deref() == Some(pair.candidate_hash.as_str())
                        && self.submission_candidate_hash == pair.candidate_hash
                        && event.payload().proposal_id.as_deref()
                            == Some(pair.meta.proposal_id.as_str())
                        && event.payload().proposal_meta_hash.as_deref()
                            == Some(pair.meta_hash.as_str())
                        && event.payload().candidate_hash.as_deref()
                            == Some(pair.candidate_hash.as_str())
                }
                (IntakeOutcome::REJECTED, None) => true,
                _ => false,
            }
    }

    fn validates_event(&self, event: &IntakeEvent) -> bool {
        let payload = event.payload();
        self.schema_version == INTAKE_PREPARED_SCHEMA
            && valid_submission_id(&self.submission_id)
            && valid_hash(&self.submission_candidate_hash)
            && valid_hash(&self.submission_meta_hash)
            && valid_hash(&self.event_hash)
            && valid_hash(&self.event_line_hash)
            && valid_hash(&self.policy_hash)
            && valid_hash(&self.profile_hash)
            && self.prior_sequence.checked_add(1) == Some(payload.sequence)
            && self.prior_event_hash == payload.previous_event_hash
            && self.outcome == payload.outcome
            && self.event_hash == event.hash()
            && self.event_line_hash == raw_hash(&event.canonical_line_bytes())
            && self.policy_hash == payload.policy_hash
            && self.profile_hash == payload.profile_hash
            && self.event == *payload
            && match self.outcome {
                IntakeOutcome::STAGED => {
                    self.proposal_id.as_deref() == payload.proposal_id.as_deref()
                        && self.proposal_meta_hash.as_deref()
                            == payload.proposal_meta_hash.as_deref()
                        && self.candidate_hash.as_deref() == payload.candidate_hash.as_deref()
                }
                IntakeOutcome::REJECTED => {
                    self.proposal_id.is_none()
                        && self.proposal_meta_hash.is_none()
                        && self.candidate_hash.is_none()
                }
            }
    }

    fn bound_event(&self) -> Result<IntakeEvent, IntakeError> {
        let event = IntakeEvent::new(self.event.clone())?;
        if !self.validates_event(&event) {
            return Err(IntakeError::RecordInvalid);
        }
        Ok(event)
    }
}

pub fn write_intake_prepared(
    paths: &StorePaths,
    transaction_root: &Path,
    record: &IntakePreparedRecord,
) -> Result<String, IntakeError> {
    let bytes = record.canonical_bytes()?;
    write_new_synced(&transaction_root.join(PREPARED_FILE), &bytes)
        .map_err(|_| IntakeError::Write)?;
    sync_directory(transaction_root).map_err(|_| IntakeError::Write)?;
    sync_directory(&paths.intake_transactions).map_err(|_| IntakeError::Write)?;
    Ok(raw_hash(&bytes))
}

pub fn write_intake_committed(
    paths: &StorePaths,
    transaction_root: &Path,
    prepared: &IntakePreparedRecord,
) -> Result<String, IntakeError> {
    let prepared_bytes =
        read_canonical_json_line::<IntakePreparedRecord>(&transaction_root.join(PREPARED_FILE))?.1;
    if raw_hash(&prepared_bytes) != raw_hash(&prepared.canonical_bytes()?) {
        return Err(IntakeError::RecordInvalid);
    }
    let record = IntakeCommittedRecord {
        schema_version: INTAKE_COMMITTED_SCHEMA.to_owned(),
        submission_id: prepared.submission_id.clone(),
        prepared_hash: raw_hash(&prepared_bytes),
        event_hash: prepared.event_hash.clone(),
    };
    let bytes = canonical_json_line(&record)?;
    write_new_synced(&transaction_root.join(COMMITTED_FILE), &bytes)
        .map_err(|_| IntakeError::Write)?;
    sync_directory(transaction_root).map_err(|_| IntakeError::Write)?;
    sync_directory(&paths.intake_transactions).map_err(|_| IntakeError::Write)?;
    Ok(raw_hash(&bytes))
}

pub fn write_intake_published(
    paths: &StorePaths,
    transaction_root: &Path,
    prepared: &IntakePreparedRecord,
) -> Result<(), IntakeError> {
    let prepared_bytes =
        fs::read(transaction_root.join(PREPARED_FILE)).map_err(|_| IntakeError::RecordInvalid)?;
    let committed_bytes =
        fs::read(transaction_root.join(COMMITTED_FILE)).map_err(|_| IntakeError::RecordInvalid)?;
    let record = IntakePublishedRecord {
        schema_version: INTAKE_PUBLISHED_SCHEMA.to_owned(),
        submission_id: prepared.submission_id.clone(),
        prepared_hash: raw_hash(&prepared_bytes),
        committed_hash: raw_hash(&committed_bytes),
        event_hash: prepared.event_hash.clone(),
    };
    let bytes = canonical_json_line(&record)?;
    write_new_synced(&transaction_root.join(PUBLISHED_FILE), &bytes)
        .map_err(|_| IntakeError::Write)?;
    sync_directory(transaction_root).map_err(|_| IntakeError::Write)?;
    sync_directory(&paths.intake_transactions).map_err(|_| IntakeError::Write)
}

pub fn completed_intake_transaction(
    paths: &StorePaths,
    submission_id: &str,
) -> Result<Option<CompletedIntakeTransaction>, IntakeError> {
    if !valid_submission_id(submission_id) {
        return Err(IntakeError::RecordInvalid);
    }
    let root = paths.intake_transactions.join(submission_id);
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(IntakeError::RecordInvalid),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(IntakeError::RecordInvalid)
        }
        Ok(_) => {}
    }
    verify_completed_intake_transaction(paths, &root).map(Some)
}

pub fn intake_transactions_have_incomplete_state(paths: &StorePaths) -> Result<bool, IntakeError> {
    let mut seen_events = BTreeSet::new();
    let mut entries = fs::read_dir(&paths.intake_transactions)
        .map_err(|_| IntakeError::Read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| IntakeError::Read)?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| IntakeError::RecordInvalid)?;
        let completed = match verify_completed_intake_transaction(paths, &entry.path()) {
            Ok(completed) => completed,
            Err(_) => return Ok(true),
        };
        if completed.submission_id != name || !seen_events.insert(completed.intake_head_hash) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn recover_incomplete_intake_transaction(
    paths: &StorePaths,
    max_candidate_bytes: usize,
    verify_recovery_lease: impl Fn() -> Result<(), IntakeError>,
) -> Result<bool, IntakeError> {
    let mut incomplete = Vec::new();
    let mut entries = fs::read_dir(&paths.intake_transactions)
        .map_err(|_| IntakeError::Read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| IntakeError::Read)?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if verify_completed_intake_transaction(paths, &entry.path()).is_err() {
            incomplete.push(entry.path());
        }
    }
    if incomplete.is_empty() {
        return Ok(false);
    }
    if incomplete.len() != 1 {
        return Err(IntakeError::RecordInvalid);
    }
    let root = incomplete.pop().expect("one incomplete intake transaction");
    let submission_id = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| valid_submission_id(name))
        .ok_or(IntakeError::RecordInvalid)?
        .to_owned();
    let prepared_path = root.join(PREPARED_FILE);
    let prepared = match fs::symlink_metadata(&prepared_path) {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            verify_recovery_lease()?;
            return Err(IntakeError::RecordInvalid);
        }
        Err(_) => return Err(IntakeError::RecordInvalid),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(IntakeError::RecordInvalid)
        }
        Ok(_) => read_canonical_json_line::<IntakePreparedRecord>(&prepared_path)?.0,
    };
    if prepared.submission_id != submission_id {
        return Err(IntakeError::RecordInvalid);
    }
    let prepared_event = prepared.bound_event()?;

    let events = parse_events(&fs::read(&paths.intake_events).map_err(|_| IntakeError::Read)?)?;
    let mut matches = events
        .iter()
        .filter(|event| event.hash() == prepared.event_hash);
    let committed_event = matches.next();
    if matches.next().is_some() {
        return Err(IntakeError::RecordInvalid);
    }
    let pair = match prepared.proposal_id.as_deref() {
        Some(proposal_id)
            if paths
                .intake_proposals
                .join(proposal_shard(proposal_id)?)
                .join(proposal_id)
                .exists() =>
        {
            Some(read_sealed_pair_structural(paths, proposal_id)?)
        }
        Some(_) | None => None,
    };

    if let Some(event) = committed_event {
        if events.last().map(IntakeEvent::hash) != Some(event.hash())
            || !prepared.validates(event, pair.as_ref())
        {
            return Err(IntakeError::RecordInvalid);
        }
        let committed_exists = root.join(COMMITTED_FILE).exists();
        let published_exists = root.join(PUBLISHED_FILE).exists();
        if published_exists && !committed_exists {
            return Err(IntakeError::RecordInvalid);
        }
        if committed_exists {
            validate_committed_prefix(&root, &prepared)?;
        }
        if published_exists {
            validate_published_prefix(&root, &prepared)?;
        }
        if committed_exists && published_exists {
            validate_remaining_bound_submission_inputs(&root, &prepared, max_candidate_bytes)?;
        } else {
            let submission = read_claimed_submission(&root, &submission_id, max_candidate_bytes)?;
            validate_prepared_submission_binding(&prepared, &prepared_event, &submission)?;
        }
        verify_recovery_lease()?;
        if !committed_exists {
            write_intake_committed(paths, &root, &prepared)?;
        }
        if !published_exists {
            write_intake_published(paths, &root, &prepared)?;
        }
        validate_committed_prefix(&root, &prepared)?;
        validate_published_prefix(&root, &prepared)?;
        verify_recovery_lease()?;
        remove_bound_submission_inputs(&root, &prepared, max_candidate_bytes)?;
        sync_directory(&root).map_err(|_| IntakeError::Write)?;
        sync_directory(&paths.intake_transactions).map_err(|_| IntakeError::Write)?;
        verify_completed_intake_transaction(paths, &root)?;
        return Ok(true);
    }

    let submission = read_claimed_submission(&root, &submission_id, max_candidate_bytes)?;
    validate_prepared_submission_binding(&prepared, &prepared_event, &submission)?;
    let prior_matches = match (
        prepared.prior_sequence,
        prepared.prior_event_hash.as_deref(),
    ) {
        (0, None) => events.is_empty(),
        (sequence, Some(hash)) => {
            usize::try_from(sequence).ok() == Some(events.len())
                && events.last().map(IntakeEvent::hash) == Some(hash)
        }
        _ => false,
    };
    if !prior_matches
        || root.join(COMMITTED_FILE).exists()
        || root.join(PUBLISHED_FILE).exists()
        || (prepared.outcome == IntakeOutcome::REJECTED && pair.is_some())
    {
        return Err(IntakeError::RecordInvalid);
    }
    if let Some(pair) = pair.as_ref() {
        let proposal_id = prepared
            .proposal_id
            .as_deref()
            .ok_or(IntakeError::RecordInvalid)?;
        let event = IntakeEvent::new(IntakePayload {
            schema_version: INTAKE_EVENT_SCHEMA.to_owned(),
            event_type: INTAKE_EVENT_TYPE.to_owned(),
            sequence: prepared
                .prior_sequence
                .checked_add(1)
                .ok_or(IntakeError::RecordInvalid)?,
            event_id: format!(
                "int-{:016}",
                prepared
                    .prior_sequence
                    .checked_add(1)
                    .ok_or(IntakeError::RecordInvalid)?
            ),
            occurred_at: pair.meta.submitted_at.clone(),
            previous_event_hash: prepared.prior_event_hash.clone(),
            outcome: IntakeOutcome::STAGED,
            source_id: Some(pair.meta.source_id.clone()),
            proposal_id: Some(pair.meta.proposal_id.clone()),
            proposal_meta_hash: Some(pair.meta_hash.clone()),
            candidate_hash: Some(pair.candidate_hash.clone()),
            policy_hash: pair.meta.policy_hash.clone(),
            profile_hash: pair.meta.profile_hash.clone(),
            rule_codes: Vec::new(),
        })?;
        if !prepared.validates(&event, Some(pair)) {
            return Err(IntakeError::RecordInvalid);
        }
        if proposal_id != pair.meta.proposal_id {
            return Err(IntakeError::RecordInvalid);
        }
    } else if prepared.outcome == IntakeOutcome::STAGED
        && (prepared.proposal_id.is_none()
            || prepared.proposal_meta_hash.is_none()
            || prepared.candidate_hash.is_none())
    {
        return Err(IntakeError::RecordInvalid);
    }
    verify_recovery_lease()?;
    Err(IntakeError::RecordInvalid)
}

fn validate_committed_prefix(
    root: &Path,
    prepared: &IntakePreparedRecord,
) -> Result<(), IntakeError> {
    let (record, committed_bytes) =
        read_canonical_json_line::<IntakeCommittedRecord>(&root.join(COMMITTED_FILE))?;
    let prepared_bytes =
        fs::read(root.join(PREPARED_FILE)).map_err(|_| IntakeError::RecordInvalid)?;
    if record.schema_version != INTAKE_COMMITTED_SCHEMA
        || record.submission_id != prepared.submission_id
        || record.prepared_hash != raw_hash(&prepared_bytes)
        || record.event_hash != prepared.event_hash
        || committed_bytes.is_empty()
    {
        return Err(IntakeError::RecordInvalid);
    }
    Ok(())
}

fn validate_published_prefix(
    root: &Path,
    prepared: &IntakePreparedRecord,
) -> Result<(), IntakeError> {
    let (record, _) =
        read_canonical_json_line::<IntakePublishedRecord>(&root.join(PUBLISHED_FILE))?;
    let prepared_bytes =
        fs::read(root.join(PREPARED_FILE)).map_err(|_| IntakeError::RecordInvalid)?;
    let committed_bytes =
        fs::read(root.join(COMMITTED_FILE)).map_err(|_| IntakeError::RecordInvalid)?;
    if record.schema_version != INTAKE_PUBLISHED_SCHEMA
        || record.submission_id != prepared.submission_id
        || record.prepared_hash != raw_hash(&prepared_bytes)
        || record.committed_hash != raw_hash(&committed_bytes)
        || record.event_hash != prepared.event_hash
    {
        return Err(IntakeError::RecordInvalid);
    }
    Ok(())
}

fn validate_prepared_submission_binding(
    prepared: &IntakePreparedRecord,
    prepared_event: &IntakeEvent,
    submission: &InboxSubmission,
) -> Result<(), IntakeError> {
    if prepared.submission_candidate_hash != raw_hash(&submission.candidate_bytes)
        || prepared.submission_meta_hash != raw_hash(&submission.meta_bytes)
        || prepared_event.payload().source_id.as_deref() != Some(submission.meta.source_id.as_str())
        || prepared_event.payload().policy_hash != submission.meta.policy_hash
        || prepared_event.payload().profile_hash != submission.meta.profile_hash
    {
        return Err(IntakeError::RecordInvalid);
    }
    Ok(())
}

fn validate_remaining_bound_submission_inputs(
    root: &Path,
    prepared: &IntakePreparedRecord,
    max_candidate_bytes: usize,
) -> Result<(bool, bool), IntakeError> {
    let mut names = fs::read_dir(root)
        .map_err(|_| IntakeError::RecordInvalid)?
        .map(|entry| {
            entry
                .map_err(|_| IntakeError::RecordInvalid)?
                .file_name()
                .into_string()
                .map_err(|_| IntakeError::RecordInvalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    if !names.iter().all(|name| {
        matches!(
            name.as_str(),
            CANDIDATE_FILE | SUBMISSION_META_FILE | PREPARED_FILE | COMMITTED_FILE | PUBLISHED_FILE
        )
    }) {
        return Err(IntakeError::RecordInvalid);
    }

    let candidate_exists = validate_optional_bound_input(
        &root.join(CANDIDATE_FILE),
        &prepared.submission_candidate_hash,
        max_candidate_bytes,
        true,
    )?;
    let meta_exists = validate_optional_bound_input(
        &root.join(SUBMISSION_META_FILE),
        &prepared.submission_meta_hash,
        P3_STRUCTURAL_META_BYTES,
        false,
    )?;
    Ok((candidate_exists, meta_exists))
}

fn validate_optional_bound_input(
    path: &Path,
    expected_hash: &str,
    maximum_bytes: usize,
    require_utf8: bool,
) -> Result<bool, IntakeError> {
    let metadata = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(IntakeError::RecordInvalid),
        Ok(metadata) => metadata,
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > maximum_bytes as u64
    {
        return Err(IntakeError::RecordInvalid);
    }
    let bytes = fs::read(path).map_err(|_| IntakeError::RecordInvalid)?;
    if bytes.len() > maximum_bytes
        || raw_hash(&bytes) != expected_hash
        || (require_utf8 && std::str::from_utf8(&bytes).is_err())
    {
        return Err(IntakeError::RecordInvalid);
    }
    Ok(true)
}

fn remove_bound_submission_inputs(
    root: &Path,
    prepared: &IntakePreparedRecord,
    max_candidate_bytes: usize,
) -> Result<(), IntakeError> {
    let (candidate_exists, meta_exists) =
        validate_remaining_bound_submission_inputs(root, prepared, max_candidate_bytes)?;
    if candidate_exists {
        fs::remove_file(root.join(CANDIDATE_FILE)).map_err(|_| IntakeError::Write)?;
    }
    if meta_exists {
        fs::remove_file(root.join(SUBMISSION_META_FILE)).map_err(|_| IntakeError::Write)?;
    }
    Ok(())
}

fn read_claimed_submission(
    root: &Path,
    submission_id: &str,
    max_candidate_bytes: usize,
) -> Result<InboxSubmission, IntakeError> {
    let mut names = fs::read_dir(root)
        .map_err(|_| IntakeError::RecordInvalid)?
        .map(|entry| {
            entry
                .map_err(|_| IntakeError::RecordInvalid)?
                .file_name()
                .into_string()
                .map_err(|_| IntakeError::RecordInvalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    if !names.iter().all(|name| {
        matches!(
            name.as_str(),
            CANDIDATE_FILE | SUBMISSION_META_FILE | PREPARED_FILE | COMMITTED_FILE | PUBLISHED_FILE
        )
    }) || !names.iter().any(|name| name == CANDIDATE_FILE)
        || !names.iter().any(|name| name == SUBMISSION_META_FILE)
    {
        return Err(IntakeError::RecordInvalid);
    }
    let candidate_path = root.join(CANDIDATE_FILE);
    let meta_path = root.join(SUBMISSION_META_FILE);
    for path in [&candidate_path, &meta_path] {
        let metadata = fs::symlink_metadata(path).map_err(|_| IntakeError::RecordInvalid)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(IntakeError::RecordInvalid);
        }
    }
    let candidate_bytes = fs::read(candidate_path).map_err(|_| IntakeError::RecordInvalid)?;
    let meta_bytes = fs::read(meta_path).map_err(|_| IntakeError::RecordInvalid)?;
    if candidate_bytes.len() > max_candidate_bytes || std::str::from_utf8(&candidate_bytes).is_err()
    {
        return Err(IntakeError::RecordInvalid);
    }
    let meta: SubmissionMeta =
        serde_json::from_slice(&meta_bytes).map_err(|_| IntakeError::RecordInvalid)?;
    if serde_json::to_vec(&meta).map_err(|_| IntakeError::RecordInvalid)? != meta_bytes
        || meta.schema_version != "sovmem-submission-v1"
        || meta.submission_id != submission_id
        || meta.submission_id != submission_id_for_recovery(&meta)
        || meta.candidate_hash != raw_hash(&candidate_bytes)
        || meta.candidate_bytes != candidate_bytes.len()
    {
        return Err(IntakeError::RecordInvalid);
    }
    Ok(InboxSubmission {
        meta,
        meta_bytes,
        candidate_bytes,
    })
}

fn submission_id_for_recovery(meta: &SubmissionMeta) -> String {
    let mut hasher = blake3::Hasher::new();
    for bytes in [
        b"sovmem.submission.v1".as_slice(),
        meta.submitted_at.as_bytes(),
        meta.source_id.as_bytes(),
        meta.operation.as_str().as_bytes(),
        meta.intent.as_str().as_bytes(),
        meta.target_id.as_bytes(),
        meta.target_path.as_bytes(),
        meta.candidate_hash.as_bytes(),
    ] {
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    let candidate_bytes = u64::try_from(meta.candidate_bytes)
        .expect("usize always fits in u64")
        .to_le_bytes();
    hasher.update(&(candidate_bytes.len() as u64).to_le_bytes());
    hasher.update(&candidate_bytes);
    for bytes in [meta.profile_hash.as_bytes(), meta.policy_hash.as_bytes()] {
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    format!("sub-{}", hasher.finalize().to_hex())
}

fn verify_completed_intake_transaction(
    paths: &StorePaths,
    root: &Path,
) -> Result<CompletedIntakeTransaction, IntakeError> {
    let metadata = fs::symlink_metadata(root).map_err(|_| IntakeError::RecordInvalid)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(IntakeError::RecordInvalid);
    }
    let mut names = fs::read_dir(root)
        .map_err(|_| IntakeError::RecordInvalid)?
        .map(|entry| {
            entry
                .map_err(|_| IntakeError::RecordInvalid)?
                .file_name()
                .into_string()
                .map_err(|_| IntakeError::RecordInvalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    if names != [COMMITTED_FILE, PREPARED_FILE, PUBLISHED_FILE] {
        return Err(IntakeError::RecordInvalid);
    }
    let (prepared, prepared_bytes) =
        read_canonical_json_line::<IntakePreparedRecord>(&root.join(PREPARED_FILE))?;
    let (committed, committed_bytes) =
        read_canonical_json_line::<IntakeCommittedRecord>(&root.join(COMMITTED_FILE))?;
    let (published, _) =
        read_canonical_json_line::<IntakePublishedRecord>(&root.join(PUBLISHED_FILE))?;
    if committed.schema_version != INTAKE_COMMITTED_SCHEMA
        || published.schema_version != INTAKE_PUBLISHED_SCHEMA
        || committed.submission_id != prepared.submission_id
        || published.submission_id != prepared.submission_id
        || committed.prepared_hash != raw_hash(&prepared_bytes)
        || published.prepared_hash != committed.prepared_hash
        || published.committed_hash != raw_hash(&committed_bytes)
        || committed.event_hash != prepared.event_hash
        || published.event_hash != prepared.event_hash
    {
        return Err(IntakeError::RecordInvalid);
    }
    let view = read_intake(paths)?;
    let event = view
        .events()
        .iter()
        .find(|event| event.hash() == prepared.event_hash)
        .ok_or(IntakeError::RecordInvalid)?;
    let pair = match prepared.outcome {
        IntakeOutcome::STAGED => Some(read_sealed_pair_structural(
            paths,
            prepared
                .proposal_id
                .as_deref()
                .ok_or(IntakeError::RecordInvalid)?,
        )?),
        IntakeOutcome::REJECTED => None,
    };
    if !prepared.validates(event, pair.as_ref()) {
        return Err(IntakeError::RecordInvalid);
    }
    Ok(CompletedIntakeTransaction {
        submission_id: prepared.submission_id,
        submission_candidate_hash: prepared.submission_candidate_hash,
        submission_meta_hash: prepared.submission_meta_hash,
        proposal_id: prepared.proposal_id,
        state: match prepared.outcome {
            IntakeOutcome::STAGED => "staged",
            IntakeOutcome::REJECTED => "rejected-at-intake",
        },
        intake_sequence: event.payload().sequence,
        intake_head_hash: event.hash().to_owned(),
        policy_hash: prepared.policy_hash,
        profile_hash: prepared.profile_hash,
    })
}

fn canonical_json_line(value: &impl Serialize) -> Result<Vec<u8>, IntakeError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| IntakeError::RecordInvalid)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn read_canonical_json_line<T>(path: &Path) -> Result<(T, Vec<u8>), IntakeError>
where
    T: for<'de> Deserialize<'de> + Serialize,
{
    let metadata = fs::symlink_metadata(path).map_err(|_| IntakeError::RecordInvalid)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(IntakeError::RecordInvalid);
    }
    let bytes = fs::read(path).map_err(|_| IntakeError::RecordInvalid)?;
    let value = serde_json::from_slice::<T>(&bytes).map_err(|_| IntakeError::RecordInvalid)?;
    if canonical_json_line(&value)? != bytes {
        return Err(IntakeError::RecordInvalid);
    }
    Ok((value, bytes))
}

#[derive(Clone, Debug)]
pub struct IntakeView {
    events: Vec<IntakeEvent>,
    projections: Vec<IntakeProjection>,
    head: IntakeHead,
}

impl IntakeView {
    pub fn events(&self) -> &[IntakeEvent] {
        &self.events
    }

    pub fn head(&self) -> &IntakeHead {
        &self.head
    }
}

#[derive(Clone, Debug)]
enum IntakeProjection {
    Staged(ProposalSummary),
    Rejected(IntakeRejection),
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum IntakeError {
    #[error("intake could not be read")]
    Read,
    #[error("intake record is invalid")]
    RecordInvalid,
    #[error("intake chain is invalid")]
    ChainInvalid,
    #[error("intake sealed pair binding is invalid")]
    PairBinding,
    #[error("intake head does not match")]
    HeadMismatch,
    #[error("intake clock moved backward")]
    ClockRegression,
    #[error("intake publication lease is unavailable")]
    Locked,
    #[error("intake could not be written")]
    Write,
}

impl IntakeError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Read => "intake.read",
            Self::RecordInvalid => "intake.record_invalid",
            Self::ChainInvalid => "intake.chain_invalid",
            Self::PairBinding => "intake.pair_binding",
            Self::HeadMismatch => "intake.head_mismatch",
            Self::ClockRegression => "intake.clock_regression",
            Self::Locked => "intake.locked",
            Self::Write => "intake.write",
        }
    }
}

pub fn intake_event_hash(payload: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sovmem.intake-event.v1");
    hasher.update(&(payload.len() as u64).to_le_bytes());
    hasher.update(payload);
    hasher.finalize().to_hex().to_string()
}

pub fn read_intake(paths: &StorePaths) -> Result<IntakeView, IntakeError> {
    let metadata = fs::symlink_metadata(&paths.intake_events).map_err(|_| IntakeError::Read)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(IntakeError::Read);
    }
    let bytes = fs::read(&paths.intake_events).map_err(|_| IntakeError::Read)?;
    let events = parse_events(&bytes)?;
    materialize_view(paths, events, None)
}

pub fn read_sealed_pair_structural(
    paths: &StorePaths,
    proposal_id: &str,
) -> Result<ProposalPair, IntakeError> {
    read_proposal_pair(
        &paths.intake_proposals,
        proposal_id,
        P3_STRUCTURAL_META_BYTES,
    )
    .map_err(|_| IntakeError::PairBinding)
}

pub fn read_sealed_pair(
    paths: &StorePaths,
    proposal_id: &str,
) -> Result<ProposalPair, IntakeError> {
    read_sealed_pair_structural(paths, proposal_id)
}

pub fn intake_prefix(view: &IntakeView, head: &IntakeHead) -> Result<IntakePrefix, IntakeError> {
    let count = if head.sequence == 0 {
        if head.event_hash.is_some() {
            return Err(IntakeError::HeadMismatch);
        }
        0
    } else {
        let index = usize::try_from(head.sequence - 1).map_err(|_| IntakeError::HeadMismatch)?;
        let event = view.events.get(index).ok_or(IntakeError::HeadMismatch)?;
        if head.event_hash.as_deref() != Some(event.hash()) {
            return Err(IntakeError::HeadMismatch);
        }
        index + 1
    };

    let mut staged = Vec::new();
    let mut rejected = Vec::new();
    for projection in view.projections.iter().take(count) {
        match projection {
            IntakeProjection::Staged(summary) => staged.push(summary.clone()),
            IntakeProjection::Rejected(summary) => rejected.push(summary.clone()),
        }
    }
    Ok(IntakePrefix {
        head: head.clone(),
        staged,
        rejected,
    })
}

pub fn publish_sealed_pair(
    paths: &StorePaths,
    lease: &mut IntakePublicationLease,
    pair: &ProposalPair,
) -> Result<PathBuf, IntakeError> {
    lease.verify(paths)?;
    verify_proposal_pair_bytes(
        &pair.meta.proposal_id,
        pair.candidate_bytes.clone(),
        pair.meta_bytes.clone(),
    )
    .map_err(|_| IntakeError::PairBinding)?;
    if pair.meta_hash != raw_hash(&pair.meta_bytes)
        || pair.candidate_hash != raw_hash(&pair.candidate_bytes)
    {
        return Err(IntakeError::PairBinding);
    }
    regular_directory(&paths.intake_proposals)?;

    let shard = paths
        .intake_proposals
        .join(proposal_shard(&pair.meta.proposal_id)?);
    ensure_shard(&paths.intake_proposals, &shard)?;
    let destination = shard.join(&pair.meta.proposal_id);
    reject_collision(&destination)?;
    let temporary = create_pair_temporary(&shard, &pair.meta.proposal_id)?;
    let publication = (|| {
        write_new_synced(&temporary.join(CANDIDATE_FILE), &pair.candidate_bytes)
            .map_err(|_| IntakeError::Write)?;
        write_new_synced(&temporary.join(META_FILE), &pair.meta_bytes)
            .map_err(|_| IntakeError::Write)?;
        sync_directory(&temporary).map_err(|_| IntakeError::Write)?;

        lease.verify_for_promotion(paths)?;
        reject_collision(&destination)?;
        fs::rename(&temporary, &destination).map_err(|_| IntakeError::Write)?;
        #[cfg(any(test, feature = "test-support"))]
        if FAIL_INTAKE_PAIR_POST_RENAME_SYNC.with(Cell::take) {
            return Err(IntakeError::Write);
        }
        sync_directory(&shard).map_err(|_| IntakeError::Write)?;
        sync_directory(&paths.intake_proposals).map_err(|_| IntakeError::Write)?;
        Ok(destination)
    })();
    if publication.is_err() && temporary.exists() {
        let _ = fs::remove_dir_all(&temporary);
    }
    publication
}

pub fn append_intake_event(
    paths: &StorePaths,
    lease: &mut IntakePublicationLease,
    event: &IntakeEvent,
) -> Result<IntakeView, IntakeError> {
    append_intake_event_with_boundaries(paths, lease, event, || Ok(()), || Ok(()))
}

pub fn append_intake_event_with_boundaries(
    paths: &StorePaths,
    lease: &mut IntakePublicationLease,
    event: &IntakeEvent,
    before_rename: impl FnOnce() -> Result<(), IntakeError>,
    after_rename: impl FnOnce() -> Result<(), IntakeError>,
) -> Result<IntakeView, IntakeError> {
    lease.verify(paths)?;
    validate_payload(event.payload())?;
    if intake_event_hash(&payload_bytes(event.payload())?) != event.hash() {
        return Err(IntakeError::ChainInvalid);
    }

    let metadata = fs::symlink_metadata(&paths.intake_events).map_err(|_| IntakeError::Read)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(IntakeError::Read);
    }
    let existing = fs::read(&paths.intake_events).map_err(|_| IntakeError::Read)?;
    let prior_events = parse_events(&existing)?;
    let pending = match event.payload().outcome {
        IntakeOutcome::STAGED => event.payload().proposal_id.as_deref(),
        IntakeOutcome::REJECTED => None,
    };
    let prior = materialize_view(paths, prior_events, pending)?;
    let expected_sequence = prior
        .head
        .sequence
        .checked_add(1)
        .ok_or(IntakeError::ChainInvalid)?;
    if event.payload().sequence != expected_sequence
        || event.payload().event_id != format!("int-{expected_sequence:016}")
        || event.payload().previous_event_hash != prior.head.event_hash
    {
        return Err(IntakeError::ChainInvalid);
    }
    if prior.events.last().is_some_and(|previous| {
        event.payload().occurred_at.as_str() < previous.payload().occurred_at.as_str()
    }) {
        return Err(IntakeError::ClockRegression);
    }

    let mut successor = existing.clone();
    successor.extend_from_slice(&event.canonical_line_bytes());
    let successor_events = parse_events(&successor)?;
    materialize_view(paths, successor_events, None)?;

    let temporary = paths
        .intake
        .join(format!(".events-{}", event.payload().sequence));
    reject_collision(&temporary)?;
    write_new_synced(&temporary, &successor).map_err(|_| IntakeError::Write)?;

    let publication = (|| {
        sync_intake_before_event_promotion(paths)?;
        lease.verify(paths)?;
        let current = fs::read(&paths.intake_events).map_err(|_| IntakeError::Read)?;
        if current != existing {
            return Err(IntakeError::RecordInvalid);
        }
        let current_events = parse_events(&current)?;
        let current_view = materialize_view(paths, current_events, pending)?;
        if current_view.head != prior.head {
            return Err(IntakeError::RecordInvalid);
        }
        lease.verify_for_promotion(paths)?;
        before_rename()?;
        fs::rename(&temporary, &paths.intake_events).map_err(|_| IntakeError::Write)?;
        after_rename()?;
        #[cfg(any(test, feature = "test-support"))]
        if FAIL_INTAKE_EVENT_POST_RENAME_SYNC.with(Cell::take) {
            return Err(IntakeError::Write);
        }
        sync_directory(&paths.intake).map_err(|_| IntakeError::Write)?;
        let committed = fs::read(&paths.intake_events).map_err(|_| IntakeError::Read)?;
        if committed != successor {
            return Err(IntakeError::Write);
        }
        read_intake(paths)
    })();
    if publication.is_err() && temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    publication
}

fn sync_intake_before_event_promotion(paths: &StorePaths) -> Result<(), IntakeError> {
    #[cfg(any(test, feature = "test-support"))]
    if FAIL_INTAKE_EVENT_PRE_RENAME_SYNC.with(Cell::take) {
        return Err(IntakeError::Write);
    }
    sync_directory(&paths.intake).map_err(|_| IntakeError::Write)
}

fn parse_events(bytes: &[u8]) -> Result<Vec<IntakeEvent>, IntakeError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    std::str::from_utf8(bytes).map_err(|_| IntakeError::RecordInvalid)?;
    if !bytes.ends_with(b"\n") || bytes.ends_with(b"\n\n") {
        return Err(IntakeError::RecordInvalid);
    }

    let mut events = Vec::new();
    for line in bytes[..bytes.len() - 1].split(|byte| *byte == b'\n') {
        if line.is_empty() {
            return Err(IntakeError::RecordInvalid);
        }
        events.push(parse_event(line)?);
    }
    validate_chain(&events)?;
    Ok(events)
}

fn parse_event(raw: &[u8]) -> Result<IntakeEvent, IntakeError> {
    let line: IntakeLine = serde_json::from_slice(raw).map_err(|_| IntakeError::RecordInvalid)?;
    let recorded_hash = line.event_hash.clone();
    let event = IntakeEvent::new(line.payload())?;
    if event.hash() != recorded_hash {
        return Err(IntakeError::ChainInvalid);
    }
    let mut expected = raw.to_vec();
    expected.push(b'\n');
    if event.canonical_line_bytes() != expected {
        return Err(IntakeError::RecordInvalid);
    }
    Ok(event)
}

fn validate_chain(events: &[IntakeEvent]) -> Result<(), IntakeError> {
    let mut prior_hash = None;
    let mut prior_time: Option<&str> = None;
    for (position, event) in events.iter().enumerate() {
        let expected_sequence = u64::try_from(position)
            .ok()
            .and_then(|position| position.checked_add(1))
            .ok_or(IntakeError::ChainInvalid)?;
        if event.payload().sequence != expected_sequence
            || event.payload().event_id != format!("int-{expected_sequence:016}")
            || event.payload().previous_event_hash.as_deref() != prior_hash
        {
            return Err(IntakeError::ChainInvalid);
        }
        if prior_time.is_some_and(|prior| event.payload().occurred_at.as_str() < prior) {
            return Err(IntakeError::ClockRegression);
        }
        prior_hash = Some(event.hash());
        prior_time = Some(event.payload().occurred_at.as_str());
    }
    Ok(())
}

fn materialize_view(
    paths: &StorePaths,
    events: Vec<IntakeEvent>,
    allowed_pending_pair: Option<&str>,
) -> Result<IntakeView, IntakeError> {
    let mut referenced_pairs = BTreeSet::new();
    for event in &events {
        if event.payload().outcome == IntakeOutcome::STAGED {
            let proposal_id = event
                .payload()
                .proposal_id
                .as_deref()
                .ok_or(IntakeError::RecordInvalid)?;
            if !referenced_pairs.insert(proposal_id.to_owned()) {
                return Err(IntakeError::PairBinding);
            }
        }
    }
    let actual_pairs = sealed_pair_ids(&paths.intake_proposals)?;
    let mut expected_pairs = referenced_pairs.clone();
    if let Some(pending) = allowed_pending_pair {
        if referenced_pairs.contains(pending) || !valid_proposal_id(pending) {
            return Err(IntakeError::PairBinding);
        }
        expected_pairs.insert(pending.to_owned());
    }
    if actual_pairs != expected_pairs {
        return Err(IntakeError::PairBinding);
    }

    let mut projections = Vec::with_capacity(events.len());
    for event in &events {
        match event.payload().outcome {
            IntakeOutcome::STAGED => {
                let pair = read_sealed_pair_structural(
                    paths,
                    event
                        .payload()
                        .proposal_id
                        .as_deref()
                        .ok_or(IntakeError::RecordInvalid)?,
                )?;
                verify_event_pair_binding(event, &pair)?;
                projections.push(IntakeProjection::Staged(ProposalSummary {
                    proposal_id: pair.meta.proposal_id,
                    state: "staged".to_owned(),
                    submitted_at: event.payload().occurred_at.clone(),
                    source_id: pair.meta.source_id,
                    target_id: pair.meta.target_id,
                    target_path: pair.meta.target_path,
                    candidate_hash: pair.candidate_hash,
                }));
            }
            IntakeOutcome::REJECTED => {
                projections.push(IntakeProjection::Rejected(IntakeRejection {
                    schema_version: INTAKE_EVENT_SCHEMA.to_owned(),
                    receipt_id: event.payload().event_id.clone(),
                    received_at: event.payload().occurred_at.clone(),
                    source_id: event.payload().source_id.clone(),
                    rule_codes: event.payload().rule_codes.clone(),
                }));
            }
        }
    }
    let head = events.last().map_or(
        IntakeHead {
            sequence: 0,
            event_hash: None,
        },
        |event| IntakeHead {
            sequence: event.payload().sequence,
            event_hash: Some(event.hash().to_owned()),
        },
    );
    Ok(IntakeView {
        events,
        projections,
        head,
    })
}

fn verify_event_pair_binding(event: &IntakeEvent, pair: &ProposalPair) -> Result<(), IntakeError> {
    let payload = event.payload();
    if payload.source_id.as_deref() != Some(pair.meta.source_id.as_str())
        || payload.proposal_id.as_deref() != Some(pair.meta.proposal_id.as_str())
        || payload.proposal_meta_hash.as_deref() != Some(pair.meta_hash.as_str())
        || payload.candidate_hash.as_deref() != Some(pair.candidate_hash.as_str())
        || payload.policy_hash != pair.meta.policy_hash
        || payload.profile_hash != pair.meta.profile_hash
        || payload.occurred_at != pair.meta.submitted_at
    {
        return Err(IntakeError::PairBinding);
    }
    Ok(())
}

fn validate_payload(payload: &IntakePayload) -> Result<(), IntakeError> {
    if payload.schema_version != INTAKE_EVENT_SCHEMA
        || payload.event_type != INTAKE_EVENT_TYPE
        || !valid_intake_event_id(&payload.event_id)
        || Timestamp::parse(&payload.occurred_at).is_err()
        || payload
            .previous_event_hash
            .as_deref()
            .is_some_and(|hash| !valid_hash(hash))
        || !valid_hash(&payload.policy_hash)
        || !valid_hash(&payload.profile_hash)
    {
        return Err(IntakeError::RecordInvalid);
    }
    match payload.outcome {
        IntakeOutcome::STAGED => {
            if !payload.source_id.as_deref().is_some_and(valid_source_id)
                || !payload
                    .proposal_id
                    .as_deref()
                    .is_some_and(valid_proposal_id)
                || !payload
                    .proposal_meta_hash
                    .as_deref()
                    .is_some_and(valid_hash)
                || !payload.candidate_hash.as_deref().is_some_and(valid_hash)
                || !payload.rule_codes.is_empty()
            {
                return Err(IntakeError::RecordInvalid);
            }
        }
        IntakeOutcome::REJECTED => {
            if payload
                .source_id
                .as_deref()
                .is_some_and(|id| !valid_source_id(id))
                || payload.proposal_id.is_some()
                || payload.proposal_meta_hash.is_some()
                || payload.candidate_hash.is_some()
                || payload.rule_codes.is_empty()
                || !payload.rule_codes.iter().all(|code| valid_rule_code(code))
                || !payload
                    .rule_codes
                    .windows(2)
                    .all(|codes| codes[0] < codes[1])
            {
                return Err(IntakeError::RecordInvalid);
            }
        }
    }
    Ok(())
}

fn payload_bytes(payload: &IntakePayload) -> Result<Vec<u8>, IntakeError> {
    serde_json::to_vec(payload).map_err(|_| IntakeError::RecordInvalid)
}

fn raw_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn reject_collision(path: &Path) -> Result<(), IntakeError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(IntakeError::RecordInvalid),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err(IntakeError::Write),
    }
}

fn proposal_shard(proposal_id: &str) -> Result<&str, IntakeError> {
    if !valid_proposal_id(proposal_id) {
        return Err(IntakeError::RecordInvalid);
    }
    Ok(&proposal_id[4..6])
}

fn create_pair_temporary(root: &Path, proposal_id: &str) -> Result<PathBuf, IntakeError> {
    proposal_shard(proposal_id)?;
    for _ in 0..1_000 {
        let sequence = NEXT_PAIR_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!(
            ".proposal-{proposal_id}-{}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(IntakeError::Write),
        }
    }
    Err(IntakeError::Write)
}

fn ensure_shard(root: &Path, shard: &Path) -> Result<(), IntakeError> {
    match fs::create_dir(shard) {
        Ok(()) => sync_directory(root).map_err(|_| IntakeError::Write),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => regular_directory(shard),
        Err(_) => Err(IntakeError::Write),
    }
}

fn sealed_pair_ids(root: &Path) -> Result<BTreeSet<String>, IntakeError> {
    regular_directory(root)?;
    let mut ids = BTreeSet::new();
    for shard in sorted_entries(root)? {
        let shard_name = shard
            .file_name()
            .into_string()
            .map_err(|_| IntakeError::PairBinding)?;
        if !valid_shard(&shard_name) {
            return Err(IntakeError::PairBinding);
        }
        regular_directory(&shard.path())?;
        for pair in sorted_entries(&shard.path())? {
            let proposal_id = pair
                .file_name()
                .into_string()
                .map_err(|_| IntakeError::PairBinding)?;
            if !valid_proposal_id(&proposal_id)
                || proposal_id[4..6] != shard_name
                || !pair
                    .file_type()
                    .map_err(|_| IntakeError::PairBinding)?
                    .is_dir()
                || !ids.insert(proposal_id)
            {
                return Err(IntakeError::PairBinding);
            }
        }
    }
    Ok(ids)
}

fn regular_directory(path: &Path) -> Result<(), IntakeError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| IntakeError::PairBinding)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(IntakeError::PairBinding);
    }
    Ok(())
}

fn sorted_entries(path: &Path) -> Result<Vec<fs::DirEntry>, IntakeError> {
    let mut entries = fs::read_dir(path)
        .map_err(|_| IntakeError::PairBinding)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| IntakeError::PairBinding)?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

fn valid_shard(value: &str) -> bool {
    value.len() == 2 && value.bytes().all(is_lower_hex)
}

fn valid_proposal_id(value: &str) -> bool {
    value.len() == 68 && value.starts_with("prp-") && value[4..].bytes().all(is_lower_hex)
}

fn valid_intake_event_id(value: &str) -> bool {
    value.len() == 20
        && value.starts_with("int-")
        && value[4..].bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_submission_id(value: &str) -> bool {
    value.len() == 68
        && value.starts_with("sub-")
        && value[4..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(is_lower_hex)
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn valid_rule_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._".contains(&byte))
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct IntakeLine {
    schema_version: String,
    event_type: String,
    sequence: u64,
    event_id: String,
    occurred_at: String,
    previous_event_hash: Option<String>,
    outcome: IntakeOutcome,
    source_id: Option<String>,
    proposal_id: Option<String>,
    proposal_meta_hash: Option<String>,
    candidate_hash: Option<String>,
    policy_hash: String,
    profile_hash: String,
    rule_codes: Vec<String>,
    event_hash: String,
}

impl IntakeLine {
    fn payload(self) -> IntakePayload {
        IntakePayload {
            schema_version: self.schema_version,
            event_type: self.event_type,
            sequence: self.sequence,
            event_id: self.event_id,
            occurred_at: self.occurred_at,
            previous_event_hash: self.previous_event_hash,
            outcome: self.outcome,
            source_id: self.source_id,
            proposal_id: self.proposal_id,
            proposal_meta_hash: self.proposal_meta_hash,
            candidate_hash: self.candidate_hash,
            policy_hash: self.policy_hash,
            profile_hash: self.profile_hash,
            rule_codes: self.rule_codes,
        }
    }
}

impl From<&IntakeEvent> for IntakeLine {
    fn from(event: &IntakeEvent) -> Self {
        Self {
            schema_version: event.payload.schema_version.clone(),
            event_type: event.payload.event_type.clone(),
            sequence: event.payload.sequence,
            event_id: event.payload.event_id.clone(),
            occurred_at: event.payload.occurred_at.clone(),
            previous_event_hash: event.payload.previous_event_hash.clone(),
            outcome: event.payload.outcome,
            source_id: event.payload.source_id.clone(),
            proposal_id: event.payload.proposal_id.clone(),
            proposal_meta_hash: event.payload.proposal_meta_hash.clone(),
            candidate_hash: event.payload.candidate_hash.clone(),
            policy_hash: event.payload.policy_hash.clone(),
            profile_hash: event.payload.profile_hash.clone(),
            rule_codes: event.payload.rule_codes.clone(),
            event_hash: event.event_hash.clone(),
        }
    }
}
