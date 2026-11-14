#[allow(dead_code)]
mod support;

use serde_json::{json, Value};
use sovmem::profile::{
    evaluate, evaluate_hits_for_test, EvalError, EvalRequest, EvaluationBaseline, Q3Decision,
};
use std::fs;
use std::sync::{Mutex, MutexGuard, OnceLock};
use support::p2::P2Fixture;

const GOLDEN_BLAKE3: &str = "f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175";
const THRESHOLDS_BLAKE3: &str = "979566e38e59f3dc4d144c8112e8280d78fa741f63f82cc292b48d4be479d021";

fn request(
    fixture: &P2Fixture,
    index_root: std::path::PathBuf,
    baseline_path: std::path::PathBuf,
) -> EvalRequest {
    EvalRequest {
        profile_path: fixture.profile_path.clone(),
        bundle_root: fixture.bundle_root.clone(),
        index_root,
        golden_path: fixture.golden_path.clone(),
        thresholds_path: fixture.thresholds_path.clone(),
        baseline_path,
    }
}

fn assert_no_private_baseline_fields(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                assert!(
                    !matches!(
                        key.as_str(),
                        "timestamp" | "path" | "score" | "query" | "rationale" | "title" | "body"
                    ),
                    "forbidden baseline key: {key}"
                );
                assert_no_private_baseline_fields(value);
            }
        }
        Value::Array(values) => values.iter().for_each(assert_no_private_baseline_fields),
        _ => {}
    }
}

fn serial_fixture() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn twenty_hits_is_go_and_nineteen_is_no_go_without_float_comparison() {
    let go = evaluate_hits_for_test(20);
    assert_eq!(go.decision, Q3Decision::Go);
    assert_eq!(go.precision_numerator, 20);
    assert_eq!(go.precision_denominator, 125);
    assert_eq!(go.recall_numerator, 20);
    assert_eq!(go.recall_denominator, 25);
    assert_eq!(go.precision_decimal, "0.1600");
    assert_eq!(go.recall_decimal, "0.8000");
    assert_eq!(evaluate_hits_for_test(19).decision, Q3Decision::NoGo);
}

#[test]
fn two_evaluations_write_identical_strict_provenance_bound_baseline_bytes() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_retrievable("eval-determinism", 25);
    let root = fixture.index_root("index");
    fixture.build_index(&root);
    let first = fixture.evaluate(&root, fixture.index_root("baseline-a.json"));
    let second = fixture.evaluate(&root, fixture.index_root("baseline-b.json"));
    let first_bytes = fs::read(&first.baseline).expect("read first baseline");
    let second_bytes = fs::read(&second.baseline).expect("read second baseline");
    assert_eq!(first_bytes, second_bytes);
    assert!(first_bytes.ends_with(b"\n"));
    assert!(!first_bytes.ends_with(b"\n\n"));

    let baseline: EvaluationBaseline =
        serde_json::from_slice(&first_bytes).expect("parse strict baseline");
    assert_eq!(baseline.schema_version, "sovmem-retrieval-baseline-v1");
    assert_eq!(baseline.evaluation_set.golden_blake3, GOLDEN_BLAKE3);
    assert_eq!(baseline.evaluation_set.thresholds_blake3, THRESHOLDS_BLAKE3);
    assert_eq!(baseline.evaluation_set.query_count, 25);
    assert_eq!(baseline.evaluation_set.k, 5);
    assert_eq!(baseline.queries.len(), 25);
    assert_eq!(baseline.metrics.gate.status, Q3Decision::Go);
    assert_eq!(first.decision, Q3Decision::Go);
    assert!(first.baseline_written);

    let golden_ids = fs::read_to_string(&fixture.golden_path)
        .expect("read golden order")
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        baseline
            .queries
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>(),
        golden_ids
    );
    for record in &baseline.queries {
        assert!(record.result_ids.len() <= 5);
        assert_eq!(record.relevant_ids.len(), 1);
        let rank = record.hit_rank.expect("retrievable fixture must hit");
        assert!((1..=5).contains(&rank));
        assert_eq!(record.result_ids[rank - 1], record.relevant_ids[0]);
    }

    let value: Value = serde_json::from_slice(&first_bytes).unwrap();
    assert_no_private_baseline_fields(&value);
    let mut unknown = value;
    unknown["unexpected"] = Value::Bool(true);
    assert!(serde_json::from_value::<EvaluationBaseline>(unknown).is_err());
}

#[test]
fn invalid_index_preserves_the_prior_baseline() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new("eval-invalid-index");
    let baseline = fixture.index_root("baseline.json");
    let prior = b"prior reviewed baseline\n";
    fs::write(&baseline, prior).expect("write prior baseline");

    let error = evaluate(request(
        &fixture,
        fixture.index_root("missing-index"),
        baseline.clone(),
    ))
    .expect_err("missing current index must fail");
    assert!(matches!(error, EvalError::Index(_)));
    assert_eq!(error.code(), "index.current_invalid");
    assert_eq!(fs::read(baseline).unwrap(), prior);
}

