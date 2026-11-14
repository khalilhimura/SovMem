use serde_yaml::Value;
use sovmem::profile::{
    load_p3_profile, load_profile, normalize_document, parse_source_document, validate_bundle,
    validate_document, validate_document_against_source, Authority, CoreType, ExcludedSource,
    GeneratedDocument, Layer, LinkRelation, LinkWarning, MappingRule, ParsedSource, Sensitivity,
    SourceDocument, TypedLink,
};
use std::path::{Path, PathBuf};

fn profile_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/profile.yaml")
}

fn p3_profile_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-p3-v2/profile.yaml")
}

fn valid_document() -> GeneratedDocument {
    let profile = load_profile(&profile_path()).expect("profile loads");
    normalize_document(&profile, valid_parsed_source()).expect("source normalizes")
}

#[test]
fn exact_p3_profile_rejects_the_frozen_extensionless_identity() {
    let p3 = load_p3_profile(&p3_profile_path()).expect("P3 profile loads");
    let document = valid_document();

    let report = validate_document(&p3.config, &document);

    assert!(
        error_codes(&report).contains(&"identity.source_path_binding"),
        "{:#?}",
        report.diagnostics()
    );
}

#[test]
fn exact_p3_profile_uses_its_configured_namespace_for_markdown_identity() {
    let mut p3 = load_p3_profile(&p3_profile_path()).expect("P3 profile loads");
    p3.config.context_namespace = "ssa-alt".to_owned();
    let mut document = valid_document();
    let id = "ssa-alt/04-Research/OKF/OKF-Concept-Map.md";
    document.metadata.context_namespace = "ssa-alt".to_owned();
    document.metadata.id = id.to_owned();
    document.metadata.resource = format!("vault://{id}");
    document.metadata.source = format!("vault://{id}");

    let report = validate_document(&p3.config, &document);

    assert!(!report.has_errors(), "{:#?}", report.diagnostics());
}

fn valid_source_document() -> SourceDocument {
    let contents =
        "# Concept map\n\nThis research synthesis explains the relationship between its sources.\n";
    SourceDocument {
        source_path: "04-Research/OKF/OKF-Concept-Map.md".into(),
        bytes: contents.as_bytes().to_vec(),
        source_hash: blake3::hash(contents.as_bytes()).to_hex().to_string(),
        created: "2026-01-02".into(),
        updated: "2026-01-05".into(),
    }
}

fn valid_parsed_source() -> ParsedSource {
    parse_source_document(valid_source_document()).expect("source parses")
}

fn document_and_source(
    contents: &str,
    created: &str,
    updated: &str,
) -> (GeneratedDocument, ParsedSource) {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let source = SourceDocument {
        source_path: "04-Research/OKF/OKF-Concept-Map.md".into(),
        bytes: contents.as_bytes().to_vec(),
        source_hash: blake3::hash(contents.as_bytes()).to_hex().to_string(),
        created: created.into(),
        updated: updated.into(),
    };
    let parsed = parse_source_document(source).expect("source parses");
    let document = normalize_document(&profile, parsed.clone()).expect("source normalizes");
    (document, parsed)
}

