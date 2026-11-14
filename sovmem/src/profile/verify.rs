use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::export::hash_bundle;
use super::{
    convert_links, fixed_corpus_config, glob_matches, parse_source_document,
    restricted_private_metadata, valid_relative_path, valid_selection_pattern, validate_bundle,
    BundleExportReport, BundleManifest, CorpusConfig, CorpusLane, CorpusReport, CorpusTheme,
    Diagnostic, ExcludedSource, GeneratedDocument, GeneratedMetadata, ManifestDocument,
    ManifestExclusion, ManifestSource, ProfileConfig, Severity, SourceDocument, CORPUS_SCHEMA,
    MINIMUM_DOCUMENTS, MINIMUM_RESEARCH_DOCUMENTS, PINNED_SOURCE_COMMIT, REQUIRED_CORE,
};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Level4Summary {
    pub status: &'static str,
    pub command: &'static str,
    pub level: u8,
    pub events: usize,
    pub verdicts: usize,
    pub accept: usize,
    pub reject: usize,
    pub rewrite: usize,
    pub tp: usize,
    pub fp: usize,
    pub applied_generations: usize,
    pub current_generation: String,
    pub bundle_hash: String,
    pub ledger_head: String,
    pub errors: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Level4Failure {
    code: &'static str,
}

impl Level4Failure {
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

#[derive(Clone, Debug, Default)]
pub struct VerificationAllowance {
    pub verdict_lease_token: Option<String>,
    pub intake_lease_token: Option<String>,
    pub recovery_verdict_lease_token: Option<String>,
}

#[derive(Clone, Debug)]
enum Level4Head {
    Seed,
    Generation(Box<super::VerifiedGeneration>),
}

impl Level4Head {
    fn as_str(&self) -> &str {
        match self {
            Self::Seed => "seed",
            Self::Generation(generation) => &generation.manifest.generation_id,
        }
    }

    fn bundle_hash<'a>(&'a self, seed: &'a super::SeedEvidence) -> &'a str {
        match self {
            Self::Seed => &seed.bundle_hash,
            Self::Generation(generation) => &generation.bundle_hash,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationLevel {
    One,
    Two,
    Three,
    Four,
}

impl VerificationLevel {
    pub const fn number(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerificationOptions {
    pub explicit_level: Option<VerificationLevel>,
    pub require_corpus: bool,
}

impl VerificationOptions {
    pub const fn compatibility(require_corpus: bool) -> Self {
        Self {
            explicit_level: None,
            require_corpus,
        }
    }

    pub const fn level(level: VerificationLevel) -> Self {
        Self {
            explicit_level: Some(level),
            require_corpus: matches!(level, VerificationLevel::Three),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct VerificationDiagnostic {
    pub(crate) severity: &'static str,
    pub(crate) code: String,
    pub(crate) message: &'static str,
}

impl VerificationDiagnostic {
    pub fn error(code: impl Into<String>, message: &'static str) -> Self {
        Self {
            severity: "error",
            code: code.into(),
            message,
        }
    }

    pub(crate) fn from_validation(diagnostic: &Diagnostic) -> Self {
        match diagnostic.severity {
            Severity::Warning => Self {
                severity: "warning",
                code: diagnostic.code.clone(),
                message: "generated document produced a profile validation warning",
            },
            Severity::Error => Self {
                severity: "error",
                code: diagnostic.code.clone(),
                message: "generated document failed profile validation",
            },
        }
    }
}

pub struct BundleVerification {
    pub(crate) documents: usize,
    pub(crate) bundle_hash: String,
    pub(crate) diagnostics: Vec<VerificationDiagnostic>,
    pub(crate) manifest: Option<BundleManifest>,
    pub(crate) corpus_report: Option<CorpusReport>,
    generated_documents: Vec<GeneratedDocument>,
    warning_count: usize,
}

impl BundleVerification {
    pub fn document_count(&self) -> usize {
        self.documents
    }

    pub fn bundle_hash(&self) -> &str {
        &self.bundle_hash
    }

    pub fn diagnostics(&self) -> &[VerificationDiagnostic] {
        &self.diagnostics
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == "error")
    }

    pub fn warning_count(&self) -> usize {
        self.warning_count
    }

    pub(crate) fn generated_documents(&self) -> &[GeneratedDocument] {
        &self.generated_documents
    }

    pub(crate) fn exclusion_count(&self) -> usize {
        self.manifest
            .as_ref()
            .map_or(0, |manifest| manifest.excluded_sources.len())
    }
}

pub fn verify_bundle(
    profile: &ProfileConfig,
    bundle_root: &Path,
    options: VerificationOptions,
) -> BundleVerification {
    if options.explicit_level == Some(VerificationLevel::Four) {
        return BundleVerification {
            documents: 0,
            bundle_hash: String::new(),
            diagnostics: vec![VerificationDiagnostic::error(
                "verify.level_invalid",
                "verification level 4 requires a verified P3 store",
            )],
            manifest: None,
            corpus_report: None,
            generated_documents: Vec::new(),
            warning_count: 0,
        };
    }
    let compatibility = options.explicit_level.is_none();
    let level = options.explicit_level;
    let run_integrity = compatibility
        || matches!(
            level,
            Some(VerificationLevel::Two | VerificationLevel::Three)
        );
    let run_semantic = compatibility || matches!(level, Some(VerificationLevel::Three));
    let require_corpus = options.require_corpus || matches!(level, Some(VerificationLevel::Three));
    let mut diagnostics = Vec::new();
    if require_corpus && !bundle_root.join("corpus-report.json").is_file() {
        diagnostics.push(VerificationDiagnostic::error(
            "bundle.corpus_report_required",
            "required corpus evidence could not be read",
        ));
    }
    if run_integrity && !valid_root_shape(bundle_root) {
        diagnostics.push(VerificationDiagnostic::error(
            "bundle.root_invalid",
            "bundle root shape is invalid",
        ));
    }

    let manifest_bytes = match read_bundle_file(bundle_root, "manifest.json") {
        Ok(bytes) => bytes,
        Err(()) => {
            diagnostics.push(VerificationDiagnostic::error(
                "bundle.manifest_invalid",
                "bundle manifest could not be read",
            ));
            return BundleVerification {
                documents: 0,
                bundle_hash: String::new(),
                diagnostics,
                manifest: None,
                corpus_report: None,
                generated_documents: Vec::new(),
                warning_count: 0,
            };
        }
    };
    let manifest: BundleManifest = match serde_json::from_slice(&manifest_bytes) {
        Ok(manifest) => manifest,
        Err(_) => {
            diagnostics.push(VerificationDiagnostic::error(
                "bundle.manifest_invalid",
                "bundle manifest could not be parsed",
            ));
            return BundleVerification {
                documents: 0,
                bundle_hash: String::new(),
                diagnostics,
                manifest: None,
                corpus_report: None,
                generated_documents: Vec::new(),
                warning_count: 0,
            };
        }
    };

    let export_report = if run_semantic {
        match read_bundle_file(bundle_root, "export-report.json") {
            Ok(bytes) => match serde_json::from_slice::<BundleExportReport>(&bytes) {
                Ok(report) => Some(report),
                Err(_) => {
                    diagnostics.push(VerificationDiagnostic::error(
                        "bundle.report_invalid",
                        "bundle export report could not be parsed",
                    ));
                    None
                }
            },
            Err(()) => {
                diagnostics.push(VerificationDiagnostic::error(
                    "bundle.report_invalid",
                    "bundle export report could not be read",
                ));
                None
            }
        }
    } else {
        None
    };

    let corpus_report = if run_semantic && bundle_root.join("corpus-report.json").exists() {
        match read_bundle_file(bundle_root, "corpus-report.json") {
            Ok(bytes) => match serde_json::from_slice::<CorpusReport>(&bytes) {
                Ok(report) => Some(report),
                Err(_) => {
                    diagnostics.push(VerificationDiagnostic::error(
                        "bundle.corpus_report_invalid",
                        "bundle corpus report could not be parsed",
                    ));
                    None
                }
            },
            Err(()) => {
                diagnostics.push(VerificationDiagnostic::error(
                    "bundle.corpus_report_invalid",
                    "bundle corpus report could not be read",
                ));
                None
            }
        }
    } else {
        None
    };

    validate_manifest_identity(profile, &manifest, &mut diagnostics);
    if run_integrity {
        validate_manifest_structure(&manifest, run_semantic, &mut diagnostics);
    }
    let source_hashes = if run_integrity {
        source_hashes(&manifest, &mut diagnostics)
    } else {
        manifest
            .sources
            .iter()
            .map(|source| (source.source_path.clone(), source.hash.clone()))
            .collect()
    };
    let mut bundle_files = BTreeMap::new();
    let mut generated_documents = Vec::new();
    let mut output_paths = BTreeSet::new();
    let mut source_paths = BTreeSet::new();

    for entry in &manifest.documents {
        let expected_output = format!("bundle/{}", entry.source_path);
        if !safe_relative_path(&entry.source_path)
            || !safe_relative_path(&entry.output_path)
            || entry.output_path != expected_output
            || entry.output_path == "bundle/index.md"
            || !output_paths.insert(entry.output_path.clone())
            || !source_paths.insert(entry.source_path.clone())
        {
            diagnostics.push(VerificationDiagnostic::error(
                "bundle.identity_mismatch",
                "bundle document identity does not match its manifest",
            ));
            continue;
        }

        let bytes = match read_bundle_file(bundle_root, &entry.output_path) {
            Ok(bytes) => bytes,
            Err(()) => {
                diagnostics.push(VerificationDiagnostic::error(
                    "bundle.hash_mismatch",
                    "generated file is missing or unreadable",
                ));
                continue;
            }
        };
        if run_integrity && blake3::hash(&bytes).to_hex().as_str() != entry.hash {
            diagnostics.push(VerificationDiagnostic::error(
                "bundle.document_hash_mismatch",
                "generated file bytes do not match the document hash",
            ));
            diagnostics.push(VerificationDiagnostic::error(
                "bundle.hash_mismatch",
                "generated file hash does not match its manifest",
            ));
        }
        if run_integrity {
            bundle_files.insert(entry.output_path.clone(), bytes.clone());
        }

        let source_hash = match source_hashes.get(&entry.source_path) {
            Some(source_hash) => source_hash.as_str(),
            None if run_semantic => {
                diagnostics.push(VerificationDiagnostic::error(
                    "bundle.identity_mismatch",
                    "generated document has no matching source identity",
                ));
                ""
            }
            None => "",
        };
        match generated_document(entry, bytes, source_hash) {
            Ok(document) => generated_documents.push(document),
            Err(document_diagnostics) => diagnostics.extend(document_diagnostics),
        }
    }

    let manifest_source_paths: BTreeSet<_> = source_hashes.keys().cloned().collect();
    if run_semantic && source_paths != manifest_source_paths {
        diagnostics.push(VerificationDiagnostic::error(
            "bundle.identity_mismatch",
            "bundle source and document identities do not correspond",
        ));
    }

    let computed_bundle_hash = if run_integrity {
        let mut expected_bundle_paths = output_paths;
        expected_bundle_paths.insert("bundle/index.md".into());
        match generated_file_paths(bundle_root) {
            Ok(actual_bundle_paths) if actual_bundle_paths == expected_bundle_paths => {}
            _ => diagnostics.push(VerificationDiagnostic::error(
                "bundle.identity_mismatch",
                "generated file set does not match the bundle manifest",
            )),
        }

        match read_bundle_file(bundle_root, "bundle/index.md") {
            Ok(index) => {
                bundle_files.insert("bundle/index.md".into(), index);
            }
            Err(()) => diagnostics.push(VerificationDiagnostic::error(
                "bundle.hash_mismatch",
                "bundle index is missing or unreadable",
            )),
        }

        let computed = hash_bundle(&bundle_files);
        if computed != manifest.bundle_hash {
            diagnostics.push(VerificationDiagnostic::error(
                "bundle.root_hash_mismatch",
                "bundle root does not match the generated files",
            ));
            diagnostics.push(VerificationDiagnostic::error(
                "bundle.hash_mismatch",
                "bundle hash does not match the generated files",
            ));
        }
        computed
    } else {
        manifest.bundle_hash.clone()
    };

    let validation = validate_bundle(profile, &generated_documents);
    let validation_warning_count = validation
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .count();
    diagnostics.extend(
        validation
            .diagnostics()
            .iter()
            .map(VerificationDiagnostic::from_validation),
    );

    if run_semantic {
        validate_typed_link_targets(profile, &generated_documents, &mut diagnostics);
        if let Some(report) = &export_report {
            validate_export_report(&manifest, report, &mut diagnostics);
        }
        if let Some(report) = &corpus_report {
            validate_corpus_report(
                profile,
                &manifest,
                export_report.as_ref(),
                report,
                &mut diagnostics,
            );
        }

        match convert_links(profile, generated_documents.clone()) {
            Err(error) => diagnostics.push(VerificationDiagnostic::error(
                error.code(),
                "generated document body failed profile isolation",
            )),
            Ok(reconstructed) => {
                if reconstructed.documents != generated_documents {
                    diagnostics.push(VerificationDiagnostic::error(
                        "document.link_state_mismatch",
                        "generated document body and link metadata are inconsistent",
                    ));
                }
                if let Some(report) = &export_report {
                    let mut reconstructed_report = validation.clone();
                    reconstructed_report.extend_link_warnings(&reconstructed.warnings);
                    let exclusions: Vec<_> = manifest
                        .excluded_sources
                        .iter()
                        .map(|source| ExcludedSource {
                            source_path: source.source_path.clone(),
                            reason: source.reason.clone(),
                        })
                        .collect();
                    reconstructed_report.extend_source_exclusions(&exclusions);
                    if reconstructed_report.diagnostics() != report.diagnostics {
                        diagnostics.push(VerificationDiagnostic::error(
                            "bundle.report_invalid",
                            "bundle export report does not match reconstructed diagnostics",
                        ));
                    }
                }
            }
        }
    }

    diagnostics.sort();
    diagnostics.dedup();
    let documents = manifest.documents.len();
    let warning_count = export_report
        .as_ref()
        .map_or(validation_warning_count, |report| report.counts.warnings);
    BundleVerification {
        documents,
        bundle_hash: computed_bundle_hash,
        diagnostics,
        manifest: Some(manifest),
        corpus_report,
        generated_documents,
        warning_count,
    }
}

pub(crate) fn verified_published_corpus_evidence(
    profile: &ProfileConfig,
    bundle_root: &Path,
) -> Option<(BundleManifest, CorpusReport)> {
    let bundle_root = fs::canonicalize(bundle_root).ok()?;
    let verification = verify_bundle(
        profile,
        &bundle_root,
        VerificationOptions::compatibility(false),
    );
    if verification
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == "error")
    {
        return None;
    }
    Some((verification.manifest?, verification.corpus_report?))
}

fn validate_manifest_structure(
    manifest: &BundleManifest,
    require_source_correspondence: bool,
    diagnostics: &mut Vec<VerificationDiagnostic>,
) {
    let inclusion_valid = !manifest.inclusion.is_empty()
        && strictly_sorted(&manifest.inclusion)
        && manifest
            .inclusion
            .iter()
            .all(|pattern| valid_selection_pattern(pattern));
    let exclusion_valid = strictly_sorted(&manifest.exclusion)
        && manifest
            .exclusion
            .iter()
            .all(|pattern| valid_selection_pattern(pattern));
    let sources_valid = strictly_sorted_by_source(&manifest.sources)
        && manifest
            .sources
            .iter()
            .all(|source| safe_relative_path(&source.source_path) && lowercase_hash(&source.hash));
    let excluded_valid = strictly_sorted_by_source(&manifest.excluded_sources)
        && manifest.excluded_sources.iter().all(|source| {
            safe_relative_path(&source.source_path)
                && matches!(source.reason.as_str(), "dirty" | "untracked")
        });
    let documents_valid = strictly_sorted_by_source(&manifest.documents)
        && manifest.documents.iter().all(|document| {
            safe_relative_path(&document.source_path)
                && safe_relative_path(&document.output_path)
                && document.output_path == format!("bundle/{}", document.source_path)
                && document.output_path != "bundle/index.md"
                && !document.id.is_empty()
                && !document.resource.is_empty()
                && lowercase_hash(&document.hash)
        });

    let source_paths: BTreeSet<_> = manifest
        .sources
        .iter()
        .map(|source| source.source_path.as_str())
        .collect();
    let document_paths: BTreeSet<_> = manifest
        .documents
        .iter()
        .map(|document| document.source_path.as_str())
        .collect();
    let excluded_paths: BTreeSet<_> = manifest
        .excluded_sources
        .iter()
        .map(|source| source.source_path.as_str())
        .collect();
    let output_paths: BTreeSet<_> = manifest
        .documents
        .iter()
        .map(|document| document.output_path.as_str())
        .collect();
    let represented_paths: BTreeSet<_> = source_paths.union(&excluded_paths).copied().collect();

    let selection_matches = represented_paths
        .iter()
        .all(|path| manifest_selects(manifest, path));
    let exact_includes_represented = manifest
        .inclusion
        .iter()
        .filter(|pattern| !pattern.contains('*'))
        .all(|path| represented_paths.contains(path.as_str()));
    let path_sets_valid = !represented_paths.is_empty()
        && source_paths.is_disjoint(&excluded_paths)
        && document_paths.is_disjoint(&excluded_paths)
        && output_paths.len() == manifest.documents.len()
        && (!require_source_correspondence || source_paths == document_paths);

    if manifest.exporter_version != env!("CARGO_PKG_VERSION")
        || !lowercase_hex(&manifest.commit, 40)
        || manifest.mode != "committed"
        || manifest.working_tree_allowed
        || !inclusion_valid
        || !exclusion_valid
        || !sources_valid
        || !excluded_valid
        || !documents_valid
        || !selection_matches
        || !exact_includes_represented
        || !path_sets_valid
        || !lowercase_hash(&manifest.bundle_hash)
    {
        diagnostics.push(VerificationDiagnostic::error(
            "bundle.manifest_invalid",
            "bundle manifest structure is invalid",
        ));
    }
}

fn validate_export_report(
    manifest: &BundleManifest,
    report: &BundleExportReport,
    diagnostics: &mut Vec<VerificationDiagnostic>,
) {
    let warning_count = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .count();
    let error_count = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .count();
    let diagnostics_valid = strictly_sorted(&report.diagnostics)
        && report.diagnostics.iter().all(|diagnostic| {
            diagnostic
                .source_path
                .as_deref()
                .is_none_or(safe_relative_path)
        });

    if report.counts.selected != manifest.sources.len() + manifest.excluded_sources.len()
        || report.counts.exported != manifest.documents.len()
        || report.counts.exported != manifest.sources.len()
        || report.counts.excluded != manifest.excluded_sources.len()
        || report.counts.warnings != warning_count
        || report.counts.errors != error_count
        || report.counts.errors != 0
        || !diagnostics_valid
    {
        diagnostics.push(VerificationDiagnostic::error(
            "bundle.report_invalid",
            "bundle export report is inconsistent",
        ));
    }
}

fn validate_corpus_report(
    profile: &ProfileConfig,
    manifest: &BundleManifest,
    export_report: Option<&BundleExportReport>,
    report: &CorpusReport,
    diagnostics: &mut Vec<VerificationDiagnostic>,
) {
    const RESEARCH_CANDIDATES: usize = 55;

    if !profile.is_canonical_audy_corpus_profile() {
        diagnostics.push(VerificationDiagnostic::error(
            "bundle.corpus_profile_invalid",
            "bundle corpus profile is not the canonical Audy profile",
        ));
        return;
    }

    let fixed_config: CorpusConfig = fixed_corpus_config();
    let fixed_paths: BTreeSet<_> = fixed_config.selected_paths().into_iter().collect();
    let manifest_documents: BTreeMap<_, _> = manifest
        .documents
        .iter()
        .map(|document| (document.source_path.as_str(), document.id.as_str()))
        .collect();
    let manifest_sources: BTreeMap<_, _> = manifest
        .sources
        .iter()
        .map(|source| (source.source_path.as_str(), source.hash.as_str()))
        .collect();
    let required_core: BTreeSet<_> = fixed_config
        .core
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    let expected_required_core: Vec<_> = required_core
        .iter()
        .map(|path| (*path).to_owned())
        .collect();
    let mut accepted_core = 0usize;
    let mut accepted_research = 0usize;
    let mut accepted_by_theme = BTreeMap::<CorpusTheme, usize>::new();
    let mut candidate_by_theme = BTreeMap::<CorpusTheme, usize>::new();
    let accepted_valid = strictly_sorted(&report.accepted)
        && report.accepted.iter().all(|accepted| {
            let identity_valid = safe_relative_path(&accepted.source_path)
                && manifest_documents.get(accepted.source_path.as_str())
                    == Some(&accepted.id.as_str())
                && manifest_sources.get(accepted.source_path.as_str())
                    == Some(&accepted.source_hash.as_str());
            let lane_valid = match (
                accepted.lane.as_str(),
                fixed_config.entry(&accepted.source_path),
            ) {
                ("core", Some(entry)) if entry.lane == CorpusLane::Core => {
                    accepted_core += 1;
                    accepted.theme.is_none()
                        && required_core.contains(accepted.source_path.as_str())
                }
                ("research", Some(entry)) if entry.lane == CorpusLane::Research => {
                    accepted_research += 1;
                    accepted.source_path.starts_with("04-Research/")
                        && accepted.theme.as_ref() == entry.theme
                        && accepted.theme.as_ref().is_some_and(|theme| {
                            *accepted_by_theme.entry(theme.clone()).or_default() += 1;
                            *candidate_by_theme.entry(theme.clone()).or_default() += 1;
                            true
                        })
                }
                _ => false,
            };
            identity_valid && lane_valid
        });
    let manifest_excluded: BTreeMap<_, _> = manifest
        .excluded_sources
        .iter()
        .map(|excluded| (excluded.source_path.as_str(), excluded.reason.as_str()))
        .collect();
    let rejected_valid = strictly_sorted(&report.rejected)
        && report.rejected.iter().all(|rejected| {
            let identity_valid = safe_relative_path(&rejected.source_path)
                && manifest_excluded.get(rejected.source_path.as_str())
                    == Some(&rejected.reason.as_str());
            let lane_valid = fixed_config
                .entry(&rejected.source_path)
                .is_some_and(|entry| {
                    rejected.lane == "research"
                        && entry.lane == CorpusLane::Research
                        && rejected.source_path.starts_with("04-Research/")
                        && rejected.theme.as_ref() == entry.theme
                        && matches!(rejected.reason.as_str(), "dirty" | "untracked")
                        && rejected.theme.as_ref().is_some_and(|theme| {
                            *candidate_by_theme.entry(theme.clone()).or_default() += 1;
                            true
                        })
                });
            identity_valid && lane_valid
        });
    let accepted_paths: BTreeSet<_> = report
        .accepted
        .iter()
        .map(|accepted| accepted.source_path.as_str())
        .collect();
    let rejected_paths: BTreeSet<_> = report
        .rejected
        .iter()
        .map(|rejected| rejected.source_path.as_str())
        .collect();
    let represented_paths: BTreeSet<_> = accepted_paths.union(&rejected_paths).copied().collect();
    let manifest_inclusion: BTreeSet<_> = manifest.inclusion.iter().map(String::as_str).collect();
    let themes = [
        CorpusTheme::TfisSsa,
        CorpusTheme::SovereignContext,
        CorpusTheme::AgentOperations,
        CorpusTheme::GovernanceEvidence,
    ];
    let expected_candidates = BTreeMap::from([
        (CorpusTheme::TfisSsa, 12usize),
        (CorpusTheme::SovereignContext, 15usize),
        (CorpusTheme::AgentOperations, 14usize),
        (CorpusTheme::GovernanceEvidence, 14usize),
    ]);
    let accepted_theme_minima = themes.iter().all(|theme| {
        accepted_by_theme.get(theme).copied().unwrap_or_default() >= theme.minimum_accepted()
    });
    let (expected_warnings, expected_errors, expected_diagnostic_codes) = export_report
        .map(|export_report| {
            let warnings = export_report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == Severity::Warning)
                .count();
            let errors = export_report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == Severity::Error)
                .count();
            let codes = export_report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            (warnings, errors, codes)
        })
        .unwrap_or_default();
    let counts_valid = report.counts.candidates == REQUIRED_CORE.len() + RESEARCH_CANDIDATES
        && report.counts.core == accepted_core
        && report.counts.research == accepted_research
        && report.counts.rejected == report.rejected.len()
        && report.counts.warnings == expected_warnings
        && report.counts.errors == expected_errors
        && report.counts.core == REQUIRED_CORE.len()
        && report.counts.research >= MINIMUM_RESEARCH_DOCUMENTS
        && report.counts.core + report.counts.research >= MINIMUM_DOCUMENTS
        && report.counts.core + report.counts.research + report.counts.rejected
            == report.counts.candidates;
    let report_valid = report.schema == CORPUS_SCHEMA
        && report.profile == manifest.profile
        && report.source_commit == PINNED_SOURCE_COMMIT
        && manifest.commit == PINNED_SOURCE_COMMIT
        && report.source_commit == manifest.commit
        && report.bundle_hash == manifest.bundle_hash
        && report.gates_passed
        && report.required_core == expected_required_core
        && accepted_valid
        && accepted_paths == manifest_documents.keys().copied().collect()
        && rejected_valid
        && report.rejected.len() == manifest_excluded.len()
        && accepted_paths.is_disjoint(&rejected_paths)
        && represented_paths.len() == report.counts.candidates
        && manifest.inclusion.iter().all(|path| !path.contains('*'))
        && manifest_inclusion == represented_paths
        && represented_paths == fixed_paths.iter().map(String::as_str).collect()
        && manifest_sources.len() + manifest_excluded.len() == report.counts.candidates
        && candidate_by_theme == expected_candidates
        && accepted_by_theme == report.research_by_theme
        && accepted_theme_minima
        && counts_valid
        && export_report.is_some()
        && report.diagnostic_codes == expected_diagnostic_codes;

    if !report_valid {
        diagnostics.push(VerificationDiagnostic::error(
            "bundle.corpus_report_invalid",
            "bundle corpus report is inconsistent",
        ));
    }
}

fn manifest_selects(manifest: &BundleManifest, source_path: &str) -> bool {
    manifest
        .inclusion
        .iter()
        .any(|pattern| glob_matches(pattern, source_path))
        && manifest
            .exclusion
            .iter()
            .all(|pattern| !glob_matches(pattern, source_path))
}

fn strictly_sorted<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

trait ManifestPath {
    fn source_path(&self) -> &str;
}

impl ManifestPath for ManifestSource {
    fn source_path(&self) -> &str {
        &self.source_path
    }
}

impl ManifestPath for ManifestExclusion {
    fn source_path(&self) -> &str {
        &self.source_path
    }
}

impl ManifestPath for ManifestDocument {
    fn source_path(&self) -> &str {
        &self.source_path
    }
}

fn strictly_sorted_by_source<T: ManifestPath>(values: &[T]) -> bool {
    values
        .windows(2)
        .all(|pair| pair[0].source_path() < pair[1].source_path())
}

fn valid_root_shape(bundle_root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(bundle_root) else {
        return false;
    };
    let mut actual = BTreeSet::new();
    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            return false;
        };
        let Ok(file_type) = entry.file_type() else {
            return false;
        };
        let valid_type = match name.as_str() {
            "manifest.json" | "export-report.json" | "corpus-report.json" => file_type.is_file(),
            "bundle" => file_type.is_dir(),
            _ => false,
        };
        if file_type.is_symlink() || !valid_type || !actual.insert(name) {
            return false;
        }
    }
    let required = BTreeSet::from([
        "bundle".to_owned(),
        "export-report.json".to_owned(),
        "manifest.json".to_owned(),
    ]);
    actual == required
        || actual
            == required
                .into_iter()
                .chain(["corpus-report.json".to_owned()])
                .collect()
}

fn validate_manifest_identity(
    profile: &ProfileConfig,
    manifest: &BundleManifest,
    diagnostics: &mut Vec<VerificationDiagnostic>,
) {
    let identity_matches = manifest.okf_version == profile.okf_version
        && manifest.profile == profile.profile
        && manifest.nous_schema == profile.nous_schema
        && manifest.ssa_id == profile.ssa_id
        && manifest.ssa_profile == profile.ssa_profile
        && manifest.familiar_id == profile.familiar_id
        && manifest.context_namespace == profile.context_namespace
        && manifest.vault_id == profile.vault_id
        && manifest.mode == "committed"
        && !manifest.working_tree_allowed;
    if !identity_matches {
        diagnostics.push(VerificationDiagnostic::error(
            "bundle.identity_mismatch",
            "bundle identity does not match the active profile",
        ));
    }
}

fn source_hashes(
    manifest: &BundleManifest,
    diagnostics: &mut Vec<VerificationDiagnostic>,
) -> BTreeMap<String, String> {
    let mut source_hashes = BTreeMap::new();
    for source in &manifest.sources {
        if !safe_relative_path(&source.source_path)
            || !lowercase_hash(&source.hash)
            || source_hashes
                .insert(source.source_path.clone(), source.hash.clone())
                .is_some()
        {
            diagnostics.push(VerificationDiagnostic::error(
                "bundle.identity_mismatch",
                "bundle source identity is invalid or duplicated",
            ));
        }
    }
    source_hashes
}

fn generated_document(
    entry: &ManifestDocument,
    bytes: Vec<u8>,
    source_hash: &str,
) -> Result<GeneratedDocument, Vec<VerificationDiagnostic>> {
    let parsed = parse_source_document(SourceDocument {
        source_path: entry.source_path.clone(),
        bytes,
        source_hash: source_hash.to_owned(),
        created: String::new(),
        updated: String::new(),
    })
    .map_err(|_| {
        vec![
            VerificationDiagnostic::error(
                "frontmatter.malformed",
                "generated frontmatter is malformed",
            ),
            VerificationDiagnostic::error(
                "bundle.document_invalid",
                "generated Markdown could not be parsed",
            ),
        ]
    })?;
    if !matches!(
        parsed
            .metadata
            .get("provenance")
            .and_then(serde_yaml::Value::as_str),
        Some("tier1" | "secondary" | "synthesis" | "unknown")
    ) {
        return Err(vec![
            VerificationDiagnostic::error(
                "document.provenance_invalid",
                "generated provenance is invalid",
            ),
            VerificationDiagnostic::error(
                "bundle.document_invalid",
                "generated metadata could not be decoded",
            ),
        ]);
    }
    let metadata_value = serde_yaml::to_value(&parsed.metadata).map_err(|_| {
        vec![VerificationDiagnostic::error(
            "bundle.document_invalid",
            "generated metadata could not be decoded",
        )]
    })?;
    let metadata: GeneratedMetadata = serde_yaml::from_value(metadata_value).map_err(|_| {
        vec![VerificationDiagnostic::error(
            "bundle.document_invalid",
            "generated metadata could not be decoded",
        )]
    })?;
    if parsed.contains_secret && !restricted_private_metadata(&metadata) {
        return Err(vec![VerificationDiagnostic::error(
            "document.restricted_content",
            "secret-like generated content requires restricted sensitivity and private scope",
        )]);
    }
    if metadata.source_path != entry.source_path
        || metadata.id != entry.id
        || metadata.resource != entry.resource
    {
        return Err(vec![VerificationDiagnostic::error(
            "bundle.identity_mismatch",
            "generated document identity does not match its manifest",
        )]);
    }

    Ok(GeneratedDocument {
        metadata,
        body: parsed.body,
        source_hash: source_hash.to_owned(),
    })
}

fn validate_typed_link_targets(
    profile: &ProfileConfig,
    documents: &[GeneratedDocument],
    diagnostics: &mut Vec<VerificationDiagnostic>,
) {
    let ids: BTreeSet<_> = documents
        .iter()
        .map(|document| document.metadata.id.as_str())
        .collect();
    let namespace_prefix = format!("{}/", profile.context_namespace);
    for document in documents {
        for link in &document.metadata.links {
            let target = link
                .target
                .split_once('#')
                .map_or(link.target.as_str(), |pair| pair.0);
            let target = target.strip_prefix("vault://").unwrap_or(target);
            if target.starts_with(&namespace_prefix) && !ids.contains(target) {
                diagnostics.push(VerificationDiagnostic::error(
                    "links.dangling",
                    "typed generated link target does not exist in the bundle",
                ));
            }
        }
    }
}

pub(crate) fn safe_relative_path(relative: &str) -> bool {
    valid_relative_path(relative)
}

fn lowercase_hash(hash: &str) -> bool {
    lowercase_hex(hash, 64)
}

fn lowercase_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn read_bundle_file(bundle_root: &Path, relative: &str) -> Result<Vec<u8>, ()> {
    if !safe_relative_path(relative) {
        return Err(());
    }
    let requested = bundle_root.join(relative);
    if fs::symlink_metadata(&requested)
        .map(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
        .unwrap_or(true)
    {
        return Err(());
    }
    let canonical = fs::canonicalize(&requested).map_err(|_| ())?;
    if !canonical.starts_with(bundle_root) {
        return Err(());
    }
    fs::read(canonical).map_err(|_| ())
}

fn generated_file_paths(bundle_root: &Path) -> Result<BTreeSet<String>, ()> {
    fn visit(
        bundle_root: &Path,
        directory: &Path,
        paths: &mut BTreeSet<String>,
    ) -> Result<bool, ()> {
        let mut entries: Vec<_> = fs::read_dir(directory)
            .map_err(|_| ())?
            .collect::<Result<_, _>>()
            .map_err(|_| ())?;
        entries.sort_by_key(|entry| entry.file_name());
        let mut contains_file = false;

        for entry in entries {
            let file_type = entry.file_type().map_err(|_| ())?;
            if file_type.is_symlink() {
                return Err(());
            }
            if file_type.is_dir() {
                if !visit(bundle_root, &entry.path(), paths)? {
                    return Err(());
                }
                contains_file = true;
                continue;
            }
            if !file_type.is_file() {
                return Err(());
            }
            contains_file = true;
            let path = entry.path();
            let relative = path.strip_prefix(bundle_root).map_err(|_| ())?;
            let relative = relative.to_str().ok_or(())?.replace('\\', "/");
            if !safe_relative_path(&relative) || !paths.insert(relative) {
                return Err(());
            }
        }
        Ok(contains_file)
    }

    let bundle_directory = bundle_root.join("bundle");
    if fs::symlink_metadata(&bundle_directory)
        .map(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
        .unwrap_or(true)
    {
        return Err(());
    }
    let mut paths = BTreeSet::new();
    if !visit(bundle_root, &bundle_directory, &mut paths)? {
        return Err(());
    }
    Ok(paths)
}

pub fn verify_store_level4(
    profile: &super::P3Profile,
    store_root: &Path,
) -> Result<Level4Summary, Level4Failure> {
    verify_store_level4_with_allowance(profile, store_root, &VerificationAllowance::default())
}

pub(crate) fn verify_store_level4_with_allowance(
    profile: &super::P3Profile,
    store_root: &Path,
    allowance: &VerificationAllowance,
) -> Result<Level4Summary, Level4Failure> {
    let snapshot = super::store::inspect_store_with_transients(profile, store_root)
        .map_err(level4_store_error)?;
    verify_level4_transient_state(&snapshot.paths, allowance)?;
    if !snapshot.sealed {
        return Err(level4_failure("ledger.genesis_missing"));
    }

    let view = super::read_ledger(&snapshot.paths.ledger).map_err(level4_ledger_error)?;
    let genesis = match view.events.first() {
        Some(super::LedgerEvent::Genesis(event)) => event.payload(),
        _ => return Err(level4_failure("ledger.genesis_missing")),
    };
    if genesis.store_id != snapshot.policy.policy.store_id
        || genesis.profile_version != profile.version
        || genesis.profile_hash != profile.hash
        || genesis.seed_manifest_hash != snapshot.seed.manifest_hash
        || genesis.seed_corpus_report_hash != snapshot.seed.corpus_report_hash
        || genesis.seed_bundle_hash != snapshot.seed.bundle_hash
        || genesis.source_commit != snapshot.seed.source_commit
        || genesis.initial_canonical_ref != "seed"
    {
        return Err(level4_failure("ledger.proposal_binding"));
    }

    let intake = super::read_intake(&snapshot.paths).map_err(|error| {
        level4_failure(match error.code() {
            "intake.chain_invalid" => "intake.chain_invalid",
            "intake.record_invalid" => "intake.record_invalid",
            "intake.pair_binding" => "intake.pair_binding",
            "intake.clock_regression" => "intake.clock_regression",
            _ => "ledger.intake_binding",
        })
    })?;
    let full_prefix = super::intake_prefix(&intake, intake.head())
        .map_err(|_| level4_failure("ledger.intake_binding"))?;
    let mut pairs_by_id = BTreeMap::new();
    for summary in &full_prefix.staged {
        let pair = super::read_sealed_pair_structural(&snapshot.paths, &summary.proposal_id)
            .map_err(|_| level4_failure("intake.pair_binding"))?;
        if pairs_by_id
            .insert(pair.meta.proposal_id.clone(), pair)
            .is_some()
        {
            return Err(level4_failure("ledger.duplicate_id"));
        }
    }
    let mut referenced_policies = BTreeSet::from([genesis.policy_hash.clone()]);
    let genesis_policy = load_historical_policy(&snapshot.paths, &genesis.policy_hash)?;
    if genesis_policy.policy.store_id != genesis.store_id
        || genesis_policy.policy.profile_version != genesis.profile_version
    {
        return Err(level4_failure("ledger.proposal_binding"));
    }

    let mut head = Level4Head::Seed;
    let mut applied_generation_ids = BTreeSet::new();
    let mut applied_verdict_ids = BTreeSet::new();
    let mut terminal_proposals = BTreeSet::new();

    for ledger_event in view.events.iter().skip(1) {
        let super::LedgerEvent::Verdict(event) = ledger_event else {
            return Err(level4_failure("ledger.genesis_duplicate"));
        };
        let event = event.payload();
        referenced_policies.insert(event.policy_hash.clone());
        if event.store_id != genesis.store_id {
            return Err(level4_failure("ledger.proposal_binding"));
        }
        let mut prefix = intake_prefix_for_event(&intake, event)?;
        if !prefix
            .staged
            .iter()
            .any(|summary| summary.proposal_id == event.proposal_id)
        {
            return Err(level4_failure("ledger.intake_binding"));
        }
        let pair = pairs_by_id
            .get(&event.proposal_id)
            .ok_or_else(|| level4_failure("ledger.proposal_binding"))?;
        verify_event_pair_binding(event, pair)?;
        if terminal_proposals.contains(&pair.meta.proposal_id) {
            return Err(level4_failure("ledger.duplicate_id"));
        }

        let policy = load_historical_policy(&snapshot.paths, &event.policy_hash)?;
        if policy.policy.human_actor != event.actor || event.profile_hash != profile.hash {
            return Err(level4_failure("ledger.verdict_binding"));
        }
        for retained in &mut prefix.staged {
            if terminal_proposals.contains(&retained.proposal_id) {
                retained.state = "ruled".to_owned();
            }
        }
        verify_historical_policy(profile, &snapshot, &policy, &head, pair, &prefix)?;
        verify_pair_base_against_head(pair, event, &head, &snapshot)?;
        terminal_proposals.insert(pair.meta.proposal_id.clone());

        if !event.verb.applies() {
            if event.resulting_canonical_head != head.as_str() {
                return Err(level4_failure("ledger.head_transition"));
            }
            continue;
        }

        let result = &event.canonical_result;
        let generation_id = result
            .generation_id
            .as_deref()
            .ok_or_else(|| level4_failure("ledger.generation_missing"))?;
        if !applied_generation_ids.insert(generation_id.to_owned())
            || !applied_verdict_ids.insert(event.verdict_id.clone())
        {
            return Err(level4_failure("ledger.duplicate_id"));
        }
        if event.resulting_canonical_head != generation_id {
            return Err(level4_failure("ledger.head_transition"));
        }
        let generation_root = snapshot.paths.canon.join("generations").join(generation_id);
        let generation =
            super::verify_generation(profile, &generation_root, VerificationLevel::Three)
                .map_err(|_| level4_failure("ledger.generation_missing"))?;
        verify_generation_binding(profile, &snapshot, &policy, &head, pair, event, &generation)?;
        verify_reindex_receipt_binding(&snapshot.paths, profile, event, &generation)?;
        head = Level4Head::Generation(Box::new(generation));
    }

    verify_level4_published_sets(
        &snapshot.paths,
        &referenced_policies,
        &snapshot.policy,
        &applied_generation_ids,
        &applied_verdict_ids,
    )?;
    if snapshot.head.as_str() != head.as_str() {
        return Err(level4_failure("ledger.current_mismatch"));
    }
    let ledger_head = view
        .final_hash
        .ok_or_else(|| level4_failure("ledger.genesis_missing"))?;
    Ok(Level4Summary {
        status: "ok",
        command: "verify",
        level: 4,
        events: view.events.len(),
        verdicts: view.verdicts,
        accept: view.accept,
        reject: view.reject,
        rewrite: view.rewrite,
        tp: view.tp,
        fp: view.fp,
        applied_generations: applied_generation_ids.len(),
        current_generation: head.as_str().to_owned(),
        bundle_hash: head.bundle_hash(&snapshot.seed).to_owned(),
        ledger_head,
        errors: 0,
    })
}

fn level4_failure(code: &'static str) -> Level4Failure {
    Level4Failure { code }
}

pub fn verify_preflight_transient_state(
    paths: &super::StorePaths,
    allowance: &VerificationAllowance,
) -> Result<(), Level4Failure> {
    verify_level4_transient_state(paths, allowance)
}

fn level4_store_error(error: super::StoreError) -> Level4Failure {
    level4_failure(error.code())
}

fn level4_ledger_error(error: super::LedgerError) -> Level4Failure {
    level4_failure(error.code())
}

fn verify_level4_transient_state(
    paths: &super::StorePaths,
    allowance: &VerificationAllowance,
) -> Result<(), Level4Failure> {
    let allowed_leases = match (
        allowance.verdict_lease_token.as_deref(),
        allowance.intake_lease_token.as_deref(),
        allowance.recovery_verdict_lease_token.as_deref(),
    ) {
        (None, None, None) => false,
        (Some(verdict), Some(intake), None) => {
            super::transaction::verify_held_lease_tokens(paths, verdict, intake)
                .map_err(|_| level4_failure("transaction.incomplete"))?;
            true
        }
        (None, None, Some(verdict)) => {
            super::transaction::verify_held_verdict_lease_token(paths, verdict)
                .map_err(|_| level4_failure("transaction.incomplete"))?;
            true
        }
        _ => return Err(level4_failure("transaction.incomplete")),
    };
    if has_lease_preparation_directories(paths)? {
        return Err(level4_failure("transaction.incomplete"));
    }
    if super::store::has_ledger_temporary(paths)
        .map_err(|_| level4_failure("transaction.incomplete"))?
    {
        return Err(level4_failure("transaction.incomplete"));
    }
    if super::store::has_current_temporary(paths)
        .map_err(|_| level4_failure("transaction.incomplete"))?
    {
        return Err(level4_failure("transaction.incomplete"));
    }
    if super::store::has_generation_or_index_staging(paths)
        .map_err(|_| level4_failure("transaction.incomplete"))?
    {
        return Err(level4_failure("transaction.incomplete"));
    }
    if super::store::has_intake_temporary(paths)
        .map_err(|_| level4_failure("transaction.incomplete"))?
    {
        return Err(level4_failure("transaction.incomplete"));
    }
    if super::intake_transactions_have_incomplete_state(paths)
        .map_err(|_| level4_failure("transaction.incomplete"))?
        || directory_has_entries(&paths.transactions)?
    {
        return Err(level4_failure("transaction.incomplete"));
    }
    if !allowed_leases
        && (paths.locks.join("verdict.lock").exists() || paths.locks.join("intake.lock").exists())
    {
        return Err(level4_failure("store.locked"));
    }
    Ok(())
}

fn has_lease_preparation_directories(paths: &super::StorePaths) -> Result<bool, Level4Failure> {
    for (path, prefix) in [
        (&paths.locks, ".lease-verdict-"),
        (&paths.locks, ".lease-intake-"),
    ] {
        for entry in fs::read_dir(path).map_err(|_| level4_failure("transaction.incomplete"))? {
            let name = entry
                .map_err(|_| level4_failure("transaction.incomplete"))?
                .file_name();
            if name.to_str().is_some_and(|name| name.starts_with(prefix)) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn verify_event_pair_binding(
    event: &super::VerdictPayload,
    pair: &super::ProposalPair,
) -> Result<(), Level4Failure> {
    let evaluator = pair
        .meta
        .evaluator
        .as_ref()
        .map(super::evaluator_commitment)
        .transpose()
        .map_err(|_| level4_failure("ledger.evaluator_binding"))?;
    if event.proposal_meta_hash != pair.meta_hash
        || event.candidate_hash != pair.candidate_hash
        || event.source_id != pair.meta.source_id
        || event.policy_hash != pair.meta.policy_hash
        || event.profile_hash != pair.meta.profile_hash
    {
        return Err(level4_failure("ledger.proposal_binding"));
    }
    if event.confidence != pair.meta.confidence {
        return Err(level4_failure("ledger.confidence_binding"));
    }
    if event.evaluator_meta_hash != evaluator {
        return Err(level4_failure("ledger.evaluator_binding"));
    }
    if event.base_kind != pair.meta.base.kind
        || event.base_generation != pair.meta.base.generation_id
        || event.base_manifest_hash != pair.meta.base.manifest_hash
        || event.base_bundle_hash != pair.meta.base.bundle_hash
        || event.base_target_hash != pair.meta.base.target_hash
    {
        return Err(level4_failure("ledger.proposal_binding"));
    }
    if event.verb == super::VerdictVerb::Accept && pair.meta.intent != super::ProposalIntent::Accept
    {
        return Err(level4_failure("ledger.verdict_binding"));
    }
    Ok(())
}

fn load_historical_policy(
    paths: &super::StorePaths,
    policy_hash: &str,
) -> Result<super::LoadedPolicy, Level4Failure> {
    let path = paths.policies.join(format!("{policy_hash}.toml"));
    let policy =
        super::load_policy(&path).map_err(|_| level4_failure("ledger.proposal_binding"))?;
    if policy.hash != policy_hash {
        return Err(level4_failure("ledger.proposal_binding"));
    }
    Ok(policy)
}

#[allow(clippy::too_many_arguments)]
fn verify_historical_policy(
    profile: &super::P3Profile,
    snapshot: &super::StoreSnapshot,
    policy: &super::LoadedPolicy,
    head: &Level4Head,
    pair: &super::ProposalPair,
    intake: &super::IntakePrefix,
) -> Result<(), Level4Failure> {
    if super::Timestamp::parse(&pair.meta.submitted_at).is_err() {
        return Err(level4_failure("ledger.proposal_binding"));
    }
    let historical_store = super::StoreSnapshot {
        paths: snapshot.paths.clone(),
        policy: policy.clone(),
        profile_hash: profile.hash.clone(),
        seed: snapshot.seed.clone(),
        head: match head {
            Level4Head::Seed => super::CanonicalHead::Seed,
            Level4Head::Generation(generation) => {
                super::CanonicalHead::Generation(generation.manifest.generation_id.clone())
            }
        },
        sealed: true,
    };
    let decision = super::PolicyEvaluator::evaluate(&super::PolicyContext {
        policy,
        profile,
        store: &historical_store,
        proposal: pair,
        intake,
    });
    if !decision.passed {
        return Err(level4_failure("ledger.proposal_binding"));
    }
    Ok(())
}

fn intake_prefix_for_event(
    view: &super::IntakeView,
    event: &super::VerdictPayload,
) -> Result<super::IntakePrefix, Level4Failure> {
    let head = super::IntakeHead {
        sequence: event.intake_sequence,
        event_hash: event.intake_head_hash.clone(),
    };
    super::intake_prefix(view, &head).map_err(|_| level4_failure("ledger.intake_binding"))
}

fn verify_pair_base_against_head(
    pair: &super::ProposalPair,
    event: &super::VerdictPayload,
    head: &Level4Head,
    snapshot: &super::StoreSnapshot,
) -> Result<(), Level4Failure> {
    let target_hash = target_hash_for_head(head, snapshot, &pair.meta.target_path)?;
    match head {
        Level4Head::Seed => {
            if pair.meta.base.kind != "seed"
                || pair.meta.base.generation_id.is_some()
                || pair.meta.base.manifest_hash != snapshot.seed.manifest_hash
                || pair.meta.base.bundle_hash != snapshot.seed.bundle_hash
            {
                return Err(level4_failure("ledger.proposal_binding"));
            }
        }
        Level4Head::Generation(generation) => {
            if pair.meta.base.kind != "generation"
                || pair.meta.base.generation_id.as_deref()
                    != Some(generation.manifest.generation_id.as_str())
                || pair.meta.base.manifest_hash != generation.manifest_hash
                || pair.meta.base.bundle_hash != generation.bundle_hash
            {
                return Err(level4_failure("ledger.proposal_binding"));
            }
        }
    }
    if pair.meta.base.target_hash != target_hash
        || event.resulting_canonical_head != head.as_str() && !event.verb.applies()
    {
        return Err(level4_failure("ledger.head_transition"));
    }
    Ok(())
}

fn verify_generation_binding(
    profile: &super::P3Profile,
    snapshot: &super::StoreSnapshot,
    policy: &super::LoadedPolicy,
    head: &Level4Head,
    pair: &super::ProposalPair,
    event: &super::VerdictPayload,
    generation: &super::VerifiedGeneration,
) -> Result<(), Level4Failure> {
    let result = &event.canonical_result;
    if generation.manifest.profile_hash != profile.hash
        || generation.manifest.policy_hash != event.policy_hash
        || generation.manifest.applied_proposal_id != event.proposal_id
        || generation.manifest.applied_verdict_id != event.verdict_id
        || result.manifest_hash.as_deref() != Some(generation.manifest_hash.as_str())
    {
        return Err(level4_failure("ledger.manifest_mismatch"));
    }
    if result.bundle_hash.as_deref() != Some(generation.bundle_hash.as_str()) {
        return Err(level4_failure("ledger.bundle_mismatch"));
    }
    match head {
        Level4Head::Seed => {
            if generation.manifest.parent_kind != super::ParentKind::Seed
                || generation.manifest.parent_generation_id.is_some()
                || generation.manifest.parent_manifest_hash != snapshot.seed.manifest_hash
                || generation.manifest.seed_manifest_hash != snapshot.seed.manifest_hash
                || generation.manifest.seed_bundle_hash != snapshot.seed.bundle_hash
            {
                return Err(level4_failure("ledger.generation_chain"));
            }
        }
        Level4Head::Generation(parent) => {
            if generation.manifest.parent_kind != super::ParentKind::Generation
                || generation.manifest.parent_generation_id.as_deref()
                    != Some(parent.manifest.generation_id.as_str())
                || generation.manifest.parent_manifest_hash != parent.manifest_hash
                || generation.manifest.seed_manifest_hash != snapshot.seed.manifest_hash
                || generation.manifest.seed_bundle_hash != snapshot.seed.bundle_hash
            {
                return Err(level4_failure("ledger.generation_chain"));
            }
        }
    }
    let target_path = generation.root.join("bundle").join(&pair.meta.target_path);
    let target_bytes = read_regular_bytes(&target_path, "ledger.generation_missing")?;
    let target_hash = blake3::hash(&target_bytes).to_hex().to_string();
    if result.target_new_hash.as_deref() != Some(target_hash.as_str()) {
        return Err(level4_failure("ledger.manifest_mismatch"));
    }
    let prior_hash = target_hash_for_head(head, snapshot, &pair.meta.target_path)?;
    if result.target_prior_hash != prior_hash {
        return Err(level4_failure("ledger.generation_chain"));
    }
    match event.verb {
        super::VerdictVerb::Accept => {
            if target_bytes != pair.candidate_bytes {
                return Err(level4_failure("ledger.verdict_binding"));
            }
        }
        super::VerdictVerb::Rewrite => {
            let rewrite =
                super::RewriteBytes::new(target_bytes.clone(), policy.policy.max_rewrite_bytes)
                    .map_err(|_| level4_failure("ledger.verdict_binding"))?;
            if rewrite.hash() != event.rewrite_hash.as_deref().unwrap_or_default()
                || u64::try_from(rewrite.bytes().len()).ok() != event.rewrite_bytes
                || super::validate_rewrite(
                    &rewrite,
                    profile,
                    &policy.policy,
                    &pair.meta.target_id,
                    &pair.meta.target_path,
                )
                .is_err()
            {
                return Err(level4_failure("ledger.verdict_binding"));
            }
        }
        _ => return Err(level4_failure("ledger.verb_semantics")),
    }
    let reason = super::Reason::new(event.reason.as_bytes(), policy.policy.max_reason_bytes)
        .map_err(|_| level4_failure("ledger.reason_empty"))?;
    let judgment = super::build_judgment(
        profile,
        &event.verdict_id,
        event.verb,
        &pair.meta.target_id,
        &reason,
        &super::Timestamp::parse(&event.occurred_at)
            .map_err(|_| level4_failure("ledger.verdict_binding"))?,
    )
    .map_err(|_| level4_failure("ledger.judgment_mismatch"))?
    .ok_or_else(|| level4_failure("ledger.judgment_mismatch"))?;
    let stored_judgment = read_regular_bytes(
        &generation.root.join("bundle").join(&judgment.path),
        "ledger.judgment_mismatch",
    )?;
    if stored_judgment != judgment.bytes
        || result.judgment_hash.as_deref() != Some(judgment.hash.as_str())
    {
        return Err(level4_failure("ledger.judgment_mismatch"));
    }
    Ok(())
}

fn verify_reindex_receipt_binding(
    paths: &super::StorePaths,
    profile: &super::P3Profile,
    event: &super::VerdictPayload,
    generation: &super::VerifiedGeneration,
) -> Result<(), Level4Failure> {
    let result = &event.canonical_result;
    let receipt_path = paths
        .evidence
        .join("reindex")
        .join(format!("{}.json", event.verdict_id));
    let bytes = read_regular_bytes(&receipt_path, "ledger.reindex_receipt_mismatch")?;
    let receipt: super::ReindexReceipt = serde_json::from_slice(&bytes)
        .map_err(|_| level4_failure("ledger.reindex_receipt_mismatch"))?;
    let receipt_hash = blake3::hash(&bytes).to_hex().to_string();
    if super::reindex_receipt_bytes(&receipt)
        .map_err(|_| level4_failure("ledger.reindex_receipt_mismatch"))?
        != bytes
        || result.reindex_receipt_hash.as_deref() != Some(receipt_hash.as_str())
        || super::verify_reindex_receipt_evidence(profile, generation, &receipt, &bytes).is_err()
    {
        return Err(level4_failure("ledger.reindex_receipt_mismatch"));
    }
    Ok(())
}

fn verify_level4_published_sets(
    paths: &super::StorePaths,
    policies: &BTreeSet<String>,
    current_policy: &super::LoadedPolicy,
    generations: &BTreeSet<String>,
    verdicts: &BTreeSet<String>,
) -> Result<(), Level4Failure> {
    let policy_files = directory_names(&paths.policies, "ledger.proposal_binding")?;
    let expected_policies = policies
        .iter()
        .map(|hash| format!("{hash}.toml"))
        .collect::<BTreeSet<_>>();
    let mut allowed_policies = expected_policies.clone();
    let current_policy_name = format!("{}.toml", current_policy.hash);
    allowed_policies.insert(current_policy_name.clone());
    if !expected_policies.is_subset(&policy_files) || !policy_files.is_subset(&allowed_policies) {
        return Err(level4_failure("ledger.proposal_binding"));
    }
    if policy_files.contains(&current_policy_name) {
        let current_snapshot = read_regular_bytes(
            &paths.policies.join(current_policy_name),
            "ledger.proposal_binding",
        )?;
        if current_snapshot != current_policy.bytes
            || blake3::hash(&current_snapshot).to_hex().as_str() != current_policy.hash
        {
            return Err(level4_failure("ledger.proposal_binding"));
        }
    }
    if directory_names(&paths.canon.join("generations"), "transaction.incomplete")? != *generations
    {
        return Err(level4_failure("transaction.incomplete"));
    }
    let expected_receipts = verdicts
        .iter()
        .map(|verdict_id| format!("{verdict_id}.json"))
        .collect::<BTreeSet<_>>();
    if directory_names(&paths.evidence.join("reindex"), "transaction.incomplete")?
        != expected_receipts
    {
        return Err(level4_failure("transaction.incomplete"));
    }
    Ok(())
}

fn target_hash_for_head(
    head: &Level4Head,
    snapshot: &super::StoreSnapshot,
    target_path: &str,
) -> Result<Option<String>, Level4Failure> {
    let root = match head {
        Level4Head::Seed => snapshot.paths.seed.join("bundle"),
        Level4Head::Generation(generation) => generation.root.join("bundle"),
    };
    let path = root.join(target_path);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(level4_failure("ledger.generation_chain"))
        }
        Ok(_) => Ok(Some(
            blake3::hash(&fs::read(path).map_err(|_| level4_failure("ledger.generation_chain"))?)
                .to_hex()
                .to_string(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(level4_failure("ledger.generation_chain")),
    }
}

fn read_regular_bytes(path: &Path, code: &'static str) -> Result<Vec<u8>, Level4Failure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| level4_failure(code))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(level4_failure(code));
    }
    fs::read(path).map_err(|_| level4_failure(code))
}

fn directory_has_entries(path: &Path) -> Result<bool, Level4Failure> {
    fs::read_dir(path)
        .map_err(|_| level4_failure("transaction.incomplete"))?
        .next()
        .transpose()
        .map(|entry| entry.is_some())
        .map_err(|_| level4_failure("transaction.incomplete"))
}

fn directory_names(path: &Path, code: &'static str) -> Result<BTreeSet<String>, Level4Failure> {
    fs::read_dir(path)
        .map_err(|_| level4_failure(code))?
        .map(|entry| {
            entry
                .map_err(|_| level4_failure(code))?
                .file_name()
                .into_string()
                .map_err(|_| level4_failure(code))
        })
        .collect()
}
