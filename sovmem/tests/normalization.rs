use serde_yaml::Value;
use sovmem::profile::{
    load_profile, normalize_document, parse_source_document, render_document, Authority, CoreType,
    Layer, LinkRelation, Provenance, Sensitivity, SourceDocument, SourceKind, Status, TypedLink,
};
use std::path::{Path, PathBuf};

fn profile_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/profile.yaml")
}

fn source(path: &str, contents: &str) -> SourceDocument {
    source_with_dates(path, contents, "2026-01-02", "2026-01-05")
}

fn source_with_dates(path: &str, contents: &str, created: &str, updated: &str) -> SourceDocument {
    SourceDocument {
        source_path: path.into(),
        bytes: contents.as_bytes().to_vec(),
        source_hash: blake3::hash(contents.as_bytes()).to_hex().to_string(),
        created: created.into(),
        updated: updated.into(),
    }
}

fn normalize(path: &str, contents: &str) -> sovmem::profile::GeneratedDocument {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let parsed = parse_source_document(source(path, contents)).expect("source parses");
    normalize_document(&profile, parsed).expect("source normalizes")
}

#[test]
fn normalizes_the_approved_concept_map_example() {
    let generated = normalize(
        "04-Research/OKF/OKF-Concept-Map.md",
        "# OKF Concept Map\n\nThis **concept** links [OKF](https://okf.example). A later sentence.\n",
    );

    assert_eq!(
        generated.metadata.id,
        "ssa-0/04-Research/OKF/OKF-Concept-Map"
    );
    assert_eq!(
        generated.metadata.resource,
        "vault://ssa-0/04-Research/OKF/OKF-Concept-Map"
    );
    assert_eq!(generated.metadata.source, generated.metadata.resource);
    assert_eq!(generated.metadata.core_type, CoreType::Concept);
    assert_eq!(generated.metadata.context_kind, "Concept Map");
    assert_eq!(generated.metadata.layer, Layer::Analysis);
    assert_eq!(generated.metadata.authority, Authority::Working);
    assert_eq!(generated.metadata.status, Status::Draft);
    assert_eq!(generated.metadata.title, "OKF Concept Map");
    assert_eq!(generated.metadata.description, "This concept links OKF.");
    assert_eq!(generated.metadata.sensitivity, Sensitivity::Internal);
    assert_eq!(
        generated.metadata.tags,
        ["audy", "concept-map", "okf", "ssa", "tfis"]
    );
    assert!(generated.metadata.links.is_empty());
    assert_eq!(
        generated.metadata.inferred_fields,
        [
            "type",
            "context_kind",
            "title",
            "description",
            "layer",
            "source_kind",
            "provenance",
            "status",
            "authority",
            "sensitivity",
            "tags",
            "links",
        ]
    );
}

