mod support;

use sovmem::profile::{export_canary, load_p3_profile, ExportRequest, P3_PROFILE_VERSION};
use std::fs;
use std::path::{Path, PathBuf};
use support::GitFixture;

const SOURCE_PATH: &str = "Research/Welcome.md";
const NEUTRAL_SOURCE: &str = include_str!("fixtures/default-ssa-v0/source/Research/Welcome.md");
const FORBIDDEN_DEFAULT_PROFILE_TOKENS: &[&str] = &[
    "ssa-0",
    "khalil",
    "audy",
    "/volumes/himura",
    "honcho",
    "verdict",
    "trust score",
    "private memory",
];

fn profile_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/default-ssa-v0/profile.yaml")
}

#[test]
fn default_profile_remains_isolated_from_p3_v2() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    assert_eq!(
        load_p3_profile(&root.join("profiles/audy-ssa0-p3-v1/profile.yaml"))
            .unwrap_err()
            .code(),
        "profile.identifier"
    );
    let p3 = load_p3_profile(&root.join("profiles/audy-ssa0-p3-v2/profile.yaml"))
        .expect("load reviewed v2 profile");
    assert_eq!(p3.version, P3_PROFILE_VERSION);
    assert_eq!(
        p3.hash,
        "66aa2c017c04a968a67aa314ce132cb7467b0382cac7585b3f83e5524941b59f"
    );
}

fn committed_fixture(name: &str, source: &str) -> GitFixture {
    let repo = GitFixture::new(name);
    repo.write(SOURCE_PATH, source);
    repo.commit_all("add neutral SSA fixture", "2026-01-02T12:00:00Z");
    repo
}

fn export_request(repo: &GitFixture, area: &GitFixture, output_root: PathBuf) -> ExportRequest {
    let canary = format!(
        "mode: committed\nsource_commit: {}\ninclude:\n  - \"Research/*.md\"\nexclude: []\n",
        repo.head()
    );
    area.write("canary.yaml", &canary);

    ExportRequest {
        vault_root: repo.root.clone(),
        output_root,
        profile_path: profile_path(),
        canary_path: area.root.join("canary.yaml"),
    }
}

fn exported_text(output_root: &Path) -> String {
    fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("read exported directory") {
            let entry = entry.expect("read exported entry");
            if entry
                .file_type()
                .expect("read exported entry type")
                .is_dir()
            {
                collect_files(&entry.path(), files);
            } else {
                files.push(entry.path());
            }
        }
    }

    let mut files = Vec::new();
    collect_files(output_root, &mut files);
    files.sort();

    let mut bytes = Vec::new();
    for file in files {
        bytes.extend(fs::read(file).expect("read exported file"));
    }
    bytes.make_ascii_lowercase();
    String::from_utf8(bytes).expect("exported artifacts are UTF-8")
}

fn assert_no_default_profile_leaks(output_root: &Path) {
    let exported_text = exported_text(output_root);
    for forbidden in FORBIDDEN_DEFAULT_PROFILE_TOKENS {
        assert!(
            !exported_text.contains(forbidden),
            "leaked token: {forbidden}"
        );
    }
}

#[test]
fn neutral_default_profile_export_contains_no_private_profile_tokens() {
    let repo = committed_fixture("default-profile-neutral", NEUTRAL_SOURCE);
    let area = GitFixture::new("default-profile-neutral-output");
    let output_root = area.root.join("output");
    let source_before = repo.hash_worktree();

    assert_eq!(repo.read(SOURCE_PATH), NEUTRAL_SOURCE);

    let summary = export_canary(export_request(&repo, &area, output_root.clone()))
        .expect("neutral default profile exports through the shared exporter");

    assert_eq!(summary.exported, 1);
    assert_no_default_profile_leaks(&output_root);
    assert_eq!(repo.hash_worktree(), source_before);
}

#[test]
fn neutral_default_profile_rejects_cross_profile_links_without_publishing() {
    let source =
        format!("{NEUTRAL_SOURCE}\nRead [foreign context](vault://ssa-0/04-Research/Foreign).\n");
    let repo = committed_fixture("default-profile-cross-profile", &source);
    let area = GitFixture::new("default-profile-cross-profile-output");
    let output_root = area.root.join("output");

    let error = export_canary(export_request(&repo, &area, output_root.clone()))
        .expect_err("cross-profile links must fail before publication");

    assert_eq!(error.code(), "links.cross_profile");
    assert!(!output_root.exists(), "failed export published a bundle");
}
