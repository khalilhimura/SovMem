mod support;

use serde_json::Value;
use sovmem::profile::{export_canary, ExportRequest};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use support::GitFixture;

static NEXT_AREA_ID: AtomicU64 = AtomicU64::new(0);

struct TemporaryArea {
    root: PathBuf,
}

impl TemporaryArea {
    fn new(name: &str) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after the Unix epoch")
            .as_nanos();
        let sequence = NEXT_AREA_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-export-{name}-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create temporary export area");
        Self { root }
    }
}

impl Drop for TemporaryArea {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn profile_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/profile.yaml")
}

fn write_canary(
    area: &TemporaryArea,
    name: &str,
    commit: &str,
    include: &[&str],
    exclude: &[&str],
) -> PathBuf {
    let mut yaml = format!("mode: committed\nsource_commit: {commit}\ninclude:\n");
    for pattern in include {
        yaml.push_str(&format!("  - {pattern:?}\n"));
    }
    yaml.push_str("exclude:\n");
    for pattern in exclude {
        yaml.push_str(&format!("  - {pattern:?}\n"));
    }
    if exclude.is_empty() {
        yaml.push_str("  []\n");
    }

    let path = area.root.join(format!("{name}.yaml"));
    fs::write(&path, yaml).expect("write canary configuration");
    path
}

fn request(repo: &GitFixture, output_root: PathBuf, canary_path: PathBuf) -> ExportRequest {
    ExportRequest {
        vault_root: repo.root.clone(),
        output_root,
        profile_path: profile_path(),
        canary_path,
    }
}

fn three_document_fixture() -> GitFixture {
    let repo = GitFixture::new("export-three-documents");
    repo.write(
        "04-Research/OKF/Alpha-Concept-Map.md",
        "# Alpha concept map\n\nThis synthesis maps the source and the research brief.\n\nSee [[README]].\n",
    );
    repo.write(
        "04-Research/OKF/README.md",
        "# OKF research brief\n\nThis research brief describes the selected evidence and its boundaries.\n",
    );
    repo.write(
        "04-Research/OKF/source-pages/Paper.md",
        "# Primary paper\n\nThis source snapshot records the primary paper used by the synthesis.\n",
    );
    repo.commit_all("add export fixture", "2026-01-02T12:00:00Z");
    repo
}

