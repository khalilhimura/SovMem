use serde_json::Value;
use sovmem::profile::{
    export_corpus, load_corpus, CorpusExportRequest, CorpusTheme, PINNED_SOURCE_COMMIT,
};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_AREA_ID: AtomicU64 = AtomicU64::new(0);
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
            "sovmem-corpus-export-{name}-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create temporary corpus export area");
        Self { root }
    }
}

impl Drop for TemporaryArea {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct CorpusFixture {
    area: TemporaryArea,
    repo: PathBuf,
    _git_environment: GitEnvironmentGuard,
}

impl CorpusFixture {
    fn new(name: &str) -> Self {
        let area = TemporaryArea::new(name);
        let repo = area.root.join("vault");
        fs::create_dir(&repo).expect("create corpus fixture repository");
        git_success(
            real_git(&repo)
                .args(["init", "--quiet"])
                .output()
                .expect("initialize corpus fixture"),
            "initialize corpus fixture",
        );
        git_success(
            real_git(&repo)
                .args(["config", "user.name", "SovMem Test"])
                .output()
                .expect("configure corpus fixture user"),
            "configure corpus fixture user",
        );
        git_success(
            real_git(&repo)
                .args(["config", "user.email", "sovmem@example.invalid"])
                .output()
                .expect("configure corpus fixture email"),
            "configure corpus fixture email",
        );
        for path in load_corpus(&corpus_path())
            .expect("load approved corpus for fixture")
            .selected_paths()
        {
            let destination = repo.join(path);
            fs::create_dir_all(destination.parent().unwrap())
                .expect("create corpus fixture source parent");
            fs::write(
                destination,
                b"# Fixture\n\nThis fixture records deterministic evidence with https://example.com/source.\n",
            )
            .expect("write corpus fixture source");
        }
        git_success(
            real_git(&repo)
                .args(["add", "--all"])
                .output()
                .expect("stage corpus fixture"),
            "stage corpus fixture",
        );
        let mut commit = real_git(&repo);
        commit
            .args(["commit", "--quiet", "--message", "add corpus fixture"])
            .env("GIT_AUTHOR_DATE", "2026-01-02T12:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-01-02T12:00:00Z");
        git_success(
            commit.output().expect("commit corpus fixture"),
            "commit corpus fixture",
        );
        let actual_commit = String::from_utf8(
            git_success(
                real_git(&repo)
                    .args(["rev-parse", "HEAD"])
                    .output()
                    .expect("resolve corpus fixture commit"),
                "resolve corpus fixture commit",
            )
            .stdout,
        )
        .expect("corpus fixture commit is UTF-8")
        .trim()
        .to_owned();
        let git_environment = install_git_shim(&actual_commit);
        Self {
            area,
            repo,
            _git_environment: git_environment,
        }
    }

    fn dirty(&self, path: &str) {
        let source = self.repo.join(path);
        let mut bytes = fs::read(&source).expect("read fixture source before dirtying");
        bytes.extend_from_slice(b"\n<!-- corpus export fixture dirtiness -->\n");
        fs::write(source, bytes).expect("dirty fixture source");
    }

    fn output(&self, name: &str) -> PathBuf {
        self.area.root.join(name)
    }
}

fn real_git(root: &Path) -> Command {
    let mut command = Command::new("/usr/bin/git");
    command.arg("-C").arg(root);
    command
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
        let directory = std::env::temp_dir().join(format!(
            "sovmem-corpus-export-git-shim-{}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create fixture Git shim directory");
        let path = directory.join("git");
        let script = format!(
            "#!/bin/bash\nset -e\npinned='{PINNED_SOURCE_COMMIT}'\nactual=\"${{SOVMEM_TEST_ACTUAL_COMMIT:?}}\"\nresolve=0\nargs=()\nfor arg in \"$@\"; do\n  if [[ \"$arg\" == \"$pinned^{{commit}}\" ]]; then\n    resolve=1\n    args+=(\"$actual^{{commit}}\")\n  else\n    args+=(\"${{arg//$pinned/$actual}}\")\n  fi\ndone\nif (( resolve )); then\n  /usr/bin/git \"${{args[@]}}\" >/dev/null\n  printf '%s\\n' \"$pinned\"\nelse\n  exec /usr/bin/git \"${{args[@]}}\"\nfi\n"
        );
        fs::write(&path, script).expect("write fixture Git shim");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("make fixture Git shim executable");
        path
    });
    assert!(shim.is_file());
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

fn profile_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/profile.yaml")
}

