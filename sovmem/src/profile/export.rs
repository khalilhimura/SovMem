use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

use super::source::read_only_git;
use super::{
    convert_links, evaluate_corpus, glob_matches, load_canary, load_corpus, load_profile,
    normalize_document, open_committed_snapshot, parse_source_document, render_document,
    valid_relative_path, validate_bundle, validate_document_against_source, BundleExportReport,
    BundleManifest, CorpusConfig, CorpusError, CorpusGateError, CorpusReport, Diagnostic,
    ExcludedSource, ExportCounts, FrontmatterError, GeneratedDocument, LinkError, ManifestDocument,
    ManifestExclusion, ManifestSource, NormalizeError, ParsedSource, ProfileError, Severity,
    SourceError, SourceSnapshot,
};

static NEXT_TEMPORARY_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportRequest {
    pub vault_root: PathBuf,
    pub output_root: PathBuf,
    pub profile_path: PathBuf,
    pub canary_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportSummary {
    pub exported: usize,
    pub excluded: usize,
    pub warnings: usize,
    pub errors: usize,
    pub bundle_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusExportRequest {
    pub vault_root: PathBuf,
    pub output_root: PathBuf,
    pub profile_path: PathBuf,
    pub corpus_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusExportSummary {
    pub exported: usize,
    pub excluded: usize,
    pub core: usize,
    pub research: usize,
    pub warnings: usize,
    pub errors: usize,
    pub bundle_hash: String,
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error(transparent)]
    Corpus(#[from] CorpusError),
    #[error(transparent)]
    CorpusGate(#[from] CorpusGateError),
    #[error(transparent)]
    Profile(#[from] ProfileError),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error("frontmatter processing failed for {source_path}: {source}")]
    Frontmatter {
        source_path: String,
        #[source]
        source: FrontmatterError,
    },
    #[error("normalization failed for {source_path}: {source}")]
    Normalize {
        source_path: String,
        #[source]
        source: NormalizeError,
    },
    #[error(transparent)]
    Links(#[from] LinkError),
    #[error("export validation failed")]
    Validation { diagnostics: Vec<Diagnostic> },
    #[error("output root overlaps the source vault")]
    OutputOverlap,
    #[error("output root is invalid: {0}")]
    OutputInvalid(String),
    #[error("canary selection matched no source paths")]
    SelectionEmpty,
    #[error("source selection changed during export")]
    SourceChanged,
    #[error("selected source path contains unsupported characters or components")]
    SourcePathInvalid,
    #[error("JSON serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("{operation} failed for {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("publication failed: {0}")]
    Publish(String),
}

impl ExportError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Corpus(error) => error.code(),
            Self::CorpusGate(error) => error.code(),
            Self::Profile(error) => error.code(),
            Self::Source(error) => error.code(),
            Self::Frontmatter { source, .. } => source.code(),
            Self::Normalize { source, .. } => source.code(),
            Self::Links(error) => error.code(),
            Self::Validation { .. } => "export.validation",
            Self::OutputOverlap => "export.output_overlap",
            Self::OutputInvalid(_) => "export.output_invalid",
            Self::SelectionEmpty => "export.selection_empty",
            Self::SourceChanged => "export.source_changed",
            Self::SourcePathInvalid => "source.path_invalid",
            Self::Serialize(_) => "export.serialize",
            Self::Io { .. } => "export.io",
            Self::Publish(_) => "export.publish",
        }
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Validation { diagnostics } => diagnostics,
            _ => &[],
        }
    }
}

struct RenderedArtifacts {
    bundle_files: BTreeMap<String, Vec<u8>>,
    manifest_bytes: Vec<u8>,
    report_bytes: Vec<u8>,
    bundle_hash: String,
}

enum ExportFlavor<'a> {
    Canary,
    Corpus(&'a CorpusConfig),
}

struct SelectionRequest {
    vault_root: PathBuf,
    output_root: PathBuf,
    profile_path: PathBuf,
    source_commit: String,
    include: Vec<String>,
    exclude: Vec<String>,
}

pub fn export_canary(request: ExportRequest) -> Result<ExportSummary, ExportError> {
    let canary = load_canary(&request.canary_path)?;
    export_selection(
        SelectionRequest {
            vault_root: request.vault_root,
            output_root: request.output_root,
            profile_path: request.profile_path,
            source_commit: canary.source_commit,
            include: canary.include,
            exclude: canary.exclude,
        },
        ExportFlavor::Canary,
    )
    .map(|(summary, _)| summary)
}

pub fn export_corpus(request: CorpusExportRequest) -> Result<CorpusExportSummary, ExportError> {
    let corpus = load_corpus(&request.corpus_path)?;
    let include = corpus.selected_paths();
    let (summary, report) = export_selection(
        SelectionRequest {
            vault_root: request.vault_root,
            output_root: request.output_root,
            profile_path: request.profile_path,
            source_commit: corpus.source_commit.clone(),
            include,
            exclude: corpus.exclude.clone(),
        },
        ExportFlavor::Corpus(&corpus),
    )?;
    let report = report.ok_or(ExportError::SourceChanged)?;
    Ok(CorpusExportSummary {
        exported: summary.exported,
        excluded: summary.excluded,
        core: report.counts.core,
        research: report.counts.research,
        warnings: summary.warnings,
        errors: 0,
        bundle_hash: summary.bundle_hash,
    })
}

fn export_selection(
    request: SelectionRequest,
    flavor: ExportFlavor<'_>,
) -> Result<(ExportSummary, Option<CorpusReport>), ExportError> {
    let vault_root = canonicalize(&request.vault_root, "canonicalize vault root")?;
    let output_root = canonical_output_root(&request.output_root)?;
    ensure_roots_separate(&vault_root, &output_root)?;

    let profile = load_profile(&request.profile_path)?;
    if matches!(flavor, ExportFlavor::Corpus(_)) && !profile.is_canonical_audy_corpus_profile() {
        return Err(ExportError::CorpusGate(CorpusGateError::profile_contract()));
    }
    let selected_paths = select_source_paths(
        &vault_root,
        &request.source_commit,
        &request.include,
        &request.exclude,
    )?;
    if selected_paths.is_empty() {
        return Err(ExportError::SelectionEmpty);
    }

    let snapshot = open_committed_snapshot(&vault_root, &request.source_commit, &selected_paths)?;
    let mut parsed_sources = BTreeMap::<String, ParsedSource>::new();
    let mut documents = Vec::with_capacity(snapshot.documents.len());

    for source_document in snapshot.documents.iter().cloned() {
        let source_path = source_document.source_path.clone();
        let parsed =
            parse_source_document(source_document).map_err(|source| ExportError::Frontmatter {
                source_path: source_path.clone(),
                source,
            })?;
        let document = normalize_document(&profile, parsed.clone()).map_err(|source| {
            ExportError::Normalize {
                source_path: source_path.clone(),
                source,
            }
        })?;
        parsed_sources.insert(source_path, parsed);
        documents.push(document);
    }

    let converted = convert_links(&profile, documents)?;
    let mut documents = converted.documents;
    documents.sort_by(|left, right| {
        left.metadata
            .source_path
            .cmp(&right.metadata.source_path)
            .then_with(|| left.metadata.id.cmp(&right.metadata.id))
    });

    let mut bundle_report = validate_bundle(&profile, &documents);
    bundle_report.extend_link_warnings(&converted.warnings);
    bundle_report.extend_source_exclusions(&snapshot.excluded);
    let mut diagnostics = bundle_report.diagnostics().to_vec();

    for document in &documents {
        let source = parsed_sources
            .get(&document.metadata.source_path)
            .ok_or(ExportError::SourceChanged)?;
        diagnostics.extend(
            validate_document_against_source(document, source)
                .diagnostics()
                .iter()
                .cloned(),
        );
    }
    diagnostics.sort();
    diagnostics.dedup();

    let corpus_report = match flavor {
        ExportFlavor::Canary => None,
        ExportFlavor::Corpus(config) => Some(evaluate_corpus(
            &profile,
            config,
            &documents,
            &snapshot.excluded,
            &diagnostics,
        )?),
    };

    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        return Err(ExportError::Validation { diagnostics });
    }

    let warning_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .count();
    let output_parent = output_root
        .parent()
        .ok_or_else(|| ExportError::OutputInvalid("output root has no parent".into()))?;
    create_dir_all(output_parent, "create output parent")?;
    let mut staging = StagingDirectory::create(output_parent)?;
    let artifacts = render_artifacts(
        &profile,
        &request.include,
        &request.exclude,
        &snapshot,
        &documents,
        diagnostics,
        warning_count,
    )?;

    for (relative_path, bytes) in &artifacts.bundle_files {
        write_synced_file(&staging.path, Path::new(relative_path), bytes)?;
    }
    write_synced_file(
        &staging.path,
        Path::new("manifest.json"),
        &artifacts.manifest_bytes,
    )?;
    write_synced_file(
        &staging.path,
        Path::new("export-report.json"),
        &artifacts.report_bytes,
    )?;
    let corpus_report = corpus_report.map(|draft| draft.finalize(artifacts.bundle_hash.clone()));
    if let Some(report) = &corpus_report {
        write_synced_file(
            &staging.path,
            Path::new("corpus-report.json"),
            &pretty_json(report)?,
        )?;
    }
    sync_directory_tree(&staging.path)?;

    let confirmed = open_committed_snapshot(&vault_root, &request.source_commit, &selected_paths)?;
    if confirmed.documents != snapshot.documents || confirmed.excluded != snapshot.excluded {
        return Err(ExportError::SourceChanged);
    }

    publish(&mut staging, &output_root)?;

    Ok((
        ExportSummary {
            exported: documents.len(),
            excluded: snapshot.excluded.len(),
            warnings: warning_count,
            errors: 0,
            bundle_hash: artifacts.bundle_hash,
        },
        corpus_report,
    ))
}

fn select_source_paths(
    vault_root: &Path,
    commit: &str,
    inclusion: &[String],
    exclusion: &[String],
) -> Result<Vec<String>, ExportError> {
    open_committed_snapshot(vault_root, commit, &[])?;

    let mut candidates: BTreeSet<String> = git_paths(
        vault_root,
        &["ls-tree", "-r", "--name-only", "-z", commit],
        "ls-tree",
    )?
    .into_iter()
    .collect();
    candidates.extend(git_paths(
        vault_root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        "ls-files",
    )?);

    if let Some(missing) = inclusion
        .iter()
        .filter(|pattern| !pattern.contains('*'))
        .find(|path| !candidates.contains(path.as_str()))
    {
        return Err(SourceError::PathMissing {
            commit: commit.to_owned(),
            path: missing.clone(),
        }
        .into());
    }

    let selected: Vec<String> = candidates
        .into_iter()
        .filter(|path| inclusion.iter().any(|pattern| glob_matches(pattern, path)))
        .filter(|path| !exclusion.iter().any(|pattern| glob_matches(pattern, path)))
        .collect();
    if selected.iter().any(|path| !valid_relative_path(path)) {
        return Err(ExportError::SourcePathInvalid);
    }
    Ok(selected)
}

fn git_paths(
    vault_root: &Path,
    arguments: &[&str],
    operation: &'static str,
) -> Result<Vec<String>, ExportError> {
    let output = read_only_git(vault_root)
        .args(arguments)
        .output()
        .map_err(|error| SourceError::Git {
            operation,
            message: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(SourceError::Git {
            operation,
            message: format!(
                "{}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }
        .into());
    }

    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            String::from_utf8(path.to_vec()).map_err(|error| {
                SourceError::Git {
                    operation,
                    message: format!("source path was not UTF-8: {error}"),
                }
                .into()
            })
        })
        .collect()
}

fn render_artifacts(
    profile: &super::ProfileConfig,
    inclusion: &[String],
    exclusion: &[String],
    snapshot: &SourceSnapshot,
    documents: &[GeneratedDocument],
    diagnostics: Vec<Diagnostic>,
    warning_count: usize,
) -> Result<RenderedArtifacts, ExportError> {
    let mut bundle_files = BTreeMap::new();
    let mut manifest_documents = Vec::with_capacity(documents.len());

    for document in documents {
        let output_path = format!("bundle/{}", document.metadata.source_path);
        if output_path == "bundle/index.md" {
            return Err(ExportError::OutputInvalid(
                "generated document collides with bundle index".into(),
            ));
        }
        let bytes = render_document(document).map_err(|source| ExportError::Frontmatter {
            source_path: document.metadata.source_path.clone(),
            source,
        })?;
        let hash = blake3::hash(&bytes).to_hex().to_string();
        manifest_documents.push(ManifestDocument {
            source_path: document.metadata.source_path.clone(),
            output_path: output_path.clone(),
            id: document.metadata.id.clone(),
            resource: document.metadata.resource.clone(),
            hash,
        });
        bundle_files.insert(output_path, bytes);
    }

    bundle_files.insert(
        "bundle/index.md".into(),
        render_index(documents).into_bytes(),
    );
    let bundle_hash = hash_bundle(&bundle_files);
    let mut inclusion = inclusion.to_vec();
    inclusion.sort();
    inclusion.dedup();
    let mut exclusion = exclusion.to_vec();
    exclusion.sort();
    exclusion.dedup();

    let manifest = BundleManifest {
        okf_version: profile.okf_version.clone(),
        profile: profile.profile.clone(),
        nous_schema: profile.nous_schema.clone(),
        ssa_id: profile.ssa_id.clone(),
        ssa_profile: profile.ssa_profile.clone(),
        familiar_id: profile.familiar_id.clone(),
        context_namespace: profile.context_namespace.clone(),
        vault_id: profile.vault_id.clone(),
        exporter_version: env!("CARGO_PKG_VERSION").into(),
        commit: snapshot.commit.clone(),
        mode: "committed".into(),
        working_tree_allowed: false,
        inclusion,
        exclusion,
        excluded_sources: snapshot.excluded.iter().map(manifest_exclusion).collect(),
        sources: snapshot
            .documents
            .iter()
            .map(|document| ManifestSource {
                source_path: document.source_path.clone(),
                hash: document.source_hash.clone(),
            })
            .collect(),
        documents: manifest_documents,
        bundle_hash: bundle_hash.clone(),
    };
    let report = BundleExportReport {
        counts: ExportCounts {
            selected: snapshot.documents.len() + snapshot.excluded.len(),
            exported: documents.len(),
            excluded: snapshot.excluded.len(),
            warnings: warning_count,
            errors: 0,
        },
        diagnostics,
    };

    Ok(RenderedArtifacts {
        bundle_files,
        manifest_bytes: pretty_json(&manifest)?,
        report_bytes: pretty_json(&report)?,
        bundle_hash,
    })
}

fn manifest_exclusion(exclusion: &ExcludedSource) -> ManifestExclusion {
    ManifestExclusion {
        source_path: exclusion.source_path.clone(),
        reason: exclusion.reason.clone(),
    }
}

pub(crate) fn render_index(documents: &[GeneratedDocument]) -> String {
    let mut entries: Vec<_> = documents
        .iter()
        .map(|document| {
            (
                percent_encode_path(&document.metadata.source_path),
                escape_markdown_label(&document.metadata.title),
            )
        })
        .collect();
    entries.sort();

    let mut index = String::from("# SovMem Bundle\n\n");
    for (destination, title) in entries {
        index.push_str(&format!("- [{title}]({destination})\n"));
    }
    index
}

fn escape_markdown_label(label: &str) -> String {
    label
        .replace('\\', "\\\\")
        .replace(']', "\\]")
        .replace(['\r', '\n'], " ")
}

fn percent_encode_path(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

pub(crate) fn hash_bundle(files: &BTreeMap<String, Vec<u8>>) -> String {
    // For each UTF-8 path in ascending byte order: u64-LE path length, path bytes,
    // u64-LE content length, then the exact file bytes.
    let mut hasher = blake3::Hasher::new();
    for (path, bytes) in files {
        hasher.update(&(path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    hasher.finalize().to_hex().to_string()
}

fn pretty_json(value: &impl Serialize) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn canonicalize(path: &Path, operation: &'static str) -> Result<PathBuf, ExportError> {
    fs::canonicalize(path).map_err(|source| ExportError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    })
}

fn canonical_output_root(path: &Path) -> Result<PathBuf, ExportError> {
    let path = normalize_output_request(path)?;
    let path = path.as_path();
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(ExportError::OutputInvalid(
                    "existing output root is not a directory".into(),
                ));
            }
            canonicalize(path, "canonicalize output root")
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(|source| ExportError::Io {
                        operation: "read current directory",
                        path: PathBuf::from("."),
                        source,
                    })?
                    .join(path)
            };
            let ancestor = absolute
                .ancestors()
                .find(|ancestor| ancestor.exists())
                .ok_or_else(|| {
                    ExportError::OutputInvalid("output root has no existing ancestor".into())
                })?;
            let canonical_ancestor = canonicalize(ancestor, "canonicalize output ancestor")?;
            let remainder = absolute.strip_prefix(ancestor).map_err(|_| {
                ExportError::OutputInvalid("output root could not be resolved".into())
            })?;
            Ok(canonical_ancestor.join(remainder))
        }
        Err(source) => Err(ExportError::Io {
            operation: "inspect output root",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn normalize_output_request(path: &Path) -> Result<PathBuf, ExportError> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err(ExportError::OutputInvalid(
                    "output root must not contain parent traversal".into(),
                ))
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        normalized.push(".");
    }
    Ok(normalized)
}

fn ensure_roots_separate(vault_root: &Path, output_root: &Path) -> Result<(), ExportError> {
    if output_root == vault_root
        || output_root.starts_with(vault_root)
        || vault_root.starts_with(output_root)
    {
        Err(ExportError::OutputOverlap)
    } else {
        Ok(())
    }
}

fn create_dir_all(path: &Path, operation: &'static str) -> Result<(), ExportError> {
    fs::create_dir_all(path).map_err(|source| ExportError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    })
}

fn write_synced_file(root: &Path, relative_path: &Path, bytes: &[u8]) -> Result<(), ExportError> {
    let path = root.join(relative_path);
    let parent = path
        .parent()
        .ok_or_else(|| ExportError::OutputInvalid("generated file has no parent".into()))?;
    create_dir_all(parent, "create staged directory")?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|source| ExportError::Io {
            operation: "create staged file",
            path: path.clone(),
            source,
        })?;
    file.write_all(bytes).map_err(|source| ExportError::Io {
        operation: "write staged file",
        path: path.clone(),
        source,
    })?;
    file.sync_all().map_err(|source| ExportError::Io {
        operation: "fsync staged file",
        path,
        source,
    })
}

fn sync_directory_tree(path: &Path) -> Result<(), ExportError> {
    let mut directories = Vec::new();
    for entry in fs::read_dir(path).map_err(|source| ExportError::Io {
        operation: "read staged directory",
        path: path.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| ExportError::Io {
            operation: "read staged directory entry",
            path: path.to_path_buf(),
            source,
        })?;
        if entry
            .file_type()
            .map_err(|source| ExportError::Io {
                operation: "inspect staged directory entry",
                path: entry.path(),
                source,
            })?
            .is_dir()
        {
            directories.push(entry.path());
        }
    }
    directories.sort();
    for directory in directories {
        sync_directory_tree(&directory)?;
    }
    sync_directory(path)
}

fn sync_directory(path: &Path) -> Result<(), ExportError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| ExportError::Io {
            operation: "fsync directory",
            path: path.to_path_buf(),
            source,
        })
}

