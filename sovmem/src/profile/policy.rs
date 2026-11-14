use super::{
    glob_matches, parse_source_document, proposal_id, validate_document, GeneratedDocument,
    GeneratedMetadata, IntakePrefix, Layer, P3Profile, ProposalIntent, ProposalPair,
    SourceDocument, StoreSnapshot, Timestamp, P3_PROFILE_VERSION, P3_STORE_ID,
};
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

pub const POLICY_SCHEMA: &str = "sovmem-policy-v1";
const PROPOSAL_SCHEMA: &str = "sovmem-proposal-v1";

const POLICY_KEYS: [&str; 23] = [
    "schema_version",
    "store_id",
    "profile_version",
    "human_actor",
    "allowed_proposer_prefixes",
    "allowed_evaluator_prefixes",
    "evaluator_protocol",
    "allowed_targets",
    "protected_layers",
    "forbidden_proposal_types",
    "provenance_values",
    "max_candidate_bytes",
    "max_meta_bytes",
    "max_rewrite_bytes",
    "max_reason_bytes",
    "max_evaluator_reasons",
    "max_evaluator_reason_bytes",
    "max_open_per_source",
    "max_submissions_per_source_utc_day",
    "max_open_total",
    "queue_alarm_at",
    "bad_accept_cost",
    "human_verdict_cost",
];

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PolicyArchiveIdentity {
    device: u64,
    inode: u64,
}

#[derive(Debug)]
struct NewPolicyArchiveFailure {
    source: io::Error,
    identity: Option<PolicyArchiveIdentity>,
}

impl NewPolicyArchiveFailure {
    fn without_identity(source: io::Error) -> Self {
        Self {
            source,
            identity: None,
        }
    }
}

#[cfg(unix)]
fn policy_archive_identity(metadata: &fs::Metadata) -> io::Result<PolicyArchiveIdentity> {
    use std::os::unix::fs::MetadataExt;

    Ok(PolicyArchiveIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn policy_archive_identity(_metadata: &fs::Metadata) -> io::Result<PolicyArchiveIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "policy archive identity requires Unix metadata",
    ))
}

static NEXT_POLICY_ARCHIVE_QUARANTINE: AtomicU64 = AtomicU64::new(0);
static NEXT_POLICY_ARCHIVE_WITNESS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyArchive {
    path: std::path::PathBuf,
    created: bool,
    identity: Option<PolicyArchiveIdentity>,
}

trait PolicyArchiveFilesystem {
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata>;
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn write_new_synced(
        &self,
        path: &Path,
        bytes: &[u8],
    ) -> Result<PolicyArchiveIdentity, NewPolicyArchiveFailure>;
    fn sync_directory(&self, path: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
}

struct StdPolicyArchiveFilesystem;

impl PolicyArchiveFilesystem for StdPolicyArchiveFilesystem {
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        fs::symlink_metadata(path)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn write_new_synced(
        &self,
        path: &Path,
        bytes: &[u8],
    ) -> Result<PolicyArchiveIdentity, NewPolicyArchiveFailure> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(NewPolicyArchiveFailure::without_identity)?;
        let identity = file
            .metadata()
            .and_then(|metadata| policy_archive_identity(&metadata))
            .map_err(NewPolicyArchiveFailure::without_identity)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map(|_| identity)
            .map_err(|source| NewPolicyArchiveFailure {
                source,
                identity: Some(identity),
            })
    }

    fn sync_directory(&self, path: &Path) -> io::Result<()> {
        File::open(path)?.sync_all()
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::hard_link(from, to)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }
}

#[derive(Clone, Debug)]
pub struct PolicyContext<'a> {
    pub policy: &'a LoadedPolicy,
    pub profile: &'a P3Profile,
    pub store: &'a StoreSnapshot,
    pub proposal: &'a ProposalPair,
    pub intake: &'a IntakePrefix,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyDecision {
    pub passed: bool,
    pub rule_codes: Vec<String>,
}

pub struct PolicyEvaluator;

impl PolicyEvaluator {
    pub fn evaluate(context: &PolicyContext<'_>) -> PolicyDecision {
        let preliminary = evaluate_pre_advisory(context);
        if !preliminary.passed {
            return preliminary;
        }
        let decision = evaluate_post_advisory(context);
        let mut erased_proposal = context.proposal.clone();
        erased_proposal.meta.confidence = None;
        erased_proposal.meta.evaluator = None;
        let erased_context = PolicyContext {
            policy: context.policy,
            profile: context.profile,
            store: context.store,
            proposal: &erased_proposal,
            intake: context.intake,
        };
        let erased = evaluate_post_advisory(&erased_context);
        if decision != erased {
            return PolicyDecision {
                passed: false,
                rule_codes: vec!["policy.advisory_authority_violation".to_owned()],
            };
        }
        decision
    }
}

fn evaluate_pre_advisory(context: &PolicyContext<'_>) -> PolicyDecision {
    evaluate_stages(
        context,
        &[
            evaluate_store_profile_policy,
            evaluate_pair_structure,
            evaluate_source_confidence_evaluator,
        ],
    )
}

fn evaluate_post_advisory(context: &PolicyContext<'_>) -> PolicyDecision {
    evaluate_stages(
        context,
        &[
            evaluate_uniqueness_and_quotas,
            evaluate_path_and_base,
            evaluate_document,
        ],
    )
}

