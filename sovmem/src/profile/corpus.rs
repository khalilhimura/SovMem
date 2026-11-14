use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use thiserror::Error;

use super::{glob_matches, valid_relative_path, SourceMode};

pub const CORPUS_SCHEMA: &str = "audy-retrieval-corpus-v1";
pub const PINNED_SOURCE_COMMIT: &str = "43824cfd9caed7b260aa442128fd1130ddb6e2ff";
pub const MINIMUM_DOCUMENTS: usize = 53;
pub const MINIMUM_RESEARCH_DOCUMENTS: usize = 45;
const FIXED_CORPUS_BYTES: &[u8] =
    include_bytes!("../../../profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml");
pub const REQUIRED_CORE: [(&str, CorpusRole); 8] = [
    ("01-Soul/Audy Himura.md", CorpusRole::Identity),
    ("01-Soul/Identity.md", CorpusRole::Doctrine),
    ("01-Soul/SSA Archetypes.md", CorpusRole::Doctrine),
    ("01-Soul/TFIS Axioms.md", CorpusRole::Doctrine),
    ("01-Soul/TFIS Canon.md", CorpusRole::Doctrine),
    ("AGENTS.md", CorpusRole::OperatingContract),
    ("OPERATIONS.md", CorpusRole::OperatingContract),
    ("00-Index.md", CorpusRole::ContextIndex),
];

const REQUIRED_EXCLUSIONS: [&str; 7] = [
    "02-Memory/**",
    "05-Daily/**",
    "08-Hermes/**",
    "09-Claude/**",
    "10-Codex/**",
    "11-Antigravity/**",
    "loops/**",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusRole {
    Identity,
    Doctrine,
    OperatingContract,
    ContextIndex,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusTheme {
    TfisSsa,
    SovereignContext,
    AgentOperations,
    GovernanceEvidence,
}

impl CorpusTheme {
    pub const fn minimum_accepted(&self) -> usize {
        match self {
            Self::TfisSsa => 10,
            Self::SovereignContext => 15,
            Self::AgentOperations => 10,
            Self::GovernanceEvidence => 10,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusLane {
    Core,
    Research,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CoreEntry {
    pub path: String,
    pub role: CorpusRole,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResearchEntry {
    pub path: String,
    pub theme: CorpusTheme,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CorpusConfig {
    pub schema: String,
    pub mode: SourceMode,
    pub source_commit: String,
    pub minimum_documents: usize,
    pub minimum_research_documents: usize,
    pub core: Vec<CoreEntry>,
    pub research: Vec<ResearchEntry>,
    pub exclude: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusEntryRef<'a> {
    pub lane: CorpusLane,
    pub path: &'a str,
    pub role: Option<&'a CorpusRole>,
    pub theme: Option<&'a CorpusTheme>,
}

#[derive(Debug, Error)]
pub enum CorpusError {
    #[error("corpus configuration could not be parsed")]
    Parse,
    #[error("corpus schema or source mode is unsupported")]
    Schema,
    #[error("corpus source commit is invalid")]
    Commit,
    #[error("canonical core contract is invalid")]
    Core,
    #[error("research candidate count is invalid")]
    Research,
    #[error("research theme distribution is invalid")]
    Theme,
    #[error("corpus path contract is invalid")]
    Path,
    #[error("corpus selection conflicts with an exclusion")]
    Selection,
    #[error("corpus configuration does not match the fixed v1 contract")]
    Contract,
}

impl CorpusError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Parse => "corpus.parse",
            Self::Schema => "corpus.schema",
            Self::Commit => "corpus.commit",
            Self::Core => "corpus.core",
            Self::Research => "corpus.research",
            Self::Theme => "corpus.theme",
            Self::Path => "corpus.path",
            Self::Selection => "corpus.selection",
            Self::Contract => "corpus.contract",
        }
    }
}

pub fn load_corpus(path: &Path) -> Result<CorpusConfig, CorpusError> {
    let source = fs::read(path).map_err(|_| CorpusError::Parse)?;
    let yaml: serde_yaml::Value =
        serde_yaml::from_slice(&source).map_err(|_| CorpusError::Parse)?;

    if yaml.get("schema").and_then(serde_yaml::Value::as_str) != Some(CORPUS_SCHEMA)
        || yaml.get("mode").and_then(serde_yaml::Value::as_str) != Some("committed")
    {
        return Err(CorpusError::Schema);
    }
    if contains_unknown_role(&yaml) {
        return Err(CorpusError::Core);
    }
    if contains_unknown_theme(&yaml) {
        return Err(CorpusError::Theme);
    }

    let corpus: CorpusConfig = serde_yaml::from_value(yaml).map_err(|_| CorpusError::Parse)?;
    corpus.validate()?;
    if source != FIXED_CORPUS_BYTES {
        return Err(CorpusError::Contract);
    }
    Ok(corpus)
}

pub(crate) fn fixed_corpus_config() -> CorpusConfig {
    serde_yaml::from_slice(FIXED_CORPUS_BYTES).expect("embedded fixed corpus v1 must parse")
}

fn contains_unknown_role(yaml: &serde_yaml::Value) -> bool {
    let accepted = [
        "identity",
        "doctrine",
        "operating-contract",
        "context-index",
    ];
    yaml.get("core")
        .and_then(serde_yaml::Value::as_sequence)
        .is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry
                    .get("role")
                    .and_then(serde_yaml::Value::as_str)
                    .is_some_and(|role| !accepted.contains(&role))
            })
        })
}

fn contains_unknown_theme(yaml: &serde_yaml::Value) -> bool {
    let accepted = [
        "tfis-ssa",
        "sovereign-context",
        "agent-operations",
        "governance-evidence",
    ];
    yaml.get("research")
        .and_then(serde_yaml::Value::as_sequence)
        .is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry
                    .get("theme")
                    .and_then(serde_yaml::Value::as_str)
                    .is_some_and(|theme| !accepted.contains(&theme))
            })
        })
}

