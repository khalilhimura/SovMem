#[allow(dead_code)]
mod support;

use serde_json::Value;
use sovmem::profile::{
    glob_matches, load_canary, load_corpus, load_profile, CorpusTheme, PINNED_SOURCE_COMMIT,
};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use support::GitFixture;

static NEXT_AREA_ID: AtomicU64 = AtomicU64::new(0);

const SOURCE_PATH: &str = "04-Research/OKF/README.md";
const NOTE_SENTINEL: &str = "DO_NOT_LEAK_NOTE_BODY_7f2c8b";
const SECRET_SENTINEL: &str = "DO_NOT_LEAK_TOKEN_91a4de";
const RESTRICTED_BUFFER: &str = "04-Research/DSOM/source-pages/2.-operational-rituals__digital-sovereignty-operational-model-palace.md";

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
            "sovmem-cli-{name}-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create temporary CLI area");
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

fn canary_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/canary.yaml")
}

fn corpus_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml")
}

fn fixture(name: &str) -> GitFixture {
    let repo = GitFixture::new(name);
    repo.write(
        SOURCE_PATH,
        &format!(
            "# Canary research brief\n\nThis prose contains {NOTE_SENTINEL} and must never reach CLI output. The private marker {SECRET_SENTINEL} must stay in the note too.\n"
        ),
    );
    repo.commit_all("add canary note", "2026-01-02T12:00:00Z");
    repo
}

fn write_canary(area: &TemporaryArea, commit: &str) -> PathBuf {
    write_canary_selection(area, commit, &[SOURCE_PATH])
}

fn write_canary_selection(area: &TemporaryArea, commit: &str, include: &[&str]) -> PathBuf {
    write_canary_rules(area, commit, include, &[])
}

fn write_canary_rules(
    area: &TemporaryArea,
    commit: &str,
    include: &[&str],
    exclude: &[&str],
) -> PathBuf {
    let path = area.root.join("canary.yaml");
    let mut yaml = format!("mode: committed\nsource_commit: {commit}\ninclude:\n");
    for source_path in include {
        yaml.push_str(&format!("  - {source_path}\n"));
    }
    if exclude.is_empty() {
        yaml.push_str("exclude: []\n");
    } else {
        yaml.push_str("exclude:\n");
        for source_path in exclude {
            yaml.push_str(&format!("  - {source_path}\n"));
        }
    }
    fs::write(&path, yaml).expect("write test canary");
    path
}

fn run_export(repo: &GitFixture, profile: &Path, canary: &Path, output_root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["export", "--vault"])
        .arg(&repo.root)
        .args(["--profile"])
        .arg(profile)
        .args(["--canary"])
        .arg(canary)
        .args(["--output"])
        .arg(output_root)
        .output()
        .expect("run sovmem export")
}

fn run_corpus(vault: &Path, profile: &Path, corpus: &Path, output_root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["corpus", "--vault"])
        .arg(vault)
        .args(["--profile"])
        .arg(profile)
        .args(["--corpus"])
        .arg(corpus)
        .args(["--output"])
        .arg(output_root)
        .output()
        .expect("run sovmem corpus")
}

fn run_verify(profile: &Path, bundle: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["verify", "--profile"])
        .arg(profile)
        .args(["--bundle"])
        .arg(bundle)
        .output()
        .expect("run sovmem verify")
}

fn run_verify_require_corpus(profile: &Path, bundle: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["verify", "--profile"])
        .arg(profile)
        .args(["--bundle"])
        .arg(bundle)
        .arg("--require-corpus")
        .output()
        .expect("run corpus-required bundle verifier")
}

fn run_verify_level(profile: &Path, bundle: &Path, level: u8) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["verify", "--profile"])
        .arg(profile)
        .args(["--bundle"])
        .arg(bundle)
        .args(["--level", &level.to_string()])
        .output()
        .expect("run leveled verifier")
}

fn run_verify_level_require_corpus(profile: &Path, bundle: &Path, level: u8) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["verify", "--profile"])
        .arg(profile)
        .args(["--bundle"])
        .arg(bundle)
        .args(["--level", &level.to_string(), "--require-corpus"])
        .output()
        .expect("run conflicting verifier")
}

fn run_stats(profile: &Path, bundle: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["stats", "--profile"])
        .arg(profile)
        .args(["--bundle"])
        .arg(bundle)
        .output()
        .expect("run sovmem stats")
}

fn run_verify_golden(profile: &Path, bundle: &Path, golden: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["verify-golden", "--profile"])
        .arg(profile)
        .args(["--bundle"])
        .arg(bundle)
        .args(["--golden"])
        .arg(golden)
        .output()
        .expect("run sovmem verify-golden")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

fn assert_no_source_content(output: &Output) {
    let combined = format!("{}{}", stdout(output), stderr(output));
    assert!(!combined.contains(NOTE_SENTINEL));
    assert!(!combined.contains(SECRET_SENTINEL));
    assert!(!combined.contains(SOURCE_PATH));
}

fn assert_no_paths(output: &Output, paths: &[&Path]) {
    let combined = format!("{}{}", stdout(output), stderr(output));
    for path in paths {
        assert!(
            !combined.contains(&path.to_string_lossy().to_string()),
            "CLI output leaked absolute path {path:?}: {combined}"
        );
    }
}

struct CorpusCliFixture {
    repo: GitFixture,
    _git_environment: GitEnvironmentGuard,
}

impl CorpusCliFixture {
    fn new() -> Self {
        let repo = GitFixture::new("corpus-cli");
        for path in load_corpus(&corpus_path())
            .expect("load approved corpus for CLI fixture")
            .selected_paths()
        {
            repo.write(
                &path,
                &format!(
                    "---\ndescription: Deterministic reviewed corpus evidence.\nprovenance: synthesis\nlinks:\n  - rel: cites\n    target: https://example.com/source\n---\n# Corpus fixture\n\nThis reviewed source contains {NOTE_SENTINEL} and deterministic evidence.\n"
                ),
            );
        }
        repo.commit_all("add corpus fixture", "2026-01-02T12:00:00Z");
        let git_environment = install_git_shim(&repo.head());
        Self {
            repo,
            _git_environment: git_environment,
        }
    }

    fn dirty(&self, path: &str) {
        let source = self.repo.root.join(path);
        let mut body = fs::read_to_string(&source).expect("read corpus fixture source");
        body.push_str("\n<!-- reviewed corpus fixture dirtiness -->\n");
        fs::write(source, body).expect("dirty corpus fixture source");
    }
}

struct GitEnvironmentState {
    users: usize,
    actual_commit: Option<String>,
    original_path: Option<OsString>,
    original_actual_commit: Option<OsString>,
}

struct GitEnvironmentGuard;

impl Drop for GitEnvironmentGuard {
    fn drop(&mut self) {
        let mut state = git_environment_state().lock().unwrap();
        state.users -= 1;
        if state.users == 0 {
            restore_environment("PATH", state.original_path.take());
            restore_environment(
                "SOVMEM_TEST_ACTUAL_COMMIT",
                state.original_actual_commit.take(),
            );
            state.actual_commit = None;
        }
    }
}

fn git_environment_state() -> &'static Mutex<GitEnvironmentState> {
    static STATE: OnceLock<Mutex<GitEnvironmentState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(GitEnvironmentState {
            users: 0,
            actual_commit: None,
            original_path: None,
            original_actual_commit: None,
        })
    })
}

fn restore_environment(name: &str, value: Option<OsString>) {
    if let Some(value) = value {
        std::env::set_var(name, value);
    } else {
        std::env::remove_var(name);
    }
}