fn evaluate_stages(
    context: &PolicyContext<'_>,
    stages: &[fn(&PolicyContext<'_>) -> Vec<String>],
) -> PolicyDecision {
    for stage in stages {
        let mut rule_codes = stage(context);
        rule_codes.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        rule_codes.dedup();
        if !rule_codes.is_empty() {
            return PolicyDecision {
                passed: false,
                rule_codes,
            };
        }
    }
    PolicyDecision {
        passed: true,
        rule_codes: Vec::new(),
    }
}

fn evaluate_store_profile_policy(context: &PolicyContext<'_>) -> Vec<String> {
    let mut codes = Vec::new();
    if context.profile.hash != context.store.profile_hash {
        codes.push("profile.hash_mismatch".to_owned());
    }
    if context.policy.hash != context.store.policy.hash {
        codes.push("policy.changed_during_transaction".to_owned());
    }
    codes
}

fn evaluate_pair_structure(context: &PolicyContext<'_>) -> Vec<String> {
    let pair = context.proposal;
    if pair.meta.schema_version != PROPOSAL_SCHEMA {
        return vec!["proposal.meta_schema".to_owned()];
    }
    let mut codes = Vec::new();
    if pair.meta_bytes.len() > context.policy.policy.max_meta_bytes {
        codes.push("proposal.meta_too_large".to_owned());
    }
    let parsed = serde_json::from_slice::<super::ProposalMeta>(&pair.meta_bytes);
    let parsed_meta = match parsed {
        Ok(meta) => meta,
        Err(error) => {
            if error.to_string().contains("unknown field") {
                codes.push("proposal.meta_unknown_field".to_owned());
            } else {
                codes.push("proposal.meta_parse".to_owned());
            }
            return codes;
        }
    };
    if parsed_meta != pair.meta {
        codes.push("proposal.meta_parse".to_owned());
    }
    let mut canonical = match serde_json::to_vec(&pair.meta) {
        Ok(bytes) => bytes,
        Err(_) => {
            codes.push("proposal.meta_parse".to_owned());
            return codes;
        }
    };
    canonical.push(b'\n');
    if pair.meta_bytes != canonical
        || pair.meta_hash != blake3::hash(&pair.meta_bytes).to_hex().to_string()
    {
        codes.push("proposal.meta_parse".to_owned());
    }
    if !valid_proposal_id(&pair.meta.proposal_id) {
        codes.push("proposal.id_invalid".to_owned());
    } else if proposal_id(&pair.meta) != pair.meta.proposal_id {
        codes.push("proposal.id_mismatch".to_owned());
    }
    if std::str::from_utf8(&pair.candidate_bytes).is_err() {
        codes.push("proposal.candidate_utf8".to_owned());
    }
    if pair.meta.candidate_bytes != pair.candidate_bytes.len() {
        codes.push("proposal.content_size_mismatch".to_owned());
    }
    let candidate_hash = blake3::hash(&pair.candidate_bytes).to_hex().to_string();
    if pair.candidate_hash != candidate_hash || pair.meta.candidate_hash != candidate_hash {
        codes.push("proposal.content_hash_mismatch".to_owned());
    }
    if codes.is_empty() && pair.meta.profile_hash != context.profile.hash {
        codes.push("profile.changed_during_transaction".to_owned());
    }
    if codes.is_empty() && pair.meta.policy_hash != context.policy.hash {
        codes.push("policy.changed_during_transaction".to_owned());
    }
    codes
}

fn evaluate_source_confidence_evaluator(context: &PolicyContext<'_>) -> Vec<String> {
    let Some(prefix) = source_prefix(&context.proposal.meta.source_id) else {
        return vec!["proposal.source_invalid".to_owned()];
    };
    if !context
        .policy
        .policy
        .allowed_proposer_prefixes
        .iter()
        .any(|allowed| allowed == prefix)
    {
        return vec!["proposal.source_forbidden".to_owned()];
    }
    let Some(evaluator) = &context.proposal.meta.evaluator else {
        return Vec::new();
    };
    let mut codes = Vec::new();
    let evaluator_prefix = source_prefix(&evaluator.source_id);
    if evaluator.protocol != context.policy.policy.evaluator_protocol
        || evaluator_prefix.is_none()
        || Timestamp::parse(&evaluator.evaluated_at).is_err()
        || !matches!(
            evaluator.recommendation.as_str(),
            "looks-sound" | "needs-rewrite" | "reject"
        )
        || evaluator.reasons.is_empty()
        || evaluator.reasons.len() > context.policy.policy.max_evaluator_reasons
        || evaluator.reasons.iter().any(|reason| {
            reason.is_empty()
                || reason.len() > context.policy.policy.max_evaluator_reason_bytes
                || reason
                    .bytes()
                    .any(|byte| byte == 0 || byte.is_ascii_control())
        })
    {
        codes.push("proposal.evaluator_invalid".to_owned());
    }
    if let Some(prefix) = evaluator_prefix {
        if !context
            .policy
            .policy
            .allowed_evaluator_prefixes
            .iter()
            .any(|allowed| allowed == prefix)
        {
            codes.push("proposal.evaluator_forbidden".to_owned());
        }
    }
    if evaluator.source_id == context.proposal.meta.source_id {
        codes.push("proposal.evaluator_self_review".to_owned());
    }
    if evaluator.candidate_hash != context.proposal.meta.candidate_hash {
        codes.push("proposal.evaluator_hash_mismatch".to_owned());
    }
    codes
}

fn evaluate_uniqueness_and_quotas(context: &PolicyContext<'_>) -> Vec<String> {
    let proposal = &context.proposal.meta;
    let mut codes = Vec::new();
    if context.intake.staged.iter().any(|retained| {
        retained.proposal_id != proposal.proposal_id
            && retained.target_id == proposal.target_id
            && retained.candidate_hash == context.proposal.candidate_hash
    }) {
        codes.push("proposal.duplicate_content".to_owned());
    }

    let open: Vec<_> = context
        .intake
        .staged
        .iter()
        .filter(|retained| retained.proposal_id != proposal.proposal_id)
        .filter(|retained| retained.state == "staged")
        .collect();
    if open
        .iter()
        .filter(|retained| retained.source_id == proposal.source_id)
        .count()
        >= context.policy.policy.max_open_per_source
    {
        codes.push("proposal.quota_source_open".to_owned());
    }
    if open.len() >= context.policy.policy.max_open_total {
        codes.push("proposal.quota_total".to_owned());
    }
    let date = proposal.submitted_at.get(..10);
    if date.is_some_and(|date| {
        let staged = context
            .intake
            .staged
            .iter()
            .filter(|retained| {
                retained.proposal_id != proposal.proposal_id
                    && retained.source_id == proposal.source_id
                    && retained.submitted_at.get(..10) == Some(date)
            })
            .count();
        let rejected = context
            .intake
            .rejected
            .iter()
            .filter(|rejection| {
                rejection.source_id.as_deref() == Some(proposal.source_id.as_str())
                    && rejection.received_at.get(..10) == Some(date)
            })
            .count();
        staged + rejected >= context.policy.policy.max_submissions_per_source_utc_day
    }) {
        codes.push("proposal.quota_source_daily".to_owned());
    }
    codes
}

fn evaluate_path_and_base(context: &PolicyContext<'_>) -> Vec<String> {
    let proposal = &context.proposal.meta;
    let mut codes = Vec::new();
    let target_id_matches = super::p3_canonical_document_id(
        &context.profile.config.context_namespace,
        &proposal.target_path,
    )
    .is_some_and(|expected| proposal.target_id == expected);
    if !safe_target_path(&proposal.target_path) || !target_id_matches {
        codes.push("proposal.target_unsafe".to_owned());
    } else {
        let mapping = context.profile.config.mapping_for(&proposal.target_path);
        if mapping.is_none()
            || !context
                .policy
                .policy
                .allowed_targets
                .iter()
                .any(|pattern| glob_matches(pattern, &proposal.target_path))
        {
            codes.push("proposal.target_forbidden".to_owned());
        }
    }

    let target_root = match &context.store.head {
        crate::profile::CanonicalHead::Seed => context.store.paths.seed.join("bundle"),
        crate::profile::CanonicalHead::Generation(generation_id) => context
            .store
            .paths
            .canon
            .join("generations")
            .join(generation_id)
            .join("bundle"),
    };
    let target = target_root.join(&proposal.target_path);
    let target_hash = fs::read(&target)
        .ok()
        .map(|bytes| blake3::hash(&bytes).to_hex().to_string());
    match proposal.operation {
        crate::profile::ProposalOperation::Create => {
            if proposal.base.target_hash.is_some() {
                codes.push("proposal.operation_invalid".to_owned());
            }
            if target_hash.is_some() {
                codes.push("proposal.create_target_exists".to_owned());
            }
        }
        crate::profile::ProposalOperation::Replace => {
            if proposal.base.target_hash.is_none() {
                codes.push("proposal.operation_invalid".to_owned());
            }
            match (proposal.base.target_hash.as_deref(), target_hash.as_deref()) {
                (_, None) => codes.push("proposal.replace_target_missing".to_owned()),
                (Some(expected), Some(actual)) if expected != actual => {
                    codes.push("proposal.base_target_stale".to_owned())
                }
                _ => {}
            }
        }
    }

    match &context.store.head {
        crate::profile::CanonicalHead::Seed => {
            if proposal.base.kind != "seed" || proposal.base.generation_id.is_some() {
                codes.push("proposal.base_kind_invalid".to_owned());
            }
            if proposal.base.bundle_hash != context.store.seed.bundle_hash {
                codes.push("proposal.base_bundle_stale".to_owned());
            }
            if proposal.base.manifest_hash != context.store.seed.manifest_hash {
                codes.push("proposal.base_manifest_stale".to_owned());
            }
        }
        crate::profile::CanonicalHead::Generation(generation_id) => {
            if proposal.base.kind != "generation"
                || proposal.base.generation_id.as_deref() != Some(generation_id)
            {
                codes.push("proposal.base_generation_stale".to_owned());
            }
        }
    }
    codes
}

pub(crate) fn safe_target_path(path: &str) -> bool {
    super::valid_relative_path(path)
        && path.ends_with(".md")
        && !matches!(
            path,
            "index.md" | "manifest.json" | "policy.toml" | "CURRENT"
        )
        && !path.starts_with("judgments/")
        && !path.starts_with("staging/")
        && !path.starts_with("ledger/")
        && !path.starts_with("evidence/")
        && !path.starts_with("derived/")
        && !path.starts_with("transactions/")
        && !path.starts_with("locks/")
}

fn evaluate_document(context: &PolicyContext<'_>) -> Vec<String> {
    if context.proposal.candidate_bytes.len() > context.policy.policy.max_candidate_bytes {
        return vec!["proposal.candidate_too_large".to_owned()];
    }
    let proposal = &context.proposal.meta;
    let date = proposal
        .submitted_at
        .get(..10)
        .unwrap_or_default()
        .to_owned();
    let source = SourceDocument {
        source_path: proposal.target_path.clone(),
        bytes: context.proposal.candidate_bytes.clone(),
        source_hash: context.proposal.candidate_hash.clone(),
        created: date.clone(),
        updated: date,
    };
    let parsed = match parse_source_document(source) {
        Ok(parsed) => parsed,
        Err(_) => return vec!["proposal.profile_mismatch".to_owned()],
    };
    if !parsed.metadata.contains_key("provenance") {
        return vec!["proposal.provenance_required".to_owned()];
    }
    let metadata = match serde_yaml::to_string(&parsed.metadata)
        .ok()
        .and_then(|yaml| serde_yaml::from_str::<GeneratedMetadata>(&yaml).ok())
    {
        Some(metadata) => metadata,
        None => return vec!["proposal.profile_mismatch".to_owned()],
    };
    if metadata.id != proposal.target_id || metadata.source_path != proposal.target_path {
        return vec!["proposal.target_binding".to_owned()];
    }
    if matches!(metadata.core_type, crate::profile::CoreType::Judgment)
        || metadata.context_kind == "Human Judgment"
    {
        return vec!["proposal.judgment_forbidden".to_owned()];
    }
    let Some(mapping) = context.profile.config.mapping_for(&proposal.target_path) else {
        return vec!["proposal.profile_mismatch".to_owned()];
    };
    if metadata.core_type != mapping.core_type
        || metadata.context_kind != mapping.context_kind
        || metadata.layer != mapping.layer
        || metadata.authority != mapping.authority
    {
        return vec!["proposal.type_binding".to_owned()];
    }
    if mapping.layer == Layer::Synthesis && proposal.intent == ProposalIntent::Accept {
        return vec!["proposal.synthesis_rewrite_required".to_owned()];
    }
    let document = GeneratedDocument {
        metadata,
        body: parsed.body,
        source_hash: context.proposal.candidate_hash.clone(),
    };
    let mut codes = validate_document(&context.profile.config, &document)
        .errors()
        .into_iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect::<Vec<_>>();
    if codes.is_empty() {
        return codes;
    }
    codes.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    codes.dedup();
    codes
}

fn source_prefix(source_id: &str) -> Option<&str> {
    let rest = source_id.strip_prefix("src:")?;
    let (kind, name) = rest.split_once(':')?;
    if rest.matches(':').count() != 1
        || !matches!(kind, "human" | "agent" | "harness" | "fixture")
        || !valid_source_name(name)
    {
        return None;
    }
    let prefix_length = "src:".len() + kind.len() + 1;
    Some(&source_id[..prefix_length])
}

pub(crate) fn valid_source_id(source_id: &str) -> bool {
    source_prefix(source_id).is_some()
}

fn valid_source_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn valid_proposal_id(proposal_id: &str) -> bool {
    proposal_id.len() == 68
        && proposal_id.starts_with("prp-")
        && proposal_id[4..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
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

impl PolicyError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Read => "policy.read",
            Self::Utf8 => "policy.utf8",
            Self::LineEndings => "policy.line_endings",
            Self::Syntax => "policy.syntax",
            Self::KeyOrder => "policy.key_order",
            Self::DuplicateKey => "policy.duplicate_key",
            Self::UnknownField => "policy.unknown_field",
            Self::MissingField => "policy.missing_field",
            Self::SchemaUnsupported => "policy.schema_unsupported",
            Self::ValueInvalid => "policy.value_invalid",
            Self::InvariantInvalid => "policy.invariant_invalid",
            Self::ChangedDuringTransaction => "policy.changed_during_transaction",
        }
    }
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

/// Persist the exact reviewed policy bytes used by a verdict before the ledger
/// can reference their hash. Existing snapshots are evidence, not a cache:
/// they must be regular files whose bytes still match exactly.
pub fn archive_policy(
    archive_root: &Path,
    policy: &LoadedPolicy,
) -> Result<PolicyArchive, PolicyError> {
    archive_policy_with(&StdPolicyArchiveFilesystem, archive_root, policy)
}

fn archive_policy_with(
    filesystem: &impl PolicyArchiveFilesystem,
    archive_root: &Path,
    policy: &LoadedPolicy,
) -> Result<PolicyArchive, PolicyError> {
    if blake3::hash(&policy.bytes).to_hex().as_str() != policy.hash {
        return Err(PolicyError::ChangedDuringTransaction);
    }
    let archive = archive_root.join(format!("{}.toml", policy.hash));
    let mut created = false;
    let mut created_identity = None;
    match filesystem.symlink_metadata(&archive) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(PolicyError::ChangedDuringTransaction);
            }
            validate_existing_policy_archive_with(filesystem, &archive, &metadata, &policy.bytes)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match filesystem.write_new_synced(&archive, &policy.bytes) {
                Ok(identity) => created_identity = Some(identity),
                Err(error) if error.source.kind() == std::io::ErrorKind::AlreadyExists => {
                    return archive_policy_with(filesystem, archive_root, policy);
                }
                Err(error) => {
                    if let Some(identity) = error.identity {
                        discard_failed_new_policy_archive(filesystem, &archive, identity)?;
                    }
                    return Err(PolicyError::Read);
                }
            }
            if filesystem.sync_directory(archive_root).is_err() {
                discard_failed_new_policy_archive(
                    filesystem,
                    &archive,
                    created_identity.expect("new policy archive identity is present"),
                )?;
                return Err(PolicyError::Read);
            }
            created = true;
        }
        Err(_) => return Err(PolicyError::Read),
    }
    Ok(PolicyArchive {
        path: archive,
        created,
        identity: created_identity,
    })
}