fn corpus_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml")
}

fn request(repo: &Path, output_root: PathBuf) -> CorpusExportRequest {
    request_with_profile(repo, output_root, profile_path())
}

fn request_with_profile(
    repo: &Path,
    output_root: PathBuf,
    profile_path: PathBuf,
) -> CorpusExportRequest {
    CorpusExportRequest {
        vault_root: repo.to_path_buf(),
        output_root,
        profile_path,
        corpus_path: corpus_path(),
    }
}

fn foreign_vault_profile(area: &TemporaryArea) -> PathBuf {
    let source = fs::read_to_string(profile_path()).expect("read canonical profile");
    let source = source.replacen("vault_id: audy-vault", "vault_id: foreign-vault", 1);
    let path = area.root.join("foreign-vault-profile.yaml");
    fs::write(&path, source).expect("write foreign-vault profile");
    path
}

fn foreign_profile(area: &TemporaryArea) -> PathBuf {
    let source = fs::read_to_string(profile_path()).expect("read canonical profile");
    let source = source
        .replacen("ssa_id: ssa-0", "ssa_id: ssa-9", 1)
        .replacen(
            "ssa_profile: audy-ssa0-v0",
            "ssa_profile: foreign-ssa9-v0",
            1,
        )
        .replacen("familiar_id: audy", "familiar_id: foreign", 1)
        .replacen("context_namespace: ssa-0", "context_namespace: ssa-9", 1);
    let path = area.root.join("foreign-profile.yaml");
    fs::write(&path, source).expect("write mutually consistent foreign profile");
    path
}

fn exact_quota_buffers() -> Vec<String> {
    let config = load_corpus(&corpus_path()).expect("load approved corpus");
    let mut by_theme = BTreeMap::<CorpusTheme, Vec<String>>::new();
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

fn worktree_hashes(root: &Path) -> BTreeMap<String, String> {
    load_corpus(&corpus_path())
        .expect("load approved corpus for source hashing")
        .selected_paths()
        .into_iter()
        .map(|path| {
            let bytes = fs::read(root.join(&path)).expect("read tracked fixture source");
            (path, blake3::hash(&bytes).to_hex().to_string())
        })
        .collect()
}

fn git_success(output: Output, operation: &str) -> Output {
    assert!(
        output.status.success(),
        "{operation} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read JSON artifact"))
        .expect("parse JSON artifact")
}

fn write_json(path: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).expect("serialize JSON artifact");
    bytes.push(b'\n');
    fs::write(path, bytes).expect("write JSON artifact");
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create copied export directory");
    for entry in fs::read_dir(source).expect("read copied export source") {
        let entry = entry.expect("read copied export entry");
        let target = destination.join(entry.file_name());
        if entry
            .file_type()
            .expect("inspect copied export entry")
            .is_dir()
        {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy exported artifact");
        }
    }
}

fn verify(output: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
        .args(["verify", "--profile"])
        .arg(profile_path())
        .arg("--bundle")
        .arg(output)
        .output()
        .expect("run offline bundle verifier")
}