struct StagingDirectory {
    path: PathBuf,
    armed: bool,
}

impl StagingDirectory {
    fn create(parent: &Path) -> Result<Self, ExportError> {
        for _ in 0..1_000 {
            let path = temporary_sibling(parent, "stage");
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path, armed: true }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(source) => {
                    return Err(ExportError::Io {
                        operation: "create staging directory",
                        path,
                        source,
                    })
                }
            }
        }
        Err(ExportError::OutputInvalid(
            "could not allocate a staging directory".into(),
        ))
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn temporary_sibling(parent: &Path, kind: &str) -> PathBuf {
    let sequence = NEXT_TEMPORARY_ID.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(".sovmem-{kind}-{}-{sequence}", std::process::id()))
}

fn unused_backup_path(parent: &Path) -> Result<PathBuf, ExportError> {
    for _ in 0..1_000 {
        let path = temporary_sibling(parent, "backup");
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(path),
            Ok(_) => continue,
            Err(source) => {
                return Err(ExportError::Io {
                    operation: "inspect backup path",
                    path,
                    source,
                })
            }
        }
    }
    Err(ExportError::OutputInvalid(
        "could not allocate a backup path".into(),
    ))
}

fn publish(staging: &mut StagingDirectory, output_root: &Path) -> Result<(), ExportError> {
    let parent = output_root
        .parent()
        .ok_or_else(|| ExportError::OutputInvalid("output root has no parent".into()))?;

    if !output_root.exists() {
        fs::rename(&staging.path, output_root).map_err(|error| {
            ExportError::Publish(format!("could not install staged output: {error}"))
        })?;
        staging.disarm();
        sync_directory(parent).map_err(|error| {
            ExportError::Publish(format!(
                "staged output was installed but its directory entry could not be synced: {error}"
            ))
        })?;
        return Ok(());
    }

    let backup = unused_backup_path(parent)?;
    fs::rename(output_root, &backup).map_err(|error| {
        ExportError::Publish(format!("could not back up prior output: {error}"))
    })?;
    if let Err(sync_error) = sync_directory(parent) {
        let restore = fs::rename(&backup, output_root)
            .and_then(|_| File::open(parent).and_then(|directory| directory.sync_all()));
        return match restore {
            Ok(()) => Err(ExportError::Publish(format!(
                "prior output backup could not be synced: {sync_error}; prior output restored"
            ))),
            Err(restore_error) => {
                staging.disarm();
                Err(ExportError::Publish(format!(
                    "prior output backup could not be synced: {sync_error}; restore failed: {restore_error}"
                )))
            }
        };
    }

    if let Err(error) = fs::rename(&staging.path, output_root) {
        let restore = fs::rename(&backup, output_root)
            .and_then(|_| File::open(parent).and_then(|directory| directory.sync_all()));
        return match restore {
            Ok(()) => Err(ExportError::Publish(format!(
                "could not install staged output: {error}; prior output restored"
            ))),
            Err(restore_error) => {
                staging.disarm();
                Err(ExportError::Publish(format!(
                    "could not install staged output: {error}; restore failed: {restore_error}"
                )))
            }
        };
    }

    if let Err(sync_error) = sync_directory(parent) {
        if let Err(move_error) = fs::rename(output_root, &staging.path) {
            staging.disarm();
            return Err(ExportError::Publish(format!(
                "new output could not be synced: {sync_error}; could not prepare restore: {move_error}"
            )));
        }
        match fs::rename(&backup, output_root) {
            Ok(()) => {
                let restore_sync = sync_directory(parent);
                return match restore_sync {
                    Ok(()) => Err(ExportError::Publish(format!(
                        "new output could not be synced: {sync_error}; prior output restored"
                    ))),
                    Err(restore_error) => Err(ExportError::Publish(format!(
                        "new output could not be synced: {sync_error}; restored output could not be synced: {restore_error}"
                    ))),
                };
            }
            Err(restore_error) => {
                let republished = fs::rename(&staging.path, output_root);
                staging.disarm();
                return Err(ExportError::Publish(format!(
                    "new output could not be synced: {sync_error}; restore failed: {restore_error}; republish result: {republished:?}"
                )));
            }
        }
    }

    match fs::remove_dir_all(&backup) {
        Ok(()) => {
            staging.disarm();
            sync_directory(parent).map_err(|error| {
                ExportError::Publish(format!(
                    "new output was published but backup removal could not be synced: {error}"
                ))
            })
        }
        Err(cleanup_error) => {
            staging.disarm();
            let sync_result = sync_directory(parent);
            Err(ExportError::Publish(format!(
                "new output was published but backup cleanup was incomplete: {cleanup_error}; directory sync result: {sync_result:?}"
            )))
        }
    }
}
