use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::{
    canonical_source_path_identity, contains_secret, glob_matches, CoreType, ExcludedSource,
    GeneratedDocument, GeneratedMetadata, LinkRelation, LinkWarning, ParsedSource, ProfileConfig,
    Sensitivity, SourceKind,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub source_path: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct ValidationReport {
    diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }

    pub fn errors(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .collect()
    }

    pub fn warnings(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Warning)
            .collect()
    }

    pub fn extend_link_warnings(&mut self, warnings: &[LinkWarning]) {
        for warning in warnings {
            self.warning(
                warning.code.clone(),
                Some(warning.source_path.clone()),
                format!("link target could not be resolved: {}", warning.target),
            );
        }
        self.sort();
    }

    pub fn extend_source_exclusions(&mut self, exclusions: &[ExcludedSource]) {
        for exclusion in exclusions {
            let code = match exclusion.reason.as_str() {
                "dirty" => "source.excluded_dirty",
                "untracked" => "source.excluded_untracked",
                _ => "source.excluded",
            };
            self.warning(
                code,
                Some(exclusion.source_path.clone()),
                format!("selected source was excluded: {}", exclusion.reason),
            );
        }
        self.sort();
    }

    fn error(
        &mut self,
        code: impl Into<String>,
        source_path: Option<String>,
        message: impl Into<String>,
    ) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            code: code.into(),
            source_path,
            message: message.into(),
        });
    }

    fn warning(
        &mut self,
        code: impl Into<String>,
        source_path: Option<String>,
        message: impl Into<String>,
    ) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: code.into(),
            source_path,
            message: message.into(),
        });
    }

    fn sort(&mut self) {
        self.diagnostics.sort_by(|left, right| {
            left.severity
                .cmp(&right.severity)
                .then_with(|| left.code.cmp(&right.code))
                .then_with(|| left.source_path.cmp(&right.source_path))
                .then_with(|| left.message.cmp(&right.message))
        });
    }
}

pub fn validate_document(
    profile: &ProfileConfig,
    document: &GeneratedDocument,
) -> ValidationReport {
    let mut report = ValidationReport::default();
    validate_document_into(profile, document, &mut report);
    report.sort();
    report
}

pub fn validate_bundle(
    profile: &ProfileConfig,
    documents: &[GeneratedDocument],
) -> ValidationReport {
    let mut report = ValidationReport::default();
    let mut ids = BTreeSet::new();
    let mut resources = BTreeSet::new();

    for document in documents {
        validate_document_into(profile, document, &mut report);
        if !ids.insert(document.metadata.id.clone()) {
            report.error(
                "bundle.duplicate_id",
                source_path(document),
                format!("duplicate document id: {}", document.metadata.id),
            );
        }
        if !resources.insert(document.metadata.resource.clone()) {
            report.error(
                "bundle.duplicate_resource",
                source_path(document),
                format!(
                    "duplicate document resource: {}",
                    document.metadata.resource
                ),
            );
        }
    }

    report.sort();
    report
}

pub fn validate_document_against_source(
    document: &GeneratedDocument,
    source: &ParsedSource,
) -> ValidationReport {
    let mut report = ValidationReport::default();
    let source_path = source_path(document);

    if document.metadata.source_path != source.source_path {
        report.error(
            "source.path_mismatch",
            source_path.clone(),
            "generated source path does not match parsed source evidence",
        );
    }
    if document.source_hash != source.source_hash {
        report.error(
            "source.hash_mismatch",
            source_path.clone(),
            "generated source hash does not match parsed source evidence",
        );
    }

    validate_date_against_source(
        "created",
        &document.metadata.created,
        document,
        source,
        source_path.clone(),
        &mut report,
    );
    validate_date_against_source(
        "updated",
        &document.metadata.updated,
        document,
        source,
        source_path,
        &mut report,
    );

    report.sort();
    report
}

fn validate_date_against_source(
    field: &str,
    generated: &str,
    document: &GeneratedDocument,
    source: &ParsedSource,
    source_path: Option<String>,
    report: &mut ValidationReport,
) {
    let expected = source_date_evidence(source, field);

    if !is_valid_gregorian_date(generated) || !expected.is_some_and(is_valid_gregorian_date) {
        report.error(
            "document.date_invalid",
            source_path.clone(),
            format!("{field} must be a valid Gregorian date using YYYY-MM-DD syntax"),
        );
    }
    if document
        .metadata
        .inferred_fields
        .iter()
        .any(|inferred| inferred == field)
    {
        report.error(
            "document.timestamp_inferred",
            source_path.clone(),
            format!("{field} must not be inferred"),
        );
    }

    if expected.is_some_and(|expected| generated != expected) {
        report.error(
            "source.date_mismatch",
            source_path,
            format!("generated {field} does not match source evidence"),
        );
    }
}