fn validate_existing_policy_archive_with(
    filesystem: &impl PolicyArchiveFilesystem,
    archive: &Path,
    original_metadata: &fs::Metadata,
    expected_bytes: &[u8],
) -> Result<(), PolicyError> {
    let expected_identity =
        policy_archive_identity(original_metadata).map_err(|_| PolicyError::Read)?;
    let witness = create_existing_policy_archive_witness(filesystem, archive)?;
    let validation = (|| {
        let witness_metadata = filesystem
            .symlink_metadata(&witness)
            .map_err(|_| PolicyError::Read)?;
        if witness_metadata.file_type().is_symlink()
            || !witness_metadata.is_file()
            || policy_archive_identity(&witness_metadata).map_err(|_| PolicyError::Read)?
                != expected_identity
        {
            return Err(PolicyError::ChangedDuringTransaction);
        }
        if filesystem.read(&witness).map_err(|_| PolicyError::Read)? != expected_bytes {
            return Err(PolicyError::ChangedDuringTransaction);
        }

        let visible_metadata = filesystem
            .symlink_metadata(archive)
            .map_err(|_| PolicyError::Read)?;
        if visible_metadata.file_type().is_symlink()
            || !visible_metadata.is_file()
            || policy_archive_identity(&visible_metadata).map_err(|_| PolicyError::Read)?
                != expected_identity
        {
            return Err(PolicyError::ChangedDuringTransaction);
        }
        Ok(())
    })();

    match discard_owned_policy_archive_with(filesystem, &witness, expected_identity, None) {
        Ok(()) => validation,
        Err(cleanup) => Err(cleanup),
    }
}

