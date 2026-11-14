use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

use super::{
    inspect_store, proposal_list, read_ledger, verify_bundle, verify_generation,
    verify_store_level4, BundleVerification, CoreType, GeneratedDocument, Layer, P3Profile,
    ProposalFilter, ProposalRecord, ProposalState, Provenance, VerificationLevel,
    VerificationOptions,
};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct BundleStats {
    pub documents: usize,
    pub by_provenance: BTreeMap<String, usize>,
    pub by_type: BTreeMap<String, usize>,
    pub by_layer: BTreeMap<String, usize>,
    pub warnings: usize,
    pub exclusions: usize,
    pub bundle_hash: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct QueueStats {
    pub submitted: usize,
    pub staged_open: usize,
    pub rejected_at_intake: usize,
    pub open: usize,
    pub alarm_at: usize,
    pub alarm: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct StoreStats {
    pub status: &'static str,
    pub command: &'static str,
    pub sealed: bool,
    pub bundle: BundleStats,
    pub events: usize,
    pub verdicts: usize,
    pub applied_generations: usize,
    pub current_generation: String,
    pub ledger_head: Option<String>,
    pub queue: QueueStats,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum StoreStatsError {
    #[error("store stats could not be verified")]
    Verification { code: &'static str },
}

impl StoreStatsError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Verification { code } => code,
        }
    }
}

pub fn store_stats(profile: &P3Profile, store_root: &Path) -> Result<StoreStats, StoreStatsError> {
    store_stats_with_observer(profile, store_root, || {})
}

pub fn store_stats_with_observer(
    profile: &P3Profile,
    store_root: &Path,
    after_collect: impl FnOnce(),
) -> Result<StoreStats, StoreStatsError> {
    let initial = inspect_store(profile, store_root).map_err(stats_store_error)?;
    let before = stats_snapshot_fingerprint(&initial.paths.root)?;
    let level4 = if initial.sealed {
        Some(verify_store_level4(profile, store_root).map_err(|error| stats_error(error.code()))?)
    } else {
        None
    };
    let records = proposal_list(profile, store_root, ProposalFilter { state: None })
        .map_err(|error| stats_error(error.code()))?;
    let snapshot = inspect_store(profile, store_root).map_err(stats_store_error)?;
    if snapshot.sealed != initial.sealed {
        return Err(stats_error("transaction.incomplete"));
    }
    let seed_verification = verify_bundle(
        &super::canon::frozen_seed_profile(profile),
        &snapshot.paths.seed,
        VerificationOptions::level(VerificationLevel::Three),
    );
    let seed_bundle = summarize_verified_bundle(&seed_verification)
        .ok_or_else(|| stats_error("bundle.verification_failed"))?;
    if !snapshot.sealed {
        let stats = StoreStats {
            status: "ok",
            command: "stats",
            sealed: false,
            bundle: seed_bundle,
            events: 0,
            verdicts: 0,
            applied_generations: 0,
            current_generation: "seed".to_owned(),
            ledger_head: None,
            queue: queue_stats(&records, snapshot.policy.policy.queue_alarm_at),
        };
        after_collect();
        ensure_unchanged_snapshot(&snapshot.paths.root, &before)?;
        return Ok(stats);
    }

    let summary = level4.ok_or_else(|| stats_error("ledger.missing"))?;
    let ledger = read_ledger(&snapshot.paths.ledger).map_err(|error| stats_error(error.code()))?;
    if ledger.final_hash.as_deref() != Some(summary.ledger_head.as_str()) {
        return Err(stats_error("transaction.incomplete"));
    }
    let bundle = if summary.current_generation == "seed" {
        seed_bundle
    } else {
        let generation = verify_generation(
            profile,
            &snapshot
                .paths
                .canon
                .join("generations")
                .join(&summary.current_generation),
            VerificationLevel::Three,
        )
        .map_err(|_| stats_error("ledger.generation_missing"))?;
        summarize_generated_documents(
            &generation.generated_documents,
            super::validate_bundle(&profile.config, &generation.generated_documents)
                .warnings()
                .len(),
            seed_bundle.exclusions,
            generation.bundle_hash.clone(),
        )
    };
    let stats = StoreStats {
        status: "ok",
        command: "stats",
        sealed: true,
        bundle,
        events: summary.events,
        verdicts: summary.verdicts,
        applied_generations: summary.applied_generations,
        current_generation: summary.current_generation,
        ledger_head: Some(summary.ledger_head),
        queue: queue_stats(&records, snapshot.policy.policy.queue_alarm_at),
    };
    after_collect();
    ensure_unchanged_snapshot(&snapshot.paths.root, &before)?;
    Ok(stats)
}

fn ensure_unchanged_snapshot(root: &Path, before: &str) -> Result<(), StoreStatsError> {
    if stats_snapshot_fingerprint(root)? != before {
        return Err(stats_error("store.changed_during_read"));
    }
    Ok(())
}

pub fn stats_snapshot_fingerprint(root: &Path) -> Result<String, StoreStatsError> {
    let mut files = Vec::new();
    collect_snapshot_files(root, root, &mut files)?;
    files.sort();
    let mut hasher = blake3::Hasher::new();
    snapshot_frame(&mut hasher, b"sovmem-store-stats-snapshot-v1");
    for (path, is_directory) in files {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| stats_error("store.read"))?
            .to_str()
            .ok_or_else(|| stats_error("store.path_unsafe"))?;
        snapshot_frame(&mut hasher, relative.as_bytes());
        snapshot_frame(
            &mut hasher,
            if is_directory { b"directory" } else { b"file" },
        );
        if !is_directory {
            snapshot_frame(
                &mut hasher,
                &fs::read(path).map_err(|_| stats_error("store.read"))?,
            );
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn snapshot_frame(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(bytes);
}

fn collect_snapshot_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(PathBuf, bool)>,
) -> Result<(), StoreStatsError> {
    for entry in fs::read_dir(directory).map_err(|_| stats_error("store.read"))? {
        let path = entry.map_err(|_| stats_error("store.read"))?.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| stats_error("store.read"))?;
        let in_derived = relative
            .components()
            .next()
            .is_some_and(|part| part.as_os_str() == "derived");
        if in_derived && relative != Path::new("derived") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|_| stats_error("store.read"))?;
        if metadata.file_type().is_symlink() {
            return Err(stats_error("store.symlink"));
        }
        if metadata.is_dir() {
            files.push((path.clone(), true));
            if !in_derived {
                collect_snapshot_files(root, &path, files)?;
            }
        } else if metadata.is_file() {
            files.push((path, false));
        } else {
            return Err(stats_error("store.layout_invalid"));
        }
    }
    Ok(())
}

