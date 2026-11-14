use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::BTreeMap;
use thiserror::Error;

use super::{Authority, CoreType, Layer, Sensitivity, SourceDocument};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    VaultNote,
    SourceSnapshot,
    GeneratedReport,
    ExternalReference,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provenance {
    Tier1,
    Secondary,
    Synthesis,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Active,
    Review,
    Draft,
    Generated,
    Superseded,
    Archived,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum LinkRelation {
    Cites,
    DerivedFrom,
    Supersedes,
    DependsOn,
    References,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct TypedLink {
    pub rel: LinkRelation,
    pub target: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct GeneratedMetadata {
    pub okf_version: String,
    pub profile: String,
    pub nous_schema: String,
    pub ssa_id: String,
    pub ssa_profile: String,
    pub familiar_id: String,
    pub context_namespace: String,
    pub id: String,
    #[serde(rename = "type")]
    pub core_type: CoreType,
    pub context_kind: String,
    pub title: String,
    pub description: String,
    pub layer: Layer,
    pub source: String,
    pub source_path: String,
    pub resource: String,
    pub source_kind: SourceKind,
    pub provenance: Provenance,
    pub status: Status,
    pub authority: Authority,
    pub sensitivity: Sensitivity,
    pub created: String,
    pub updated: String,
    pub tags: Vec<String>,
    pub links: Vec<TypedLink>,
    pub inferred_fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedSource {
    pub source_path: String,
    pub body: String,
    pub source_hash: String,
    pub created: String,
    pub updated: String,
    pub metadata: BTreeMap<String, Value>,
    pub(crate) contains_secret: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedDocument {
    pub metadata: GeneratedMetadata,
    pub body: String,
    pub source_hash: String,
}

#[derive(Debug, Error)]
pub enum FrontmatterError {
    #[error("source document is not valid UTF-8: {0}")]
    Utf8(String),
    #[error("source frontmatter is not terminated")]
    Unterminated,
    #[error("source frontmatter could not be parsed: {0}")]
    Parse(String),
    #[error("generated frontmatter could not be serialized: {0}")]
    Serialize(String),
}

impl FrontmatterError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Utf8(_) => "frontmatter.utf8",
            Self::Unterminated => "frontmatter.unterminated",
            Self::Parse(_) => "frontmatter.parse",
            Self::Serialize(_) => "frontmatter.serialize",
        }
    }
}

pub fn parse_source_document(source: SourceDocument) -> Result<ParsedSource, FrontmatterError> {
    let text = String::from_utf8(source.bytes)
        .map_err(|error| FrontmatterError::Utf8(error.to_string()))?;
    let contains_secret = contains_secret(&text);
    let (metadata, body) = split_frontmatter(&text)?;

    Ok(ParsedSource {
        source_path: source.source_path,
        body,
        source_hash: source.source_hash,
        created: source.created,
        updated: source.updated,
        metadata,
        contains_secret,
    })
}

pub fn render_document(document: &GeneratedDocument) -> Result<Vec<u8>, FrontmatterError> {
    let yaml = serde_yaml::to_string(&document.metadata)
        .map_err(|error| FrontmatterError::Serialize(error.to_string()))?;
    let yaml = yaml.strip_prefix("---\n").unwrap_or(&yaml);
    let mut rendered = Vec::with_capacity(yaml.len() + document.body.len() + 8);
    rendered.extend_from_slice(b"---\n");
    rendered.extend_from_slice(yaml.as_bytes());
    if !yaml.ends_with('\n') {
        rendered.push(b'\n');
    }
    rendered.extend_from_slice(b"---\n");
    rendered.extend_from_slice(document.body.as_bytes());
    Ok(rendered)
}

fn split_frontmatter(text: &str) -> Result<(BTreeMap<String, Value>, String), FrontmatterError> {
    let start = if text.starts_with("---\n") {
        4
    } else if text.starts_with("---\r\n") {
        5
    } else {
        return Ok((BTreeMap::new(), text.to_owned()));
    };

    let rest = &text[start..];
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            let yaml = &rest[..offset];
            let metadata = if yaml.trim().is_empty() {
                BTreeMap::new()
            } else {
                serde_yaml::from_str(yaml)
                    .map_err(|error| FrontmatterError::Parse(error.to_string()))?
            };
            let body = rest[offset + line.len()..].to_owned();
            return Ok((metadata, body));
        }
        offset += line.len();
    }

    Err(FrontmatterError::Unterminated)
}

pub(crate) fn contains_secret(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start_matches(|character: char| character.is_ascii_whitespace());
        let folded = trimmed.to_ascii_lowercase();
        ["api_key:", "token:", "password:", "secret:"]
            .iter()
            .any(|prefix| folded.starts_with(prefix))
            || (folded.contains("-----begin ") && folded.contains("private key-----"))
    })
}
