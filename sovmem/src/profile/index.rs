use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tantivy::collector::TopDocs;
use tantivy::query::AllQuery;
use tantivy::schema::{
    Field, IndexRecordOption, OwnedValue, Schema, TextFieldIndexing, TextOptions, STORED, STRING,
};
use tantivy::tokenizer::{LowerCaser, RemoveLongFilter, SimpleTokenizer, TextAnalyzer};
use tantivy::{Index, IndexReader, ReloadPolicy};
use thiserror::Error;

use super::{
    derive_graph, load_profile, verify_bundle, verify_generation, CoreType, GeneratedDocument,
    GraphProjection, Layer, P3Profile, ProfileError, Provenance, VerificationDiagnostic,
    VerificationLevel, VerificationOptions, VerifiedGeneration, P3_STORE_ID,
};

pub const INDEX_MANIFEST_SCHEMA_V1: &str = "sovmem-derived-index-v1";
pub const INDEX_MANIFEST_SCHEMA_V2: &str = "sovmem-derived-index-v2";
pub const INDEX_MANIFEST_SCHEMA: &str = INDEX_MANIFEST_SCHEMA_V1;
pub const INDEX_SCHEMA_ID: &str = "sovmem-tantivy-schema-v1";
pub const TOKENIZER_ID: &str = "sovmem-simple-v1";
pub const RANKING_ID: &str = "sovmem-bm25-v1";
pub const TANTIVY_VERSION: &str = "0.24.2";
pub const PETGRAPH_VERSION: &str = "0.8.3";
pub const TOKEN_LIMIT_BYTES: usize = 80;
pub const TITLE_BOOST: f32 = 3.0;
pub const TAGS_BOOST: f32 = 2.0;
pub const BODY_BOOST: f32 = 1.0;

static NEXT_BUILD_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum IndexInputKind {
    FrozenCorpus,
    P3Canon,
}

impl IndexInputKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::FrozenCorpus => "frozen-corpus",
            Self::P3Canon => "p3-canon",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexInputBinding {
    pub kind: IndexInputKind,
    pub store_id: Option<String>,
    pub generation_id: Option<String>,
    pub manifest_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexDocument {
    pub source_path: String,
    pub id: String,
    pub title: String,
    pub body: String,
    pub layer: String,
    pub core_type: String,
    pub tags: Vec<String>,
    pub provenance: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedCorpusSummary {
    pub documents: usize,
    pub core: usize,
    pub research: usize,
    pub source_commit: String,
    pub bundle_hash: String,
    pub document_ids: Vec<String>,
    pub source_paths: Vec<String>,
    pub documents_hash: String,
}

pub struct IndexRequest {
    pub profile_path: PathBuf,
    pub bundle_root: PathBuf,
    pub index_root: PathBuf,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct IndexSummary {
    pub documents: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub external_typed_links: usize,
    pub bundle_hash: String,
    pub index_config: String,
    pub replaced_invalid_current: bool,
}

pub struct StoreIndexRequest {
    pub profile_path: PathBuf,
    pub store_root: PathBuf,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct StoreIndexSummary {
    pub documents: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub external_typed_links: usize,
    pub bundle_hash: String,
    pub index_config: String,
    pub store_id: String,
    pub generation: String,
    pub manifest_hash: String,
    pub receipt_hash: String,
}

pub struct P3IndexRequest<'a> {
    pub profile: &'a P3Profile,
    pub generation: &'a VerifiedGeneration,
    pub index_root: &'a Path,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P3IndexEvidence {
    pub input_kind: &'static str,
    pub store_id: Option<String>,
    pub generation_id: Option<String>,
    pub manifest_hash: Option<String>,
    pub documents_hash: String,
    pub graph_hash: String,
    pub documents: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub external_typed_links: usize,
    pub config_hash: String,
    pub semantic_manifest_hash: String,
    pub receipt: ReindexReceipt,
    pub receipt_bytes: Vec<u8>,
    pub receipt_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReindexReceipt {
    pub schema_version: String,
    pub verdict_id: String,
    pub generation_id: String,
    pub profile_hash: String,
    pub manifest_hash: String,
    pub bundle_hash: String,
    pub documents_hash: String,
    pub documents: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub external_typed_links: usize,
    pub graph_hash: String,
    pub index_schema: String,
    pub tokenizer: String,
    pub ranking: String,
    pub tantivy_version: String,
    pub petgraph_version: String,
    pub index_config_hash: String,
    pub semantic_manifest_hash: String,
}

#[allow(dead_code)]
pub struct VerifiedCorpus {
    pub profile: String,
    pub context_namespace: String,
    pub source_commit: String,
    pub input: IndexInputBinding,
    pub bundle_hash: String,
    pub core: usize,
    pub research: usize,
    pub graph: GraphProjection,
    pub documents: Vec<IndexDocument>,
    pub documents_hash: String,
}

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("profile configuration could not be loaded")]
    Profile(#[source] ProfileError),
    #[error("bundle root could not be read")]
    BundleRead,
    #[error("bundle did not pass explicit Level 3")]
    BundleUnverified(Vec<VerificationDiagnostic>),
    #[error("index output is invalid")]
    OutputInvalid,
    #[error("index build failed")]
    Build,
    #[error("index self-check failed")]
    SelfCheck,
    #[error("index publication is unsupported")]
    PublishUnsupported,
    #[error("index publication failed")]
    Publish,
    #[error("current index is invalid")]
    CurrentInvalid,
    #[error("index manifest is invalid")]
    ManifestInvalid,
    #[error("index bundle binding does not match")]
    BundleMismatch,
    #[error("index schema does not match")]
    SchemaMismatch,
    #[error("index document set is invalid")]
    DocumentSetInvalid,
    #[error("index graph is invalid")]
    GraphInvalid,
    #[error("Tantivy index is invalid")]
    TantivyInvalid,
    #[error("P3 canonical generation is unverified")]
    P3CanonUnverified,
    #[error("P3 index input binding does not match")]
    InputMismatch,
    #[error("P3 re-index receipt is invalid")]
    ReceiptInvalid,
    #[error("store verification failed")]
    StoreUnverified(&'static str),
    #[error("index input became stale")]
    InputStale,
}

impl IndexError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Profile(error) => error.code(),
            Self::BundleRead => "bundle.read",
            Self::BundleUnverified(_) => "index.bundle_unverified",
            Self::OutputInvalid => "index.output_invalid",
            Self::Build => "index.build_failed",
            Self::SelfCheck => "index.self_check_failed",
            Self::PublishUnsupported => "index.publish_unsupported",
            Self::Publish => "index.publish_failed",
            Self::CurrentInvalid => "index.current_invalid",
            Self::ManifestInvalid => "index.manifest_invalid",
            Self::BundleMismatch => "index.bundle_mismatch",
            Self::SchemaMismatch => "index.schema_mismatch",
            Self::DocumentSetInvalid => "index.document_set_invalid",
            Self::GraphInvalid => "index.graph_invalid",
            Self::TantivyInvalid => "index.tantivy_invalid",
            Self::P3CanonUnverified => "index.p3_canon_unverified",
            Self::InputMismatch => "index.input_mismatch",
            Self::ReceiptInvalid => "index.receipt_invalid",
            Self::StoreUnverified(code) => code,
            Self::InputStale => "index.input_stale",
        }
    }
}

pub fn load_verified_corpus(
    profile_path: &Path,
    bundle_root: &Path,
) -> Result<VerifiedCorpus, IndexError> {
    let profile = load_profile(profile_path).map_err(IndexError::Profile)?;
    let bundle_root: PathBuf = fs::canonicalize(bundle_root).map_err(|_| IndexError::BundleRead)?;
    let verification = verify_bundle(
        &profile,
        &bundle_root,
        VerificationOptions::level(VerificationLevel::Three),
    );

    if verification.has_errors() {
        return Err(IndexError::BundleUnverified(
            verification.diagnostics().to_vec(),
        ));
    }

    let manifest = verification.manifest.as_ref().ok_or_else(|| {
        IndexError::BundleUnverified(vec![VerificationDiagnostic::error(
            "bundle.manifest_required",
            "required Level 3 manifest evidence is absent",
        )])
    })?;
    let corpus_report = verification.corpus_report.as_ref().ok_or_else(|| {
        IndexError::BundleUnverified(vec![VerificationDiagnostic::error(
            "bundle.corpus_report_required",
            "required Level 3 corpus evidence is absent",
        )])
    })?;
    let evidence_is_exact = verification.document_count() == 62
        && manifest.documents.len() == 62
        && corpus_report.accepted.len() == 62
        && corpus_report.counts.core == 8
        && corpus_report.counts.research == 54
        && corpus_report.counts.errors == 0
        && corpus_report.gates_passed;
    if !evidence_is_exact {
        return Err(IndexError::BundleUnverified(vec![
            VerificationDiagnostic::error(
                "bundle.corpus_evidence_invalid",
                "Level 3 corpus evidence does not match the accepted corpus contract",
            ),
        ]));
    }

    let graph = derive_graph(
        verification.bundle_hash(),
        verification.generated_documents(),
    )
    .map_err(|_| IndexError::GraphInvalid)?;

    let mut ids = BTreeSet::new();
    let mut documents = Vec::with_capacity(verification.generated_documents().len());
    for generated in verification.generated_documents() {
        let metadata = &generated.metadata;
        if !ids.insert(metadata.id.clone()) {
            return Err(IndexError::OutputInvalid);
        }

        let mut tags = metadata.tags.clone();
        tags.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        if tags.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(IndexError::OutputInvalid);
        }
        tags.dedup();

        let layer = match metadata.layer {
            Layer::Synthesis => "synthesis",
            Layer::Analysis => "analysis",
            Layer::Concept => "concept",
        };
        let core_type = match metadata.core_type {
            CoreType::Concept => "concept",
            CoreType::Resource => "resource",
            CoreType::Judgment => "judgment",
        };
        let provenance = match metadata.provenance {
            Provenance::Tier1 => "tier1",
            Provenance::Secondary => "secondary",
            Provenance::Synthesis => "synthesis",
            Provenance::Unknown => "unknown",
        };
        documents.push(IndexDocument {
            source_path: metadata.source_path.clone(),
            id: metadata.id.clone(),
            title: metadata.title.clone(),
            body: generated.body.clone(),
            layer: layer.to_owned(),
            core_type: core_type.to_owned(),
            tags,
            provenance: provenance.to_owned(),
        });
    }
    documents.sort_by(|left, right| {
        left.source_path
            .as_bytes()
            .cmp(right.source_path.as_bytes())
            .then_with(|| left.id.as_bytes().cmp(right.id.as_bytes()))
    });
    let documents_hash = canonical_document_hash(&documents);

    Ok(VerifiedCorpus {
        profile: profile.profile,
        context_namespace: profile.context_namespace,
        source_commit: corpus_report.source_commit.clone(),
        input: IndexInputBinding {
            kind: IndexInputKind::FrozenCorpus,
            store_id: None,
            generation_id: None,
            manifest_hash: blake3::hash(
                &fs::read(bundle_root.join("manifest.json")).map_err(|_| IndexError::BundleRead)?,
            )
            .to_hex()
            .to_string(),
        },
        bundle_hash: verification.bundle_hash().to_owned(),
        core: corpus_report.counts.core,
        research: corpus_report.counts.research,
        graph,
        documents,
        documents_hash,
    })
}

