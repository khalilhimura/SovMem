use super::{
    load_policy, verify_bundle, LoadedPolicy, P3Profile, VerificationLevel, VerificationOptions,
};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

#[allow(dead_code)]
static NEXT_TEMPORARY_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct StorePaths {
    pub root: PathBuf,
    pub policy: PathBuf,
    pub policies: PathBuf,
    pub seed: PathBuf,
    pub staging: PathBuf,
    pub inbox: PathBuf,
    pub intake: PathBuf,
    pub intake_events: PathBuf,
    pub intake_proposals: PathBuf,
    pub intake_transactions: PathBuf,
    pub ledger: PathBuf,
    pub canon: PathBuf,
    pub current: PathBuf,
    pub evidence: PathBuf,
    pub derived: PathBuf,
    pub transactions: PathBuf,
    pub locks: PathBuf,
}

impl StorePaths {
    pub fn from_root(root: PathBuf) -> Self {
        Self {
            policy: root.join("policy.toml"),
            policies: root.join("policies"),
            seed: root.join("seed"),
            staging: root.join("staging"),
            inbox: root.join("staging/inbox"),
            intake: root.join("intake"),
            intake_events: root.join("intake/events.jsonl"),
            intake_proposals: root.join("intake/proposals"),
            intake_transactions: root.join("intake/transactions"),
            ledger: root.join("ledger/verdicts.jsonl"),
            canon: root.join("canon"),
            current: root.join("canon/CURRENT"),
            evidence: root.join("evidence"),
            derived: root.join("derived"),
            transactions: root.join("transactions"),
            locks: root.join("locks"),
            root,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalHead {
    Seed,
    Generation(String),
}

impl CanonicalHead {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Seed => "seed",
            Self::Generation(value) => value,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SeedEvidence {
    pub documents: usize,
    pub manifest_hash: String,
    pub corpus_report_hash: String,
    pub bundle_hash: String,
    pub source_commit: String,
}

#[derive(Clone, Debug)]
pub struct StoreSnapshot {
    pub paths: StorePaths,
    pub policy: LoadedPolicy,
    pub profile_hash: String,
    pub seed: SeedEvidence,
    pub head: CanonicalHead,
    pub sealed: bool,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("store could not be read")]
    Read,
    #[error("store layout is invalid")]
    LayoutInvalid,
    #[error("store path is unsafe")]
    PathUnsafe,
    #[error("store contains a symlink")]
    Symlink,
    #[error("store roots cross filesystems")]
    CrossFilesystem,
    #[error("store is locked")]
    Locked,
    #[error("transaction is incomplete")]
    Incomplete,
    #[error("store write failed")]
    Write,
}

impl StoreError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Read => "store.read",
            Self::LayoutInvalid => "store.layout_invalid",
            Self::PathUnsafe => "store.path_unsafe",
            Self::Symlink => "store.symlink",
            Self::CrossFilesystem => "store.cross_filesystem",
            Self::Locked => "store.locked",
            Self::Incomplete => "transaction.incomplete",
            Self::Write => "store.write",
        }
    }
}

pub fn inspect_store(
    profile: &P3Profile,
    requested_root: &Path,
) -> Result<StoreSnapshot, StoreError> {
    let snapshot = inspect_store_with_transients(profile, requested_root)?;
    if has_public_transient_state(&snapshot.paths)? {
        return Err(StoreError::Incomplete);
    }
    Ok(snapshot)
}

pub fn inspect_store_with_transients(
    profile: &P3Profile,
    requested_root: &Path,
) -> Result<StoreSnapshot, StoreError> {
    let root = canonical_store_root(requested_root)?;
    reject_unsafe_tree(&root)?;
    let paths = StorePaths::from_root(root);
    verify_durable_layout(&paths)?;
    verify_same_filesystem(&paths)?;
    let policy = load_policy(&paths.policy).map_err(|_| StoreError::LayoutInvalid)?;
    let seed = inspect_seed(profile, &paths.seed)?;
    let ledger = fs::read(&paths.ledger).map_err(|_| StoreError::Read)?;
    let head = parse_current(&fs::read(&paths.current).map_err(|_| StoreError::Read)?)?;

    Ok(StoreSnapshot {
        paths,
        policy,
        profile_hash: profile.hash.clone(),
        seed,
        head,
        sealed: !ledger.is_empty(),
    })
}

