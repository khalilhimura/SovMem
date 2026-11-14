#[allow(dead_code)]
mod support;

use serde_json::{json, Value};
use sovmem::profile::{
    export_corpus, load_corpus, load_profile, validate_golden_file, CorpusExportRequest,
    GoldenError, ProfileConfig, PINNED_SOURCE_COMMIT,
};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use support::GitFixture;

const COVERAGE: [&str; 5] = [
    "core",
    "tfis-ssa",
    "sovereign-context",
    "agent-operations",
    "governance-evidence",
];

#[derive(Clone)]
struct Evidence {
    id: String,
}

struct TemporaryArea {
    root: PathBuf,
}

impl TemporaryArea {
    fn new(name: &str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must follow the Unix epoch")
            .as_nanos();
        let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-golden-{name}-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create temporary golden-query area");
        Self { root }
    }
}

impl Drop for TemporaryArea {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct GoldenFixture {
    _area: TemporaryArea,
    profile: ProfileConfig,
    bundle: PathBuf,
    golden: PathBuf,
    queries: Vec<Value>,
    evidence: Vec<Evidence>,
}

impl GoldenFixture {
    fn new(name: &str) -> Self {
        let area = TemporaryArea::new(name);
        let profile = load_profile(&profile_path()).expect("load Audy profile fixture");
        let bundle = area.root.join("bundle");
        copy_tree(base_bundle(), &bundle);

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
        let evidence = COVERAGE
            .iter()
            .map(|category| Evidence {
                id: by_category[*category].clone(),
            })
            .collect::<Vec<_>>();

        let queries = (1..=25)
            .map(|number| {
                let evidence = &evidence[(number - 1) % evidence.len()];
                json!({
                    "id": format!("q-{number:03}"),
                    "query": format!("Synthetic query {number:03}"),
                    "relevant_ids": [&evidence.id],
                    "rationale": format!("Synthetic rationale {number:03}"),
                    "authorship": "human"
                })
            })
            .collect::<Vec<_>>();
        let golden = area.root.join("golden.jsonl");
        write_jsonl(&golden, &queries);

        Self {
            _area: area,
            profile,
            bundle,
            golden,
            queries,
            evidence,
        }
    }

    fn write_queries(&self) {
        write_jsonl(&self.golden, &self.queries);
    }

