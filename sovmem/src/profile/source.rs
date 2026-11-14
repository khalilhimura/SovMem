use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSnapshot {
    pub vault_root: PathBuf,
    pub commit: String,
    pub documents: Vec<SourceDocument>,
    pub excluded: Vec<ExcludedSource>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDocument {
    pub source_path: String,
    pub bytes: Vec<u8>,
    pub source_hash: String,
    pub created: String,
    pub updated: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExcludedSource {
    pub source_path: String,
    pub reason: String,
}

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("source commit does not exist: {0}")]
    CommitMissing(String),
    #[error("source commit resolved to {actual}, expected {expected}")]
    CommitMismatch { expected: String, actual: String },
    #[error("source path is missing from commit {commit}: {path}")]
    PathMissing { commit: String, path: String },
    #[error("source {date_kind} date is missing at commit {commit}: {path}")]
    DateMissing {
        commit: String,
        path: String,
        date_kind: &'static str,
    },
    #[error("Git {operation} failed: {message}")]
    Git {
        operation: &'static str,
        message: String,
    },
}

impl SourceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::CommitMissing(_) => "source.commit_missing",
            Self::CommitMismatch { .. } => "source.commit_mismatch",
            Self::PathMissing { .. } => "source.path_missing",
            Self::DateMissing { .. } => "source.date_missing",
            Self::Git { .. } => "source.git",
        }
    }
}

pub fn open_committed_snapshot(
    vault_root: &Path,
    expected_commit: &str,
    paths: &[String],
) -> Result<SourceSnapshot, SourceError> {
    let commit = resolve_commit(vault_root, expected_commit)?;
    let mut documents = Vec::new();
    let mut excluded = Vec::new();
    let selected_paths: BTreeSet<&str> = paths.iter().map(String::as_str).collect();

    for source_path in selected_paths {
        if let Some(reason) = selected_path_exclusion(vault_root, source_path)? {
            excluded.push(ExcludedSource {
                source_path: source_path.to_owned(),
                reason: reason.to_owned(),
            });
            continue;
        }

        let bytes = read_committed_bytes(vault_root, &commit, source_path)?;
        let created = created_date(vault_root, &commit, source_path)?;
        let updated = updated_date(vault_root, &commit, source_path)?;
        let source_hash = blake3::hash(&bytes).to_hex().to_string();
        documents.push(SourceDocument {
            source_path: source_path.to_owned(),
            bytes,
            source_hash,
            created,
            updated,
        });
    }

    Ok(SourceSnapshot {
        vault_root: vault_root.to_path_buf(),
        commit,
        documents,
        excluded,
    })
}

fn resolve_commit(vault_root: &Path, expected_commit: &str) -> Result<String, SourceError> {
    let revision = format!("{expected_commit}^{{commit}}");
    let output = read_only_git(vault_root)
        .args(["rev-parse", "--verify", &revision])
        .output()
        .map_err(|error| git_error("rev-parse", error.to_string()))?;
    if !output.status.success() {
        return Err(SourceError::CommitMissing(expected_commit.to_owned()));
    }

    let actual = stdout_text(output, "rev-parse")?.trim().to_owned();
    if actual != expected_commit {
        return Err(SourceError::CommitMismatch {
            expected: expected_commit.to_owned(),
            actual,
        });
    }
    Ok(actual)
}

fn selected_path_exclusion(
    vault_root: &Path,
    source_path: &str,
) -> Result<Option<&'static str>, SourceError> {
    let output = read_only_git(vault_root)
        .args([
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--",
            source_path,
        ])
        .output()
        .map_err(|error| git_error("status", error.to_string()))?;
    if !output.status.success() {
        return Err(git_output_error("status", &output));
    }
    if output.stdout.is_empty() {
        return Ok(None);
    }
    if output.stdout.starts_with(b"??") {
        return Ok(Some("untracked"));
    }
    Ok(Some("dirty"))
}

fn read_committed_bytes(
    vault_root: &Path,
    commit: &str,
    source_path: &str,
) -> Result<Vec<u8>, SourceError> {
    let object = format!("{commit}:{source_path}");
    let output = read_only_git(vault_root)
        .args(["show", &object])
        .output()
        .map_err(|error| git_error("show", error.to_string()))?;
    if !output.status.success() {
        return Err(SourceError::PathMissing {
            commit: commit.to_owned(),
            path: source_path.to_owned(),
        });
    }
    Ok(output.stdout)
}

fn created_date(vault_root: &Path, commit: &str, source_path: &str) -> Result<String, SourceError> {
    let output = read_only_git(vault_root)
        .args([
            "log",
            "--follow",
            "--diff-filter=A",
            "--format=%cs",
            commit,
            "--",
            source_path,
        ])
        .output()
        .map_err(|error| git_error("log created", error.to_string()))?;
    let text = successful_stdout(output, "log created")?;
    text.lines()
        .rev()
        .find(|date| !date.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| SourceError::DateMissing {
            commit: commit.to_owned(),
            path: source_path.to_owned(),
            date_kind: "created",
        })
}

fn updated_date(vault_root: &Path, commit: &str, source_path: &str) -> Result<String, SourceError> {
    let output = read_only_git(vault_root)
        .args(["log", "-1", "--format=%cs", commit, "--", source_path])
        .output()
        .map_err(|error| git_error("log updated", error.to_string()))?;
    let text = successful_stdout(output, "log updated")?;
    text.lines()
        .find(|date| !date.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| SourceError::DateMissing {
            commit: commit.to_owned(),
            path: source_path.to_owned(),
            date_kind: "updated",
        })
}

pub(super) fn read_only_git(vault_root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(vault_root);
    command
}

fn successful_stdout(output: Output, operation: &'static str) -> Result<String, SourceError> {
    if !output.status.success() {
        return Err(git_output_error(operation, &output));
    }
    stdout_text(output, operation)
}

fn stdout_text(output: Output, operation: &'static str) -> Result<String, SourceError> {
    String::from_utf8(output.stdout)
        .map_err(|error| git_error(operation, format!("stdout was not UTF-8: {error}")))
}

fn git_output_error(operation: &'static str, output: &Output) -> SourceError {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    git_error(operation, format!("{}: {stderr}", output.status))
}

fn git_error(operation: &'static str, message: String) -> SourceError {
    SourceError::Git { operation, message }
}