fn error_codes(report: &sovmem::profile::ValidationReport) -> Vec<&str> {
    report
        .errors()
        .into_iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

fn warning_codes(report: &sovmem::profile::ValidationReport) -> Vec<&str> {
    report
        .warnings()
        .into_iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

fn malformed_judgment(document: &mut GeneratedDocument) {
    document.metadata.core_type = CoreType::Judgment;
    document.metadata.context_kind = "Approved Judgment".into();
    document.metadata.verdict = None;
    document.metadata.reason = Some("A human reviewed the proposal.".into());
    document.metadata.subject = Some("ssa-0/proposals/one".into());
    document
        .metadata
        .inferred_fields
        .retain(|field| field != "type");
}

#[test]
fn reports_stable_codes_for_each_hard_failure_class() {
    struct Case {
        name: &'static str,
        validate: fn(
            &sovmem::profile::ProfileConfig,
            GeneratedDocument,
        ) -> sovmem::profile::ValidationReport,
        expected_code: &'static str,
    }

    fn missing_required_field(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        document.metadata.title.clear();
        validate_document(profile, &document)
    }

    fn invalid_enum_for_known_kind(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        document.metadata.core_type = CoreType::Resource;
        validate_document(profile, &document)
    }

    fn duplicate_id(
        profile: &sovmem::profile::ProfileConfig,
        document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        let mut duplicate = document.clone();
        duplicate.metadata.source_path = "04-Research/OKF/Other.md".into();
        validate_bundle(profile, &[document, duplicate])
    }

    fn duplicate_resource(
        profile: &sovmem::profile::ProfileConfig,
        document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        let mut duplicate = document.clone();
        duplicate.metadata.id = "ssa-0/04-Research/OKF/Other".into();
        duplicate.metadata.source_path = "04-Research/OKF/Other.md".into();
        validate_bundle(profile, &[document, duplicate])
    }

    fn namespace_mismatch(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        document.metadata.context_namespace = "ssa-1".into();
        validate_document(profile, &document)
    }

    fn source_identity_mismatch(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        document.metadata.source = "https://foreign.example/document".into();
        validate_document(profile, &document)
    }

    fn namespace_root_id(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        document.metadata.id = "ssa-0/".into();
        document.metadata.resource = "vault://ssa-0/".into();
        document.metadata.source = "vault://ssa-0/".into();
        validate_document(profile, &document)
    }

    fn namespace_root_resource(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        document.metadata.resource = "vault://ssa-0/".into();
        validate_document(profile, &document)
    }

    fn namespace_root_source(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        document.metadata.source = "vault://ssa-0/".into();
        validate_document(profile, &document)
    }

    fn cross_profile_target(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        document.metadata.links.push(TypedLink {
            rel: LinkRelation::References,
            target: "VaUlT://ssa-1/foreign".into(),
        });
        validate_document(profile, &document)
    }

    fn timestamp_absence(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        document.metadata.created.clear();
        validate_document(profile, &document)
    }

    fn restricted_scope_violation(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        document.metadata.sensitivity = sovmem::profile::Sensitivity::Restricted;
        validate_document(profile, &document)
    }

    fn malformed_judgment_case(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        malformed_judgment(&mut document);
        validate_document(profile, &document)
    }

    fn empty_judgment_reason(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        malformed_judgment(&mut document);
        document.metadata.verdict = Some("ACCEPT".into());
        document.metadata.reason = Some("   ".into());
        validate_document(profile, &document)
    }

    fn invalid_verdict(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        malformed_judgment(&mut document);
        document.metadata.verdict = Some("APPROVE".into());
        validate_document(profile, &document)
    }

    fn empty_profile_scoped_subject(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        malformed_judgment(&mut document);
        document.metadata.verdict = Some("ACCEPT".into());
        document.metadata.subject = Some("ssa-0/".into());
        validate_document(profile, &document)
    }

    fn conflicting_explicit_metadata(
        profile: &sovmem::profile::ProfileConfig,
        mut document: GeneratedDocument,
    ) -> sovmem::profile::ValidationReport {
        document
            .metadata
            .extensions
            .insert("profile".into(), Value::String("foreign-profile".into()));
        validate_document(profile, &document)
    }

    let cases = [
        Case {
            name: "missing required field",
            validate: missing_required_field,
            expected_code: "document.required.title",
        },
        Case {
            name: "invalid typed enum for known context kind",
            validate: invalid_enum_for_known_kind,
            expected_code: "document.core_type_mismatch",
        },
        Case {
            name: "duplicate ID",
            validate: duplicate_id,
            expected_code: "bundle.duplicate_id",
        },
        Case {
            name: "duplicate resource",
            validate: duplicate_resource,
            expected_code: "bundle.duplicate_resource",
        },
        Case {
            name: "namespace mismatch",
            validate: namespace_mismatch,
            expected_code: "identity.context_namespace",
        },
        Case {
            name: "source identity mismatch",
            validate: source_identity_mismatch,
            expected_code: "identity.source",
        },
        Case {
            name: "namespace-root ID",
            validate: namespace_root_id,
            expected_code: "identity.id_path",
        },
        Case {
            name: "namespace-root resource",
            validate: namespace_root_resource,
            expected_code: "identity.resource_path",
        },
        Case {
            name: "namespace-root source",
            validate: namespace_root_source,
            expected_code: "identity.source_path",
        },
        Case {
            name: "cross-profile target",
            validate: cross_profile_target,
            expected_code: "links.cross_profile",
        },
        Case {
            name: "timestamp absence",
            validate: timestamp_absence,
            expected_code: "document.timestamp_missing",
        },
        Case {
            name: "restricted scope violation",
            validate: restricted_scope_violation,
            expected_code: "document.restricted_scope",
        },
        Case {
            name: "malformed judgment",
            validate: malformed_judgment_case,
            expected_code: "judgment.verdict_missing",
        },
        Case {
            name: "empty judgment reason",
            validate: empty_judgment_reason,
            expected_code: "judgment.reason_empty",
        },
        Case {
            name: "invalid verdict",
            validate: invalid_verdict,
            expected_code: "judgment.verdict_invalid",
        },
        Case {
            name: "empty profile-scoped judgment subject",
            validate: empty_profile_scoped_subject,
            expected_code: "judgment.subject_not_scoped",
        },
        Case {
            name: "conflicting explicit metadata",
            validate: conflicting_explicit_metadata,
            expected_code: "metadata.conflicting_explicit",
        },
    ];

    let profile = load_profile(&profile_path()).expect("profile loads");
    for case in cases {
        let report = (case.validate)(&profile, valid_document());
        assert!(report.has_errors(), "{} must fail", case.name);
        assert!(
            error_codes(&report).contains(&case.expected_code),
            "{} diagnostics: {:?}",
            case.name,
            error_codes(&report)
        );
    }
}

#[test]
fn rejects_noncanonical_document_identity_paths() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let invalid_ids = [
        "ssa-0/",
        "ssa-0/has space",
        "ssa-0/Research//Note",
        "ssa-0/Research/%",
        "ssa-0/Research/%2",
        "ssa-0/Research/%GG",
        "ssa-0/Research/.",
        "ssa-0/Research/%2E",
        "ssa-0/Research/..",
        "ssa-0/Research/%2e%2e",
        "ssa-0/Research/%2F",
        "ssa-0/Research/ssa-1/Note",
        "ssa-0/Research/ssa%2D1/Note",
    ];

    for id in invalid_ids {
        let mut document = valid_document();
        document.metadata.id = id.into();
        document.metadata.resource = format!("vault://{id}");
        document.metadata.source = format!("vault://{id}");

        let report = validate_document(&profile, &document);
        assert!(
            error_codes(&report).contains(&"identity.id_path"),
            "identity {id} diagnostics: {:?}",
            error_codes(&report)
        );
    }
}

#[test]
fn rejects_canonical_same_profile_identity_not_bound_to_source_path() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let mut document = valid_document();
    document.metadata.id = "ssa-0/04-Research/OKF/Rebound".into();
    document.metadata.resource = "vault://ssa-0/04-Research/OKF/Rebound".into();
    document.metadata.source = "vault://ssa-0/04-Research/OKF/Rebound".into();

    let report = validate_document(&profile, &document);
    assert!(
        error_codes(&report).contains(&"identity.source_path_binding"),
        "diagnostics: {:?}",
        error_codes(&report)
    );
}

