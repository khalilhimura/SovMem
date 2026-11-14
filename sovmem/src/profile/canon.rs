use super::{
    convert_links, parse_source_document, render_document, render_index, validate_document,
    verify_bundle, GeneratedDocument, GeneratedMetadata, JudgmentDocument, LinkRelation, P3Profile,
    ProposalOperation, SourceDocument, VerificationLevel, VerificationOptions, P3_STORE_ID,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const CANON_MANIFEST_SCHEMA: &str = "sovmem-canon-manifest-v1";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OriginKind {
    Seed,
    Verdict,
}

impl OriginKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Seed => "seed",
            Self::Verdict => "verdict",
        }
    }
}

impl fmt::Display for OriginKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ParentKind {
    Seed,
    Generation,
}

impl ParentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Seed => "seed",
            Self::Generation => "generation",
        }
    }
}

impl fmt::Display for ParentKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanonDocument {
    pub canonical_path: String,
    pub seed_source_path: Option<String>,
    pub output_path: String,
    pub id: String,
    pub resource: String,
    pub hash: String,
    pub origin_kind: OriginKind,
    pub seed_source_hash: Option<String>,
    pub proposal_id: Option<String>,
    pub verdict_id: Option<String>,
    pub previous_hash: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanonManifest {
    pub schema_version: String,
    pub store_id: String,
    pub profile_version: String,
    pub profile_hash: String,
    pub generation_id: String,
    pub parent_kind: ParentKind,
    pub parent_generation_id: Option<String>,
    pub parent_manifest_hash: String,
    pub seed_bundle_hash: String,
    pub seed_manifest_hash: String,
    pub policy_hash: String,
    pub applied_proposal_id: String,
    pub applied_verdict_id: String,
    pub documents: Vec<CanonDocument>,
    pub bundle_hash: String,
}

#[derive(Clone, Debug)]
pub struct VerifiedGeneration {
    pub root: PathBuf,
    pub manifest: CanonManifest,
    pub manifest_hash: String,
    pub bundle_hash: String,
    pub documents_hash: String,
    pub generated_documents: Vec<GeneratedDocument>,
}

impl VerifiedGeneration {
    pub fn applied_target_id(&self) -> Option<&str> {
        let mut targets = self.manifest.documents.iter().filter(|entry| {
            entry.proposal_id.as_deref() == Some(self.manifest.applied_proposal_id.as_str())
                && entry.verdict_id.as_deref() == Some(self.manifest.applied_verdict_id.as_str())
                && !entry.canonical_path.starts_with("judgments/")
        });
        let target = targets.next()?;
        if targets.next().is_some() {
            return None;
        }
        Some(&target.id)
    }
}

#[derive(Clone, Debug)]
pub struct AppliedGenerationInput {
    pub generation_id: String,
    pub proposal_id: String,
    pub verdict_id: String,
    pub policy_hash: String,
    pub operation: ProposalOperation,
    pub target_path: String,
    pub target_id: String,
    pub target_bytes: Vec<u8>,
    pub judgment: JudgmentDocument,
}

#[derive(Clone, Debug)]
struct ParentDocument {
    document: CanonDocument,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct GenerationPlan {
    parent_kind: ParentKind,
    parent_generation_id: Option<String>,
    parent_manifest_hash: String,
    seed_bundle_hash: String,
    seed_manifest_hash: String,
    parent_documents: Vec<ParentDocument>,
    parent_predecessor_hashes: BTreeMap<String, String>,
    input: AppliedGenerationInput,
}

impl GenerationPlan {
    pub fn from_seed(
        profile: &P3Profile,
        seed_root: &Path,
        input: AppliedGenerationInput,
    ) -> Result<Self, CanonError> {
        let seed_root = canonical_regular_directory(seed_root)?;
        let seed_profile = frozen_seed_profile(profile);
        let verified = verify_bundle(
            &seed_profile,
            &seed_root,
            VerificationOptions::level(VerificationLevel::Three),
        );
        if verified.has_errors() {
            return Err(CanonError::SeedInvalid);
        }
        let seed_manifest = verified.manifest.clone().ok_or(CanonError::SeedInvalid)?;
        let manifest_hash = raw_hash(&seed_root.join("manifest.json"))?;
        let source_hashes: BTreeMap<_, _> = seed_manifest
            .sources
            .iter()
            .map(|source| (source.source_path.as_str(), source.hash.as_str()))
            .collect();
        let mut rebound_documents = BTreeMap::new();
        let mut parent_predecessor_hashes = BTreeMap::new();
        for entry in &seed_manifest.documents {
            let seed_source_hash = source_hashes
                .get(entry.source_path.as_str())
                .ok_or(CanonError::SeedInvalid)?;
            let id = super::p3_canonical_document_id(
                &profile.config.context_namespace,
                &entry.source_path,
            )
            .ok_or(CanonError::SeedInvalid)?;
            let bytes = rebind_seed_document(
                profile,
                &entry.source_path,
                &read_regular_file(&seed_root.join(&entry.output_path))?,
            )?;
            parent_predecessor_hashes.insert(entry.source_path.clone(), entry.hash.clone());
            let document = document_from_bytes(profile, &entry.source_path, &bytes)?;
            if document.metadata.id != id
                || document.metadata.resource != format!("vault://{id}")
                || rebound_documents
                    .insert(
                        entry.source_path.clone(),
                        (entry, (*seed_source_hash).to_owned(), document),
                    )
                    .is_some()
            {
                return Err(CanonError::SeedInvalid);
            }
        }
        let normalized = convert_links(
            &profile.config,
            rebound_documents
                .values()
                .map(|(_, _, document)| document.clone())
                .collect(),
        )
        .map_err(|_| CanonError::LinkInvalid)?;
        if normalized.documents.len() != rebound_documents.len() {
            return Err(CanonError::SeedInvalid);
        }
        let mut normalized_by_path = normalized
            .documents
            .into_iter()
            .map(|document| (document.metadata.source_path.clone(), document))
            .collect::<BTreeMap<_, _>>();
        if normalized_by_path.len() != rebound_documents.len() {
            return Err(CanonError::SeedInvalid);
        }
        let mut parent_documents = Vec::with_capacity(rebound_documents.len());
        for (canonical_path, (entry, seed_source_hash, _)) in rebound_documents {
            let document = normalized_by_path
                .remove(&canonical_path)
                .ok_or(CanonError::SeedInvalid)?;
            let bytes = render_document(&document).map_err(|_| CanonError::SeedInvalid)?;
            parent_documents.push(ParentDocument {
                document: CanonDocument {
                    canonical_path,
                    seed_source_path: Some(entry.source_path.clone()),
                    output_path: entry.output_path.clone(),
                    id: document.metadata.id.clone(),
                    resource: document.metadata.resource.clone(),
                    hash: blake3::hash(&bytes).to_hex().to_string(),
                    origin_kind: OriginKind::Seed,
                    seed_source_hash: Some(seed_source_hash),
                    proposal_id: None,
                    verdict_id: None,
                    previous_hash: None,
                },
                bytes,
            });
        }
        if !normalized_by_path.is_empty() {
            return Err(CanonError::SeedInvalid);
        }
        validate_input(&profile.config.context_namespace, &input)?;
        Ok(Self {
            parent_kind: ParentKind::Seed,
            parent_generation_id: None,
            parent_manifest_hash: manifest_hash.clone(),
            seed_bundle_hash: verified.bundle_hash().to_owned(),
            seed_manifest_hash: manifest_hash,
            parent_documents,
            parent_predecessor_hashes,
            input,
        })
    }