pub fn load_verified_p3_generation(
    profile: &P3Profile,
    generation: &VerifiedGeneration,
) -> Result<(VerifiedGeneration, VerifiedCorpus), IndexError> {
    let verified = verify_generation(profile, &generation.root, VerificationLevel::Three)
        .map_err(|_| IndexError::P3CanonUnverified)?;
    if verified.manifest_hash != generation.manifest_hash
        || verified.manifest.generation_id != generation.manifest.generation_id
        || verified.manifest.store_id != P3_STORE_ID
    {
        return Err(IndexError::InputMismatch);
    }
    let graph = derive_graph(&verified.bundle_hash, &verified.generated_documents)
        .map_err(|_| IndexError::GraphInvalid)?;
    let documents = index_documents(&verified.generated_documents)?;
    let documents_hash = canonical_document_hash(&documents);
    let corpus = VerifiedCorpus {
        profile: profile.config.profile.clone(),
        context_namespace: profile.config.context_namespace.clone(),
        source_commit: "p3-canon".to_owned(),
        input: IndexInputBinding {
            kind: IndexInputKind::P3Canon,
            store_id: Some(verified.manifest.store_id.clone()),
            generation_id: Some(verified.manifest.generation_id.clone()),
            manifest_hash: verified.manifest_hash.clone(),
        },
        bundle_hash: verified.bundle_hash.clone(),
        core: 0,
        research: 0,
        graph,
        documents,
        documents_hash,
    };
    Ok((verified, corpus))
}

pub fn index_documents(
    generated_documents: &[GeneratedDocument],
) -> Result<Vec<IndexDocument>, IndexError> {
    let mut ids = BTreeSet::new();
    let mut documents = Vec::with_capacity(generated_documents.len());
    for generated in generated_documents {
        let metadata = &generated.metadata;
        if !ids.insert(metadata.id.clone()) {
            return Err(IndexError::OutputInvalid);
        }
        let mut tags = metadata.tags.clone();
        tags.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        if tags.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(IndexError::OutputInvalid);
        }
        tags.dedup();
        let layer = match metadata.layer {
            Layer::Synthesis => "synthesis",
            Layer::Analysis => "analysis",
            Layer::Concept => "concept",
        };
        let core_type = match metadata.core_type {
            CoreType::Concept => "concept",
            CoreType::Resource => "resource",
            CoreType::Judgment => "judgment",
        };
        let provenance = match metadata.provenance {
            Provenance::Tier1 => "tier1",
            Provenance::Secondary => "secondary",
            Provenance::Synthesis => "synthesis",
            Provenance::Unknown => "unknown",
        };
        documents.push(IndexDocument {
            source_path: metadata.source_path.clone(),
            id: metadata.id.clone(),
            title: metadata.title.clone(),
            body: generated.body.clone(),
            layer: layer.to_owned(),
            core_type: core_type.to_owned(),
            tags,
            provenance: provenance.to_owned(),
        });
    }
    documents.sort_by(|left, right| {
        left.source_path
            .as_bytes()
            .cmp(right.source_path.as_bytes())
            .then_with(|| left.id.as_bytes().cmp(right.id.as_bytes()))
    });
    Ok(documents)
}

pub fn frame(hasher: &mut blake3::Hasher, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_be_bytes());
    hasher.update(value);
}

pub fn canonical_document_hash(documents: &[IndexDocument]) -> String {
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, b"sovmem-index-documents-v1");
    for document in documents {
        for value in [
            document.id.as_bytes(),
            document.title.as_bytes(),
            document.body.as_bytes(),
            document.layer.as_bytes(),
            document.core_type.as_bytes(),
            document.provenance.as_bytes(),
        ] {
            frame(&mut hasher, value);
        }
        frame(&mut hasher, &(document.tags.len() as u64).to_be_bytes());
        for tag in &document.tags {
            frame(&mut hasher, tag.as_bytes());
        }
    }
    hasher.finalize().to_hex().to_string()
}

pub fn prepare_verified_corpus(
    profile_path: &Path,
    bundle_root: &Path,
) -> Result<PreparedCorpusSummary, IndexError> {
    let corpus = load_verified_corpus(profile_path, bundle_root)?;
    Ok(PreparedCorpusSummary {
        documents: corpus.documents.len(),
        core: corpus.core,
        research: corpus.research,
        source_commit: corpus.source_commit,
        bundle_hash: corpus.bundle_hash,
        document_ids: corpus
            .documents
            .iter()
            .map(|document| document.id.clone())
            .collect(),
        source_paths: corpus
            .documents
            .iter()
            .map(|document| document.source_path.clone())
            .collect(),
        documents_hash: corpus.documents_hash,
    })
}

pub fn build_index(request: IndexRequest) -> Result<IndexSummary, IndexError> {
    build_index_for_platform(request, cfg!(any(target_os = "macos", target_os = "linux")))
}

