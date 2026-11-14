use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use thiserror::Error;

use super::{
    canonical_source_path_identity, contains_secret, verified_published_corpus_evidence,
    BundleManifest, CorpusAccepted, CorpusTheme, ProfileConfig,
};

const COVERAGE: [&str; 5] = [
    "core",
    "tfis-ssa",
    "sovereign-context",
    "agent-operations",
    "governance-evidence",
];

const FORBIDDEN_CONTENT: [&str; 9] = [
    "02-memory",
    "05-daily",
    "08-hermes",
    "09-claude",
    "10-codex",
    "11-antigravity",
    "token:",
    "password:",
    "private key",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GoldenQuery {
    pub id: String,
    pub query: String,
    pub relevant_ids: Vec<String>,
    pub rationale: String,
    pub authorship: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct GoldenSummary {
    pub queries: usize,
    pub relevant_ids: usize,
    pub coverage: Vec<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GoldenError {
    #[error("golden query file could not be read")]
    Read,
    #[error("golden query line could not be parsed")]
    Parse,
    #[error("golden query count is below the approved minimum")]
    Count,
    #[error("golden query identifier is invalid")]
    Id,
    #[error("golden query text is invalid")]
    Query,
    #[error("golden query authorship is invalid")]
    Authorship,
    #[error("golden query relevance is invalid")]
    Relevance,
    #[error("golden query contains unsafe content")]
    Privacy,
    #[error("golden query coverage is incomplete")]
    Coverage,
    #[error("golden query evidence bundle is invalid")]
    Bundle,
}

impl GoldenError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Read => "golden.read",
            Self::Parse => "golden.parse",
            Self::Count => "golden.count",
            Self::Id => "golden.id",
            Self::Query => "golden.query",
            Self::Authorship => "golden.authorship",
            Self::Relevance => "golden.relevance",
            Self::Privacy => "golden.privacy",
            Self::Coverage => "golden.coverage",
            Self::Bundle => "golden.bundle",
        }
    }
}

pub fn validate_golden_file(
    profile: &ProfileConfig,
    bundle_root: &Path,
    golden_path: &Path,
) -> Result<GoldenSummary, GoldenError> {
    let bytes = fs::read(golden_path).map_err(|_| GoldenError::Read)?;
    let queries = parse_queries(&bytes)?;
    if queries.len() < 25 {
        return Err(GoldenError::Count);
    }
    validate_query_fields(&queries)?;

    let evidence = load_evidence(profile, bundle_root)?;
    let mut covered = BTreeSet::new();
    let mut relevant_ids = 0;
    for query in &queries {
        if query.relevant_ids.is_empty() || !strictly_sorted(&query.relevant_ids) {
            return Err(GoldenError::Relevance);
        }
        for id in &query.relevant_ids {
            let Some(category) = evidence.get(id) else {
                return Err(GoldenError::Relevance);
            };
            covered.insert(*category);
            relevant_ids += 1;
        }
    }
    if COVERAGE.iter().any(|category| !covered.contains(category)) {
        return Err(GoldenError::Coverage);
    }

    Ok(GoldenSummary {
        queries: queries.len(),
        relevant_ids,
        coverage: COVERAGE.iter().map(|value| (*value).to_owned()).collect(),
    })
}

pub(crate) fn parse_queries(bytes: &[u8]) -> Result<Vec<GoldenQuery>, GoldenError> {
    if bytes.is_empty() || !bytes.ends_with(b"\n") {
        return Err(GoldenError::Parse);
    }
    let source = std::str::from_utf8(bytes).map_err(|_| GoldenError::Parse)?;
    source
        .strip_suffix('\n')
        .ok_or(GoldenError::Parse)?
        .split('\n')
        .map(|line| {
            if line.is_empty() || line.ends_with('\r') {
                return Err(GoldenError::Parse);
            }
            serde_json::from_str(line).map_err(|_| GoldenError::Parse)
        })
        .collect()
}

pub(crate) fn validate_query_fields(queries: &[GoldenQuery]) -> Result<(), GoldenError> {
    let mut ids = BTreeSet::new();
    let mut normalized_queries = BTreeSet::new();
    for query in queries {
        if !valid_query_id(&query.id) || !ids.insert(query.id.as_str()) {
            return Err(GoldenError::Id);
        }
        if query.query.trim().is_empty()
            || query.query.chars().any(char::is_control)
            || !normalized_queries.insert(normalize_query(&query.query))
        {
            return Err(GoldenError::Query);
        }
        if query.authorship != "human" {
            return Err(GoldenError::Authorship);
        }
        if unsafe_content(&query.query) || unsafe_content(&query.rationale) {
            return Err(GoldenError::Privacy);
        }
    }
    Ok(())
}

fn valid_query_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 5
        && bytes.starts_with(b"q-")
        && bytes[2..].iter().all(u8::is_ascii_digit)
        && &bytes[2..] != b"000"
}

