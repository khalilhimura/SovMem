use super::{
    glob_matches, parse_source_document, render_document, valid_relative_path, validate_document,
    Authority, CoreType, GeneratedDocument, GeneratedMetadata, Layer, LinkRelation, P3Profile,
    Policy, ProposalIntent, Provenance, Sensitivity, SourceDocument, SourceKind, Status, Timestamp,
    TypedLink,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

const VERDICT_ID_DOMAIN: &[u8] = b"sovmem.verdict.v2";

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
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
            || text.chars().any(|value| {
                value == '\0' || (value.is_control() && value != '\n' && value != '\t')
            })
        {
            return Err(JudgmentError::ReasonInvalid);
        }
        Ok(Self(bytes.to_vec()))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        // Construction verifies UTF-8 and the field remains private.
        std::str::from_utf8(&self.0).expect("validated reason remains UTF-8")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewriteBytes {
    bytes: Vec<u8>,
    hash: String,
}

impl RewriteBytes {
    pub fn new(bytes: Vec<u8>, maximum: usize) -> Result<Self, JudgmentError> {
        let text = std::str::from_utf8(&bytes).map_err(|_| JudgmentError::RewriteInvalid)?;
        if bytes.len() > maximum
            || text.is_empty()
            || text.chars().any(|value| {
                value == '\0' || (value.is_control() && value != '\n' && value != '\t')
            })
        {
            return Err(JudgmentError::RewriteInvalid);
        }
        Ok(Self {
            hash: blake3::hash(&bytes).to_hex().to_string(),
            bytes,
        })
    }

    pub fn read(path: &Path, maximum: usize) -> Result<Self, JudgmentError> {
        let metadata = fs::symlink_metadata(path).map_err(|_| JudgmentError::RewriteRead)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > maximum as u64
        {
            return Err(JudgmentError::RewriteInvalid);
        }
        Self::new(
            fs::read(path).map_err(|_| JudgmentError::RewriteRead)?,
            maximum,
        )
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }
}

pub fn validate_rewrite(
    rewrite: &RewriteBytes,
    profile: &P3Profile,
    policy: &Policy,
    target_id: &str,
    target_path: &str,
) -> Result<GeneratedDocument, JudgmentError> {
    let expected_target_id =
        super::p3_canonical_document_id(&profile.config.context_namespace, target_path);
    if !safe_rewrite_target_path(target_path)
        || expected_target_id.as_deref() != Some(target_id)
        || !policy
            .allowed_targets
            .iter()
            .any(|pattern| glob_matches(pattern, target_path))
        || rewrite.bytes.len() > policy.max_rewrite_bytes
    {
        return Err(JudgmentError::RewriteInvalid);
    }

    let parsed = parse_source_document(SourceDocument {
        source_path: target_path.to_owned(),
        bytes: rewrite.bytes.clone(),
        source_hash: rewrite.hash.clone(),
        created: String::new(),
        updated: String::new(),
    })
    .map_err(|_| JudgmentError::RewriteInvalid)?;
    if !parsed.metadata.contains_key("provenance") {
        return Err(JudgmentError::RewriteInvalid);
    }
    let metadata = serde_yaml::to_string(&parsed.metadata)
        .ok()
        .and_then(|yaml| serde_yaml::from_str::<GeneratedMetadata>(&yaml).ok())
        .ok_or(JudgmentError::RewriteInvalid)?;
    let Some(mapping) = profile.config.mapping_for(target_path) else {
        return Err(JudgmentError::RewriteInvalid);
    };
    if metadata.id != target_id
        || metadata.source_path != target_path
        || metadata.core_type != mapping.core_type
        || metadata.context_kind != mapping.context_kind
        || metadata.layer != mapping.layer
        || metadata.authority != mapping.authority
        || metadata.core_type == CoreType::Judgment
        || metadata.context_kind == "Human Judgment"
        || !policy
            .provenance_values
            .iter()
            .any(|value| value == provenance_name(&metadata.provenance))
    {
        return Err(JudgmentError::RewriteInvalid);
    }
    let document = GeneratedDocument {
        metadata,
        body: parsed.body,
        source_hash: rewrite.hash.clone(),
    };
    if validate_document(&profile.config, &document).has_errors() {
        return Err(JudgmentError::RewriteInvalid);
    }
    Ok(document)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerdictIdentityInput<'a> {
    pub proposal_id: &'a str,
    pub verb: VerdictVerb,
    pub reason: &'a Reason,
    pub rewrite: Option<&'a RewriteBytes>,
    pub actor: &'a str,
    pub ruled_at: &'a Timestamp,
    pub prior_ledger_head: Option<&'a str>,
    pub intake_sequence: u64,
    pub intake_head_hash: Option<&'a str>,
    pub policy_hash: &'a str,
    pub profile_hash: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionMaterial {
    pub verb: VerdictVerb,
    pub reason: Reason,
    pub rewrite: Option<RewriteBytes>,
    pub verdict_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JudgmentDocument {
    pub verdict_id: String,
    pub path: String,
    pub id: String,
    pub bytes: Vec<u8>,
    pub hash: String,
    pub document: GeneratedDocument,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum JudgmentError {
    #[error("verdict verb is invalid")]
    VerbInvalid,
    #[error("verdict reason is invalid")]
    ReasonInvalid,
    #[error("rewrite bytes are invalid")]
    RewriteInvalid,
    #[error("rewrite bytes could not be read")]
    RewriteRead,
    #[error("verdict conflicts with proposal intent")]
    IntentConflict,
    #[error("rewrite bytes are required")]
    RewriteRequired,
    #[error("rewrite bytes are forbidden")]
    RewriteForbidden,
    #[error("Judgment document could not be rendered")]
    Render,
}

impl JudgmentError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::VerbInvalid => "verdict.verb_invalid",
            Self::ReasonInvalid => "human.reason_invalid",
            Self::RewriteInvalid => "human.rewrite_invalid",
            Self::RewriteRead => "proposal.read",
            Self::IntentConflict => "verdict.intent_conflict",
            Self::RewriteRequired => "human.rewrite_required",
            Self::RewriteForbidden => "human.rewrite_forbidden",
            Self::Render => "judgment.render",
        }
    }
}

pub fn verdict_id(input: &VerdictIdentityInput<'_>) -> String {
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, VERDICT_ID_DOMAIN);
    frame(&mut hasher, input.proposal_id.as_bytes());
    frame(&mut hasher, input.verb.as_str().as_bytes());
    frame(&mut hasher, input.reason.bytes());
    frame_option(&mut hasher, input.rewrite.map(RewriteBytes::hash));
    frame(&mut hasher, input.actor.as_bytes());
    frame(&mut hasher, input.ruled_at.as_str().as_bytes());
    frame_option(&mut hasher, input.prior_ledger_head);
    frame(&mut hasher, &input.intake_sequence.to_le_bytes());
    frame_option(&mut hasher, input.intake_head_hash);
    frame(&mut hasher, input.policy_hash.as_bytes());
    frame(&mut hasher, input.profile_hash.as_bytes());
    format!("ver-{}", hasher.finalize().to_hex())
}

