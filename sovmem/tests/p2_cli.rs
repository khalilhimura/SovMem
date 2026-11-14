#[allow(dead_code)]
mod support;

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, MutexGuard, OnceLock};
use support::p2::P2Fixture;

const SECRET_QUERY: &str = "private-query-sentinel";
const TITLE_SENTINEL: &str = "fieldboostneedle";
const BODY_SENTINEL: &str = "This offline fixture document";
const CHANGED_DOCUMENT_SENTINEL: &str = "private-changed-document-sentinel";
const REPAIRED_ROOT_SENTINEL: &str = "private-repaired-root-sentinel";
const INVALID_PROVENANCE_SENTINEL: &str = "private-invalid-provenance-sentinel";
const P2_USAGE_CODE: &str = "cli.usage";
const P2_USAGE_MESSAGE: &str = "P2 command usage is invalid";

type IndexOpenCorruptionCase = (&'static str, &'static str, fn(&Path));

fn sovmem() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sovmem"))
}

fn run_index(fixture: &P2Fixture, index_root: &Path, format: Option<&str>) -> Output {
    let mut command = sovmem();
    command
        .arg("index")
        .arg("--profile")
        .arg(&fixture.profile_path)
        .arg("--bundle")
        .arg(&fixture.bundle_root)
        .arg("--index")
        .arg(index_root);
    if let Some(format) = format {
        command.arg("--format").arg(format);
    }
    command.output().expect("run sovmem index")
}

fn run_query(
    fixture: &P2Fixture,
    index_root: &Path,
    query: &str,
    top_k: Option<&str>,
    expansion: Option<&str>,
    format: Option<&str>,
) -> Output {
    let mut command = sovmem();
    command
        .arg("query")
        .arg("--profile")
        .arg(&fixture.profile_path)
        .arg("--bundle")
        .arg(&fixture.bundle_root)
        .arg("--index")
        .arg(index_root)
        .arg("--query")
        .arg(query);
    if let Some(top_k) = top_k {
        command.arg("--top-k").arg(top_k);
    }
    if let Some(expansion) = expansion {
        command.arg("--expand").arg(expansion);
    }
    if let Some(format) = format {
        command.arg("--format").arg(format);
    }
    command.output().expect("run sovmem query")
}

fn run_eval(fixture: &P2Fixture, index_root: &Path, baseline: &Path, format: &str) -> Output {
    sovmem()
        .arg("eval")
        .arg("--profile")
        .arg(&fixture.profile_path)
        .arg("--bundle")
        .arg(&fixture.bundle_root)
        .arg("--index")
        .arg(index_root)
        .arg("--golden")
        .arg(&fixture.golden_path)
        .arg("--thresholds")
        .arg(&fixture.thresholds_path)
        .arg("--baseline")
        .arg(baseline)
        .arg("--format")
        .arg(format)
        .output()
        .expect("run sovmem eval")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

fn serial_fixture() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn assert_one_trailing_newline(bytes: &[u8]) {
    assert!(bytes.ends_with(b"\n"));
    assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .expect("read current directory")
            .join(path)
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in absolute_path(path).components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                }
            }
            Component::Normal(value) => normalized.push(value),
        }
    }
    normalized
}

fn canonicalize_with_missing_tail(path: &Path) -> PathBuf {
    let normalized = normalize_path(path);
    let mut existing = normalized.clone();
    let mut missing = Vec::<OsString>::new();
    loop {
        if let Ok(mut canonical) = fs::canonicalize(&existing) {
            for component in missing.iter().rev() {
                canonical.push(component);
            }
            return normalize_path(&canonical);
        }
        let Some(component) = existing.file_name().map(|value| value.to_owned()) else {
            return normalized;
        };
        missing.push(component);
        if !existing.pop() {
            return normalized;
        }
    }
}

fn insert_macos_var_aliases(values: &mut BTreeSet<String>) {
    for value in values.clone() {
        if value == "/var" || value.starts_with("/var/") {
            values.insert(format!("/private{value}"));
        }
        if value == "/private/var" || value.starts_with("/private/var/") {
            values.insert(value.replacen("/private", "", 1));
        }
    }
}

fn banned_path_values(path: &Path) -> Vec<String> {
    let absolute = absolute_path(path);
    let mut values = BTreeSet::new();
    for candidate in [
        path.to_owned(),
        absolute.clone(),
        normalize_path(&absolute),
        fs::canonicalize(&absolute).unwrap_or_else(|_| canonicalize_with_missing_tail(&absolute)),
    ] {
        let value = candidate.to_string_lossy().into_owned();
        if !value.is_empty() {
            values.insert(value);
        }
    }
    insert_macos_var_aliases(&mut values);
    values.into_iter().collect()
}

fn assert_combined_excludes(combined: &str, banned: &[String]) {
    for private in banned {
        assert!(
            !private.is_empty() && !combined.contains(private),
            "diagnostic leaked {private:?}"
        );
    }
}