#[test]
fn explicit_valid_metadata_overrides_mapping_defaults() {
    let generated = normalize(
        "04-Research/OKF/OKF-Concept-Map.md",
        r#"---
type: concept
context_kind: Doctrine
title: Explicit title
description: Explicit description.
layer: synthesis
source_kind: vault-note
provenance: secondary
status: archived
authority: canonical
sensitivity: internal
created: 2025-12-01
updated: 2025-12-03
tags: [TFIS, custom, tfis, audy, ssa, doctrine, okf]
links:
  - { rel: references, target: ssa-0/01-Soul/Doctrine }
  - { rel: cites, target: https://example.com }
---
# Ignored H1

Ignored inferred description.
"#,
    );

    assert_eq!(generated.metadata.core_type, CoreType::Concept);
    assert_eq!(generated.metadata.context_kind, "Doctrine");
    assert_eq!(generated.metadata.title, "Explicit title");
    assert_eq!(generated.metadata.description, "Explicit description.");
    assert_eq!(generated.metadata.layer, Layer::Synthesis);
    assert_eq!(generated.metadata.source_kind, SourceKind::VaultNote);
    assert_eq!(generated.metadata.provenance, Provenance::Secondary);
    assert_eq!(generated.metadata.status, Status::Archived);
    assert_eq!(generated.metadata.authority, Authority::Canonical);
    assert_eq!(generated.metadata.created, "2025-12-01");
    assert_eq!(generated.metadata.updated, "2025-12-03");
    assert_eq!(
        generated.metadata.tags,
        ["audy", "custom", "doctrine", "okf", "ssa", "tfis"]
    );
    assert_eq!(
        generated.metadata.links,
        [
            TypedLink {
                rel: LinkRelation::Cites,
                target: "https://example.com".into(),
            },
            TypedLink {
                rel: LinkRelation::References,
                target: "ssa-0/01-Soul/Doctrine".into(),
            },
        ]
    );
    assert!(generated.metadata.inferred_fields.is_empty());
}

#[test]
fn explicit_source_must_equal_the_profile_scoped_vault_uri() {
    let expected = "vault://ssa-0/04-Research/OKF/OKF-Concept-Map";
    let equal = normalize(
        "04-Research/OKF/OKF-Concept-Map.md",
        &format!("---\nsource: {expected}\n---\n# Map\n\nBody.\n"),
    );
    assert_eq!(equal.metadata.source, expected);

    let profile = load_profile(&profile_path()).expect("profile loads");
    let divergent = parse_source_document(source(
        "04-Research/OKF/OKF-Concept-Map.md",
        "---\nsource: https://example.com/other\n---\n# Map\n\nBody.\n",
    ))
    .expect("source parses");
    let error = normalize_document(&profile, divergent).expect_err("divergent source must fail");
    assert_eq!(error.code(), "normalize.identity_conflict");
}

#[test]
fn conflicting_explicit_profile_identity_fails() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let conflicts = [
        ("ssa_id", "ssa-1"),
        ("ssa_profile", "another-profile"),
        ("familiar_id", "another-familiar"),
        ("context_namespace", "ssa-1"),
        ("profile", "another-schema-profile"),
        ("nous_schema", "nous-ssa-context-v1"),
    ];

    for (field, value) in conflicts {
        let contents = format!("---\n{field}: {value}\n---\n# Note\n\nBody.\n");
        let parsed = parse_source_document(source("01-Soul/Note.md", &contents)).unwrap();
        let error = normalize_document(&profile, parsed).expect_err(field);
        assert_eq!(error.code(), "normalize.identity_conflict", "{field}");
    }
}

#[test]
fn falls_back_to_the_filename_and_percent_encodes_each_path_segment() {
    let generated = normalize(
        "01-Soul/Über 100% Ready.md",
        "A plain opening sentence. Another sentence.\n",
    );

    assert_eq!(generated.metadata.title, "Über 100% Ready");
    assert_eq!(
        generated.metadata.id,
        "ssa-0/01-Soul/%C3%9Cber%20100%25%20Ready"
    );
    assert_eq!(
        generated.metadata.resource,
        "vault://ssa-0/01-Soul/%C3%9Cber%20100%25%20Ready"
    );
}

#[test]
fn strips_markdown_from_descriptions_and_caps_at_240_characters() {
    let long = format!(
        "# Long\n\n{}\n",
        "Read [the source](https://example.com), `code`, and **bold words** ".repeat(12)
    );
    let generated = normalize("01-Soul/Long.md", &long);

    assert_eq!(generated.metadata.description.chars().count(), 240);
    assert!(generated
        .metadata
        .description
        .starts_with("Read the source, code, and bold words"));
    assert!(!generated
        .metadata
        .description
        .contains(['[', ']', '`', '*']));
}

