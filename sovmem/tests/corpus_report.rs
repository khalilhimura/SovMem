use serde_json::to_string;
use sovmem::profile::{
    evaluate_corpus as evaluate_corpus_with_profile, load_corpus, load_profile, normalize_document,
    parse_source_document, CorpusConfig, CorpusGateError, CorpusReportDraft, CorpusTheme,
    Diagnostic, ExcludedSource, GeneratedDocument, Severity, SourceDocument,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const BODY_SENTINEL: &str = "NOTE-BODY-MUST-NOT-LEAK-7fca71";

fn corpus_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml")
}

fn profile_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/profile.yaml")
}

fn documents_for(config: &CorpusConfig) -> Vec<GeneratedDocument> {
    let profile = load_profile(&profile_path()).expect("profile loads");
    config
        .selected_paths()
        .into_iter()
        .map(|source_path| {
            let bytes = format!("# Fixture\n\n{BODY_SENTINEL}\n").into_bytes();
            let source = SourceDocument {
                source_path,
                source_hash: blake3::hash(&bytes).to_hex().to_string(),
                bytes,
                created: "2026-01-02".into(),
                updated: "2026-01-05".into(),
            };
            let parsed = parse_source_document(source).expect("fixture parses");
            normalize_document(&profile, parsed).expect("fixture normalizes")
        })
        .collect()
}

fn evaluate_corpus(
    config: &CorpusConfig,
    documents: &[GeneratedDocument],
    exclusions: &[ExcludedSource],
    diagnostics: &[Diagnostic],
) -> Result<CorpusReportDraft, CorpusGateError> {
    let profile = load_profile(&profile_path()).expect("profile loads");
    evaluate_corpus_with_profile(&profile, config, documents, exclusions, diagnostics)
}

fn exclude(documents: &mut Vec<GeneratedDocument>, paths: &[String]) -> Vec<ExcludedSource> {
    documents.retain(|document| !paths.contains(&document.metadata.source_path));
    paths
        .iter()
        .enumerate()
        .map(|(index, source_path)| ExcludedSource {
            source_path: source_path.clone(),
            reason: if index % 2 == 0 { "dirty" } else { "untracked" }.into(),
        })
        .collect()
}

fn research_buffers(config: &CorpusConfig) -> BTreeMap<CorpusTheme, Vec<String>> {
    let mut by_theme = BTreeMap::<CorpusTheme, Vec<String>>::new();
    for entry in &config.research {
        by_theme
            .entry(entry.theme.clone())
            .or_default()
            .push(entry.path.clone());
    }
    for (theme, paths) in &mut by_theme {
        paths.truncate(paths.len() - theme.minimum_accepted());
    }
    by_theme
}

fn all_buffer_paths(config: &CorpusConfig) -> Vec<String> {
    research_buffers(config).into_values().flatten().collect()
}

#[test]
fn approved_composition_passes_with_deterministic_counts() {
    let config = load_corpus(&corpus_path()).unwrap();
    let documents = documents_for(&config);
    let draft = evaluate_corpus(&config, &documents, &[], &[]).unwrap();
    let report = draft.finalize("a".repeat(64));
    assert_eq!(report.counts.candidates, 63);
    assert_eq!(report.counts.core, 8);
    assert_eq!(report.counts.research, 55);
    assert_eq!(report.research_by_theme[&CorpusTheme::TfisSsa], 12);
    assert!(report.gates_passed);
}

#[test]
fn missing_or_excluded_core_fails_closed() {
    let config = load_corpus(&corpus_path()).unwrap();
    for excluded in [false, true] {
        let mut documents = documents_for(&config);
        let core_path = config.core[0].path.clone();
        documents.retain(|document| document.metadata.source_path != core_path);
        let exclusions = excluded.then(|| ExcludedSource {
            source_path: core_path,
            reason: "dirty".into(),
        });
        let error = evaluate_corpus(&config, &documents, exclusions.as_slice(), &[])
            .expect_err("core absence must fail");
        assert_eq!(error.code(), "corpus.core_missing");
    }
}