fn install_git_shim(actual_commit: &str) -> GitEnvironmentGuard {
    use std::os::unix::fs::PermissionsExt;

    static SHIM: OnceLock<PathBuf> = OnceLock::new();
    let shim = SHIM.get_or_init(|| {
        let directory =
            std::env::temp_dir().join(format!("sovmem-corpus-cli-git-shim-{}", std::process::id()));
        fs::create_dir(&directory).expect("create corpus CLI Git shim directory");
        let path = directory.join("git");
        let script = format!(
            "#!/bin/bash\nset -e\npinned='{PINNED_SOURCE_COMMIT}'\nactual=\"${{SOVMEM_TEST_ACTUAL_COMMIT:?}}\"\nresolve=0\nargs=()\nfor arg in \"$@\"; do\n  if [[ \"$arg\" == \"$pinned^{{commit}}\" ]]; then\n    resolve=1\n    args+=(\"$actual^{{commit}}\")\n  else\n    args+=(\"${{arg//$pinned/$actual}}\")\n  fi\ndone\nif (( resolve )); then\n  /usr/bin/git \"${{args[@]}}\" >/dev/null\n  printf '%s\\n' \"$pinned\"\nelse\n  exec /usr/bin/git \"${{args[@]}}\"\nfi\n"
        );
        fs::write(&path, script).expect("write corpus CLI Git shim");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("make corpus CLI Git shim executable");
        path
    });

    let mut state = git_environment_state().lock().unwrap();
    if state.users == 0 {
        let original_path = std::env::var_os("PATH");
        let mut paths = vec![shim.parent().unwrap().to_path_buf()];
        paths.extend(std::env::split_paths(
            original_path.as_deref().unwrap_or_default(),
        ));
        state.original_path = original_path;
        state.original_actual_commit = std::env::var_os("SOVMEM_TEST_ACTUAL_COMMIT");
        state.actual_commit = Some(actual_commit.to_owned());
        std::env::set_var(
            "PATH",
            std::env::join_paths(paths).expect("join fixture PATH"),
        );
        std::env::set_var("SOVMEM_TEST_ACTUAL_COMMIT", actual_commit);
    } else {
        assert_eq!(state.actual_commit.as_deref(), Some(actual_commit));
    }
    state.users += 1;
    GitEnvironmentGuard
}

fn exact_quota_buffers() -> Vec<String> {
    let config = load_corpus(&corpus_path()).expect("load approved corpus");
    let mut by_theme = std::collections::BTreeMap::<CorpusTheme, Vec<String>>::new();
    for entry in config.research {
        by_theme.entry(entry.theme).or_default().push(entry.path);
    }
    by_theme
        .into_iter()
        .flat_map(|(theme, mut paths)| {
            paths.sort();
            if theme == CorpusTheme::GovernanceEvidence {
                let index = paths
                    .iter()
                    .position(|path| path == RESTRICTED_BUFFER)
                    .expect("restricted source is a governance buffer candidate");
                paths.swap(0, index);
            }
            paths.truncate(paths.len() - theme.minimum_accepted());
            paths
        })
        .collect()
}

fn one_json_line(output: &Output) -> Value {
    let stdout = stdout(output);
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout:?}");
    serde_json::from_str(stdout.trim_end()).expect("stdout is one JSON summary")
}

fn export_fixture(name: &str) -> (GitFixture, TemporaryArea, PathBuf) {
    let repo = fixture(name);
    let area = TemporaryArea::new(name);
    let canary = write_canary(&area, &repo.head());
    let output_root = area.root.join("output");
    let output = run_export(&repo, &profile_path(), &canary, &output_root);
    assert!(
        output.status.success(),
        "export stderr: {}",
        stderr(&output)
    );
    (repo, area, output_root)
}

fn manifest(bundle: &Path) -> Value {
    serde_json::from_slice(&fs::read(bundle.join("manifest.json")).expect("read manifest"))
        .expect("parse manifest")
}

fn write_manifest(bundle: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).expect("serialize manifest");
    bytes.push(b'\n');
    fs::write(bundle.join("manifest.json"), bytes).expect("write manifest");
}

fn report(bundle: &Path) -> Value {
    serde_json::from_slice(
        &fs::read(bundle.join("export-report.json")).expect("read export report"),
    )
    .expect("parse export report")
}

fn write_report(bundle: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).expect("serialize export report");
    bytes.push(b'\n');
    fs::write(bundle.join("export-report.json"), bytes).expect("write export report");
}

fn corpus_report(bundle: &Path) -> Value {
    serde_json::from_slice(
        &fs::read(bundle.join("corpus-report.json")).expect("read corpus report"),
    )
    .expect("parse corpus report")
}

fn write_corpus_report(bundle: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).expect("serialize corpus report");
    bytes.push(b'\n');
    fs::write(bundle.join("corpus-report.json"), bytes).expect("write corpus report");
}

fn write_golden_cli_fixture(area: &TemporaryArea) -> (PathBuf, PathBuf) {
    let fixture = CorpusCliFixture::new();
    let bundle = area.root.join("golden-bundle");
    let output = run_corpus(&fixture.repo.root, &profile_path(), &corpus_path(), &bundle);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let report: Value =
        serde_json::from_slice(&fs::read(bundle.join("corpus-report.json")).unwrap()).unwrap();
    let mut by_category = std::collections::BTreeMap::new();
    for accepted in report["accepted"].as_array().unwrap() {
        let category = if accepted["lane"] == "core" {
            "core"
        } else {
            accepted["theme"].as_str().unwrap()
        };
        by_category
            .entry(category.to_owned())
            .or_insert_with(|| accepted["id"].as_str().unwrap().to_owned());
    }
    let categories = [
        "core",
        "tfis-ssa",
        "sovereign-context",
        "agent-operations",
        "governance-evidence",
    ];
    let evidence = categories
        .iter()
        .map(|category| by_category[*category].clone())
        .collect::<Vec<_>>();

    let golden = area.root.join("golden.jsonl");
    let mut golden_bytes = Vec::new();
    for number in 1..=25 {
        let relevant_id = &evidence[(number - 1) % evidence.len()];
        serde_json::to_writer(
            &mut golden_bytes,
            &serde_json::json!({
                "id": format!("q-{number:03}"),
                "query": format!("Synthetic CLI query {number:03}"),
                "relevant_ids": [relevant_id],
                "rationale": format!("Synthetic CLI rationale {number:03}"),
                "authorship": "human"
            }),
        )
        .unwrap();
        golden_bytes.push(b'\n');
    }
    fs::write(&golden, golden_bytes).unwrap();
    (bundle, golden)
}

fn mutate_document(
    bundle: &Path,
    manifest: &Value,
    mutate: impl FnOnce(&mut serde_yaml::Value, &mut String),
) {
    let output_path = manifest["documents"][0]["output_path"]
        .as_str()
        .expect("document output path is a string");
    let path = bundle.join(output_path);
    let rendered = fs::read_to_string(&path).expect("read generated document");
    let rendered = rendered
        .strip_prefix("---\n")
        .expect("generated document starts with frontmatter");
    let (yaml, body) = rendered
        .split_once("\n---\n")
        .expect("generated document has terminated frontmatter");
    let mut metadata: serde_yaml::Value =
        serde_yaml::from_str(yaml).expect("parse generated frontmatter");
    let mut body = body.to_owned();
    mutate(&mut metadata, &mut body);

    let yaml = serde_yaml::to_string(&metadata).expect("serialize generated frontmatter");
    let yaml = yaml.strip_prefix("---\n").unwrap_or(&yaml);
    fs::write(path, format!("---\n{yaml}---\n{body}")).expect("write generated document");
}