#[test]
fn exports_identical_exact_quota_corpora_without_mutating_sources() {
    let fixture = CorpusFixture::new("deterministic");
    for path in exact_quota_buffers() {
        fixture.dirty(&path);
    }
    let first_output = fixture.output("first");
    let second_output = fixture.output("second");
    let source_before = worktree_hashes(&fixture.repo);

    let first = export_corpus(request(&fixture.repo, first_output.clone())).unwrap();
    let second = export_corpus(request(&fixture.repo, second_output.clone())).unwrap();

    assert_eq!(first, second);
    assert_eq!(tree_bytes(&first_output), tree_bytes(&second_output));
    assert_eq!(first.core, 8);
    assert_eq!(first.research, 45);
    assert_eq!(first.exported, 53);
    assert_eq!(first.excluded, 10);
    assert!(first_output.join("corpus-report.json").is_file());
    assert_eq!(source_before, worktree_hashes(&fixture.repo));

    let manifest = json(&first_output.join("manifest.json"));
    let report = json(&first_output.join("corpus-report.json"));
    assert_eq!(report["bundle_hash"], manifest["bundle_hash"]);
    assert_eq!(report["accepted"].as_array().unwrap().len(), 53);
    assert!(fs::read_to_string(first_output.join("corpus-report.json"))
        .unwrap()
        .ends_with('\n'));
    let verified = verify(&first_output);
    assert!(
        verified.status.success(),
        "corpus bundle verification failed: {}",
        String::from_utf8_lossy(&verified.stderr)
    );

    let report_path = second_output.join("corpus-report.json");
    let original_report = fs::read(&report_path).unwrap();
    let mut tampered_report: Value = serde_json::from_slice(&original_report).unwrap();
    tampered_report["bundle_hash"] = Value::String("0".repeat(64));
    fs::write(
        &report_path,
        serde_json::to_vec_pretty(&tampered_report).unwrap(),
    )
    .unwrap();
    let rejected = verify(&second_output);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("bundle.corpus_report_invalid"));

    fs::write(&report_path, &original_report).unwrap();
    let mut tampered_report: Value = serde_json::from_slice(&original_report).unwrap();
    tampered_report["accepted"][0]["id"] = Value::String("foreign/id".into());
    fs::write(
        &report_path,
        serde_json::to_vec_pretty(&tampered_report).unwrap(),
    )
    .unwrap();
    let rejected = verify(&second_output);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("bundle.corpus_report_invalid"));
}

#[test]
fn corpus_gate_failure_preserves_prior_output() {
    let fixture = CorpusFixture::new("gate-failure");
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
    let output = fixture.output("output");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("sentinel.txt"), b"prior corpus\n").unwrap();
    let before = tree_bytes(&output);

    let error = export_corpus(request(&fixture.repo, output.clone())).unwrap_err();

    assert_eq!(error.code(), "corpus.theme_minimum");
    assert_eq!(tree_bytes(&output), before);
}

#[test]
fn dirty_core_fails_closed_without_publication() {
    let fixture = CorpusFixture::new("dirty-core");
    fixture.dirty("01-Soul/Audy Himura.md");
    let output = fixture.output("output");

    let error = export_corpus(request(&fixture.repo, output.clone())).unwrap_err();

    assert_eq!(error.code(), "corpus.core_missing");
    assert!(!output.exists());
}

#[test]
fn dirty_research_buffer_is_reported_and_may_pass() {
    let fixture = CorpusFixture::new("dirty-buffer");
    let buffer = RESTRICTED_BUFFER.to_owned();
    fixture.dirty(&buffer);
    let output = fixture.output("output");

    let summary = export_corpus(request(&fixture.repo, output.clone())).unwrap();

    assert_eq!(summary.exported, 62);
    assert_eq!(summary.excluded, 1);
    let report = json(&output.join("corpus-report.json"));
    assert!(report["rejected"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["source_path"] == buffer && entry["reason"] == "dirty"));
}

#[test]
fn corpus_export_rejects_a_profile_that_changes_only_vault_id() {
    let fixture = CorpusFixture::new("foreign-vault-profile");
    let profile = foreign_vault_profile(&fixture.area);
    let output = fixture.output("output");

    let error =
        export_corpus(request_with_profile(&fixture.repo, output.clone(), profile)).unwrap_err();

    assert_eq!(error.code(), "corpus.profile_contract");
    assert!(!output.exists());
}