    pub fn from_generation(
        profile: &P3Profile,
        parent: &VerifiedGeneration,
        input: AppliedGenerationInput,
    ) -> Result<Self, CanonError> {
        validate_input(&profile.config.context_namespace, &input)?;
        let mut parent_documents = Vec::with_capacity(parent.manifest.documents.len());
        let mut parent_predecessor_hashes = BTreeMap::new();
        for document in &parent.manifest.documents {
            parent_predecessor_hashes
                .insert(document.canonical_path.clone(), document.hash.clone());
            parent_documents.push(ParentDocument {
                document: document.clone(),
                bytes: read_regular_file(&parent.root.join(&document.output_path))?,
            });
        }
        Ok(Self {
            parent_kind: ParentKind::Generation,
            parent_generation_id: Some(parent.manifest.generation_id.clone()),
            parent_manifest_hash: parent.manifest_hash.clone(),
            seed_bundle_hash: parent.manifest.seed_bundle_hash.clone(),
            seed_manifest_hash: parent.manifest.seed_manifest_hash.clone(),
            parent_documents,
            parent_predecessor_hashes,
            input,
        })
    }

    pub fn generation_id(&self) -> &str {
        &self.input.generation_id
    }

    pub fn target_id(&self) -> &str {
        &self.input.target_id
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum CanonError {
    #[error("canonical generation could not be read")]
    Read,
    #[error("canonical generation path is unsafe")]
    PathUnsafe,
    #[error("canonical generation contains a symlink")]
    Symlink,
    #[error("canonical generation root shape is invalid")]
    RootInvalid,
    #[error("canonical generation manifest is invalid")]
    ManifestInvalid,
    #[error("canonical generation profile binding is invalid")]
    ProfileInvalid,
    #[error("canonical generation parent binding is invalid")]
    ParentInvalid,
    #[error("canonical generation document is invalid")]
    DocumentInvalid,
    #[error("canonical generation raw hashes are invalid")]
    HashMismatch,
    #[error("canonical generation index is invalid")]
    IndexMismatch,
    #[error("canonical generation lineage is invalid")]
    LineageInvalid,
    #[error("canonical generation typed links are invalid")]
    LinkInvalid,
    #[error("canonical generation input is invalid")]
    InputInvalid,
    #[error("canonical seed is invalid")]
    SeedInvalid,
    #[error("canonical generation staging directory is invalid")]
    StagingInvalid,
    #[error("canonical generation write failed")]
    Write,
    #[error("verification level is invalid for canonical generation")]
    LevelInvalid,
}

impl CanonError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Read => "canon.read",
            Self::PathUnsafe => "canon.path_unsafe",
            Self::Symlink => "canon.symlink",
            Self::RootInvalid => "canon.root_invalid",
            Self::ManifestInvalid => "canon.manifest_invalid",
            Self::ProfileInvalid => "canon.profile_invalid",
            Self::ParentInvalid => "canon.parent_invalid",
            Self::DocumentInvalid => "canon.document_invalid",
            Self::HashMismatch => "canon.hash_mismatch",
            Self::IndexMismatch => "canon.index_mismatch",
            Self::LineageInvalid => "canon.lineage_invalid",
            Self::LinkInvalid => "canon.link_invalid",
            Self::InputInvalid => "canon.input_invalid",
            Self::SeedInvalid => "canon.seed_invalid",
            Self::StagingInvalid => "canon.staging_invalid",
            Self::Write => "canon.write",
            Self::LevelInvalid => "canon.level_invalid",
        }
    }
}

pub(crate) fn stage_generation(
    profile: &P3Profile,
    plan: &GenerationPlan,
    staging_root: &Path,
) -> Result<VerifiedGeneration, CanonError> {
    create_hidden_staging_root(staging_root)?;
    let bundle_root = staging_root.join("bundle");
    fs::create_dir(&bundle_root).map_err(|_| CanonError::Write)?;

    let mut files: BTreeMap<String, (CanonDocument, Vec<u8>)> = BTreeMap::new();
    for parent in &plan.parent_documents {
        if files
            .insert(
                parent.document.canonical_path.clone(),
                (parent.document.clone(), parent.bytes.clone()),
            )
            .is_some()
        {
            return Err(CanonError::LineageInvalid);
        }
    }

    let existing = files.remove(&plan.input.target_path);
    let (seed_source_path, seed_source_hash, previous_hash) = match (plan.input.operation, existing)
    {
        (ProposalOperation::Create, Some(_)) | (ProposalOperation::Replace, None) => {
            return Err(CanonError::InputInvalid)
        }
        (ProposalOperation::Create, None) => (None, None, None),
        (ProposalOperation::Replace, Some((entry, _))) => {
            let previous_hash = plan
                .parent_predecessor_hashes
                .get(&plan.input.target_path)
                .cloned()
                .ok_or(CanonError::LineageInvalid)?;
            (
                entry.seed_source_path,
                entry.seed_source_hash,
                Some(previous_hash),
            )
        }
    };
    let target_hash = blake3::hash(&plan.input.target_bytes).to_hex().to_string();
    files.insert(
        plan.input.target_path.clone(),
        (
            CanonDocument {
                canonical_path: plan.input.target_path.clone(),
                seed_source_path,
                output_path: format!("bundle/{}", plan.input.target_path),
                id: plan.input.target_id.clone(),
                resource: format!("vault://{}", plan.input.target_id),
                hash: target_hash,
                origin_kind: OriginKind::Verdict,
                seed_source_hash,
                proposal_id: Some(plan.input.proposal_id.clone()),
                verdict_id: Some(plan.input.verdict_id.clone()),
                previous_hash,
            },
            plan.input.target_bytes.clone(),
        ),
    );

    let judgment_path = plan.input.judgment.path.clone();
    if files.contains_key(&judgment_path)
        || plan.input.judgment.verdict_id != plan.input.verdict_id
        || plan.input.judgment.hash
            != blake3::hash(&plan.input.judgment.bytes)
                .to_hex()
                .to_string()
    {
        return Err(CanonError::InputInvalid);
    }
    files.insert(
        judgment_path.clone(),
        (
            CanonDocument {
                canonical_path: judgment_path.clone(),
                seed_source_path: None,
                output_path: format!("bundle/{judgment_path}"),
                id: plan.input.judgment.id.clone(),
                resource: format!("vault://{}", plan.input.judgment.id),
                hash: plan.input.judgment.hash.clone(),
                origin_kind: OriginKind::Verdict,
                seed_source_hash: None,
                proposal_id: Some(plan.input.proposal_id.clone()),
                verdict_id: Some(plan.input.verdict_id.clone()),
                previous_hash: None,
            },
            plan.input.judgment.bytes.clone(),
        ),
    );

    let mut generated_documents = Vec::with_capacity(files.len());
    let mut manifest_documents = Vec::with_capacity(files.len());
    let mut bundle_files = BTreeMap::new();
    for (canonical_path, (entry, bytes)) in files {
        if entry.canonical_path != canonical_path
            || entry.output_path != format!("bundle/{canonical_path}")
        {
            return Err(CanonError::LineageInvalid);
        }
        let document = document_from_bytes(profile, &canonical_path, &bytes)?;
        if !document_matches_manifest(profile, &entry, &document) {
            return Err(CanonError::DocumentInvalid);
        }
        write_bundle_document(&bundle_root, &canonical_path, &bytes)?;
        bundle_files.insert(entry.output_path.clone(), bytes);
        generated_documents.push(document);
        manifest_documents.push(entry);
    }
    let index = render_index(&generated_documents).into_bytes();
    write_new(&bundle_root.join("index.md"), &index)?;
    bundle_files.insert("bundle/index.md".to_owned(), index);
    let bundle_hash = super::export::hash_bundle(&bundle_files);
    let manifest = CanonManifest {
        schema_version: CANON_MANIFEST_SCHEMA.to_owned(),
        store_id: P3_STORE_ID.to_owned(),
        profile_version: profile.version.clone(),
        profile_hash: profile.hash.clone(),
        generation_id: plan.input.generation_id.clone(),
        parent_kind: plan.parent_kind,
        parent_generation_id: plan.parent_generation_id.clone(),
        parent_manifest_hash: plan.parent_manifest_hash.clone(),
        seed_bundle_hash: plan.seed_bundle_hash.clone(),
        seed_manifest_hash: plan.seed_manifest_hash.clone(),
        policy_hash: plan.input.policy_hash.clone(),
        applied_proposal_id: plan.input.proposal_id.clone(),
        applied_verdict_id: plan.input.verdict_id.clone(),
        documents: manifest_documents,
        bundle_hash,
    };
    let manifest_bytes = canonical_manifest_bytes(&manifest)?;
    write_new(&staging_root.join("manifest.json"), &manifest_bytes)?;
    sync_tree(&bundle_root)?;
    sync_directory(staging_root)?;
    verify_generation(profile, staging_root, VerificationLevel::Three)
}

pub fn verify_generation(
    profile: &P3Profile,
    root: &Path,
    level: VerificationLevel,
) -> Result<VerifiedGeneration, CanonError> {
    if !matches!(
        level,
        VerificationLevel::One | VerificationLevel::Two | VerificationLevel::Three
    ) {
        return Err(CanonError::LevelInvalid);
    }
    let root = canonical_regular_directory(root)?;
    if !generation_root_shape(&root)? {
        return Err(CanonError::RootInvalid);
    }
    let manifest_bytes = read_regular_file(&root.join("manifest.json"))?;
    let manifest: CanonManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|_| CanonError::ManifestInvalid)?;
    if canonical_manifest_bytes(&manifest)? != manifest_bytes {
        return Err(CanonError::ManifestInvalid);
    }
    validate_manifest(profile, &manifest)?;