#[test]
fn ordinary_document_validation_rejects_invalid_gregorian_dates() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let mut document = valid_document();
    document.metadata.created = "2026-02-30".into();

    let report = validate_document(&profile, &document);
    assert!(
        error_codes(&report).contains(&"document.date_invalid"),
        "diagnostics: {:?}",
        error_codes(&report)
    );
}

#[test]
fn allows_secret_like_body_only_with_restricted_private_metadata() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let mut document = valid_document();
    document.body.push_str("\ntoken: REDACTED_TEST_VALUE\n");
    document.metadata.sensitivity = Sensitivity::Restricted;
    document
        .metadata
        .extensions
        .insert("scope".into(), Value::String("private".into()));

    let report = validate_document(&profile, &document);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics());
}

#[test]
fn applies_identity_path_grammar_to_vault_resource_and_source() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let invalid_uris = [
        "VaUlT://ssa-0/",
        "VaUlT://ssa-0/has space",
        "VaUlT://ssa-0/Research//Note",
        "VaUlT://ssa-0/Research/%",
        "VaUlT://ssa-0/Research/%2e%2e",
        "VaUlT://ssa-0/Research/ssa%2D1/Note",
    ];

    for uri in invalid_uris {
        let mut resource_document = valid_document();
        resource_document.metadata.resource = uri.into();
        let resource_report = validate_document(&profile, &resource_document);
        assert!(
            error_codes(&resource_report).contains(&"identity.resource_path"),
            "resource {uri} diagnostics: {:?}",
            error_codes(&resource_report)
        );

        let mut source_document = valid_document();
        source_document.metadata.source = uri.into();
        let source_report = validate_document(&profile, &source_document);
        assert!(
            error_codes(&source_report).contains(&"identity.source_path"),
            "source {uri} diagnostics: {:?}",
            error_codes(&source_report)
        );
    }
}