#[test]
fn ten_research_exclusions_pass_at_exact_quotas() {
    let config = load_corpus(&corpus_path()).unwrap();
    let mut documents = documents_for(&config);
    let paths = all_buffer_paths(&config);
    assert_eq!(paths.len(), 10);
    let exclusions = exclude(&mut documents, &paths);

    let report = evaluate_corpus(&config, &documents, &exclusions, &[])
        .unwrap()
        .finalize("b".repeat(64));

    assert_eq!(report.counts.core, 8);
    assert_eq!(report.counts.research, 45);
    assert_eq!(report.counts.rejected, 10);
    assert_eq!(report.research_by_theme[&CorpusTheme::TfisSsa], 10);
    assert_eq!(report.research_by_theme[&CorpusTheme::SovereignContext], 15);
    assert_eq!(report.research_by_theme[&CorpusTheme::AgentOperations], 10);
    assert_eq!(
        report.research_by_theme[&CorpusTheme::GovernanceEvidence],
        10
    );
}

#[test]
fn dropping_a_theme_below_its_minimum_fails() {
    let config = load_corpus(&corpus_path()).unwrap();
    let mut documents = documents_for(&config);
    let paths: Vec<_> = config
        .research
        .iter()
        .filter(|entry| entry.theme == CorpusTheme::TfisSsa)
        .take(3)
        .map(|entry| entry.path.clone())
        .collect();
    let exclusions = exclude(&mut documents, &paths);

    let error = evaluate_corpus(&config, &documents, &exclusions, &[]).unwrap_err();
    assert_eq!(error.code(), "corpus.theme_minimum");
}

#[test]
fn theme_gate_rejects_below_minimum_theme_after_prior_gates_pass() {
    let config = load_corpus(&corpus_path()).unwrap();
    let mut documents = documents_for(&config);
    let paths: Vec<_> = config
        .research
        .iter()
        .take(11)
        .map(|entry| entry.path.clone())
        .collect();
    let exclusions = exclude(&mut documents, &paths);

    let error = evaluate_corpus(&config, &documents, &exclusions, &[]).unwrap_err();
    assert_eq!(error.code(), "corpus.theme_minimum");
}

#[test]
fn passing_report_records_derived_research_and_total_minima() {
    let config = load_corpus(&corpus_path()).unwrap();
    let mut documents = documents_for(&config);
    let exclusions = exclude(&mut documents, &all_buffer_paths(&config));
    let report = evaluate_corpus(&config, &documents, &exclusions, &[])
        .unwrap()
        .finalize("d".repeat(64));

    assert!(report.counts.research >= 45);
    assert!(report.counts.core + report.counts.research >= 53);
}

#[test]
fn mutated_noncanonical_config_fails_at_entry() {
    let mut config = load_corpus(&corpus_path()).unwrap();
    let documents = documents_for(&config);
    config.minimum_documents = 64;

    let error = evaluate_corpus(&config, &documents, &[], &[]).unwrap_err();
    assert_eq!(error.code(), "corpus.config_invalid");
}

#[test]
fn structurally_valid_path_replacement_fails_the_in_memory_contract() {
    let mut config = load_corpus(&corpus_path()).unwrap();
    let documents = documents_for(&config);
    config.research[0].path = "04-Research/Reviewed-Replacement.md".into();

    let error = evaluate_corpus(&config, &documents, &[], &[]).unwrap_err();
    assert_eq!(error.code(), "corpus.config_invalid");
}