pub fn verify_unsealed_store(
    profile: &P3Profile,
    requested_root: &Path,
) -> Result<StoreSnapshot, StoreError> {
    let snapshot = inspect_store(profile, requested_root)?;
    if snapshot.sealed {
        return Err(StoreError::Incomplete);
    }
    Ok(snapshot)
}

pub fn has_public_transient_state(paths: &StorePaths) -> Result<bool, StoreError> {
    if has_ledger_temporary(paths)? {
        return Ok(true);
    }
    if has_current_temporary(paths)? {
        return Ok(true);
    }
    if has_generation_or_index_staging(paths)? {
        return Ok(true);
    }
    if has_intake_temporary(paths)? {
        return Ok(true);
    }
    if super::intake_transactions_have_incomplete_state(paths).unwrap_or(true) {
        return Ok(true);
    }
    for path in [&paths.transactions, &paths.locks] {
        if fs::read_dir(path)
            .map_err(|_| StoreError::Read)?
            .next()
            .transpose()
            .map_err(|_| StoreError::Read)?
            .is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn has_intake_temporary(paths: &StorePaths) -> Result<bool, StoreError> {
    for entry in fs::read_dir(&paths.intake).map_err(|_| StoreError::Read)? {
        let name = entry.map_err(|_| StoreError::Read)?.file_name();
        if name.to_str().is_some_and(valid_intake_temporary) {
            return Ok(true);
        }
    }
    for shard in fs::read_dir(&paths.intake_proposals).map_err(|_| StoreError::Read)? {
        let shard = shard.map_err(|_| StoreError::Read)?;
        if !shard.file_type().map_err(|_| StoreError::Read)?.is_dir() {
            continue;
        }
        for entry in fs::read_dir(shard.path()).map_err(|_| StoreError::Read)? {
            let name = entry.map_err(|_| StoreError::Read)?.file_name();
            if name.to_str().is_some_and(valid_proposal_temporary) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn has_ledger_temporary(paths: &StorePaths) -> Result<bool, StoreError> {
    for entry in fs::read_dir(paths.ledger.parent().ok_or(StoreError::Read)?)
        .map_err(|_| StoreError::Read)?
    {
        let name = entry.map_err(|_| StoreError::Read)?.file_name();
        if name.to_str().is_some_and(valid_ledger_temporary) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn canonical_store_root(requested_root: &Path) -> Result<PathBuf, StoreError> {
    let metadata = fs::symlink_metadata(requested_root).map_err(|_| StoreError::Read)?;
    if metadata.file_type().is_symlink() {
        return Err(StoreError::Symlink);
    }
    if !metadata.is_dir() {
        return Err(StoreError::LayoutInvalid);
    }
    fs::canonicalize(requested_root).map_err(|_| StoreError::Read)
}

fn reject_unsafe_tree(path: &Path) -> Result<(), StoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| StoreError::Read)?;
    if metadata.file_type().is_symlink() {
        return Err(StoreError::Symlink);
    }
    if metadata.is_file() {
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(StoreError::LayoutInvalid);
    }
    let mut entries = fs::read_dir(path)
        .map_err(|_| StoreError::Read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| StoreError::Read)?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if entry.file_name().to_str().is_none() {
            return Err(StoreError::PathUnsafe);
        }
        reject_unsafe_tree(&entry.path())?;
    }
    Ok(())
}

fn verify_durable_layout(paths: &StorePaths) -> Result<(), StoreError> {
    exact_directory(
        &paths.root,
        &[
            "policy.toml",
            "policies",
            "seed",
            "staging",
            "intake",
            "ledger",
            "canon",
            "evidence",
            "derived",
            "transactions",
            "locks",
        ],
    )?;
    regular_file(&paths.policy)?;
    regular_directory(&paths.seed)?;
    verify_ledger_directory(&paths.root.join("ledger"))?;
    verify_canon_directory(&paths.canon)?;
    exact_directory(&paths.evidence, &["reindex"])?;
    exact_directory(&paths.derived, &["indexes"])?;
    regular_file(&paths.ledger)?;
    regular_file(&paths.current)?;
    verify_policy_archives(&paths.policies)?;
    verify_generations(&paths.canon.join("generations"))?;
    verify_reindex_evidence(&paths.evidence.join("reindex"))?;
    verify_derived_indexes(&paths.derived.join("indexes"))?;
    verify_transactions(&paths.transactions)?;
    verify_verdict_lock_root(&paths.locks)?;
    verify_staging_root(&paths.staging)?;
    verify_intake_root(&paths.intake)?;
    Ok(())
}

fn verify_ledger_directory(path: &Path) -> Result<(), StoreError> {
    regular_directory(path)?;
    let mut has_verdicts = false;
    for entry in sorted_entries(path)? {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StoreError::PathUnsafe)?;
        if name == "verdicts.jsonl" {
            has_verdicts = true;
        } else if !valid_ledger_temporary(&name) {
            return Err(StoreError::LayoutInvalid);
        }
        regular_file(&entry.path())?;
    }
    if has_verdicts {
        Ok(())
    } else {
        Err(StoreError::LayoutInvalid)
    }
}

fn verify_canon_directory(path: &Path) -> Result<(), StoreError> {
    regular_directory(path)?;
    let mut has_current = false;
    let mut has_generations = false;
    for entry in sorted_entries(path)? {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StoreError::PathUnsafe)?;
        match name.as_str() {
            "CURRENT" => {
                regular_file(&entry.path())?;
                has_current = true;
            }
            "generations" => {
                regular_directory(&entry.path())?;
                has_generations = true;
            }
            _ if valid_current_temporary(&name) => regular_file(&entry.path())?,
            _ => return Err(StoreError::LayoutInvalid),
        }
    }
    if has_current && has_generations {
        Ok(())
    } else {
        Err(StoreError::LayoutInvalid)
    }
}

fn verify_policy_archives(path: &Path) -> Result<(), StoreError> {
    regular_directory(path)?;
    for entry in sorted_entries(path)? {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StoreError::PathUnsafe)?;
        if !name.ends_with(".toml") || !valid_hash(&name[..name.len() - 5]) {
            return Err(StoreError::LayoutInvalid);
        }
        regular_file(&entry.path())?;
    }
    Ok(())
}

fn verify_generations(path: &Path) -> Result<(), StoreError> {
    regular_directory(path)?;
    for entry in sorted_entries(path)? {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StoreError::PathUnsafe)?;
        if !valid_generation_id(&name) && !valid_transaction_staging(&name) {
            return Err(StoreError::LayoutInvalid);
        }
        regular_directory(&entry.path())?;
    }
    Ok(())
}