#[test]
fn accepts_canonical_percent_encoded_document_identity_paths() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let mut document = valid_document();
    document.metadata.source_path = "Research/Audy Himura/é.md".into();
    document.metadata.id = "ssa-0/Research/Audy%20Himura/%C3%A9".into();
    document.metadata.resource = "vault://ssa-0/Research/Audy%20Himura/%C3%A9".into();
    document.metadata.source = "vault://ssa-0/Research/Audy%20Himura/%C3%A9".into();

    let report = validate_document(&profile, &document);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics());
}

#[test]
fn rejects_noncanonical_percent_encoding_in_document_identity_paths() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let invalid_ids = [
        "ssa-0/Research/%41udy",
        "ssa-0/Research/Audy%7E",
        "ssa-0/Research/Audy%3f",
        "ssa-0/Research/Audy%c3%a9",
    ];

    for id in invalid_ids {
        let mut document = valid_document();
        document.metadata.id = id.into();
        document.metadata.resource = format!("vault://{id}");
        document.metadata.source = format!("vault://{id}");

        let report = validate_document(&profile, &document);
        assert!(
            error_codes(&report).contains(&"identity.id_path"),
            "identity {id} diagnostics: {:?}",
            error_codes(&report)
        );
    }
}

#[test]
fn rejects_noncanonical_percent_encoding_across_profile_scoped_consumers() {
    let profile = load_profile(&profile_path()).expect("profile loads");

    let mut resource_document = valid_document();
    resource_document.metadata.resource = "vault://ssa-0/Research/%41udy".into();
    let resource_report = validate_document(&profile, &resource_document);
    assert!(
        error_codes(&resource_report).contains(&"identity.resource_path"),
        "resource diagnostics: {:?}",
        error_codes(&resource_report)
    );

    let mut source_document = valid_document();
    source_document.metadata.source = "vault://ssa-0/Research/Audy%7E".into();
    let source_report = validate_document(&profile, &source_document);
    assert!(
        error_codes(&source_report).contains(&"identity.source_path"),
        "source diagnostics: {:?}",
        error_codes(&source_report)
    );

    let mut judgment = valid_document();
    malformed_judgment(&mut judgment);
    judgment.metadata.verdict = Some("ACCEPT".into());
    judgment.metadata.subject = Some("ssa-0/proposals/%41udy".into());
    let judgment_report = validate_document(&profile, &judgment);
    assert!(
        error_codes(&judgment_report).contains(&"judgment.subject_not_scoped"),
        "judgment diagnostics: {:?}",
        error_codes(&judgment_report)
    );

    for target in [
        "ssa%2D0/Research/Note",
        "VaUlT://ssa%2D0/Research/Note",
        "Research/Audy%3f",
    ] {
        let mut document = valid_document();
        document.metadata.links.push(TypedLink {
            rel: LinkRelation::References,
            target: target.into(),
        });
        let report = validate_document(&profile, &document);
        assert!(
            error_codes(&report).contains(&"links.target_invalid"),
            "link {target} diagnostics: {:?}",
            error_codes(&report)
        );
    }
}