    fn error(&self) -> GoldenError {
        validate_golden_file(&self.profile, &self.bundle, &self.golden)
            .expect_err("synthetic mutation must fail")
    }
}

struct BaseBundle {
    _area: TemporaryArea,
    bundle: PathBuf,
}

fn base_bundle() -> &'static Path {
    static BASE: OnceLock<BaseBundle> = OnceLock::new();
    &BASE
        .get_or_init(|| {
            let area = TemporaryArea::new("strict-base");
            let repo = GitFixture::new("golden-strict-base");
            for path in load_corpus(&corpus_path())
                .expect("load approved corpus for golden fixture")
                .selected_paths()
            {
                repo.write(
                    &path,
                    "# Synthetic evidence\n\nThis synthetic source records deterministic evidence with https://example.com/source.\n",
                );
            }
            repo.commit_all("add synthetic corpus", "2026-01-02T12:00:00Z");
            let git_environment = install_git_shim(&repo.head());
            let bundle = area.root.join("published-bundle");
            export_corpus(CorpusExportRequest {
                vault_root: repo.root.clone(),
                output_root: bundle.clone(),
                profile_path: profile_path(),
                corpus_path: corpus_path(),
            })
            .expect("export strict synthetic golden evidence bundle");
            drop(git_environment);
            drop(repo);
            BaseBundle {
                _area: area,
                bundle,
            }
        })
        .bundle
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create copied golden bundle directory");
    for entry in fs::read_dir(source).expect("read golden bundle source") {
        let entry = entry.expect("read golden bundle entry");
        let target = destination.join(entry.file_name());
        if entry
            .file_type()
            .expect("inspect golden bundle entry")
            .is_dir()
        {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy golden bundle artifact");
        }
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
            std::env::temp_dir().join(format!("sovmem-golden-git-shim-{}", std::process::id()));
        fs::create_dir(&directory).expect("create golden fixture Git shim directory");
        let path = directory.join("git");
        let script = format!(
            "#!/bin/bash\nset -e\npinned='{PINNED_SOURCE_COMMIT}'\nactual=\"${{SOVMEM_TEST_ACTUAL_COMMIT:?}}\"\nresolve=0\nargs=()\nfor arg in \"$@\"; do\n  if [[ \"$arg\" == \"$pinned^{{commit}}\" ]]; then\n    resolve=1\n    args+=(\"$actual^{{commit}}\")\n  else\n    args+=(\"${{arg//$pinned/$actual}}\")\n  fi\ndone\nif (( resolve )); then\n  /usr/bin/git \"${{args[@]}}\" >/dev/null\n  printf '%s\\n' \"$pinned\"\nelse\n  exec /usr/bin/git \"${{args[@]}}\"\nfi\n"
        );
        fs::write(&path, script).expect("write golden fixture Git shim");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("make golden fixture Git shim executable");
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
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
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

fn write_json(path: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).expect("serialize synthetic JSON");
    bytes.push(b'\n');
    fs::write(path, bytes).expect("write synthetic JSON");
}

fn write_jsonl(path: &Path, values: &[Value]) {
    let mut output = Vec::new();
    for value in values {
        serde_json::to_writer(&mut output, value).expect("serialize synthetic JSONL line");
        output.push(b'\n');
    }
    fs::write(path, output).expect("write synthetic JSONL");
}

#[test]
fn validates_twenty_five_synthetic_queries_covering_core_and_every_theme() {
    let fixture = GoldenFixture::new("valid");

    let summary = validate_golden_file(&fixture.profile, &fixture.bundle, &fixture.golden).unwrap();

    assert_eq!(summary.queries, 25);
    assert_eq!(summary.relevant_ids, 25);
    assert_eq!(
        summary.coverage,
        vec![
            "core".to_owned(),
            "tfis-ssa".to_owned(),
            "sovereign-context".to_owned(),
            "agent-operations".to_owned(),
            "governance-evidence".to_owned(),
        ]
    );
}

#[test]
fn missing_golden_file_has_stable_read_code() {
    let fixture = GoldenFixture::new("read");
    fs::remove_file(&fixture.golden).unwrap();
    assert_eq!(fixture.error().code(), "golden.read");
}

#[test]
fn non_newline_terminated_golden_file_has_stable_parse_code() {
    let fixture = GoldenFixture::new("parse");
    let mut bytes = fs::read(&fixture.golden).unwrap();
    bytes.pop();
    fs::write(&fixture.golden, bytes).unwrap();
    assert_eq!(fixture.error().code(), "golden.parse");
}

#[test]
fn blank_invalid_and_unknown_field_lines_have_stable_parse_code() {
    let blank = GoldenFixture::new("parse-blank");
    let mut blank_bytes = fs::read(&blank.golden).unwrap();
    let first_newline = blank_bytes.iter().position(|byte| *byte == b'\n').unwrap();
    blank_bytes.insert(first_newline + 1, b'\n');
    fs::write(&blank.golden, blank_bytes).unwrap();
    assert_eq!(blank.error().code(), "golden.parse");

    let invalid = GoldenFixture::new("parse-invalid-json");
    let mut invalid_bytes = fs::read(&invalid.golden).unwrap();
    let first_newline = invalid_bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap();
    invalid_bytes.splice(..first_newline, b"{".iter().copied());
    fs::write(&invalid.golden, invalid_bytes).unwrap();
    assert_eq!(invalid.error().code(), "golden.parse");

    let mut unknown = GoldenFixture::new("parse-unknown-field");
    unknown.queries[0]["unexpected"] = json!(true);
    unknown.write_queries();
    assert_eq!(unknown.error().code(), "golden.parse");
}

#[test]
fn too_few_golden_queries_have_stable_count_code() {
    let mut fixture = GoldenFixture::new("count");
    fixture.queries.pop();
    fixture.write_queries();
    assert_eq!(fixture.error().code(), "golden.count");
}

#[test]
fn malformed_query_identifier_has_stable_id_code() {
    let mut fixture = GoldenFixture::new("id");
    fixture.queries[0]["id"] = json!("q-000");
    fixture.write_queries();
    assert_eq!(fixture.error().code(), "golden.id");
}

#[test]
fn duplicate_query_identifier_has_stable_id_code() {
    let mut fixture = GoldenFixture::new("duplicate-id");
    fixture.queries[1]["id"] = fixture.queries[0]["id"].clone();
    fixture.write_queries();
    assert_eq!(fixture.error().code(), "golden.id");
}

#[test]
fn duplicate_normalized_query_has_stable_query_code() {
    let mut fixture = GoldenFixture::new("query");
    fixture.queries[1]["query"] = json!("  Synthetic   query 001  ");
    fixture.write_queries();
    assert_eq!(fixture.error().code(), "golden.query");
}

#[test]
fn blank_and_control_queries_have_stable_query_code() {
    let mut blank = GoldenFixture::new("blank-query");
    blank.queries[0]["query"] = json!("   ");
    blank.write_queries();
    assert_eq!(blank.error().code(), "golden.query");

    let mut control = GoldenFixture::new("control-query");
    control.queries[0]["query"] = json!("Synthetic\tquery");
    control.write_queries();
    assert_eq!(control.error().code(), "golden.query");
}

#[test]
fn nonhuman_authorship_has_stable_authorship_code() {
    let mut fixture = GoldenFixture::new("authorship");
    fixture.queries[0]["authorship"] = json!("synthetic-generator");
    fixture.write_queries();
    assert_eq!(fixture.error().code(), "golden.authorship");
}

#[test]
fn foreign_profile_relevance_has_stable_relevance_code() {
    let mut fixture = GoldenFixture::new("relevance");
    fixture.queries[0]["relevant_ids"] = json!(["ssa-1/01-Soul/Synthetic%20Core"]);
    fixture.write_queries();
    assert_eq!(fixture.error().code(), "golden.relevance");
}

#[test]
fn empty_duplicate_unsorted_noncanonical_and_absent_relevance_have_stable_code() {
    let mut empty = GoldenFixture::new("relevance-empty");
    empty.queries[0]["relevant_ids"] = json!([]);
    empty.write_queries();
    assert_eq!(empty.error().code(), "golden.relevance");

    let mut duplicate = GoldenFixture::new("relevance-duplicate");
    let id = duplicate.evidence[0].id.clone();
    duplicate.queries[0]["relevant_ids"] = json!([id, duplicate.evidence[0].id.clone()]);
    duplicate.write_queries();
    assert_eq!(duplicate.error().code(), "golden.relevance");

    let mut unsorted = GoldenFixture::new("relevance-unsorted");
    let mut ids = [
        unsorted.evidence[0].id.clone(),
        unsorted.evidence[1].id.clone(),
    ];
    ids.sort();
    ids.reverse();
    unsorted.queries[0]["relevant_ids"] = json!(ids);
    unsorted.write_queries();
    assert_eq!(unsorted.error().code(), "golden.relevance");

    let mut noncanonical = GoldenFixture::new("relevance-noncanonical");
    noncanonical.queries[0]["relevant_ids"] = json!(["ssa-0/04-Research/%2f"]);
    noncanonical.write_queries();
    assert_eq!(noncanonical.error().code(), "golden.relevance");

    let mut absent = GoldenFixture::new("relevance-absent");
    absent.queries[0]["relevant_ids"] = json!(["ssa-0/04-Research/Synthetic%20Absent"]);
    absent.write_queries();
    assert_eq!(absent.error().code(), "golden.relevance");
}

#[test]
fn explicit_forbidden_content_has_stable_privacy_code() {
    let mut fixture = GoldenFixture::new("privacy");
    fixture.queries[0]["rationale"] = json!("Synthetic rationale referencing /Users/example");
    fixture.write_queries();
    assert_eq!(fixture.error().code(), "golden.privacy");
}

#[test]
fn absolute_filesystem_paths_in_queries_and_rationales_have_stable_privacy_code() {
    let cases = [
        "/home/example/private.md",
        "/private/tmp/retrieval-evidence.json",
        r"C:\Users\example\private.md",
        "D:/research/private.md",
        r"\\server\share\private.md",
        "//server/share/private.md",
    ];

    for (index, absolute_path) in cases.into_iter().enumerate() {
        let mut query = GoldenFixture::new(&format!("privacy-absolute-query-{index}"));
        query.queries[0]["query"] = json!(format!("Find evidence at {absolute_path}"));
        query.write_queries();
        assert_eq!(query.error().code(), "golden.privacy", "{absolute_path}");

        let mut rationale = GoldenFixture::new(&format!("privacy-absolute-rationale-{index}"));
        rationale.queries[0]["rationale"] =
            json!(format!("The evidence was copied from {absolute_path}"));
        rationale.write_queries();
        assert_eq!(
            rationale.error().code(),
            "golden.privacy",
            "{absolute_path}"
        );
    }
}

#[test]
fn urls_and_ordinary_slash_separated_prose_are_not_filesystem_paths() {
    let mut fixture = GoldenFixture::new("privacy-safe-slashes");
    fixture.queries[0]["query"] =
        json!("How does https://example.com/Users/example/research support input/output review?");
    fixture.queries[0]["rationale"] =
        json!("The source compares doctrine/evidence and act/escalate boundaries.");
    fixture.write_queries();

    assert!(validate_golden_file(&fixture.profile, &fixture.bundle, &fixture.golden).is_ok());
}

#[test]
fn existing_secret_detection_and_forbidden_lane_have_stable_privacy_code() {
    let mut secret = GoldenFixture::new("privacy-secret");
    secret.queries[0]["rationale"] = json!("api_key: synthetic-value");
    secret.write_queries();
    assert_eq!(secret.error().code(), "golden.privacy");

    let mut lane = GoldenFixture::new("privacy-lane");
    lane.queries[0]["query"] = json!("Find synthetic 08-Hermes evidence");
    lane.write_queries();
    assert_eq!(lane.error().code(), "golden.privacy");
}

#[test]
fn missing_theme_relevance_has_stable_coverage_code() {
    let mut fixture = GoldenFixture::new("coverage");
    for query in &mut fixture.queries {
        query["relevant_ids"] = json!([fixture.evidence[0].id.clone()]);
    }
    fixture.write_queries();
    assert_eq!(fixture.error().code(), "golden.coverage");
}

#[test]
fn inconsistent_bundle_hash_has_stable_bundle_code() {
    let fixture = GoldenFixture::new("bundle");
    let report_path = fixture.bundle.join("corpus-report.json");
    let mut report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    report["bundle_hash"] =
        json!("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd");
    write_json(&report_path, &report);
    assert_eq!(fixture.error().code(), "golden.bundle");
}

#[test]
fn balanced_theme_relabeling_cannot_redefine_golden_coverage() {
    let fixture = GoldenFixture::new("balanced-theme-relabel");
    let report_path = fixture.bundle.join("corpus-report.json");
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
    accepted[tfis]["theme"] = json!("agent-operations");
    accepted[operations]["theme"] = json!("tfis-ssa");
    write_json(&report_path, &report);

    assert_eq!(fixture.error().code(), "golden.bundle");
}

#[test]
fn matching_forged_hash_claims_cannot_replace_published_content_verification() {
    let fixture = GoldenFixture::new("forged-hash-claims");
    let manifest_path = fixture.bundle.join("manifest.json");
    let report_path = fixture.bundle.join("corpus-report.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let mut report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    let output_path = manifest["documents"][0]["output_path"].as_str().unwrap();
    let generated_path = fixture.bundle.join(output_path);
    let mut generated = fs::read(&generated_path).unwrap();
    generated.extend_from_slice(b"\nforged published content\n");
    fs::write(generated_path, generated).unwrap();
    let forged_hash = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    manifest["bundle_hash"] = json!(forged_hash);
    report["bundle_hash"] = json!(forged_hash);
    write_json(&manifest_path, &manifest);
    write_json(&report_path, &report);

    assert_eq!(fixture.error().code(), "golden.bundle");
}

#[test]
fn schema_counts_core_theme_and_accepted_order_claims_are_reconstructed() {
    let schema = GoldenFixture::new("bundle-schema");
    mutate_report(&schema, |report| {
        report["schema"] = json!("foreign-corpus-v1")
    });
    assert_eq!(schema.error().code(), "golden.bundle");

    let counts = GoldenFixture::new("bundle-counts");
    mutate_report(&counts, |report| report["counts"]["candidates"] = json!(62));
    assert_eq!(counts.error().code(), "golden.bundle");

    let core = GoldenFixture::new("bundle-core");
    mutate_report(&core, |report| report["required_core"] = json!([]));
    assert_eq!(core.error().code(), "golden.bundle");

    let themes = GoldenFixture::new("bundle-theme-counts");
    mutate_report(&themes, |report| {
        report["research_by_theme"]["tfis-ssa"] = json!(11)
    });
    assert_eq!(themes.error().code(), "golden.bundle");

    let order = GoldenFixture::new("bundle-accepted-order");
    mutate_report(&order, |report| {
        report["accepted"].as_array_mut().unwrap().swap(0, 1)
    });
    assert_eq!(order.error().code(), "golden.bundle");
}

#[test]
fn malformed_profile_set_source_hash_and_lane_claims_are_rejected() {
    let malformed = GoldenFixture::new("bundle-malformed");
    fs::write(
        malformed.bundle.join("corpus-report.json"),
        b"{ invalid synthetic evidence\n",
    )
    .unwrap();
    assert_eq!(malformed.error().code(), "golden.bundle");

    let profile = GoldenFixture::new("bundle-profile");
    mutate_report(&profile, |report| {
        report["profile"] = json!("foreign-profile")
    });
    assert_eq!(profile.error().code(), "golden.bundle");

    let set = GoldenFixture::new("bundle-set");
    mutate_report(&set, |report| {
        report["accepted"].as_array_mut().unwrap().pop();
    });
    assert_eq!(set.error().code(), "golden.bundle");

    let source_hash = GoldenFixture::new("bundle-source-hash");
    mutate_report(&source_hash, |report| {
        report["accepted"][0]["source_hash"] = json!("0".repeat(64));
    });
    assert_eq!(source_hash.error().code(), "golden.bundle");

    let lane = GoldenFixture::new("bundle-lane");
    mutate_report(&lane, |report| {
        let accepted = report["accepted"].as_array_mut().unwrap();
        let core_index = accepted
            .iter()
            .position(|entry| entry["lane"] == "core")
            .unwrap();
        let research_index = accepted
            .iter()
            .position(|entry| entry["theme"] == "tfis-ssa")
            .unwrap();
        accepted[core_index]["lane"] = json!("research");
        accepted[core_index]["theme"] = json!("tfis-ssa");
        accepted[research_index]["lane"] = json!("core");
        accepted[research_index]["theme"] = Value::Null;
    });
    assert_eq!(lane.error().code(), "golden.bundle");
}

fn mutate_report(fixture: &GoldenFixture, mutate: impl FnOnce(&mut Value)) {
    let path = fixture.bundle.join("corpus-report.json");
    let mut report: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    mutate(&mut report);
    write_json(&path, &report);
}