fn mutate_document_at(
    bundle: &Path,
    output_path: &str,
    mutate: impl FnOnce(&mut serde_yaml::Value),
) {
    let path = bundle.join(output_path);
    let rendered = fs::read_to_string(&path).expect("read generated document");
    let rendered = rendered
        .strip_prefix("---\n")
        .expect("generated document starts with frontmatter");
    let (yaml, body) = rendered
        .split_once("\n---\n")
        .expect("generated document has terminated frontmatter");
    let mut metadata: serde_yaml::Value =
        serde_yaml::from_str(yaml).expect("parse generated frontmatter");
    mutate(&mut metadata);
    let yaml = serde_yaml::to_string(&metadata).expect("serialize generated frontmatter");
    let yaml = yaml.strip_prefix("---\n").unwrap_or(&yaml);
    fs::write(path, format!("---\n{yaml}---\n{body}")).expect("write generated document");
}

fn repair_hashes(bundle: &Path, manifest: &mut Value) {
    let output_path = manifest["documents"][0]["output_path"]
        .as_str()
        .expect("document output path is a string");
    let bytes = fs::read(bundle.join(output_path)).expect("read mutated generated document");
    manifest["documents"][0]["hash"] = Value::String(blake3::hash(&bytes).to_hex().to_string());
    manifest["bundle_hash"] = Value::String(canonical_bundle_hash(bundle, manifest));
    write_manifest(bundle, manifest);
}

fn repair_all_document_hashes(bundle: &Path, manifest: &mut Value) {
    for document in manifest["documents"]
        .as_array_mut()
        .expect("manifest documents are an array")
    {
        let output_path = document["output_path"]
            .as_str()
            .expect("output path is a string");
        let bytes = fs::read(bundle.join(output_path)).expect("read generated document");
        document["hash"] = Value::String(blake3::hash(&bytes).to_hex().to_string());
    }
    write_manifest(bundle, manifest);
}

fn repair_bundle_root_claims(bundle: &Path, manifest: &mut Value) {
    let root = canonical_bundle_hash(bundle, manifest);
    manifest["bundle_hash"] = Value::String(root.clone());
    write_manifest(bundle, manifest);
    if bundle.join("corpus-report.json").is_file() {
        let mut corpus = corpus_report(bundle);
        corpus["bundle_hash"] = Value::String(root);
        write_corpus_report(bundle, &corpus);
    }
}

fn assert_verify_rejects(bundle: &Path, expected_code: &str) -> Output {
    let output = run_verify(&profile_path(), bundle);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    assert!(stdout(&output).is_empty());
    assert!(
        stderr(&output).contains(expected_code),
        "missing {expected_code:?} in stderr: {}",
        stderr(&output)
    );
    assert_no_source_content(&output);
    output
}

fn canonical_bundle_hash(bundle: &Path, manifest: &Value) -> String {
    let mut paths: Vec<_> = manifest["documents"]
        .as_array()
        .expect("manifest documents are an array")
        .iter()
        .map(|document| {
            document["output_path"]
                .as_str()
                .expect("output path is a string")
                .to_owned()
        })
        .collect();
    paths.push("bundle/index.md".into());
    paths.sort();
    paths.dedup();

    let mut hasher = blake3::Hasher::new();
    for path in paths {
        let bytes = fs::read(bundle.join(&path)).expect("read canonical bundle file");
        hasher.update(&(path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    hasher.finalize().to_hex().to_string()
}

#[test]
fn export_prints_one_deterministic_json_summary_without_source_content() {
    let repo = fixture("export-success");
    let area = TemporaryArea::new("export-success");
    let canary = write_canary(&area, &repo.head());
    let output_root = area.root.join("output");

    let output = run_export(&repo, &profile_path(), &canary, &output_root);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_no_source_content(&output);
    let summary = one_json_line(&output);
    let manifest = manifest(&output_root);
    let report: Value = serde_json::from_slice(
        &fs::read(output_root.join("export-report.json")).expect("read export report"),
    )
    .expect("parse export report");
    assert_eq!(summary["status"], "ok");
    assert_eq!(summary["command"], "export");
    assert_eq!(summary["exported"], 1);
    assert_eq!(summary["excluded"], 0);
    assert_eq!(summary["warnings"], report["counts"]["warnings"]);
    assert_eq!(summary["errors"], 0);
    assert_eq!(summary["bundle_hash"], manifest["bundle_hash"]);
}

#[test]
fn corpus_prints_one_exact_quota_json_summary_without_source_content_or_paths() {
    let fixture = CorpusCliFixture::new();
    for path in exact_quota_buffers() {
        fixture.dirty(&path);
    }
    let area = TemporaryArea::new("corpus-success");
    let output_root = area.root.join("output");

    let output = run_corpus(
        &fixture.repo.root,
        &profile_path(),
        &corpus_path(),
        &output_root,
    );

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_no_source_content(&output);
    assert_no_paths(
        &output,
        &[
            &fixture.repo.root,
            &output_root,
            &profile_path(),
            &corpus_path(),
        ],
    );
    let summary = one_json_line(&output);
    assert_eq!(summary["status"], "ok");
    assert_eq!(summary["command"], "corpus");
    assert_eq!(summary["exported"], 53);
    assert_eq!(summary["excluded"], 10);
    assert_eq!(summary["core"], 8);
    assert_eq!(summary["research"], 45);
    assert_eq!(summary["warnings"], 10);
    assert_eq!(summary["errors"], 0);
    let hash = summary["bundle_hash"].as_str().unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    assert_eq!(
        stdout(&output),
        format!(
            "{{\"status\":\"ok\",\"command\":\"corpus\",\"exported\":53,\"excluded\":10,\"core\":8,\"research\":45,\"warnings\":10,\"errors\":0,\"bundle_hash\":\"{hash}\"}}\n"
        )
    );
}

#[test]
fn corpus_missing_required_arguments_are_redacted_and_exit_two() {
    let area = TemporaryArea::new("corpus-missing-required-arguments");
    let supplied_vault = area.root.join(NOTE_SENTINEL);

    let output = Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["corpus", "--vault"])
        .arg(&supplied_vault)
        .output()
        .expect("run incomplete sovmem corpus command");

    assert_eq!(output.status.code(), Some(2), "stderr: {}", stderr(&output));
    assert!(stdout(&output).is_empty());
    let stderr = stderr(&output);
    assert!(stderr.contains("required arguments were not provided"));
    assert!(stderr.contains("--profile"));
    assert!(stderr.contains("--corpus"));
    assert!(stderr.contains("--output"));
    assert!(!stderr.contains(NOTE_SENTINEL));
    assert_no_source_content(&output);
    assert_no_paths(&output, &[&area.root, &supplied_vault]);
}

#[test]
fn corpus_configuration_and_source_failures_are_redacted_and_exit_two() {
    let area = TemporaryArea::new("corpus-configuration-source-failures");
    let malformed_corpus = area.root.join("malformed-corpus.yaml");
    fs::write(&malformed_corpus, "not: the corpus contract\n").unwrap();
    let missing_vault = area.root.join("missing-vault");
    let output_root = area.root.join("output");

    let cases = [
        (
            run_corpus(&area.root, &profile_path(), &malformed_corpus, &output_root),
            "corpus.schema",
        ),
        (
            run_corpus(
                &missing_vault,
                &profile_path(),
                &corpus_path(),
                &output_root,
            ),
            "export.io",
        ),
    ];

    for (output, code) in cases {
        assert_eq!(output.status.code(), Some(2), "stderr: {}", stderr(&output));
        assert!(stdout(&output).is_empty());
        assert!(
            stderr(&output).contains(code),
            "stderr: {}",
            stderr(&output)
        );
        assert_no_source_content(&output);
        assert_no_paths(
            &output,
            &[
                &area.root,
                &malformed_corpus,
                &missing_vault,
                &output_root,
                &profile_path(),
                &corpus_path(),
            ],
        );
    }
}

#[test]
fn corpus_gate_failure_is_redacted_and_exits_one() {
    let fixture = CorpusCliFixture::new();
    let config = load_corpus(&corpus_path()).unwrap();
    for path in config
        .research
        .iter()
        .filter(|entry| entry.theme == CorpusTheme::TfisSsa)
        .take(3)
        .map(|entry| entry.path.as_str())
    {
        fixture.dirty(path);
    }
    fixture.dirty(RESTRICTED_BUFFER);
    let area = TemporaryArea::new("corpus-gate-failure");
    let output_root = area.root.join("output");

    let output = run_corpus(
        &fixture.repo.root,
        &profile_path(),
        &corpus_path(),
        &output_root,
    );

    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("corpus.theme_minimum"));
    assert_no_source_content(&output);
    assert_no_paths(
        &output,
        &[
            &fixture.repo.root,
            &output_root,
            &profile_path(),
            &corpus_path(),
        ],
    );
}

#[test]
fn verify_golden_prints_exact_stable_json_summary() {
    let area = TemporaryArea::new("verify-golden-success");
    let (bundle, golden) = write_golden_cli_fixture(&area);

    let output = run_verify_golden(&profile_path(), &bundle, &golden);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "{\"status\":\"ok\",\"command\":\"verify-golden\",\"queries\":25,\"relevant_ids\":25,\"coverage\":[\"core\",\"tfis-ssa\",\"sovereign-context\",\"agent-operations\",\"governance-evidence\"],\"errors\":0}\n"
    );
    assert!(stderr(&output).is_empty());
    assert_no_paths(&output, &[&area.root, &bundle, &golden, &profile_path()]);
}

