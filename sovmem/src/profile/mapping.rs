use serde::de::DeserializeOwned;
use serde_yaml::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use thiserror::Error;

use super::{
    glob_matches, Authority, CoreType, GeneratedDocument, GeneratedMetadata, Layer, ParsedSource,
    ProfileConfig, Provenance, Sensitivity, SourceKind, Status, TypedLink,
};

#[derive(Debug, Error)]
pub enum NormalizeError {
    #[error("no profile mapping matches source path: {0}")]
    MappingMissing(String),
    #[error("explicit {field} conflicts with profile identity")]
    IdentityConflict { field: String },
    #[error("explicit metadata field {field} is invalid: {message}")]
    MetadataInvalid { field: String, message: String },
    #[error("source {field} date is missing: {source_path}")]
    DateMissing {
        field: &'static str,
        source_path: String,
    },
    #[error("public sensitivity is not allowlisted: {0}")]
    PublicNotAllowlisted(String),
    #[error("source path cannot produce a document identity: {0}")]
    PathInvalid(String),
}

impl NormalizeError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MappingMissing(_) => "normalize.mapping_missing",
            Self::IdentityConflict { .. } => "normalize.identity_conflict",
            Self::MetadataInvalid { .. } => "normalize.metadata_invalid",
            Self::DateMissing { .. } => "normalize.date_missing",
            Self::PublicNotAllowlisted(_) => "normalize.public_not_allowlisted",
            Self::PathInvalid(_) => "normalize.path_invalid",
        }
    }
}