#[test]
fn corpus_export_rejects_a_mutually_consistent_foreign_profile() {
    let fixture = CorpusFixture::new("foreign-profile");
    let profile = foreign_profile(&fixture.area);
    let output = fixture.output("output");

    let error =
        export_corpus(request_with_profile(&fixture.repo, output.clone(), profile)).unwrap_err();

    assert_eq!(error.code(), "corpus.profile_contract");
    assert!(!output.exists());
}

#[test]
fn output_inside_vault_fails_before_writes() {
    let fixture = CorpusFixture::new("overlap");
    let output = fixture.repo.join("generated/corpus");
    let source_before = worktree_hashes(&fixture.repo);

    let error = export_corpus(request(&fixture.repo, output.clone())).unwrap_err();

    assert_eq!(error.code(), "export.output_overlap");
    assert!(!output.exists());
    assert_eq!(source_before, worktree_hashes(&fixture.repo));
}

#[test]
fn successful_publication_atomically_replaces_prior_output_with_all_reports() {
    let fixture = CorpusFixture::new("publication");
    fixture.dirty(RESTRICTED_BUFFER);
    let output = fixture.output("output");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("sentinel.txt"), b"prior corpus\n").unwrap();

    export_corpus(request(&fixture.repo, output.clone())).unwrap();

    assert!(!output.join("sentinel.txt").exists());
    assert!(output.join("manifest.json").is_file());
    assert!(output.join("export-report.json").is_file());
    assert!(output.join("corpus-report.json").is_file());
}