fn verify_reindex_evidence(path: &Path) -> Result<(), StoreError> {
    regular_directory(path)?;
    for entry in sorted_entries(path)? {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StoreError::PathUnsafe)?;
        if !name.ends_with(".json") || !valid_verdict_id(&name[..name.len() - 5]) {
            return Err(StoreError::LayoutInvalid);
        }
        regular_file(&entry.path())?;
    }
    Ok(())
}

fn verify_derived_indexes(path: &Path) -> Result<(), StoreError> {
    regular_directory(path)?;
    for entry in sorted_entries(path)? {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StoreError::PathUnsafe)?;
        if !valid_generation_id(&name) && !valid_transaction_staging(&name) {
            return Err(StoreError::LayoutInvalid);
        }
        regular_directory(&entry.path())?;
    }
    Ok(())
}

fn verify_transactions(path: &Path) -> Result<(), StoreError> {
    regular_directory(path)?;
    for entry in sorted_entries(path)? {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StoreError::PathUnsafe)?;
        if valid_plan_preparation(&name) {
            regular_file(&entry.path())?;
            continue;
        }
        let transaction_id = name.strip_prefix(".cleanup-").unwrap_or(&name);
        if !valid_transaction_id(transaction_id) {
            return Err(StoreError::LayoutInvalid);
        }
        regular_directory(&entry.path())?;
    }
    Ok(())
}

fn verify_verdict_lock_root(path: &Path) -> Result<(), StoreError> {
    regular_directory(path)?;
    for entry in sorted_entries(path)? {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StoreError::PathUnsafe)?;
        if name != "verdict.lock"
            && name != "intake.lock"
            && !valid_lease_temporary(&name, "verdict")
            && !valid_lease_temporary(&name, "intake")
            && !valid_recovery_lease(&name, "verdict")
            && !valid_recovery_lease(&name, "intake")
        {
            return Err(StoreError::LayoutInvalid);
        }
        verify_lease_directory(&entry.path())?;
    }
    Ok(())
}

fn verify_staging_root(path: &Path) -> Result<(), StoreError> {
    exact_directory(path, &["inbox"])
}