fn queue_stats(records: &[ProposalRecord], alarm_at: usize) -> QueueStats {
    let staged_open = records
        .iter()
        .filter(|record| record.state == ProposalState::Staged)
        .count();
    let submitted = records
        .iter()
        .filter(|record| record.state == ProposalState::Submitted)
        .count();
    let rejected_at_intake = records
        .iter()
        .filter(|record| record.state == ProposalState::RejectedAtIntake)
        .count();
    QueueStats {
        submitted,
        staged_open,
        rejected_at_intake,
        open: staged_open,
        alarm_at,
        alarm: staged_open >= alarm_at,
    }
}

fn stats_store_error(error: super::StoreError) -> StoreStatsError {
    stats_error(error.code())
}

fn stats_error(code: &'static str) -> StoreStatsError {
    StoreStatsError::Verification { code }
}

pub fn summarize_verified_bundle(verification: &BundleVerification) -> Option<BundleStats> {
    if verification.has_errors() {
        return None;
    }

    Some(summarize_documents(
        verification.generated_documents(),
        verification.document_count(),
        verification.warning_count(),
        verification.exclusion_count(),
        verification.bundle_hash().to_owned(),
    ))
}

fn summarize_generated_documents(
    documents: &[GeneratedDocument],
    warnings: usize,
    exclusions: usize,
    bundle_hash: String,
) -> BundleStats {
    summarize_documents(
        documents,
        documents.len(),
        warnings,
        exclusions,
        bundle_hash,
    )
}

fn summarize_documents(
    documents: &[GeneratedDocument],
    document_count: usize,
    warnings: usize,
    exclusions: usize,
    bundle_hash: String,
) -> BundleStats {
    let mut by_provenance = BTreeMap::from([
        ("secondary".to_owned(), 0),
        ("synthesis".to_owned(), 0),
        ("tier1".to_owned(), 0),
        ("unknown".to_owned(), 0),
    ]);
    let mut by_type = BTreeMap::from([
        ("concept".to_owned(), 0),
        ("judgment".to_owned(), 0),
        ("resource".to_owned(), 0),
    ]);
    let mut by_layer = BTreeMap::from([
        ("analysis".to_owned(), 0),
        ("concept".to_owned(), 0),
        ("synthesis".to_owned(), 0),
    ]);

    for document in documents {
        let provenance = match document.metadata.provenance {
            Provenance::Tier1 => "tier1",
            Provenance::Secondary => "secondary",
            Provenance::Synthesis => "synthesis",
            Provenance::Unknown => "unknown",
        };
        let core_type = match document.metadata.core_type {
            CoreType::Concept => "concept",
            CoreType::Resource => "resource",
            CoreType::Judgment => "judgment",
        };
        let layer = match document.metadata.layer {
            Layer::Synthesis => "synthesis",
            Layer::Analysis => "analysis",
            Layer::Concept => "concept",
        };
        *by_provenance.get_mut(provenance).unwrap() += 1;
        *by_type.get_mut(core_type).unwrap() += 1;
        *by_layer.get_mut(layer).unwrap() += 1;
    }

    BundleStats {
        documents: document_count,
        by_provenance,
        by_type,
        by_layer,
        warnings,
        exclusions,
        bundle_hash,
    }
}