    let mut entries = BTreeMap::new();
    for entry in &manifest.documents {
        if entries
            .insert(entry.canonical_path.clone(), entry)
            .is_some()
        {
            return Err(CanonError::LineageInvalid);
        }
    }
    let actual_files = collect_bundle_files(&root.join("bundle"))?;
    let expected_files: BTreeSet<_> = manifest
        .documents
        .iter()
        .map(|document| document.output_path.clone())
        .chain(std::iter::once("bundle/index.md".to_owned()))
        .collect();
    if actual_files != expected_files {
        return Err(CanonError::HashMismatch);
    }

    let mut generated_documents = Vec::with_capacity(manifest.documents.len());
    let mut bundle_files = BTreeMap::new();
    for entry in &manifest.documents {
        let bytes = read_regular_file(&root.join(&entry.output_path))?;
        if matches!(level, VerificationLevel::Two | VerificationLevel::Three)
            && blake3::hash(&bytes).to_hex().as_str() != entry.hash
        {
            return Err(CanonError::HashMismatch);
        }
        let document = document_from_bytes(profile, &entry.canonical_path, &bytes)?;
        if !document_matches_manifest(profile, entry, &document) {
            return Err(CanonError::DocumentInvalid);
        }
        generated_documents.push(document);
        bundle_files.insert(entry.output_path.clone(), bytes);
    }
    let index = read_regular_file(&root.join("bundle/index.md"))?;
    if index != render_index(&generated_documents).into_bytes() {
        return Err(CanonError::IndexMismatch);
    }
    bundle_files.insert("bundle/index.md".to_owned(), index);
    if matches!(level, VerificationLevel::Two | VerificationLevel::Three)
        && super::export::hash_bundle(&bundle_files) != manifest.bundle_hash
    {
        return Err(CanonError::HashMismatch);
    }
    if level == VerificationLevel::Three {
        validate_level_three(profile, &manifest, &generated_documents)?;
    }
    let documents_hash = blake3::hash(
        &serde_json::to_vec(&manifest.documents).map_err(|_| CanonError::ManifestInvalid)?,
    )
    .to_hex()
    .to_string();
    Ok(VerifiedGeneration {
        root,
        manifest_hash: blake3::hash(&manifest_bytes).to_hex().to_string(),
        bundle_hash: manifest.bundle_hash.clone(),
        manifest,
        documents_hash,
        generated_documents,
    })
}