pub fn normalize_document(
    profile: &ProfileConfig,
    source: ParsedSource,
) -> Result<GeneratedDocument, NormalizeError> {
    let mapping = profile
        .mapping_for(&source.source_path)
        .cloned()
        .ok_or_else(|| NormalizeError::MappingMissing(source.source_path.clone()))?;
    let mut explicit = source.metadata;

    check_fixed(&mut explicit, "okf_version", &profile.okf_version)?;
    check_fixed(&mut explicit, "profile", &profile.profile)?;
    check_fixed(&mut explicit, "nous_schema", &profile.nous_schema)?;
    check_fixed(&mut explicit, "ssa_id", &profile.ssa_id)?;
    check_fixed(&mut explicit, "ssa_profile", &profile.ssa_profile)?;
    check_fixed(&mut explicit, "familiar_id", &profile.familiar_id)?;
    check_fixed(
        &mut explicit,
        "context_namespace",
        &profile.context_namespace,
    )?;

    let relative_identity = canonical_source_path_identity(&source.source_path)?;
    let id = format!("{}/{}", profile.context_namespace, relative_identity);
    let resource = format!(
        "vault://{}/{}",
        profile.context_namespace, relative_identity
    );
    check_fixed(&mut explicit, "id", &id)?;
    check_fixed(&mut explicit, "resource", &resource)?;
    check_fixed(&mut explicit, "source_path", &source.source_path)?;

    let explicit_type = take_enum::<CoreType>(&mut explicit, "type")?;
    let core_type = explicit_type
        .clone()
        .unwrap_or_else(|| mapping.core_type.clone());
    let explicit_context_kind = take_string(&mut explicit, "context_kind")?;
    let context_kind = explicit_context_kind
        .clone()
        .unwrap_or_else(|| mapping.context_kind.clone());
    if context_kind.trim().is_empty() {
        return invalid("context_kind", "must not be empty");
    }
    if let Some(expected_type) = profile.context_kinds.get(&context_kind) {
        if expected_type != &core_type {
            return invalid(
                "type",
                format!("does not match context_kind {context_kind:?}"),
            );
        }
    }

    let explicit_title = take_string(&mut explicit, "title")?;
    let title = match explicit_title.clone() {
        Some(title) if !title.trim().is_empty() => title,
        Some(_) => return invalid("title", "must not be empty"),
        None => infer_title(&source.body, &source.source_path)?,
    };
    let explicit_description = take_string(&mut explicit, "description")?;
    let description = match explicit_description.clone() {
        Some(description) if !description.trim().is_empty() => description,
        Some(_) => return invalid("description", "must not be empty"),
        None => infer_description(&source.body).ok_or_else(|| NormalizeError::MetadataInvalid {
            field: "description".into(),
            message: "no meaningful prose paragraph exists".into(),
        })?,
    };

    let explicit_layer = take_enum::<Layer>(&mut explicit, "layer")?;
    let layer = explicit_layer
        .clone()
        .unwrap_or_else(|| mapping.layer.clone());
    check_fixed(&mut explicit, "source", &resource)?;

    let explicit_source_kind = take_enum::<SourceKind>(&mut explicit, "source_kind")?;
    let source_kind = explicit_source_kind.clone().unwrap_or_else(|| {
        if context_kind == "Source Snapshot" {
            SourceKind::SourceSnapshot
        } else {
            SourceKind::VaultNote
        }
    });
    let explicit_provenance = take_enum::<Provenance>(&mut explicit, "provenance")?;
    let provenance = explicit_provenance.clone().unwrap_or_else(|| {
        if context_kind == "Source Snapshot" {
            Provenance::Unknown
        } else {
            Provenance::Synthesis
        }
    });

    let explicit_authority = take_enum::<Authority>(&mut explicit, "authority")?;
    let authority = explicit_authority
        .clone()
        .unwrap_or_else(|| mapping.authority.clone());
    let explicit_status = take_enum::<Status>(&mut explicit, "status")?;
    let status = explicit_status
        .clone()
        .unwrap_or_else(|| status_for(&authority));

    let explicit_sensitivity = take_enum::<Sensitivity>(&mut explicit, "sensitivity")?;
    let mut sensitivity = explicit_sensitivity
        .clone()
        .unwrap_or_else(|| profile.sensitivity_for(&source.source_path));
    if source.contains_secret {
        sensitivity = Sensitivity::Restricted;
    }
    if sensitivity == Sensitivity::Public
        && !profile
            .public_allowlist
            .iter()
            .any(|pattern| glob_matches(pattern, &source.source_path))
    {
        return Err(NormalizeError::PublicNotAllowlisted(source.source_path));
    }

    let explicit_created = take_string(&mut explicit, "created")?;
    let created = required_date(
        explicit_created,
        source.created,
        "created",
        &source.source_path,
    )?;
    let explicit_updated = take_string(&mut explicit, "updated")?;
    let updated = required_date(
        explicit_updated,
        source.updated,
        "updated",
        &source.source_path,
    )?;

    let explicit_tags = take_string_list(&mut explicit, "tags")?;
    let (tags, tags_inferred) = normalize_tags(
        explicit_tags.as_deref(),
        profile,
        &context_kind,
        &source.source_path,
    );
    let explicit_links = take_enum::<Vec<TypedLink>>(&mut explicit, "links")?;
    let mut links = explicit_links.clone().unwrap_or_default();
    links.sort();
    links.dedup();

    let owners = take_string_list(&mut explicit, "owners")?;
    let valid_from = take_string(&mut explicit, "valid_from")?;
    let valid_to = take_string(&mut explicit, "valid_to")?;
    let supersedes = take_string(&mut explicit, "supersedes")?;
    let superseded_by = take_string(&mut explicit, "superseded_by")?;
    let verdict = take_string(&mut explicit, "verdict")?;
    let reason = take_string(&mut explicit, "reason")?;
    let subject = take_string(&mut explicit, "subject")?;
    explicit.remove("inferred_fields");

    let mut inferred_fields = Vec::new();
    push_inferred(&mut inferred_fields, "type", explicit_type.is_none());
    push_inferred(
        &mut inferred_fields,
        "context_kind",
        explicit_context_kind.is_none(),
    );
    push_inferred(&mut inferred_fields, "title", explicit_title.is_none());
    push_inferred(
        &mut inferred_fields,
        "description",
        explicit_description.is_none(),
    );
    push_inferred(&mut inferred_fields, "layer", explicit_layer.is_none());
    push_inferred(
        &mut inferred_fields,
        "source_kind",
        explicit_source_kind.is_none(),
    );
    push_inferred(
        &mut inferred_fields,
        "provenance",
        explicit_provenance.is_none(),
    );
    push_inferred(&mut inferred_fields, "status", explicit_status.is_none());
    push_inferred(
        &mut inferred_fields,
        "authority",
        explicit_authority.is_none(),
    );
    push_inferred(
        &mut inferred_fields,
        "sensitivity",
        explicit_sensitivity.is_none()
            || (source.contains_secret && explicit_sensitivity != Some(Sensitivity::Restricted)),
    );
    push_inferred(&mut inferred_fields, "tags", tags_inferred);
    push_inferred(&mut inferred_fields, "links", explicit_links.is_none());

    Ok(GeneratedDocument {
        metadata: GeneratedMetadata {
            okf_version: profile.okf_version.clone(),
            profile: profile.profile.clone(),
            nous_schema: profile.nous_schema.clone(),
            ssa_id: profile.ssa_id.clone(),
            ssa_profile: profile.ssa_profile.clone(),
            familiar_id: profile.familiar_id.clone(),
            context_namespace: profile.context_namespace.clone(),
            id,
            core_type,
            context_kind,
            title,
            description,
            layer,
            source: resource.clone(),
            source_path: source.source_path,
            resource,
            source_kind,
            provenance,
            status,
            authority,
            sensitivity,
            created,
            updated,
            tags,
            links,
            inferred_fields,
            owners,
            valid_from,
            valid_to,
            supersedes,
            superseded_by,
            verdict,
            reason,
            subject,
            extensions: explicit,
        },
        body: source.body,
        source_hash: source.source_hash,
    })
}