fn failure_banned_values(
    fixture: &P2Fixture,
    index_root: &Path,
    extra_paths: &[&Path],
) -> Vec<String> {
    let mut private_values = BTreeSet::from([
        SECRET_QUERY.to_owned(),
        TITLE_SENTINEL.to_owned(),
        BODY_SENTINEL.to_owned(),
        CHANGED_DOCUMENT_SENTINEL.to_owned(),
        REPAIRED_ROOT_SENTINEL.to_owned(),
        INVALID_PROVENANCE_SENTINEL.to_owned(),
        "private-current-target".to_owned(),
        "private_unknown_sentinel".to_owned(),
        "private-version-sentinel".to_owned(),
        "index-manifest.json".to_owned(),
        "graph.json".to_owned(),
        "/tantivy/".to_owned(),
        "meta.json".to_owned(),
        ".managed.json".to_owned(),
        "slot-a".to_owned(),
        "slot-b".to_owned(),
        ".building-".to_owned(),
    ]);
    let mut private_paths = vec![
        fixture.profile_path.as_path(),
        fixture.bundle_root.as_path(),
        fixture.golden_path.as_path(),
        fixture.thresholds_path.as_path(),
        index_root,
        index_root.parent().expect("fixture index root has parent"),
    ];
    private_paths.extend_from_slice(extra_paths);
    for path in private_paths {
        private_values.extend(banned_path_values(path));
    }

    if let Ok(bytes) = fs::read(fixture.bundle_root.join("manifest.json")) {
        if let Ok(manifest) = serde_json::from_slice::<Value>(&bytes) {
            for document in manifest["documents"].as_array().into_iter().flatten() {
                for key in ["source_path", "output_path"] {
                    if let Some(path) = document[key].as_str() {
                        private_values.insert(path.to_owned());
                    }
                }
            }
        }
    }
    for slot in ["slot-a", "slot-b"] {
        let tantivy = index_root.join(slot).join("tantivy");
        if let Ok(entries) = fs::read_dir(tantivy) {
            for entry in entries.flatten() {
                private_values.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    if let Ok(entries) = fs::read_dir(index_root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".building-") {
                private_values.insert(name);
                private_values.extend(banned_path_values(&entry.path()));
            }
        }
    }
    private_values.into_iter().collect()
}

#[test]
fn path_bans_catch_normalized_and_macos_canonical_only_forms() {
    let raw = Path::new("/var/tmp/private-parent/../canonical-only-sentinel");
    let canonical_only = "/private/var/tmp/canonical-only-sentinel";
    let banned = banned_path_values(raw);

    assert!(banned.iter().any(|value| value == canonical_only));
    let caught = std::panic::catch_unwind(|| {
        assert_combined_excludes(
            &format!("redacted diagnostic leaked {canonical_only}"),
            &banned,
        );
    });
    assert!(caught.is_err(), "canonical-only path leak was not detected");
}

#[test]
fn failure_bans_include_building_prefix_and_actual_staging_entries() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new("cli-banned-staging-name");
    let root = fixture.index_root("index");
    fs::create_dir(&root).expect("create fixture index root");
    let staging_name = format!(".building-{}-987", std::process::id());
    fs::create_dir(root.join(&staging_name)).expect("create dynamic staging entry");

    let banned = failure_banned_values(&fixture, &root, &[]);
    assert!(banned.iter().any(|value| value == ".building-"));
    assert!(banned.iter().any(|value| value == &staging_name));
}

fn assert_redacted_p2_usage(output: &Output, private_values: &[&str]) {
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = stderr(output);
    assert_eq!(
        stderr,
        format!(
            "{{\"severity\":\"error\",\"code\":\"{P2_USAGE_CODE}\",\"message\":\"{P2_USAGE_MESSAGE}\"}}\n"
        )
    );
    let lines: Vec<_> = stderr.lines().collect();
    assert_eq!(lines.len(), 1);
    let diagnostic: Value = serde_json::from_str(lines[0]).expect("P2 usage diagnostic is JSONL");
    assert_eq!(diagnostic["severity"], "error");
    assert_eq!(diagnostic["code"], P2_USAGE_CODE);
    assert_eq!(diagnostic["message"], P2_USAGE_MESSAGE);
    let mut banned = BTreeSet::new();
    for private in private_values {
        banned.insert((*private).to_owned());
        if Path::new(private).is_absolute() {
            banned.extend(banned_path_values(Path::new(private)));
        }
    }
    assert_combined_excludes(
        &format!("{}{}", stdout(output), stderr),
        &banned.into_iter().collect::<Vec<_>>(),
    );
}

fn assert_redacted_failure(
    output: &Output,
    expected_exit: i32,
    expected_code: &str,
    fixture: &P2Fixture,
    index_root: &Path,
) {
    assert_redacted_failure_with_paths(
        output,
        expected_exit,
        expected_code,
        fixture,
        index_root,
        &[],
    );
}