#[test]
fn verify_golden_data_failure_is_redacted_and_exits_one() {
    let area = TemporaryArea::new("verify-golden-data-failure");
    let (bundle, golden) = write_golden_cli_fixture(&area);
    let source = fs::read_to_string(&golden).unwrap();
    fs::write(&golden, source.replacen("\"human\"", "\"machine\"", 1)).unwrap();

    let output = run_verify_golden(&profile_path(), &bundle, &golden);

    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    assert!(stdout(&output).is_empty());
    assert_eq!(
        stderr(&output),
        "{\"severity\":\"error\",\"code\":\"golden.authorship\",\"message\":\"golden query validation failed\"}\n"
    );
    assert_no_paths(&output, &[&area.root, &bundle, &golden, &profile_path()]);

    fs::write(&golden, source).unwrap();
    let report_path = bundle.join("corpus-report.json");
    let mut report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    let accepted = report["accepted"].as_array_mut().unwrap();
    let tfis = accepted
        .iter()
        .position(|entry| entry["theme"] == "tfis-ssa")
        .unwrap();
    let operations = accepted
        .iter()
        .position(|entry| entry["theme"] == "agent-operations")
        .unwrap();
    accepted[tfis]["theme"] = Value::String("agent-operations".into());
    accepted[operations]["theme"] = Value::String("tfis-ssa".into());
    let mut bytes = serde_json::to_vec_pretty(&report).unwrap();
    bytes.push(b'\n');
    fs::write(&report_path, bytes).unwrap();

    let output = run_verify_golden(&profile_path(), &bundle, &golden);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr(&output));
    assert!(stdout(&output).is_empty());
    assert_eq!(
        stderr(&output),
        "{\"severity\":\"error\",\"code\":\"golden.bundle\",\"message\":\"golden query validation failed\"}\n"
    );
    assert_no_paths(&output, &[&area.root, &bundle, &golden, &profile_path()]);
}

#[test]
fn verify_golden_missing_configuration_is_redacted_and_exits_two() {
    let area = TemporaryArea::new("verify-golden-missing-configuration");
    let (bundle, golden) = write_golden_cli_fixture(&area);
    let missing_profile = area.root.join("missing-profile.yaml");
    let missing_bundle = area.root.join("missing-bundle");
    let missing_golden = area.root.join("missing-golden.jsonl");
    let cases = [
        (
            run_verify_golden(&missing_profile, &bundle, &golden),
            "profile.parse",
        ),
        (
            run_verify_golden(&profile_path(), &missing_bundle, &golden),
            "bundle.read",
        ),
        (
            run_verify_golden(&profile_path(), &bundle, &missing_golden),
            "golden.read",
        ),
    ];

    for (output, code) in cases {
        assert_eq!(output.status.code(), Some(2), "stderr: {}", stderr(&output));
        assert!(stdout(&output).is_empty());
        assert!(
            stderr(&output).contains(code),
            "stderr: {}",
            stderr(&output)
        );
        assert_no_paths(
            &output,
            &[
                &area.root,
                &bundle,
                &golden,
                &missing_profile,
                &missing_bundle,
                &missing_golden,
                &profile_path(),
            ],
        );
    }
}

#[test]
fn invalid_profile_is_a_configuration_failure() {
    let repo = fixture("invalid-profile");
    let area = TemporaryArea::new("invalid-profile");
    let canary = write_canary(&area, &repo.head());
    let invalid_profile = area.root.join("invalid-profile.yaml");
    fs::write(&invalid_profile, "not: a profile\n").expect("write invalid profile");

    let output = run_export(&repo, &invalid_profile, &canary, &area.root.join("output"));

    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("profile.parse"));
    assert!(stderr(&output).contains("export failed"));
    assert!(!stderr(&output).contains("canary export failed"));
    assert_no_source_content(&output);
}

#[test]
fn source_commit_mismatch_is_a_source_failure() {
    let repo = fixture("commit-mismatch");
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
        "tag stderr: {}",
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
    let area = TemporaryArea::new("commit-mismatch");
    let canary = write_canary(&area, &tag_object);
    let output_root = area.root.join("output");

    let output = run_export(&repo, &profile_path(), &canary, &output_root);

    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("source.commit_mismatch"));
    assert!(!output_root.exists());
    assert_no_source_content(&output);
}

#[test]
fn missing_exact_selection_is_a_redacted_source_failure_that_preserves_prior_output() {
    let repo = fixture("missing-exact-selection");
    let area = TemporaryArea::new("missing-exact-selection");
    let canary = write_canary_selection(
        &area,
        &repo.head(),
        &[SOURCE_PATH, "04-Research/OKF/Missing.md"],
    );
    let output_root = area.root.join("output");
    fs::create_dir(&output_root).expect("create prior output");
    let sentinel = output_root.join("sentinel.txt");
    fs::write(&sentinel, "prior validated output\n").expect("write prior output sentinel");

    let output = run_export(&repo, &profile_path(), &canary, &output_root);

    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("source.path_missing"));
    assert!(!stderr(&output).contains("Missing.md"));
    assert_no_source_content(&output);
    assert_eq!(
        fs::read_to_string(&sentinel).expect("read preserved prior output"),
        "prior validated output\n"
    );
    assert_eq!(
        fs::read_dir(&output_root)
            .expect("read preserved output")
            .count(),
        1
    );
}

#[test]
fn exact_selection_cannot_be_removed_by_an_exclusion() {
    let repo = fixture("excluded-exact-selection");
    let second_source = "04-Research/OKF/source-pages/Second.md";
    repo.write(
        second_source,
        "# Second research note\n\nPublic evidence.\n",
    );
    repo.commit_all("add second canary note", "2026-01-03T12:00:00Z");
    let area = TemporaryArea::new("excluded-exact-selection");
    let canary = write_canary_rules(
        &area,
        &repo.head(),
        &[SOURCE_PATH, second_source],
        &[SOURCE_PATH],
    );
    let output_root = area.root.join("output");

    let output = run_export(&repo, &profile_path(), &canary, &output_root);

    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty());
    assert!(
        stderr(&output).contains("canary.selection"),
        "stderr: {}",
        stderr(&output)
    );
    assert!(!output_root.exists());
    assert_no_source_content(&output);
}