fn verify_intake_root(path: &Path) -> Result<(), StoreError> {
    regular_directory(path)?;
    let mut required = BTreeSet::from([
        "events.jsonl".to_owned(),
        "proposals".to_owned(),
        "transactions".to_owned(),
    ]);
    for entry in sorted_entries(path)? {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StoreError::PathUnsafe)?;
        if required.remove(&name) {
            match name.as_str() {
                "events.jsonl" => regular_file(&entry.path())?,
                "proposals" => verify_intake_proposals(&entry.path())?,
                "transactions" => regular_directory(&entry.path())?,
                _ => unreachable!(),
            }
        } else if valid_intake_temporary(&name) {
            regular_file(&entry.path())?;
        } else {
            return Err(StoreError::LayoutInvalid);
        }
    }
    if required.is_empty() {
        Ok(())
    } else {
        Err(StoreError::LayoutInvalid)
    }
}

fn verify_intake_proposals(path: &Path) -> Result<(), StoreError> {
    regular_directory(path)?;
    for shard in sorted_entries(path)? {
        let shard_name = shard
            .file_name()
            .into_string()
            .map_err(|_| StoreError::PathUnsafe)?;
        if !valid_intake_shard(&shard_name) {
            return Err(StoreError::LayoutInvalid);
        }
        regular_directory(&shard.path())?;
        for pair in sorted_entries(&shard.path())? {
            let name = pair
                .file_name()
                .into_string()
                .map_err(|_| StoreError::PathUnsafe)?;
            if valid_proposal_temporary(&name) {
                if proposal_temporary_proposal_id(&name)
                    .is_none_or(|proposal_id| proposal_id[4..6] != shard_name)
                {
                    return Err(StoreError::LayoutInvalid);
                }
                regular_directory(&pair.path())?;
                continue;
            }
            if !valid_proposal_id(&name) || name[4..6] != shard_name {
                return Err(StoreError::LayoutInvalid);
            }
            exact_directory(&pair.path(), &["candidate.md", "proposal.meta.json"])?;
            regular_file(&pair.path().join("candidate.md"))?;
            regular_file(&pair.path().join("proposal.meta.json"))?;
        }
    }
    Ok(())
}