#[test]
fn valid_no_go_writes_complete_evidence_with_one_based_and_null_hit_ranks() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_retrievable("eval-no-go", 19);
    let root = fixture.index_root("index");
    fixture.build_index(&root);
    let baseline_path = fixture.index_root("baseline.json");
    fs::write(&baseline_path, b"prior baseline\n").expect("write prior baseline");

    let outcome = fixture.evaluate(&root, baseline_path.clone());
    assert_eq!(outcome.decision, Q3Decision::NoGo);
    assert_eq!(outcome.queries_with_hit, 19);
    assert!(outcome.baseline_written);

    let bytes = fs::read(&baseline_path).expect("read NO-GO baseline");
    assert_ne!(bytes, b"prior baseline\n");
    let baseline: EvaluationBaseline = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(baseline.metrics.gate.status, Q3Decision::NoGo);
    assert_eq!(baseline.metrics.queries_with_hit, 19);
    assert_eq!(baseline.metrics.queries_without_hit, 6);
    assert_eq!(
        baseline
            .queries
            .iter()
            .filter(|record| record.hit_rank.is_some())
            .count(),
        19
    );
    for record in &baseline.queries {
        assert!(record.result_ids.len() <= 5);
        match record.hit_rank {
            Some(rank) => {
                assert!((1..=5).contains(&rank));
                assert_eq!(record.result_ids[rank - 1], record.relevant_ids[0]);
            }
            None => assert!(!record.result_ids.contains(&record.relevant_ids[0])),
        }
    }
}

#[test]
fn changed_frozen_inputs_return_exact_codes_and_preserve_prior_baseline() {
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
            "threshold-schema",
            |fixture| {
                mutate_threshold(fixture, |value| {
                    value["schema_version"] = json!("private-schema-sentinel")
                })
            },
            "thresholds.schema",
        ),
        (
            "threshold-evaluation-set",
            |fixture| {
                mutate_threshold(fixture, |value| {
                    value["evaluation_set"]["query_count"] = json!(24)
                })
            },
            "thresholds.evaluation_set",
        ),
        (
            "threshold-retrieval",
            |fixture| {
                mutate_threshold(fixture, |value| {
                    value["retrieval"]["minimum_queries_with_hit"] = json!(19)
                })
            },
            "thresholds.retrieval",
        ),
        (
            "threshold-approval",
            |fixture| {
                mutate_threshold(fixture, |value| {
                    value["approval"]["human_approved"] = json!(false)
                })
            },
            "thresholds.approval",
        ),
    ];

    for (name, mutate, expected) in cases {
        let _serial = serial_fixture();
        let fixture = P2Fixture::new_retrievable(&format!("eval-frozen-{name}"), 25);
        let root = fixture.index_root("index");
        fixture.build_index(&root);
        let baseline = fixture.index_root("baseline.json");
        let prior = b"prior-reviewed-baseline\n";
        fs::write(&baseline, prior).expect("write prior baseline");
        mutate(&fixture);

        let error = evaluate(request(&fixture, root, baseline.clone()))
            .expect_err("changed frozen input must fail evaluation");
        assert_eq!(error.code(), *expected, "case {name}");
        assert_eq!(fs::read(&baseline).unwrap(), prior, "case {name}");
    }
}

fn mutate_threshold(fixture: &P2Fixture, mutate: impl FnOnce(&mut Value)) {
    let mut value: Value =
        serde_json::from_slice(&fs::read(&fixture.thresholds_path).expect("read thresholds"))
            .expect("parse thresholds");
    mutate(&mut value);
    let mut bytes = serde_json::to_vec_pretty(&value).expect("serialize thresholds");
    bytes.push(b'\n');
    fs::write(&fixture.thresholds_path, bytes).expect("write thresholds");
}

#[test]
fn baseline_reports_the_measured_corpus_split_not_a_compiled_constant() {
    let _serial = serial_fixture();
    let fixture = P2Fixture::new_frozen_eval("eval-measured-corpus-split");
    let root = fixture.index_root("index");
    fixture.build_index(&root);
    let baseline_path = fixture.index_root("baseline.json");

    fixture.evaluate(&root, baseline_path.clone());

    let bytes = fs::read(&baseline_path).expect("read baseline");
    let baseline: EvaluationBaseline = serde_json::from_slice(&bytes).expect("parse baseline");

    // The split must be derived from the verified corpus that was actually
    // opened, and must account for every indexed document. A compiled-in
    // constant would satisfy the first assertion by coincidence but could not
    // survive the accounting identity if composition ever drifted.
    let verified =
        sovmem::profile::prepare_verified_corpus(&fixture.profile_path, &fixture.bundle_root)
            .expect("prepare verified corpus");

    assert_eq!(baseline.corpus.core, verified.core);
    assert_eq!(baseline.corpus.research, verified.research);
    assert_eq!(baseline.corpus.documents, verified.documents);
    assert_eq!(
        baseline.corpus.core + baseline.corpus.research,
        baseline.corpus.documents,
        "core + research must account for every verified document"
    );
}

// ---------------------------------------------------------------------------
// F06 — Regression detection gate (uses evaluate_hits_for_test, no fixture)
// ---------------------------------------------------------------------------

#[test]
fn evaluate_hits_detects_regression_degradation() {
    // The evaluate_hits_for_test function directly tests the Q3 scoring gate
    // without needing a P2Fixture (which is a process-wide singleton).
    // A drop from 20 hits (GO minimum) to 19 (below threshold) proves the
    // gate can detect retrieval regression.
    let go = evaluate_hits_for_test(20);
    assert_eq!(
        go.decision,
        Q3Decision::Go,
        "20/25 hits must pass Q3 gate (= regression not detected)"
    );
    assert_eq!(go.precision_numerator, 20);
    assert_eq!(go.recall_numerator, 20);

    let regressed = evaluate_hits_for_test(19);
    assert_eq!(
        regressed.decision,
        Q3Decision::NoGo,
        "19/25 hits must fail Q3 gate (= regression detected)"
    );
    assert_eq!(regressed.precision_numerator, 19);
    assert_eq!(regressed.recall_numerator, 19);
}
