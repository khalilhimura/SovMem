use super::{
    transaction::{FaultInjector, FaultPoint, FilesystemOps, LeaseSet, TransactionError},
    valid_source_id, Confidence, Timestamp, VerdictVerb,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

pub const LEDGER_EVENT_SCHEMA: &str = "sovmem-ledger-event-v2";
const EVENT_HASH_DOMAIN: &[u8] = b"sovmem.ledger-event.v2";
#[allow(dead_code)]
static NEXT_LEDGER_TEMPORARY: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanonicalResult {
    pub generation_id: Option<String>,
    pub manifest_hash: Option<String>,
    pub bundle_hash: Option<String>,
    pub target_prior_hash: Option<String>,
    pub target_new_hash: Option<String>,
    pub judgment_hash: Option<String>,
    pub reindex_receipt_hash: Option<String>,
}

impl CanonicalResult {
    pub const fn none() -> Self {
        Self {
            generation_id: None,
            manifest_hash: None,
            bundle_hash: None,
            target_prior_hash: None,
            target_new_hash: None,
            judgment_hash: None,
            reindex_receipt_hash: None,
        }
    }

    fn is_none(&self) -> bool {
        self == &Self::none()
    }

    fn is_complete(&self) -> bool {
        self.generation_id
            .as_deref()
            .is_some_and(valid_generation_id)
            && self.manifest_hash.as_deref().is_some_and(valid_hash)
            && self.bundle_hash.as_deref().is_some_and(valid_hash)
            && self.target_new_hash.as_deref().is_some_and(valid_hash)
            && self.judgment_hash.as_deref().is_some_and(valid_hash)
            && self.reindex_receipt_hash.as_deref().is_some_and(valid_hash)
            && self.target_prior_hash.as_deref().is_none_or(valid_hash)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VerdictPayload {
    pub schema_version: String,
    pub event_type: String,
    pub sequence: u64,
    pub event_id: String,
    pub occurred_at: String,
    pub previous_event_hash: Option<String>,
    pub store_id: String,
    pub transaction_id: String,
    pub proposal_id: String,
    pub proposal_meta_hash: String,
    pub candidate_hash: String,
    pub source_id: String,
    pub verb: VerdictVerb,
    pub reason: String,
    pub verdict_id: String,
    pub actor: String,
    pub policy_hash: String,
    pub profile_hash: String,
    pub confidence: Option<Confidence>,
    pub evaluator_meta_hash: Option<String>,
    pub base_kind: String,
    pub base_generation: Option<String>,
    pub base_manifest_hash: String,
    pub base_bundle_hash: String,
    pub base_target_hash: Option<String>,
    pub rewrite_hash: Option<String>,
    pub rewrite_bytes: Option<u64>,
    pub canonical_result: CanonicalResult,
    pub resulting_canonical_head: String,
    pub intake_sequence: u64,
    pub intake_head_hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenesisEvent {
    payload: GenesisPayload,
    event_hash: String,
}

impl GenesisEvent {
    pub fn new(payload: GenesisPayload) -> Result<Self, LedgerError> {
        validate_genesis_payload(&payload)?;
        let event_hash = event_hash(&payload_bytes(&payload)?);
        Ok(Self {
            payload,
            event_hash,
        })
    }

    pub fn hash(&self) -> &str {
        &self.event_hash
    }

    pub fn payload(&self) -> &GenesisPayload {
        &self.payload
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerdictEvent {
    payload: VerdictPayload,
    event_hash: String,
}

impl VerdictEvent {
    pub fn new(payload: VerdictPayload) -> Result<Self, LedgerError> {
        validate_verdict_payload(&payload)?;
        let event_hash = event_hash(&payload_bytes(&payload)?);
        Ok(Self {
            payload,
            event_hash,
        })
    }

    pub fn hash(&self) -> &str {
        &self.event_hash
    }

    pub fn payload(&self) -> &VerdictPayload {
        &self.payload
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerEvent {
    Genesis(Box<GenesisEvent>),
    Verdict(Box<VerdictEvent>),
}

impl LedgerEvent {
    pub fn hash(&self) -> &str {
        match self {
            Self::Genesis(event) => event.hash(),
            Self::Verdict(event) => event.hash(),
        }
    }

    pub fn previous_hash(&self) -> Option<&str> {
        match self {
            Self::Genesis(event) => event.payload.previous_event_hash.as_deref(),
            Self::Verdict(event) => event.payload.previous_event_hash.as_deref(),
        }
    }

    pub fn sequence(&self) -> u64 {
        match self {
            Self::Genesis(event) => event.payload.sequence,
            Self::Verdict(event) => event.payload.sequence,
        }
    }

    pub fn event_id(&self) -> &str {
        match self {
            Self::Genesis(event) => &event.payload.event_id,
            Self::Verdict(event) => &event.payload.event_id,
        }
    }

    pub fn canonical_line_bytes(&self) -> Result<Vec<u8>, LedgerError> {
        let mut bytes = match self {
            Self::Genesis(event) => serde_json::to_vec(&GenesisLine::from(event.as_ref()))
                .map_err(|_| LedgerError::Parse)?,
            Self::Verdict(event) => serde_json::to_vec(&VerdictLine::from(event.as_ref()))
                .map_err(|_| LedgerError::Parse)?,
        };
        bytes.push(b'\n');
        Ok(bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerView {
    pub events: Vec<LedgerEvent>,
    pub terminal_proposals: BTreeMap<String, String>,
    pub verdicts: usize,
    pub accept: usize,
    pub reject: usize,
    pub rewrite: usize,
    pub tp: usize,
    pub fp: usize,
    pub applied_heads: Vec<String>,
    pub final_hash: Option<String>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum LedgerError {
    #[error("ledger could not be read")]
    Read,
    #[error("ledger is not valid UTF-8")]
    Utf8,
    #[error("ledger line encoding is invalid")]
    LineEncoding,
    #[error("ledger JSON is invalid")]
    Parse,
    #[error("ledger schema is invalid")]
    Schema,
    #[error("ledger genesis is missing")]
    GenesisMissing,
    #[error("ledger has duplicate genesis")]
    GenesisDuplicate,
    #[error("ledger sequence is invalid")]
    Sequence,
    #[error("ledger event identifier is invalid")]
    EventId,
    #[error("ledger previous hash is invalid")]
    PreviousHash,
    #[error("ledger event hash is invalid")]
    EventHash,
    #[error("ledger has a duplicate identifier")]
    DuplicateId,
    #[error("ledger reason is empty")]
    ReasonEmpty,
    #[error("ledger confidence binding is invalid")]
    ConfidenceBinding,
    #[error("ledger evaluator binding is invalid")]
    EvaluatorBinding,
    #[error("ledger verb semantics are invalid")]
    VerbSemantics,
    #[error("ledger canonical head transition is invalid")]
    HeadTransition,
    #[error("ledger intake high-water binding is invalid")]
    IntakeBinding,
    #[error("ledger changed during append")]
    AppendConflict,
    #[error("ledger could not be written")]
    Write,
    #[error("ledger append lease is invalid")]
    LeaseInvalid,
}

impl LedgerError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Read => "ledger.read",
            Self::Utf8 => "ledger.utf8",
            Self::LineEncoding => "ledger.line_encoding",
            Self::Parse => "ledger.parse",
            Self::Schema => "ledger.schema",
            Self::GenesisMissing => "ledger.genesis_missing",
            Self::GenesisDuplicate => "ledger.genesis_duplicate",
            Self::Sequence => "ledger.sequence",
            Self::EventId => "ledger.event_id",
            Self::PreviousHash => "ledger.previous_hash",
            Self::EventHash => "ledger.event_hash",
            Self::DuplicateId => "ledger.duplicate_id",
            Self::ReasonEmpty => "ledger.reason_empty",
            Self::ConfidenceBinding => "ledger.confidence_binding",
            Self::EvaluatorBinding => "ledger.evaluator_binding",
            Self::VerbSemantics => "ledger.verb_semantics",
            Self::HeadTransition => "ledger.head_transition",
            Self::IntakeBinding => "ledger.intake_binding",
            Self::AppendConflict => "ledger.append_conflict",
            Self::Write => "ledger.write",
            Self::LeaseInvalid => "store.locked",
        }
    }
}

pub fn event_hash(payload: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(EVENT_HASH_DOMAIN);
    hasher.update(&(payload.len() as u64).to_le_bytes());
    hasher.update(payload);
    hasher.finalize().to_hex().to_string()
}

pub fn read_ledger(path: &Path) -> Result<LedgerView, LedgerError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| LedgerError::Read)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(LedgerError::Read);
    }
    let bytes = fs::read(path).map_err(|_| LedgerError::Read)?;
    read_ledger_bytes(&bytes)
}

#[allow(dead_code)]
pub(crate) fn append_events(
    path: &Path,
    leases: &LeaseSet,
    events: &[LedgerEvent],
) -> Result<Vec<u8>, LedgerError> {
    if events.is_empty() {
        return Err(LedgerError::Write);
    }
    verify_ledger_leases(path, leases)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| LedgerError::Read)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(LedgerError::Read);
    }
    let existing = fs::read(path).map_err(|_| LedgerError::Read)?;
    let existing_view = read_ledger_bytes(&existing)?;
    let mut complete = existing_view.events;
    complete.extend_from_slice(events);
    validate_events(&complete)?;

    let mut successor = existing.clone();
    for event in events {
        successor.extend_from_slice(&event.canonical_line_bytes()?);
    }
    let parent = path.parent().ok_or(LedgerError::Write)?;
    let temporary = write_ledger_temporary(parent, &successor)?;
    let result = (|| {
        verify_ledger_leases(path, leases)?;
        let current = fs::read(path).map_err(|_| LedgerError::Read)?;
        if current != existing {
            return Err(LedgerError::AppendConflict);
        }
        verify_ledger_leases(path, leases)?;
        fs::rename(&temporary, path).map_err(|_| LedgerError::Write)?;
        sync_directory(parent)?;
        let verified = fs::read(path).map_err(|_| LedgerError::Read)?;
        if !verified.starts_with(&existing) {
            return Err(LedgerError::Write);
        }
        read_ledger_bytes(&verified)?;
        Ok(verified)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn append_events_with<O, F>(
    path: &Path,
    leases: &LeaseSet,
    events: &[LedgerEvent],
    expected_successor: &[u8],
    ops: &O,
    faults: &F,
) -> Result<Vec<u8>, TransactionError>
where
    O: FilesystemOps,
    F: FaultInjector,
{
    if events.is_empty() {
        return Err(TransactionError::Write);
    }
    leases.verify_all_with(ops)?;
    let metadata = ops.metadata(path).map_err(|_| TransactionError::Read)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TransactionError::Read);
    }
    let existing = ops.read(path).map_err(|_| TransactionError::Read)?;
    let successor = successor_bytes(&existing, events)?;
    if successor != expected_successor {
        return Err(TransactionError::Read);
    }
    let parent = path.parent().ok_or(TransactionError::Write)?;
    let temporary = parent.join(format!(
        ".verdicts-{}",
        NEXT_LEDGER_TEMPORARY.fetch_add(1, Ordering::Relaxed)
    ));
    ops.write_new_synced(&temporary, &successor)
        .map_err(|_| TransactionError::Write)?;
    ops.sync_directory(parent)
        .map_err(|_| TransactionError::Write)?;
    faults.check(FaultPoint::AfterLedgerTemporarySync)?;

    leases.verify_all_with(ops)?;
    let current = ops.read(path).map_err(|_| TransactionError::Read)?;
    if current != existing {
        return Err(TransactionError::Read);
    }
    leases.verify_all_with(ops)?;
    faults.check(FaultPoint::BeforeLedgerRename)?;
    ops.rename(&temporary, path)
        .map_err(|_| TransactionError::JournalState)?;
    ops.sync_directory(parent)
        .map_err(|_| TransactionError::JournalState)?;
    faults
        .check(FaultPoint::AfterLedgerRename)
        .map_err(|_| TransactionError::JournalState)?;

    let metadata = ops
        .metadata(path)
        .map_err(|_| TransactionError::JournalState)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TransactionError::JournalState);
    }
    let verified = ops.read(path).map_err(|_| TransactionError::JournalState)?;
    if verified != expected_successor {
        return Err(TransactionError::JournalState);
    }
    read_ledger_bytes(&verified).map_err(|_| TransactionError::JournalState)?;
    Ok(verified)
}

pub(crate) fn successor_bytes_with(
    path: &Path,
    events: &[LedgerEvent],
    ops: &impl FilesystemOps,
) -> Result<Vec<u8>, TransactionError> {
    let metadata = ops.metadata(path).map_err(|_| TransactionError::Read)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TransactionError::Read);
    }
    let existing = ops.read(path).map_err(|_| TransactionError::Read)?;
    successor_bytes(&existing, events)
}

fn successor_bytes(existing: &[u8], events: &[LedgerEvent]) -> Result<Vec<u8>, TransactionError> {
    if events.is_empty() {
        return Err(TransactionError::Write);
    }
    let existing_view = read_ledger_bytes(existing).map_err(|_| TransactionError::Read)?;
    let mut complete = existing_view.events;
    complete.extend_from_slice(events);
    validate_events(&complete).map_err(|_| TransactionError::Write)?;

    let mut successor = existing.to_vec();
    for event in events {
        successor.extend_from_slice(
            &event
                .canonical_line_bytes()
                .map_err(|_| TransactionError::Write)?,
        );
    }
    Ok(successor)
}

#[allow(dead_code)]
fn verify_ledger_leases(path: &Path, leases: &LeaseSet) -> Result<(), LedgerError> {
    if path != leases.store_root().join("ledger/verdicts.jsonl") {
        return Err(LedgerError::LeaseInvalid);
    }
    leases.verify_all().map_err(|_| LedgerError::LeaseInvalid)
}

fn read_ledger_bytes(bytes: &[u8]) -> Result<LedgerView, LedgerError> {
    if bytes.is_empty() {
        return Ok(LedgerView {
            events: Vec::new(),
            terminal_proposals: BTreeMap::new(),
            verdicts: 0,
            accept: 0,
            reject: 0,
            rewrite: 0,
            tp: 0,
            fp: 0,
            applied_heads: Vec::new(),
            final_hash: None,
        });
    }
    std::str::from_utf8(bytes).map_err(|_| LedgerError::Utf8)?;
    if !bytes.ends_with(b"\n") || bytes.ends_with(b"\n\n") {
        return Err(LedgerError::LineEncoding);
    }
    let mut events = Vec::new();
    for line in bytes[..bytes.len() - 1].split(|byte| *byte == b'\n') {
        if line.is_empty() {
            return Err(LedgerError::LineEncoding);
        }
        events.push(parse_line(line)?);
    }
    validate_events(&events)
}

fn parse_line(raw: &[u8]) -> Result<LedgerEvent, LedgerError> {
    let probe: EventTypeProbe = serde_json::from_slice(raw).map_err(|_| LedgerError::Parse)?;
    match probe.event_type.as_str() {
        "genesis" => {
            let wire: GenesisLine = serde_json::from_slice(raw).map_err(|_| LedgerError::Schema)?;
            let expected_hash = wire.event_hash.clone();
            let event = GenesisEvent::new(wire.payload())?;
            if expected_hash != event.event_hash {
                return Err(LedgerError::EventHash);
            }
            let canonical = LedgerEvent::Genesis(Box::new(event.clone())).canonical_line_bytes()?;
            let mut expected_line = raw.to_vec();
            expected_line.push(b'\n');
            if canonical != expected_line {
                return Err(LedgerError::LineEncoding);
            }
            Ok(LedgerEvent::Genesis(Box::new(event)))
        }
        "verdict" => {
            let wire: VerdictLine = serde_json::from_slice(raw).map_err(|_| LedgerError::Schema)?;
            let expected_hash = wire.event_hash.clone();
            let event = VerdictEvent::new(wire.payload())?;
            if expected_hash != event.event_hash {
                return Err(LedgerError::EventHash);
            }
            let canonical = LedgerEvent::Verdict(Box::new(event.clone())).canonical_line_bytes()?;
            let mut expected_line = raw.to_vec();
            expected_line.push(b'\n');
            if canonical != expected_line {
                return Err(LedgerError::LineEncoding);
            }
            Ok(LedgerEvent::Verdict(Box::new(event)))
        }
        _ => Err(LedgerError::Schema),
    }
}

fn validate_events(events: &[LedgerEvent]) -> Result<LedgerView, LedgerError> {
    if events.is_empty() {
        return Ok(LedgerView {
            events: Vec::new(),
            terminal_proposals: BTreeMap::new(),
            verdicts: 0,
            accept: 0,
            reject: 0,
            rewrite: 0,
            tp: 0,
            fp: 0,
            applied_heads: Vec::new(),
            final_hash: None,
        });
    }
    if !matches!(events.first(), Some(LedgerEvent::Genesis(_))) {
        return Err(LedgerError::GenesisMissing);
    }
    let mut terminal_proposals = BTreeMap::new();
    let mut event_ids = BTreeSet::new();
    let mut transaction_ids = BTreeSet::new();
    let mut verdict_ids = BTreeSet::new();
    let mut accept = 0;
    let mut reject = 0;
    let mut rewrite = 0;
    let mut tp = 0;
    let mut fp = 0;
    let mut applied_heads = Vec::new();
    let mut prior_hash = None;

    for (position, event) in events.iter().enumerate() {
        let sequence = u64::try_from(position).map_err(|_| LedgerError::Sequence)?;
        if event.sequence() != sequence {
            return Err(LedgerError::Sequence);
        }
        if event.event_id() != format!("evt-{sequence:016}") {
            return Err(LedgerError::EventId);
        }
        if event.previous_hash() != prior_hash.as_deref() {
            return Err(LedgerError::PreviousHash);
        }
        if !event_ids.insert(event.event_id().to_owned()) {
            return Err(LedgerError::DuplicateId);
        }

        match event {
            LedgerEvent::Genesis(event) => {
                if position != 0 {
                    return Err(LedgerError::GenesisDuplicate);
                }
                validate_genesis_payload(&event.payload)?;
                if event.event_hash != event_hash(&payload_bytes(&event.payload)?) {
                    return Err(LedgerError::EventHash);
                }
            }
            LedgerEvent::Verdict(event) => {
                if position == 0 {
                    return Err(LedgerError::GenesisMissing);
                }
                validate_verdict_payload(&event.payload)?;
                if event.event_hash != event_hash(&payload_bytes(&event.payload)?) {
                    return Err(LedgerError::EventHash);
                }
                if terminal_proposals
                    .insert(
                        event.payload.proposal_id.clone(),
                        event.payload.verdict_id.clone(),
                    )
                    .is_some()
                    || !transaction_ids.insert(event.payload.transaction_id.clone())
                    || !verdict_ids.insert(event.payload.verdict_id.clone())
                {
                    return Err(LedgerError::DuplicateId);
                }
                match event.payload.verb {
                    VerdictVerb::Accept => accept += 1,
                    VerdictVerb::Reject => reject += 1,
                    VerdictVerb::Rewrite => rewrite += 1,
                    VerdictVerb::Tp => tp += 1,
                    VerdictVerb::Fp => fp += 1,
                }
                if event.payload.verb.applies() {
                    applied_heads.push(event.payload.resulting_canonical_head.clone());
                }
            }
        }
        prior_hash = Some(event.hash().to_owned());
    }

    Ok(LedgerView {
        events: events.to_vec(),
        terminal_proposals,
        verdicts: events.len() - 1,
        accept,
        reject,
        rewrite,
        tp,
        fp,
        applied_heads,
        final_hash: prior_hash,
    })
}

fn validate_genesis_payload(payload: &GenesisPayload) -> Result<(), LedgerError> {
    if payload.schema_version != LEDGER_EVENT_SCHEMA
        || payload.event_type != "genesis"
        || Timestamp::parse(&payload.created_at).is_err()
        || payload.store_id.is_empty()
        || payload.profile_version.is_empty()
        || !valid_hash(&payload.profile_hash)
        || !valid_hash(&payload.policy_hash)
        || !valid_hash(&payload.seed_manifest_hash)
        || !valid_hash(&payload.seed_corpus_report_hash)
        || !valid_hash(&payload.seed_bundle_hash)
        || !valid_source_commit(&payload.source_commit)
        || payload.initial_canonical_ref != "seed"
    {
        return Err(LedgerError::Schema);
    }
    Ok(())
}

fn validate_verdict_payload(payload: &VerdictPayload) -> Result<(), LedgerError> {
    if payload.schema_version != LEDGER_EVENT_SCHEMA
        || payload.event_type != "verdict"
        || Timestamp::parse(&payload.occurred_at).is_err()
        || !valid_hash(payload.previous_event_hash.as_deref().unwrap_or_default())
        || payload.store_id.is_empty()
        || !valid_prefixed_hash(&payload.transaction_id, "txn-")
        || !valid_prefixed_hash(&payload.proposal_id, "prp-")
        || !valid_hash(&payload.proposal_meta_hash)
        || !valid_hash(&payload.candidate_hash)
        || !valid_source_id(&payload.source_id)
        || !valid_prefixed_hash(&payload.verdict_id, "ver-")
        || !valid_source_id(&payload.actor)
        || !valid_hash(&payload.policy_hash)
        || !valid_hash(&payload.profile_hash)
        || !valid_hash(&payload.base_manifest_hash)
        || !valid_hash(&payload.base_bundle_hash)
        || !valid_base(&payload.base_kind, payload.base_generation.as_deref())
        || !payload.base_target_hash.as_deref().is_none_or(valid_hash)
    {
        return Err(LedgerError::Schema);
    }
    let intake_high_water_valid =
        match (payload.intake_sequence, payload.intake_head_hash.as_deref()) {
            (0, None) => true,
            (sequence, Some(value)) if sequence > 0 && valid_hash(value) => true,
            _ => false,
        };
    if !intake_high_water_valid {
        return Err(LedgerError::IntakeBinding);
    }
    if payload.reason.is_empty()
        || payload.reason.chars().all(char::is_whitespace)
        || payload
            .reason
            .chars()
            .any(|value| value == '\0' || (value.is_control() && value != '\n' && value != '\t'))
    {
        return Err(LedgerError::ReasonEmpty);
    }
    if payload
        .evaluator_meta_hash
        .as_deref()
        .is_some_and(|value| !valid_hash(value))
    {
        return Err(LedgerError::EvaluatorBinding);
    }
    let rewrite_bound = match (&payload.rewrite_hash, payload.rewrite_bytes) {
        (None, None) => false,
        (Some(hash), Some(bytes)) if valid_hash(hash) && bytes > 0 => true,
        _ => return Err(LedgerError::VerbSemantics),
    };
    let base_head = payload.base_generation.as_deref().unwrap_or("seed");
    match payload.verb {
        VerdictVerb::Accept => {
            if rewrite_bound
                || !payload.canonical_result.is_complete()
                || payload.resulting_canonical_head
                    != payload
                        .canonical_result
                        .generation_id
                        .as_deref()
                        .unwrap_or_default()
            {
                return Err(LedgerError::VerbSemantics);
            }
        }
        VerdictVerb::Rewrite => {
            if !rewrite_bound
                || !payload.canonical_result.is_complete()
                || payload.resulting_canonical_head
                    != payload
                        .canonical_result
                        .generation_id
                        .as_deref()
                        .unwrap_or_default()
            {
                return Err(LedgerError::VerbSemantics);
            }
        }
        VerdictVerb::Reject | VerdictVerb::Tp | VerdictVerb::Fp => {
            if rewrite_bound || !payload.canonical_result.is_none() {
                return Err(LedgerError::VerbSemantics);
            }
            if payload.resulting_canonical_head != base_head {
                return Err(LedgerError::HeadTransition);
            }
        }
    }
    Ok(())
}

fn valid_base(kind: &str, generation: Option<&str>) -> bool {
    (kind == "seed" && generation.is_none())
        || (kind == "generation" && generation.is_some_and(valid_generation_id))
}

fn valid_generation_id(value: &str) -> bool {
    value.len() == 20
        && value.starts_with("gen-")
        && value[4..].bytes().all(|byte| byte.is_ascii_digit())
        && &value[4..] != "0000000000000000"
}

fn valid_prefixed_hash(value: &str, prefix: &str) -> bool {
    value.len() == prefix.len() + 64
        && value.starts_with(prefix)
        && valid_hash(&value[prefix.len()..])
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_source_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn payload_bytes<T: Serialize>(payload: &T) -> Result<Vec<u8>, LedgerError> {
    serde_json::to_vec(payload).map_err(|_| LedgerError::Parse)
}

#[allow(dead_code)]
fn write_ledger_temporary(parent: &Path, bytes: &[u8]) -> Result<std::path::PathBuf, LedgerError> {
    for _ in 0..1_000 {
        let sequence = NEXT_LEDGER_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".sovmem-ledger-{}-{sequence}", std::process::id()));
        let open = OpenOptions::new().write(true).create_new(true).open(&path);
        let mut file = match open {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(LedgerError::Write),
        };
        let result = file
            .write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| LedgerError::Write);
        drop(file);
        if result.is_err() {
            let _ = fs::remove_file(&path);
            return result.map(|_| path);
        }
        return Ok(path);
    }
    Err(LedgerError::Write)
}