fn check_fixed(
    metadata: &mut BTreeMap<String, Value>,
    field: &str,
    expected: &str,
) -> Result<(), NormalizeError> {
    let Some(value) = metadata.remove(field) else {
        return Ok(());
    };
    if value.as_str() == Some(expected) {
        Ok(())
    } else {
        Err(NormalizeError::IdentityConflict {
            field: field.into(),
        })
    }
}

fn take_string(
    metadata: &mut BTreeMap<String, Value>,
    field: &str,
) -> Result<Option<String>, NormalizeError> {
    let Some(value) = metadata.remove(field) else {
        return Ok(None);
    };
    value
        .as_str()
        .map(|value| Some(value.to_owned()))
        .ok_or_else(|| NormalizeError::MetadataInvalid {
            field: field.into(),
            message: "expected a string".into(),
        })
}

fn take_enum<T: DeserializeOwned>(
    metadata: &mut BTreeMap<String, Value>,
    field: &str,
) -> Result<Option<T>, NormalizeError> {
    let Some(value) = metadata.remove(field) else {
        return Ok(None);
    };
    serde_yaml::from_value(value)
        .map(Some)
        .map_err(|error| NormalizeError::MetadataInvalid {
            field: field.into(),
            message: error.to_string(),
        })
}

fn take_string_list(
    metadata: &mut BTreeMap<String, Value>,
    field: &str,
) -> Result<Option<Vec<String>>, NormalizeError> {
    take_enum(metadata, field)
}

fn invalid<T>(field: &str, message: impl Into<String>) -> Result<T, NormalizeError> {
    Err(NormalizeError::MetadataInvalid {
        field: field.into(),
        message: message.into(),
    })
}

fn required_date(
    explicit: Option<String>,
    git_date: String,
    field: &'static str,
    source_path: &str,
) -> Result<String, NormalizeError> {
    let date = explicit.unwrap_or(git_date);
    if date.trim().is_empty() {
        Err(NormalizeError::DateMissing {
            field,
            source_path: source_path.into(),
        })
    } else {
        Ok(date)
    }
}