pub fn build_p3_index_generation(
    request: P3IndexRequest<'_>,
) -> Result<P3IndexEvidence, IndexError> {
    let (generation, corpus) = load_verified_p3_generation(request.profile, request.generation)?;
    let index_root = create_hidden_p3_index_root(&generation.root, request.index_root)?;
    let build = build_generation(&corpus, &index_root);
    if build.is_err() {
        let _ = fs::remove_dir_all(&index_root);
        return Err(IndexError::Build);
    }
    let opened = match open_generation(&corpus, &index_root) {
        Ok(opened) => opened,
        Err(_) => {
            let _ = fs::remove_dir_all(&index_root);
            return Err(IndexError::SelfCheck);
        }
    };
    let evidence = p3_evidence(request.profile, &generation, &opened.manifest, &index_root)?;
    drop(opened);
    if let Err(error) = sync_index_tree(&index_root) {
        let _ = fs::remove_dir_all(&index_root);
        return Err(error);
    }
    Ok(evidence)
}

pub fn rebuild_store_index(request: StoreIndexRequest) -> Result<StoreIndexSummary, IndexError> {
    rebuild_store_index_with_prepublication_hook(request, || {})
}

#[cfg(any(test, feature = "test-support"))]
pub fn rebuild_store_index_with_test_hook(
    request: StoreIndexRequest,
    hook: impl FnOnce(),
) -> Result<StoreIndexSummary, IndexError> {
    rebuild_store_index_with_prepublication_hook(request, hook)
}

fn rebuild_store_index_with_prepublication_hook(
    request: StoreIndexRequest,
    hook: impl FnOnce(),
) -> Result<StoreIndexSummary, IndexError> {
    let profile = super::load_p3_profile(&request.profile_path).map_err(IndexError::Profile)?;
    let level4 = super::verify_store_level4(&profile, &request.store_root)
        .map_err(|error| IndexError::StoreUnverified(error.code()))?;
    if level4.current_generation == "seed" {
        return Err(IndexError::P3CanonUnverified);
    }
    let snapshot = super::inspect_store(&profile, &request.store_root)
        .map_err(|error| IndexError::StoreUnverified(error.code()))?;
    let generation = super::verify_generation(
        &profile,
        &snapshot
            .paths
            .canon
            .join("generations")
            .join(&level4.current_generation),
        super::VerificationLevel::Three,
    )
    .map_err(|_| IndexError::P3CanonUnverified)?;
    let indexes = snapshot.paths.derived.join("indexes");
    rebuild_store_index_inner(&profile, &snapshot, &level4, &generation, &indexes, hook)
}

fn rebuild_store_index_inner(
    profile: &P3Profile,
    snapshot: &super::StoreSnapshot,
    level4: &super::Level4Summary,
    generation: &VerifiedGeneration,
    indexes: &Path,
    prepublication_hook: impl FnOnce(),
) -> Result<StoreIndexSummary, IndexError> {
    let final_root = indexes.join(&level4.current_generation);
    fs::create_dir_all(&final_root).map_err(|_| IndexError::Publish)?;
    sync_directory(indexes).map_err(|_| IndexError::Publish)?;
    let (slot, staging) = allocate_store_build_paths(&final_root)?;
    let evidence = build_p3_index_generation(P3IndexRequest {
        profile,
        generation,
        index_root: &staging,
    })?;
    let mut staging = StagingGeneration {
        path: staging,
        armed: true,
    };
    let receipt_path = snapshot
        .paths
        .evidence
        .join("reindex")
        .join(format!("{}.json", generation.manifest.applied_verdict_id));
    let receipt_bytes = fs::read(&receipt_path).map_err(|_| IndexError::ReceiptInvalid)?;
    let receipt: ReindexReceipt =
        serde_json::from_slice(&receipt_bytes).map_err(|_| IndexError::ReceiptInvalid)?;
    if evidence.receipt != receipt || evidence.receipt_bytes != receipt_bytes {
        return Err(IndexError::ReceiptInvalid);
    }

    let current = fs::read(&snapshot.paths.current).map_err(|_| IndexError::InputStale)?;
    let ledger = super::read_ledger(&snapshot.paths.ledger).map_err(|_| IndexError::InputStale)?;
    if !store_index_input_matches(
        &current,
        ledger.final_hash.as_deref(),
        &level4.current_generation,
        &level4.ledger_head,
    ) {
        return Err(IndexError::InputStale);
    }

    let prior_slot = match read_store_current_pointer(&final_root) {
        StoreCurrentPointer::Absent => None,
        StoreCurrentPointer::Exact(slot) => Some(slot),
        StoreCurrentPointer::Invalid => return Err(IndexError::CurrentInvalid),
    };
    install_store_generation(&mut staging, &final_root, &slot)?;
    let installed = final_root.join(&slot);
    if verify_reindex_receipt(profile, generation, &installed, &receipt, &receipt_bytes).is_err() {
        let _ = fs::remove_dir_all(&installed);
        return Err(IndexError::SelfCheck);
    }
    prepublication_hook();
    let input_current = (|| {
        let current = fs::read(&snapshot.paths.current).map_err(|_| IndexError::InputStale)?;
        let ledger =
            super::read_ledger(&snapshot.paths.ledger).map_err(|_| IndexError::InputStale)?;
        Ok(store_index_input_matches(
            &current,
            ledger.final_hash.as_deref(),
            &level4.current_generation,
            &level4.ledger_head,
        ))
    })();
    if !matches!(input_current, Ok(true)) {
        let _ = fs::remove_dir_all(&installed);
        return Err(input_current.err().unwrap_or(IndexError::InputStale));
    }
    if let Err(error) = publish_store_current_pointer(&final_root, &slot) {
        let _ = fs::remove_dir_all(&installed);
        return Err(error);
    }
    finish_store_publication_after_pointer(&final_root, &slot, prior_slot.as_deref())?;
    sync_directory(indexes).map_err(|_| IndexError::Publish)?;
    Ok(StoreIndexSummary {
        documents: evidence.documents,
        graph_nodes: evidence.graph_nodes,
        graph_edges: evidence.graph_edges,
        external_typed_links: evidence.external_typed_links,
        bundle_hash: generation.bundle_hash.clone(),
        index_config: evidence.config_hash,
        store_id: P3_STORE_ID.to_owned(),
        generation: level4.current_generation.clone(),
        manifest_hash: generation.manifest_hash.clone(),
        receipt_hash: evidence.receipt_hash,
    })
}

pub fn finish_store_publication_after_pointer(
    final_root: &Path,
    slot: &str,
    prior_slot: Option<&str>,
) -> Result<(), IndexError> {
    finish_store_publication_after_pointer_with(
        final_root,
        slot,
        prior_slot,
        sync_directory,
        |path| fs::remove_dir_all(path),
    )
}

pub fn finish_store_publication_after_pointer_with(
    final_root: &Path,
    slot: &str,
    prior_slot: Option<&str>,
    mut sync: impl FnMut(&Path) -> io::Result<()>,
    mut remove: impl FnMut(&Path) -> io::Result<()>,
) -> Result<(), IndexError> {
    sync(final_root).map_err(|_| IndexError::Publish)?;
    if let Some(prior_slot) = prior_slot {
        if matches!(read_store_current_pointer(final_root), StoreCurrentPointer::Exact(ref selected) if selected == slot)
        {
            let _ = remove(&final_root.join(prior_slot));
            sync(final_root).map_err(|_| IndexError::Publish)?;
        }
    }
    Ok(())
}

pub fn allocate_store_build_paths(final_root: &Path) -> Result<(String, PathBuf), IndexError> {
    allocate_store_build_paths_with(final_root, || {
        let sequence = NEXT_BUILD_ID.fetch_add(1, Ordering::Relaxed);
        format!("{:016x}{:016x}", std::process::id(), sequence)
    })
}

pub fn allocate_store_build_paths_with(
    final_root: &Path,
    mut next_token: impl FnMut() -> String,
) -> Result<(String, PathBuf), IndexError> {
    for _ in 0..1_000 {
        let token = next_token();
        let staging = final_root.join(format!(".slot-building-{token}"));
        let slot = format!("slot-{token}");
        if fs::symlink_metadata(&staging).is_err()
            && fs::symlink_metadata(final_root.join(&slot)).is_err()
            && fs::symlink_metadata(final_root.join(format!("CURRENT.next-{token}"))).is_err()
        {
            return Ok((slot, staging));
        }
    }
    Err(IndexError::Build)
}