#[test]
fn balanced_theme_swap_fails_the_in_memory_contract() {
    let mut config = load_corpus(&corpus_path()).unwrap();
    let documents = documents_for(&config);
    let first = config
        .research
        .iter()
        .position(|entry| entry.theme == CorpusTheme::TfisSsa)
        .unwrap();
    let second = config
        .research
        .iter()
        .position(|entry| entry.theme == CorpusTheme::AgentOperations)
        .unwrap();
    let first_theme = config.research[first].theme.clone();
    config.research[first].theme = config.research[second].theme.clone();
    config.research[second].theme = first_theme;

    let error = evaluate_corpus(&config, &documents, &[], &[]).unwrap_err();
    assert_eq!(error.code(), "corpus.config_invalid");
}

#[test]
fn generated_path_outside_manifest_fails() {
    let config = load_corpus(&corpus_path()).unwrap();
    let mut documents = documents_for(&config);
    let mut extra = documents[0].clone();
    extra.metadata.source_path = "04-Research/Unexpected.md".into();
    extra.metadata.id = "ssa-0/04-Research/Unexpected".into();
    extra.metadata.resource = "vault://ssa-0/04-Research/Unexpected".into();
    extra.metadata.source = extra.metadata.resource.clone();
    documents.push(extra);

    let error = evaluate_corpus(&config, &documents, &[], &[]).unwrap_err();
    assert_eq!(error.code(), "corpus.unmanifested_document");
}

#[test]
fn generated_identity_must_be_canonical_for_its_source_path() {
    let config = load_corpus(&corpus_path()).unwrap();
    let mut documents = documents_for(&config);
    let source_path = config
        .research
        .iter()
        .find(|entry| entry.theme == CorpusTheme::AgentOperations)
        .unwrap()
        .path
        .as_str();
    let document = documents
        .iter_mut()
        .find(|document| document.metadata.source_path == source_path)
        .unwrap();
    let identity = document.metadata.id.split_once('/').unwrap().1.to_owned();
    document.metadata.id = format!("other-profile/{identity}");

    let error = evaluate_corpus(&config, &documents, &[], &[]).unwrap_err();
    assert_eq!(error.code(), "corpus.profile_identity");
}

#[test]
fn generated_documents_cannot_mix_profile_identity_tuples() {
    let config = load_corpus(&corpus_path()).unwrap();
    let mut documents = documents_for(&config);
    let source_path = config
        .research
        .iter()
        .find(|entry| entry.theme == CorpusTheme::AgentOperations)
        .unwrap()
        .path
        .as_str();
    let document = documents
        .iter_mut()
        .find(|document| document.metadata.source_path == source_path)
        .unwrap();
    document.metadata.ssa_id = "ssa-1".into();
    document.metadata.ssa_profile = "foreign-ssa-v0".into();
    document.metadata.familiar_id = "foreign".into();
    document.metadata.context_namespace = "ssa-1".into();
    document.metadata.id = document.metadata.id.replacen("ssa-0/", "ssa-1/", 1);
    document.metadata.resource =
        document
            .metadata
            .resource
            .replacen("vault://ssa-0/", "vault://ssa-1/", 1);
    document.metadata.source = document.metadata.resource.clone();

    let error = evaluate_corpus(&config, &documents, &[], &[]).unwrap_err();
    assert_eq!(error.code(), "corpus.profile_identity");
}

#[test]
fn uniformly_foreign_profile_tuple_cannot_establish_authority() {
    let config = load_corpus(&corpus_path()).unwrap();
    let mut documents = documents_for(&config);
    for document in &mut documents {
        document.metadata.profile = "foreign-profile-v0".into();
        document.metadata.ssa_id = "ssa-1".into();
        document.metadata.ssa_profile = "foreign-ssa-v0".into();
        document.metadata.familiar_id = "foreign".into();
        document.metadata.context_namespace = "ssa-1".into();
        document.metadata.id = document.metadata.id.replacen("ssa-0/", "ssa-1/", 1);
        document.metadata.resource =
            document
                .metadata
                .resource
                .replacen("vault://ssa-0/", "vault://ssa-1/", 1);
        document.metadata.source = document.metadata.resource.clone();
    }

    let error = evaluate_corpus(&config, &documents, &[], &[]).unwrap_err();
    assert_eq!(error.code(), "corpus.profile_identity");
}