#[test]
fn accepts_generator_canonical_percent_encoding() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let mut document = valid_document();
    let id = "ssa-0/AZaz09-._~/Audy%20Himura/%3F/%25/%C3%A9";
    document.metadata.source_path = "AZaz09-._~/Audy Himura/?/%/é.md".into();
    document.metadata.id = id.into();
    document.metadata.resource = format!("vault://{id}");
    document.metadata.source = format!("vault://{id}");
    document.metadata.links.push(TypedLink {
        rel: LinkRelation::References,
        target: "VaUlT://ssa-0/AZaz09-._~/Audy%20Himura/%3F/%25/%C3%A9".into(),
    });

    let report = validate_document(&profile, &document);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics());
}

#[test]
fn allows_a_complete_explicit_human_judgment_with_a_profile_scoped_subject() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let mut document = valid_document();
    malformed_judgment(&mut document);
    document.metadata.verdict = Some("ACCEPT".into());
    document.metadata.reason = Some("A human reviewed the proposal and accepted it.".into());
    document
        .metadata
        .inferred_fields
        .retain(|field| !matches!(field.as_str(), "type" | "verdict" | "reason" | "subject"));

    let report = validate_document(&profile, &document);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics());
}

#[test]
fn permits_only_the_exact_p3_human_judgment_mapping() {
    let mut document = valid_document();
    malformed_judgment(&mut document);
    document.metadata.context_kind = "Human Judgment".into();
    document.metadata.verdict = Some("ACCEPT".into());
    document.metadata.reason = Some("A human reviewed the proposal and accepted it.".into());
    document
        .metadata
        .inferred_fields
        .retain(|field| !matches!(field.as_str(), "type" | "verdict" | "reason" | "subject"));

    let p3 = load_p3_profile(&p3_profile_path()).expect("P3 profile loads");
    let p3_id = format!("{}.md", document.metadata.id);
    document.metadata.id = p3_id.clone();
    document.metadata.resource = format!("vault://{p3_id}");
    document.metadata.source = format!("vault://{p3_id}");
    let p3_report = validate_document(&p3.config, &document);
    assert!(!p3_report.has_errors(), "{:#?}", p3_report.diagnostics());

    let mut unreviewed = load_profile(&profile_path()).expect("P2 profile loads");
    unreviewed
        .context_kinds
        .insert("Human Judgment".into(), CoreType::Judgment);
    unreviewed.mapping_rules.insert(
        0,
        MappingRule {
            pattern: "judgments/*.md".into(),
            core_type: CoreType::Judgment,
            context_kind: "Human Judgment".into(),
            layer: Layer::Concept,
            authority: Authority::Canonical,
        },
    );
    let report = validate_document(&unreviewed, &document);
    assert!(
        error_codes(&report).contains(&"judgment.mapping_forbidden"),
        "{:#?}",
        report.diagnostics()
    );
}

#[test]
fn rejects_noncanonical_profile_scoped_judgment_subjects() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let invalid_subjects = [
        "   ",
        "ssa-0",
        "ssa-0/",
        "ssa-0/proposals//one",
        "ssa-0/proposals/../one",
        "ssa-0/proposals/%2E%2E/one",
        "ssa-0/proposals/%",
        "ssa-0/proposals/%GG",
        "ssa-0/proposals/ssa-1/foreign",
        "ssa-0/proposals/ssa%2D1/foreign",
    ];

    for subject in invalid_subjects {
        let mut document = valid_document();
        malformed_judgment(&mut document);
        document.metadata.verdict = Some("ACCEPT".into());
        document.metadata.subject = Some(subject.into());

        let report = validate_document(&profile, &document);
        assert!(
            error_codes(&report).contains(&"judgment.subject_not_scoped"),
            "subject {subject:?} diagnostics: {:?}",
            error_codes(&report)
        );
    }
}