fn tree_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        let mut entries: Vec<_> = fs::read_dir(directory)
            .expect("read export tree")
            .map(|entry| entry.expect("read export entry"))
            .collect();
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let path = entry.path();
            if entry.file_type().expect("read export entry type").is_dir() {
                visit(root, &path, files);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("export path stays below root")
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(relative, fs::read(path).expect("read exported file"));
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn json_file(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read JSON file")).expect("parse JSON file")
}

fn committed_bytes(repo: &GitFixture, commit: &str, source_path: &str) -> Vec<u8> {
    let object = format!("{commit}:{source_path}");
    let output = Command::new("git")
        .arg("-C")
        .arg(&repo.root)
        .args(["show", &object])
        .output()
        .expect("read committed source independently");
    assert!(
        output.status.success(),
        "git show failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn git(repo: &GitFixture, arguments: &[&str]) -> std::process::Output {
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

fn canonical_bundle_hash(output_root: &Path, manifest: &Value) -> String {
    let mut paths: Vec<_> = manifest["documents"]
        .as_array()
        .expect("manifest documents are an array")
        .iter()
        .map(|document| {
            document["output_path"]
                .as_str()
                .expect("document output path is a string")
                .to_owned()
        })
        .collect();
    paths.push("bundle/index.md".into());
    paths.sort();
    paths.dedup();

    // Independently reproduce the canonical sequence: for each UTF-8 path in ascending
    // byte order, hash u64-LE path length, path bytes, u64-LE content length, raw bytes.
    let mut hasher = blake3::Hasher::new();
    for path in paths {
        let bytes = fs::read(output_root.join(&path)).expect("read canonical bundle input");
        hasher.update(&(path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    hasher.finalize().to_hex().to_string()
}

#[test]
fn exports_identical_validated_trees_without_mutating_sources() {
    let repo = three_document_fixture();
    let area = TemporaryArea::new("deterministic");
    let canary_path = write_canary(
        &area,
        "canary",
        &repo.head(),
        &["04-Research/OKF/**/*.md"],
        &[],
    );
    let first_output = area.root.join("first");
    let second_output = area.root.join("second");
    let source_before = repo.hash_worktree();

    let first = export_canary(request(&repo, first_output.clone(), canary_path.clone()))
        .expect("first export succeeds");
    let second = export_canary(request(&repo, second_output.clone(), canary_path))
        .expect("second export succeeds");

    assert_eq!(first, second);
    assert_eq!(first.exported, 3);
    assert_eq!(first.errors, 0);
    assert_eq!(second.exported, 3);
    assert_eq!(tree_bytes(&first_output), tree_bytes(&second_output));
    assert_eq!(source_before, repo.hash_worktree());
    assert!(repo
        .read("04-Research/OKF/Alpha-Concept-Map.md")
        .contains("This synthesis maps the source"));

    let paths: Vec<_> = tree_bytes(&first_output).into_keys().collect();
    assert_eq!(
        paths,
        [
            "bundle/04-Research/OKF/Alpha-Concept-Map.md",
            "bundle/04-Research/OKF/README.md",
            "bundle/04-Research/OKF/source-pages/Paper.md",
            "bundle/index.md",
            "export-report.json",
            "manifest.json",
        ]
    );

    let manifest_text =
        fs::read_to_string(first_output.join("manifest.json")).expect("read generated manifest");
    let manifest: Value = serde_json::from_str(&manifest_text).expect("parse generated manifest");
    assert_eq!(manifest["nous_schema"], "nous-ssa-context-v0");
    assert_eq!(manifest["profile"], "nous-sovmem-v0");
    assert_eq!(manifest["mode"], "committed");
    assert_eq!(manifest["documents"].as_array().unwrap().len(), 3);
    assert_eq!(manifest["sources"].as_array().unwrap().len(), 3);
    assert_eq!(
        manifest["inclusion"],
        serde_json::json!(["04-Research/OKF/**/*.md"])
    );
    assert_eq!(manifest["exclusion"], serde_json::json!([]));
    assert_eq!(manifest["commit"], repo.head());
    let independently_computed_bundle_hash = canonical_bundle_hash(&first_output, &manifest);
    assert_eq!(manifest["bundle_hash"], independently_computed_bundle_hash);
    assert_eq!(first.bundle_hash, independently_computed_bundle_hash);

    for source in manifest["sources"].as_array().unwrap() {
        let source_path = source["source_path"].as_str().unwrap();
        let bytes = committed_bytes(&repo, manifest["commit"].as_str().unwrap(), source_path);
        assert_eq!(
            source["hash"].as_str().unwrap(),
            blake3::hash(&bytes).to_hex().as_str()
        );
    }

    for document in manifest["documents"].as_array().unwrap() {
        let output_path = document["output_path"].as_str().unwrap();
        let bytes = fs::read(first_output.join(output_path)).expect("read generated document");
        assert_eq!(
            document["hash"].as_str().unwrap(),
            blake3::hash(&bytes).to_hex().as_str()
        );
    }

    let index =
        fs::read_to_string(first_output.join("bundle/index.md")).expect("read generated index");
    let destinations: Vec<_> = index
        .lines()
        .filter_map(|line| line.strip_prefix("- ["))
        .map(|line| line.rsplit_once("](").unwrap().1.trim_end_matches(')'))
        .collect();
    let mut sorted_destinations = destinations.clone();
    sorted_destinations.sort_unstable();
    assert_eq!(destinations, sorted_destinations);

    let report_text =
        fs::read_to_string(first_output.join("export-report.json")).expect("read export report");
    assert!(manifest_text.ends_with('\n'));
    assert!(report_text.ends_with('\n'));
    for (name, text) in [("manifest", &manifest_text), ("report", &report_text)] {
        assert!(!text.contains(repo.root.to_string_lossy().as_ref()));
        assert!(!text.contains(first_output.to_string_lossy().as_ref()));
        for forbidden in [
            "timestamp",
            "hostname",
            "username",
            "vault_root",
            "output_root",
        ] {
            assert!(!text.contains(forbidden), "{name} leaked {forbidden}");
        }
    }
}

#[test]
fn export_ignores_replacement_refs_and_keeps_the_manifest_bound_to_the_pinned_commit() {
    const SOURCE: &str = "04-Research/OKF/README.md";
    const ORIGINAL: &str = "# Original research brief\n\nThis is the pinned original evidence.\n";
    const REPLACEMENT: &str =
        "# Replacement research brief\n\nThis must not replace the pinned evidence.\n";

    let repo = GitFixture::new("export-ignore-replacements");
    repo.write(SOURCE, ORIGINAL);
    repo.commit_all("add original source", "2026-01-02T12:00:00Z");
    let original_commit = repo.head();

    git(&repo, &["checkout", "--orphan", "replacement-export"]);
    git(&repo, &["rm", "--force", "--quiet", "-r", "."]);
    repo.write(SOURCE, REPLACEMENT);
    repo.commit_all("add replacement source", "2026-01-05T12:00:00Z");
    let replacement_commit = repo.head();
    git(&repo, &["replace", &original_commit, &replacement_commit]);

    let area = TemporaryArea::new("ignore-replacements");
    let canary_path = write_canary(&area, "canary", &original_commit, &[SOURCE], &[]);
    let output = area.root.join("output");
    let source_before = repo.hash_worktree();

    let summary = export_canary(request(&repo, output.clone(), canary_path))
        .expect("replacement refs must not affect pinned export");

    assert_eq!(summary.exported, 1);
    let manifest = json_file(&output.join("manifest.json"));
    assert_eq!(manifest["commit"], original_commit);
    assert_eq!(
        manifest["sources"][0]["hash"],
        blake3::hash(ORIGINAL.as_bytes()).to_hex().as_str()
    );
    let generated =
        fs::read_to_string(output.join("bundle").join(SOURCE)).expect("read generated source");
    assert!(generated.contains("This is the pinned original evidence."));
    assert!(!generated.contains("This must not replace the pinned evidence."));
    assert!(generated.contains("created: 2026-01-02"));
    assert!(generated.contains("updated: 2026-01-02"));
    assert_eq!(repo.hash_worktree(), source_before);
}

#[test]
fn validation_failure_publishes_no_output() {
    let repo = GitFixture::new("export-validation-failure");
    repo.write(
        "04-Research/Invalid-Concept-Map.md",
        "---\ncreated: 2026-02-30\nupdated: 2026-02-30\n---\n# Invalid date\n\nThis document has invalid explicit source evidence dates.\n",
    );
    repo.commit_all("add invalid source evidence", "2026-01-02T12:00:00Z");
    let area = TemporaryArea::new("validation-failure");
    let canary_path = write_canary(
        &area,
        "canary",
        &repo.head(),
        &["04-Research/*Concept-Map*.md"],
        &[],
    );
    let output = area.root.join("output");
    fs::create_dir(&output).expect("create existing output");
    fs::create_dir(output.join("prior")).expect("create existing output subtree");
    fs::write(
        output.join("prior/sentinel.txt"),
        "validated prior output\n",
    )
    .expect("write existing output sentinel");
    let output_before = tree_bytes(&output);

    let error = export_canary(request(&repo, output.clone(), canary_path))
        .expect_err("invalid source evidence must fail before publication");

    assert_eq!(error.code(), "export.validation");
    assert_eq!(tree_bytes(&output), output_before);
}

#[test]
fn rejects_an_output_root_inside_the_vault() {
    let repo = three_document_fixture();
    let area = TemporaryArea::new("inside-vault");
    let canary_path = write_canary(
        &area,
        "canary",
        &repo.head(),
        &["04-Research/OKF/**/*.md"],
        &[],
    );
    let output = repo.root.join("generated/export");

    let error = export_canary(request(&repo, output.clone(), canary_path))
        .expect_err("output beneath the vault must fail closed");

    assert_eq!(error.code(), "export.output_overlap");
    assert!(!output.exists());
}

#[test]
fn rejects_a_nonexistent_output_alias_with_parent_traversal_before_any_write() {
    let repo = three_document_fixture();
    let area = TemporaryArea::new("parent-traversal-alias");
    let canary_path = write_canary(
        &area,
        "canary",
        &repo.head(),
        &["04-Research/OKF/**/*.md"],
        &[],
    );
    let vault_parent = repo.root.parent().expect("fixture vault has a parent");
    let missing = vault_parent.join(format!(
        "{}-missing",
        area.root
            .file_name()
            .and_then(|name| name.to_str())
            .expect("temporary area name is UTF-8")
    ));
    let output_alias = missing
        .join("..")
        .join(repo.root.file_name().expect("fixture vault has a name"))
        .join("generated");
    let actual_output = repo.root.join("generated");
    let source_before = repo.hash_worktree();

    let result = export_canary(request(&repo, output_alias, canary_path));
    let source_after = repo.hash_worktree();
    let missing_was_created = missing.exists();
    let output_was_created = actual_output.exists();
    let _ = fs::remove_dir_all(&actual_output);
    let _ = fs::remove_dir_all(&missing);

    let error = result.expect_err("unresolved parent traversal must fail closed");
    assert_eq!(error.code(), "export.output_invalid");
    assert_eq!(source_after, source_before);
    assert!(!missing_was_created);
    assert!(!output_was_created);
}

#[test]
fn rejects_a_missing_pinned_source_commit_without_output() {
    let repo = three_document_fixture();
    let area = TemporaryArea::new("commit-mismatch");
    let canary_path = write_canary(
        &area,
        "canary",
        "0000000000000000000000000000000000000000",
        &["04-Research/OKF/**/*.md"],
        &[],
    );
    let output = area.root.join("output");

    let error = export_canary(request(&repo, output.clone(), canary_path))
        .expect_err("missing pinned source commit must fail");

    assert_eq!(error.code(), "source.commit_missing");
    assert!(!output.exists());
}

#[test]
fn rejects_a_missing_exact_selection_instead_of_exporting_a_partial_bundle() {
    let repo = three_document_fixture();
    let area = TemporaryArea::new("missing-exact-selection");
    let canary_path = write_canary(
        &area,
        "canary",
        &repo.head(),
        &["04-Research/OKF/README.md", "04-Research/OKF/Missing.md"],
        &[],
    );
    let output = area.root.join("output");

    let error = export_canary(request(&repo, output.clone(), canary_path))
        .expect_err("a missing exact selection must fail the whole export");

    assert_eq!(error.code(), "source.path_missing");
    assert!(
        !output.exists(),
        "partial selection published an output bundle"
    );
}

#[test]
fn rejects_a_source_commit_that_resolves_to_a_different_commit_object() {
    let repo = three_document_fixture();
    let tag = Command::new("git")
        .arg("-C")
        .arg(&repo.root)
        .args([
            "tag",
            "--annotate",
            "snapshot-tag",
            "--message",
            "snapshot tag",
        ])
        .output()
        .expect("create annotated tag");
    assert!(
        tag.status.success(),
        "create annotated tag: {}",
        String::from_utf8_lossy(&tag.stderr)
    );
    let tag_object = Command::new("git")
        .arg("-C")
        .arg(&repo.root)
        .args(["rev-parse", "snapshot-tag^{tag}"])
        .output()
        .expect("resolve annotated tag object");
    assert!(tag_object.status.success());
    let tag_object = String::from_utf8(tag_object.stdout)
        .expect("tag object ID is UTF-8")
        .trim()
        .to_owned();
    let area = TemporaryArea::new("commit-object-mismatch");
    let canary_path = write_canary(
        &area,
        "canary",
        &tag_object,
        &["04-Research/OKF/**/*.md"],
        &[],
    );
    let output = area.root.join("output");

    let error = export_canary(request(&repo, output.clone(), canary_path))
        .expect_err("tag object must not silently resolve to its target commit");

    assert_eq!(error.code(), "source.commit_mismatch");
    assert!(!output.exists());
}

#[test]
fn replaces_an_existing_output_only_after_validation() {
    let repo = three_document_fixture();
    let area = TemporaryArea::new("replace-existing");
    let canary_path = write_canary(
        &area,
        "canary",
        &repo.head(),
        &["04-Research/OKF/**/*.md"],
        &[],
    );
    let output = area.root.join("output");
    fs::create_dir(&output).expect("create prior output");
    fs::write(output.join("sentinel.txt"), "prior export\n").expect("write prior output");

    let summary = export_canary(request(&repo, output.clone(), canary_path))
        .expect("valid export replaces prior output");

    assert_eq!(summary.exported, 3);
    assert!(!output.join("sentinel.txt").exists());
    assert!(output.join("manifest.json").is_file());
}

#[cfg(unix)]
#[test]
fn backup_cleanup_failure_keeps_the_valid_new_output_published() {
    use std::os::unix::fs::PermissionsExt;

    let repo = three_document_fixture();
    let area = TemporaryArea::new("backup-cleanup-failure");
    let canary_path = write_canary(
        &area,
        "canary",
        &repo.head(),
        &["04-Research/OKF/**/*.md"],
        &[],
    );
    let output = area.root.join("output");
    let locked = output.join("locked");
    fs::create_dir(&output).expect("create prior output");
    fs::create_dir(&locked).expect("create locked prior subtree");
    fs::write(locked.join("sentinel.txt"), "prior export\n").expect("write prior sentinel");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o500))
        .expect("make prior subtree non-removable");

    let result = export_canary(request(&repo, output.clone(), canary_path));
    let new_output_was_preserved = output.join("manifest.json").is_file();
    let backup_paths: Vec<_> = fs::read_dir(&area.root)
        .expect("read export area")
        .map(|entry| entry.expect("read export area entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".sovmem-backup-"))
        })
        .collect();

    for root in std::iter::once(&output).chain(backup_paths.iter()) {
        let locked = root.join("locked");
        if locked.exists() {
            fs::set_permissions(locked, fs::Permissions::from_mode(0o700))
                .expect("restore cleanup permissions");
        }
    }

    let error = result.expect_err("backup cleanup failure is reported");
    assert_eq!(error.code(), "export.publish");
    assert!(new_output_was_preserved);
    assert_eq!(backup_paths.len(), 1);
}

#[test]
fn reports_selected_dirty_and_untracked_sources_through_validation_adapters() {
    let repo = GitFixture::new("export-exclusions");
    repo.write(
        "04-Research/Clean-Concept-Map.md",
        "# Clean\n\nThis clean selected document remains exportable.\n",
    );
    repo.write(
        "04-Research/Dirty-Concept-Map.md",
        "# Dirty\n\nThis selected document starts committed.\n",
    );
    repo.commit_all("add selected documents", "2026-01-02T12:00:00Z");
    repo.write(
        "04-Research/Dirty-Concept-Map.md",
        "# Dirty\n\nThis selected document now has working-tree changes.\n",
    );
    repo.write(
        "04-Research/Untracked-Concept-Map.md",
        "# Untracked\n\nThis selected document is not committed.\n",
    );
    let source_before = repo.hash_worktree();
    let area = TemporaryArea::new("exclusions");
    let canary_path = write_canary(
        &area,
        "canary",
        &repo.head(),
        &["04-Research/*Concept-Map.md"],
        &[],
    );
    let output = area.root.join("output");

    let summary = export_canary(request(&repo, output.clone(), canary_path))
        .expect("selected working-tree changes are excluded as warnings");

    assert_eq!(summary.exported, 1);
    assert_eq!(summary.excluded, 2);
    assert_eq!(source_before, repo.hash_worktree());
    let report = json_file(&output.join("export-report.json"));
    let codes: Vec<_> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"source.excluded_dirty"));
    assert!(codes.contains(&"source.excluded_untracked"));
}

#[test]
fn exports_fifty_documents_without_changing_the_schema() {
    let repo = GitFixture::new("export-fifty-documents");
    for index in 0..50 {
        repo.write(
            &format!("04-Research/Corpus/Doc-{index:03}-Concept-Map.md"),
            &format!(
                "# Concept map {index:03}\n\nThis deterministic synthesis is document {index:03} in the expansion corpus.\n"
            ),
        );
    }
    repo.commit_all("add expansion corpus", "2026-01-02T12:00:00Z");
    let area = TemporaryArea::new("fifty");
    let canary_path = write_canary(
        &area,
        "canary",
        &repo.head(),
        &["04-Research/Corpus/*Concept-Map.md"],
        &[],
    );
    let output = area.root.join("output");

    let summary = export_canary(request(&repo, output.clone(), canary_path))
        .expect("50-document export succeeds with the existing schema");

    assert_eq!(summary.exported, 50);
    assert_eq!(summary.errors, 0);
    let manifest = json_file(&output.join("manifest.json"));
    assert_eq!(manifest["nous_schema"], "nous-ssa-context-v0");
    assert_eq!(manifest["documents"].as_array().unwrap().len(), 50);
}