fn assert_redacted_failure_with_paths(
    output: &Output,
    expected_exit: i32,
    expected_code: &str,
    fixture: &P2Fixture,
    index_root: &Path,
    extra_paths: &[&Path],
) {
    assert_eq!(output.status.code(), Some(expected_exit));
    assert!(output.stdout.is_empty(), "failure emitted success stdout");
    let stderr = stderr(output);
    let combined = format!("{}{}", stdout(output), stderr);
    let lines: Vec<_> = stderr.lines().collect();
    assert!(!lines.is_empty(), "failure emitted no diagnostic");
    assert!(lines.windows(2).all(|pair| pair[0] <= pair[1]));
    let diagnostics: Vec<Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("diagnostic is JSONL"))
        .collect();
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic["severity"] == "error" && diagnostic["code"] == expected_code
        }),
        "missing {expected_code} in {stderr}"
    );
    assert_combined_excludes(
        &combined,
        &failure_banned_values(fixture, index_root, extra_paths),
    );
}

fn assert_redacted_failure_codes(
    output: &Output,
    expected_exit: i32,
    expected_codes: &[&str],
    fixture: &P2Fixture,
    index_root: &Path,
) {
    assert_redacted_failure(
        output,
        expected_exit,
        expected_codes[0],
        fixture,
        index_root,
    );
    let diagnostics = stderr(output)
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("diagnostic is JSONL"))
        .collect::<Vec<_>>();
    for code in expected_codes {
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic["code"] == *code),
            "missing {code}"
        );
    }
}

fn current_generation(index_root: &Path) -> std::path::PathBuf {
    let slot = fs::read_to_string(index_root.join("CURRENT"))
        .expect("read CURRENT")
        .trim()
        .to_owned();
    index_root.join(slot)
}

fn mutate_manifest(index_root: &Path, key: &str, value: Value) {
    let path = current_generation(index_root).join("index-manifest.json");
    let bytes = fs::read_to_string(&path).expect("read index manifest");
    let manifest: Value = serde_json::from_str(&bytes).expect("parse index manifest");
    let old_value = serde_json::to_string(&manifest[key]).expect("serialize old manifest value");
    let new_value = serde_json::to_string(&value).expect("serialize new manifest value");
    let old_field = format!("  \"{key}\": {old_value}");
    let new_field = format!("  \"{key}\": {new_value}");
    assert_eq!(bytes.matches(&old_field).count(), 1);
    fs::write(path, bytes.replacen(&old_field, &new_field, 1))
        .expect("write mutated index manifest");
}

fn write_pretty_json(path: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).expect("serialize mutated JSON");
    bytes.push(b'\n');
    fs::write(path, bytes).expect("write mutated JSON");
}

fn mutate_graph(index_root: &Path, mutate: impl FnOnce(&mut Value)) {
    let path = current_generation(index_root).join("graph.json");
    let mut graph: Value =
        serde_json::from_slice(&fs::read(&path).expect("read graph")).expect("parse graph");
    mutate(&mut graph);
    write_pretty_json(&path, &graph);
}

fn first_bundle_document(fixture: &P2Fixture) -> std::path::PathBuf {
    let manifest: Value = serde_json::from_slice(
        &fs::read(fixture.bundle_root.join("manifest.json")).expect("read bundle manifest"),
    )
    .expect("parse bundle manifest");
    fixture.bundle_root.join(
        manifest["documents"][0]["output_path"]
            .as_str()
            .expect("first bundle output path"),
    )
}

fn mutate_threshold(fixture: &P2Fixture, mutate: impl FnOnce(&mut Value)) {
    let mut thresholds: Value =
        serde_json::from_slice(&fs::read(&fixture.thresholds_path).expect("read thresholds"))
            .expect("parse thresholds");
    mutate(&mut thresholds);
    write_pretty_json(&fixture.thresholds_path, &thresholds);
}

fn query_json_expected(value: &Value) -> String {
    let mut output = format!(
        "{{\"status\":\"ok\",\"command\":\"query\",\"top_k\":{},\"expansion\":{},\"bundle_hash\":{},\"results\":[",
        value["top_k"], value["expansion"], value["bundle_hash"]
    );
    for (position, hit) in value["results"].as_array().unwrap().iter().enumerate() {
        if position > 0 {
            output.push(',');
        }
        output.push_str(&format!(
            "{{\"rank\":{},\"id\":{},\"title\":{},\"type\":{},\"layer\":{},\"provenance\":{},\"score\":{}}}",
            hit["rank"], hit["id"], hit["title"], hit["type"], hit["layer"],
            hit["provenance"], hit["score"]
        ));
    }
    output.push_str("],\"expanded\":[");
    for (position, hit) in value["expanded"].as_array().unwrap().iter().enumerate() {
        if position > 0 {
            output.push(',');
        }
        output.push_str(&format!(
            "{{\"rank\":{},\"id\":{},\"title\":{},\"type\":{},\"layer\":{},\"provenance\":{},\"via_id\":{},\"direction\":{},\"relation\":{}}}",
            hit["rank"], hit["id"], hit["title"], hit["type"], hit["layer"],
            hit["provenance"], hit["via_id"], hit["direction"], hit["relation"]
        ));
    }
    output.push_str("]}\n");
    output
}