fn valid_intake_shard(value: &str) -> bool {
    value.len() == 2
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_intake_temporary(name: &str) -> bool {
    name.strip_prefix(".events-")
        .and_then(|sequence| {
            sequence
                .parse::<u64>()
                .ok()
                .map(|parsed| (sequence, parsed))
        })
        .is_some_and(|(sequence, parsed)| parsed.to_string() == sequence)
}

fn verify_lease_directory(path: &Path) -> Result<(), StoreError> {
    regular_directory(path)?;
    let entries = sorted_entries(path)?;
    if entries.is_empty() {
        return Ok(());
    }
    if entries.len() == 1 && entries[0].file_name().to_str() == Some("owner.json") {
        return regular_file(&entries[0].path());
    }
    Err(StoreError::LayoutInvalid)
}

fn valid_lease_temporary(name: &str, kind: &str) -> bool {
    let prefix = format!(".lease-{kind}-");
    name.len() == prefix.len() + 64
        && name.starts_with(&prefix)
        && name[prefix.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_recovery_lease(name: &str, kind: &str) -> bool {
    let prefix = format!(".recovery-{kind}-");
    name.len() == prefix.len() + 64
        && name.starts_with(&prefix)
        && name[prefix.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_ledger_temporary(name: &str) -> bool {
    name.strip_prefix(".verdicts-")
        .and_then(|sequence| {
            sequence
                .parse::<u64>()
                .ok()
                .map(|parsed| (sequence, parsed))
        })
        .is_some_and(|(sequence, parsed)| parsed.to_string() == sequence)
}

pub fn has_current_temporary(paths: &StorePaths) -> Result<bool, StoreError> {
    for entry in fs::read_dir(&paths.canon).map_err(|_| StoreError::Read)? {
        let name = entry.map_err(|_| StoreError::Read)?.file_name();
        if name.to_str().is_some_and(valid_current_temporary) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn has_generation_or_index_staging(paths: &StorePaths) -> Result<bool, StoreError> {
    for path in [
        paths.canon.join("generations"),
        paths.derived.join("indexes"),
    ] {
        for entry in fs::read_dir(path).map_err(|_| StoreError::Read)? {
            let name = entry.map_err(|_| StoreError::Read)?.file_name();
            if name.to_str().is_some_and(valid_transaction_staging) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn valid_current_temporary(name: &str) -> bool {
    name.strip_prefix(".current-")
        .is_some_and(valid_transaction_id)
}

fn valid_transaction_staging(name: &str) -> bool {
    name.strip_prefix(".txn-").is_some_and(valid_transaction_id)
}

pub fn valid_proposal_temporary(name: &str) -> bool {
    proposal_temporary_tail(name).is_some_and(|tail| {
        valid_proposal_temporary_bound_tail(tail) || valid_proposal_temporary_legacy_tail(tail)
    })
}

fn proposal_temporary_proposal_id(name: &str) -> Option<&str> {
    let value = name.strip_prefix(".proposal-")?;
    let proposal_id = value.get(..68)?;
    valid_proposal_id(proposal_id).then_some(proposal_id)
}

pub fn proposal_temporary_owner_hash(name: &str) -> Option<&str> {
    let tail = proposal_temporary_tail(name)?;
    let mut segments = tail.split('-');
    let owner_hash = segments.next()?;
    let process = segments.next()?;
    let sequence = segments.next()?;
    if segments.next().is_some()
        || !valid_hash(owner_hash)
        || !valid_decimal_u64(process)
        || !valid_decimal_u64(sequence)
    {
        return None;
    }
    Some(owner_hash)
}

fn proposal_temporary_tail(name: &str) -> Option<&str> {
    let value = name.strip_prefix(".proposal-")?;
    let proposal_id = value.get(..68)?;
    if value.len() <= 70 || !valid_proposal_id(proposal_id) {
        return None;
    }
    value.get(68..)?.strip_prefix('-')
}

fn valid_proposal_temporary_bound_tail(tail: &str) -> bool {
    let mut segments = tail.split('-');
    let Some(owner_hash) = segments.next() else {
        return false;
    };
    let Some(process) = segments.next() else {
        return false;
    };
    let Some(sequence) = segments.next() else {
        return false;
    };
    segments.next().is_none()
        && valid_hash(owner_hash)
        && valid_decimal_u64(process)
        && valid_decimal_u64(sequence)
}

fn valid_proposal_temporary_legacy_tail(tail: &str) -> bool {
    let mut segments = tail.split('-');
    let Some(process) = segments.next() else {
        return false;
    };
    let Some(sequence) = segments.next() else {
        return false;
    };
    segments.next().is_none() && valid_decimal_u64(process) && valid_decimal_u64(sequence)
}

fn valid_decimal_u64(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value
            .parse::<u64>()
            .ok()
            .is_some_and(|parsed| parsed.to_string() == value)
}

fn valid_proposal_id(value: &str) -> bool {
    value.len() == 68
        && value.starts_with("prp-")
        && value[4..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_verdict_id(value: &str) -> bool {
    value.len() == 68 && value.starts_with("ver-") && valid_hash(&value[4..])
}

fn valid_transaction_id(value: &str) -> bool {
    value.len() == 68 && value.starts_with("txn-") && valid_hash(&value[4..])
}

fn valid_plan_preparation(value: &str) -> bool {
    value
        .strip_prefix(".plan-")
        .and_then(|value| value.strip_suffix(".json"))
        .is_some_and(valid_transaction_id)
}

fn valid_generation_id(value: &str) -> bool {
    value.len() == 20
        && value.starts_with("gen-")
        && value[4..].bytes().all(|byte| byte.is_ascii_digit())
        && &value[4..] != "0000000000000000"
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn sorted_entries(path: &Path) -> Result<Vec<fs::DirEntry>, StoreError> {
    let mut entries = fs::read_dir(path)
        .map_err(|_| StoreError::Read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| StoreError::Read)?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

fn verify_same_filesystem(paths: &StorePaths) -> Result<(), StoreError> {
    for path in [
        &paths.policies,
        &paths.seed,
        &paths.staging,
        &paths.inbox,
        &paths.intake,
        &paths.intake_proposals,
        &paths.intake_transactions,
        &paths.root.join("ledger"),
        &paths.canon,
        &paths.evidence,
        &paths.derived,
        &paths.transactions,
        &paths.locks,
    ] {
        if !same_filesystem(&paths.root, path)? {
            return Err(StoreError::CrossFilesystem);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn same_filesystem(root: &Path, path: &Path) -> Result<bool, StoreError> {
    use std::os::unix::fs::MetadataExt;

    let root = fs::metadata(root).map_err(|_| StoreError::Read)?;
    let path = fs::metadata(path).map_err(|_| StoreError::Read)?;
    Ok(root.dev() == path.dev())
}

#[cfg(not(unix))]
fn same_filesystem(_root: &Path, _path: &Path) -> Result<bool, StoreError> {
    Ok(false)
}

fn exact_directory(path: &Path, expected: &[&str]) -> Result<(), StoreError> {
    regular_directory(path)?;
    let actual: BTreeSet<String> = fs::read_dir(path)
        .map_err(|_| StoreError::Read)?
        .map(|entry| {
            entry
                .map_err(|_| StoreError::Read)?
                .file_name()
                .into_string()
                .map_err(|_| StoreError::PathUnsafe)
        })
        .collect::<Result<_, _>>()?;
    let expected: BTreeSet<String> = expected.iter().map(|value| (*value).to_owned()).collect();
    if actual == expected {
        Ok(())
    } else {
        Err(StoreError::LayoutInvalid)
    }
}

fn regular_file(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(StoreError::Symlink),
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(StoreError::LayoutInvalid),
        Err(_) => Err(StoreError::Read),
    }
}

fn regular_directory(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(StoreError::Symlink),
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(StoreError::LayoutInvalid),
        Err(_) => Err(StoreError::Read),
    }
}

fn inspect_seed(profile: &P3Profile, seed_root: &Path) -> Result<SeedEvidence, StoreError> {
    let seed_profile = super::canon::frozen_seed_profile(profile);
    let verification = verify_bundle(
        &seed_profile,
        seed_root,
        VerificationOptions::level(VerificationLevel::Three),
    );
    if verification.has_errors() {
        return Err(StoreError::LayoutInvalid);
    }
    let documents = verification.document_count();
    let bundle_hash = verification.bundle_hash().to_owned();
    let manifest = verification.manifest.ok_or(StoreError::LayoutInvalid)?;
    if verification.corpus_report.is_none() {
        return Err(StoreError::LayoutInvalid);
    }
    Ok(SeedEvidence {
        documents,
        manifest_hash: raw_hash(&seed_root.join("manifest.json"))?,
        corpus_report_hash: raw_hash(&seed_root.join("corpus-report.json"))?,
        bundle_hash,
        source_commit: manifest.commit,
    })
}

fn raw_hash(path: &Path) -> Result<String, StoreError> {
    fs::read(path)
        .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
        .map_err(|_| StoreError::Read)
}

fn parse_current(bytes: &[u8]) -> Result<CanonicalHead, StoreError> {
    if bytes == b"seed\n" {
        return Ok(CanonicalHead::Seed);
    }
    if bytes.len() == 21
        && bytes.starts_with(b"gen-")
        && bytes[4..20].iter().all(u8::is_ascii_digit)
        && bytes[20] == b'\n'
    {
        let value = std::str::from_utf8(&bytes[..20]).map_err(|_| StoreError::LayoutInvalid)?;
        return Ok(CanonicalHead::Generation(value.to_owned()));
    }
    Err(StoreError::LayoutInvalid)
}

#[allow(dead_code)]
pub fn sync_file(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[allow(dead_code)]
pub fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[allow(dead_code)]
pub fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| StoreError::Write)?;
    let result = (|| {
        file.write_all(bytes).map_err(|_| StoreError::Write)?;
        file.sync_all().map_err(|_| StoreError::Write)
    })();
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

#[allow(dead_code)]
pub fn atomic_replace_synced(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().ok_or(StoreError::Write)?;
    let temporary = create_temporary_file(parent, bytes)?;
    let replace = fs::rename(&temporary, path)
        .and_then(|_| sync_directory(parent))
        .map_err(|_| StoreError::Write);
    if replace.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    replace
}

#[allow(dead_code)]
fn create_temporary_file(parent: &Path, bytes: &[u8]) -> Result<PathBuf, StoreError> {
    for _ in 0..1_000 {
        let sequence = NEXT_TEMPORARY_FILE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".sovmem-write-{}-{sequence}", std::process::id()));
        match write_new_synced(&path, bytes) {
            Ok(()) => return Ok(path),
            Err(StoreError::Write) if path.exists() => continue,
            Err(error) => return Err(error),
        }
    }
    Err(StoreError::Write)
}