fn validate_input(namespace: &str, input: &AppliedGenerationInput) -> Result<(), CanonError> {
    if !valid_generation_id(&input.generation_id)
        || !valid_prefixed_hash(&input.proposal_id, "prp-")
        || !valid_prefixed_hash(&input.verdict_id, "ver-")
        || !lowercase_hash(&input.policy_hash)
        || !safe_canonical_path(&input.target_path)
        || super::p3_canonical_document_id(namespace, &input.target_path)
            .is_none_or(|expected| expected != input.target_id)
        || input.target_bytes.is_empty()
    {
        return Err(CanonError::InputInvalid);
    }
    Ok(())
}

fn validate_manifest(profile: &P3Profile, manifest: &CanonManifest) -> Result<(), CanonError> {
    if manifest.schema_version != CANON_MANIFEST_SCHEMA || manifest.store_id != P3_STORE_ID {
        return Err(CanonError::ManifestInvalid);
    }
    if manifest.profile_version != profile.version || manifest.profile_hash != profile.hash {
        return Err(CanonError::ProfileInvalid);
    }
    if !valid_generation_id(&manifest.generation_id)
        || !lowercase_hash(&manifest.parent_manifest_hash)
        || !lowercase_hash(&manifest.seed_bundle_hash)
        || !lowercase_hash(&manifest.seed_manifest_hash)
        || !lowercase_hash(&manifest.policy_hash)
        || !valid_prefixed_hash(&manifest.applied_proposal_id, "prp-")
        || !valid_prefixed_hash(&manifest.applied_verdict_id, "ver-")
        || !lowercase_hash(&manifest.bundle_hash)
    {
        return Err(CanonError::ManifestInvalid);
    }
    match (
        manifest.parent_kind,
        manifest.parent_generation_id.as_deref(),
    ) {
        (ParentKind::Seed, None) => {}
        (ParentKind::Generation, Some(value)) if valid_generation_id(value) => {}
        _ => return Err(CanonError::ParentInvalid),
    }
    let mut prior_path = None;
    let mut ids = BTreeSet::new();
    let mut resources = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for document in &manifest.documents {
        if prior_path
            .as_deref()
            .is_some_and(|previous: &str| previous.as_bytes() >= document.canonical_path.as_bytes())
        {
            return Err(CanonError::LineageInvalid);
        }
        prior_path = Some(document.canonical_path.clone());
        if !safe_canonical_path(&document.canonical_path)
            || document.output_path != format!("bundle/{}", document.canonical_path)
            || super::p3_canonical_document_id(
                &profile.config.context_namespace,
                &document.canonical_path,
            )
            .is_none_or(|expected| expected != document.id)
            || document.resource != format!("vault://{}", document.id)
            || !lowercase_hash(&document.hash)
            || !ids.insert(document.id.clone())
            || !resources.insert(document.resource.clone())
            || !outputs.insert(document.output_path.clone())
        {
            return Err(CanonError::LineageInvalid);
        }
        match document.origin_kind {
            OriginKind::Seed => {
                if document.seed_source_path.as_deref() != Some(document.canonical_path.as_str())
                    || !document
                        .seed_source_hash
                        .as_deref()
                        .is_some_and(lowercase_hash)
                    || document.proposal_id.is_some()
                    || document.verdict_id.is_some()
                    || document.previous_hash.is_some()
                    || document.canonical_path.starts_with("judgments/")
                {
                    return Err(CanonError::LineageInvalid);
                }
            }
            OriginKind::Verdict => {
                if !document
                    .proposal_id
                    .as_deref()
                    .is_some_and(|value| valid_prefixed_hash(value, "prp-"))
                    || !document
                        .verdict_id
                        .as_deref()
                        .is_some_and(|value| valid_prefixed_hash(value, "ver-"))
                    || !document
                        .seed_source_hash
                        .as_deref()
                        .map_or(true, lowercase_hash)
                    || !document
                        .previous_hash
                        .as_deref()
                        .map_or(true, lowercase_hash)
                {
                    return Err(CanonError::LineageInvalid);
                }
                if document.seed_source_path.is_some() != document.seed_source_hash.is_some() {
                    return Err(CanonError::LineageInvalid);
                }
                if document.seed_source_path.is_some() && document.previous_hash.is_none() {
                    return Err(CanonError::LineageInvalid);
                }
            }
        }
    }
    Ok(())
}