#[test]
fn glob_selected_control_character_path_is_rejected_before_publication() {
    let repo = GitFixture::new("control-character-source-path");
    repo.write(
        "04-Research/OKF/source-pages/Bad\nName.md",
        "# Unsafe source path\n\nPublic evidence.\n",
    );
    repo.commit_all("add control character path", "2026-01-02T12:00:00Z");
    let area = TemporaryArea::new("control-character-source-path");
    let canary =
        write_canary_selection(&area, &repo.head(), &["04-Research/OKF/source-pages/*.md"]);
    let output_root = area.root.join("output");

    let output = run_export(&repo, &profile_path(), &canary, &output_root);

    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty());
    assert!(
        stderr(&output).contains("source.path_invalid"),
        "stderr: {}",
        stderr(&output)
    );
    assert!(!output_root.exists());
}

#[test]
fn output_inside_vault_is_a_configuration_failure() {
    let repo = fixture("output-overlap");
    let area = TemporaryArea::new("output-overlap");
    let canary = write_canary(&area, &repo.head());
    let output_root = repo.root.join("generated");

    let output = run_export(&repo, &profile_path(), &canary, &output_root);

    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("export.output_overlap"));
    assert!(!output_root.exists());
    assert_no_source_content(&output);
}

#[test]
fn valid_bundle_verification_prints_one_json_summary() {
    let (_repo, _area, output_root) = export_fixture("verify-success");

    let output = run_verify(&profile_path(), &output_root);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_no_source_content(&output);
    let summary = one_json_line(&output);
    let manifest = manifest(&output_root);
    assert_eq!(summary["status"], "ok");
    assert_eq!(summary["command"], "verify");
    assert_eq!(summary["documents"], 1);
    assert_eq!(summary["errors"], 0);
    assert_eq!(summary["bundle_hash"], manifest["bundle_hash"]);
    assert_eq!(summary.as_object().unwrap().len(), 6);
    assert!(summary.get("level").is_none());
}

#[test]
fn explicit_levels_are_reported_and_level_three_requires_corpus() {
    let (_repo, _area, output_root) = export_fixture("verify-levels");
    for level in [1, 2] {
        let output = run_verify_level(&profile_path(), &output_root, level);
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(stderr(&output).is_empty());
        assert_eq!(one_json_line(&output)["level"], level);
        assert_no_source_content(&output);
    }

    let level_three = run_verify_level(&profile_path(), &output_root, 3);
    assert_eq!(level_three.status.code(), Some(1));
    assert!(stderr(&level_three).contains("bundle.corpus_report_required"));
    assert_no_source_content(&level_three);
}

#[test]
fn legacy_and_explicit_level_contracts_do_not_conflict_silently() {
    let (_repo, _area, output_root) = export_fixture("verify-level-conflict");
    for level in [1, 2] {
        let output = run_verify_level_require_corpus(&profile_path(), &output_root, level);
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains("verify.level_conflict"));
        assert_no_source_content(&output);
    }
}

#[test]
fn explicit_level_rejects_values_outside_one_through_three() {
    let (_repo, _area, output_root) = export_fixture("verify-level-boundaries");
    for level in [0, 4] {
        let output = run_verify_level(&profile_path(), &output_root, level);
        assert_eq!(output.status.code(), Some(2));
        assert!(stdout(&output).is_empty());
        assert_no_source_content(&output);
    }
}

#[test]
fn changed_document_bytes_have_a_specific_level_two_code() {
    let (_repo, _area, bundle) = export_fixture("corrupt-document-bytes");
    let document = bundle.join(format!("bundle/{SOURCE_PATH}"));
    let bytes = [
        fs::read(&document).expect("read generated document"),
        b"\nchanged\n".to_vec(),
    ]
    .concat();
    fs::write(&document, bytes).expect("change generated document bytes");

    let output = run_verify_level(&profile_path(), &bundle, 2);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("bundle.document_hash_mismatch"));
    assert_no_source_content(&output);
}

#[test]
fn malformed_frontmatter_has_a_specific_level_one_code() {
    let (_repo, _area, bundle) = export_fixture("corrupt-frontmatter");
    let document = bundle.join(format!("bundle/{SOURCE_PATH}"));
    fs::write(document, b"---\nprovenance: synthesis\n").expect("write malformed frontmatter");

    let output = run_verify_level(&profile_path(), &bundle, 1);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("frontmatter.malformed"));
    assert_no_source_content(&output);
}

#[test]
fn invalid_provenance_has_a_specific_level_one_code() {
    let (_repo, _area, bundle) = export_fixture("corrupt-provenance");
    let manifest = manifest(&bundle);
    mutate_document(&bundle, &manifest, |metadata, _body| {
        metadata["provenance"] = serde_yaml::Value::from("forged");
    });

    let output = run_verify_level(&profile_path(), &bundle, 1);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("document.provenance_invalid"));
    assert_no_source_content(&output);
}

#[test]
fn repaired_document_hashes_do_not_repair_the_bundle_root() {
    let (_repo, _area, bundle) = export_fixture("corrupt-bundle-root");
    let document = bundle.join(format!("bundle/{SOURCE_PATH}"));
    let bytes = [
        fs::read(&document).expect("read generated document"),
        b"\nchanged\n".to_vec(),
    ]
    .concat();
    fs::write(&document, bytes).expect("change generated document bytes");
    let mut manifest = manifest(&bundle);
    repair_all_document_hashes(&bundle, &mut manifest);

    let output = run_verify_level(&profile_path(), &bundle, 2);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("bundle.root_hash_mismatch"));
    assert_no_source_content(&output);
}

#[test]
fn dangling_typed_link_has_a_specific_level_three_code() {
    let fixture = CorpusCliFixture::new();
    let area = TemporaryArea::new("corrupt-dangling-link");
    let bundle = area.root.join("bundle");
    let exported = run_corpus(&fixture.repo.root, &profile_path(), &corpus_path(), &bundle);
    assert!(exported.status.success(), "stderr: {}", stderr(&exported));
    let mut manifest = manifest(&bundle);
    let output_path = manifest["documents"][0]["output_path"]
        .as_str()
        .expect("output path is a string")
        .to_owned();
    mutate_document_at(&bundle, &output_path, |metadata| {
        metadata["links"]
            .as_sequence_mut()
            .expect("links are a sequence")
            .push(
                serde_yaml::to_value(serde_json::json!({
                    "rel": "references",
                    "target": "ssa-0/04-Research/Missing"
                }))
                .expect("serialize dangling typed link"),
            );
    });
    repair_all_document_hashes(&bundle, &mut manifest);
    repair_bundle_root_claims(&bundle, &mut manifest);

    let output = run_verify_level(&profile_path(), &bundle, 3);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("links.dangling"));
    assert_no_source_content(&output);
}