#[allow(dead_code)]
fn sync_directory(path: &Path) -> Result<(), LedgerError> {
    std::fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| LedgerError::Write)
}

#[derive(Deserialize)]
struct EventTypeProbe {
    event_type: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GenesisLine {
    schema_version: String,
    event_type: String,
    sequence: u64,
    event_id: String,
    created_at: String,
    previous_event_hash: Option<String>,
    store_id: String,
    profile_version: String,
    profile_hash: String,
    policy_hash: String,
    seed_manifest_hash: String,
    seed_corpus_report_hash: String,
    seed_bundle_hash: String,
    source_commit: String,
    initial_canonical_ref: String,
    event_hash: String,
}

impl GenesisLine {
    fn payload(self) -> GenesisPayload {
        GenesisPayload {
            schema_version: self.schema_version,
            event_type: self.event_type,
            sequence: self.sequence,
            event_id: self.event_id,
            created_at: self.created_at,
            previous_event_hash: self.previous_event_hash,
            store_id: self.store_id,
            profile_version: self.profile_version,
            profile_hash: self.profile_hash,
            policy_hash: self.policy_hash,
            seed_manifest_hash: self.seed_manifest_hash,
            seed_corpus_report_hash: self.seed_corpus_report_hash,
            seed_bundle_hash: self.seed_bundle_hash,
            source_commit: self.source_commit,
            initial_canonical_ref: self.initial_canonical_ref,
        }
    }
}

impl From<&GenesisEvent> for GenesisLine {
    fn from(event: &GenesisEvent) -> Self {
        Self {
            schema_version: event.payload.schema_version.clone(),
            event_type: event.payload.event_type.clone(),
            sequence: event.payload.sequence,
            event_id: event.payload.event_id.clone(),
            created_at: event.payload.created_at.clone(),
            previous_event_hash: event.payload.previous_event_hash.clone(),
            store_id: event.payload.store_id.clone(),
            profile_version: event.payload.profile_version.clone(),
            profile_hash: event.payload.profile_hash.clone(),
            policy_hash: event.payload.policy_hash.clone(),
            seed_manifest_hash: event.payload.seed_manifest_hash.clone(),
            seed_corpus_report_hash: event.payload.seed_corpus_report_hash.clone(),
            seed_bundle_hash: event.payload.seed_bundle_hash.clone(),
            source_commit: event.payload.source_commit.clone(),
            initial_canonical_ref: event.payload.initial_canonical_ref.clone(),
            event_hash: event.event_hash.clone(),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VerdictLine {
    schema_version: String,
    event_type: String,
    sequence: u64,
    event_id: String,
    occurred_at: String,
    previous_event_hash: Option<String>,
    store_id: String,
    transaction_id: String,
    proposal_id: String,
    proposal_meta_hash: String,
    candidate_hash: String,
    source_id: String,
    verb: VerdictVerb,
    reason: String,
    verdict_id: String,
    actor: String,
    policy_hash: String,
    profile_hash: String,
    confidence: Option<Confidence>,
    evaluator_meta_hash: Option<String>,
    base_kind: String,
    base_generation: Option<String>,
    base_manifest_hash: String,
    base_bundle_hash: String,
    base_target_hash: Option<String>,
    rewrite_hash: Option<String>,
    rewrite_bytes: Option<u64>,
    canonical_result: CanonicalResult,
    resulting_canonical_head: String,
    intake_sequence: u64,
    intake_head_hash: Option<String>,
    event_hash: String,
}

impl VerdictLine {
    fn payload(self) -> VerdictPayload {
        VerdictPayload {
            schema_version: self.schema_version,
            event_type: self.event_type,
            sequence: self.sequence,
            event_id: self.event_id,
            occurred_at: self.occurred_at,
            previous_event_hash: self.previous_event_hash,
            store_id: self.store_id,
            transaction_id: self.transaction_id,
            proposal_id: self.proposal_id,
            proposal_meta_hash: self.proposal_meta_hash,
            candidate_hash: self.candidate_hash,
            source_id: self.source_id,
            verb: self.verb,
            reason: self.reason,
            verdict_id: self.verdict_id,
            actor: self.actor,
            policy_hash: self.policy_hash,
            profile_hash: self.profile_hash,
            confidence: self.confidence,
            evaluator_meta_hash: self.evaluator_meta_hash,
            base_kind: self.base_kind,
            base_generation: self.base_generation,
            base_manifest_hash: self.base_manifest_hash,
            base_bundle_hash: self.base_bundle_hash,
            base_target_hash: self.base_target_hash,
            rewrite_hash: self.rewrite_hash,
            rewrite_bytes: self.rewrite_bytes,
            canonical_result: self.canonical_result,
            resulting_canonical_head: self.resulting_canonical_head,
            intake_sequence: self.intake_sequence,
            intake_head_hash: self.intake_head_hash,
        }
    }
}

impl From<&VerdictEvent> for VerdictLine {
    fn from(event: &VerdictEvent) -> Self {
        Self {
            schema_version: event.payload.schema_version.clone(),
            event_type: event.payload.event_type.clone(),
            sequence: event.payload.sequence,
            event_id: event.payload.event_id.clone(),
            occurred_at: event.payload.occurred_at.clone(),
            previous_event_hash: event.payload.previous_event_hash.clone(),
            store_id: event.payload.store_id.clone(),
            transaction_id: event.payload.transaction_id.clone(),
            proposal_id: event.payload.proposal_id.clone(),
            proposal_meta_hash: event.payload.proposal_meta_hash.clone(),
            candidate_hash: event.payload.candidate_hash.clone(),
            source_id: event.payload.source_id.clone(),
            verb: event.payload.verb,
            reason: event.payload.reason.clone(),
            verdict_id: event.payload.verdict_id.clone(),
            actor: event.payload.actor.clone(),
            policy_hash: event.payload.policy_hash.clone(),
            profile_hash: event.payload.profile_hash.clone(),
            confidence: event.payload.confidence,
            evaluator_meta_hash: event.payload.evaluator_meta_hash.clone(),
            base_kind: event.payload.base_kind.clone(),
            base_generation: event.payload.base_generation.clone(),
            base_manifest_hash: event.payload.base_manifest_hash.clone(),
            base_bundle_hash: event.payload.base_bundle_hash.clone(),
            base_target_hash: event.payload.base_target_hash.clone(),
            rewrite_hash: event.payload.rewrite_hash.clone(),
            rewrite_bytes: event.payload.rewrite_bytes,
            canonical_result: event.payload.canonical_result.clone(),
            resulting_canonical_head: event.payload.resulting_canonical_head.clone(),
            intake_sequence: event.payload.intake_sequence,
            intake_head_hash: event.payload.intake_head_hash.clone(),
            event_hash: event.event_hash.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        append_events, append_events_with, successor_bytes_with, CanonicalResult, GenesisEvent,
        GenesisPayload, LedgerError, LedgerEvent, VerdictEvent, VerdictPayload,
        LEDGER_EVENT_SCHEMA,
    };

    #[test]
    fn intake_binding_requires_the_v2_ledger_schema() {
        assert_eq!(LEDGER_EVENT_SCHEMA, "sovmem-ledger-event-v2");
    }
    use crate::profile::transaction::{
        FilesystemOps, LeaseInspection, LeaseSet, NoFaults, StdFilesystem, TransactionError,
        TransactionRuntime, VerdictLeaseReceipt,
    };
    use crate::profile::{Timestamp, VerdictVerb};
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone, Copy, Debug)]
    enum PostRenameMutation {
        Whitespace,
        Symlink,
    }

    struct MutateLedgerAfterRename(PostRenameMutation);

    impl FilesystemOps for MutateLedgerAfterRename {
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
            StdFilesystem.rename(from, to)?;
            match self.0 {
                PostRenameMutation::Whitespace => {
                    let mut bytes = fs::read(to)?;
                    bytes.insert(0, b' ');
                    fs::write(to, bytes)
                }
                PostRenameMutation::Symlink => {
                    let redirected = to.with_file_name("redirected-verdicts.jsonl");
                    fs::write(&redirected, fs::read(to)?)?;
                    fs::remove_file(to)?;
                    std::os::unix::fs::symlink(redirected, to)
                }
            }
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

    fn transaction_append_fixture(name: &str) -> (std::path::PathBuf, LeaseSet, LedgerEvent) {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-ledger-transaction-{name}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        for path in ["locks", "staging", "ledger"] {
            fs::create_dir(root.join(path)).unwrap();
        }
        fs::write(root.join("ledger/verdicts.jsonl"), b"").unwrap();
        let runtime = TransactionRuntime::with_parts(
            root.clone(),
            LeaseInspection::fixed(
                Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap(),
                7,
            ),
            StdFilesystem,
            NoFaults,
        );
        let receipt = VerdictLeaseReceipt::new(
            "audy-sovmem-writeback-v2",
            Some("seed"),
            &"a".repeat(64),
            &"b".repeat(64),
            &format!("prp-{}", "a".repeat(64)),
        )
        .unwrap();
        let leases = runtime.acquire_verdict_leases(&receipt).unwrap();
        let genesis = GenesisEvent::new(GenesisPayload {
            schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "genesis".to_owned(),
            sequence: 0,
            event_id: "evt-0000000000000000".to_owned(),
            created_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
            previous_event_hash: None,
            store_id: "audy-sovmem-writeback-v2".to_owned(),
            profile_version: "audy-ssa0-p3-v2".to_owned(),
            profile_hash: "a".repeat(64),
            policy_hash: "b".repeat(64),
            seed_manifest_hash: "c".repeat(64),
            seed_corpus_report_hash: "d".repeat(64),
            seed_bundle_hash: "e".repeat(64),
            source_commit: "f".repeat(40),
            initial_canonical_ref: "seed".to_owned(),
        })
        .unwrap();
        (root, leases, LedgerEvent::Genesis(Box::new(genesis)))
    }

    #[test]
    fn c2a_transaction_append_rechecks_exact_regular_successor_after_rename() {
        for (name, mutation) in [
            ("byte-mismatch", PostRenameMutation::Whitespace),
            ("symlink", PostRenameMutation::Symlink),
        ] {
            let (root, leases, genesis) = transaction_append_fixture(name);
            let ledger = root.join("ledger/verdicts.jsonl");
            let expected = successor_bytes_with(&ledger, &[genesis.clone()], &StdFilesystem)
                .expect("prepare exact successor");
            assert_eq!(
                append_events_with(
                    &ledger,
                    &leases,
                    &[genesis],
                    &expected,
                    &MutateLedgerAfterRename(mutation),
                    &NoFaults,
                )
                .unwrap_err(),
                TransactionError::JournalState,
                "{name}"
            );
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn append_requires_exact_ordered_lease_tokens() {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-ledger-lease-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        for path in ["locks", "staging", "ledger"] {
            fs::create_dir(root.join(path)).unwrap();
        }
        let ledger = root.join("ledger/verdicts.jsonl");
        fs::write(&ledger, b"").unwrap();
        let runtime = TransactionRuntime::with_parts(
            root.clone(),
            LeaseInspection::fixed(
                Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap(),
                7,
            ),
            StdFilesystem,
            NoFaults,
        );
        let receipt = VerdictLeaseReceipt::new(
            "audy-sovmem-writeback-v2",
            Some("seed"),
            &"a".repeat(64),
            &"b".repeat(64),
            &format!("prp-{}", "a".repeat(64)),
        )
        .unwrap();
        let leases = runtime.acquire_verdict_leases(&receipt).unwrap();
        let genesis = LedgerEvent::Genesis(Box::new(
            GenesisEvent::new(GenesisPayload {
                schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
                event_type: "genesis".to_owned(),
                sequence: 0,
                event_id: "evt-0000000000000000".to_owned(),
                created_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
                previous_event_hash: None,
                store_id: "audy-sovmem-writeback-v2".to_owned(),
                profile_version: "audy-ssa0-p3-v2".to_owned(),
                profile_hash: "a".repeat(64),
                policy_hash: "b".repeat(64),
                seed_manifest_hash: "c".repeat(64),
                seed_corpus_report_hash: "d".repeat(64),
                seed_bundle_hash: "e".repeat(64),
                source_commit: "f".repeat(40),
                initial_canonical_ref: "seed".to_owned(),
            })
            .unwrap(),
        ));
        let first = append_events(&ledger, &leases, &[genesis.clone()]).unwrap();
        fs::write(root.join("locks/verdict.lock/owner.json"), b"{}\n").unwrap();
        let error = append_events(&ledger, &leases, &[genesis]).unwrap_err();
        assert_eq!(error, LedgerError::LeaseInvalid);
        assert_eq!(fs::read(&ledger).unwrap(), first);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn append_preserves_the_exact_prior_ledger_prefix() {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-ledger-prefix-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        for path in ["locks", "staging", "ledger"] {
            fs::create_dir(root.join(path)).unwrap();
        }
        let ledger = root.join("ledger/verdicts.jsonl");
        fs::write(&ledger, b"").unwrap();
        let runtime = TransactionRuntime::with_parts(
            root.clone(),
            LeaseInspection::fixed(
                Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap(),
                7,
            ),
            StdFilesystem,
            NoFaults,
        );
        let receipt = VerdictLeaseReceipt::new(
            "audy-sovmem-writeback-v2",
            Some("seed"),
            &"a".repeat(64),
            &"b".repeat(64),
            &format!("prp-{}", "b".repeat(64)),
        )
        .unwrap();
        let leases = runtime.acquire_verdict_leases(&receipt).unwrap();
        let genesis = GenesisEvent::new(GenesisPayload {
            schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "genesis".to_owned(),
            sequence: 0,
            event_id: "evt-0000000000000000".to_owned(),
            created_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
            previous_event_hash: None,
            store_id: "audy-sovmem-writeback-v2".to_owned(),
            profile_version: "audy-ssa0-p3-v2".to_owned(),
            profile_hash: "a".repeat(64),
            policy_hash: "b".repeat(64),
            seed_manifest_hash: "c".repeat(64),
            seed_corpus_report_hash: "d".repeat(64),
            seed_bundle_hash: "e".repeat(64),
            source_commit: "f".repeat(40),
            initial_canonical_ref: "seed".to_owned(),
        })
        .unwrap();
        let first = append_events(
            &ledger,
            &leases,
            &[LedgerEvent::Genesis(Box::new(genesis.clone()))],
        )
        .unwrap();
        let reject = VerdictEvent::new(VerdictPayload {
            schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "verdict".to_owned(),
            sequence: 1,
            event_id: "evt-0000000000000001".to_owned(),
            occurred_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
            previous_event_hash: Some(genesis.hash().to_owned()),
            store_id: "audy-sovmem-writeback-v2".to_owned(),
            transaction_id: "txn-".to_owned() + &"a".repeat(64),
            proposal_id: "prp-".to_owned() + &"b".repeat(64),
            proposal_meta_hash: "c".repeat(64),
            candidate_hash: "d".repeat(64),
            source_id: "src:fixture:ledger".to_owned(),
            verb: VerdictVerb::Reject,
            reason: "fixture exact-prefix rejection".to_owned(),
            verdict_id: "ver-".to_owned() + &"e".repeat(64),
            actor: "src:human:khalil".to_owned(),
            policy_hash: "b".repeat(64),
            profile_hash: "a".repeat(64),
            confidence: None,
            evaluator_meta_hash: None,
            base_kind: "seed".to_owned(),
            base_generation: None,
            base_manifest_hash: "c".repeat(64),
            base_bundle_hash: "e".repeat(64),
            base_target_hash: None,
            rewrite_hash: None,
            rewrite_bytes: None,
            canonical_result: CanonicalResult::none(),
            resulting_canonical_head: "seed".to_owned(),
            intake_sequence: 0,
            intake_head_hash: None,
        })
        .unwrap();
        let successor =
            append_events(&ledger, &leases, &[LedgerEvent::Verdict(Box::new(reject))]).unwrap();

        assert!(successor.starts_with(&first));
        assert!(successor.len() > first.len());
        let _ = fs::remove_dir_all(&root);
    }
}