#[test]
fn accepts_canonical_percent_encoded_judgment_subject() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let mut document = valid_document();
    malformed_judgment(&mut document);
    document.metadata.verdict = Some("ACCEPT".into());
    document.metadata.subject = Some("ssa-0/proposals/Audy%20Himura".into());

    let report = validate_document(&profile, &document);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics());
}

#[test]
fn rejects_cross_profile_links_hidden_by_traversal_or_percent_encoding() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let targets = [
        "../ssa-1/foreign",
        "vault://ssa-0/../ssa-1/foreign",
        "%2E%2E/ssa%2D1/foreign",
        "vault://ssa-0/%2e%2e/ssa%2D1/foreign",
        "Research/ssa%2D1/foreign#ignored-fragment",
    ];

    for target in targets {
        let mut document = valid_document();
        document.metadata.links.push(TypedLink {
            rel: LinkRelation::References,
            target: target.into(),
        });

        let report = validate_document(&profile, &document);
        assert!(
            error_codes(&report).contains(&"links.cross_profile"),
            "link {target} diagnostics: {:?}",
            error_codes(&report)
        );
    }
}

#[test]
fn rejects_noncanonical_internal_link_targets() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let targets = [
        "../local",
        "Research//Note",
        "Research/./Note",
        "Research/%2E%2E/Note",
        "Research/%",
        "Research/%GG",
        "vault://ssa-0/",
        "ssa-0",
        "ssa-0/",
    ];

    for target in targets {
        let mut document = valid_document();
        document.metadata.links.push(TypedLink {
            rel: LinkRelation::References,
            target: target.into(),
        });

        let report = validate_document(&profile, &document);
        assert!(
            error_codes(&report).contains(&"links.target_invalid"),
            "link {target} diagnostics: {:?}",
            error_codes(&report)
        );
    }
}

#[test]
fn allows_external_and_canonical_profile_vault_and_relative_links() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let mut document = valid_document();
    for target in [
        "https://example.test/../ssa-1/foreign",
        "HtTp://example.test/%2E%2E/ssa%2D1/foreign",
        "ssa-0/Research/Audy%20Himura#fragment",
        "VaUlT://ssa-0/Research/Audy%20Himura#fragment",
        "Research/Audy%20Himura#fragment",
    ] {
        document.metadata.links.push(TypedLink {
            rel: LinkRelation::References,
            target: target.into(),
        });
    }

    let report = validate_document(&profile, &document);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics());
}

#[test]
fn warns_without_invalidating_unknown_context_and_inferred_research_metadata() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let mut document = valid_document();
    document.metadata.context_kind = "Unapproved Research Kind".into();

    let report = validate_document(&profile, &document);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics());
    let warnings = warning_codes(&report);
    assert!(warnings.contains(&"document.context_kind_unknown"));
    assert!(warnings.contains(&"document.inferred_description"));
    assert!(warnings.contains(&"document.inferred_provenance"));
    assert!(warnings.contains(&"research.weak_citation"));
}