#[test]
fn verifier_reconstructs_every_fixed_v1_corpus_report_invariant() {
    let fixture = CorpusFixture::new("verifier-invariants");
    for path in exact_quota_buffers() {
        fixture.dirty(&path);
    }
    let baseline = fixture.output("baseline");
    export_corpus(request(&fixture.repo, baseline.clone())).unwrap();

    let cases = [
        "schema",
        "source-commit",
        "required-core",
        "research-by-theme",
        "accepted-lane",
        "accepted-theme",
        "foreign-full-size-selection",
        "balanced-theme-swap",
        "rejected-lane",
        "rejected-theme",
        "count-candidates",
        "count-core",
        "count-research",
        "count-rejected",
        "count-warnings",
        "count-errors",
        "diagnostic-codes",
        "gates-passed",
        "manifest-inclusion",
        "rejected-union",
    ];

    for case in cases {
        let output = fixture.output(case);
        copy_tree(&baseline, &output);
        let report_path = output.join("corpus-report.json");
        let mut corpus = json(&report_path);
        match case {
            "schema" => corpus["schema"] = Value::String("foreign-corpus-v1".into()),
            "source-commit" => corpus["source_commit"] = Value::String("0".repeat(40)),
            "required-core" => {
                corpus["required_core"].as_array_mut().unwrap().remove(0);
            }
            "research-by-theme" => corpus["research_by_theme"]["tfis-ssa"] = Value::from(11),
            "accepted-lane" => corpus["accepted"][0]["lane"] = Value::String("research".into()),
            "accepted-theme" => {
                let accepted = corpus["accepted"].as_array_mut().unwrap();
                let research = accepted
                    .iter_mut()
                    .find(|entry| entry["lane"] == "research")
                    .unwrap();
                research["theme"] = Value::String("governance-evidence".into());
            }
            "foreign-full-size-selection" => {
                let original = corpus["rejected"][0]["source_path"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                let foreign = "04-Research/Foreign-Candidate.md";
                corpus["rejected"][0]["source_path"] = Value::String(foreign.into());
                corpus["rejected"]
                    .as_array_mut()
                    .unwrap()
                    .sort_by(|left, right| {
                        left["source_path"]
                            .as_str()
                            .cmp(&right["source_path"].as_str())
                    });
                let mut manifest = json(&output.join("manifest.json"));
                let excluded = manifest["excluded_sources"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|entry| entry["source_path"] == original)
                    .unwrap();
                excluded["source_path"] = Value::String(foreign.into());
                manifest["excluded_sources"]
                    .as_array_mut()
                    .unwrap()
                    .sort_by(|left, right| {
                        left["source_path"]
                            .as_str()
                            .cmp(&right["source_path"].as_str())
                    });
                let inclusion = manifest["inclusion"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|path| path.as_str() == Some(&original))
                    .unwrap();
                *inclusion = Value::String(foreign.into());
                manifest["inclusion"]
                    .as_array_mut()
                    .unwrap()
                    .sort_by(|left, right| left.as_str().cmp(&right.as_str()));
                write_json(&output.join("manifest.json"), &manifest);
                let export_report_path = output.join("export-report.json");
                let mut export_report = json(&export_report_path);
                for diagnostic in export_report["diagnostics"].as_array_mut().unwrap() {
                    if diagnostic["source_path"] == original {
                        diagnostic["source_path"] = Value::String(foreign.into());
                    }
                }
                export_report["diagnostics"]
                    .as_array_mut()
                    .unwrap()
                    .sort_by(|left, right| {
                        (
                            left["severity"].as_str(),
                            left["code"].as_str(),
                            left["source_path"].as_str(),
                            left["message"].as_str(),
                        )
                            .cmp(&(
                                right["severity"].as_str(),
                                right["code"].as_str(),
                                right["source_path"].as_str(),
                                right["message"].as_str(),
                            ))
                    });
                write_json(&export_report_path, &export_report);
            }
            "balanced-theme-swap" => {
                let accepted = corpus["accepted"].as_array_mut().unwrap();
                let first = accepted
                    .iter()
                    .position(|entry| entry["theme"] == "tfis-ssa")
                    .unwrap();
                let second = accepted
                    .iter()
                    .position(|entry| entry["theme"] == "agent-operations")
                    .unwrap();
                accepted[first]["theme"] = Value::String("agent-operations".into());
                accepted[second]["theme"] = Value::String("tfis-ssa".into());
            }
            "rejected-lane" => corpus["rejected"][0]["lane"] = Value::String("core".into()),
            "rejected-theme" => {
                corpus["rejected"][0]["theme"] = Value::String("sovereign-context".into())
            }
            "count-candidates" => corpus["counts"]["candidates"] = Value::from(62),
            "count-core" => corpus["counts"]["core"] = Value::from(7),
            "count-research" => corpus["counts"]["research"] = Value::from(46),
            "count-rejected" => corpus["counts"]["rejected"] = Value::from(9),
            "count-warnings" => corpus["counts"]["warnings"] = Value::from(0),
            "count-errors" => corpus["counts"]["errors"] = Value::from(1),
            "diagnostic-codes" => corpus["diagnostic_codes"] = serde_json::json!([]),
            "gates-passed" => corpus["gates_passed"] = Value::Bool(false),
            "manifest-inclusion" => {
                let mut manifest = json(&output.join("manifest.json"));
                manifest["inclusion"]
                    .as_array_mut()
                    .unwrap()
                    .push(Value::String("ZZZ*.md".into()));
                write_json(&output.join("manifest.json"), &manifest);
            }
            "rejected-union" => {
                corpus["rejected"][0]["source_path"] = corpus["accepted"][0]["source_path"].clone();
            }
            _ => unreachable!(),
        }
        write_json(&report_path, &corpus);

        let rejected = verify(&output);
        assert_eq!(
            rejected.status.code(),
            Some(1),
            "{case} was accepted: {}",
            String::from_utf8_lossy(&rejected.stdout)
        );
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains("bundle.corpus_report_invalid"),
            "{case} did not emit corpus report rejection: {}",
            String::from_utf8_lossy(&rejected.stderr)
        );
    }
}
