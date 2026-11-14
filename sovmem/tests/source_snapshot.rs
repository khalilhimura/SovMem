mod support;

use sovmem::profile::open_committed_snapshot;
use std::process::{Command, Output};
use support::GitFixture;

const COMMITTED_BODY: &str = "# One\n\nCommitted body.\n";
const UPDATED_BODY: &str = "# One\n\nUpdated body.\n";

fn git(repo: &GitFixture, arguments: &[&str]) -> Output {
    let output = Command::new("git")
        .arg("-C")
        .arg(&repo.root)
        .env_remove("GIT_NO_REPLACE_OBJECTS")
        .env_remove("GIT_OPTIONAL_LOCKS")
        .env_remove("GIT_LITERAL_PATHSPECS")
        .args(arguments)
        .output()
        .expect("run fixture Git command");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn reads_the_pinned_commit_with_git_dates_and_raw_byte_hashes() {
    let repo = GitFixture::new("source-pinned-commit");
    repo.write("Research/One.md", COMMITTED_BODY);
    repo.commit_all("add research note", "2026-01-02T12:00:00Z");
    let first_commit = repo.head();

    repo.write("Research/One.md", UPDATED_BODY);
    repo.commit_all("update research note", "2026-01-05T12:00:00Z");

    let before = repo.hash_worktree();
    let snapshot = open_committed_snapshot(&repo.root, &first_commit, &["Research/One.md".into()])
        .expect("pinned source snapshot opens");

    assert_eq!(snapshot.vault_root, repo.root);
    assert_eq!(snapshot.commit, first_commit);
    assert!(snapshot.excluded.is_empty());
    assert_eq!(snapshot.documents.len(), 1);
    assert_eq!(snapshot.documents[0].source_path, "Research/One.md");
    assert_eq!(snapshot.documents[0].created, "2026-01-02");
    assert_eq!(snapshot.documents[0].updated, "2026-01-02");
    assert_eq!(snapshot.documents[0].bytes, COMMITTED_BODY.as_bytes());
    assert_eq!(
        snapshot.documents[0].source_hash,
        blake3::hash(COMMITTED_BODY.as_bytes()).to_hex().to_string()
    );
    assert_eq!(repo.hash_worktree(), before);
}

#[test]
fn ignores_git_replacement_refs_when_reading_a_pinned_commit() {
    let repo = GitFixture::new("source-ignore-replacements");
    repo.write("Research/One.md", COMMITTED_BODY);
    repo.commit_all("add original research note", "2026-01-02T12:00:00Z");
    let original_commit = repo.head();

    git(&repo, &["checkout", "--orphan", "replacement"]);
    git(&repo, &["rm", "--force", "--quiet", "-r", "."]);
    repo.write("Research/One.md", UPDATED_BODY);
    repo.commit_all("add replacement research note", "2026-01-05T12:00:00Z");
    let replacement_commit = repo.head();
    git(&repo, &["replace", &original_commit, &replacement_commit]);

    let replaced_object = format!("{original_commit}:Research/One.md");
    let replaced_bytes = git(&repo, &["show", &replaced_object]).stdout;
    assert_eq!(
        replaced_bytes,
        UPDATED_BODY.as_bytes(),
        "fixture must prove ordinary Git follows the replacement ref"
    );
    let replaced_date = git(
        &repo,
        &[
            "log",
            "-1",
            "--format=%cs",
            &original_commit,
            "--",
            "Research/One.md",
        ],
    );
    assert_eq!(
        String::from_utf8(replaced_date.stdout)
            .expect("replacement date is UTF-8")
            .trim(),
        "2026-01-05"
    );

    let before = repo.hash_worktree();
    let snapshot =
        open_committed_snapshot(&repo.root, &original_commit, &["Research/One.md".into()])
            .expect("pinned reads must ignore local replacement refs");

    assert_eq!(snapshot.commit, original_commit);
    assert!(
        snapshot.excluded.is_empty(),
        "replacement refs made the pinned path appear dirty"
    );
    assert_eq!(snapshot.documents.len(), 1);
    assert_eq!(snapshot.documents[0].bytes, COMMITTED_BODY.as_bytes());
    assert_eq!(snapshot.documents[0].created, "2026-01-02");
    assert_eq!(snapshot.documents[0].updated, "2026-01-02");
    assert_eq!(repo.hash_worktree(), before);
}

#[test]
fn sorts_documents_by_source_path() {
    let repo = GitFixture::new("source-sort");
    repo.write("Research/Zulu.md", "zulu\n");
    repo.write("Research/Alpha.md", "alpha\n");
    repo.commit_all("add sorted sources", "2026-01-02T12:00:00Z");
    let commit = repo.head();

    let before = repo.hash_worktree();
    let snapshot = open_committed_snapshot(
        &repo.root,
        &commit,
        &["Research/Zulu.md".into(), "Research/Alpha.md".into()],
    )
    .expect("source snapshot opens");

    let source_paths: Vec<_> = snapshot
        .documents
        .iter()
        .map(|document| document.source_path.as_str())
        .collect();
    assert_eq!(source_paths, ["Research/Alpha.md", "Research/Zulu.md"]);
    assert_eq!(repo.hash_worktree(), before);
}

#[test]
fn excludes_and_reports_a_selected_dirty_path() {
    let repo = GitFixture::new("source-dirty");
    repo.write("Research/One.md", COMMITTED_BODY);
    repo.write("Research/Two.md", "# Two\n\nCommitted body.\n");
    repo.commit_all("add research notes", "2026-01-02T12:00:00Z");

    repo.write("Research/One.md", UPDATED_BODY);
    repo.commit_all("update research note", "2026-01-05T12:00:00Z");
    let commit = repo.head();

    repo.write("Research/One.md", "# One\n\nUncommitted body.\n");
    let before = repo.hash_worktree();
    let snapshot = open_committed_snapshot(
        &repo.root,
        &commit,
        &["Research/One.md".into(), "Research/Two.md".into()],
    )
    .expect("dirty source is excluded without failing the snapshot");

    assert_eq!(snapshot.documents.len(), 1);
    assert_eq!(snapshot.documents[0].source_path, "Research/Two.md");
    assert_eq!(snapshot.excluded.len(), 1);
    assert_eq!(snapshot.excluded[0].source_path, "Research/One.md");
    assert_eq!(snapshot.excluded[0].reason, "dirty");
    assert_eq!(repo.read("Research/One.md"), "# One\n\nUncommitted body.\n");
    assert_eq!(repo.hash_worktree(), before);
}

#[test]
fn excludes_and_reports_a_selected_untracked_path() {
    let repo = GitFixture::new("source-untracked");
    repo.write("Research/One.md", COMMITTED_BODY);
    repo.commit_all("add research note", "2026-01-02T12:00:00Z");
    let commit = repo.head();

    repo.write("Research/Untracked.md", "# Untracked\n");
    let before = repo.hash_worktree();
    let snapshot = open_committed_snapshot(&repo.root, &commit, &["Research/Untracked.md".into()])
        .expect("untracked source is excluded without failing the snapshot");

    assert!(snapshot.documents.is_empty());
    assert_eq!(snapshot.excluded.len(), 1);
    assert_eq!(snapshot.excluded[0].source_path, "Research/Untracked.md");
    assert_eq!(snapshot.excluded[0].reason, "untracked");
    assert_eq!(repo.hash_worktree(), before);
}

#[test]
fn rejects_a_missing_commit_without_touching_the_worktree() {
    let repo = GitFixture::new("source-missing-commit");
    repo.write("Research/One.md", COMMITTED_BODY);
    repo.commit_all("add research note", "2026-01-02T12:00:00Z");
    let missing_commit = "0000000000000000000000000000000000000000";

    let before = repo.hash_worktree();
    let error = open_committed_snapshot(&repo.root, missing_commit, &["Research/One.md".into()])
        .expect_err("missing commit must fail");

    assert_eq!(error.code(), "source.commit_missing");
    assert_eq!(repo.hash_worktree(), before);
}

#[test]
fn rejects_a_missing_tracked_path_without_touching_the_worktree() {
    let repo = GitFixture::new("source-missing-path");
    repo.write("Research/One.md", COMMITTED_BODY);
    repo.commit_all("add research note", "2026-01-02T12:00:00Z");
    let commit = repo.head();

    let before = repo.hash_worktree();
    let error = open_committed_snapshot(&repo.root, &commit, &["Research/Missing.md".into()])
        .expect_err("missing tracked path must fail");

    assert_eq!(error.code(), "source.path_missing");
    assert_eq!(repo.hash_worktree(), before);
}