#[test]
fn index_prints_one_exact_compact_json_summary_by_default() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_with_query_cases("cli-index-json");
    let root = fixture.index_root("index");
    let output = run_index(&fixture, &root, None);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty());
    assert_one_trailing_newline(&output.stdout);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "ok");
    assert_eq!(value["command"], "index");
    assert_eq!(value["schema_version"], "sovmem-derived-index-v1");
    assert_eq!(value["documents"], 62);
    assert_eq!(value["graph_nodes"], 62);
    assert_eq!(value["graph_edges"], 19);
    assert_eq!(value["external_typed_links"], 81);
    assert_eq!(value["bundle_hash"].as_str().unwrap().len(), 64);
    assert_eq!(value["index_config"].as_str().unwrap().len(), 64);
    assert_eq!(value["replaced_invalid_current"], false);
    let expected = format!(
        "{{\"status\":\"ok\",\"command\":\"index\",\"schema_version\":\"sovmem-derived-index-v1\",\"documents\":62,\"graph_nodes\":62,\"graph_edges\":19,\"external_typed_links\":81,\"bundle_hash\":{},\"index_config\":{},\"replaced_invalid_current\":false}}\n",
        value["bundle_hash"], value["index_config"]
    );
    assert_eq!(stdout(&output), expected);
}

#[test]
fn index_text_is_exact_and_exposes_no_path_or_slot() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_with_query_cases("cli-index-text");
    let root = fixture.index_root("index");
    let output = run_index(&fixture, &root, Some("text"));
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(output.stderr.is_empty());
    let value = stdout(&output);
    let lines: Vec<_> = value.lines().collect();
    assert_eq!(lines.len(), 9);
    assert_eq!(
        &lines[..6],
        &[
            "status: ok",
            "command: index",
            "documents: 62",
            "graph-nodes: 62",
            "graph-edges: 19",
            "external-typed-links: 81",
        ]
    );
    assert!(lines[6].starts_with("bundle-hash: "));
    assert_eq!(lines[6].len(), "bundle-hash: ".len() + 64);
    assert!(lines[7].starts_with("index-config: "));
    assert_eq!(lines[7].len(), "index-config: ".len() + 64);
    assert_eq!(lines[8], "replaced-invalid-current: false");
    assert!(value.ends_with('\n'));
    assert!(!value.contains(root.to_string_lossy().as_ref()));
    assert!(!value.contains("slot-a"));
    assert!(!value.contains("slot-b"));
}

#[test]
fn query_json_is_exact_metadata_only_and_never_echoes_query() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_with_query_cases("cli-query-json");
    let root = fixture.index_root("index");
    assert!(run_index(&fixture, &root, Some("json")).status.success());

    let empty = run_query(&fixture, &root, SECRET_QUERY, None, None, None);
    assert!(empty.status.success(), "{}", stderr(&empty));
    assert!(empty.stderr.is_empty());
    assert_eq!(
        stdout(&empty),
        format!(
            "{{\"status\":\"ok\",\"command\":\"query\",\"top_k\":5,\"expansion\":\"none\",\"bundle_hash\":\"{}\",\"results\":[],\"expanded\":[]}}\n",
            fixture.bundle_hash()
        )
    );

    let matches = run_query(
        &fixture,
        &root,
        TITLE_SENTINEL,
        Some("5"),
        Some("one-hop"),
        Some("json"),
    );
    assert!(matches.status.success(), "{}", stderr(&matches));
    assert!(matches.stderr.is_empty());
    assert_one_trailing_newline(&matches.stdout);
    let value: Value = serde_json::from_slice(&matches.stdout).unwrap();
    assert_eq!(stdout(&matches), query_json_expected(&value));
    assert!(!value["results"].as_array().unwrap().is_empty());
    assert!(!value["expanded"].as_array().unwrap().is_empty());
    let combined = format!("{}{}", stdout(&matches), stderr(&matches));
    assert!(!combined.contains(SECRET_QUERY));
    assert!(!combined.contains("\"body\""));
    assert!(!combined.contains("raw_score"));
    assert!(!combined.contains("graph_slot"));
}

#[test]
fn query_text_is_exact_line_oriented_and_json_escapes_titles() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_with_query_cases("cli-query-text");
    let root = fixture.index_root("index");
    assert!(run_index(&fixture, &root, Some("json")).status.success());
    let json_output = run_query(
        &fixture,
        &root,
        TITLE_SENTINEL,
        Some("5"),
        Some("one-hop"),
        Some("json"),
    );
    let value: Value = serde_json::from_slice(&json_output.stdout).unwrap();
    let text_output = run_query(
        &fixture,
        &root,
        TITLE_SENTINEL,
        Some("5"),
        Some("one-hop"),
        Some("text"),
    );
    assert!(text_output.status.success(), "{}", stderr(&text_output));
    assert!(text_output.stderr.is_empty());
    let mut expected = format!(
        "status: ok\ncommand: query\ntop-k: 5\nexpansion: one-hop\nbundle-hash: {}\n",
        value["bundle_hash"].as_str().unwrap()
    );
    for hit in value["results"].as_array().unwrap() {
        expected.push_str(&format!(
            "bm25[{}]: id={} score={} title={}\n",
            hit["rank"].as_u64().unwrap(),
            hit["id"].as_str().unwrap(),
            hit["score"].as_str().unwrap(),
            serde_json::to_string(hit["title"].as_str().unwrap()).unwrap()
        ));
    }
    for hit in value["expanded"].as_array().unwrap() {
        expected.push_str(&format!(
            "expanded[{}]: id={} via={} direction={} relation={} title={}\n",
            hit["rank"].as_u64().unwrap(),
            hit["id"].as_str().unwrap(),
            hit["via_id"].as_str().unwrap(),
            hit["direction"].as_str().unwrap(),
            hit["relation"].as_str().unwrap(),
            serde_json::to_string(hit["title"].as_str().unwrap()).unwrap()
        ));
    }
    let text = stdout(&text_output);
    assert_eq!(text, expected);
    assert!(text.contains("title=\"fieldboostneedle \\\"quoted\\\" \\\\ path\""));
    assert!(!text.contains('\r'));
    assert!(!text.contains(BODY_SENTINEL));
}

