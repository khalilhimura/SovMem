use serde_json::{json, Value};
use sovmem::profile::load_retrieval_thresholds;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn repository_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative)
}

struct TemporaryThresholds {
    root: PathBuf,
    thresholds: PathBuf,
    golden: PathBuf,
}

impl TemporaryThresholds {
    fn new(name: &str) -> Self {
        let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-thresholds-{name}-{}-{sequence}",
            std::process::id(),
        ));
        fs::create_dir(&root).expect("create threshold area");
        let thresholds = root.join("thresholds.json");
        let golden = root.join("golden.jsonl");
        fs::copy(
            repository_path("evals/audy-sovmem-v0/thresholds.json"),
            &thresholds,
        )
        .expect("copy thresholds");
        fs::copy(
            repository_path("evals/audy-sovmem-v0/golden.jsonl"),
            &golden,
        )
        .expect("copy golden");
        Self {
            root,
            thresholds,
            golden,
        }
    }

    fn thresholds_value(&self) -> Value {
        serde_json::from_slice(&fs::read(&self.thresholds).expect("read thresholds"))
            .expect("parse thresholds")
    }

    fn write_thresholds(&self, value: &Value) {
        let mut bytes = serde_json::to_vec_pretty(value).expect("serialize thresholds");
        bytes.push(b'\n');
        fs::write(&self.thresholds, bytes).expect("write thresholds");
    }

    fn golden_values(&self) -> Vec<Value> {
        fs::read_to_string(&self.golden)
            .expect("read golden")
            .lines()
            .map(|line| serde_json::from_str(line).expect("parse golden line"))
            .collect()
    }

    fn write_golden_values(&self, values: &[Value]) {
        let mut bytes = Vec::new();
        for value in values {
            bytes.extend(serde_json::to_vec(value).expect("serialize golden line"));
            bytes.push(b'\n');
        }
        fs::write(&self.golden, bytes).expect("write golden");
    }

    fn error_code(&self) -> &'static str {
        load_retrieval_thresholds(&self.thresholds, &self.golden)
            .expect_err("fixture must fail")
            .code()
    }
}

impl Drop for TemporaryThresholds {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn loads_the_human_approved_q3_freeze() {
    let thresholds = load_retrieval_thresholds(
        &repository_path("evals/audy-sovmem-v0/thresholds.json"),
        &repository_path("evals/audy-sovmem-v0/golden.jsonl"),
    )
    .expect("approved thresholds load");

    assert_eq!(thresholds.evaluation_set.id, "audy-sovmem-v0");
    assert_eq!(thresholds.evaluation_set.query_count, 25);
    assert_eq!(thresholds.evaluation_set.relevant_documents_per_query, 1);
    assert_eq!(thresholds.retrieval.k, 5);
    assert_eq!(thresholds.retrieval.macro_precision_at_k_minimum, "0.16");
    assert_eq!(thresholds.retrieval.macro_recall_at_k_minimum, "0.80");
    assert_eq!(thresholds.minimum_queries_with_hit(), 20);
    assert_eq!(
        thresholds.golden_blake3(),
        "f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175"
    );
    assert_eq!(thresholds.approval.freeze_date, "2026-07-12");
}

#[test]
fn rejects_changed_threshold_contract_fields_with_stable_codes() {
    type Mutation = fn(&mut Value);
    let cases: &[(&str, Mutation, &str)] = &[
        (
            "schema",
            |value| value["schema_version"] = json!("foreign-v1"),
            "thresholds.schema",
        ),
        (
            "identity",
            |value| value["evaluation_set"]["id"] = json!("foreign"),
            "thresholds.evaluation_set",
        ),
        (
            "count",
            |value| value["evaluation_set"]["query_count"] = json!(24),
            "thresholds.evaluation_set",
        ),
        (
            "relevance-count",
            |value| value["evaluation_set"]["relevant_documents_per_query"] = json!(2),
            "thresholds.evaluation_set",
        ),
        (
            "k",
            |value| value["retrieval"]["k"] = json!(10),
            "thresholds.retrieval",
        ),
        (
            "precision",
            |value| value["retrieval"]["macro_precision_at_k_minimum"] = json!("0.15"),
            "thresholds.retrieval",
        ),
        (
            "recall",
            |value| value["retrieval"]["macro_recall_at_k_minimum"] = json!("0.79"),
            "thresholds.retrieval",
        ),
        (
            "hits",
            |value| value["retrieval"]["minimum_queries_with_hit"] = json!(19),
            "thresholds.retrieval",
        ),
        (
            "approval",
            |value| value["approval"]["human_approved"] = json!(false),
            "thresholds.approval",
        ),
    ];

    for (name, mutate, expected) in cases {
        let fixture = TemporaryThresholds::new(name);
        let mut value = fixture.thresholds_value();
        mutate(&mut value);
        fixture.write_thresholds(&value);
        assert_eq!(fixture.error_code(), *expected, "case {name}");
    }
}

#[test]
fn rejects_unknown_threshold_fields_and_changed_golden_bytes() {
    let unknown = TemporaryThresholds::new("unknown-field");
    let mut value = unknown.thresholds_value();
    value["unexpected"] = json!(true);
    unknown.write_thresholds(&value);
    assert_eq!(unknown.error_code(), "thresholds.parse");

    let changed = TemporaryThresholds::new("changed-golden");
    let mut queries = changed.golden_values();
    queries[0]["query"] = json!("A different but structurally valid human query");
    changed.write_golden_values(&queries);
    assert_eq!(changed.error_code(), "thresholds.golden_hash");
}

#[test]
fn rejects_nonhuman_or_multi_relevance_golden_contracts_before_hash_acceptance() {
    let nonhuman = TemporaryThresholds::new("nonhuman");
    let mut queries = nonhuman.golden_values();
    queries[0]["authorship"] = json!("model");
    nonhuman.write_golden_values(&queries);
    assert_eq!(nonhuman.error_code(), "thresholds.golden_contract");

    let multiple = TemporaryThresholds::new("multiple-relevant");
    let mut queries = multiple.golden_values();
    let relevant = queries[0]["relevant_ids"][0].clone();
    queries[0]["relevant_ids"] = json!([relevant, "ssa-0/04-Research/Missing"]);
    multiple.write_golden_values(&queries);
    assert_eq!(multiple.error_code(), "thresholds.golden_contract");
}