fn source_date_evidence<'a>(source: &'a ParsedSource, field: &str) -> Option<&'a str> {
    match source.metadata.get(field) {
        Some(serde_yaml::Value::String(date)) => Some(date),
        Some(_) => None,
        None => Some(match field {
            "created" => &source.created,
            "updated" => &source.updated,
            _ => unreachable!("only created and updated are validated"),
        }),
    }
}

fn is_valid_gregorian_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    if !(bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit()))
    {
        return false;
    }

    let year = date[..4].parse::<u32>().expect("year digits were checked");
    let month = date[5..7]
        .parse::<u32>()
        .expect("month digits were checked");
    let day = date[8..].parse::<u32>().expect("day digits were checked");
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };

    (1..=days_in_month).contains(&day)
}

fn validate_document_into(
    profile: &ProfileConfig,
    document: &GeneratedDocument,
    report: &mut ValidationReport,
) {
    let metadata = &document.metadata;
    let source_path = source_path(document);

    for (field, value) in required_fields(document) {
        if value.trim().is_empty() {
            report.error(
                format!("document.required.{field}"),
                source_path.clone(),
                format!("required field {field} must not be empty"),
            );
        }
    }

    for (field, actual, expected) in profile_identity_fields(profile, document) {
        if actual != expected {
            report.error(
                format!("identity.{field}"),
                source_path.clone(),
                format!("{field} must equal the active profile identity"),
            );
        }
    }

    let expected_id_prefix = format!("{}/", profile.context_namespace);
    if metadata.id == profile.context_namespace {
        report.error(
            "identity.id_path",
            source_path.clone(),
            "document id must include a canonical non-empty path after the profile namespace",
        );
    } else if let Some(path) = metadata.id.strip_prefix(&expected_id_prefix) {
        if !analyze_relative_path(path, &profile.context_namespace).is_profile_scoped_valid() {
            report.error(
                "identity.id_path",
                source_path.clone(),
                "document id path must use canonical percent-encoded relative path syntax",
            );
        }
    } else {
        report.error(
            "identity.id_namespace",
            source_path.clone(),
            "document id must use the active profile namespace",
        );
    }
    let expected_resource = format!("vault://{}", metadata.id);
    if metadata.resource != expected_resource {
        report.error(
            "identity.resource_namespace",
            source_path.clone(),
            "resource must be the vault URI for the document id",
        );
    }
    if vault_path_analysis(&metadata.resource, &profile.context_namespace)
        .is_some_and(|analysis| !analysis.is_profile_scoped_valid())
    {
        report.error(
            "identity.resource_path",
            source_path.clone(),
            "resource must include a canonical percent-encoded relative path",
        );
    }
    if metadata.source != expected_resource {
        report.error(
            "identity.source",
            source_path.clone(),
            "source must be the profile-scoped vault URI for the document id",
        );
    }

    let expected_id = if profile.has_exact_p3_judgment_extension() {
        super::p3_canonical_document_id(&profile.context_namespace, &metadata.source_path)
    } else {
        canonical_source_path_identity(&metadata.source_path)
            .ok()
            .map(|identity| format!("{}/{}", profile.context_namespace, identity))
    };
    let source_binding_matches = expected_id.is_some_and(|id| {
        let resource = format!("vault://{id}");
        metadata.id == id && metadata.resource == resource && metadata.source == resource
    });
    if !source_binding_matches {
        report.error(
            "identity.source_path_binding",
            source_path.clone(),
            "document identity must be canonically derived from its source path",
        );
    }
    if vault_path_analysis(&metadata.source, &profile.context_namespace)
        .is_some_and(|analysis| !analysis.is_profile_scoped_valid())
    {
        report.error(
            "identity.source_path",
            source_path.clone(),
            "source must include a canonical percent-encoded relative path",
        );
    }

    if let Some(expected_core_type) = profile.context_kinds.get(&metadata.context_kind) {
        if expected_core_type != &metadata.core_type {
            report.error(
                "document.core_type_mismatch",
                source_path.clone(),
                "core type does not match the declared context kind",
            );
        }
    } else {
        report.warning(
            "document.context_kind_unknown",
            source_path.clone(),
            "context kind is not approved by the active profile",
        );
    }

    if metadata.created.trim().is_empty() || metadata.updated.trim().is_empty() {
        report.error(
            "document.timestamp_missing",
            source_path.clone(),
            "created and updated timestamps are required",
        );
    }
    for (field, date) in [
        ("created", metadata.created.as_str()),
        ("updated", metadata.updated.as_str()),
    ] {
        if !date.trim().is_empty() && !is_valid_gregorian_date(date) {
            report.error(
                "document.date_invalid",
                source_path.clone(),
                format!("{field} must be a valid Gregorian date using YYYY-MM-DD syntax"),
            );
        }
    }
    if metadata
        .inferred_fields
        .iter()
        .any(|field| matches!(field.as_str(), "created" | "updated"))
    {
        report.error(
            "document.timestamp_inferred",
            source_path.clone(),
            "timestamps must come from explicit source metadata or the pinned Git snapshot",
        );
    }

    if metadata.sensitivity == Sensitivity::Public
        && !profile
            .public_allowlist
            .iter()
            .any(|pattern| glob_matches(pattern, &metadata.source_path))
    {
        report.error(
            "document.public_not_allowlisted",
            source_path.clone(),
            "public sensitivity requires an explicit profile allowlist entry",
        );
    }
    if metadata.sensitivity == Sensitivity::Restricted
        && !private_scope(metadata.extensions.get("scope"))
    {
        report.error(
            "document.restricted_scope",
            source_path.clone(),
            "restricted material requires an explicit private scope",
        );
    }
    if contains_secret(&document.body) && !restricted_private_metadata(metadata) {
        report.error(
            "document.restricted_content",
            source_path.clone(),
            "secret-like generated content requires restricted sensitivity and private scope",
        );
    }

    validate_judgment(profile, document, report);
    validate_links(profile, document, report);
    validate_extension_conflicts(document, report);

    if metadata
        .inferred_fields
        .iter()
        .any(|field| field == "description")
    {
        report.warning(
            "document.inferred_description",
            source_path.clone(),
            "description was inferred instead of copied from explicit source metadata",
        );
    }
    if metadata
        .inferred_fields
        .iter()
        .any(|field| field == "provenance")
    {
        report.warning(
            "document.inferred_provenance",
            source_path.clone(),
            "provenance was inferred instead of copied from explicit source metadata",
        );
    }
    if metadata.source_path.starts_with("04-Research/")
        && metadata.source_kind != SourceKind::SourceSnapshot
        && !metadata
            .links
            .iter()
            .any(|link| link.rel == LinkRelation::Cites)
    {
        report.warning(
            "research.weak_citation",
            source_path,
            "research document has no explicit citation",
        );
    }
}