fn normalize_query(query: &str) -> String {
    let mut normalized = String::new();
    let mut pending_space = false;
    for character in query.trim().chars() {
        if character.is_ascii_whitespace() {
            pending_space = true;
        } else {
            if pending_space && !normalized.is_empty() {
                normalized.push(' ');
            }
            normalized.push(character);
            pending_space = false;
        }
    }
    normalized
}

fn unsafe_content(text: &str) -> bool {
    if contains_secret(text) || contains_absolute_filesystem_path(text) {
        return true;
    }
    let folded = text.to_ascii_lowercase();
    FORBIDDEN_CONTENT
        .iter()
        .any(|forbidden| folded.contains(forbidden))
}

fn contains_absolute_filesystem_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    (0..bytes.len()).any(|index| {
        let boundary = index == 0 || path_boundary(bytes[index - 1]);
        let unix = boundary
            && bytes[index] == b'/'
            && bytes
                .get(index + 1)
                .is_some_and(|next| !next.is_ascii_whitespace() && *next != b'/');
        let forward_unc = boundary
            && bytes[index] == b'/'
            && bytes.get(index + 1) == Some(&b'/')
            && bytes
                .get(index + 2)
                .is_some_and(|next| !next.is_ascii_whitespace() && *next != b'/');
        let drive = boundary
            && bytes[index].is_ascii_alphabetic()
            && bytes.get(index + 1) == Some(&b':')
            && bytes
                .get(index + 2)
                .is_some_and(|separator| matches!(separator, b'/' | b'\\'));
        let unc = boundary
            && bytes[index] == b'\\'
            && bytes.get(index + 1) == Some(&b'\\')
            && bytes
                .get(index + 2)
                .is_some_and(|next| !next.is_ascii_whitespace() && *next != b'\\');
        unix || forward_unc || drive || unc
    })
}

fn path_boundary(previous: u8) -> bool {
    previous.is_ascii_whitespace()
        || (!previous.is_ascii_alphanumeric()
            && !matches!(previous, b'_' | b'-' | b':' | b'/' | b'\\'))
}

fn load_evidence(
    profile: &ProfileConfig,
    bundle_root: &Path,
) -> Result<BTreeMap<String, &'static str>, GoldenError> {
    let (manifest, report) =
        verified_published_corpus_evidence(profile, bundle_root).ok_or(GoldenError::Bundle)?;

    if !manifest_matches_profile(profile, &manifest)
        || report.profile != profile.profile
        || report.source_commit != manifest.commit
        || report.bundle_hash != manifest.bundle_hash
        || !report.gates_passed
        || report.counts.errors != 0
    {
        return Err(GoldenError::Bundle);
    }

    let manifest_ids = manifest
        .documents
        .iter()
        .map(|document| {
            let canonical = canonical_id(profile, &document.source_path)?;
            if document.id != canonical {
                return Err(GoldenError::Bundle);
            }
            Ok(document.id.clone())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if manifest_ids.len() != manifest.documents.len() {
        return Err(GoldenError::Bundle);
    }

    let mut evidence = BTreeMap::new();
    for accepted in &report.accepted {
        let category = evidence_category(accepted)?;
        if accepted.id != canonical_id(profile, &accepted.source_path)?
            || evidence.insert(accepted.id.clone(), category).is_some()
        {
            return Err(GoldenError::Bundle);
        }
    }
    let accepted_ids = evidence.keys().cloned().collect::<BTreeSet<_>>();
    if accepted_ids != manifest_ids {
        return Err(GoldenError::Bundle);
    }
    Ok(evidence)
}

fn manifest_matches_profile(profile: &ProfileConfig, manifest: &BundleManifest) -> bool {
    manifest.okf_version == profile.okf_version
        && manifest.profile == profile.profile
        && manifest.nous_schema == profile.nous_schema
        && manifest.ssa_id == profile.ssa_id
        && manifest.ssa_profile == profile.ssa_profile
        && manifest.familiar_id == profile.familiar_id
        && manifest.context_namespace == profile.context_namespace
        && manifest.vault_id == profile.vault_id
        && manifest.mode == "committed"
        && !manifest.working_tree_allowed
}

fn canonical_id(profile: &ProfileConfig, source_path: &str) -> Result<String, GoldenError> {
    canonical_source_path_identity(source_path)
        .map(|identity| format!("{}/{identity}", profile.context_namespace))
        .map_err(|_| GoldenError::Bundle)
}

fn evidence_category(accepted: &CorpusAccepted) -> Result<&'static str, GoldenError> {
    match (accepted.lane.as_str(), accepted.theme.as_ref()) {
        ("core", None) => Ok("core"),
        ("research", Some(CorpusTheme::TfisSsa)) => Ok("tfis-ssa"),
        ("research", Some(CorpusTheme::SovereignContext)) => Ok("sovereign-context"),
        ("research", Some(CorpusTheme::AgentOperations)) => Ok("agent-operations"),
        ("research", Some(CorpusTheme::GovernanceEvidence)) => Ok("governance-evidence"),
        _ => Err(GoldenError::Bundle),
    }
}

fn strictly_sorted(values: &[String]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}