#[test]
fn markdown_description_strips_block_list_and_table_syntax() {
    let cases = [
        (
            "# Quote\n\n> **Quoted** text.\n",
            "Quoted text.",
        ),
        (
            "# Compact quote\n\n>Quoted without a space.\n",
            "Quoted without a space.",
        ),
        ("# Bullet\n\n- Listed item.\n", "Listed item."),
        ("# Ordered\n\n1. Ordered item.\n", "Ordered item."),
        (
            "# Table\n\n| **Name** | [Value](https://example.com) |\n| --- | --- |\n| Alpha | Beta |\n",
            "Name Value Alpha Beta",
        ),
        (
            "# Compact table\n\n**Name** | [Value](https://example.com)\n--- | ---\nAlpha | Beta\n",
            "Name Value Alpha Beta",
        ),
    ];
    let actual: Vec<String> = cases
        .iter()
        .map(|(body, _)| normalize("01-Soul/Markdown.md", body).metadata.description)
        .collect();
    let expected: Vec<&str> = cases.iter().map(|(_, expected)| *expected).collect();
    assert_eq!(actual, expected);
}

#[test]
fn markdown_title_removes_closing_atx_hashes() {
    let generated = normalize(
        "01-Soul/Heading.md",
        "# **Operational** Contract ###\n\nBody.\n",
    );

    assert_eq!(generated.metadata.title, "Operational Contract");
}

#[test]
fn markdown_description_preserves_literal_comparison_characters() {
    let generated = normalize(
        "01-Soul/Comparison.md",
        "# Comparison\n\nUse x < y and z > 0. A later sentence.\n",
    );

    assert_eq!(generated.metadata.description, "Use x < y and z > 0.");
}

#[test]
fn uses_explicit_dates_then_git_dates_and_has_no_clock_fallback() {
    let explicit = normalize(
        "01-Soul/Dated.md",
        "---\ncreated: 2020-02-03\nupdated: 2020-02-04\n---\n# Dated\n\nBody.\n",
    );
    assert_eq!(explicit.metadata.created, "2020-02-03");
    assert_eq!(explicit.metadata.updated, "2020-02-04");

    let git = normalize("01-Soul/Git-Dated.md", "# Git dated\n\nBody.\n");
    assert_eq!(git.metadata.created, "2026-01-02");
    assert_eq!(git.metadata.updated, "2026-01-05");

    let profile = load_profile(&profile_path()).expect("profile loads");
    let parsed = parse_source_document(source_with_dates(
        "01-Soul/No-Date.md",
        "# No date\n\nBody.\n",
        "",
        "",
    ))
    .unwrap();
    let error = normalize_document(&profile, parsed).expect_err("missing dates must fail");
    assert_eq!(error.code(), "normalize.date_missing");
}

#[test]
fn maps_source_snapshots_to_resources_with_unknown_provenance() {
    let generated = normalize(
        "04-Research/OKF/source-pages/Paper.md",
        "# Preserved paper\n\nA preserved source page.\n",
    );

    assert_eq!(generated.metadata.core_type, CoreType::Resource);
    assert_eq!(generated.metadata.context_kind, "Source Snapshot");
    assert_eq!(generated.metadata.source_kind, SourceKind::SourceSnapshot);
    assert_eq!(generated.metadata.provenance, Provenance::Unknown);
    assert_eq!(generated.metadata.authority, Authority::Raw);
    assert_eq!(generated.metadata.status, Status::Draft);
}

#[test]
fn adds_only_the_approved_harness_path_tag() {
    let generated = normalize(
        "08-Hermes/Lane/Note.md",
        "# Harness note\n\nA scoped harness note.\n",
    );

    assert_eq!(
        generated.metadata.tags,
        ["audy", "harness-lane", "hermes", "ssa", "tfis"]
    );
}

#[test]
fn rejects_unmapped_paths_and_does_not_infer_judgments_from_prose() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let parsed = parse_source_document(source(
        "Unmapped/Note.md",
        "# Note\n\nThis is a verdict. ACCEPT because it is correct.\n",
    ))
    .unwrap();
    let error = normalize_document(&profile, parsed).expect_err("unmapped path must fail");
    assert_eq!(error.code(), "normalize.mapping_missing");

    let verdict_like = normalize(
        "05-Daily/2026-01-02.md",
        "# Court output\n\nVerdict: ACCEPT. Reason: looks correct.\n",
    );
    assert_eq!(verdict_like.metadata.core_type, CoreType::Concept);
    assert_eq!(verdict_like.metadata.context_kind, "Daily Log");
}