fn required_fields(document: &GeneratedDocument) -> [(&'static str, &str); 17] {
    let metadata = &document.metadata;
    [
        ("okf_version", &metadata.okf_version),
        ("profile", &metadata.profile),
        ("nous_schema", &metadata.nous_schema),
        ("ssa_id", &metadata.ssa_id),
        ("ssa_profile", &metadata.ssa_profile),
        ("familiar_id", &metadata.familiar_id),
        ("context_namespace", &metadata.context_namespace),
        ("id", &metadata.id),
        ("context_kind", &metadata.context_kind),
        ("title", &metadata.title),
        ("description", &metadata.description),
        ("source", &metadata.source),
        ("source_path", &metadata.source_path),
        ("resource", &metadata.resource),
        ("created", &metadata.created),
        ("updated", &metadata.updated),
        ("source_hash", &document.source_hash),
    ]
}

fn profile_identity_fields<'a>(
    profile: &'a ProfileConfig,
    document: &'a GeneratedDocument,
) -> [(&'static str, &'a str, &'a str); 7] {
    let metadata = &document.metadata;
    [
        ("okf_version", &metadata.okf_version, &profile.okf_version),
        ("profile", &metadata.profile, &profile.profile),
        ("nous_schema", &metadata.nous_schema, &profile.nous_schema),
        ("ssa_id", &metadata.ssa_id, &profile.ssa_id),
        ("ssa_profile", &metadata.ssa_profile, &profile.ssa_profile),
        ("familiar_id", &metadata.familiar_id, &profile.familiar_id),
        (
            "context_namespace",
            &metadata.context_namespace,
            &profile.context_namespace,
        ),
    ]
}

fn private_scope(scope: Option<&serde_yaml::Value>) -> bool {
    scope.and_then(serde_yaml::Value::as_str) == Some("private")
}

pub(crate) fn restricted_private_metadata(metadata: &GeneratedMetadata) -> bool {
    metadata.sensitivity == Sensitivity::Restricted
        && private_scope(metadata.extensions.get("scope"))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RelativePathAnalysis {
    valid: bool,
    foreign_namespace: bool,
    segment_count: usize,
    first_is_profile_namespace: bool,
}

impl RelativePathAnalysis {
    fn is_profile_scoped_valid(self) -> bool {
        self.valid && !self.foreign_namespace
    }
}

fn analyze_relative_path(path: &str, context_namespace: &str) -> RelativePathAnalysis {
    if path.is_empty() {
        return RelativePathAnalysis::default();
    }

    let mut analysis = RelativePathAnalysis {
        valid: true,
        foreign_namespace: false,
        segment_count: 0,
        first_is_profile_namespace: false,
    };
    for (index, segment) in path.split('/').enumerate() {
        analysis.segment_count += 1;
        let Ok(decoded) = decode_canonical_path_segment(segment) else {
            analysis.valid = false;
            continue;
        };
        if !decoded.canonical
            || decoded.value.is_empty()
            || matches!(decoded.value.as_str(), "." | "..")
            || decoded.value.contains(['/', '\\'])
        {
            analysis.valid = false;
        }
        if index == 0 && decoded.value.starts_with("ssa-") {
            analysis.first_is_profile_namespace = true;
        }
        if decoded.value.starts_with("ssa-") && decoded.value != context_namespace {
            analysis.foreign_namespace = true;
        }
    }
    analysis
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DecodedPathSegment {
    value: String,
    canonical: bool,
}

fn decode_canonical_path_segment(segment: &str) -> Result<DecodedPathSegment, ()> {
    if segment.is_empty() {
        return Err(());
    }

    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if is_rfc3986_unreserved(byte) {
            decoded.push(byte);
            index += 1;
        } else if byte == b'%' && index + 2 < bytes.len() {
            let Some(high) = hex_value(bytes[index + 1]) else {
                return Err(());
            };
            let Some(low) = hex_value(bytes[index + 2]) else {
                return Err(());
            };
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            return Err(());
        }
    }

    let value = String::from_utf8(decoded).map_err(|_| ())?;
    Ok(DecodedPathSegment {
        canonical: encode_canonical_path_segment(&value) == segment,
        value,
    })
}

fn encode_canonical_path_segment(segment: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if is_rfc3986_unreserved(byte) {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

fn is_rfc3986_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn vault_path_analysis(value: &str, context_namespace: &str) -> Option<RelativePathAnalysis> {
    const VAULT_PREFIX: &str = "vault://";
    if !value
        .get(..VAULT_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(VAULT_PREFIX))
    {
        return None;
    }

    let rest = &value[VAULT_PREFIX.len()..];
    let Some((_, path)) = rest.split_once('/') else {
        return Some(RelativePathAnalysis::default());
    };
    Some(analyze_relative_path(path, context_namespace))
}

fn validate_judgment(
    profile: &ProfileConfig,
    document: &GeneratedDocument,
    report: &mut ValidationReport,
) {
    let metadata = &document.metadata;
    let source_path = source_path(document);

    if !profile.has_exact_p3_judgment_extension()
        && profile
            .mapping_rules
            .iter()
            .any(|rule| rule.core_type == CoreType::Judgment)
    {
        report.error(
            "judgment.mapping_forbidden",
            source_path.clone(),
            "mapping rules must not create judgment records",
        );
    }
    if metadata.core_type != CoreType::Judgment {
        return;
    }

    match metadata.verdict.as_deref() {
        None | Some("") => report.error(
            "judgment.verdict_missing",
            source_path.clone(),
            "judgment records require a human verdict",
        ),
        Some("ACCEPT" | "REJECT" | "REWRITE" | "TP" | "FP") => {}
        Some(_) => report.error(
            "judgment.verdict_invalid",
            source_path.clone(),
            "judgment verdict must be ACCEPT, REJECT, REWRITE, TP, or FP",
        ),
    }
    match metadata.reason.as_deref() {
        None => report.error(
            "judgment.reason_missing",
            source_path.clone(),
            "judgment records require a human reason",
        ),
        Some(reason) if reason.trim().is_empty() => report.error(
            "judgment.reason_empty",
            source_path.clone(),
            "judgment reason must not be empty",
        ),
        Some(_) => {}
    }
    match metadata.subject.as_deref() {
        None | Some("") => report.error(
            "judgment.subject_missing",
            source_path.clone(),
            "judgment records require a profile-scoped subject",
        ),
        Some(subject)
            if subject
                .strip_prefix(&format!("{}/", profile.context_namespace))
                .is_some_and(|identity| {
                    analyze_relative_path(identity, &profile.context_namespace)
                        .is_profile_scoped_valid()
                }) => {}
        Some(_) => report.error(
            "judgment.subject_not_scoped",
            source_path.clone(),
            "judgment subject must use the active profile namespace",
        ),
    }
    if metadata
        .inferred_fields
        .iter()
        .any(|field| matches!(field.as_str(), "type" | "verdict" | "reason" | "subject"))
    {
        report.error(
            "judgment.not_human",
            source_path,
            "judgment type, verdict, reason, and subject must be explicit human-produced metadata",
        );
    }
}

fn validate_links(
    profile: &ProfileConfig,
    document: &GeneratedDocument,
    report: &mut ValidationReport,
) {
    for link in &document.metadata.links {
        let analysis = analyze_link_target(&link.target, &profile.context_namespace);
        if analysis.invalid {
            report.error(
                "links.target_invalid",
                source_path(document),
                format!(
                    "link target is not a canonical internal path: {}",
                    link.target
                ),
            );
        }
        if analysis.foreign_namespace {
            report.error(
                "links.cross_profile",
                source_path(document),
                format!(
                    "link target is outside the active profile namespace: {}",
                    link.target
                ),
            );
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LinkTargetAnalysis {
    invalid: bool,
    foreign_namespace: bool,
}

fn analyze_link_target(target: &str, context_namespace: &str) -> LinkTargetAnalysis {
    let target = target.split_once('#').map_or(target, |(path, _)| path);

    if let Some((scheme, rest)) = target.split_once("://") {
        if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
            return LinkTargetAnalysis::default();
        }
        if !scheme.eq_ignore_ascii_case("vault") {
            return LinkTargetAnalysis::default();
        }
        return analyze_vault_link_target(rest, context_namespace);
    }
    if target
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("mailto:"))
    {
        return LinkTargetAnalysis::default();
    }

    let path = analyze_relative_path(target, context_namespace);
    LinkTargetAnalysis {
        invalid: !path.valid || (path.first_is_profile_namespace && path.segment_count == 1),
        foreign_namespace: path.foreign_namespace,
    }
}

fn analyze_vault_link_target(rest: &str, context_namespace: &str) -> LinkTargetAnalysis {
    let Some((namespace, path)) = rest.split_once('/') else {
        return LinkTargetAnalysis {
            invalid: true,
            foreign_namespace: false,
        };
    };
    let (namespace_invalid, namespace_is_foreign) = match decode_canonical_path_segment(namespace) {
        Ok(decoded) => (
            !decoded.canonical
                || decoded.value.is_empty()
                || matches!(decoded.value.as_str(), "." | "..")
                || decoded.value.contains(['/', '\\']),
            decoded.value != context_namespace,
        ),
        Err(()) => (true, false),
    };
    let path = analyze_relative_path(path, context_namespace);

    LinkTargetAnalysis {
        invalid: namespace_invalid || !path.valid,
        foreign_namespace: namespace_is_foreign || path.foreign_namespace,
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn validate_extension_conflicts(document: &GeneratedDocument, report: &mut ValidationReport) {
    const RESERVED_FIELDS: &[&str] = &[
        "okf_version",
        "profile",
        "nous_schema",
        "ssa_id",
        "ssa_profile",
        "familiar_id",
        "context_namespace",
        "id",
        "type",
        "context_kind",
        "title",
        "description",
        "layer",
        "source",
        "source_path",
        "resource",
        "source_kind",
        "provenance",
        "status",
        "authority",
        "sensitivity",
        "created",
        "updated",
        "tags",
        "links",
        "inferred_fields",
        "owners",
        "valid_from",
        "valid_to",
        "supersedes",
        "superseded_by",
        "verdict",
        "reason",
        "subject",
    ];

    for field in document.metadata.extensions.keys() {
        if RESERVED_FIELDS.contains(&field.as_str()) {
            report.error(
                "metadata.conflicting_explicit",
                source_path(document),
                format!("extension field conflicts with generated metadata: {field}"),
            );
        }
    }
}

fn source_path(document: &GeneratedDocument) -> Option<String> {
    (!document.metadata.source_path.is_empty()).then(|| document.metadata.source_path.clone())
}