#[test]
fn stats_are_complete_deterministic_and_derived_from_verified_evidence() {
    let (_repo, _area, bundle) = export_fixture("stats-success");

    let first = run_stats(&profile_path(), &bundle);
    let second = run_stats(&profile_path(), &bundle);

    assert!(first.status.success(), "{}", stderr(&first));
    assert!(stderr(&first).is_empty());
    assert_eq!(first.stdout, second.stdout);
    let value = one_json_line(&first);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["command"], "stats");
    assert_eq!(value["documents"], 1);
    assert_eq!(
        value["warnings"].as_u64().unwrap(),
        report(&bundle)["counts"]["warnings"].as_u64().unwrap(),
    );
    assert_eq!(value["exclusions"], 0);
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "status".to_owned(),
            "command".to_owned(),
            "documents".to_owned(),
            "by_provenance".to_owned(),
            "by_type".to_owned(),
            "by_layer".to_owned(),
            "warnings".to_owned(),
            "exclusions".to_owned(),
            "bundle_hash".to_owned(),
        ]),
    );
    for key in ["by_provenance", "by_type", "by_layer"] {
        let sum: u64 = value[key]
            .as_object()
            .unwrap()
            .values()
            .map(|count| count.as_u64().unwrap())
            .sum();
        assert_eq!(sum, 1);
    }
    assert_eq!(
        value["by_provenance"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            "secondary".to_owned(),
            "synthesis".to_owned(),
            "tier1".to_owned(),
            "unknown".to_owned(),
        ],
    );
    assert_no_source_content(&first);
}

#[test]
fn stats_suppress_summary_when_bundle_verification_fails() {
    let (_repo, _area, bundle) = export_fixture("stats-failure");
    let document = bundle.join(format!("bundle/{SOURCE_PATH}"));
    let mut bytes = fs::read(&document).expect("read generated document");
    bytes.extend_from_slice(b"\nchanged\n");
    fs::write(document, bytes).expect("tamper generated document");

    let output = run_stats(&profile_path(), &bundle);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("bundle.document_hash_mismatch"));
    assert_no_source_content(&output);
}

#[test]
fn verification_and_stats_preserve_all_verified_warning_occurrences() {
    const SECOND_SOURCE: &str = "04-Research/OKF/source-pages/Second.md";
    let repo = GitFixture::new("verified-warning-count");
    for path in [SOURCE_PATH, SECOND_SOURCE] {
        repo.write(
            path,
            "# Warning evidence\n\nEach generated document retains [[Missing Target]] as a warning.\n",
        );
    }
    repo.commit_all("add repeated warning evidence", "2026-01-02T12:00:00Z");
    let area = TemporaryArea::new("verified-warning-count");
    let canary = write_canary_selection(&area, &repo.head(), &[SOURCE_PATH, SECOND_SOURCE]);
    let bundle = area.root.join("bundle");
    let exported = run_export(&repo, &profile_path(), &canary, &bundle);
    assert!(exported.status.success(), "stderr: {}", stderr(&exported));
    let expected = report(&bundle)["counts"]["warnings"]
        .as_u64()
        .expect("warning count is an integer");
    assert!(expected > 1, "fixture must expose repeated warnings");

    let verified = run_verify(&profile_path(), &bundle);
    let stats = run_stats(&profile_path(), &bundle);

    assert!(verified.status.success(), "stderr: {}", stderr(&verified));
    assert!(stats.status.success(), "stderr: {}", stderr(&stats));
    assert_eq!(one_json_line(&verified)["warnings"], expected);
    assert_eq!(one_json_line(&stats)["warnings"], expected);
}

#[test]
fn tampered_generated_markdown_is_an_invalid_bundle() {
    let (_repo, _area, output_root) = export_fixture("verify-tampered-markdown");
    let document = output_root.join(format!("bundle/{SOURCE_PATH}"));
    let mut bytes = fs::read(&document).expect("read generated document");
    bytes.extend_from_slice(format!("\n{NOTE_SENTINEL}\n").as_bytes());
    fs::write(&document, bytes).expect("tamper generated document");

    let output = run_verify(&profile_path(), &output_root);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).contains("bundle.hash_mismatch"));
    assert_no_source_content(&output);
}

#[test]
fn tampered_manifest_hash_is_an_invalid_bundle() {
    let (_repo, _area, output_root) = export_fixture("verify-tampered-manifest-hash");
    let mut manifest = manifest(&output_root);
    manifest["documents"][0]["hash"] = Value::String("0".repeat(64));
    write_manifest(&output_root, &manifest);

    let output = run_verify(&profile_path(), &output_root);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("bundle.hash_mismatch"));
    assert_no_source_content(&output);
}

#[test]
fn tampered_manifest_identity_is_an_invalid_bundle() {
    let (_repo, _area, output_root) = export_fixture("verify-tampered-manifest-identity");
    let mut manifest = manifest(&output_root);
    manifest["ssa_id"] = Value::String("ssa-9".into());
    write_manifest(&output_root, &manifest);

    let output = run_verify(&profile_path(), &output_root);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("bundle.identity_mismatch"));
    assert_no_source_content(&output);
}

#[test]
fn unmanifested_generated_markdown_is_an_invalid_bundle() {
    let (_repo, _area, output_root) = export_fixture("verify-unmanifested-markdown");
    fs::write(
        output_root.join("bundle/unexpected.md"),
        format!("# Unexpected\n\n{NOTE_SENTINEL}\n"),
    )
    .expect("write unmanifested generated document");

    let output = run_verify(&profile_path(), &output_root);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("bundle.identity_mismatch"));
    assert_no_source_content(&output);
}

#[test]
fn orphan_manifest_source_is_an_invalid_bundle() {
    let (_repo, _area, output_root) = export_fixture("verify-orphan-source");
    let mut manifest = manifest(&output_root);
    manifest["sources"]
        .as_array_mut()
        .expect("manifest sources are an array")
        .push(serde_json::json!({
            "source_path": "04-Research/OKF/Orphan.md",
            "hash": "0".repeat(64),
        }));
    write_manifest(&output_root, &manifest);

    let output = run_verify(&profile_path(), &output_root);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("bundle.identity_mismatch"));
    assert_no_source_content(&output);
}

#[test]
fn document_identity_tampering_fails_even_when_hashes_are_repaired() {
    let (_repo, _area, output_root) = export_fixture("verify-repaired-hashes");
    let document_path = output_root.join(format!("bundle/{SOURCE_PATH}"));
    let document = fs::read_to_string(&document_path).expect("read generated document");
    let document = document.replacen("ssa_id: ssa-0", "ssa_id: ssa-9", 1);
    assert!(document.contains("ssa_id: ssa-9"));
    fs::write(&document_path, document.as_bytes()).expect("tamper generated identity");

    let mut manifest = manifest(&output_root);
    manifest["documents"][0]["hash"] =
        Value::String(blake3::hash(document.as_bytes()).to_hex().to_string());
    manifest["bundle_hash"] = Value::String(canonical_bundle_hash(&output_root, &manifest));
    write_manifest(&output_root, &manifest);

    let output = run_verify(&profile_path(), &output_root);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("identity.ssa_id"));
    assert_no_source_content(&output);
}

#[test]
fn same_profile_identity_rebind_fails_even_when_document_and_hashes_are_repaired() {
    let (_repo, _area, output_root) = export_fixture("verify-source-path-rebind");
    let mut manifest = manifest(&output_root);
    let rebound_id = "ssa-0/04-Research/OKF/Rebound";
    let rebound_resource = format!("vault://{rebound_id}");
    mutate_document(&output_root, &manifest, |metadata, _| {
        metadata["id"] = serde_yaml::Value::String(rebound_id.into());
        metadata["resource"] = serde_yaml::Value::String(rebound_resource.clone());
        metadata["source"] = serde_yaml::Value::String(rebound_resource.clone());
    });
    manifest["documents"][0]["id"] = Value::String(rebound_id.into());
    manifest["documents"][0]["resource"] = Value::String(rebound_resource);
    repair_hashes(&output_root, &mut manifest);

    assert_verify_rejects(&output_root, "identity.source_path_binding");
}

#[test]
fn invalid_calendar_date_fails_even_when_hashes_are_repaired() {
    let (_repo, _area, output_root) = export_fixture("verify-invalid-calendar-date");
    let mut manifest = manifest(&output_root);
    mutate_document(&output_root, &manifest, |metadata, _| {
        metadata["created"] = serde_yaml::Value::String("2026-02-30".into());
    });
    repair_hashes(&output_root, &mut manifest);

    assert_verify_rejects(&output_root, "document.date_invalid");
}