#[test]
fn accepts_an_explicit_unknown_context_kind_with_a_valid_core_type() {
    let generated = normalize(
        "04-Research/OKF/OKF-Concept-Map.md",
        "---\ntype: concept\ncontext_kind: Experimental Synthesis\n---\n# Experiment\n\nBody.\n",
    );

    assert_eq!(generated.metadata.core_type, CoreType::Concept);
    assert_eq!(generated.metadata.context_kind, "Experimental Synthesis");
}

#[test]
fn preserves_unknown_extensions_and_renders_stably() {
    let generated = normalize(
        "01-Soul/Extended.md",
        r#"---
custom_scalar: retained
x_nested:
  enabled: true
  count: 2
---
# Extended

Extension body.
"#,
    );

    assert_eq!(
        generated.metadata.extensions.get("custom_scalar"),
        Some(&Value::String("retained".into()))
    );
    let first = render_document(&generated).expect("document renders");
    let second = render_document(&generated).expect("document renders deterministically");
    assert_eq!(first, second);

    let reparsed = parse_source_document(SourceDocument {
        source_path: "01-Soul/Extended.md".into(),
        source_hash: blake3::hash(&first).to_hex().to_string(),
        bytes: first,
        created: "2026-01-02".into(),
        updated: "2026-01-05".into(),
    })
    .expect("rendered frontmatter reparses");
    assert_eq!(
        reparsed.metadata.get("custom_scalar"),
        Some(&Value::String("retained".into()))
    );
    assert_eq!(
        reparsed.metadata.get("x_nested"),
        generated.metadata.extensions.get("x_nested")
    );
}

#[test]
fn detects_only_conservative_secret_patterns_and_honors_public_allowlisting() {
    for body in [
        "# Secret\n\n  API_KEY: value\n",
        "# Secret\n\n\ttOkEn: value\n",
        "# Secret\n\n-----BEGIN PRIVATE KEY-----\n",
    ] {
        let generated = normalize("01-Soul/Secret.md", body);
        assert_eq!(generated.metadata.sensitivity, Sensitivity::Restricted);
    }

    let prose = normalize(
        "04-Research/OKF/README.md",
        "# Security research\n\nThis prose discusses token, password, secret, and API key handling.\n",
    );
    assert_eq!(prose.metadata.sensitivity, Sensitivity::Internal);

    let mut profile = load_profile(&profile_path()).expect("profile loads");
    profile
        .public_allowlist
        .push("04-Research/OKF/README.md".into());
    let parsed = parse_source_document(source(
        "04-Research/OKF/README.md",
        "---\nsensitivity: public\n---\n# Public research\n\nApproved public prose.\n",
    ))
    .unwrap();
    let generated =
        normalize_document(&profile, parsed).expect("allowlisted public note normalizes");
    assert_eq!(generated.metadata.sensitivity, Sensitivity::Public);

    let parsed = parse_source_document(source(
        "04-Research/Other/README.md",
        "---\nsensitivity: public\n---\n# Public research\n\nUnapproved public prose.\n",
    ))
    .unwrap();
    let error = normalize_document(&profile, parsed).expect_err("public must be allowlisted");
    assert_eq!(error.code(), "normalize.public_not_allowlisted");
}

#[test]
fn detects_encrypted_and_dsa_pem_private_key_headers() {
    for header in [
        "-----BEGIN ENCRYPTED PRIVATE KEY-----",
        "-----BEGIN DSA PRIVATE KEY-----",
    ] {
        let generated = normalize(
            "01-Soul/Secret.md",
            &format!("# Secret\n\nprefix {header} suffix\n"),
        );
        assert_eq!(
            generated.metadata.sensitivity,
            Sensitivity::Restricted,
            "{header}"
        );
    }
}