#[test]
fn eval_go_prints_integer_summary_and_writes_baseline() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_retrievable("cli-eval-go", 25);
    let root = fixture.index_root("index");
    assert!(run_index(&fixture, &root, Some("json")).status.success());
    let baseline = fixture.index_root("baseline.json");
    let output = run_eval(&fixture, &root, &baseline, "json");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        stdout(&output),
        "{\"status\":\"go\",\"command\":\"eval\",\"evaluation_set\":\"audy-sovmem-v0\",\"queries\":25,\"k\":5,\"queries_with_hit\":25,\"required_queries_with_hit\":20,\"precision_numerator\":25,\"precision_denominator\":125,\"recall_numerator\":25,\"recall_denominator\":25,\"baseline_written\":true}\n"
    );
    assert!(baseline.is_file());
}

#[test]
fn eval_text_is_exact_and_lists_missed_ids_in_golden_order() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_retrievable("cli-eval-text", 20);
    let root = fixture.index_root("index");
    fixture.build_index(&root);
    let baseline = fixture.index_root("baseline.json");
    let output = run_eval(&fixture, &root, &baseline, "text");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        stdout(&output),
        "Q3: GO\nevaluation-set: audy-sovmem-v0\nqueries: 25\nk: 5\nhits: 20/25\nrequired-hits: 20/25\nmacro-precision-at-5: 20/125\nmacro-recall-at-5: 20/25\nbaseline-written: true\nmissed-query-ids: q-021,q-022,q-023,q-024,q-025\n"
    );
}

#[test]
fn eval_text_prints_none_for_zero_misses() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_retrievable("cli-eval-text-no-misses", 25);
    let root = fixture.index_root("index");
    fixture.build_index(&root);
    let baseline = fixture.index_root("baseline.json");
    let output = run_eval(&fixture, &root, &baseline, "text");
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).ends_with("missed-query-ids: none\n"));
}

#[test]
fn valid_no_go_prints_complete_evidence_writes_baseline_and_exits_one() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_retrievable("cli-eval-no-go", 19);
    let root = fixture.index_root("index");
    fixture.build_index(&root);
    let baseline = fixture.index_root("baseline.json");
    let output = run_eval(&fixture, &root, &baseline, "json");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stdout(&output),
        "{\"status\":\"no-go\",\"command\":\"eval\",\"evaluation_set\":\"audy-sovmem-v0\",\"queries\":25,\"k\":5,\"queries_with_hit\":19,\"required_queries_with_hit\":20,\"precision_numerator\":19,\"precision_denominator\":125,\"recall_numerator\":19,\"recall_denominator\":25,\"baseline_written\":true}\n"
    );
    assert_eq!(
        stderr(&output),
        "{\"severity\":\"error\",\"code\":\"eval.q3_no_go\",\"message\":\"frozen Q3 retrieval gate did not pass\"}\n"
    );
    assert!(baseline.is_file());
}

#[test]
fn invalid_eval_emits_no_success_output_and_preserves_prior_baseline() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_retrievable("cli-eval-invalid", 25);
    let root = fixture.index_root("index");
    fixture.build_index(&root);
    fs::write(
        &fixture.thresholds_path,
        b"not-json private-threshold-sentinel\n",
    )
    .unwrap();
    let baseline = fixture.index_root("baseline.json");
    let prior = b"prior reviewed baseline\n";
    fs::write(&baseline, prior).unwrap();

    let output = run_eval(&fixture, &root, &baseline, "json");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(&baseline).unwrap(), prior);
    let error = stderr(&output);
    assert_eq!(
        error,
        "{\"severity\":\"error\",\"code\":\"thresholds.parse\",\"message\":\"evaluation operation failed\"}\n"
    );
    assert_redacted_failure_with_paths(
        &output,
        1,
        "thresholds.parse",
        &fixture,
        &root,
        &[&baseline],
    );
    assert!(!error.contains("private-threshold-sentinel"));
}