fn validate_level_three(
    profile: &P3Profile,
    manifest: &CanonManifest,
    documents: &[GeneratedDocument],
) -> Result<(), CanonError> {
    let reconstructed =
        convert_links(&profile.config, documents.to_vec()).map_err(|_| CanonError::LinkInvalid)?;
    if reconstructed.documents != documents {
        return Err(CanonError::LinkInvalid);
    }
    let current: Vec<_> = manifest
        .documents
        .iter()
        .filter(|entry| {
            entry.proposal_id.as_deref() == Some(manifest.applied_proposal_id.as_str())
                && entry.verdict_id.as_deref() == Some(manifest.applied_verdict_id.as_str())
        })
        .collect();
    let judgment_path = format!("judgments/{}.md", manifest.applied_verdict_id);
    let judgment_entry = current
        .iter()
        .copied()
        .find(|entry| entry.canonical_path == judgment_path)
        .ok_or(CanonError::LineageInvalid)?;
    if judgment_entry.seed_source_path.is_some()
        || judgment_entry.seed_source_hash.is_some()
        || judgment_entry.previous_hash.is_some()
    {
        return Err(CanonError::LineageInvalid);
    }
    let target_entries: Vec<_> = current
        .into_iter()
        .filter(|entry| entry.canonical_path != judgment_path)
        .collect();
    if target_entries.len() != 1 {
        return Err(CanonError::LineageInvalid);
    }
    let target = target_entries[0];
    let by_path: BTreeMap<_, _> = documents
        .iter()
        .map(|document| (document.metadata.source_path.as_str(), document))
        .collect();
    let judgment = by_path
        .get(judgment_path.as_str())
        .copied()
        .ok_or(CanonError::DocumentInvalid)?;
    if judgment.metadata.subject.as_deref() != Some(target.id.as_str())
        || !matches!(
            judgment.metadata.verdict.as_deref(),
            Some("ACCEPT" | "REWRITE")
        )
        || judgment.metadata.links.len() != 1
        || judgment.metadata.links[0].rel != LinkRelation::References
        || judgment.metadata.links[0].target != format!("vault://{}", target.id)
    {
        return Err(CanonError::LineageInvalid);
    }
    Ok(())
}