pub fn store_index_input_matches(
    current: &[u8],
    ledger_head: Option<&str>,
    generation: &str,
    verified_ledger_head: &str,
) -> bool {
    current == format!("{generation}\n").as_bytes() && ledger_head == Some(verified_ledger_head)
}

pub fn open_verified_store_index(
    profile_path: &Path,
    store_root: &Path,
) -> Result<(OpenedIndex, P3IndexEvidence, String, String), IndexError> {
    let profile = super::load_p3_profile(profile_path).map_err(IndexError::Profile)?;
    let level4 = super::verify_store_level4(&profile, store_root)
        .map_err(|error| IndexError::StoreUnverified(error.code()))?;
    if level4.current_generation == "seed" {
        return Err(IndexError::P3CanonUnverified);
    }
    let snapshot = super::inspect_store(&profile, store_root)
        .map_err(|error| IndexError::StoreUnverified(error.code()))?;
    let generation = super::verify_generation(
        &profile,
        &snapshot
            .paths
            .canon
            .join("generations")
            .join(&level4.current_generation),
        super::VerificationLevel::Three,
    )
    .map_err(|_| IndexError::P3CanonUnverified)?;
    let index_root = snapshot
        .paths
        .derived
        .join("indexes")
        .join(&level4.current_generation);
    let receipt_path = snapshot
        .paths
        .evidence
        .join("reindex")
        .join(format!("{}.json", generation.manifest.applied_verdict_id));
    let receipt_bytes = fs::read(receipt_path).map_err(|_| IndexError::ReceiptInvalid)?;
    let receipt: ReindexReceipt =
        serde_json::from_slice(&receipt_bytes).map_err(|_| IndexError::ReceiptInvalid)?;
    let selected_root = match read_store_current_pointer(&index_root) {
        StoreCurrentPointer::Exact(slot) => index_root.join(slot),
        StoreCurrentPointer::Absent => index_root,
        StoreCurrentPointer::Invalid => return Err(IndexError::CurrentInvalid),
    };
    verify_reindex_receipt(
        &profile,
        &generation,
        &selected_root,
        &receipt,
        &receipt_bytes,
    )?;
    let (opened, evidence) = open_p3_index_generation(&profile, &generation, &selected_root)?;
    Ok((
        opened,
        evidence,
        level4.current_generation,
        generation.manifest_hash,
    ))
}

pub fn open_p3_index_generation(
    profile: &P3Profile,
    generation: &VerifiedGeneration,
    index_root: &Path,
) -> Result<(OpenedIndex, P3IndexEvidence), IndexError> {
    let (verified, corpus) = load_verified_p3_generation(profile, generation)?;
    let opened = open_generation(&corpus, index_root)?;
    let evidence = p3_evidence(profile, &verified, &opened.manifest, index_root)?;
    Ok((opened, evidence))
}