#[test]
fn carries_prior_link_and_source_exclusion_diagnostics_into_a_sorted_report() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let mut report = validate_bundle(&profile, &[valid_document()]);
    report.extend_link_warnings(&[
        LinkWarning {
            code: "links.broken".into(),
            source_path: "04-Research/OKF/Missing.md".into(),
            target: "Missing".into(),
        },
        LinkWarning {
            code: "links.ambiguous".into(),
            source_path: "04-Research/OKF/Ambiguous.md".into(),
            target: "README".into(),
        },
    ]);
    report.extend_source_exclusions(&[
        ExcludedSource {
            source_path: "04-Research/OKF/Dirty.md".into(),
            reason: "dirty".into(),
        },
        ExcludedSource {
            source_path: "04-Research/OKF/New.md".into(),
            reason: "untracked".into(),
        },
    ]);

    let warnings = warning_codes(&report);
    assert!(warnings.contains(&"links.broken"));
    assert!(warnings.contains(&"links.ambiguous"));
    assert!(warnings.contains(&"source.excluded_dirty"));
    assert!(warnings.contains(&"source.excluded_untracked"));
    assert!(report
        .diagnostics()
        .windows(2)
        .all(|pair| pair[0].severity <= pair[1].severity
            && (pair[0].severity != pair[1].severity
                || (
                    pair[0].code.clone(),
                    pair[0].source_path.clone(),
                    pair[0].message.clone()
                ) <= (
                    pair[1].code.clone(),
                    pair[1].source_path.clone(),
                    pair[1].message.clone()
                ))));
}

#[test]
fn validation_is_side_effect_free() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let document = valid_document();
    let original = document.clone();

    let _ = validate_bundle(&profile, std::slice::from_ref(&document));
    assert_eq!(document, original);
}

#[test]
fn source_evidence_validation_uses_explicit_dates_then_git_dates_and_rejects_tampering() {
    struct Case {
        name: &'static str,
        mutate: fn(&mut GeneratedDocument, &mut ParsedSource),
        expected_code: &'static str,
    }

    fn invalid_date(document: &mut GeneratedDocument, _: &mut ParsedSource) {
        document.metadata.created = "January 2, 2026".into();
    }

    fn inferred_date(document: &mut GeneratedDocument, _: &mut ParsedSource) {
        document.metadata.inferred_fields.push("updated".into());
    }

    fn fabricated_git_date(document: &mut GeneratedDocument, _: &mut ParsedSource) {
        document.metadata.updated = "2026-01-06".into();
    }

    fn source_path_mismatch(_: &mut GeneratedDocument, source: &mut ParsedSource) {
        source.source_path = "04-Research/OKF/Other.md".into();
    }

    fn source_hash_mismatch(_: &mut GeneratedDocument, source: &mut ParsedSource) {
        source.source_hash = "not-the-generated-hash".into();
    }

    let cases = [
        Case {
            name: "non-date string",
            mutate: invalid_date,
            expected_code: "document.date_invalid",
        },
        Case {
            name: "inferred timestamp",
            mutate: inferred_date,
            expected_code: "document.timestamp_inferred",
        },
        Case {
            name: "fabricated Git date",
            mutate: fabricated_git_date,
            expected_code: "source.date_mismatch",
        },
        Case {
            name: "source path mismatch",
            mutate: source_path_mismatch,
            expected_code: "source.path_mismatch",
        },
        Case {
            name: "source hash mismatch",
            mutate: source_hash_mismatch,
            expected_code: "source.hash_mismatch",
        },
    ];

    for case in cases {
        let (mut document, mut source) = document_and_source(
            "# Concept map\n\nThis research synthesis explains its sources.\n",
            "2026-01-02",
            "2026-01-05",
        );
        (case.mutate)(&mut document, &mut source);
        let report = validate_document_against_source(&document, &source);
        assert!(report.has_errors(), "{} must fail", case.name);
        assert!(
            error_codes(&report).contains(&case.expected_code),
            "{} diagnostics: {:?}",
            case.name,
            error_codes(&report)
        );
    }

    let (mut document, source) = document_and_source(
        "---\ncreated: 2020-02-03\nupdated: 2020-02-04\n---\n# Concept map\n\nThis research synthesis explains its sources.\n",
        "2026-01-02",
        "2026-01-05",
    );
    assert!(!validate_document_against_source(&document, &source).has_errors());
    document.metadata.updated = "2026-01-05".into();
    let report = validate_document_against_source(&document, &source);
    assert!(error_codes(&report).contains(&"source.date_mismatch"));
}