fn status_for(authority: &Authority) -> Status {
    match authority {
        Authority::Canonical => Status::Active,
        Authority::Reviewed => Status::Review,
        Authority::Working | Authority::Raw => Status::Draft,
    }
}

fn push_inferred(fields: &mut Vec<String>, field: &str, inferred: bool) {
    if inferred {
        fields.push(field.into());
    }
}

pub(crate) fn canonical_source_path_identity(source_path: &str) -> Result<String, NormalizeError> {
    let without_extension = source_path.strip_suffix(".md").unwrap_or(source_path);
    if without_extension.is_empty()
        || without_extension.starts_with('/')
        || without_extension
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(NormalizeError::PathInvalid(source_path.into()));
    }
    Ok(without_extension
        .split('/')
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/"))
}

fn percent_encode_segment(segment: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

fn infer_title(body: &str, source_path: &str) -> Result<String, NormalizeError> {
    if let Some(title) = first_h1(body) {
        let title = strip_markdown(strip_closing_atx_hashes(title));
        if !title.is_empty() {
            return Ok(title);
        }
    }
    let stem = Path::new(source_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| NormalizeError::PathInvalid(source_path.into()))?;
    Ok(stem.to_owned())
}

fn first_h1(body: &str) -> Option<&str> {
    body.lines().find_map(|line| {
        line.trim_start_matches(' ')
            .strip_prefix("# ")
            .map(str::trim)
            .filter(|title| !title.is_empty())
    })
}

fn strip_closing_atx_hashes(title: &str) -> &str {
    let trimmed = title.trim_end();
    let without_hashes = trimmed.trim_end_matches('#');
    if without_hashes.len() < trimmed.len()
        && without_hashes
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
    {
        without_hashes.trim_end()
    } else {
        title
    }
}

fn infer_description(body: &str) -> Option<String> {
    let lines: Vec<&str> = body.lines().collect();
    let h1_index = lines
        .iter()
        .position(|line| line.trim_start_matches(' ').strip_prefix("# ").is_some());
    let start = h1_index.map_or(0, |index| index + 1);
    let mut paragraph = Vec::new();
    let mut fenced = false;

    for line in &lines[start..] {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if paragraph.is_empty()
            && (trimmed.starts_with('#')
                || trimmed == "---"
                || trimmed.starts_with("| ---")
                || trimmed.starts_with("<!--"))
        {
            continue;
        }
        if let Some(prose) = clean_prose_line(trimmed) {
            paragraph.push(prose);
        }
    }

    if paragraph.is_empty() {
        return None;
    }
    let plain = strip_markdown(&paragraph.join(" "));
    if plain.is_empty() {
        return None;
    }
    let sentence = first_sentence(&plain);
    Some(sentence.chars().take(240).collect())
}

fn clean_prose_line(line: &str) -> Option<String> {
    let mut prose = line;
    while let Some(rest) = prose.strip_prefix('>') {
        prose = rest.trim_start();
    }

    if let Some(rest) = prose
        .strip_prefix("- ")
        .or_else(|| prose.strip_prefix("* "))
        .or_else(|| prose.strip_prefix("+ "))
    {
        prose = rest;
    } else if let Some(marker_end) = ordered_list_marker_end(prose) {
        prose = prose[marker_end..].trim_start();
    }

    if prose.contains('|') {
        let cells: Vec<&str> = prose.trim_matches('|').split('|').map(str::trim).collect();
        if !cells.is_empty() && cells.iter().all(|cell| is_table_separator_cell(cell)) {
            return None;
        }
        prose = prose.trim_matches('|').trim();
        return Some(prose.replace('|', " "));
    }

    Some(prose.to_owned())
}

fn ordered_list_marker_end(line: &str) -> Option<usize> {
    let digit_count = line.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 {
        return None;
    }
    let suffix = line.get(digit_count..)?;
    if suffix.starts_with(". ") || suffix.starts_with(") ") {
        Some(digit_count + 1)
    } else {
        None
    }
}

fn is_table_separator_cell(cell: &str) -> bool {
    let hyphens = cell.bytes().filter(|byte| *byte == b'-').count();
    hyphens >= 3 && cell.bytes().all(|byte| matches!(byte, b'-' | b':'))
}

fn first_sentence(text: &str) -> &str {
    for (index, character) in text.char_indices() {
        if matches!(character, '.' | '!' | '?') {
            let end = index + character.len_utf8();
            if text[end..].chars().next().is_none_or(char::is_whitespace) {
                return &text[..end];
            }
        }
    }
    text
}

fn strip_markdown(input: &str) -> String {
    let with_wikilinks = replace_wikilinks(input);
    let with_link_labels = replace_markdown_links(&with_wikilinks);
    let mut plain = String::with_capacity(with_link_labels.len());
    for character in with_link_labels.chars() {
        match character {
            '*' | '_' | '~' | '`' => {}
            _ => plain.push(character),
        }
    }
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn replace_wikilinks(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("[[") {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("]]") else {
            output.push_str(&rest[start..]);
            return output;
        };
        let target = &after_start[..end];
        let label = target
            .rsplit_once('|')
            .map(|(_, alias)| alias)
            .unwrap_or_else(|| target.split('#').next().unwrap_or(target));
        output.push_str(label);
        rest = &after_start[end + 2..];
    }
    output.push_str(rest);
    output
}

fn replace_markdown_links(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        let remaining = &input[index..];
        let (prefix, open_offset) = if remaining.starts_with("![") {
            (2, 0)
        } else if remaining.starts_with('[') {
            (1, 0)
        } else {
            let character = remaining.chars().next().expect("remaining is non-empty");
            output.push(character);
            index += character.len_utf8();
            continue;
        };
        let label_start = index + open_offset + prefix;
        let Some(close_offset) = input[label_start..].find(']') else {
            output.push_str(&input[index..]);
            break;
        };
        let close = label_start + close_offset;
        if !input[close..].starts_with("](") {
            output.push_str(&input[index..=close]);
            index = close + 1;
            continue;
        }
        let url_start = close + 2;
        let Some(url_end_offset) = input[url_start..].find(')') else {
            output.push_str(&input[index..]);
            break;
        };
        output.push_str(&input[label_start..close]);
        index = url_start + url_end_offset + 1;
    }
    output
}

fn normalize_tags(
    explicit: Option<&[String]>,
    profile: &ProfileConfig,
    context_kind: &str,
    source_path: &str,
) -> (Vec<String>, bool) {
    let explicit_tags: BTreeSet<String> = explicit
        .unwrap_or_default()
        .iter()
        .map(|tag| tag.trim().to_lowercase())
        .filter(|tag| !tag.is_empty())
        .collect();
    let mut tags = explicit_tags.clone();
    let mut required = BTreeSet::from([
        profile.familiar_id.to_lowercase(),
        "ssa".into(),
        "tfis".into(),
        kebab_case(context_kind),
    ]);

    let segments: Vec<&str> = source_path.split('/').collect();
    if segments.first() == Some(&"04-Research") && segments.len() > 1 {
        required.insert(kebab_case(segments[1]));
    }
    if let Some(lane) = segments.first().filter(|segment| {
        matches!(
            **segment,
            "08-Hermes" | "09-Claude" | "10-Codex" | "11-Antigravity"
        )
    }) {
        let lane = lane.split_once('-').map(|(_, name)| name).unwrap_or(lane);
        required.insert(kebab_case(lane));
    }
    required.retain(|tag| !tag.is_empty());
    let inferred = explicit.is_none() || required.iter().any(|tag| !explicit_tags.contains(tag));
    tags.extend(required);
    (tags.into_iter().collect(), inferred)
}

fn kebab_case(value: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for character in value.chars() {
        if character.is_alphanumeric() {
            if separator && !result.is_empty() {
                result.push('-');
            }
            result.extend(character.to_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    result
}