fn create_existing_policy_archive_witness(
    filesystem: &impl PolicyArchiveFilesystem,
    archive: &Path,
) -> Result<PathBuf, PolicyError> {
    let parent = archive.parent().ok_or(PolicyError::Read)?;
    for _ in 0..128 {
        let sequence = NEXT_POLICY_ARCHIVE_WITNESS.fetch_add(1, Ordering::Relaxed);
        let witness = parent.join(format!(
            ".sovmem-policy-witness-{}-{sequence}",
            std::process::id()
        ));
        match filesystem.hard_link(archive, &witness) {
            Ok(()) => return Ok(witness),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(PolicyError::Read),
        }
    }
    Err(PolicyError::Read)
}

fn discard_failed_new_policy_archive(
    filesystem: &impl PolicyArchiveFilesystem,
    archive: &Path,
    identity: PolicyArchiveIdentity,
) -> Result<(), PolicyError> {
    discard_owned_policy_archive_with(filesystem, archive, identity, None)
}

pub(crate) fn discard_unreferenced_policy_archive(
    archive: &PolicyArchive,
    policy: &LoadedPolicy,
) -> Result<(), PolicyError> {
    discard_unreferenced_policy_archive_with(&StdPolicyArchiveFilesystem, archive, policy)
}

fn discard_unreferenced_policy_archive_with(
    filesystem: &impl PolicyArchiveFilesystem,
    archive: &PolicyArchive,
    policy: &LoadedPolicy,
) -> Result<(), PolicyError> {
    // This rollback path is intentionally limited to the new snapshot created
    // by this preflight. Quarantine the visible path before inspecting it, so
    // a replacement can never be checked and then removed at that path.
    // Historical evidence is immutable: a mismatch stays quarantined as
    // explicit evidence and the transaction fails closed.
    if !archive.created {
        return Ok(());
    }
    let identity = archive
        .identity
        .ok_or(PolicyError::ChangedDuringTransaction)?;
    discard_owned_policy_archive_with(filesystem, &archive.path, identity, Some(&policy.bytes))
}

fn discard_owned_policy_archive_with(
    filesystem: &impl PolicyArchiveFilesystem,
    archive: &Path,
    expected_identity: PolicyArchiveIdentity,
    expected_bytes: Option<&[u8]>,
) -> Result<(), PolicyError> {
    let parent = archive.parent().ok_or(PolicyError::Read)?;
    let quarantine = reserve_policy_archive_quarantine(parent, archive)?;
    filesystem
        .rename(archive, &quarantine)
        .map_err(|_| PolicyError::Read)?;

    let metadata = filesystem
        .symlink_metadata(&quarantine)
        .map_err(|_| PolicyError::Read)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || policy_archive_identity(&metadata).map_err(|_| PolicyError::Read)? != expected_identity
    {
        return Err(PolicyError::ChangedDuringTransaction);
    }
    if let Some(expected_bytes) = expected_bytes {
        if filesystem
            .read(&quarantine)
            .map_err(|_| PolicyError::Read)?
            != expected_bytes
        {
            return Err(PolicyError::ChangedDuringTransaction);
        }
    }

    filesystem
        .remove_file(&quarantine)
        .map_err(|_| PolicyError::Read)?;
    let quarantine_dir = quarantine.parent().ok_or(PolicyError::Read)?;
    fs::remove_dir(quarantine_dir).map_err(|_| PolicyError::Read)?;
    filesystem
        .sync_directory(parent)
        .map_err(|_| PolicyError::Read)
}

fn reserve_policy_archive_quarantine(
    parent: &Path,
    archive: &Path,
) -> Result<PathBuf, PolicyError> {
    let archive_name = archive.file_name().ok_or(PolicyError::Read)?;
    for _ in 0..128 {
        let sequence = NEXT_POLICY_ARCHIVE_QUARANTINE.fetch_add(1, Ordering::Relaxed);
        let directory = parent.join(format!(
            ".sovmem-policy-rollback-{}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&directory) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;

                    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                        .map_err(|_| PolicyError::Read)?;
                }
                return Ok(directory.join(archive_name));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(PolicyError::Read),
        }
    }
    Err(PolicyError::Read)
}

pub fn parse_policy(bytes: &[u8]) -> Result<Policy, PolicyError> {
    if bytes.contains(&b'\r') || bytes.last() != Some(&b'\n') {
        return Err(PolicyError::LineEndings);
    }
    let source = std::str::from_utf8(bytes).map_err(|_| PolicyError::Utf8)?;
    let body = source.strip_suffix('\n').ok_or(PolicyError::LineEndings)?;
    let mut fields = Vec::with_capacity(POLICY_KEYS.len());

    for line in body.split('\n') {
        let (key, raw_value) = line.split_once(" = ").ok_or(PolicyError::Syntax)?;
        if !valid_key(key) {
            return Err(PolicyError::Syntax);
        }
        let Some(index) = POLICY_KEYS.iter().position(|expected| *expected == key) else {
            return Err(PolicyError::UnknownField);
        };
        if fields.iter().any(|(existing, _)| *existing == key) {
            return Err(PolicyError::DuplicateKey);
        }
        if index != fields.len() {
            return Err(PolicyError::KeyOrder);
        }
        fields.push((key, parse_field(key, raw_value)?));
    }

    if fields.len() != POLICY_KEYS.len() {
        return Err(PolicyError::MissingField);
    }

    let mut values = fields.into_iter().map(|(_, value)| value);
    let policy = Policy {
        schema_version: take_string(&mut values),
        store_id: take_string(&mut values),
        profile_version: take_string(&mut values),
        human_actor: take_string(&mut values),
        allowed_proposer_prefixes: take_strings(&mut values),
        allowed_evaluator_prefixes: take_strings(&mut values),
        evaluator_protocol: take_string(&mut values),
        allowed_targets: take_strings(&mut values),
        protected_layers: take_strings(&mut values),
        forbidden_proposal_types: take_strings(&mut values),
        provenance_values: take_strings(&mut values),
        max_candidate_bytes: take_number(&mut values),
        max_meta_bytes: take_number(&mut values),
        max_rewrite_bytes: take_number(&mut values),
        max_reason_bytes: take_number(&mut values),
        max_evaluator_reasons: take_number(&mut values),
        max_evaluator_reason_bytes: take_number(&mut values),
        max_open_per_source: take_number(&mut values),
        max_submissions_per_source_utc_day: take_number(&mut values),
        max_open_total: take_number(&mut values),
        queue_alarm_at: take_number(&mut values),
        bad_accept_cost: take_number(&mut values),
        human_verdict_cost: take_number(&mut values),
    };
    validate_policy(&policy)?;
    Ok(policy)
}