#[test]
fn rejects_blank_explicit_source_date_without_git_fallback() {
    let (document, mut source) = document_and_source(
        "# Concept map\n\nThis research synthesis explains its sources.\n",
        "2026-01-02",
        "2026-01-05",
    );
    source
        .metadata
        .insert("created".into(), Value::String("   ".into()));

    let report = validate_document_against_source(&document, &source);
    assert!(
        error_codes(&report).contains(&"document.date_invalid"),
        "blank explicit date diagnostics: {:?}",
        error_codes(&report)
    );
}

#[test]
fn rejects_non_string_explicit_source_date_without_git_fallback() {
    let (document, mut source) = document_and_source(
        "# Concept map\n\nThis research synthesis explains its sources.\n",
        "2026-01-02",
        "2026-01-05",
    );
    source.metadata.insert("created".into(), Value::Bool(true));

    let report = validate_document_against_source(&document, &source);
    assert!(
        error_codes(&report).contains(&"document.date_invalid"),
        "non-string explicit date diagnostics: {:?}",
        error_codes(&report)
    );
}

#[test]
fn rejects_malformed_and_calendar_invalid_explicit_source_dates() {
    for date in ["not-a-date", "2026-02-30"] {
        let (document, mut source) = document_and_source(
            "# Concept map\n\nThis research synthesis explains its sources.\n",
            "2026-01-02",
            "2026-01-05",
        );
        source
            .metadata
            .insert("created".into(), Value::String(date.into()));

        let report = validate_document_against_source(&document, &source);
        assert!(
            error_codes(&report).contains(&"document.date_invalid"),
            "explicit date {date} diagnostics: {:?}",
            error_codes(&report)
        );
    }
}

#[test]
fn uses_git_source_date_only_when_explicit_key_is_absent() {
    let (document, source) = document_and_source(
        "# Concept map\n\nThis research synthesis explains its sources.\n",
        "2026-01-02",
        "2026-01-05",
    );
    assert!(!source.metadata.contains_key("created"));
    assert!(!source.metadata.contains_key("updated"));

    let report = validate_document_against_source(&document, &source);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics());
}

#[test]
fn rejects_invalid_gregorian_generated_dates() {
    for date in ["2026-13-40", "2026-02-30", "2025-02-29"] {
        let (mut document, source) = document_and_source(
            "# Concept map\n\nThis research synthesis explains its sources.\n",
            "2026-01-02",
            "2026-01-05",
        );

        document.metadata.created = date.into();
        let report = validate_document_against_source(&document, &source);
        assert!(
            error_codes(&report).contains(&"document.date_invalid"),
            "generated date {date} diagnostics: {:?}",
            error_codes(&report)
        );
    }
}

#[test]
fn rejects_invalid_gregorian_source_evidence() {
    for date in ["2026-13-40", "2026-02-30", "2025-02-29"] {
        let (document, mut source) = document_and_source(
            "# Concept map\n\nThis research synthesis explains its sources.\n",
            "2026-01-02",
            "2026-01-05",
        );
        source.created = date.into();
        let report = validate_document_against_source(&document, &source);
        assert!(
            error_codes(&report).contains(&"document.date_invalid"),
            "source date {date} diagnostics: {:?}",
            error_codes(&report)
        );
    }
}

#[test]
fn rejects_invalid_gregorian_dates_even_when_generated_equals_source() {
    for date in ["2026-13-40", "2026-02-30", "2025-02-29"] {
        let (document, source) = document_and_source(
            "# Concept map\n\nThis research synthesis explains its sources.\n",
            date,
            "2026-01-05",
        );
        let report = validate_document_against_source(&document, &source);
        assert!(
            error_codes(&report).contains(&"document.date_invalid"),
            "equal invalid date {date} diagnostics: {:?}",
            error_codes(&report)
        );
    }
}

#[test]
fn accepts_valid_gregorian_leap_day() {
    let (document, source) = document_and_source(
        "# Concept map\n\nThis research synthesis explains its sources.\n",
        "2024-02-29",
        "2026-01-05",
    );

    let report = validate_document_against_source(&document, &source);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics());
}