impl CorpusConfig {
    pub(crate) fn validate(&self) -> Result<(), CorpusError> {
        if self.schema != CORPUS_SCHEMA
            || self.mode != SourceMode::Committed
            || self.minimum_documents != MINIMUM_DOCUMENTS
            || self.minimum_research_documents != MINIMUM_RESEARCH_DOCUMENTS
        {
            return Err(CorpusError::Schema);
        }
        if self.source_commit != PINNED_SOURCE_COMMIT {
            return Err(CorpusError::Commit);
        }
        if self.core.len() != REQUIRED_CORE.len()
            || REQUIRED_CORE.iter().any(|(required_path, required_role)| {
                !self
                    .core
                    .iter()
                    .any(|entry| entry.path == *required_path && entry.role == *required_role)
            })
        {
            return Err(CorpusError::Core);
        }
        if self.research.len() != 55 {
            return Err(CorpusError::Research);
        }

        let mut themes = BTreeMap::new();
        for entry in &self.research {
            *themes.entry(&entry.theme).or_insert(0usize) += 1;
        }
        let required_themes = [
            CorpusTheme::TfisSsa,
            CorpusTheme::SovereignContext,
            CorpusTheme::AgentOperations,
            CorpusTheme::GovernanceEvidence,
        ];
        if required_themes
            .iter()
            .any(|theme| themes.get(theme).copied().unwrap_or_default() < theme.minimum_accepted())
        {
            return Err(CorpusError::Theme);
        }

        let selected = self.selected_paths();
        let selected_count = self.core.len() + self.research.len();
        let selected_paths_are_invalid = selected.len() != selected_count
            || selected
                .iter()
                .any(|path| !valid_relative_path(path) || path.contains('*'))
            || self
                .research
                .iter()
                .any(|entry| !entry.path.starts_with("04-Research/"));
        let exclusions_are_invalid = self
            .exclude
            .iter()
            .any(|pattern| !valid_relative_path(pattern))
            || REQUIRED_EXCLUSIONS
                .iter()
                .any(|required| !self.exclude.iter().any(|pattern| pattern == required));
        if selected_paths_are_invalid || exclusions_are_invalid {
            return Err(CorpusError::Path);
        }
        if selected.iter().any(|path| {
            self.exclude
                .iter()
                .any(|pattern| glob_matches(pattern, path))
        }) {
            return Err(CorpusError::Selection);
        }
        if self != &fixed_corpus_config() {
            return Err(CorpusError::Contract);
        }
        Ok(())
    }

    pub fn selected_paths(&self) -> Vec<String> {
        self.core
            .iter()
            .map(|entry| entry.path.clone())
            .chain(self.research.iter().map(|entry| entry.path.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn entry(&self, path: &str) -> Option<CorpusEntryRef<'_>> {
        self.core
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| CorpusEntryRef {
                lane: CorpusLane::Core,
                path: &entry.path,
                role: Some(&entry.role),
                theme: None,
            })
            .or_else(|| {
                self.research
                    .iter()
                    .find(|entry| entry.path == path)
                    .map(|entry| CorpusEntryRef {
                        lane: CorpusLane::Research,
                        path: &entry.path,
                        role: None,
                        theme: Some(&entry.theme),
                    })
            })
    }

    pub fn research_theme(&self, path: &str) -> Option<&CorpusTheme> {
        self.research
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| &entry.theme)
    }
}