#[test]
fn frozen_eval_corruption_is_redacted_and_preserves_prior_baseline() {
    type Mutation = fn(&P2Fixture);
    let cases: &[(&str, Mutation, &str)] = &[
        (
            "golden-byte",
            |fixture| {
                let mut queries = fs::read_to_string(&fixture.golden_path)
                    .expect("read golden")
                    .lines()
                    .map(|line| serde_json::from_str::<Value>(line).expect("parse golden line"))
                    .collect::<Vec<_>>();
                queries[0]["query"] = json!("A changed but structurally valid human query");
                let mut bytes = Vec::new();
                for query in queries {
                    bytes.extend(serde_json::to_vec(&query).expect("serialize golden line"));
                    bytes.push(b'\n');
                }
                fs::write(&fixture.golden_path, bytes).expect("write changed golden");
            },
            "thresholds.golden_hash",
        ),
        (
            "threshold-field",
            |fixture| {
                mutate_threshold(fixture, |thresholds| {
                    thresholds["retrieval"]["minimum_queries_with_hit"] = json!(19)
                })
            },
            "thresholds.retrieval",
        ),
    ];

    for (name, mutate, expected) in cases {
        let _serial = serial_fixture();
        let fixture = P2Fixture::new_retrievable(&format!("cli-eval-{name}"), 25);
        let root = fixture.index_root("index");
        fixture.build_index(&root);
        let baseline = fixture.index_root("baseline.json");
        let prior = b"prior-reviewed-baseline\n";
        fs::write(&baseline, prior).expect("write prior baseline");
        mutate(&fixture);

        let output = run_eval(&fixture, &root, &baseline, "json");
        assert_redacted_failure_with_paths(&output, 1, expected, &fixture, &root, &[&baseline]);
        assert_eq!(fs::read(&baseline).unwrap(), prior, "case {name}");
    }
}

#[test]
fn query_validation_codes_are_redacted_and_exit_two() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_with_query_cases("cli-query-errors");
    let root = fixture.index_root("index");
    assert!(run_index(&fixture, &root, Some("json")).status.success());
    let many_tokens = (0..65)
        .map(|position| format!("t{position}"))
        .collect::<Vec<_>>()
        .join(" ");
    let cases = [
        (format!("{SECRET_QUERY}\u{0007}"), None, "query.invalid"),
        ("   ".to_owned(), None, "query.empty"),
        (many_tokens, None, "query.too_many_tokens"),
        (SECRET_QUERY.to_owned(), Some("0"), "query.invalid"),
        (SECRET_QUERY.to_owned(), Some("63"), "query.invalid"),
    ];
    for (query, top_k, code) in cases {
        let output = run_query(&fixture, &root, &query, top_k, Some("none"), Some("json"));
        assert_redacted_failure(&output, 2, code, &fixture, &root);
        assert!(!stderr(&output).contains(&query));
    }
}

#[test]
fn index_build_failures_have_stable_codes_exits_and_no_success_stdout() {
    let _serial = serial_fixture();
    let unverified = P2Fixture::new_with_query_cases("cli-index-unverified");
    let unverified_root = unverified.index_root("index");
    unverified.remove_corpus_report();
    let output = run_index(&unverified, &unverified_root, Some("json"));
    assert_redacted_failure(
        &output,
        1,
        "index.bundle_unverified",
        &unverified,
        &unverified_root,
    );

    let invalid = P2Fixture::new_with_query_cases("cli-index-output-invalid");
    let output = run_index(&invalid, &invalid.bundle_root, Some("json"));
    assert_redacted_failure(
        &output,
        2,
        "index.output_invalid",
        &invalid,
        &invalid.bundle_root,
    );

    let build = P2Fixture::new_with_query_cases("cli-index-build-failed");
    let build_root = build.index_root("index");
    fs::create_dir(&build_root).unwrap();
    let mut permissions = fs::metadata(&build_root).unwrap().permissions();
    permissions.set_mode(0o555);
    fs::set_permissions(&build_root, permissions).unwrap();
    let output = run_index(&build, &build_root, Some("json"));
    let mut permissions = fs::metadata(&build_root).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&build_root, permissions).unwrap();
    assert_redacted_failure(&output, 2, "index.build_failed", &build, &build_root);

    let publish = P2Fixture::new_with_query_cases("cli-index-publish-failed");
    let publish_root = publish.index_root("index");
    assert!(run_index(&publish, &publish_root, Some("json"))
        .status
        .success());
    fs::write(publish_root.join("slot-b"), b"blocked\n").unwrap();
    let output = run_index(&publish, &publish_root, Some("json"));
    assert_redacted_failure(&output, 2, "index.publish_failed", &publish, &publish_root);
    assert_eq!(fs::read(publish_root.join("CURRENT")).unwrap(), b"slot-a\n");
}