#[test]
fn foreign_profile_body_link_fails_even_when_hashes_are_repaired() {
    let (_repo, _area, output_root) = export_fixture("verify-foreign-body-link");
    let mut manifest = manifest(&output_root);
    mutate_document(&output_root, &manifest, |_, body| {
        body.push_str("\n[foreign](vault://ssa-9/04-Research/Foreign)\n");
    });
    repair_hashes(&output_root, &mut manifest);

    assert_verify_rejects(&output_root, "links.cross_profile");
}

#[test]
fn added_same_profile_body_link_must_match_generated_link_state() {
    let (_repo, _area, output_root) = export_fixture("verify-body-link-state");
    let mut manifest = manifest(&output_root);
    mutate_document(&output_root, &manifest, |_, body| {
        body.push_str("\n[self](vault://ssa-0/04-Research/OKF/README)\n");
    });
    repair_hashes(&output_root, &mut manifest);

    assert_verify_rejects(&output_root, "document.link_state_mismatch");
}

#[test]
fn added_broken_same_profile_body_link_must_match_reported_link_state() {
    let (_repo, _area, output_root) = export_fixture("verify-broken-body-link-state");
    let mut manifest = manifest(&output_root);
    mutate_document(&output_root, &manifest, |_, body| {
        body.push_str("\n[broken](Missing.md)\n");
    });
    repair_hashes(&output_root, &mut manifest);

    assert_verify_rejects(&output_root, "bundle.report_invalid");
}

#[test]
fn secret_like_body_line_fails_redacted_even_when_hashes_are_repaired() {
    let (_repo, _area, output_root) = export_fixture("verify-secret-body-line");
    let mut manifest = manifest(&output_root);
    mutate_document(&output_root, &manifest, |_, body| {
        body.push_str(&format!("\ntoken: {SECRET_SENTINEL}\n"));
    });
    repair_hashes(&output_root, &mut manifest);

    let output = assert_verify_rejects(&output_root, "document.restricted_content");
    assert!(!stderr(&output).contains(SECRET_SENTINEL));
}

#[test]
fn secret_like_frontmatter_line_fails_redacted_even_when_hashes_are_repaired() {
    let (_repo, _area, output_root) = export_fixture("verify-secret-frontmatter-line");
    let mut manifest = manifest(&output_root);
    mutate_document(&output_root, &manifest, |metadata, _| {
        metadata["token"] = serde_yaml::Value::String(SECRET_SENTINEL.into());
    });
    repair_hashes(&output_root, &mut manifest);

    let output = assert_verify_rejects(&output_root, "document.restricted_content");
    assert!(!stderr(&output).contains(SECRET_SENTINEL));
}

#[test]
fn invalid_manifest_provenance_is_rejected() {
    type ManifestMutation = fn(&mut Value, &Path);
    let cases: &[(&str, ManifestMutation)] = &[
        ("unsupported-version", |manifest, _| {
            manifest["exporter_version"] = Value::String("999.0.0".into());
        }),
        ("missing-version", |manifest, _| {
            manifest
                .as_object_mut()
                .expect("manifest is an object")
                .remove("exporter_version");
        }),
        ("unknown-field", |manifest, _| {
            manifest
                .as_object_mut()
                .expect("manifest is an object")
                .insert("unexpected".into(), Value::Bool(true));
        }),
        ("invalid-commit", |manifest, _| {
            manifest["commit"] = Value::String("not-a-commit".into());
        }),
        ("unsafe-selection", |manifest, _| {
            manifest["inclusion"] = serde_json::json!(["../private.md"]);
        }),
        ("duplicate-selection", |manifest, _| {
            let first = manifest["inclusion"][0].clone();
            manifest["inclusion"] = Value::Array(vec![first.clone(), first]);
        }),
        ("invalid-excluded-source", |manifest, bundle| {
            manifest["excluded_sources"] = serde_json::json!([{
                "source_path": "../private.md",
                "reason": "dirty"
            }]);
            let mut report = report(bundle);
            report["counts"]["selected"] = Value::from(2);
            report["counts"]["excluded"] = Value::from(1);
            write_report(bundle, &report);
        }),
    ];

    for (name, mutate) in cases {
        let (_repo, _area, output_root) = export_fixture(name);
        let mut manifest = manifest(&output_root);
        mutate(&mut manifest, &output_root);
        write_manifest(&output_root, &manifest);
        assert_verify_rejects(&output_root, "bundle.manifest_invalid");
    }
}

#[test]
fn missing_malformed_and_inconsistent_export_reports_are_rejected() {
    for case in ["missing", "malformed", "unknown-field", "inconsistent"] {
        let (_repo, _area, output_root) = export_fixture(&format!("verify-report-{case}"));
        match case {
            "missing" => fs::remove_file(output_root.join("export-report.json"))
                .expect("remove export report"),
            "malformed" => fs::write(output_root.join("export-report.json"), b"not json\n")
                .expect("malform export report"),
            "unknown-field" => {
                let mut report = report(&output_root);
                report
                    .as_object_mut()
                    .expect("export report is an object")
                    .insert("unexpected".into(), Value::Bool(true));
                write_report(&output_root, &report);
            }
            "inconsistent" => {
                let mut report = report(&output_root);
                report["counts"]["exported"] = Value::from(99);
                write_report(&output_root, &report);
            }
            _ => unreachable!(),
        }
        assert_verify_rejects(&output_root, "bundle.report_invalid");
    }
}

#[test]
fn unexpected_root_artifact_is_rejected() {
    let (_repo, _area, output_root) = export_fixture("verify-unexpected-root");
    fs::write(
        output_root.join("unexpected.txt"),
        "not part of the bundle\n",
    )
    .expect("write unexpected root artifact");

    assert_verify_rejects(&output_root, "bundle.root_invalid");
}

#[test]
fn malformed_optional_corpus_report_is_rejected_strictly() {
    let (_repo, _area, output_root) = export_fixture("verify-malformed-corpus-report");
    fs::write(output_root.join("corpus-report.json"), b"not json\n")
        .expect("write malformed corpus report");

    assert_verify_rejects(&output_root, "bundle.corpus_report_invalid");
}

#[test]
fn corpus_required_verify_rejects_missing_evidence_but_generic_verify_remains_compatible() {
    let fixture = CorpusCliFixture::new();
    let area = TemporaryArea::new("verify-required-corpus");
    let bundle = area.root.join("bundle");
    let exported = run_corpus(&fixture.repo.root, &profile_path(), &corpus_path(), &bundle);
    assert!(exported.status.success(), "stderr: {}", stderr(&exported));
    fs::remove_file(bundle.join("corpus-report.json")).expect("remove corpus evidence");

    let generic = run_verify(&profile_path(), &bundle);
    assert!(generic.status.success(), "stderr: {}", stderr(&generic));

    let required = run_verify_require_corpus(&profile_path(), &bundle);
    assert_eq!(required.status.code(), Some(1));
    assert!(stdout(&required).is_empty());
    assert!(stderr(&required).contains("bundle.corpus_report_required"));
    assert_no_source_content(&required);
    assert_no_paths(&required, &[&bundle]);
}