#[derive(Debug)]
enum FieldValue {
    String(String),
    Strings(Vec<String>),
    Number(usize),
}

fn parse_field(key: &str, raw_value: &str) -> Result<FieldValue, PolicyError> {
    match key {
        "schema_version" | "store_id" | "profile_version" | "human_actor"
        | "evaluator_protocol" => parse_string(raw_value).map(FieldValue::String),
        "allowed_proposer_prefixes"
        | "allowed_evaluator_prefixes"
        | "allowed_targets"
        | "protected_layers"
        | "forbidden_proposal_types"
        | "provenance_values" => parse_strings(raw_value).map(FieldValue::Strings),
        "max_candidate_bytes"
        | "max_meta_bytes"
        | "max_rewrite_bytes"
        | "max_reason_bytes"
        | "max_evaluator_reasons"
        | "max_evaluator_reason_bytes"
        | "max_open_per_source"
        | "max_submissions_per_source_utc_day"
        | "max_open_total"
        | "queue_alarm_at"
        | "bad_accept_cost"
        | "human_verdict_cost" => parse_number(raw_value).map(FieldValue::Number),
        _ => Err(PolicyError::UnknownField),
    }
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
}

fn parse_string(raw_value: &str) -> Result<String, PolicyError> {
    let Some(value) = raw_value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return Err(PolicyError::Syntax);
    };
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\\'))
    {
        return Err(PolicyError::Syntax);
    }
    Ok(value.to_owned())
}

fn parse_strings(raw_value: &str) -> Result<Vec<String>, PolicyError> {
    let Some(inner) = raw_value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return Err(PolicyError::Syntax);
    };
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    inner.split(", ").map(parse_string).collect()
}

fn parse_number(raw_value: &str) -> Result<usize, PolicyError> {
    if raw_value.is_empty()
        || !raw_value.bytes().all(|byte| byte.is_ascii_digit())
        || (raw_value.len() > 1 && raw_value.starts_with('0'))
    {
        return Err(PolicyError::Syntax);
    }
    raw_value.parse().map_err(|_| PolicyError::ValueInvalid)
}

fn take_string(values: &mut impl Iterator<Item = FieldValue>) -> String {
    match values.next() {
        Some(FieldValue::String(value)) => value,
        _ => unreachable!("fixed policy parser keeps field types aligned"),
    }
}

fn take_strings(values: &mut impl Iterator<Item = FieldValue>) -> Vec<String> {
    match values.next() {
        Some(FieldValue::Strings(value)) => value,
        _ => unreachable!("fixed policy parser keeps field types aligned"),
    }
}

fn take_number(values: &mut impl Iterator<Item = FieldValue>) -> usize {
    match values.next() {
        Some(FieldValue::Number(value)) => value,
        _ => unreachable!("fixed policy parser keeps field types aligned"),
    }
}

fn validate_policy(policy: &Policy) -> Result<(), PolicyError> {
    if policy.schema_version != POLICY_SCHEMA {
        return Err(PolicyError::SchemaUnsupported);
    }
    if policy.store_id != P3_STORE_ID || policy.profile_version != P3_PROFILE_VERSION {
        return Err(PolicyError::ValueInvalid);
    }
    if !valid_human_actor(&policy.human_actor)
        || !valid_label(&policy.evaluator_protocol)
        || !all_unique(&policy.allowed_proposer_prefixes)
        || !all_unique(&policy.allowed_evaluator_prefixes)
        || !all_unique(&policy.allowed_targets)
        || !all_unique(&policy.protected_layers)
        || !all_unique(&policy.forbidden_proposal_types)
        || !policy
            .allowed_proposer_prefixes
            .iter()
            .all(|value| valid_source_prefix(value))
        || !policy
            .allowed_evaluator_prefixes
            .iter()
            .all(|value| valid_source_prefix(value))
        || !policy.allowed_targets.iter().all(|value| valid_glob(value))
        || !policy
            .protected_layers
            .iter()
            .all(|value| matches!(value.as_str(), "synthesis" | "analysis" | "concept"))
        || !policy
            .forbidden_proposal_types
            .iter()
            .all(|value| matches!(value.as_str(), "concept" | "resource" | "judgment"))
        || policy.provenance_values != ["tier1", "secondary", "synthesis", "unknown"]
    {
        return Err(PolicyError::ValueInvalid);
    }

    let nonzero = [
        policy.max_candidate_bytes,
        policy.max_meta_bytes,
        policy.max_rewrite_bytes,
        policy.max_reason_bytes,
        policy.max_evaluator_reasons,
        policy.max_evaluator_reason_bytes,
        policy.max_open_per_source,
        policy.max_submissions_per_source_utc_day,
        policy.max_open_total,
        policy.queue_alarm_at,
        policy.bad_accept_cost,
        policy.human_verdict_cost,
    ];
    if nonzero.contains(&0)
        || policy.allowed_proposer_prefixes.is_empty()
        || policy.allowed_evaluator_prefixes.is_empty()
        || policy.allowed_targets.is_empty()
        || policy.protected_layers.is_empty()
        || policy.forbidden_proposal_types.is_empty()
        || policy.queue_alarm_at > policy.max_open_total
        || policy.max_open_per_source > policy.max_open_total
    {
        return Err(PolicyError::InvariantInvalid);
    }
    Ok(())
}

fn valid_human_actor(value: &str) -> bool {
    value
        .strip_prefix("src:human:")
        .is_some_and(valid_source_identity)
}

fn valid_source_prefix(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("src:") else {
        return false;
    };
    let Some(kind) = rest.strip_suffix(':') else {
        return false;
    };
    !kind.is_empty() && valid_source_identity(kind)
}

fn valid_source_identity(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_label(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn all_unique(values: &[String]) -> bool {
    values
        .iter()
        .enumerate()
        .all(|(index, value)| !values[..index].contains(value))
}

fn valid_glob(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && value.split('/').all(valid_glob_segment)
}

fn valid_glob_segment(segment: &str) -> bool {
    if segment.is_empty() || matches!(segment, "." | "..") {
        return false;
    }
    if segment == "**" {
        return true;
    }
    !segment.contains("**")
        && segment.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'.' | b'-' | b'_' | b'*')
        })
}