pub fn reindex_receipt_bytes(receipt: &ReindexReceipt) -> Result<Vec<u8>, IndexError> {
    let mut bytes = serde_json::to_vec_pretty(receipt).map_err(|_| IndexError::ReceiptInvalid)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn verify_reindex_receipt(
    profile: &P3Profile,
    generation: &VerifiedGeneration,
    index_root: &Path,
    receipt: &ReindexReceipt,
    receipt_bytes: &[u8],
) -> Result<(), IndexError> {
    verify_reindex_receipt_evidence(profile, generation, receipt, receipt_bytes)?;
    let (_, evidence) = open_p3_index_generation(profile, generation, index_root)?;
    if evidence.receipt != *receipt || evidence.receipt_bytes != receipt_bytes {
        return Err(IndexError::ReceiptInvalid);
    }
    Ok(())
}

pub fn verify_reindex_receipt_evidence(
    profile: &P3Profile,
    generation: &VerifiedGeneration,
    receipt: &ReindexReceipt,
    receipt_bytes: &[u8],
) -> Result<(), IndexError> {
    if reindex_receipt_bytes(receipt)? != receipt_bytes {
        return Err(IndexError::ReceiptInvalid);
    }
    let (verified, corpus) = load_verified_p3_generation(profile, generation)?;
    let (_, graph_hash, _) = validate_corpus_commitments(&corpus)?;
    let manifest = manifest_for(&corpus, graph_hash);
    let semantic_manifest_hash =
        blake3::hash(&canonical_manifest_bytes(&manifest).map_err(|_| IndexError::ReceiptInvalid)?)
            .to_hex()
            .to_string();
    let expected = ReindexReceipt {
        schema_version: "sovmem-reindex-receipt-v1".to_owned(),
        verdict_id: verified.manifest.applied_verdict_id.clone(),
        generation_id: verified.manifest.generation_id.clone(),
        profile_hash: profile.hash.clone(),
        manifest_hash: verified.manifest_hash.clone(),
        bundle_hash: verified.bundle_hash.clone(),
        documents_hash: manifest.documents_hash,
        documents: manifest.documents,
        graph_nodes: manifest.graph_nodes,
        graph_edges: manifest.graph_edges,
        external_typed_links: manifest.external_typed_links,
        graph_hash: manifest.graph_hash,
        index_schema: manifest.index_schema,
        tokenizer: manifest.tokenizer,
        ranking: manifest.ranking,
        tantivy_version: manifest.tantivy_version,
        petgraph_version: manifest.petgraph_version,
        index_config_hash: manifest.config_hash,
        semantic_manifest_hash,
    };
    if expected != *receipt {
        return Err(IndexError::ReceiptInvalid);
    }
    Ok(())
}

pub fn p3_evidence(
    profile: &P3Profile,
    generation: &VerifiedGeneration,
    manifest: &IndexManifest,
    index_root: &Path,
) -> Result<P3IndexEvidence, IndexError> {
    if manifest.input_kind != Some(IndexInputKind::P3Canon)
        || manifest.store_id.as_deref() != Some(P3_STORE_ID)
        || manifest.generation_id.as_deref() != Some(generation.manifest.generation_id.as_str())
        || manifest.input_manifest_hash.as_deref() != Some(generation.manifest_hash.as_str())
    {
        return Err(IndexError::InputMismatch);
    }
    let receipt = reindex_receipt(profile, generation, manifest, index_root)?;
    let semantic_manifest_hash = receipt.semantic_manifest_hash.clone();
    let receipt_bytes = reindex_receipt_bytes(&receipt)?;
    let receipt_hash = blake3::hash(&receipt_bytes).to_hex().to_string();
    Ok(P3IndexEvidence {
        input_kind: IndexInputKind::P3Canon.as_str(),
        store_id: manifest.store_id.clone(),
        generation_id: manifest.generation_id.clone(),
        manifest_hash: manifest.input_manifest_hash.clone(),
        documents_hash: manifest.documents_hash.clone(),
        graph_hash: manifest.graph_hash.clone(),
        documents: manifest.documents,
        graph_nodes: manifest.graph_nodes,
        graph_edges: manifest.graph_edges,
        external_typed_links: manifest.external_typed_links,
        config_hash: manifest.config_hash.clone(),
        semantic_manifest_hash,
        receipt,
        receipt_bytes,
        receipt_hash,
    })
}

fn reindex_receipt(
    profile: &P3Profile,
    generation: &VerifiedGeneration,
    manifest: &IndexManifest,
    index_root: &Path,
) -> Result<ReindexReceipt, IndexError> {
    let semantic_manifest_hash = blake3::hash(
        &fs::read(index_root.join("index-manifest.json"))
            .map_err(|_| IndexError::ManifestInvalid)?,
    )
    .to_hex()
    .to_string();
    Ok(ReindexReceipt {
        schema_version: "sovmem-reindex-receipt-v1".to_owned(),
        verdict_id: generation.manifest.applied_verdict_id.clone(),
        generation_id: generation.manifest.generation_id.clone(),
        profile_hash: profile.hash.clone(),
        manifest_hash: generation.manifest_hash.clone(),
        bundle_hash: generation.bundle_hash.clone(),
        documents_hash: manifest.documents_hash.clone(),
        documents: manifest.documents,
        graph_nodes: manifest.graph_nodes,
        graph_edges: manifest.graph_edges,
        external_typed_links: manifest.external_typed_links,
        graph_hash: manifest.graph_hash.clone(),
        index_schema: manifest.index_schema.clone(),
        tokenizer: manifest.tokenizer.clone(),
        ranking: manifest.ranking.clone(),
        tantivy_version: manifest.tantivy_version.clone(),
        petgraph_version: manifest.petgraph_version.clone(),
        index_config_hash: manifest.config_hash.clone(),
        semantic_manifest_hash,
    })
}

fn build_index_for_platform(
    request: IndexRequest,
    publish_supported: bool,
) -> Result<IndexSummary, IndexError> {
    let corpus = load_verified_corpus(&request.profile_path, &request.bundle_root)?;
    let bundle_root = fs::canonicalize(&request.bundle_root).map_err(|_| IndexError::BundleRead)?;
    build_verified_index_for_platform(
        &corpus,
        &bundle_root,
        &request.index_root,
        publish_supported,
    )
}

pub fn build_verified_index_for_platform(
    corpus: &VerifiedCorpus,
    bundle_root: &Path,
    requested_index_root: &Path,
    publish_supported: bool,
) -> Result<IndexSummary, IndexError> {
    let index_root = resolve_index_root(requested_index_root)?;
    ensure_roots_separate(bundle_root, &index_root)?;
    ensure_publish_supported(publish_supported)?;
    create_index_root(&index_root)?;

    let current = inspect_current(corpus, &index_root);
    let (inactive_slot, replaced_invalid_current) = match current {
        CurrentState::Absent => ("slot-a", false),
        CurrentState::Valid("slot-a") => ("slot-b", false),
        CurrentState::Valid("slot-b") => ("slot-a", false),
        CurrentState::Invalid(Some("slot-a")) => ("slot-b", true),
        CurrentState::Invalid(Some("slot-b")) => ("slot-a", true),
        CurrentState::Invalid(None) => ("slot-a", true),
        CurrentState::Valid(_) | CurrentState::Invalid(Some(_)) => {
            return Err(IndexError::CurrentInvalid)
        }
    };

    let mut staging = StagingGeneration::create(&index_root)?;
    build_generation(corpus, &staging.path)?;
    let opened = open_generation(corpus, &staging.path).map_err(|_| IndexError::SelfCheck)?;
    let manifest = opened.manifest.clone();
    drop(opened);

    install_inactive_generation(&mut staging, &index_root, inactive_slot)?;
    if replaced_invalid_current {
        remove_invalid_current_directory(&index_root)?;
    }
    publish_current_pointer(&index_root, inactive_slot)?;
    cleanup_stale_building_directories(&index_root);

    Ok(IndexSummary {
        documents: manifest.documents,
        graph_nodes: manifest.graph_nodes,
        graph_edges: manifest.graph_edges,
        external_typed_links: manifest.external_typed_links,
        bundle_hash: manifest.bundle_hash,
        index_config: manifest.config_hash,
        replaced_invalid_current,
    })
}

#[allow(dead_code)]
pub fn open_current_index(
    profile_path: &Path,
    bundle_root: &Path,
    index_root: &Path,
) -> Result<OpenedIndex, IndexError> {
    let corpus = load_verified_corpus(profile_path, bundle_root)?;
    let bundle_root = fs::canonicalize(bundle_root).map_err(|_| IndexError::BundleRead)?;
    let index_root = resolve_index_root(index_root)?;
    ensure_roots_separate(&bundle_root, &index_root)?;
    open_selected_generation(&corpus, &index_root)
}

pub fn ensure_publish_supported(supported: bool) -> Result<(), IndexError> {
    if supported {
        Ok(())
    } else {
        Err(IndexError::PublishUnsupported)
    }
}

pub fn parse_current(bytes: &[u8]) -> Result<&'static str, IndexError> {
    match bytes {
        b"slot-a\n" => Ok("slot-a"),
        b"slot-b\n" => Ok("slot-b"),
        _ => Err(IndexError::CurrentInvalid),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrentPointer {
    Absent,
    Invalid,
    Exact(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreCurrentPointer {
    Absent,
    Invalid,
    Exact(String),
}

pub fn parse_store_current(bytes: &[u8]) -> Option<String> {
    let value = bytes.strip_suffix(b"\n")?;
    let token = value.strip_prefix(b"slot-")?;
    if token.len() != 32
        || !token
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return None;
    }
    String::from_utf8(value.to_vec()).ok()
}

pub fn read_store_current_pointer(index_root: &Path) -> StoreCurrentPointer {
    let current = index_root.join("CURRENT");
    let metadata = match fs::symlink_metadata(&current) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return StoreCurrentPointer::Absent
        }
        Err(_) => return StoreCurrentPointer::Invalid,
    };
    if !metadata.file_type().is_file() {
        return StoreCurrentPointer::Invalid;
    }
    match fs::read(current)
        .ok()
        .and_then(|bytes| parse_store_current(&bytes))
    {
        Some(slot) => StoreCurrentPointer::Exact(slot),
        None => StoreCurrentPointer::Invalid,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrentState {
    Absent,
    Valid(&'static str),
    Invalid(Option<&'static str>),
}

pub fn read_current_pointer(index_root: &Path) -> CurrentPointer {
    let current = index_root.join("CURRENT");
    let metadata = match fs::symlink_metadata(&current) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return CurrentPointer::Absent,
        Err(_) => return CurrentPointer::Invalid,
    };
    if !metadata.file_type().is_file() || metadata.len() != b"slot-a\n".len() as u64 {
        return CurrentPointer::Invalid;
    }
    match fs::read(current)
        .ok()
        .and_then(|bytes| parse_current(&bytes).ok())
    {
        Some(slot) => CurrentPointer::Exact(slot),
        None => CurrentPointer::Invalid,
    }
}

pub fn inspect_current(corpus: &VerifiedCorpus, index_root: &Path) -> CurrentState {
    match read_current_pointer(index_root) {
        CurrentPointer::Absent => CurrentState::Absent,
        CurrentPointer::Invalid => CurrentState::Invalid(None),
        CurrentPointer::Exact(slot) => match open_slot(corpus, index_root, slot) {
            Ok(opened) => {
                drop(opened);
                CurrentState::Valid(slot)
            }
            Err(_) => CurrentState::Invalid(Some(slot)),
        },
    }
}

pub fn open_selected_generation(
    corpus: &VerifiedCorpus,
    index_root: &Path,
) -> Result<OpenedIndex, IndexError> {
    let slot = match read_current_pointer(index_root) {
        CurrentPointer::Exact(slot) => slot,
        CurrentPointer::Absent | CurrentPointer::Invalid => return Err(IndexError::CurrentInvalid),
    };
    open_slot(corpus, index_root, slot)
}

fn open_slot(
    corpus: &VerifiedCorpus,
    index_root: &Path,
    slot: &'static str,
) -> Result<OpenedIndex, IndexError> {
    let generation = index_root.join(slot);
    let metadata = fs::symlink_metadata(&generation).map_err(|_| IndexError::CurrentInvalid)?;
    if !metadata.file_type().is_dir() {
        return Err(IndexError::CurrentInvalid);
    }
    open_generation(corpus, &generation)
}

fn resolve_index_root(requested: &Path) -> Result<PathBuf, IndexError> {
    let normalized = normalize_index_request(requested)?;
    let absolute = if normalized.is_absolute() {
        normalized
    } else {
        std::env::current_dir()
            .map_err(|_| IndexError::OutputInvalid)?
            .join(normalized)
    };

    let mut ancestor = absolute.as_path();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if !metadata.file_type().is_dir() {
                    return Err(IndexError::OutputInvalid);
                }
                let canonical =
                    fs::canonicalize(ancestor).map_err(|_| IndexError::OutputInvalid)?;
                let remainder = absolute
                    .strip_prefix(ancestor)
                    .map_err(|_| IndexError::OutputInvalid)?;
                return Ok(canonical.join(remainder));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or(IndexError::OutputInvalid)?;
            }
            Err(_) => return Err(IndexError::OutputInvalid),
        }
    }
}

fn normalize_index_request(path: &Path) -> Result<PathBuf, IndexError> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => return Err(IndexError::OutputInvalid),
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

fn ensure_roots_separate(bundle_root: &Path, index_root: &Path) -> Result<(), IndexError> {
    if index_root == bundle_root
        || index_root.starts_with(bundle_root)
        || bundle_root.starts_with(index_root)
    {
        Err(IndexError::OutputInvalid)
    } else {
        Ok(())
    }
}

fn create_index_root(index_root: &Path) -> Result<(), IndexError> {
    fs::create_dir_all(index_root).map_err(|_| IndexError::OutputInvalid)?;
    let metadata = fs::symlink_metadata(index_root).map_err(|_| IndexError::OutputInvalid)?;
    if !metadata.file_type().is_dir()
        || fs::canonicalize(index_root).map_err(|_| IndexError::OutputInvalid)? != index_root
    {
        return Err(IndexError::OutputInvalid);
    }
    Ok(())
}

fn create_hidden_p3_index_root(
    generation_root: &Path,
    index_root: &Path,
) -> Result<PathBuf, IndexError> {
    let Some(name) = index_root.file_name().and_then(|value| value.to_str()) else {
        return Err(IndexError::OutputInvalid);
    };
    let Some(parent) = index_root.parent() else {
        return Err(IndexError::OutputInvalid);
    };
    let parent_metadata = fs::symlink_metadata(parent).map_err(|_| IndexError::OutputInvalid)?;
    if !name.starts_with('.')
        || index_root.exists()
        || parent_metadata.file_type().is_symlink()
        || !parent_metadata.is_dir()
    {
        return Err(IndexError::OutputInvalid);
    }
    let parent = fs::canonicalize(parent).map_err(|_| IndexError::OutputInvalid)?;
    let index_root = parent.join(name);
    if index_root.starts_with(generation_root) || generation_root.starts_with(&index_root) {
        return Err(IndexError::OutputInvalid);
    }
    fs::create_dir(&index_root).map_err(|_| IndexError::Build)?;
    Ok(index_root)
}

fn sync_index_tree(root: &Path) -> Result<(), IndexError> {
    let mut entries = fs::read_dir(root)
        .map_err(|_| IndexError::Build)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| IndexError::Build)?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let metadata = entry.metadata().map_err(|_| IndexError::Build)?;
        if metadata.is_dir() {
            sync_index_tree(&entry.path())?;
        } else if metadata.is_file() {
            File::open(entry.path())
                .and_then(|file| file.sync_all())
                .map_err(|_| IndexError::Build)?;
        } else {
            return Err(IndexError::Build);
        }
    }
    sync_directory(root).map_err(|_| IndexError::Build)
}

