use crate::PROFILE_SCHEMA_VERSION;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs;
use std::path::{Component, Path};
use thiserror::Error;

pub const P3_PROFILE_VERSION: &str = "audy-ssa0-p3-v2";
pub const P3_STORE_ID: &str = "audy-sovmem-writeback-v2";
pub const P3_STRUCTURAL_META_BYTES: usize = 262_144;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CoreType {
    Concept,
    Resource,
    Judgment,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    Synthesis,
    Analysis,
    Concept,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Authority {
    Canonical,
    Reviewed,
    Working,
    Raw,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Sensitivity {
    Public,
    Internal,
    Private,
    Restricted,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MappingRule {
    pub pattern: String,
    pub core_type: CoreType,
    pub context_kind: String,
    pub layer: Layer,
    pub authority: Authority,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SensitivityRule {
    pub pattern: String,
    pub sensitivity: Sensitivity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProfileConfig {
    pub okf_version: String,
    pub profile: String,
    pub nous_schema: String,
    pub ssa_id: String,
    pub ssa_profile: String,
    pub familiar_id: String,
    pub context_namespace: String,
    pub vault_id: String,
    pub context_kinds: BTreeMap<String, CoreType>,
    pub mapping_rules: Vec<MappingRule>,
    pub sensitivity_rules: Vec<SensitivityRule>,
    pub default_sensitivity: Sensitivity,
    pub public_allowlist: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct P3ProfileConfig {
    okf_version: String,
    profile: String,
    nous_schema: String,
    ssa_id: String,
    ssa_profile: String,
    familiar_id: String,
    context_namespace: String,
    vault_id: String,
    context_kinds: BTreeMap<String, CoreType>,
    mapping_rules: Vec<P3MappingRule>,
    sensitivity_rules: Vec<P3SensitivityRule>,
    default_sensitivity: Sensitivity,
    public_allowlist: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct P3MappingRule {
    pattern: String,
    core_type: CoreType,
    context_kind: String,
    layer: Layer,
    authority: Authority,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct P3SensitivityRule {
    pattern: String,
    sensitivity: Sensitivity,
}

impl From<P3ProfileConfig> for ProfileConfig {
    fn from(config: P3ProfileConfig) -> Self {
        Self {
            okf_version: config.okf_version,
            profile: config.profile,
            nous_schema: config.nous_schema,
            ssa_id: config.ssa_id,
            ssa_profile: config.ssa_profile,
            familiar_id: config.familiar_id,
            context_namespace: config.context_namespace,
            vault_id: config.vault_id,
            context_kinds: config.context_kinds,
            mapping_rules: config
                .mapping_rules
                .into_iter()
                .map(|rule| MappingRule {
                    pattern: rule.pattern,
                    core_type: rule.core_type,
                    context_kind: rule.context_kind,
                    layer: rule.layer,
                    authority: rule.authority,
                })
                .collect(),
            sensitivity_rules: config
                .sensitivity_rules
                .into_iter()
                .map(|rule| SensitivityRule {
                    pattern: rule.pattern,
                    sensitivity: rule.sensitivity,
                })
                .collect(),
            default_sensitivity: config.default_sensitivity,
            public_allowlist: config.public_allowlist,
        }
    }
}

#[derive(Clone, Debug)]
pub struct P3Profile {
    pub config: ProfileConfig,
    pub version: String,
    pub hash: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceMode {
    Committed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CanaryConfig {
    pub mode: SourceMode,
    pub source_commit: String,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("profile configuration could not be parsed: {0}")]
    Parse(String),
    #[error("profile schema is unsupported: {0}")]
    Schema(String),
    #[error("profile context namespace must equal its SSA identifier")]
    NamespaceMismatch,
    #[error("profile identifier is invalid: {0}")]
    Identifier(String),
    #[error("profile context-kind mapping is invalid: {0}")]
    KindMapping(String),
    #[error("profile mapping precedence is ambiguous: {0}")]
    MappingPrecedence(String),
    #[error("canary source mode must be committed")]
    CanaryMode,
    #[error("canary source commit must be a full lowercase hexadecimal object ID")]
    CanaryCommit,
    #[error("canary source selection is invalid: {0}")]
    CanarySelection(String),
}

impl ProfileError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Parse(_) => "profile.parse",
            Self::Schema(_) => "profile.schema",
            Self::NamespaceMismatch => "profile.namespace_mismatch",
            Self::Identifier(_) => "profile.identifier",
            Self::KindMapping(_) => "profile.kind_mapping",
            Self::MappingPrecedence(_) => "profile.mapping_precedence",
            Self::CanaryMode => "canary.mode",
            Self::CanaryCommit => "canary.commit",
            Self::CanarySelection(_) => "canary.selection",
        }
    }
}

pub fn load_profile(path: &Path) -> Result<ProfileConfig, ProfileError> {
    let source =
        fs::read_to_string(path).map_err(|error| ProfileError::Parse(error.to_string()))?;
    let profile: ProfileConfig =
        serde_yaml::from_str(&source).map_err(|error| ProfileError::Parse(error.to_string()))?;
    profile.validate()?;
    Ok(profile)
}

pub fn load_p3_profile(path: &Path) -> Result<P3Profile, ProfileError> {
    let bytes = fs::read(path).map_err(|error| ProfileError::Parse(error.to_string()))?;
    let version = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .ok_or_else(|| ProfileError::Identifier("P3 profile version path is invalid".into()))?;
    if version != P3_PROFILE_VERSION {
        return Err(ProfileError::Identifier(
            "P3 profile path must carry the reviewed version".into(),
        ));
    }
    let source =
        std::str::from_utf8(&bytes).map_err(|error| ProfileError::Parse(error.to_string()))?;
    let config: ProfileConfig = serde_yaml::from_str::<P3ProfileConfig>(source)
        .map(ProfileConfig::from)
        .map_err(|error| ProfileError::Parse(error.to_string()))?;
    config.validate_p3()?;
    Ok(P3Profile {
        config,
        version: version.to_owned(),
        hash: blake3::hash(&bytes).to_hex().to_string(),
        bytes,
    })
}

pub fn load_canary(path: &Path) -> Result<CanaryConfig, ProfileError> {
    let source =
        fs::read_to_string(path).map_err(|error| ProfileError::Parse(error.to_string()))?;
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(&source).map_err(|error| ProfileError::Parse(error.to_string()))?;

    if yaml.get("mode").and_then(serde_yaml::Value::as_str) != Some("committed") {
        return Err(ProfileError::CanaryMode);
    }

    let canary: CanaryConfig =
        serde_yaml::from_value(yaml).map_err(|error| ProfileError::Parse(error.to_string()))?;
    canary.validate()?;
    Ok(canary)
}

impl ProfileConfig {
    pub fn validate(&self) -> Result<(), ProfileError> {
        self.validate_with_judgment_mapping(false)
    }

    pub(crate) fn validate_p3(&self) -> Result<(), ProfileError> {
        self.validate_with_judgment_mapping(true)?;
        if !self.has_exact_p3_judgment_extension() {
            return Err(ProfileError::KindMapping(
                "P3 requires the exact reviewed Human Judgment extension".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn has_exact_p3_judgment_extension(&self) -> bool {
        let judgment_kinds: Vec<_> = self
            .context_kinds
            .iter()
            .filter(|(_, value)| **value == CoreType::Judgment)
            .collect();
        let judgment_rules: Vec<_> = self
            .mapping_rules
            .iter()
            .filter(|rule| rule.core_type == CoreType::Judgment)
            .collect();
        let judgment_sensitivity_rules: Vec<_> = self
            .sensitivity_rules
            .iter()
            .filter(|rule| rule.pattern.starts_with("judgments/"))
            .collect();

        judgment_kinds.len() == 1
            && judgment_kinds[0].0 == "Human Judgment"
            && judgment_rules.len() == 1
            && judgment_rules[0].pattern == "judgments/*.md"
            && judgment_rules[0].context_kind == "Human Judgment"
            && judgment_rules[0].layer == Layer::Concept
            && judgment_rules[0].authority == Authority::Canonical
            && self
                .mapping_for("judgments/ver-example.md")
                .is_some_and(|rule| {
                    rule.core_type == CoreType::Judgment
                        && rule.context_kind == "Human Judgment"
                        && rule.layer == Layer::Concept
                        && rule.authority == Authority::Canonical
                })
            && judgment_sensitivity_rules.len() == 1
            && judgment_sensitivity_rules[0].pattern == "judgments/*.md"
            && judgment_sensitivity_rules[0].sensitivity == Sensitivity::Internal
            && self.sensitivity_for("judgments/ver-example.md") == Sensitivity::Internal
    }

    fn validate_with_judgment_mapping(
        &self,
        allow_judgment_mapping: bool,
    ) -> Result<(), ProfileError> {
        if self.okf_version != "0.1"
            || self.profile != "nous-sovmem-v0"
            || self.nous_schema != PROFILE_SCHEMA_VERSION
        {
            return Err(ProfileError::Schema(format!(
                "expected okf_version 0.1, profile nous-sovmem-v0, and nous_schema {PROFILE_SCHEMA_VERSION}"
            )));
        }

        let identifiers = [
            ("profile", self.profile.as_str()),
            ("ssa_id", self.ssa_id.as_str()),
            ("ssa_profile", self.ssa_profile.as_str()),
            ("familiar_id", self.familiar_id.as_str()),
            ("context_namespace", self.context_namespace.as_str()),
            ("vault_id", self.vault_id.as_str()),
        ];
        if let Some((name, _)) = identifiers
            .iter()
            .find(|(_, value)| value.trim().is_empty())
        {
            return Err(ProfileError::Identifier(format!(
                "{name} must not be empty"
            )));
        }
        if self.vault_id.contains(['/', '\\']) {
            return Err(ProfileError::Identifier(
                "vault_id must be a single path-safe segment".into(),
            ));
        }

        if self.ssa_id != self.context_namespace {
            return Err(ProfileError::NamespaceMismatch);
        }

        if self.context_kinds.is_empty() {
            return Err(ProfileError::KindMapping(
                "at least one context kind is required".into(),
            ));
        }
        for (kind, core_type) in &self.context_kinds {
            if kind.trim().is_empty()
                || (!allow_judgment_mapping && *core_type == CoreType::Judgment)
            {
                return Err(ProfileError::KindMapping(format!(
                    "context kind {kind:?} must map to concept or resource"
                )));
            }
        }
        for rule in &self.mapping_rules {
            let Some(expected_core_type) = self.context_kinds.get(&rule.context_kind) else {
                return Err(ProfileError::KindMapping(format!(
                    "mapping rule references unknown context kind {:?}",
                    rule.context_kind
                )));
            };
            if rule.pattern.trim().is_empty()
                || (!allow_judgment_mapping && rule.core_type == CoreType::Judgment)
                || &rule.core_type != expected_core_type
            {
                return Err(ProfileError::KindMapping(format!(
                    "mapping for {:?} does not match its declared core type",
                    rule.context_kind
                )));
            }
        }

        validate_pattern_precedence(self.mapping_rules.iter().map(|rule| &rule.pattern))?;
        validate_pattern_precedence(self.sensitivity_rules.iter().map(|rule| &rule.pattern))?;
        Ok(())
    }

    pub fn mapping_for(&self, path: &str) -> Option<&MappingRule> {
        self.mapping_rules
            .iter()
            .find(|rule| glob_matches(&rule.pattern, path))
    }

    pub fn sensitivity_for(&self, path: &str) -> Sensitivity {
        self.sensitivity_rules
            .iter()
            .find(|rule| glob_matches(&rule.pattern, path))
            .map(|rule| rule.sensitivity.clone())
            .unwrap_or_else(|| self.default_sensitivity.clone())
    }

    pub(crate) fn is_canonical_audy_corpus_profile(&self) -> bool {
        self.okf_version == "0.1"
            && self.profile == "nous-sovmem-v0"
            && self.nous_schema == "nous-ssa-context-v0"
            && self.ssa_id == "ssa-0"
            && self.ssa_profile == "audy-ssa0-v0"
            && self.familiar_id == "audy"
            && self.context_namespace == "ssa-0"
            && self.vault_id == "audy-vault"
    }
}

impl CanaryConfig {
    fn validate(&self) -> Result<(), ProfileError> {
        if self.mode != SourceMode::Committed {
            return Err(ProfileError::CanaryMode);
        }
        if self.source_commit.len() != 40
            || !self
                .source_commit
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(ProfileError::CanaryCommit);
        }
        if self.include.is_empty() {
            return Err(ProfileError::CanarySelection(
                "at least one include pattern is required".into(),
            ));
        }
        if self
            .include
            .iter()
            .chain(&self.exclude)
            .any(|pattern| !valid_selection_pattern(pattern))
        {
            return Err(ProfileError::CanarySelection(
                "patterns must be non-empty vault-relative paths without parent traversal".into(),
            ));
        }
        if self
            .include
            .iter()
            .filter(|pattern| !pattern.contains('*'))
            .any(|path| {
                self.exclude
                    .iter()
                    .any(|pattern| glob_matches(pattern, path))
            })
        {
            return Err(ProfileError::CanarySelection(
                "an exact include cannot also match an exclusion".into(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn valid_selection_pattern(pattern: &str) -> bool {
    !pattern.trim().is_empty() && valid_relative_path(pattern)
}

pub(crate) fn valid_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && !Path::new(path).is_absolute()
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."))
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub fn glob_matches(pattern: &str, path: &str) -> bool {
    if pattern.is_empty() || path.is_empty() {
        return false;
    }

    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    let path_segments: Vec<&str> = path.split('/').collect();
    let mut pending = vec![(0, 0)];
    let mut visited = HashSet::new();

    while let Some((pattern_index, path_index)) = pending.pop() {
        if !visited.insert((pattern_index, path_index)) {
            continue;
        }
        if pattern_index == pattern_segments.len() && path_index == path_segments.len() {
            return true;
        }
        if pattern_index == pattern_segments.len() {
            continue;
        }

        if pattern_segments[pattern_index] == "**" {
            pending.push((pattern_index + 1, path_index));
            if path_index < path_segments.len() {
                pending.push((pattern_index, path_index + 1));
            }
        } else if path_index < path_segments.len()
            && segment_matches(pattern_segments[pattern_index], path_segments[path_index])
        {
            pending.push((pattern_index + 1, path_index + 1));
        }
    }
    false
}

fn segment_matches(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let mut reachable = vec![false; value.len() + 1];
    reachable[0] = true;

    for token in pattern {
        if token == '*' {
            for index in 1..=value.len() {
                reachable[index] = reachable[index] || reachable[index - 1];
            }
        } else {
            for index in (1..=value.len()).rev() {
                reachable[index] = reachable[index - 1] && value[index - 1] == token;
            }
            reachable[0] = false;
        }
    }
    reachable[value.len()]
}

#[derive(PartialEq, Eq)]
struct PatternSpecificity {
    literal_segments: usize,
    literal_characters: usize,
    segments: usize,
    single_stars: usize,
    double_stars: usize,
}

fn pattern_specificity(pattern: &str) -> PatternSpecificity {
    let segments: Vec<&str> = pattern.split('/').collect();
    PatternSpecificity {
        literal_segments: segments
            .iter()
            .filter(|segment| !segment.contains('*'))
            .count(),
        literal_characters: pattern
            .chars()
            .filter(|character| *character != '*')
            .count(),
        segments: segments.len(),
        single_stars: segments
            .iter()
            .filter(|segment| **segment != "**")
            .map(|segment| {
                segment
                    .chars()
                    .filter(|character| *character == '*')
                    .count()
            })
            .sum(),
        double_stars: segments.iter().filter(|segment| **segment == "**").count(),
    }
}

fn validate_pattern_precedence<'a>(
    patterns: impl Iterator<Item = &'a String>,
) -> Result<(), ProfileError> {
    let patterns: Vec<&str> = patterns.map(String::as_str).collect();
    for first_index in 0..patterns.len() {
        for second_index in (first_index + 1)..patterns.len() {
            let first = patterns[first_index];
            let second = patterns[second_index];
            if pattern_specificity(first) == pattern_specificity(second)
                && patterns_overlap(first, second)
            {
                return Err(ProfileError::MappingPrecedence(format!(
                    "{first:?} and {second:?} overlap at equal specificity"
                )));
            }
        }
    }
    Ok(())
}

fn patterns_overlap(first: &str, second: &str) -> bool {
    let first: Vec<&str> = first.split('/').collect();
    let second: Vec<&str> = second.split('/').collect();
    let mut pending = vec![(0, 0)];
    let mut visited = HashSet::new();

    while let Some((first_index, second_index)) = pending.pop() {
        if !visited.insert((first_index, second_index)) {
            continue;
        }
        if first_index == first.len() && second_index == second.len() {
            return true;
        }

        match (first.get(first_index), second.get(second_index)) {
            (Some(&"**"), Some(&"**")) => {
                pending.push((first_index + 1, second_index));
                pending.push((first_index, second_index + 1));
            }
            (Some(&"**"), Some(second_segment)) => {
                pending.push((first_index + 1, second_index));
                if segment_has_nonempty_match(second_segment) {
                    pending.push((first_index, second_index + 1));
                }
            }
            (Some(first_segment), Some(&"**")) => {
                pending.push((first_index, second_index + 1));
                if segment_has_nonempty_match(first_segment) {
                    pending.push((first_index + 1, second_index));
                }
            }
            (Some(first_segment), Some(second_segment)) => {
                if segment_patterns_overlap(first_segment, second_segment) {
                    pending.push((first_index + 1, second_index + 1));
                }
            }
            (Some(&"**"), None) => pending.push((first_index + 1, second_index)),
            (None, Some(&"**")) => pending.push((first_index, second_index + 1)),
            _ => {}
        }
    }
    false
}

fn segment_has_nonempty_match(pattern: &str) -> bool {
    !pattern.is_empty()
}

fn segment_patterns_overlap(first: &str, second: &str) -> bool {
    let first: Vec<char> = first.chars().collect();
    let second: Vec<char> = second.chars().collect();
    let mut pending = VecDeque::from([(0, 0, false)]);
    let mut visited = HashSet::new();

    while let Some((first_index, second_index, consumed)) = pending.pop_front() {
        if !visited.insert((first_index, second_index, consumed)) {
            continue;
        }
        if first_index == first.len() && second_index == second.len() && consumed {
            return true;
        }

        if first.get(first_index) == Some(&'*') {
            pending.push_back((first_index + 1, second_index, consumed));
        }
        if second.get(second_index) == Some(&'*') {
            pending.push_back((first_index, second_index + 1, consumed));
        }

        let first_consumer = character_consumer(&first, first_index);
        let second_consumer = character_consumer(&second, second_index);
        if let (Some((first_character, next_first)), Some((second_character, next_second))) =
            (first_consumer, second_consumer)
        {
            if first_character.is_none()
                || second_character.is_none()
                || first_character == second_character
            {
                pending.push_back((next_first, next_second, true));
            }
        }
    }
    false
}

fn character_consumer(pattern: &[char], index: usize) -> Option<(Option<char>, usize)> {
    match pattern.get(index) {
        Some('*') => Some((None, index)),
        Some(character) => Some((Some(*character), index + 1)),
        None => None,
    }
}