#[cfg(test)]
mod evaluator_tests {
    use super::{
        archive_policy, archive_policy_with, discard_unreferenced_policy_archive,
        discard_unreferenced_policy_archive_with, load_policy, policy_archive_identity,
        NewPolicyArchiveFailure, PolicyArchiveFilesystem, PolicyArchiveIdentity, PolicyContext,
        PolicyEvaluator, StoreSnapshot,
    };
    use crate::profile::{
        load_p3_profile, prepare_proposal, CanonicalHead, Confidence, EvaluatorBlock, IntakeHead,
        IntakePrefix, IntakeRejection, ProposalBase, ProposalDraft, ProposalIntent,
        ProposalOperation, ProposalSummary, SeedEvidence, StorePaths, Timestamp,
    };
    use std::cell::{Cell, RefCell};
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_POLICY_ARCHIVE_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct FailingNewArchiveFilesystem {
        write_full_bytes: bool,
    }

    impl PolicyArchiveFilesystem for FailingNewArchiveFilesystem {
        fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
            fs::symlink_metadata(path)
        }

        fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
            fs::read(path)
        }

        fn write_new_synced(
            &self,
            path: &Path,
            bytes: &[u8],
        ) -> Result<PolicyArchiveIdentity, NewPolicyArchiveFailure> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(NewPolicyArchiveFailure::without_identity)?;
            let identity = file
                .metadata()
                .and_then(|metadata| policy_archive_identity(&metadata))
                .map_err(NewPolicyArchiveFailure::without_identity)?;
            let partial = if self.write_full_bytes {
                bytes
            } else {
                &bytes[..1]
            };
            file.write_all(partial)
                .map_err(|source| NewPolicyArchiveFailure {
                    source,
                    identity: Some(identity),
                })?;
            Err(NewPolicyArchiveFailure {
                source: io::Error::new(
                    io::ErrorKind::Other,
                    "injected archive write or sync failure",
                ),
                identity: Some(identity),
            })
        }

        fn sync_directory(&self, path: &Path) -> io::Result<()> {
            File::open(path)?.sync_all()
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            fs::remove_file(path)
        }

        fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
            fs::hard_link(from, to)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            fs::rename(from, to)
        }
    }

    #[cfg(unix)]
    #[derive(Clone, Copy)]
    enum ArchiveReplacement {
        Regular,
        Symlink,
    }

    #[cfg(unix)]
    struct ReplaceBeforeQuarantineFilesystem {
        replacement: ArchiveReplacement,
        injected: Cell<bool>,
        moved_to: RefCell<Option<PathBuf>>,
        original_path: RefCell<Option<PathBuf>>,
        removed_paths: RefCell<Vec<PathBuf>>,
    }

    #[cfg(unix)]
    impl ReplaceBeforeQuarantineFilesystem {
        fn new(replacement: ArchiveReplacement) -> Self {
            Self {
                replacement,
                injected: Cell::new(false),
                moved_to: RefCell::new(None),
                original_path: RefCell::new(None),
                removed_paths: RefCell::new(Vec::new()),
            }
        }
    }

    #[cfg(unix)]
    impl PolicyArchiveFilesystem for ReplaceBeforeQuarantineFilesystem {
        fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
            fs::symlink_metadata(path)
        }

        fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
            fs::read(path)
        }

        fn write_new_synced(
            &self,
            _path: &Path,
            _bytes: &[u8],
        ) -> Result<PolicyArchiveIdentity, NewPolicyArchiveFailure> {
            panic!("the replacement filesystem is only used for cleanup")
        }

        fn sync_directory(&self, path: &Path) -> io::Result<()> {
            File::open(path)?.sync_all()
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            self.removed_paths.borrow_mut().push(path.to_path_buf());
            fs::remove_file(path)
        }

        fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
            fs::hard_link(from, to)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            if !self.injected.replace(true) {
                let original = from.with_extension("owned-before-replacement");
                fs::rename(from, &original)?;
                match self.replacement {
                    ArchiveReplacement::Regular => fs::write(from, b"replacement evidence\n")?,
                    ArchiveReplacement::Symlink => {
                        let target = from.with_extension("replacement-target");
                        fs::write(&target, b"replacement evidence\n")?;
                        std::os::unix::fs::symlink(target, from)?;
                    }
                }
                self.original_path.replace(Some(original));
            }
            self.moved_to.replace(Some(to.to_path_buf()));
            fs::rename(from, to)
        }
    }

    #[cfg(unix)]
    struct SwapExistingArchiveAtReadFilesystem {
        archive_path: PathBuf,
        policy_bytes: Vec<u8>,
        swapped: Cell<bool>,
        original_path: RefCell<Option<PathBuf>>,
    }

    #[cfg(unix)]
    impl SwapExistingArchiveAtReadFilesystem {
        fn new(archive_path: PathBuf, policy_bytes: Vec<u8>) -> Self {
            Self {
                archive_path,
                policy_bytes,
                swapped: Cell::new(false),
                original_path: RefCell::new(None),
            }
        }
    }

    #[cfg(unix)]
    impl PolicyArchiveFilesystem for SwapExistingArchiveAtReadFilesystem {
        fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
            fs::symlink_metadata(path)
        }

        fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
            if !self.swapped.replace(true) {
                let original = self.archive_path.with_extension("before-symlink-swap");
                let target = self.archive_path.with_extension("byte-identical-target");
                fs::rename(&self.archive_path, &original)?;
                fs::write(&target, &self.policy_bytes)?;
                std::os::unix::fs::symlink(&target, &self.archive_path)?;
                self.original_path.replace(Some(original));
            }
            fs::read(path)
        }

        fn write_new_synced(
            &self,
            _path: &Path,
            _bytes: &[u8],
        ) -> Result<PolicyArchiveIdentity, NewPolicyArchiveFailure> {
            panic!("the swap filesystem is only used for existing archive validation")
        }

        fn sync_directory(&self, path: &Path) -> io::Result<()> {
            File::open(path)?.sync_all()
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            fs::remove_file(path)
        }

        fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
            fs::hard_link(from, to)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            fs::rename(from, to)
        }
    }

    fn repository_path(relative: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(relative)
    }

    #[test]
    fn policy_archive_is_create_new_byte_exact_and_immutable() {
        let sequence = NEXT_POLICY_ARCHIVE_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-policy-archive-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let archive_root = root.join("policies");
        std::fs::create_dir(&archive_root).unwrap();
        let policy = load_policy(&repository_path("profiles/audy-ssa0-p3-v2/policy.toml"))
            .expect("load reviewed policy");

        let first = archive_policy(&archive_root, &policy).expect("create exact archive");
        assert_eq!(
            first.path,
            archive_root.join(format!("{}.toml", policy.hash))
        );
        assert!(first.created);
        assert_eq!(std::fs::read(&first.path).unwrap(), policy.bytes);

        archive_policy(&archive_root, &policy).expect("matching archive is idempotent");
        std::fs::write(&first.path, b"tampered\n").unwrap();
        assert_eq!(
            archive_policy(&archive_root, &policy).unwrap_err().code(),
            "policy.changed_during_transaction"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn existing_archive_rejects_byte_identical_symlink_swapped_between_metadata_and_read() {
        let sequence = NEXT_POLICY_ARCHIVE_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-policy-archive-existing-symlink-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let archive_root = root.join("policies");
        fs::create_dir(&archive_root).unwrap();
        let policy = load_policy(&repository_path("profiles/audy-ssa0-p3-v2/policy.toml"))
            .expect("load reviewed policy");
        let archive = archive_policy(&archive_root, &policy).expect("create archive");
        let filesystem =
            SwapExistingArchiveAtReadFilesystem::new(archive.path.clone(), policy.bytes.clone());

        let error = archive_policy_with(&filesystem, &archive_root, &policy)
            .expect_err("byte-identical symlink swap must fail closed");
        assert_eq!(error.code(), "policy.changed_during_transaction");
        assert!(filesystem.swapped.get());
        assert!(fs::symlink_metadata(&archive.path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&archive.path).unwrap(), policy.bytes);
        let original = filesystem
            .original_path
            .borrow()
            .clone()
            .expect("the original archive must remain evidence");
        assert_eq!(fs::read(original).unwrap(), policy.bytes);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_write_or_sync_failure_removes_the_provisional_snapshot() {
        let sequence = NEXT_POLICY_ARCHIVE_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-policy-archive-failure-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let archive_root = root.join("policies");
        fs::create_dir(&archive_root).unwrap();
        let policy = load_policy(&repository_path("profiles/audy-ssa0-p3-v2/policy.toml"))
            .expect("load reviewed policy");

        for write_full_bytes in [false, true] {
            let error = archive_policy_with(
                &FailingNewArchiveFilesystem { write_full_bytes },
                &archive_root,
                &policy,
            )
            .expect_err("injected archive failure must fail closed");
            assert_eq!(error.code(), "policy.read");
            assert!(
                !archive_root.join(format!("{}.toml", policy.hash)).exists(),
                "partial or unsynced archive must be removed"
            );
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_cleanup_removes_only_the_owned_exact_provisional_snapshot() {
        let sequence = NEXT_POLICY_ARCHIVE_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-policy-archive-cleanup-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let archive_root = root.join("policies");
        fs::create_dir(&archive_root).unwrap();
        let policy = load_policy(&repository_path("profiles/audy-ssa0-p3-v2/policy.toml"))
            .expect("load reviewed policy");
        let archive = archive_policy(&archive_root, &policy).expect("create archive");

        discard_unreferenced_policy_archive(&archive, &policy)
            .expect("owned exact archive must be cleaned up");
        assert!(!archive.path.exists());
        assert!(fs::read_dir(&archive_root).unwrap().next().is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn archive_cleanup_quarantines_replacement_or_symlink_without_removal() {
        for replacement in [ArchiveReplacement::Regular, ArchiveReplacement::Symlink] {
            let sequence = NEXT_POLICY_ARCHIVE_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "sovmem-policy-archive-replacement-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&root).unwrap();
            let archive_root = root.join("policies");
            fs::create_dir(&archive_root).unwrap();
            let policy = load_policy(&repository_path("profiles/audy-ssa0-p3-v2/policy.toml"))
                .expect("load reviewed policy");
            let archive = archive_policy(&archive_root, &policy).expect("create archive");
            let filesystem = ReplaceBeforeQuarantineFilesystem::new(replacement);

            let error = discard_unreferenced_policy_archive_with(&filesystem, &archive, &policy)
                .expect_err("replacement must fail closed");
            assert_eq!(error.code(), "policy.changed_during_transaction");
            assert!(filesystem.removed_paths.borrow().is_empty());

            let moved = filesystem
                .moved_to
                .borrow()
                .clone()
                .expect("replacement must be quarantined");
            match replacement {
                ArchiveReplacement::Regular => {
                    assert_eq!(fs::read(&moved).unwrap(), b"replacement evidence\n");
                }
                ArchiveReplacement::Symlink => {
                    assert!(fs::symlink_metadata(&moved)
                        .unwrap()
                        .file_type()
                        .is_symlink());
                }
            }
            let original = filesystem
                .original_path
                .borrow()
                .clone()
                .expect("owned archive must remain evidence too");
            assert_eq!(fs::read(original).unwrap(), policy.bytes);

            let _ = fs::remove_dir_all(root);
        }
    }

    fn context_parts(
        source_id: &str,
        evaluator_hash: Option<String>,
        base_bundle_hash: Option<String>,
    ) -> (
        crate::profile::P3Profile,
        StoreSnapshot,
        crate::profile::ProposalPair,
    ) {
        let profile = load_p3_profile(&repository_path("profiles/audy-ssa0-p3-v2/profile.yaml"))
            .expect("load P3 profile");
        let policy = load_policy(&repository_path("profiles/audy-ssa0-p3-v2/policy.toml"))
            .expect("load P3 policy");
        let store = StoreSnapshot {
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
            policy: policy.clone(),
            profile_hash: profile.hash.clone(),
            seed: SeedEvidence {
                documents: 62,
                manifest_hash: "b".repeat(64),
                corpus_report_hash: "c".repeat(64),
                bundle_hash: "a".repeat(64),
                source_commit: "fixture".to_owned(),
            },
            head: CanonicalHead::Seed,
            sealed: false,
        };
        let candidate_bytes =
            b"---\nid: ssa-0/04-Research/AVM/AVM-OKF-Substrate-Investigation.md\n---\n# Example\n"
                .to_vec();
        let proposal = prepare_proposal(ProposalDraft {
            submitted_at: Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap(),
            source_id: source_id.to_owned(),
            operation: ProposalOperation::Create,
            intent: ProposalIntent::Accept,
            target_id: "ssa-0/04-Research/AVM/AVM-OKF-Substrate-Investigation.md".to_owned(),
            target_path: "04-Research/AVM/AVM-OKF-Substrate-Investigation.md".to_owned(),
            base: ProposalBase {
                kind: "seed".to_owned(),
                generation_id: None,
                bundle_hash: base_bundle_hash.unwrap_or_else(|| store.seed.bundle_hash.clone()),
                manifest_hash: store.seed.manifest_hash.clone(),
                target_hash: None,
            },
            candidate_bytes: candidate_bytes.clone(),
            policy_hash: policy.hash,
            profile_hash: profile.hash.clone(),
            confidence: None,
            evaluator: evaluator_hash.map(|candidate_hash| EvaluatorBlock {
                protocol: "adversarial-bug-finding-v1".to_owned(),
                source_id: "src:agent:skeptic".to_owned(),
                evaluated_at: "2026-07-14T04:29:00.000000000Z".to_owned(),
                candidate_hash,
                recommendation: "looks-sound".to_owned(),
                reasons: vec!["No policy conflict found.".to_owned()],
            }),
        })
        .expect("prepare pair");
        (profile, store, proposal)
    }

    fn policy_intake(staged: Vec<ProposalSummary>, rejected: Vec<IntakeRejection>) -> IntakePrefix {
        IntakePrefix {
            head: IntakeHead {
                sequence: 0,
                event_hash: None,
            },
            staged,
            rejected,
        }
    }

    #[test]
    fn source_stage_rejects_malformed_source_before_later_stages() {
        let (profile, store, proposal) = context_parts("src:bogus:Example", None, None);
        let intake = policy_intake(Vec::new(), Vec::new());
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };

        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["proposal.source_invalid"]
        );
    }

    #[test]
    fn evaluator_stage_rejects_a_forged_candidate_commitment() {
        let (profile, store, proposal) =
            context_parts("src:agent:example", Some("0".repeat(64)), None);
        let intake = policy_intake(Vec::new(), Vec::new());
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };

        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["proposal.evaluator_hash_mismatch"]
        );
    }

    #[test]
    fn uniqueness_stage_rejects_the_same_target_and_candidate_hash() {
        let (profile, store, proposal) = context_parts("src:agent:example", None, None);
        let retained = [ProposalSummary {
            proposal_id: "prp-other".to_owned(),
            state: "staged".to_owned(),
            submitted_at: "2026-07-14T04:00:00.000000000Z".to_owned(),
            source_id: "src:agent:other".to_owned(),
            target_id: proposal.meta.target_id.clone(),
            target_path: proposal.meta.target_path.clone(),
            candidate_hash: proposal.candidate_hash.clone(),
        }];
        let intake = policy_intake(retained.to_vec(), Vec::new());
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };

        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["proposal.duplicate_content"]
        );
    }

    #[test]
    fn path_and_base_stage_rejects_a_stale_bundle_before_document_rules() {
        let (profile, store, proposal) =
            context_parts("src:agent:example", None, Some("0".repeat(64)));
        let intake = policy_intake(Vec::new(), Vec::new());
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };

        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["proposal.base_bundle_stale"]
        );
    }

    #[test]
    fn document_stage_requires_explicit_provenance() {
        let (profile, store, proposal) = context_parts("src:agent:example", None, None);
        let intake = policy_intake(Vec::new(), Vec::new());
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };

        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["proposal.provenance_required"]
        );
    }

    #[test]
    fn advisory_fields_do_not_change_the_post_structure_policy_decision() {
        let (profile, store, mut proposal) = context_parts("src:agent:example", None, None);
        proposal.meta.confidence = Some(Confidence::parse("1").unwrap());
        proposal.meta.evaluator = Some(EvaluatorBlock {
            protocol: "adversarial-bug-finding-v1".to_owned(),
            source_id: "src:agent:skeptic".to_owned(),
            evaluated_at: "2026-07-14T04:29:00.000000000Z".to_owned(),
            candidate_hash: proposal.candidate_hash.clone(),
            recommendation: "reject".to_owned(),
            reasons: vec!["Advisory only.".to_owned()],
        });
        proposal.meta.proposal_id = super::proposal_id(&proposal.meta);
        proposal.meta_bytes = serde_json::to_vec(&proposal.meta).unwrap();
        proposal.meta_bytes.push(b'\n');
        proposal.meta_hash = blake3::hash(&proposal.meta_bytes).to_hex().to_string();
        let mut erased = proposal.clone();
        erased.meta.confidence = None;
        erased.meta.evaluator = None;
        erased.meta.proposal_id = super::proposal_id(&erased.meta);
        erased.meta_bytes = serde_json::to_vec(&erased.meta).unwrap();
        erased.meta_bytes.push(b'\n');
        erased.meta_hash = blake3::hash(&erased.meta_bytes).to_hex().to_string();
        let intake = policy_intake(Vec::new(), Vec::new());
        let full = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };
        let without_advisory = PolicyContext {
            proposal: &erased,
            ..full.clone()
        };

        assert_eq!(
            PolicyEvaluator::evaluate(&full),
            PolicyEvaluator::evaluate(&without_advisory)
        );
    }

    #[test]
    fn structural_content_hash_error_precedes_profile_and_policy_drift() {
        let (profile, store, mut proposal) = context_parts("src:agent:example", None, None);
        proposal.meta.profile_hash = "0".repeat(64);
        proposal.meta.policy_hash = "1".repeat(64);
        proposal.meta.proposal_id = super::proposal_id(&proposal.meta);
        proposal.meta_bytes = serde_json::to_vec(&proposal.meta).unwrap();
        proposal.meta_bytes.push(b'\n');
        proposal.meta_hash = blake3::hash(&proposal.meta_bytes).to_hex().to_string();
        proposal.candidate_hash = "2".repeat(64);
        let intake = policy_intake(Vec::new(), Vec::new());
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };

        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["proposal.content_hash_mismatch"]
        );
    }

    fn canonicalize_proposal_meta(proposal: &mut crate::profile::ProposalPair) {
        proposal.meta.proposal_id = super::proposal_id(&proposal.meta);
        proposal.meta_bytes = serde_json::to_vec(&proposal.meta).unwrap();
        proposal.meta_bytes.push(b'\n');
        proposal.meta_hash = blake3::hash(&proposal.meta_bytes).to_hex().to_string();
    }

    #[test]
    fn profile_drift_precedes_later_source_advisory_and_policy_failures() {
        let (profile, store, mut proposal) = context_parts(
            "src:bogus:Example",
            Some("0".repeat(64)),
            Some("1".repeat(64)),
        );
        proposal.meta.profile_hash = "2".repeat(64);
        canonicalize_proposal_meta(&mut proposal);
        let intake = policy_intake(Vec::new(), Vec::new());
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };

        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["profile.changed_during_transaction"]
        );
    }

    #[test]
    fn profile_drift_precedes_policy_drift_when_both_change() {
        let (profile, store, mut proposal) = context_parts("src:agent:example", None, None);
        proposal.meta.profile_hash = "0".repeat(64);
        proposal.meta.policy_hash = "1".repeat(64);
        canonicalize_proposal_meta(&mut proposal);
        let intake = policy_intake(Vec::new(), Vec::new());
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };

        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["profile.changed_during_transaction"]
        );
    }

    #[test]
    fn policy_drift_precedes_later_source_advisory_and_policy_failures() {
        let (profile, store, mut proposal) = context_parts(
            "src:bogus:Example",
            Some("0".repeat(64)),
            Some("1".repeat(64)),
        );
        proposal.meta.policy_hash = "2".repeat(64);
        canonicalize_proposal_meta(&mut proposal);
        let intake = policy_intake(Vec::new(), Vec::new());
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };

        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["policy.changed_during_transaction"]
        );
    }

    #[test]
    fn flood_control_stops_before_base_and_document_rules_and_sorts_codes() {
        let (profile, store, proposal) = context_parts("src:agent:example", None, None);
        let retained = (0..store.policy.policy.max_open_per_source)
            .map(|index| ProposalSummary {
                proposal_id: format!("prp-open-{index}"),
                state: "staged".to_owned(),
                submitted_at: "2026-07-14T04:00:00.000000000Z".to_owned(),
                source_id: proposal.meta.source_id.clone(),
                target_id: format!("ssa-0/other-{index}"),
                target_path: format!("04-Research/other-{index}.md"),
                candidate_hash: format!("{index:064x}"),
            })
            .collect::<Vec<_>>();
        let intake = policy_intake(retained, Vec::new());
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };
        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["proposal.quota_source_open"]
        );

        let receipts = (0..store.policy.policy.max_submissions_per_source_utc_day)
            .map(|index| IntakeRejection {
                schema_version: "sovmem-intake-rejection-v1".to_owned(),
                receipt_id: format!("rec-{index:064x}"),
                received_at: "2026-07-14T03:00:00.000000000Z".to_owned(),
                source_id: Some(proposal.meta.source_id.clone()),
                rule_codes: vec!["proposal.profile_mismatch".to_owned()],
            })
            .collect::<Vec<_>>();
        let intake = policy_intake(Vec::new(), receipts);
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };
        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["proposal.quota_source_daily"]
        );

        let retained = [ProposalSummary {
            proposal_id: proposal.meta.proposal_id.clone(),
            state: "staged".to_owned(),
            submitted_at: "2026-07-14T04:00:00.000000000Z".to_owned(),
            source_id: "src:agent:other".to_owned(),
            target_id: proposal.meta.target_id.clone(),
            target_path: proposal.meta.target_path.clone(),
            candidate_hash: proposal.candidate_hash.clone(),
        }];
        let intake = policy_intake(retained.to_vec(), Vec::new());
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };
        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["proposal.provenance_required"]
        );
    }
}