struct StagingGeneration {
    path: PathBuf,
    armed: bool,
}

impl StagingGeneration {
    fn create(index_root: &Path) -> Result<Self, IndexError> {
        for _ in 0..1_000 {
            let sequence = NEXT_BUILD_ID.fetch_add(1, Ordering::Relaxed);
            let path = index_root.join(format!(".building-{}-{sequence}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path, armed: true }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(IndexError::Build),
            }
        }
        Err(IndexError::Build)
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StagingGeneration {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn install_inactive_generation(
    staging: &mut StagingGeneration,
    index_root: &Path,
    inactive_slot: &'static str,
) -> Result<(), IndexError> {
    let target = index_root.join(inactive_slot);
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            fs::remove_dir_all(&target).map_err(|_| IndexError::Publish)?;
        }
        Ok(_) => return Err(IndexError::Publish),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(IndexError::Publish),
    }
    fs::rename(&staging.path, &target).map_err(|_| IndexError::Publish)?;
    staging.disarm();
    sync_directory(index_root).map_err(|_| IndexError::Publish)
}

fn install_store_generation(
    staging: &mut StagingGeneration,
    index_root: &Path,
    slot: &str,
) -> Result<(), IndexError> {
    let target = index_root.join(slot);
    if fs::symlink_metadata(&target).is_ok() {
        return Err(IndexError::Publish);
    }
    fs::rename(&staging.path, &target).map_err(|_| IndexError::Publish)?;
    staging.disarm();
    sync_directory(index_root).map_err(|_| IndexError::Publish)
}

fn remove_invalid_current_directory(index_root: &Path) -> Result<(), IndexError> {
    let current = index_root.join("CURRENT");
    match fs::symlink_metadata(&current) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            fs::remove_dir_all(&current).map_err(|_| IndexError::Publish)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(IndexError::Publish),
    }
}

pub fn publish_current_pointer(
    index_root: &Path,
    inactive_slot: &'static str,
) -> Result<(), IndexError> {
    publish_current_pointer_with(index_root, inactive_slot, |from, to| fs::rename(from, to))
}

pub fn publish_store_current_pointer(index_root: &Path, slot: &str) -> Result<(), IndexError> {
    let next = index_root.join(format!("CURRENT.next-{}", &slot[5..]));
    let pointer_result = (|| {
        let mut pointer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&next)
            .map_err(|_| IndexError::Publish)?;
        pointer
            .write_all(format!("{slot}\n").as_bytes())
            .map_err(|_| IndexError::Publish)?;
        pointer.flush().map_err(|_| IndexError::Publish)?;
        pointer.sync_all().map_err(|_| IndexError::Publish)?;
        drop(pointer);
        fs::rename(&next, index_root.join("CURRENT")).map_err(|_| IndexError::Publish)
    })();
    if pointer_result.is_err() {
        let _ = fs::remove_file(&next);
    }
    pointer_result
}

pub fn publish_current_pointer_with(
    index_root: &Path,
    inactive_slot: &'static str,
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<(), IndexError> {
    let next = index_root.join("CURRENT.next");
    match fs::symlink_metadata(&next) {
        Ok(metadata) if metadata.file_type().is_file() => {
            fs::remove_file(&next).map_err(|_| IndexError::Publish)?;
        }
        Ok(_) => return Err(IndexError::Publish),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(IndexError::Publish),
    }

    let pointer_result = (|| {
        let mut pointer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&next)
            .map_err(|_| IndexError::Publish)?;
        pointer
            .write_all(format!("{inactive_slot}\n").as_bytes())
            .map_err(|_| IndexError::Publish)?;
        pointer.flush().map_err(|_| IndexError::Publish)?;
        pointer.sync_all().map_err(|_| IndexError::Publish)?;
        drop(pointer);
        rename(&next, &index_root.join("CURRENT")).map_err(|_| IndexError::Publish)
    })();
    if pointer_result.is_err() {
        let _ = fs::remove_file(&next);
    }
    pointer_result
}

fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