fn document_from_bytes(
    profile: &P3Profile,
    canonical_path: &str,
    bytes: &[u8],
) -> Result<GeneratedDocument, CanonError> {
    let hash = blake3::hash(bytes).to_hex().to_string();
    let parsed = parse_source_document(SourceDocument {
        source_path: canonical_path.to_owned(),
        bytes: bytes.to_vec(),
        source_hash: hash.clone(),
        created: String::new(),
        updated: String::new(),
    })
    .map_err(|_| CanonError::DocumentInvalid)?;
    let metadata = serde_yaml::to_string(&parsed.metadata)
        .ok()
        .and_then(|yaml| serde_yaml::from_str::<GeneratedMetadata>(&yaml).ok())
        .ok_or(CanonError::DocumentInvalid)?;
    let document = GeneratedDocument {
        metadata,
        body: parsed.body,
        source_hash: hash,
    };
    if validate_document(&profile.config, &document).has_errors() {
        return Err(CanonError::DocumentInvalid);
    }
    Ok(document)
}

fn document_matches_manifest(
    _profile: &P3Profile,
    entry: &CanonDocument,
    document: &GeneratedDocument,
) -> bool {
    document.metadata.id == entry.id
        && document.metadata.resource == entry.resource
        && document.metadata.source == entry.resource
}