#[test]
fn level_three_cli_corruption_emits_underlying_and_umbrella_codes_without_leaks() {
    let _serial = serial_fixture();
    type Mutation = fn(&P2Fixture);
    let cases: &[(&str, Mutation, &str)] = &[
        (
            "missing-report",
            P2Fixture::remove_corpus_report,
            "bundle.corpus_report_required",
        ),
        (
            "changed-document",
            |fixture| {
                let path = first_bundle_document(fixture);
                let mut bytes = fs::read(&path).expect("read generated document");
                bytes.extend_from_slice(format!("\n{CHANGED_DOCUMENT_SENTINEL}\n").as_bytes());
                fs::write(path, bytes).expect("change generated document");
            },
            "bundle.document_hash_mismatch",
        ),
        (
            "repaired-root",
            |fixture| {
                let path = first_bundle_document(fixture);
                let mut bytes = fs::read(&path).expect("read generated document");
                bytes.extend_from_slice(format!("\n{REPAIRED_ROOT_SENTINEL}\n").as_bytes());
                fs::write(&path, &bytes).expect("change generated document");
                let manifest_path = fixture.bundle_root.join("manifest.json");
                let mut manifest: Value =
                    serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
                manifest["documents"][0]["hash"] = json!(blake3::hash(&bytes).to_hex().to_string());
                write_pretty_json(&manifest_path, &manifest);
            },
            "bundle.root_hash_mismatch",
        ),
        (
            "invalid-provenance",
            |fixture| {
                let path = first_bundle_document(fixture);
                let bytes = fs::read_to_string(&path).expect("read generated document");
                fs::write(
                    path,
                    bytes.replacen(
                        "provenance: synthesis",
                        &format!("provenance: {INVALID_PROVENANCE_SENTINEL}"),
                        1,
                    ),
                )
                .expect("write invalid provenance");
            },
            "document.provenance_invalid",
        ),
    ];

    for (name, mutate, underlying) in cases {
        let fixture = P2Fixture::new_with_query_cases(&format!("cli-bundle-{name}"));
        let root = fixture.index_root("index");
        assert!(run_index(&fixture, &root, Some("json")).status.success());
        let prior_current = fs::read(root.join("CURRENT")).expect("read prior CURRENT");
        let prior_manifest =
            fs::read(current_generation(&root).join("index-manifest.json")).unwrap();
        mutate(&fixture);

        let output = run_index(&fixture, &root, Some("json"));
        assert_redacted_failure_codes(
            &output,
            1,
            &["index.bundle_unverified", underlying],
            &fixture,
            &root,
        );
        assert_eq!(
            fs::read(root.join("CURRENT")).unwrap(),
            prior_current,
            "case {name}"
        );
        assert_eq!(
            fs::read(current_generation(&root).join("index-manifest.json")).unwrap(),
            prior_manifest,
            "case {name}"
        );
    }
}

#[test]
fn every_index_open_corruption_is_redacted_and_exits_one() {
    let _serial = serial_fixture();
    let cases: &[IndexOpenCorruptionCase] = &[
        ("current-traversal", "index.current_invalid", |root| {
            fs::write(root.join("CURRENT"), b"../slot-a\n").unwrap();
        }),
        ("current-symlink", "index.current_invalid", |root| {
            use std::os::unix::fs::symlink;
            let target = root.join("private-current-target");
            fs::write(&target, b"slot-a\n").unwrap();
            fs::remove_file(root.join("CURRENT")).unwrap();
            symlink(target, root.join("CURRENT")).unwrap();
        }),
        ("manifest", "index.manifest_invalid", |root| {
            fs::write(
                current_generation(root).join("index-manifest.json"),
                format!("not-json {SECRET_QUERY} /private/tmp/hidden\n"),
            )
            .unwrap();
        }),
        ("manifest-unknown", "index.manifest_invalid", |root| {
            let path = current_generation(root).join("index-manifest.json");
            let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            manifest["private_unknown_sentinel"] = json!(true);
            write_pretty_json(&path, &manifest);
        }),
        ("bundle", "index.bundle_mismatch", |root| {
            mutate_manifest(root, "bundle_hash", json!("0".repeat(64)));
        }),
        ("config", "index.schema_mismatch", |root| {
            mutate_manifest(root, "config_hash", json!("0".repeat(64)));
        }),
        ("version", "index.schema_mismatch", |root| {
            mutate_manifest(root, "sovmem_version", json!("private-version-sentinel"));
        }),
        ("schema", "index.schema_mismatch", |root| {
            mutate_manifest(root, "ranking", json!("wrong-ranking"));
        }),
        ("documents", "index.document_set_invalid", |root| {
            mutate_manifest(root, "documents", json!(61));
        }),
        ("graph", "index.graph_invalid", |root| {
            mutate_manifest(root, "graph_edges", json!(20));
        }),
        ("graph-duplicate", "index.graph_invalid", |root| {
            mutate_graph(root, |graph| {
                let duplicate = graph["edges"][0].clone();
                graph["edges"].as_array_mut().unwrap().insert(0, duplicate);
            });
        }),
        ("graph-unbundled-edge", "index.graph_invalid", |root| {
            mutate_graph(root, |graph| {
                let nodes = graph["nodes"].as_array().unwrap();
                let source = nodes.last().unwrap()["id"].clone();
                let target = nodes.first().unwrap()["id"].clone();
                graph["edges"].as_array_mut().unwrap().push(json!({
                    "source": source,
                    "relation": "references",
                    "target": target,
                }));
            });
        }),
        ("tantivy", "index.tantivy_invalid", |root| {
            fs::remove_file(current_generation(root).join("tantivy/meta.json")).unwrap();
        }),
    ];
    for (name, code, corrupt) in cases {
        let fixture = P2Fixture::new_with_query_cases(&format!("cli-open-{name}"));
        let root = fixture.index_root("index");
        assert!(run_index(&fixture, &root, Some("json")).status.success());
        corrupt(&root);
        let output = run_query(
            &fixture,
            &root,
            SECRET_QUERY,
            None,
            Some("none"),
            Some("json"),
        );
        assert_redacted_failure(&output, 1, code, &fixture, &root);
    }
}