fn cleanup_stale_building_directories(index_root: &Path) {
    let Ok(entries) = fs::read_dir(index_root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(".building-")
            && entry.file_type().is_ok_and(|file_type| file_type.is_dir())
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct IndexFields {
    pub id: Field,
    pub title: Field,
    pub body: Field,
    pub layer: Field,
    pub core_type: Field,
    pub tags: Field,
    pub provenance: Field,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct IndexSchemaDescriptor {
    pub schema_id: &'static str,
    pub tokenizer: &'static str,
    pub ranking: &'static str,
    pub fields: [&'static str; 7],
    pub token_limit_bytes: usize,
    pub title_boost: &'static str,
    pub tags_boost: &'static str,
    pub body_boost: &'static str,
    pub tantivy_version: &'static str,
    pub petgraph_version: &'static str,
}

pub fn index_schema_descriptor() -> IndexSchemaDescriptor {
    IndexSchemaDescriptor {
        schema_id: INDEX_SCHEMA_ID,
        tokenizer: TOKENIZER_ID,
        ranking: RANKING_ID,
        fields: ["id", "title", "body", "layer", "type", "tags", "provenance"],
        token_limit_bytes: TOKEN_LIMIT_BYTES,
        title_boost: "3.0",
        tags_boost: "2.0",
        body_boost: "1.0",
        tantivy_version: TANTIVY_VERSION,
        petgraph_version: PETGRAPH_VERSION,
    }
}

#[allow(dead_code)]
pub fn build_schema() -> (Schema, IndexFields) {
    let searchable = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(TOKENIZER_ID)
            .set_index_option(IndexRecordOption::WithFreqs),
    );
    let mut builder = Schema::builder();
    let id = builder.add_text_field("id", STRING | STORED);
    let title = builder.add_text_field("title", searchable.clone().set_stored());
    let body = builder.add_text_field("body", searchable.clone());
    let layer = builder.add_text_field("layer", STRING | STORED);
    let core_type = builder.add_text_field("type", STRING | STORED);
    let tags = builder.add_text_field("tags", searchable.set_stored());
    let provenance = builder.add_text_field("provenance", STRING | STORED);
    let schema = builder.build();
    (
        schema,
        IndexFields {
            id,
            title,
            body,
            layer,
            core_type,
            tags,
            provenance,
        },
    )
}

#[allow(dead_code)]
pub fn register_tokenizer(index: &Index) {
    let analyzer = TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(TOKEN_LIMIT_BYTES))
        .filter(LowerCaser)
        .build();
    index.tokenizers().register(TOKENIZER_ID, analyzer);
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IndexManifest {
    pub schema_version: String,
    pub index_schema: String,
    pub tokenizer: String,
    pub ranking: String,
    pub sovmem_version: String,
    pub tantivy_version: String,
    pub petgraph_version: String,
    pub profile: String,
    pub context_namespace: String,
    pub source_commit: String,
    pub bundle_hash: String,
    pub documents_hash: String,
    pub graph_hash: String,
    pub documents: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub external_typed_links: usize,
    pub config_hash: String,
    #[serde(default)]
    pub input_kind: Option<IndexInputKind>,
    #[serde(default)]
    pub store_id: Option<String>,
    #[serde(default)]
    pub generation_id: Option<String>,
    #[serde(default)]
    pub input_manifest_hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexMetadata {
    pub id: String,
    pub title: String,
    pub layer: String,
    pub core_type: String,
    pub tags: Vec<String>,
    pub provenance: String,
}

type CorpusCommitments = (Vec<u8>, String, BTreeMap<String, IndexMetadata>);

#[allow(dead_code)]
pub struct OpenedIndex {
    pub manifest: IndexManifest,
    pub index: Index,
    pub reader: IndexReader,
    pub fields: IndexFields,
    pub graph: GraphProjection,
    pub metadata: BTreeMap<String, IndexMetadata>,
    // Carried from the freshly verified corpus so evaluation can report the
    // measured core/research split instead of restating a compiled-in constant.
    pub core: usize,
    pub research: usize,
}

impl std::fmt::Debug for OpenedIndex {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenedIndex")
            .field("manifest", &self.manifest)
            .field("metadata_count", &self.metadata.len())
            .finish_non_exhaustive()
    }
}

#[allow(dead_code)]
pub fn retrieval_config_hash() -> String {
    let mut hasher = blake3::Hasher::new();
    let descriptor = index_schema_descriptor();
    let token_limit = TOKEN_LIMIT_BYTES.to_string();
    for value in [
        "sovmem-retrieval-config-v1",
        INDEX_SCHEMA_ID,
        TOKENIZER_ID,
        RANKING_ID,
        INDEX_MANIFEST_SCHEMA,
        super::GRAPH_SCHEMA,
        "id",
        "indexed",
        "stored",
        "raw-tokenizer",
        "basic",
        "fieldnorms-disabled",
        "title",
        "indexed",
        "stored",
        TOKENIZER_ID,
        "with-freqs",
        "fieldnorms-enabled",
        "body",
        "indexed",
        "not-stored",
        TOKENIZER_ID,
        "with-freqs",
        "fieldnorms-enabled",
        "layer",
        "indexed",
        "stored",
        "raw-tokenizer",
        "basic",
        "fieldnorms-disabled",
        "type",
        "indexed",
        "stored",
        "raw-tokenizer",
        "basic",
        "fieldnorms-disabled",
        "tags",
        "indexed",
        "stored",
        TOKENIZER_ID,
        "with-freqs",
        "fieldnorms-enabled",
        "provenance",
        "indexed",
        "stored",
        "raw-tokenizer",
        "basic",
        "fieldnorms-disabled",
        "simple-tokenizer",
        "remove-long-filter",
        token_limit.as_str(),
        "lower-caser",
        "title",
        descriptor.title_boost,
        "tags",
        descriptor.tags_boost,
        "body",
        descriptor.body_boost,
        "boolean-should",
        "score-desc-id-asc",
        "one-hop-separated",
        "tantivy",
        TANTIVY_VERSION,
        "petgraph",
        PETGRAPH_VERSION,
    ] {
        frame(&mut hasher, value.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

#[allow(dead_code)]
pub fn build_generation(
    corpus: &VerifiedCorpus,
    generation_root: &Path,
) -> Result<IndexManifest, IndexError> {
    let (graph_bytes, graph_hash, _) = validate_corpus_commitments(corpus)?;
    let tantivy_root = generation_root.join("tantivy");
    fs::create_dir_all(&tantivy_root).map_err(|_| IndexError::Build)?;

    let (schema, fields) = build_schema();
    let index = Index::create_in_dir(&tantivy_root, schema).map_err(|_| IndexError::Build)?;
    register_tokenizer(&index);
    let mut writer = index
        .writer_with_num_threads(1, 50_000_000)
        .map_err(|_| IndexError::Build)?;
    for source in &corpus.documents {
        let mut document = tantivy::TantivyDocument::new();
        document.add_text(fields.id, &source.id);
        document.add_text(fields.title, &source.title);
        document.add_text(fields.body, &source.body);
        document.add_text(fields.layer, &source.layer);
        document.add_text(fields.core_type, &source.core_type);
        for tag in &source.tags {
            document.add_text(fields.tags, tag);
        }
        document.add_text(fields.provenance, &source.provenance);
        writer
            .add_document(document)
            .map_err(|_| IndexError::Build)?;
    }
    writer.commit().map_err(|_| IndexError::Build)?;
    writer
        .wait_merging_threads()
        .map_err(|_| IndexError::Build)?;

    fs::write(generation_root.join("graph.json"), &graph_bytes).map_err(|_| IndexError::Build)?;
    let manifest = manifest_for(corpus, graph_hash);
    let manifest_bytes = canonical_manifest_bytes(&manifest).map_err(|_| IndexError::Build)?;
    fs::write(generation_root.join("index-manifest.json"), manifest_bytes)
        .map_err(|_| IndexError::Build)?;
    Ok(manifest)
}

#[allow(dead_code)]
pub fn open_generation(
    corpus: &VerifiedCorpus,
    generation_root: &Path,
) -> Result<OpenedIndex, IndexError> {
    let (expected_graph_bytes, graph_hash, expected_metadata) =
        validate_corpus_commitments(corpus)?;
    let manifest_bytes = fs::read(generation_root.join("index-manifest.json"))
        .map_err(|_| IndexError::ManifestInvalid)?;
    let manifest: IndexManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|_| IndexError::ManifestInvalid)?;
    let canonical_manifest =
        canonical_manifest_bytes(&manifest).map_err(|_| IndexError::ManifestInvalid)?;
    if canonical_manifest != manifest_bytes {
        return Err(IndexError::ManifestInvalid);
    }
    validate_manifest(&manifest, &manifest_for(corpus, graph_hash))?;

    let graph_bytes =
        fs::read(generation_root.join("graph.json")).map_err(|_| IndexError::GraphInvalid)?;
    if graph_bytes != expected_graph_bytes
        || blake3::hash(&graph_bytes).to_hex().as_str() != manifest.graph_hash
    {
        return Err(IndexError::GraphInvalid);
    }
    let graph = GraphProjection::from_bytes(&graph_bytes).map_err(|_| IndexError::GraphInvalid)?;

    let index = Index::open_in_dir(generation_root.join("tantivy"))
        .map_err(|_| IndexError::TantivyInvalid)?;
    let (expected_schema, fields) = build_schema();
    if index.schema() != expected_schema {
        return Err(IndexError::SchemaMismatch);
    }
    register_tokenizer(&index);
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()
        .map_err(|_| IndexError::TantivyInvalid)?;
    let searcher = reader.searcher();
    if searcher.num_docs() != corpus.documents.len() as u64 {
        return Err(IndexError::DocumentSetInvalid);
    }
    let documents = searcher
        .search(&AllQuery, &TopDocs::with_limit(corpus.documents.len()))
        .map_err(|_| IndexError::TantivyInvalid)?;
    if documents.len() != corpus.documents.len() {
        return Err(IndexError::DocumentSetInvalid);
    }

    let mut metadata = BTreeMap::new();
    for (_, address) in documents {
        let document: tantivy::TantivyDocument = searcher
            .doc(address)
            .map_err(|_| IndexError::TantivyInvalid)?;
        let stored = metadata_from_document(&document, fields)?;
        if metadata.insert(stored.id.clone(), stored).is_some() {
            return Err(IndexError::DocumentSetInvalid);
        }
    }
    if metadata != expected_metadata {
        return Err(IndexError::DocumentSetInvalid);
    }

    Ok(OpenedIndex {
        manifest,
        index,
        reader,
        fields,
        graph,
        metadata,
        core: corpus.core,
        research: corpus.research,
    })
}

#[allow(dead_code)]
fn validate_corpus_commitments(corpus: &VerifiedCorpus) -> Result<CorpusCommitments, IndexError> {
    if canonical_document_hash(&corpus.documents) != corpus.documents_hash {
        return Err(IndexError::DocumentSetInvalid);
    }
    if corpus.documents.windows(2).any(|pair| {
        pair[0]
            .source_path
            .as_bytes()
            .cmp(pair[1].source_path.as_bytes())
            .then_with(|| pair[0].id.as_bytes().cmp(pair[1].id.as_bytes()))
            .is_ge()
    }) {
        return Err(IndexError::DocumentSetInvalid);
    }

    let mut metadata = BTreeMap::new();
    for document in &corpus.documents {
        if document.id.is_empty()
            || document
                .tags
                .windows(2)
                .any(|pair| pair[0].as_bytes() >= pair[1].as_bytes())
        {
            return Err(IndexError::DocumentSetInvalid);
        }
        let stored = IndexMetadata {
            id: document.id.clone(),
            title: document.title.clone(),
            layer: document.layer.clone(),
            core_type: document.core_type.clone(),
            tags: document.tags.clone(),
            provenance: document.provenance.clone(),
        };
        if metadata.insert(document.id.clone(), stored).is_some() {
            return Err(IndexError::DocumentSetInvalid);
        }
    }

    if corpus.graph.bundle_hash != corpus.bundle_hash
        || corpus
            .graph
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .ne(corpus.documents.iter().map(|document| document.id.as_str()))
    {
        return Err(IndexError::GraphInvalid);
    }
    let graph_bytes = corpus
        .graph
        .to_bytes()
        .map_err(|_| IndexError::GraphInvalid)?;
    let graph_hash = corpus
        .graph
        .blake3()
        .map_err(|_| IndexError::GraphInvalid)?;
    if graph_hash != blake3::hash(&graph_bytes).to_hex().as_str() {
        return Err(IndexError::GraphInvalid);
    }
    Ok((graph_bytes, graph_hash, metadata))
}

#[allow(dead_code)]
fn manifest_for(corpus: &VerifiedCorpus, graph_hash: String) -> IndexManifest {
    IndexManifest {
        schema_version: INDEX_MANIFEST_SCHEMA_V2.into(),
        index_schema: INDEX_SCHEMA_ID.into(),
        tokenizer: TOKENIZER_ID.into(),
        ranking: RANKING_ID.into(),
        sovmem_version: env!("CARGO_PKG_VERSION").into(),
        tantivy_version: TANTIVY_VERSION.into(),
        petgraph_version: PETGRAPH_VERSION.into(),
        profile: corpus.profile.clone(),
        context_namespace: corpus.context_namespace.clone(),
        source_commit: corpus.source_commit.clone(),
        bundle_hash: corpus.bundle_hash.clone(),
        documents_hash: corpus.documents_hash.clone(),
        graph_hash,
        documents: corpus.documents.len(),
        graph_nodes: corpus.graph.nodes.len(),
        graph_edges: corpus.graph.edges.len(),
        external_typed_links: corpus.graph.external_typed_links,
        config_hash: retrieval_config_hash(),
        input_kind: Some(corpus.input.kind.clone()),
        store_id: corpus.input.store_id.clone(),
        generation_id: corpus.input.generation_id.clone(),
        input_manifest_hash: Some(corpus.input.manifest_hash.clone()),
    }
}

#[allow(dead_code)]
pub fn canonical_manifest_bytes(manifest: &IndexManifest) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec_pretty(manifest)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[allow(dead_code)]
fn validate_manifest(actual: &IndexManifest, expected: &IndexManifest) -> Result<(), IndexError> {
    let legacy_frozen = actual.schema_version == INDEX_MANIFEST_SCHEMA_V1;
    if !legacy_frozen && actual.schema_version != INDEX_MANIFEST_SCHEMA_V2 {
        return Err(IndexError::SchemaMismatch);
    }
    if actual.index_schema != expected.index_schema
        || actual.tokenizer != expected.tokenizer
        || actual.ranking != expected.ranking
        || actual.sovmem_version != expected.sovmem_version
        || actual.tantivy_version != expected.tantivy_version
        || actual.petgraph_version != expected.petgraph_version
        || actual.config_hash != expected.config_hash
    {
        return Err(IndexError::SchemaMismatch);
    }
    if legacy_frozen {
        if expected.input_kind != Some(IndexInputKind::FrozenCorpus)
            || actual.input_kind.is_some()
            || actual.store_id.is_some()
            || actual.generation_id.is_some()
            || actual.input_manifest_hash.is_some()
        {
            return Err(IndexError::InputMismatch);
        }
    } else if actual.input_kind != expected.input_kind
        || actual.store_id != expected.store_id
        || actual.generation_id != expected.generation_id
        || actual.input_manifest_hash != expected.input_manifest_hash
    {
        return Err(IndexError::InputMismatch);
    }
    if actual.profile != expected.profile
        || actual.context_namespace != expected.context_namespace
        || actual.source_commit != expected.source_commit
        || actual.bundle_hash != expected.bundle_hash
    {
        return Err(IndexError::BundleMismatch);
    }
    if actual.documents_hash != expected.documents_hash || actual.documents != expected.documents {
        return Err(IndexError::DocumentSetInvalid);
    }
    if actual.graph_hash != expected.graph_hash
        || actual.graph_nodes != expected.graph_nodes
        || actual.graph_edges != expected.graph_edges
        || actual.external_typed_links != expected.external_typed_links
    {
        return Err(IndexError::GraphInvalid);
    }
    Ok(())
}

#[allow(dead_code)]
fn metadata_from_document(
    document: &tantivy::TantivyDocument,
    fields: IndexFields,
) -> Result<IndexMetadata, IndexError> {
    Ok(IndexMetadata {
        id: one_stored_text(document, fields.id)?,
        title: one_stored_text(document, fields.title)?,
        layer: one_stored_text(document, fields.layer)?,
        core_type: one_stored_text(document, fields.core_type)?,
        tags: stored_texts(document, fields.tags)?,
        provenance: one_stored_text(document, fields.provenance)?,
    })
}

#[allow(dead_code)]
fn one_stored_text(
    document: &tantivy::TantivyDocument,
    field: Field,
) -> Result<String, IndexError> {
    let values = stored_texts(document, field)?;
    if values.len() != 1 {
        return Err(IndexError::DocumentSetInvalid);
    }
    Ok(values.into_iter().next().expect("length checked"))
}

#[allow(dead_code)]
fn stored_texts(
    document: &tantivy::TantivyDocument,
    field: Field,
) -> Result<Vec<String>, IndexError> {
    document
        .get_all(field)
        .map(|value| match OwnedValue::from(value) {
            OwnedValue::Str(text) => Ok(text),
            _ => Err(IndexError::DocumentSetInvalid),
        })
        .collect()
}