pub fn validate_decision(
    intent: ProposalIntent,
    verb: VerdictVerb,
    reason: Reason,
    rewrite: Option<RewriteBytes>,
    identity: &VerdictIdentityInput<'_>,
) -> Result<DecisionMaterial, JudgmentError> {
    if verb == VerdictVerb::Accept && intent != ProposalIntent::Accept {
        return Err(JudgmentError::IntentConflict);
    }
    if verb == VerdictVerb::Rewrite && rewrite.is_none() {
        return Err(JudgmentError::RewriteRequired);
    }
    if verb != VerdictVerb::Rewrite && rewrite.is_some() {
        return Err(JudgmentError::RewriteForbidden);
    }
    if identity.verb != verb || identity.reason.bytes() != reason.bytes() {
        return Err(JudgmentError::ReasonInvalid);
    }
    if identity.rewrite.map(RewriteBytes::hash) != rewrite.as_ref().map(RewriteBytes::hash) {
        return Err(JudgmentError::RewriteInvalid);
    }
    Ok(DecisionMaterial {
        verb,
        reason,
        rewrite,
        verdict_id: verdict_id(identity),
    })
}

pub fn build_judgment(
    profile: &P3Profile,
    verdict_id: &str,
    verb: VerdictVerb,
    target_id: &str,
    reason: &Reason,
    ruled_at: &Timestamp,
) -> Result<Option<JudgmentDocument>, JudgmentError> {
    if !verb.applies() {
        return Ok(None);
    }
    let path = format!("judgments/{verdict_id}.md");
    let id = super::p3_canonical_document_id(&profile.config.context_namespace, &path)
        .ok_or(JudgmentError::Render)?;
    let resource = format!("vault://{id}");
    let body = format!(
        "# Human Judgment: {verb}\n\nSubject: `{target_id}`\nVerdict: `{verb}`\n\n{}",
        reason.as_str(),
        verb = verb.as_str(),
    );
    let source_hash = blake3::hash(body.as_bytes()).to_hex().to_string();
    let document = GeneratedDocument {
        metadata: GeneratedMetadata {
            okf_version: profile.config.okf_version.clone(),
            profile: profile.config.profile.clone(),
            nous_schema: profile.config.nous_schema.clone(),
            ssa_id: profile.config.ssa_id.clone(),
            ssa_profile: profile.config.ssa_profile.clone(),
            familiar_id: profile.config.familiar_id.clone(),
            context_namespace: profile.config.context_namespace.clone(),
            id: id.clone(),
            core_type: CoreType::Judgment,
            context_kind: "Human Judgment".to_owned(),
            title: format!("Human Judgment: {} {target_id}", verb.as_str()),
            description: format!("Human {} verdict for {target_id}.", verb.as_str()),
            layer: Layer::Concept,
            source: resource.clone(),
            source_path: path.clone(),
            resource,
            source_kind: SourceKind::GeneratedReport,
            provenance: Provenance::Synthesis,
            status: Status::Active,
            authority: Authority::Canonical,
            sensitivity: Sensitivity::Internal,
            created: ruled_at.utc_date().to_owned(),
            updated: ruled_at.utc_date().to_owned(),
            tags: vec![
                "human-verdict".to_owned(),
                verb.as_str().to_ascii_lowercase(),
            ],
            links: vec![TypedLink {
                rel: LinkRelation::References,
                target: format!("vault://{target_id}"),
            }],
            inferred_fields: Vec::new(),
            owners: None,
            valid_from: None,
            valid_to: None,
            supersedes: None,
            superseded_by: None,
            verdict: Some(verb.as_str().to_owned()),
            reason: Some(reason.as_str().to_owned()),
            subject: Some(target_id.to_owned()),
            extensions: BTreeMap::new(),
        },
        body,
        source_hash,
    };
    let bytes = render_document(&document).map_err(|_| JudgmentError::Render)?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    Ok(Some(JudgmentDocument {
        verdict_id: verdict_id.to_owned(),
        path,
        id,
        bytes,
        hash,
        document,
    }))
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

fn safe_rewrite_target_path(path: &str) -> bool {
    valid_relative_path(path)
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

fn provenance_name(provenance: &Provenance) -> &'static str {
    match provenance {
        Provenance::Tier1 => "tier1",
        Provenance::Secondary => "secondary",
        Provenance::Synthesis => "synthesis",
        Provenance::Unknown => "unknown",
    }
}