#[test]
fn p2_help_exposes_only_the_frozen_runtime_arguments() {
    for command in ["index", "query", "eval"] {
        let output = sovmem()
            .arg(command)
            .arg("--help")
            .output()
            .expect("run P2 help");
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(output.stderr.is_empty());
        let help = stdout(&output);
        assert!(help.contains(&format!("Usage: sovmem {command}")));
        for forbidden in [
            "--boost",
            "--tokenizer",
            "--field",
            "--incremental",
            "--network",
            "--model",
        ] {
            assert!(!help.contains(forbidden));
        }
    }

    let eval_help = stdout(
        &sovmem()
            .arg("eval")
            .arg("--help")
            .output()
            .expect("run eval help"),
    );
    for required in [
        "--profile",
        "--bundle",
        "--index",
        "--golden",
        "--thresholds",
        "--baseline",
        "--format",
    ] {
        assert!(eval_help.contains(required));
    }
    for forbidden in [
        "--top-k",
        "--expand",
        "--query",
        "--label",
        "--threshold",
        "--relevance",
    ] {
        assert!(!eval_help
            .split_ascii_whitespace()
            .any(|argument| argument == forbidden));
    }
}

#[test]
fn top_level_help_and_version_keep_success_output() {
    for argument in ["--help", "--version"] {
        let output = sovmem()
            .arg(argument)
            .output()
            .expect("run top-level informational command");
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.is_empty());
    }
}

#[test]
fn every_p2_parse_failure_is_one_static_redacted_jsonl_diagnostic() {
    const PROFILE: &str = "/private/tmp/private-profile-sentinel";
    const BUNDLE: &str = "/private/tmp/private-bundle-sentinel";
    const INDEX: &str = "/private/tmp/private-index-sentinel";
    const QUERY: &str = "private-query-value-sentinel";
    const GOLDEN: &str = "/private/tmp/private-golden-sentinel";
    const THRESHOLDS: &str = "/private/tmp/private-thresholds-sentinel";
    const BASELINE: &str = "/private/tmp/private-baseline-sentinel";

    let cases = vec![
        (vec!["index", "--profile", PROFILE], vec![PROFILE]),
        (vec!["query", "--profile", PROFILE], vec![PROFILE]),
        (vec!["eval", "--profile", PROFILE], vec![PROFILE]),
        (
            vec![
                "index",
                "--profile",
                PROFILE,
                "--bundle",
                BUNDLE,
                "--index",
                INDEX,
                "--format",
                "xml",
            ],
            vec![PROFILE, BUNDLE, INDEX, "xml"],
        ),
        (
            vec![
                "query",
                "--profile",
                PROFILE,
                "--bundle",
                BUNDLE,
                "--index",
                INDEX,
                "--query",
                QUERY,
                "--expand",
                "private-expand-sentinel",
            ],
            vec![PROFILE, BUNDLE, INDEX, QUERY, "private-expand-sentinel"],
        ),
        (
            vec![
                "index",
                "--profile",
                PROFILE,
                "--bundle",
                BUNDLE,
                "--index",
                INDEX,
                "--private-unknown-sentinel",
                "private-unknown-value-sentinel",
            ],
            vec![
                PROFILE,
                BUNDLE,
                INDEX,
                "private-unknown-sentinel",
                "private-unknown-value-sentinel",
            ],
        ),
        (
            vec![
                "query",
                "--profile",
                PROFILE,
                "--bundle",
                BUNDLE,
                "--index",
                INDEX,
                "--query",
                "-private-query-sentinel",
            ],
            vec![PROFILE, BUNDLE, INDEX, "private-query-sentinel"],
        ),
        (
            vec![
                "eval",
                "--profile",
                PROFILE,
                "--bundle",
                BUNDLE,
                "--index",
                INDEX,
                "--golden",
                GOLDEN,
                "--thresholds",
                THRESHOLDS,
                "--baseline",
                BASELINE,
                "--top-k",
                "private-top-k-sentinel",
            ],
            vec![
                PROFILE,
                BUNDLE,
                INDEX,
                GOLDEN,
                THRESHOLDS,
                BASELINE,
                "private-top-k-sentinel",
            ],
        ),
    ];

    for (arguments, private_values) in cases {
        let output = sovmem()
            .args(arguments)
            .output()
            .expect("run invalid P2 command");
        assert_redacted_p2_usage(&output, &private_values);
    }
}
