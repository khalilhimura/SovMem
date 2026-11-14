use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

use super::{
    canonical_source_path_identity, CorpusConfig, CorpusLane, CorpusTheme, Diagnostic,
    ExcludedSource, GeneratedDocument, ProfileConfig, Severity,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CorpusCounts {
    pub candidates: usize,
    pub core: usize,
    pub research: usize,
    pub rejected: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct CorpusAccepted {
    pub source_path: String,
    pub id: String,
    pub lane: String,
    pub theme: Option<CorpusTheme>,
    pub source_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct CorpusRejected {
    pub source_path: String,
    pub lane: String,
    pub theme: Option<CorpusTheme>,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CorpusReport {
    pub schema: String,
    pub profile: String,
    pub source_commit: String,
    pub counts: CorpusCounts,
    pub required_core: Vec<String>,
    pub research_by_theme: BTreeMap<CorpusTheme, usize>,
    pub accepted: Vec<CorpusAccepted>,
    pub rejected: Vec<CorpusRejected>,
    pub diagnostic_codes: Vec<String>,
    pub bundle_hash: String,
    pub gates_passed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusReportDraft {
    report: CorpusReport,
}

impl CorpusReportDraft {
    pub fn finalize(mut self, bundle_hash: String) -> CorpusReport {
        self.report.bundle_hash = bundle_hash;
        self.report
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("corpus acceptance gate failed: {code}")]
pub struct CorpusGateError {
    code: &'static str,
}

impl CorpusGateError {
    pub const fn code(&self) -> &'static str {
        self.code
    }

    const fn new(code: &'static str) -> Self {
        Self { code }
    }

    pub(crate) const fn profile_contract() -> Self {
        Self::new("corpus.profile_contract")
    }
}

pub fn evaluate_corpus(
    profile: &ProfileConfig,
    config: &CorpusConfig,
    documents: &[GeneratedDocument],
    exclusions: &[ExcludedSource],
    diagnostics: &[Diagnostic],
) -> Result<CorpusReportDraft, CorpusGateError> {
    config
        .validate()
        .map_err(|_| CorpusGateError::new("corpus.config_invalid"))?;
    if !profile.is_canonical_audy_corpus_profile() {
        return Err(CorpusGateError::profile_contract());
    }

    if config.core.iter().any(|core| {
        documents
            .iter()
            .filter(|document| document.metadata.source_path == core.path)
            .count()
            != 1
            || exclusions
                .iter()
                .any(|exclusion| exclusion.source_path == core.path)
    }) {
        return Err(CorpusGateError::new("corpus.core_missing"));
    }
    if documents
        .iter()
        .any(|document| !has_canonical_profile_identity(profile, document))
    {
        return Err(CorpusGateError::new("corpus.profile_identity"));
    }
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        return Err(CorpusGateError::new("corpus.validation_error"));
    }

    let selected_paths: BTreeSet<_> = config.selected_paths().into_iter().collect();
    let mut evidenced_paths = BTreeSet::new();
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    let mut evidence_mismatch = false;
    let mut unmanifested_document = false;

    for document in documents {
        let source_path = &document.metadata.source_path;
        let Some(entry) = config.entry(source_path) else {
            unmanifested_document = true;
            continue;
        };
        if !evidenced_paths.insert(source_path.clone()) {
            evidence_mismatch = true;
            continue;
        }
        accepted.push(CorpusAccepted {
            source_path: source_path.clone(),
            id: document.metadata.id.clone(),
            lane: lane_name(entry.lane).into(),
            theme: entry.theme.cloned(),
            source_hash: document.source_hash.clone(),
        });
    }

    for exclusion in exclusions {
        let Some(entry) = config.entry(&exclusion.source_path) else {
            evidence_mismatch = true;
            continue;
        };
        if !evidenced_paths.insert(exclusion.source_path.clone())
            || entry.lane == CorpusLane::Core
            || !matches!(exclusion.reason.as_str(), "dirty" | "untracked")
        {
            evidence_mismatch = true;
            continue;
        }
        rejected.push(CorpusRejected {
            source_path: exclusion.source_path.clone(),
            lane: lane_name(entry.lane).into(),
            theme: entry.theme.cloned(),
            reason: exclusion.reason.clone(),
        });
    }

    accepted.sort();
    rejected.sort();

    let accepted_paths: BTreeSet<_> = accepted
        .iter()
        .map(|entry| entry.source_path.as_str())
        .collect();
    assert!(config
        .core
        .iter()
        .all(|entry| accepted_paths.contains(entry.path.as_str())));

    let core = accepted.iter().filter(|entry| entry.lane == "core").count();
    let research = accepted
        .iter()
        .filter(|entry| entry.lane == "research")
        .count();
    let mut research_by_theme = BTreeMap::new();
    for entry in accepted.iter().filter(|entry| entry.lane == "research") {
        if let Some(theme) = &entry.theme {
            *research_by_theme.entry(theme.clone()).or_insert(0) += 1;
        } else {
            evidence_mismatch = true;
        }
    }

    if required_themes().iter().any(|theme| {
        research_by_theme.get(theme).copied().unwrap_or_default() < theme.minimum_accepted()
    }) {
        return Err(CorpusGateError::new("corpus.theme_minimum"));
    }
    assert!(research >= config.minimum_research_documents);
    assert!(core + research >= config.minimum_documents);
    if unmanifested_document {
        return Err(CorpusGateError::new("corpus.unmanifested_document"));
    }
    if evidence_mismatch || evidenced_paths != selected_paths {
        return Err(CorpusGateError::new("corpus.evidence_mismatch"));
    }

    let warning_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .count();
    let error_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .count();
    let diagnostic_codes = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let required_core = config
        .core
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    Ok(CorpusReportDraft {
        report: CorpusReport {
            schema: config.schema.clone(),
            profile: profile.profile.clone(),
            source_commit: config.source_commit.clone(),
            counts: CorpusCounts {
                candidates: selected_paths.len(),
                core,
                research,
                rejected: rejected.len(),
                warnings: warning_count,
                errors: error_count,
            },
            required_core,
            research_by_theme,
            accepted,
            rejected,
            diagnostic_codes,
            bundle_hash: String::new(),
            gates_passed: true,
        },
    })
}

fn has_canonical_profile_identity(profile: &ProfileConfig, document: &GeneratedDocument) -> bool {
    let metadata = &document.metadata;
    metadata.okf_version == profile.okf_version
        && metadata.profile == profile.profile
        && metadata.nous_schema == profile.nous_schema
        && metadata.ssa_id == profile.ssa_id
        && metadata.ssa_profile == profile.ssa_profile
        && metadata.familiar_id == profile.familiar_id
        && metadata.context_namespace == profile.context_namespace
        && has_source_derived_id(document)
}

fn has_source_derived_id(document: &GeneratedDocument) -> bool {
    canonical_source_path_identity(&document.metadata.source_path).is_ok_and(|identity| {
        let id = format!("{}/{identity}", document.metadata.context_namespace);
        let resource = format!("vault://{id}");
        document.metadata.id == id
            && document.metadata.resource == resource
            && document.metadata.source == resource
    })
}

const fn lane_name(lane: CorpusLane) -> &'static str {
    match lane {
        CorpusLane::Core => "core",
        CorpusLane::Research => "research",
    }
}

fn required_themes() -> [CorpusTheme; 4] {
    [
        CorpusTheme::TfisSsa,
        CorpusTheme::SovereignContext,
        CorpusTheme::AgentOperations,
        CorpusTheme::GovernanceEvidence,
    ]
}