#[test]
fn corpus_required_verify_rejects_a_consistent_foreign_profile_bundle() {
    let fixture = CorpusCliFixture::new();
    let area = TemporaryArea::new("verify-foreign-corpus-profile");
    let bundle = area.root.join("bundle");
    let exported = run_corpus(&fixture.repo.root, &profile_path(), &corpus_path(), &bundle);
    assert!(exported.status.success(), "stderr: {}", stderr(&exported));

    let mut manifest = manifest(&bundle);
    let documents = manifest["documents"].as_array().unwrap().clone();
    for document in &documents {
        let output_path = document["output_path"].as_str().unwrap();
        mutate_document_at(&bundle, output_path, |metadata| {
            metadata["ssa_profile"] = serde_yaml::Value::from("foreign-ssa9-v0");
        });
    }
    for (entry, document) in manifest["documents"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(documents.iter())
    {
        let output_path = document["output_path"].as_str().unwrap();
        let bytes = fs::read(bundle.join(output_path)).unwrap();
        entry["hash"] = Value::String(blake3::hash(&bytes).to_hex().to_string());
    }
    manifest["ssa_profile"] = Value::String("foreign-ssa9-v0".into());
    manifest["bundle_hash"] = Value::String(canonical_bundle_hash(&bundle, &manifest));
    write_manifest(&bundle, &manifest);
    let mut corpus_report: Value = serde_json::from_slice(
        &fs::read(bundle.join("corpus-report.json")).expect("read corpus report"),
    )
    .unwrap();
    corpus_report["bundle_hash"] = manifest["bundle_hash"].clone();
    fs::write(
        bundle.join("corpus-report.json"),
        serde_json::to_vec_pretty(&corpus_report).unwrap(),
    )
    .unwrap();

    let profile_source = fs::read_to_string(profile_path()).unwrap().replacen(
        "ssa_profile: audy-ssa0-v0",
        "ssa_profile: foreign-ssa9-v0",
        1,
    );
    let foreign_profile = area.root.join("foreign-profile.yaml");
    fs::write(&foreign_profile, profile_source).unwrap();

    for output in [
        run_verify(&foreign_profile, &bundle),
        run_verify_require_corpus(&foreign_profile, &bundle),
    ] {
        assert_eq!(output.status.code(), Some(1), "stdout: {}", stdout(&output));
        assert!(stdout(&output).is_empty());
        assert!(stderr(&output).contains("bundle.corpus_profile_invalid"));
        assert_no_paths(&output, &[&bundle, &foreign_profile]);
    }
}

#[test]
fn corpus_verifiers_reject_a_fully_rebound_foreign_vault_profile() {
    let fixture = CorpusCliFixture::new();
    let area = TemporaryArea::new("verify-foreign-corpus-profile");
    let bundle = area.root.join("bundle");
    let exported = run_corpus(&fixture.repo.root, &profile_path(), &corpus_path(), &bundle);
    assert!(exported.status.success(), "stderr: {}", stderr(&exported));

    let mut manifest = manifest(&bundle);
    manifest["vault_id"] = Value::String("foreign-vault".into());
    write_manifest(&bundle, &manifest);

    let profile_source = fs::read_to_string(profile_path()).unwrap().replacen(
        "vault_id: audy-vault",
        "vault_id: foreign-vault",
        1,
    );
    let foreign_profile = area.root.join("foreign-vault-profile.yaml");
    fs::write(&foreign_profile, profile_source).unwrap();

    for output in [
        run_verify(&foreign_profile, &bundle),
        run_verify_require_corpus(&foreign_profile, &bundle),
    ] {
        assert_eq!(output.status.code(), Some(1), "stdout: {}", stdout(&output));
        assert!(stdout(&output).is_empty());
        assert!(stderr(&output).contains("bundle.corpus_profile_invalid"));
        assert_no_paths(&output, &[&bundle, &foreign_profile]);
    }
}

#[test]
fn corpus_report_that_does_not_correspond_to_canary_manifest_is_rejected() {
    let (_repo, _area, output_root) = export_fixture("verify-foreign-corpus-report");
    let manifest = manifest(&output_root);
    let document = &manifest["documents"][0];
    let source = &manifest["sources"][0];
    let export_report = report(&output_root);
    let warning_count = export_report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["severity"] == "warning")
        .count();
    let diagnostic_codes: BTreeSet<_> = export_report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["code"].as_str().unwrap().to_owned())
        .collect();
    let report = serde_json::json!({
        "schema": "audy-retrieval-corpus-v1",
        "profile": manifest["profile"],
        "source_commit": manifest["commit"],
        "counts": {
            "candidates": 1,
            "core": 0,
            "research": 1,
            "rejected": 0,
            "warnings": warning_count,
            "errors": 0
        },
        "required_core": [],
        "research_by_theme": { "tfis-ssa": 1 },
        "accepted": [{
            "source_path": document["source_path"],
            "id": document["id"],
            "lane": "research",
            "theme": "tfis-ssa",
            "source_hash": source["hash"]
        }],
        "rejected": [],
        "diagnostic_codes": diagnostic_codes,
        "bundle_hash": manifest["bundle_hash"],
        "gates_passed": true
    });
    fs::write(
        output_root.join("corpus-report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .expect("write foreign corpus report");

    assert_verify_rejects(&output_root, "bundle.corpus_report_invalid");
}

#[test]
fn unexpected_empty_bundle_directory_is_rejected() {
    let (_repo, _area, output_root) = export_fixture("verify-empty-bundle-directory");
    fs::create_dir(output_root.join("bundle/empty-extra"))
        .expect("create unmanifested empty bundle directory");

    assert_verify_rejects(&output_root, "bundle.identity_mismatch");
}

#[test]
fn zero_representation_glob_bundle_is_rejected_as_non_exportable() {
    let (_repo, _area, output_root) = export_fixture("verify-zero-representation");
    let mut manifest = manifest(&output_root);
    let output_path = manifest["documents"][0]["output_path"]
        .as_str()
        .expect("document output path is a string")
        .to_owned();
    fs::remove_file(output_root.join(output_path)).expect("remove represented document");
    fs::write(output_root.join("bundle/index.md"), "# SovMem Bundle\n\n")
        .expect("write empty canonical index");

    manifest["inclusion"] = serde_json::json!(["04-Research/**/*.md"]);
    manifest["sources"] = serde_json::json!([]);
    manifest["documents"] = serde_json::json!([]);
    manifest["excluded_sources"] = serde_json::json!([]);
    manifest["bundle_hash"] = Value::String(canonical_bundle_hash(&output_root, &manifest));
    write_manifest(&output_root, &manifest);

    let mut report = report(&output_root);
    report["counts"] = serde_json::json!({
        "selected": 0,
        "exported": 0,
        "excluded": 0,
        "warnings": 0,
        "errors": 0
    });
    report["diagnostics"] = serde_json::json!([]);
    write_report(&output_root, &report);

    assert_verify_rejects(&output_root, "bundle.manifest_invalid");
}

#[test]
fn pinned_audy_canary_has_thirteen_unique_nonexcluded_deterministically_mapped_paths() {
    let profile = load_profile(&profile_path()).expect("load Audy profile");
    let canary = load_canary(&canary_path()).expect("load pinned Audy canary");
    let unique: BTreeSet<_> = canary.include.iter().collect();

    assert_eq!(canary.include.len(), 13);
    assert_eq!(unique.len(), 13);
    for path in &canary.include {
        assert!(
            canary
                .exclude
                .iter()
                .all(|pattern| !glob_matches(pattern, path)),
            "included path {path} overlaps an exclusion"
        );
        let first = profile
            .mapping_for(path)
            .unwrap_or_else(|| panic!("no profile mapping for {path}"));
        let second = profile
            .mapping_for(path)
            .unwrap_or_else(|| panic!("mapping disappeared for {path}"));
        assert_eq!(first.pattern, second.pattern, "unstable mapping for {path}");
        assert_eq!(
            first.context_kind, second.context_kind,
            "unstable mapping for {path}"
        );
    }
}