pub(crate) fn frozen_seed_profile(profile: &P3Profile) -> super::ProfileConfig {
    let mut seed_profile = profile.config.clone();
    seed_profile
        .context_kinds
        .retain(|_, core_type| *core_type != super::CoreType::Judgment);
    seed_profile
        .mapping_rules
        .retain(|rule| rule.core_type != super::CoreType::Judgment);
    seed_profile
        .sensitivity_rules
        .retain(|rule| !rule.pattern.starts_with("judgments/"));
    seed_profile
}

fn rebind_seed_document(
    profile: &P3Profile,
    canonical_path: &str,
    bytes: &[u8],
) -> Result<Vec<u8>, CanonError> {
    let parsed = parse_source_document(SourceDocument {
        source_path: canonical_path.to_owned(),
        bytes: bytes.to_vec(),
        source_hash: blake3::hash(bytes).to_hex().to_string(),
        created: String::new(),
        updated: String::new(),
    })
    .map_err(|_| CanonError::SeedInvalid)?;
    let mut metadata = serde_yaml::to_string(&parsed.metadata)
        .ok()
        .and_then(|yaml| serde_yaml::from_str::<GeneratedMetadata>(&yaml).ok())
        .ok_or(CanonError::SeedInvalid)?;
    let id = super::p3_canonical_document_id(&profile.config.context_namespace, canonical_path)
        .ok_or(CanonError::SeedInvalid)?;
    let resource = format!("vault://{id}");
    metadata.context_namespace = profile.config.context_namespace.clone();
    metadata.id = id;
    metadata.resource = resource.clone();
    metadata.source = resource;
    let document = GeneratedDocument {
        metadata,
        body: parsed.body,
        source_hash: parsed.source_hash,
    };
    if validate_document(&profile.config, &document).has_errors() {
        return Err(CanonError::SeedInvalid);
    }
    render_document(&document).map_err(|_| CanonError::SeedInvalid)
}

fn canonical_manifest_bytes(manifest: &CanonManifest) -> Result<Vec<u8>, CanonError> {
    let mut bytes = serde_json::to_vec_pretty(manifest).map_err(|_| CanonError::ManifestInvalid)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn create_hidden_staging_root(path: &Path) -> Result<(), CanonError> {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return Err(CanonError::StagingInvalid);
    };
    if !name.starts_with('.')
        || path.exists()
        || path.parent().is_none()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(CanonError::StagingInvalid);
    }
    fs::create_dir(path).map_err(|_| CanonError::Write)
}

fn write_bundle_document(bundle_root: &Path, path: &str, bytes: &[u8]) -> Result<(), CanonError> {
    let destination = bundle_root.join(path);
    let parent = destination.parent().ok_or(CanonError::PathUnsafe)?;
    fs::create_dir_all(parent).map_err(|_| CanonError::Write)?;
    write_new(&destination, bytes)?;
    sync_directory(parent)?;
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), CanonError> {
    use std::io::Write;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| CanonError::Write)?;
    let result = (|| {
        file.write_all(bytes).map_err(|_| CanonError::Write)?;
        file.sync_all().map_err(|_| CanonError::Write)
    })();
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn sync_tree(root: &Path) -> Result<(), CanonError> {
    let mut entries = fs::read_dir(root)
        .map_err(|_| CanonError::Write)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CanonError::Write)?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let metadata = entry.metadata().map_err(|_| CanonError::Write)?;
        if metadata.is_dir() {
            sync_tree(&entry.path())?;
        }
    }
    sync_directory(root)
}

