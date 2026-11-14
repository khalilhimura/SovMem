use super::{
    inspect_store, load_p3_profile, sync_directory, valid_relative_path, write_new_synced, Clock,
    SystemClock, Timestamp, VerdictVerb,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[cfg(any(test, feature = "test-support"))]
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::ErrorKind;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

const PROPOSAL_SCHEMA: &str = "sovmem-proposal-v1";
const SUBMISSION_SCHEMA: &str = "sovmem-submission-v1";
const SUBMISSION_ID_DOMAIN: &[u8] = b"sovmem.submission.v1";
const PROPOSAL_ID_DOMAIN: &[u8] = b"sovmem.proposal.v1";
const CANDIDATE_FILE: &str = "candidate.md";
const META_FILE: &str = "proposal.meta.json";
const SUBMISSION_META_FILE: &str = "submission.meta.json";
static NEXT_PROPOSAL_TEMPORARY: AtomicU64 = AtomicU64::new(0);

#[cfg(any(test, feature = "test-support"))]
std::thread_local! {
    static PROJECTION_POST_LEVEL4_HOOK: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

#[cfg(any(test, feature = "test-support"))]
pub fn install_projection_post_level4_hook_for_test(hook: impl FnOnce() + 'static) {
    PROJECTION_POST_LEVEL4_HOOK.with(|slot| {
        assert!(
            slot.borrow_mut().replace(Box::new(hook)).is_none(),
            "a projection post-Level-4 hook is already installed"
        );
    });
}

#[cfg(any(test, feature = "test-support"))]
pub fn run_projection_post_level4_hook_for_test() {
    let hook = PROJECTION_POST_LEVEL4_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

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

impl Serialize for Confidence {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let raw = serde_json::value::RawValue::from_string(self.canonical_json_number())
            .map_err(serde::ser::Error::custom)?;
        raw.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Confidence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Box::<serde_json::value::RawValue>::deserialize(deserializer)?;
        Self::parse(raw.get()).map_err(|_| serde::de::Error::custom("confidence is invalid"))
    }
}

impl Confidence {
    fn canonical_json_number(self) -> String {
        match self.0 {
            0 => "0".to_owned(),
            1_000_000 => "1".to_owned(),
            millionths => format!("0.{millionths:06}")
                .trim_end_matches('0')
                .to_owned(),
        }
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

#[derive(Clone, Debug)]
pub struct ProposalPair {
    pub meta: ProposalMeta,
    pub meta_bytes: Vec<u8>,
    pub candidate_bytes: Vec<u8>,
    pub meta_hash: String,
    pub candidate_hash: String,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SubmissionMeta {
    pub schema_version: String,
    pub submission_id: String,
    pub submitted_at: String,
    pub source_id: String,
    pub operation: ProposalOperation,
    pub intent: ProposalIntent,
    pub target_id: String,
    pub target_path: String,
    pub candidate_hash: String,
    pub candidate_bytes: usize,
    pub policy_hash: String,
    pub profile_hash: String,
    pub confidence: Option<Confidence>,
    pub evaluator: Option<EvaluatorBlock>,
}

#[derive(Clone, Debug)]
pub struct InboxSubmission {
    pub meta: SubmissionMeta,
    pub meta_bytes: Vec<u8>,
    pub candidate_bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmitProposalResult {
    pub submission_id: String,
    // Retained temporarily for crate-internal fixture migration and always
    // empty: an inbox submission is not a protected proposal identity.
    pub proposal_id: String,
    pub state: String,
    pub operation: ProposalOperation,
    pub intent: ProposalIntent,
    pub target_id: String,
    pub candidate_hash: String,
    pub base_kind: String,
    pub base_generation: Option<String>,
    pub bundle_hash: String,
    pub manifest_hash: String,
    pub policy_hash: String,
    pub profile_hash: String,
    pub submitted_at: String,
}

#[derive(Clone, Debug)]
pub struct ProposalDraft {
    pub submitted_at: Timestamp,
    pub source_id: String,
    pub operation: ProposalOperation,
    pub intent: ProposalIntent,
    pub target_id: String,
    pub target_path: String,
    pub base: ProposalBase,
    pub candidate_bytes: Vec<u8>,
    pub policy_hash: String,
    pub profile_hash: String,
    pub confidence: Option<Confidence>,
    pub evaluator: Option<EvaluatorBlock>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalSummary {
    pub proposal_id: String,
    pub state: String,
    pub submitted_at: String,
    pub source_id: String,
    pub target_id: String,
    pub target_path: String,
    pub candidate_hash: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalState {
    Submitted,
    RejectedAtIntake,
    Staged,
    Ruled,
    Applied,
    Superseded,
    Retained,
}

impl ProposalState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::RejectedAtIntake => "rejected-at-intake",
            Self::Staged => "staged",
            Self::Ruled => "ruled",
            Self::Applied => "applied",
            Self::Superseded => "superseded",
            Self::Retained => "retained",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalFilter {
    pub state: Option<ProposalState>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EvaluatorSummary {
    pub source_id: String,
    pub recommendation: String,
    pub reason_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposalRecord {
    pub submission_id: Option<String>,
    pub proposal_id: String,
    pub receipt_id: Option<String>,
    pub state: ProposalState,
    pub submitted_at: String,
    pub source_id: Option<String>,
    pub operation: Option<ProposalOperation>,
    pub intent: Option<ProposalIntent>,
    pub target_id: Option<String>,
    pub candidate_hash: Option<String>,
    pub confidence: Option<Confidence>,
    pub evaluator: Option<EvaluatorSummary>,
    pub terminal_verb: Option<VerdictVerb>,
    pub verdict_id: Option<String>,
    pub retained: bool,
    pub policy_stale: bool,
    pub base_kind: Option<String>,
    pub base_generation: Option<String>,
    pub base_bundle_hash: Option<String>,
    pub base_manifest_hash: Option<String>,
    pub base_target_hash: Option<String>,
    pub policy_hash: Option<String>,
    pub profile_hash: Option<String>,
    pub metadata_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluator_reasons: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VerdictRecord {
    #[serde(skip)]
    pub sequence: u64,
    pub event_id: String,
    pub event_hash: String,
    pub occurred_at: String,
    pub proposal_id: String,
    pub verdict_id: String,
    pub source_id: String,
    pub verb: VerdictVerb,
    pub actor: String,
    pub confidence: Option<Confidence>,
    pub evaluator: Option<EvaluatorSummary>,
    pub applied: bool,
    #[serde(skip)]
    pub target_id: String,
    pub candidate_hash: String,
    pub rewrite_hash: Option<String>,
    pub target_prior_hash: Option<String>,
    pub target_new_hash: Option<String>,
    pub judgment_hash: Option<String>,
    #[serde(skip)]
    pub policy_hash: String,
    #[serde(skip)]
    pub profile_hash: String,
    pub generation_id: Option<String>,
    pub manifest_hash: Option<String>,
    pub bundle_hash: Option<String>,
    pub reindex_receipt_hash: Option<String>,
    pub reason_hash: String,
    pub reason_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluator_reasons: Option<Vec<String>>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ProposalReadError {
    #[error("proposal read model could not be verified")]
    Verification { code: &'static str },
    #[error("proposal record could not be found")]
    NotFound,
}

impl ProposalReadError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Verification { code } => code,
            Self::NotFound => "proposal.not_found",
        }
    }
}

pub fn proposal_list(
    profile: &super::P3Profile,
    store_root: &Path,
    filter: ProposalFilter,
) -> Result<Vec<ProposalRecord>, ProposalReadError> {
    let initial = inspect_store(profile, store_root).map_err(read_store_error)?;
    let (snapshot, intake, verified_ledger_head) = if initial.sealed {
        let (snapshot, intake, ledger_head) = sealed_projection_snapshot(
            profile,
            store_root,
            initial,
            run_proposal_projection_post_level4_hook,
        )?;
        (snapshot, intake, Some(ledger_head))
    } else {
        let snapshot = inspect_store(profile, store_root).map_err(read_store_error)?;
        if snapshot.sealed != initial.sealed {
            return Err(read_error("transaction.incomplete"));
        }
        let intake =
            super::read_intake(&snapshot.paths).map_err(|error| read_error(error.code()))?;
        (snapshot, intake, None)
    };
    let intake_sequences = intake
        .events()
        .iter()
        .filter_map(|event| {
            event
                .payload()
                .proposal_id
                .as_ref()
                .map(|proposal_id| (proposal_id.clone(), event.payload().sequence))
        })
        .collect::<BTreeMap<_, _>>();
    let rejection_sequences = intake
        .events()
        .iter()
        .filter(|event| event.payload().proposal_id.is_none())
        .map(|event| (event.payload().event_id.clone(), event.payload().sequence))
        .collect::<BTreeMap<_, _>>();
    let prefix =
        super::intake_prefix(&intake, intake.head()).map_err(|error| read_error(error.code()))?;
    let pairs = prefix
        .staged
        .iter()
        .map(|summary| super::read_sealed_pair_structural(&snapshot.paths, &summary.proposal_id))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| read_error(error.code()))?;
    let receipts = prefix.rejected;
    let ledger_view = if let Some(verified_head) = verified_ledger_head.as_deref() {
        let view =
            super::read_ledger(&snapshot.paths.ledger).map_err(|error| read_error(error.code()))?;
        require_verified_ledger_head(verified_head, view.final_hash.as_deref())?;
        Some(view)
    } else {
        super::verify_unsealed_store(profile, store_root).map_err(read_store_error)?;
        None
    };
    let submissions = read_complete_inbox(&snapshot)?;

    let mut pairs_by_id = BTreeMap::new();
    for pair in pairs {
        if pairs_by_id
            .insert(pair.meta.proposal_id.clone(), pair)
            .is_some()
        {
            return Err(read_error("ledger.duplicate_id"));
        }
    }
    let mut terminal = BTreeMap::new();
    let mut superseded = BTreeSet::new();
    if let Some(view) = &ledger_view {
        let mut latest_applied_target = BTreeMap::new();
        for event in view.events.iter().skip(1) {
            let super::LedgerEvent::Verdict(event) = event else {
                return Err(read_error("ledger.genesis_duplicate"));
            };
            let event = event.payload();
            let pair = pairs_by_id
                .get(&event.proposal_id)
                .ok_or_else(|| read_error("ledger.proposal_binding"))?;
            if event.verb.applies() {
                if let Some(previous) = latest_applied_target
                    .insert(pair.meta.target_id.clone(), pair.meta.proposal_id.clone())
                {
                    superseded.insert(previous);
                }
            }
            terminal.insert(
                pair.meta.proposal_id.clone(),
                (event.sequence, event.clone()),
            );
        }
    }

    let mut records = pairs_by_id
        .values()
        .map(|pair| {
            let terminal_event = terminal.get(&pair.meta.proposal_id);
            let state = match terminal_event.map(|(_, event)| event.verb) {
                None => ProposalState::Staged,
                Some(verb) if !verb.applies() => ProposalState::Ruled,
                Some(_) if superseded.contains(&pair.meta.proposal_id) => ProposalState::Superseded,
                Some(_) => ProposalState::Applied,
            };
            let evaluator = pair.meta.evaluator.as_ref().map(|block| EvaluatorSummary {
                source_id: block.source_id.clone(),
                recommendation: block.recommendation.clone(),
                reason_count: block.reasons.len(),
            });
            (
                ProposalRecord {
                    submission_id: None,
                    proposal_id: pair.meta.proposal_id.clone(),
                    receipt_id: None,
                    state,
                    submitted_at: pair.meta.submitted_at.clone(),
                    source_id: Some(pair.meta.source_id.clone()),
                    operation: Some(pair.meta.operation),
                    intent: Some(pair.meta.intent),
                    target_id: Some(pair.meta.target_id.clone()),
                    candidate_hash: Some(pair.candidate_hash.clone()),
                    confidence: pair.meta.confidence,
                    evaluator,
                    terminal_verb: terminal_event.map(|(_, event)| event.verb),
                    verdict_id: terminal_event.map(|(_, event)| event.verdict_id.clone()),
                    retained: true,
                    policy_stale: terminal_event.is_none()
                        && (pair.meta.policy_hash != snapshot.policy.hash
                            || pair.meta_bytes.len() > snapshot.policy.policy.max_meta_bytes),
                    base_kind: Some(pair.meta.base.kind.clone()),
                    base_generation: pair.meta.base.generation_id.clone(),
                    base_bundle_hash: Some(pair.meta.base.bundle_hash.clone()),
                    base_manifest_hash: Some(pair.meta.base.manifest_hash.clone()),
                    base_target_hash: pair.meta.base.target_hash.clone(),
                    policy_hash: Some(pair.meta.policy_hash.clone()),
                    profile_hash: Some(pair.meta.profile_hash.clone()),
                    metadata_hash: Some(pair.meta_hash.clone()),
                    target_path: None,
                    reason: None,
                    evaluator_reasons: None,
                },
                intake_sequences.get(&pair.meta.proposal_id).copied(),
            )
        })
        .collect::<Vec<_>>();
    records.extend(receipts.into_iter().map(|receipt| {
        (
            ProposalRecord {
                submission_id: None,
                proposal_id: String::new(),
                receipt_id: Some(receipt.receipt_id.clone()),
                state: ProposalState::RejectedAtIntake,
                submitted_at: receipt.received_at,
                source_id: receipt.source_id,
                operation: None,
                intent: None,
                target_id: None,
                candidate_hash: None,
                confidence: None,
                evaluator: None,
                terminal_verb: None,
                verdict_id: None,
                retained: false,
                policy_stale: false,
                base_kind: None,
                base_generation: None,
                base_bundle_hash: None,
                base_manifest_hash: None,
                base_target_hash: None,
                policy_hash: None,
                profile_hash: None,
                metadata_hash: None,
                target_path: None,
                reason: None,
                evaluator_reasons: None,
            },
            rejection_sequences.get(&receipt.receipt_id).copied(),
        )
    }));
    records.extend(submissions.into_iter().map(|submission| {
        let meta = submission.meta;
        let evaluator = meta.evaluator.as_ref().map(|block| EvaluatorSummary {
            source_id: block.source_id.clone(),
            recommendation: block.recommendation.clone(),
            reason_count: block.reasons.len(),
        });
        (
            ProposalRecord {
                submission_id: Some(meta.submission_id),
                proposal_id: String::new(),
                receipt_id: None,
                state: ProposalState::Submitted,
                submitted_at: meta.submitted_at,
                source_id: Some(meta.source_id),
                operation: Some(meta.operation),
                intent: Some(meta.intent),
                target_id: Some(meta.target_id),
                candidate_hash: Some(meta.candidate_hash),
                confidence: meta.confidence,
                evaluator,
                terminal_verb: None,
                verdict_id: None,
                retained: false,
                policy_stale: meta.policy_hash != snapshot.policy.hash
                    || submission.meta_bytes.len() > snapshot.policy.policy.max_meta_bytes,
                base_kind: None,
                base_generation: None,
                base_bundle_hash: None,
                base_manifest_hash: None,
                base_target_hash: None,
                policy_hash: Some(meta.policy_hash),
                profile_hash: Some(meta.profile_hash),
                metadata_hash: Some(blake3::hash(&submission.meta_bytes).to_hex().to_string()),
                target_path: None,
                reason: None,
                evaluator_reasons: None,
            },
            None,
        )
    }));
    records.sort_by(|(left, left_sequence), (right, right_sequence)| {
        proposal_record_order(left, *left_sequence)
            .cmp(&proposal_record_order(right, *right_sequence))
    });

    let mut output = Vec::new();
    for (record, _) in records {
        let include = match filter.state {
            None => true,
            Some(ProposalState::Retained) => record.retained,
            Some(state) => record.state == state,
        };
        if include {
            output.push(record);
        }
    }
    Ok(output)
}

pub fn proposal_status(
    profile: &super::P3Profile,
    store_root: &Path,
    proposal_id: &str,
) -> Result<ProposalRecord, ProposalReadError> {
    proposal_list(profile, store_root, ProposalFilter { state: None })?
        .into_iter()
        .find(|record| {
            record.submission_id.as_deref() == Some(proposal_id)
                || record.proposal_id == proposal_id
                || record.receipt_id.as_deref() == Some(proposal_id)
        })
        .ok_or(ProposalReadError::NotFound)
}

pub fn proposal_status_with_advice(
    profile: &super::P3Profile,
    store_root: &Path,
    proposal_id: &str,
    include_advice: bool,
) -> Result<ProposalRecord, ProposalReadError> {
    let mut record = proposal_status(profile, store_root, proposal_id)?;
    if include_advice && !record.proposal_id.is_empty() {
        let snapshot = inspect_store(profile, store_root).map_err(read_store_error)?;
        let pair = super::read_sealed_pair_structural(&snapshot.paths, &record.proposal_id)
            .map_err(|error| read_error(error.code()))?;
        record.evaluator_reasons = Some(
            pair.meta
                .evaluator
                .map(|evaluator| evaluator.reasons)
                .unwrap_or_default(),
        );
    }
    Ok(record)
}

pub fn verdict_list(
    profile: &super::P3Profile,
    store_root: &Path,
    include_reason: bool,
    include_advice: bool,
) -> Result<Vec<VerdictRecord>, ProposalReadError> {
    verdict_list_with_post_level4_hook(profile, store_root, include_reason, include_advice, || {})
}

#[cfg(any(test, feature = "test-support"))]
pub fn verdict_list_with_test_hook(
    profile: &super::P3Profile,
    store_root: &Path,
    include_reason: bool,
    include_advice: bool,
    hook: impl FnOnce(),
) -> Result<Vec<VerdictRecord>, ProposalReadError> {
    verdict_list_with_post_level4_hook(profile, store_root, include_reason, include_advice, hook)
}

fn verdict_list_with_post_level4_hook(
    profile: &super::P3Profile,
    store_root: &Path,
    include_reason: bool,
    include_advice: bool,
    hook: impl FnOnce(),
) -> Result<Vec<VerdictRecord>, ProposalReadError> {
    let initial = inspect_store(profile, store_root).map_err(read_store_error)?;
    let (snapshot, intake, verified_ledger_head) =
        sealed_projection_snapshot(profile, store_root, initial, hook)?;
    let prefix =
        super::intake_prefix(&intake, intake.head()).map_err(|error| read_error(error.code()))?;
    let pairs = prefix
        .staged
        .iter()
        .map(|summary| {
            super::read_sealed_pair_structural(&snapshot.paths, &summary.proposal_id)
                .map(|pair| (pair.meta.proposal_id.clone(), pair))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|error| read_error(error.code()))?;
    let view =
        super::read_ledger(&snapshot.paths.ledger).map_err(|error| read_error(error.code()))?;
    require_verified_ledger_head(&verified_ledger_head, view.final_hash.as_deref())?;
    let mut records = Vec::with_capacity(view.verdicts);
    for event in view.events.iter().skip(1) {
        let super::LedgerEvent::Verdict(event) = event else {
            return Err(read_error("ledger.genesis_duplicate"));
        };
        let event_hash = event.hash().to_owned();
        let event = event.payload();
        let pair = pairs
            .get(&event.proposal_id)
            .ok_or_else(|| read_error("ledger.proposal_binding"))?;
        records.push(VerdictRecord {
            sequence: event.sequence,
            event_id: event.event_id.clone(),
            event_hash,
            occurred_at: event.occurred_at.clone(),
            proposal_id: event.proposal_id.clone(),
            verdict_id: event.verdict_id.clone(),
            source_id: event.source_id.clone(),
            verb: event.verb,
            actor: event.actor.clone(),
            confidence: event.confidence,
            evaluator: pair.meta.evaluator.as_ref().map(|block| EvaluatorSummary {
                source_id: block.source_id.clone(),
                recommendation: block.recommendation.clone(),
                reason_count: block.reasons.len(),
            }),
            applied: event.verb.applies(),
            target_id: pair.meta.target_id.clone(),
            candidate_hash: event.candidate_hash.clone(),
            rewrite_hash: event.rewrite_hash.clone(),
            target_prior_hash: event.canonical_result.target_prior_hash.clone(),
            target_new_hash: event.canonical_result.target_new_hash.clone(),
            judgment_hash: event.canonical_result.judgment_hash.clone(),
            policy_hash: event.policy_hash.clone(),
            profile_hash: event.profile_hash.clone(),
            generation_id: event.canonical_result.generation_id.clone(),
            manifest_hash: event.canonical_result.manifest_hash.clone(),
            bundle_hash: event.canonical_result.bundle_hash.clone(),
            reindex_receipt_hash: event.canonical_result.reindex_receipt_hash.clone(),
            reason_hash: blake3::hash(event.reason.as_bytes()).to_hex().to_string(),
            reason_bytes: event.reason.len(),
            reason: include_reason.then(|| event.reason.clone()),
            evaluator_reasons: include_advice.then(|| {
                pair.meta
                    .evaluator
                    .as_ref()
                    .map(|block| block.reasons.clone())
                    .unwrap_or_default()
            }),
        });
    }
    Ok(records)
}

pub fn run_proposal_projection_post_level4_hook() {
    #[cfg(any(test, feature = "test-support"))]
    run_projection_post_level4_hook_for_test();
}

pub fn sealed_projection_snapshot(
    profile: &super::P3Profile,
    store_root: &Path,
    initial: super::StoreSnapshot,
    post_level4: impl FnOnce(),
) -> Result<(super::StoreSnapshot, super::IntakeView, String), ProposalReadError> {
    if !initial.sealed {
        return super::verify_store_level4(profile, store_root)
            .map(|_| unreachable!("Level 4 cannot succeed for an unsealed store"))
            .map_err(|error| read_error(error.code()));
    }
    let verified_intake_head = super::read_intake(&initial.paths)
        .map_err(|error| read_error(error.code()))?
        .head()
        .clone();
    let verified_ledger_head = super::verify_store_level4(profile, store_root)
        .map_err(|error| read_error(error.code()))?
        .ledger_head;
    post_level4();
    let snapshot = inspect_store(profile, store_root).map_err(read_store_error)?;
    if !snapshot.sealed {
        return Err(read_error("transaction.incomplete"));
    }
    let intake = super::read_intake(&snapshot.paths).map_err(|error| read_error(error.code()))?;
    if intake.head() != &verified_intake_head {
        return Err(read_error("transaction.incomplete"));
    }
    Ok((snapshot, intake, verified_ledger_head))
}

// Kept as a narrow boundary so the post-Level-4 concurrent-head case can be
// driven deterministically without a timing-dependent filesystem race.
pub fn require_verified_ledger_head(
    verified_head: &str,
    reread_head: Option<&str>,
) -> Result<(), ProposalReadError> {
    if reread_head != Some(verified_head) {
        return Err(read_error("transaction.incomplete"));
    }
    Ok(())
}

fn read_complete_inbox(
    snapshot: &super::StoreSnapshot,
) -> Result<Vec<InboxSubmission>, ProposalReadError> {
    regular_directory(&snapshot.paths.inbox).map_err(|error| read_error(error.code()))?;
    let mut entries = fs::read_dir(&snapshot.paths.inbox)
        .map_err(|_| read_error("proposal.read"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| read_error("proposal.read"))?;
    entries.sort_by_key(|entry| entry.file_name());
    entries
        .into_iter()
        .filter(|entry| {
            !entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".submission-"))
        })
        .map(|entry| {
            let submission_id = entry
                .file_name()
                .into_string()
                .map_err(|_| read_error("proposal.id_invalid"))?;
            read_inbox_submission(
                &snapshot.paths.inbox,
                &submission_id,
                snapshot.policy.policy.max_candidate_bytes,
            )
            .map_err(|error| read_error(error.code()))
        })
        .collect()
}

pub fn proposal_record_order(
    record: &ProposalRecord,
    sequence: Option<u64>,
) -> (u8, String, String) {
    match record.state {
        ProposalState::Submitted => (
            0,
            record.submitted_at.clone(),
            record.submission_id.clone().unwrap_or_default(),
        ),
        ProposalState::Staged
        | ProposalState::Retained
        | ProposalState::RejectedAtIntake
        | ProposalState::Ruled
        | ProposalState::Applied
        | ProposalState::Superseded => (
            1,
            format!("{:020}", sequence.unwrap_or(u64::MAX)),
            if record.proposal_id.is_empty() {
                record.receipt_id.clone().unwrap_or_default()
            } else {
                record.proposal_id.clone()
            },
        ),
    }
}

fn read_store_error(error: super::StoreError) -> ProposalReadError {
    read_error(error.code())
}

fn read_error(code: &'static str) -> ProposalReadError {
    ProposalReadError::Verification { code }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IntakeRejection {
    pub schema_version: String,
    pub receipt_id: String,
    pub received_at: String,
    pub source_id: Option<String>,
    pub rule_codes: Vec<String>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ProposalError {
    #[error("proposal confidence is invalid")]
    ConfidenceInvalid,
    #[error("proposal pair is incomplete")]
    PairIncomplete,
    #[error("proposal metadata is too large")]
    MetaTooLarge,
    #[error("proposal metadata is invalid")]
    MetaParse,
    #[error("proposal metadata is not canonical")]
    MetaNoncanonical,
    #[error("proposal identifier is invalid")]
    IdInvalid,
    #[error("proposal identifier does not match")]
    IdMismatch,
    #[error("proposal candidate is not UTF-8")]
    CandidateUtf8,
    #[error("proposal candidate hash does not match")]
    CandidateHashMismatch,
    #[error("proposal candidate size does not match")]
    CandidateSizeMismatch,
    #[error("proposal identifier already exists")]
    DuplicateId,
    #[error("proposal path is unsafe")]
    PathUnsafe,
    #[error("proposal could not be read")]
    Read,
    #[error("proposal could not be written")]
    Write,
    #[error("proposal intake is locked")]
    Locked,
    #[error("store is locked by a protected verdict or recovery transaction")]
    ProtectedLocked,
    #[error("proposal was rejected by policy")]
    Rejected {
        rule_codes: Vec<String>,
        receipt_id: Option<String>,
    },
    #[error("intake rejection receipt could not be written")]
    ReceiptWrite { rule_codes: Vec<String> },
}

impl ProposalError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ConfidenceInvalid => "proposal.confidence_invalid",
            Self::PairIncomplete => "proposal.pair_incomplete",
            Self::MetaTooLarge => "proposal.meta_too_large",
            Self::MetaParse => "proposal.meta_parse",
            Self::MetaNoncanonical => "proposal.meta_noncanonical",
            Self::IdInvalid => "proposal.id_invalid",
            Self::IdMismatch => "proposal.id_mismatch",
            Self::CandidateUtf8 => "proposal.candidate_utf8",
            Self::CandidateHashMismatch => "proposal.content_hash_mismatch",
            Self::CandidateSizeMismatch => "proposal.content_size_mismatch",
            Self::DuplicateId => "proposal.duplicate_id",
            Self::PathUnsafe => "proposal.path_unsafe",
            Self::Read => "proposal.read",
            Self::Write => "proposal.write",
            Self::Locked => "proposal.locked",
            Self::ProtectedLocked => "store.locked",
            Self::Rejected { .. } => "proposal.rejected",
            Self::ReceiptWrite { .. } => "intake.receipt_write",
        }
    }

    pub fn rule_codes(&self) -> Option<&[String]> {
        match self {
            Self::Rejected { rule_codes, .. } | Self::ReceiptWrite { rule_codes } => {
                Some(rule_codes)
            }
            _ => None,
        }
    }
}

pub fn submit_proposal(request: ProposalSubmission) -> Result<SubmitProposalResult, ProposalError> {
    submit_proposal_with(&request, &SystemClock, || Ok(()), |_| Ok(()))
}

#[cfg(any(test, feature = "test-support"))]
pub fn submit_proposal_with_hook(
    request: ProposalSubmission,
    after_intake_lease: impl FnOnce() -> Result<(), ProposalError>,
) -> Result<SubmitProposalResult, ProposalError> {
    submit_proposal_with(&request, &SystemClock, after_intake_lease, |_| Ok(()))
}

#[cfg(any(test, feature = "test-support"))]
pub fn submit_proposal_with_final_publication_hook(
    request: ProposalSubmission,
    final_publication: impl Fn() -> Result<(), ProposalError>,
) -> Result<SubmitProposalResult, ProposalError> {
    submit_proposal_with(
        &request,
        &SystemClock,
        || Ok(()),
        |boundary| {
            if boundary == PublicationBoundary::BeforeDirectoryRename {
                final_publication()?;
            }
            Ok(())
        },
    )
}

pub fn submit_proposal_with_clock(
    request: ProposalSubmission,
    clock: &impl Clock,
) -> Result<SubmitProposalResult, ProposalError> {
    submit_proposal_with(&request, clock, || Ok(()), |_| Ok(()))
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationFaultPoint {
    AfterCandidateSync,
    AfterMetadataSync,
    BeforeDirectoryRename,
}

#[cfg(any(test, feature = "test-support"))]
pub fn submit_proposal_with_publication_fault(
    request: ProposalSubmission,
    fault: PublicationFaultPoint,
) -> Result<SubmitProposalResult, ProposalError> {
    submit_proposal_with(
        &request,
        &SystemClock,
        || Ok(()),
        |boundary| {
            let matches = matches!(
                (fault, boundary),
                (
                    PublicationFaultPoint::AfterCandidateSync,
                    PublicationBoundary::AfterCandidateSync
                ) | (
                    PublicationFaultPoint::AfterMetadataSync,
                    PublicationBoundary::AfterMetaSync
                ) | (
                    PublicationFaultPoint::BeforeDirectoryRename,
                    PublicationBoundary::BeforeDirectoryRename
                )
            );
            if matches {
                Err(ProposalError::Write)
            } else {
                Ok(())
            }
        },
    )
}

fn submit_proposal_with<C: Clock, H: Fn(PublicationBoundary) -> Result<(), ProposalError>>(
    request: &ProposalSubmission,
    clock: &C,
    after_validation: impl FnOnce() -> Result<(), ProposalError>,
    publication_hook: H,
) -> Result<SubmitProposalResult, ProposalError> {
    let profile = load_p3_profile(&request.profile_path).map_err(|_| ProposalError::Read)?;
    let store = inspect_store(&profile, &request.store_root).map_err(|error| match error {
        super::StoreError::Incomplete
            if fs::read_dir(request.store_root.join("locks"))
                .is_ok_and(|mut entries| entries.next().is_some()) =>
        {
            ProposalError::ProtectedLocked
        }
        _ => ProposalError::Read,
    })?;
    if store.sealed {
        super::verify_store_level4(&profile, &request.store_root)
            .map_err(|_| ProposalError::Read)?;
    }
    let now = clock.now().map_err(|_| ProposalError::Read)?;
    let candidate_bytes = read_regular_file(
        &request.candidate_path,
        store.policy.policy.max_candidate_bytes,
    )?;
    let evaluator = match &request.evaluator_path {
        Some(path) => Some(parse_evaluator(path, store.policy.policy.max_meta_bytes)?),
        None => None,
    };
    if std::str::from_utf8(&candidate_bytes).is_err()
        || !super::valid_source_id(&request.source_id)
        || !safe_submission_target_path(&request.target_path)
        || super::p3_canonical_document_id(&profile.config.context_namespace, &request.target_path)
            .as_deref()
            != Some(request.target_id.as_str())
    {
        return Err(ProposalError::PathUnsafe);
    }
    let candidate_hash = blake3::hash(&candidate_bytes).to_hex().to_string();
    if evaluator
        .as_ref()
        .is_some_and(|value| value.candidate_hash != candidate_hash)
    {
        return Err(ProposalError::CandidateHashMismatch);
    }
    let mut meta = SubmissionMeta {
        schema_version: SUBMISSION_SCHEMA.to_owned(),
        submission_id: String::new(),
        submitted_at: now.as_str().to_owned(),
        source_id: request.source_id.clone(),
        operation: request.operation,
        intent: request.intent,
        target_id: request.target_id.clone(),
        target_path: request.target_path.clone(),
        candidate_hash: candidate_hash.clone(),
        candidate_bytes: candidate_bytes.len(),
        policy_hash: store.policy.hash.clone(),
        profile_hash: profile.hash.clone(),
        confidence: request.confidence,
        evaluator,
    };
    meta.submission_id = submission_id(&meta);
    let meta_bytes = canonical_submission_meta_bytes(&meta)?;
    if meta_bytes.len() > super::P3_STRUCTURAL_META_BYTES {
        return Err(ProposalError::MetaTooLarge);
    }
    after_validation()?;
    require_no_protected_lease(&request.store_root)?;
    publish_inbox_submission(
        &store.paths.inbox,
        &meta.submission_id,
        &candidate_bytes,
        &meta_bytes,
        publication_hook,
    )?;
    Ok(SubmitProposalResult {
        submission_id: meta.submission_id.clone(),
        proposal_id: String::new(),
        state: "submitted".to_owned(),
        operation: meta.operation,
        intent: meta.intent,
        target_id: meta.target_id,
        candidate_hash,
        base_kind: String::new(),
        base_generation: None,
        bundle_hash: String::new(),
        manifest_hash: String::new(),
        policy_hash: meta.policy_hash,
        profile_hash: meta.profile_hash,
        submitted_at: meta.submitted_at,
    })
}

pub fn read_inbox_submission(
    inbox: &Path,
    submission_id: &str,
    max_candidate_bytes: usize,
) -> Result<InboxSubmission, ProposalError> {
    if !valid_submission_id(submission_id) {
        return Err(ProposalError::IdInvalid);
    }
    regular_directory(inbox)?;
    let root = inbox.join(submission_id);
    regular_directory(&root)?;
    let mut entries = fs::read_dir(&root)
        .map_err(|_| ProposalError::Read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProposalError::Read)?;
    entries.sort_by_key(|entry| entry.file_name());
    if entries.len() != 2
        || entries[0].file_name().to_str() != Some(CANDIDATE_FILE)
        || entries[1].file_name().to_str() != Some(SUBMISSION_META_FILE)
    {
        return Err(ProposalError::PairIncomplete);
    }
    let candidate_bytes = read_regular_file(&entries[0].path(), max_candidate_bytes)?;
    let meta_bytes = read_regular_file(&entries[1].path(), super::P3_STRUCTURAL_META_BYTES)?;
    let meta: SubmissionMeta =
        serde_json::from_slice(&meta_bytes).map_err(|_| ProposalError::MetaParse)?;
    if canonical_submission_meta_bytes(&meta)? != meta_bytes
        || meta.schema_version != SUBMISSION_SCHEMA
        || meta.submission_id != submission_id
        || meta.submission_id != submission_id_for_meta(&meta)
        || Timestamp::parse(&meta.submitted_at).is_err()
        || !super::valid_source_id(&meta.source_id)
        || !safe_submission_target_path(&meta.target_path)
        || meta.candidate_hash != blake3::hash(&candidate_bytes).to_hex().to_string()
        || meta.candidate_bytes != candidate_bytes.len()
    {
        return Err(ProposalError::MetaNoncanonical);
    }
    Ok(InboxSubmission {
        meta,
        meta_bytes,
        candidate_bytes,
    })
}

fn publish_inbox_submission<H: Fn(PublicationBoundary) -> Result<(), ProposalError>>(
    inbox: &Path,
    submission_id: &str,
    candidate_bytes: &[u8],
    meta_bytes: &[u8],
    publication_hook: H,
) -> Result<(), ProposalError> {
    regular_directory(inbox)?;
    let inbox_descriptor = File::open(inbox).map_err(|_| ProposalError::Write)?;
    let temporary = create_submission_temporary(inbox, submission_id)?;
    write_new_synced(&temporary.join(CANDIDATE_FILE), candidate_bytes)
        .map_err(|_| ProposalError::Write)?;
    publication_hook(PublicationBoundary::AfterCandidateSync)?;
    write_new_synced(&temporary.join(SUBMISSION_META_FILE), meta_bytes)
        .map_err(|_| ProposalError::Write)?;
    publication_hook(PublicationBoundary::AfterMetaSync)?;
    sync_directory(&temporary).map_err(|_| ProposalError::Write)?;
    publication_hook(PublicationBoundary::BeforeDirectoryRename)?;
    let temporary_name = temporary.file_name().ok_or(ProposalError::Write)?;
    match rustix::fs::renameat_with(
        &inbox_descriptor,
        temporary_name,
        &inbox_descriptor,
        submission_id,
        rustix::fs::RenameFlags::NOREPLACE,
    ) {
        Ok(()) => inbox_descriptor
            .sync_all()
            .map_err(|_| ProposalError::Write),
        Err(rustix::io::Errno::EXIST) => Err(ProposalError::DuplicateId),
        Err(_) => Err(ProposalError::Write),
    }
}

fn create_submission_temporary(
    inbox: &Path,
    submission_id: &str,
) -> Result<PathBuf, ProposalError> {
    let temporary = inbox.join(format!(".submission-{submission_id}"));
    match fs::create_dir(&temporary) {
        Ok(()) => Ok(temporary),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Err(ProposalError::DuplicateId),
        Err(_) => Err(ProposalError::Write),
    }
}

fn canonical_submission_meta_bytes(meta: &SubmissionMeta) -> Result<Vec<u8>, ProposalError> {
    serde_json::to_vec(meta).map_err(|_| ProposalError::MetaParse)
}

fn submission_id(meta: &SubmissionMeta) -> String {
    submission_id_for_meta(meta)
}

fn submission_id_for_meta(meta: &SubmissionMeta) -> String {
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, SUBMISSION_ID_DOMAIN);
    frame(&mut hasher, meta.submitted_at.as_bytes());
    frame(&mut hasher, meta.source_id.as_bytes());
    frame(&mut hasher, meta.operation.as_str().as_bytes());
    frame(&mut hasher, meta.intent.as_str().as_bytes());
    frame(&mut hasher, meta.target_id.as_bytes());
    frame(&mut hasher, meta.target_path.as_bytes());
    frame(&mut hasher, meta.candidate_hash.as_bytes());
    frame(
        &mut hasher,
        &u64::try_from(meta.candidate_bytes)
            .expect("usize always fits in u64")
            .to_le_bytes(),
    );
    frame(&mut hasher, meta.profile_hash.as_bytes());
    frame(&mut hasher, meta.policy_hash.as_bytes());
    format!("sub-{}", hasher.finalize().to_hex())
}

fn valid_submission_id(value: &str) -> bool {
    value.len() == 68
        && value.starts_with("sub-")
        && value[4..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn current_base(
    profile: &super::P3Profile,
    store: &super::StoreSnapshot,
    operation: ProposalOperation,
    target_path: &str,
) -> Result<ProposalBase, ProposalError> {
    match &store.head {
        super::CanonicalHead::Seed => proposal_base(
            "seed",
            None,
            &store.paths.seed.join("bundle"),
            store.seed.bundle_hash.clone(),
            store.seed.manifest_hash.clone(),
            operation,
            target_path,
        ),
        super::CanonicalHead::Generation(generation_id) => {
            let generation = super::verify_generation(
                profile,
                &store.paths.canon.join("generations").join(generation_id),
                super::VerificationLevel::Three,
            )
            .map_err(|_| ProposalError::Read)?;
            proposal_base(
                "generation",
                Some(generation_id.clone()),
                &generation.root.join("bundle"),
                generation.bundle_hash,
                generation.manifest_hash,
                operation,
                target_path,
            )
        }
    }
}

fn proposal_base(
    kind: &str,
    generation_id: Option<String>,
    bundle_root: &Path,
    bundle_hash: String,
    manifest_hash: String,
    operation: ProposalOperation,
    target_path: &str,
) -> Result<ProposalBase, ProposalError> {
    let target_hash =
        if operation == ProposalOperation::Replace && safe_submission_target_path(target_path) {
            let path = bundle_root.join(target_path);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => None,
                Ok(_) => Some(
                    blake3::hash(&fs::read(path).map_err(|_| ProposalError::Read)?)
                        .to_hex()
                        .to_string(),
                ),
                Err(_) => None,
            }
        } else {
            None
        };
    Ok(ProposalBase {
        kind: kind.to_owned(),
        generation_id,
        bundle_hash,
        manifest_hash,
        target_hash,
    })
}

fn safe_submission_target_path(path: &str) -> bool {
    valid_relative_path(path)
        && path.ends_with(".md")
        && !matches!(
            path,
            "index.md" | "manifest.json" | "policy.toml" | "CURRENT"
        )
        && ![
            "judgments/",
            "staging/",
            "ledger/",
            "evidence/",
            "derived/",
            "transactions/",
            "locks/",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn read_regular_file(path: &Path, maximum: usize) -> Result<Vec<u8>, ProposalError> {
    let path_metadata = fs::symlink_metadata(path).map_err(|_| ProposalError::Read)?;
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
        return Err(ProposalError::PathUnsafe);
    }
    if path_metadata.len() > maximum as u64 {
        return Err(ProposalError::Read);
    }
    let file = File::open(path).map_err(|_| ProposalError::Read)?;
    let opened_metadata = file.metadata().map_err(|_| ProposalError::Read)?;
    if !opened_metadata.is_file() || !same_file_identity(&path_metadata, &opened_metadata) {
        return Err(ProposalError::PathUnsafe);
    }
    let mut bytes = Vec::with_capacity(path_metadata.len() as usize);
    file.take((maximum as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| ProposalError::Read)?;
    if bytes.len() > maximum {
        return Err(ProposalError::Read);
    }
    Ok(bytes)
}

#[cfg(unix)]
fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file_identity(_left: &fs::Metadata, _right: &fs::Metadata) -> bool {
    false
}

pub fn parse_evaluator(path: &Path, maximum: usize) -> Result<EvaluatorBlock, ProposalError> {
    let bytes = read_regular_file(path, maximum)?;
    let evaluator = serde_json::from_slice(&bytes).map_err(|_| ProposalError::MetaParse)?;
    let canonical = serde_json::to_vec(&evaluator).map_err(|_| ProposalError::MetaParse)?;
    if bytes != canonical {
        return Err(ProposalError::MetaParse);
    }
    Ok(evaluator)
}

fn require_no_protected_lease(store_root: &Path) -> Result<(), ProposalError> {
    let locks = store_root.join("locks");
    let entries = fs::read_dir(&locks)
        .map_err(|_| ProposalError::Read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProposalError::Read)?;
    if entries.is_empty() {
        Ok(())
    } else {
        Err(ProposalError::ProtectedLocked)
    }
}

fn intake_owner_bytes(store: &super::StoreSnapshot, request: &ProposalSubmission) -> Vec<u8> {
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, b"sovmem.lease-receipt.v1");
    frame(&mut hasher, b"propose");
    frame(&mut hasher, store.policy.policy.store_id.as_bytes());
    frame(&mut hasher, store.head.as_str().as_bytes());
    frame(&mut hasher, store.profile_hash.as_bytes());
    frame(&mut hasher, store.policy.hash.as_bytes());
    frame(&mut hasher, request.source_id.as_bytes());
    frame(&mut hasher, request.operation.as_str().as_bytes());
    frame(&mut hasher, request.intent.as_str().as_bytes());
    frame(&mut hasher, request.target_id.as_bytes());
    let owner_id = format!("rec-{}", hasher.finalize().to_hex());
    format!(
        "{{\"schema_version\":\"sovmem-lease-owner-v1\",\"owner_id\":\"{owner_id}\",\"command_class\":\"propose\"}}\n"
    )
    .into_bytes()
}

pub fn acquire_intake_lease(
    staging: &Path,
    owner_bytes: &[u8],
) -> Result<IntakeLease, ProposalError> {
    let lock = staging.join(".intake-lock");
    match fs::create_dir(&lock) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            return Err(ProposalError::Locked)
        }
        Err(_) => return Err(ProposalError::Write),
    }
    let result = (|| {
        write_new_synced(&lock.join("owner.json"), owner_bytes)
            .map_err(|_| ProposalError::Write)?;
        sync_directory(&lock).map_err(|_| ProposalError::Write)?;
        sync_directory(staging).map_err(|_| ProposalError::Write)?;
        Ok(IntakeLease::from_verified_owner(
            staging.to_owned(),
            owner_bytes,
        ))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&lock);
        let _ = sync_directory(staging);
    }
    result
}

pub fn release_intake_lease(lease: &IntakeLease) -> Result<(), ProposalError> {
    release_intake_lease_with(lease, &StdIntakeLeaseReleaseOps)
}

pub trait IntakeLeaseReleaseOps {
    fn remove_file(&self, path: &Path) -> std::io::Result<()>;
    fn remove_dir(&self, path: &Path) -> std::io::Result<()>;
    fn sync_directory(&self, path: &Path) -> std::io::Result<()>;
}

pub struct StdIntakeLeaseReleaseOps;

impl IntakeLeaseReleaseOps for StdIntakeLeaseReleaseOps {
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_dir(path)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        sync_directory(path)
    }
}

pub fn release_intake_lease_with(
    lease: &IntakeLease,
    ops: &impl IntakeLeaseReleaseOps,
) -> Result<(), ProposalError> {
    let proposals = lease.staging.join("proposals");
    lease.verify(&proposals)?;
    let lock = lease.staging.join(".intake-lock");
    ops.sync_directory(&lease.staging)
        .map_err(|_| ProposalError::Write)?;
    ops.remove_file(&lock.join("owner.json"))
        .map_err(|_| ProposalError::Write)?;
    ops.remove_dir(&lock).map_err(|_| ProposalError::Write)?;
    Ok(())
}

fn retained_proposals(
    root: &Path,
    max_meta_bytes: usize,
) -> Result<Vec<ProposalSummary>, ProposalError> {
    Ok(retained_pairs(root, max_meta_bytes)?
        .into_iter()
        .map(|pair| ProposalSummary {
            proposal_id: pair.meta.proposal_id,
            state: "staged".to_owned(),
            submitted_at: pair.meta.submitted_at,
            source_id: pair.meta.source_id,
            target_id: pair.meta.target_id,
            target_path: pair.meta.target_path,
            candidate_hash: pair.candidate_hash,
        })
        .collect())
}

pub fn retained_pairs(
    root: &Path,
    max_meta_bytes: usize,
) -> Result<Vec<ProposalPair>, ProposalError> {
    regular_directory(root)?;
    let mut shards = fs::read_dir(root)
        .map_err(|_| ProposalError::Read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProposalError::Read)?;
    shards.sort_by_key(|entry| entry.file_name());
    let mut retained = Vec::new();
    for shard in shards {
        let name = shard
            .file_name()
            .into_string()
            .map_err(|_| ProposalError::PathUnsafe)?;
        if name.len() != 2
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(ProposalError::PathUnsafe);
        }
        regular_directory(&shard.path())?;
        let mut pairs = fs::read_dir(shard.path())
            .map_err(|_| ProposalError::Read)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ProposalError::Read)?;
        pairs.sort_by_key(|entry| entry.file_name());
        for pair in pairs {
            let proposal_id = pair
                .file_name()
                .into_string()
                .map_err(|_| ProposalError::PathUnsafe)?;
            let pair = read_proposal_pair(root, &proposal_id, max_meta_bytes)?;
            retained.push(pair);
        }
    }
    Ok(retained)
}

pub fn prepare_proposal(draft: ProposalDraft) -> Result<ProposalPair, ProposalError> {
    std::str::from_utf8(&draft.candidate_bytes).map_err(|_| ProposalError::CandidateUtf8)?;
    let candidate_hash = blake3::hash(&draft.candidate_bytes).to_hex().to_string();
    let mut meta = ProposalMeta {
        schema_version: PROPOSAL_SCHEMA.to_owned(),
        proposal_id: String::new(),
        submitted_at: draft.submitted_at.as_str().to_owned(),
        source_id: draft.source_id,
        operation: draft.operation,
        intent: draft.intent,
        target_id: draft.target_id,
        target_path: draft.target_path,
        base: draft.base,
        candidate_hash: candidate_hash.clone(),
        candidate_bytes: draft.candidate_bytes.len(),
        policy_hash: draft.policy_hash,
        profile_hash: draft.profile_hash,
        confidence: draft.confidence,
        evaluator: draft.evaluator,
    };
    meta.proposal_id = proposal_id(&meta);
    let meta_bytes = canonical_meta_bytes(&meta)?;
    let meta_hash = meta_hash(&meta_bytes);
    Ok(ProposalPair {
        meta,
        meta_bytes,
        candidate_bytes: draft.candidate_bytes,
        meta_hash,
        candidate_hash,
    })
}

pub fn proposal_id(meta: &ProposalMeta) -> String {
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, PROPOSAL_ID_DOMAIN);
    frame(&mut hasher, meta.submitted_at.as_bytes());
    frame(&mut hasher, meta.source_id.as_bytes());
    frame(&mut hasher, meta.operation.as_str().as_bytes());
    frame(&mut hasher, meta.intent.as_str().as_bytes());
    frame(&mut hasher, meta.target_id.as_bytes());
    frame(&mut hasher, meta.target_path.as_bytes());
    frame(&mut hasher, meta.base.kind.as_bytes());
    frame_option(&mut hasher, meta.base.generation_id.as_deref());
    frame(&mut hasher, meta.base.bundle_hash.as_bytes());
    frame(&mut hasher, meta.base.manifest_hash.as_bytes());
    frame_option(&mut hasher, meta.base.target_hash.as_deref());
    frame(&mut hasher, meta.candidate_hash.as_bytes());
    frame(
        &mut hasher,
        &u64::try_from(meta.candidate_bytes)
            .expect("usize always fits in u64")
            .to_le_bytes(),
    );
    frame(&mut hasher, meta.policy_hash.as_bytes());
    frame(&mut hasher, meta.profile_hash.as_bytes());
    match meta.confidence {
        Some(confidence) => {
            frame(&mut hasher, &[1]);
            frame(&mut hasher, &confidence.millionths().to_le_bytes());
        }
        None => frame(&mut hasher, &[0]),
    }
    let evaluator = meta
        .evaluator
        .as_ref()
        .map(evaluator_commitment)
        .transpose()
        .expect("evaluator serialization cannot fail");
    frame_option(&mut hasher, evaluator.as_deref());
    format!("prp-{}", hasher.finalize().to_hex())
}

fn frame(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn frame_option(hasher: &mut blake3::Hasher, value: Option<&str>) {
    match value {
        Some(value) => {
            frame(hasher, &[1]);
            frame(hasher, value.as_bytes());
        }
        None => frame(hasher, &[0]),
    }
}

pub fn evaluator_commitment(evaluator: &EvaluatorBlock) -> Result<String, ProposalError> {
    let bytes = serde_json::to_vec(evaluator).map_err(|_| ProposalError::MetaParse)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

pub fn canonical_meta_bytes(meta: &ProposalMeta) -> Result<Vec<u8>, ProposalError> {
    let mut bytes = serde_json::to_vec(meta).map_err(|_| ProposalError::MetaParse)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn meta_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[derive(Clone, Debug)]
pub struct IntakeLease {
    staging: PathBuf,
    owner_commitment: String,
}

impl IntakeLease {
    #[allow(dead_code)]
    pub fn from_verified_owner(staging: PathBuf, owner_bytes: &[u8]) -> Self {
        Self {
            staging,
            owner_commitment: blake3::hash(owner_bytes).to_hex().to_string(),
        }
    }

    fn owner_hash(&self) -> &str {
        &self.owner_commitment
    }

    fn verify(&self, proposals_root: &Path) -> Result<(), ProposalError> {
        if proposals_root.parent() != Some(self.staging.as_path()) {
            return Err(ProposalError::Locked);
        }
        let owner = self.staging.join(".intake-lock/owner.json");
        let metadata = fs::symlink_metadata(&owner).map_err(|_| ProposalError::Locked)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ProposalError::Locked);
        }
        let owner_bytes = fs::read(owner).map_err(|_| ProposalError::Locked)?;
        if blake3::hash(&owner_bytes).to_hex().as_str() != self.owner_commitment {
            return Err(ProposalError::Locked);
        }
        Ok(())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(staging: &Path) -> Result<Self, ProposalError> {
        let lock = staging.join(".intake-lock");
        fs::create_dir(&lock).map_err(|_| ProposalError::Write)?;
        let owner_bytes = b"{\"lease_token\":\"fixture\"}\n";
        write_new_synced(&lock.join("owner.json"), owner_bytes)
            .map_err(|_| ProposalError::Write)?;
        sync_directory(&lock).map_err(|_| ProposalError::Write)?;
        sync_directory(staging).map_err(|_| ProposalError::Write)?;
        Ok(Self::from_verified_owner(staging.to_owned(), owner_bytes))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PublicationBoundary {
    AfterCandidateSync,
    AfterMetaSync,
    BeforeDirectoryRename,
    AfterDirectoryRename,
}

pub fn publish_proposal_pair(
    proposals_root: &Path,
    lease: &IntakeLease,
    pair: &ProposalPair,
) -> Result<PathBuf, ProposalError> {
    publish_proposal_pair_with_hook(proposals_root, lease, pair, |_| Ok(()))
}

fn publish_proposal_pair_with_hook(
    proposals_root: &Path,
    lease: &IntakeLease,
    pair: &ProposalPair,
    hook: impl Fn(PublicationBoundary) -> Result<(), ProposalError>,
) -> Result<PathBuf, ProposalError> {
    verify_pair(pair, None)?;
    lease.verify(proposals_root)?;
    let temporary =
        create_temporary_directory(proposals_root, &pair.meta.proposal_id, lease.owner_hash())?;
    write_new_synced(&temporary.join(CANDIDATE_FILE), &pair.candidate_bytes)
        .map_err(|_| ProposalError::Write)?;
    hook(PublicationBoundary::AfterCandidateSync)?;
    write_new_synced(&temporary.join(META_FILE), &pair.meta_bytes)
        .map_err(|_| ProposalError::Write)?;
    hook(PublicationBoundary::AfterMetaSync)?;
    sync_directory(&temporary).map_err(|_| ProposalError::Write)?;
    hook(PublicationBoundary::BeforeDirectoryRename)?;

    let shard_name = proposal_shard(&pair.meta.proposal_id)?;
    let shard = proposals_root.join(shard_name);
    ensure_shard_directory(proposals_root, &shard)?;
    let destination = shard.join(&pair.meta.proposal_id);
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(ProposalError::DuplicateId);
    }
    lease.verify(proposals_root)?;
    fs::rename(&temporary, &destination).map_err(|_| ProposalError::Write)?;
    hook(PublicationBoundary::AfterDirectoryRename)?;
    sync_directory(&shard).map_err(|_| ProposalError::Write)?;
    sync_directory(proposals_root).map_err(|_| ProposalError::Write)?;
    Ok(destination)
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationFailpoint {
    AfterCandidateSync,
    AfterMetaSync,
    BeforeDirectoryRename,
    AfterDirectoryRename,
}

#[cfg(any(test, feature = "test-support"))]
pub fn publish_proposal_pair_with_failpoint(
    proposals_root: &Path,
    lease: &IntakeLease,
    pair: &ProposalPair,
    failpoint: PublicationFailpoint,
) -> Result<PathBuf, ProposalError> {
    publish_proposal_pair_with_hook(proposals_root, lease, pair, |boundary| {
        let matches = matches!(
            (failpoint, boundary),
            (
                PublicationFailpoint::AfterCandidateSync,
                PublicationBoundary::AfterCandidateSync
            ) | (
                PublicationFailpoint::AfterMetaSync,
                PublicationBoundary::AfterMetaSync
            ) | (
                PublicationFailpoint::BeforeDirectoryRename,
                PublicationBoundary::BeforeDirectoryRename
            ) | (
                PublicationFailpoint::AfterDirectoryRename,
                PublicationBoundary::AfterDirectoryRename
            )
        );
        if matches {
            Err(ProposalError::Write)
        } else {
            Ok(())
        }
    })
}

pub fn read_proposal_pair(
    proposals_root: &Path,
    proposal_id: &str,
    max_meta_bytes: usize,
) -> Result<ProposalPair, ProposalError> {
    let shard_name = proposal_shard(proposal_id)?;
    regular_directory(proposals_root)?;
    let shard = proposals_root.join(shard_name);
    regular_directory(&shard)?;
    let pair_root = shard.join(proposal_id);
    regular_directory(&pair_root)?;

    let mut entries = fs::read_dir(&pair_root)
        .map_err(|_| ProposalError::Read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProposalError::Read)?;
    entries.sort_by_key(|entry| entry.file_name());
    if entries.len() != 2 {
        return Err(ProposalError::PairIncomplete);
    }
    let mut names = Vec::with_capacity(2);
    for entry in &entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| ProposalError::PathUnsafe)?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|_| ProposalError::Read)?;
        if metadata.file_type().is_symlink() {
            return Err(ProposalError::PathUnsafe);
        }
        if !metadata.is_file() {
            return Err(ProposalError::PairIncomplete);
        }
        names.push(name);
    }
    if names != [CANDIDATE_FILE, META_FILE] {
        return Err(ProposalError::PairIncomplete);
    }

    let candidate_path = pair_root.join(CANDIDATE_FILE);
    let meta_path = pair_root.join(META_FILE);
    let meta_size = fs::metadata(&meta_path)
        .map_err(|_| ProposalError::Read)?
        .len();
    if meta_size > max_meta_bytes as u64 {
        return Err(ProposalError::MetaTooLarge);
    }
    let candidate_bytes = fs::read(&candidate_path).map_err(|_| ProposalError::Read)?;
    let meta_bytes = fs::read(&meta_path).map_err(|_| ProposalError::Read)?;
    let meta: ProposalMeta =
        serde_json::from_slice(&meta_bytes).map_err(|_| ProposalError::MetaParse)?;
    let pair = ProposalPair {
        candidate_hash: blake3::hash(&candidate_bytes).to_hex().to_string(),
        meta_hash: meta_hash(&meta_bytes),
        meta,
        meta_bytes,
        candidate_bytes,
    };
    verify_pair(&pair, Some(proposal_id))?;
    Ok(pair)
}

fn verify_pair(pair: &ProposalPair, expected_id: Option<&str>) -> Result<(), ProposalError> {
    if pair.meta.schema_version != PROPOSAL_SCHEMA {
        return Err(ProposalError::MetaParse);
    }
    let canonical = canonical_meta_bytes(&pair.meta)?;
    if pair.meta_bytes != canonical || pair.meta_hash != meta_hash(&pair.meta_bytes) {
        return Err(ProposalError::MetaNoncanonical);
    }
    if !valid_proposal_id(&pair.meta.proposal_id) {
        return Err(ProposalError::IdInvalid);
    }
    if expected_id.is_some_and(|id| id != pair.meta.proposal_id) {
        return Err(ProposalError::IdMismatch);
    }
    if proposal_id(&pair.meta) != pair.meta.proposal_id {
        return Err(ProposalError::IdMismatch);
    }
    std::str::from_utf8(&pair.candidate_bytes).map_err(|_| ProposalError::CandidateUtf8)?;
    if pair.meta.candidate_bytes != pair.candidate_bytes.len() {
        return Err(ProposalError::CandidateSizeMismatch);
    }
    let computed_candidate_hash = blake3::hash(&pair.candidate_bytes).to_hex().to_string();
    if pair.meta.candidate_hash != computed_candidate_hash
        || pair.candidate_hash != computed_candidate_hash
    {
        return Err(ProposalError::CandidateHashMismatch);
    }
    Ok(())
}

pub fn verify_proposal_pair_bytes(
    proposal_id: &str,
    candidate_bytes: Vec<u8>,
    meta_bytes: Vec<u8>,
) -> Result<(), ProposalError> {
    let meta: ProposalMeta =
        serde_json::from_slice(&meta_bytes).map_err(|_| ProposalError::MetaParse)?;
    let pair = ProposalPair {
        candidate_hash: blake3::hash(&candidate_bytes).to_hex().to_string(),
        meta_hash: meta_hash(&meta_bytes),
        meta,
        meta_bytes,
        candidate_bytes,
    };
    verify_pair(&pair, Some(proposal_id))
}

fn create_temporary_directory(
    proposals_root: &Path,
    proposal_id: &str,
    owner_hash: &str,
) -> Result<PathBuf, ProposalError> {
    regular_directory(proposals_root)?;
    for _ in 0..1_000 {
        let sequence = NEXT_PROPOSAL_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let path = proposals_root.join(format!(
            ".proposal-{proposal_id}-{owner_hash}-{}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(ProposalError::Write),
        }
    }
    Err(ProposalError::Write)
}

fn ensure_shard_directory(proposals_root: &Path, shard: &Path) -> Result<(), ProposalError> {
    match fs::create_dir(shard) {
        Ok(()) => sync_directory(proposals_root).map_err(|_| ProposalError::Write),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => regular_directory(shard),
        Err(_) => Err(ProposalError::Write),
    }
}

fn proposal_shard(proposal_id: &str) -> Result<&str, ProposalError> {
    if !valid_proposal_id(proposal_id) {
        return Err(ProposalError::IdInvalid);
    }
    Ok(&proposal_id[4..6])
}

fn valid_proposal_id(proposal_id: &str) -> bool {
    proposal_id.len() == 68
        && proposal_id.starts_with("prp-")
        && proposal_id[4..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn regular_directory(path: &Path) -> Result<(), ProposalError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ProposalError::Read)?;
    if metadata.file_type().is_symlink() {
        return Err(ProposalError::PathUnsafe);
    }
    if !metadata.is_dir() {
        return Err(ProposalError::PairIncomplete);
    }
    Ok(())
}