#[test]
fn error_diagnostic_hard_fails_before_report_construction() {
    let config = load_corpus(&corpus_path()).unwrap();
    let documents = documents_for(&config);
    let diagnostics = [Diagnostic {
        severity: Severity::Error,
        code: "validation.failure".into(),
        source_path: None,
        message: format!("{BODY_SENTINEL} at /Volumes/himura/private"),
    }];

    let error = evaluate_corpus(&config, &documents, &[], &diagnostics).unwrap_err();
    assert_eq!(error.code(), "corpus.validation_error");
}

#[test]
fn unsupported_or_incomplete_evidence_fails_closed() {
    let config = load_corpus(&corpus_path()).unwrap();
    let buffer_path = config
        .research
        .iter()
        .find(|entry| entry.theme == CorpusTheme::AgentOperations)
        .unwrap()
        .path
        .clone();

    let mut missing = documents_for(&config);
    missing.retain(|document| document.metadata.source_path != buffer_path);
    assert_eq!(
        evaluate_corpus(&config, &missing, &[], &[])
            .unwrap_err()
            .code(),
        "corpus.evidence_mismatch"
    );

    let mut invalid_exclusion_documents = documents_for(&config);
    invalid_exclusion_documents.retain(|document| document.metadata.source_path != buffer_path);
    let invalid_exclusion = ExcludedSource {
        source_path: buffer_path.clone(),
        reason: "validation".into(),
    };
    assert_eq!(
        evaluate_corpus(
            &config,
            &invalid_exclusion_documents,
            &[invalid_exclusion],
            &[]
        )
        .unwrap_err()
        .code(),
        "corpus.evidence_mismatch"
    );

    let mut duplicated = documents_for(&config);
    let duplicate = duplicated
        .iter()
        .find(|document| document.metadata.source_path == buffer_path)
        .unwrap()
        .clone();
    duplicated.push(duplicate);
    assert_eq!(
        evaluate_corpus(&config, &duplicated, &[], &[])
            .unwrap_err()
            .code(),
        "corpus.evidence_mismatch"
    );

    let unknown = ExcludedSource {
        source_path: "04-Research/Unknown.md".into(),
        reason: "dirty".into(),
    };
    assert_eq!(
        evaluate_corpus(&config, &documents_for(&config), &[unknown], &[])
            .unwrap_err()
            .code(),
        "corpus.evidence_mismatch"
    );
}

#[test]
fn report_is_stably_ordered_and_never_serializes_bodies_or_absolute_paths() {
    let config = load_corpus(&corpus_path()).unwrap();
    let mut documents = documents_for(&config);
    documents.reverse();
    let mut paths = all_buffer_paths(&config);
    paths.truncate(2);
    let mut exclusions = exclude(&mut documents, &paths);
    exclusions.reverse();
    let diagnostics = vec![
        Diagnostic {
            severity: Severity::Warning,
            code: "z.warning".into(),
            source_path: None,
            message: format!("{BODY_SENTINEL} at /Volumes/himura/private"),
        },
        Diagnostic {
            severity: Severity::Warning,
            code: "z.warning".into(),
            source_path: None,
            message: "duplicate code".into(),
        },
    ];

    let report = evaluate_corpus(&config, &documents, &exclusions, &diagnostics)
        .unwrap()
        .finalize("c".repeat(64));

    assert!(report
        .accepted
        .windows(2)
        .all(|pair| pair[0].source_path < pair[1].source_path));
    assert!(report
        .rejected
        .windows(2)
        .all(|pair| pair[0].source_path < pair[1].source_path));
    assert_eq!(report.diagnostic_codes, ["z.warning"]);
    assert_eq!(report.counts.warnings, 2);
    assert_eq!(report.counts.errors, 0);

    let json = to_string(&report).unwrap();
    assert!(!json.contains(BODY_SENTINEL));
    assert!(!json.contains("/Volumes/himura"));
}