fn sync_directory(path: &Path) -> Result<(), CanonError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| CanonError::Write)
}

fn generation_root_shape(root: &Path) -> Result<bool, CanonError> {
    let mut entries = fs::read_dir(root)
        .map_err(|_| CanonError::Read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CanonError::Read)?;
    entries.sort_by_key(|entry| entry.file_name());
    if entries.len() != 2 {
        return Ok(false);
    }
    let mut actual = BTreeSet::new();
    for entry in entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| CanonError::PathUnsafe)?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|_| CanonError::Read)?;
        if metadata.file_type().is_symlink() {
            return Err(CanonError::Symlink);
        }
        let valid = match name.as_str() {
            "manifest.json" => metadata.is_file(),
            "bundle" => metadata.is_dir(),
            _ => false,
        };
        if !valid {
            return Ok(false);
        }
        actual.insert(name);
    }
    Ok(actual == BTreeSet::from(["bundle".to_owned(), "manifest.json".to_owned()]))
}

fn collect_bundle_files(bundle_root: &Path) -> Result<BTreeSet<String>, CanonError> {
    fn visit(
        root: &Path,
        directory: &Path,
        files: &mut BTreeSet<String>,
    ) -> Result<(), CanonError> {
        let mut entries = fs::read_dir(directory)
            .map_err(|_| CanonError::Read)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CanonError::Read)?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_type = entry.file_type().map_err(|_| CanonError::Read)?;
            if file_type.is_symlink() {
                return Err(CanonError::Symlink);
            }
            if file_type.is_dir() {
                visit(root, &entry.path(), files)?;
                continue;
            }
            if !file_type.is_file() {
                return Err(CanonError::RootInvalid);
            }
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| CanonError::PathUnsafe)?
                .to_str()
                .ok_or(CanonError::PathUnsafe)?
                .replace('\\', "/");
            if !safe_canonical_path(&relative) && relative != "index.md" {
                return Err(CanonError::PathUnsafe);
            }
            let output = format!("bundle/{relative}");
            if !files.insert(output) {
                return Err(CanonError::RootInvalid);
            }
        }
        Ok(())
    }

    let metadata = fs::symlink_metadata(bundle_root).map_err(|_| CanonError::Read)?;
    if metadata.file_type().is_symlink() {
        return Err(CanonError::Symlink);
    }
    if !metadata.is_dir() {
        return Err(CanonError::RootInvalid);
    }
    let mut files = BTreeSet::new();
    visit(bundle_root, bundle_root, &mut files)?;
    Ok(files)
}

fn canonical_regular_directory(path: &Path) -> Result<PathBuf, CanonError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CanonError::Read)?;
    if metadata.file_type().is_symlink() {
        return Err(CanonError::Symlink);
    }
    if !metadata.is_dir() {
        return Err(CanonError::RootInvalid);
    }
    fs::canonicalize(path).map_err(|_| CanonError::Read)
}

fn read_regular_file(path: &Path) -> Result<Vec<u8>, CanonError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CanonError::Read)?;
    if metadata.file_type().is_symlink() {
        return Err(CanonError::Symlink);
    }
    if !metadata.is_file() {
        return Err(CanonError::RootInvalid);
    }
    fs::read(path).map_err(|_| CanonError::Read)
}

fn raw_hash(path: &Path) -> Result<String, CanonError> {
    Ok(blake3::hash(&read_regular_file(path)?).to_hex().to_string())
}

fn safe_canonical_path(path: &str) -> bool {
    super::valid_relative_path(path)
        && path.ends_with(".md")
        && !matches!(
            path,
            "index.md" | "manifest.json" | "policy.toml" | "CURRENT"
        )
        && !path.starts_with("staging/")
        && !path.starts_with("ledger/")
        && !path.starts_with("evidence/")
        && !path.starts_with("derived/")
        && !path.starts_with("transactions/")
        && !path.starts_with("locks/")
}

fn valid_generation_id(value: &str) -> bool {
    value.len() == 20
        && value.starts_with("gen-")
        && value[4..].bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_prefixed_hash(value: &str, prefix: &str) -> bool {
    value.len() == prefix.len() + 64
        && value.starts_with(prefix)
        && value[prefix.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn lowercase_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
