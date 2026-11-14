//! Verification of the tracked P2 gate evidence.
//!
//! The published audit records BLAKE3 digests and retrieval counts for
//! artifacts that are committed to this repository. Those recorded values were
//! previously only ever compared against hardcoded literals, so a corrupted or
//! hand-edited artifact could keep every check green.
//!
//! These tests recompute the digests from the tracked bytes and re-derive the
//! baseline's own arithmetic, so `docs/sovmem-build-state.json`,
//! `evals/audy-sovmem-v0/baseline.json`, `golden.jsonl`, and `thresholds.json`
//! cannot drift apart silently.
//!
//! Content-dependent evidence that has no in-repository source — the 226
//! verification warnings and the bundle hash of the private Audy Vault — is
//! deliberately out of scope here; it cannot be recomputed without the vault.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn repository_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative)
}

fn read_tracked(relative: &str) -> Vec<u8> {
    fs::read(repository_path(relative))
        .unwrap_or_else(|error| panic!("read tracked {relative}: {error}"))
}

fn read_tracked_json(relative: &str) -> Value {
    serde_json::from_slice(&read_tracked(relative))
        .unwrap_or_else(|error| panic!("parse tracked {relative}: {error}"))
}

fn blake3_of(relative: &str) -> String {
    blake3::hash(&read_tracked(relative)).to_hex().to_string()
}

fn usize_at(value: &Value, pointer: &str) -> usize {
    let found = value
        .pointer(pointer)
        .unwrap_or_else(|| panic!("missing {pointer}"));
    usize::try_from(
        found
            .as_u64()
            .unwrap_or_else(|| panic!("{pointer} must be a non-negative integer")),
    )
    .expect("value fits in usize")
}

fn str_at<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing string {pointer}"))
}

const BUILD_STATE: &str = "docs/sovmem-build-state.json";
const BASELINE: &str = "evals/audy-sovmem-v0/baseline.json";
const GOLDEN: &str = "evals/audy-sovmem-v0/golden.jsonl";
const THRESHOLDS: &str = "evals/audy-sovmem-v0/thresholds.json";

#[test]
fn recorded_blake3_digests_match_the_tracked_artifact_bytes() {
    let build_state = read_tracked_json(BUILD_STATE);
    let baseline = read_tracked_json(BASELINE);
    let thresholds = read_tracked_json(THRESHOLDS);

    let baseline_digest = blake3_of(BASELINE);
    let golden_digest = blake3_of(GOLDEN);
    let thresholds_digest = blake3_of(THRESHOLDS);

    // Every place that restates a digest must agree with the actual bytes.
    assert_eq!(
        str_at(&build_state, "/evidence/retrieval/baseline_blake3"),
        baseline_digest,
        "build state must record the real baseline digest"
    );
    assert_eq!(
        str_at(&build_state, "/evidence/golden/blake3"),
        golden_digest,
        "build state must record the real golden digest"
    );
    assert_eq!(
        str_at(&baseline, "/evaluation_set/golden_blake3"),
        golden_digest,
        "baseline must bind the real golden digest"
    );
    assert_eq!(
        str_at(&baseline, "/evaluation_set/thresholds_blake3"),
        thresholds_digest,
        "baseline must bind the real thresholds digest"
    );
    assert_eq!(
        str_at(&thresholds, "/evaluation_set/golden_blake3"),
        golden_digest,
        "thresholds must bind the real golden digest"
    );
}

#[test]
fn tracked_baseline_arithmetic_is_internally_consistent() {
    let baseline = read_tracked_json(BASELINE);
    let records = baseline
        .pointer("/queries")
        .and_then(Value::as_array)
        .expect("baseline queries array");

    let query_count = usize_at(&baseline, "/evaluation_set/query_count");
    let k = usize_at(&baseline, "/evaluation_set/k");
    let declared_hits = usize_at(&baseline, "/metrics/queries_with_hit");
    let declared_misses = usize_at(&baseline, "/metrics/queries_without_hit");
    let minimum = usize_at(&baseline, "/metrics/gate/minimum_queries_with_hit");

    assert_eq!(records.len(), query_count, "one record per frozen query");

    // Re-derive the hit count from the per-query evidence rather than trusting
    // the summary, and prove each recorded rank actually locates the relevant
    // document at that one-based position.
    let mut recomputed_hits = 0;
    for record in records {
        let relevant = record
            .pointer("/relevant_ids")
            .and_then(Value::as_array)
            .expect("relevant_ids");
        assert_eq!(
            relevant.len(),
            1,
            "frozen set has one relevant id per query"
        );
        let relevant_id = relevant[0].as_str().expect("relevant id string");

        let results = record
            .pointer("/result_ids")
            .and_then(Value::as_array)
            .expect("result_ids");
        assert!(
            results.len() <= k,
            "a query may not report more than k results"
        );

        let hit_rank = record.pointer("/hit_rank").expect("hit_rank present");
        match hit_rank.as_u64() {
            Some(rank) => {
                let rank = usize::try_from(rank).expect("rank fits in usize");
                assert!((1..=results.len()).contains(&rank), "rank is one-based");
                assert_eq!(
                    results[rank - 1].as_str(),
                    Some(relevant_id),
                    "hit_rank must locate the relevant document"
                );
                recomputed_hits += 1;
            }
            None => {
                assert!(hit_rank.is_null(), "hit_rank is a rank or null");
                assert!(
                    !results.iter().any(|id| id.as_str() == Some(relevant_id)),
                    "a null hit_rank must mean the relevant document is absent"
                );
            }
        }
    }

    assert_eq!(
        recomputed_hits, declared_hits,
        "queries_with_hit must be measured"
    );
    assert_eq!(
        declared_misses,
        query_count - recomputed_hits,
        "misses must complete the query set"
    );

    // Exact rational metric components, then the integer gate. No float
    // comparison is involved in deciding GO.
    assert_eq!(
        usize_at(&baseline, "/metrics/precision_at_5/numerator"),
        recomputed_hits
    );
    assert_eq!(
        usize_at(&baseline, "/metrics/precision_at_5/denominator"),
        query_count * k
    );
    assert_eq!(
        usize_at(&baseline, "/metrics/recall_at_5/numerator"),
        recomputed_hits
    );
    assert_eq!(
        usize_at(&baseline, "/metrics/recall_at_5/denominator"),
        query_count
    );
    assert_eq!(
        str_at(&baseline, "/metrics/precision_at_5/decimal"),
        format!("{:.4}", recomputed_hits as f64 / (query_count * k) as f64)
    );
    assert_eq!(
        str_at(&baseline, "/metrics/recall_at_5/decimal"),
        format!("{:.4}", recomputed_hits as f64 / query_count as f64)
    );

    let expected_status = if recomputed_hits >= minimum {
        "go"
    } else {
        "no-go"
    };
    assert_eq!(
        str_at(&baseline, "/metrics/gate/status"),
        expected_status,
        "gate status must follow from the integer hit count"
    );
    assert_eq!(
        baseline
            .pointer("/corpus/core")
            .and_then(Value::as_u64)
            .unwrap()
            + baseline
                .pointer("/corpus/research")
                .and_then(Value::as_u64)
                .unwrap(),
        baseline
            .pointer("/corpus/documents")
            .and_then(Value::as_u64)
            .unwrap(),
        "core + research must account for every document"
    );
}

#[test]
fn build_state_retrieval_evidence_matches_the_tracked_baseline() {
    let build_state = read_tracked_json(BUILD_STATE);
    let baseline = read_tracked_json(BASELINE);
    let thresholds = read_tracked_json(THRESHOLDS);

    // The published audit must restate the baseline, not a parallel set of
    // numbers that happens to have been correct when it was written.
    for (state_pointer, baseline_pointer) in [
        ("/evidence/retrieval/documents", "/index/documents"),
        ("/evidence/retrieval/graph_nodes", "/index/graph_nodes"),
        ("/evidence/retrieval/graph_edges", "/index/graph_edges"),
        (
            "/evidence/retrieval/external_typed_links",
            "/index/external_typed_links",
        ),
        ("/evidence/retrieval/queries", "/evaluation_set/query_count"),
        ("/evidence/retrieval/k", "/evaluation_set/k"),
        (
            "/evidence/retrieval/queries_with_hit",
            "/metrics/queries_with_hit",
        ),
        (
            "/evidence/retrieval/minimum_queries_with_hit",
            "/metrics/gate/minimum_queries_with_hit",
        ),
        ("/evidence/corpus/accepted", "/corpus/documents"),
        ("/evidence/corpus/core", "/corpus/core"),
        ("/evidence/corpus/research", "/corpus/research"),
    ] {
        assert_eq!(
            usize_at(&build_state, state_pointer),
            usize_at(&baseline, baseline_pointer),
            "{state_pointer} must equal baseline {baseline_pointer}"
        );
    }

    for (state_pointer, baseline_pointer) in [
        (
            "/evidence/retrieval/index_config",
            "/retrieval/index_config",
        ),
        ("/evidence/retrieval/bundle_hash", "/corpus/bundle_hash"),
        ("/evidence/bundle_hash", "/corpus/bundle_hash"),
    ] {
        assert_eq!(
            str_at(&build_state, state_pointer),
            str_at(&baseline, baseline_pointer),
            "{state_pointer} must equal baseline {baseline_pointer}"
        );
    }

    // The frozen gate the audit advertises must be the gate the baseline used.
    assert_eq!(
        usize_at(&build_state, "/evidence/thresholds/k"),
        usize_at(&thresholds, "/retrieval/k")
    );
    assert_eq!(
        usize_at(
            &build_state,
            "/evidence/thresholds/minimum_queries_with_hit"
        ),
        usize_at(&thresholds, "/retrieval/minimum_queries_with_hit")
    );
    assert_eq!(
        usize_at(&baseline, "/metrics/gate/minimum_queries_with_hit"),
        usize_at(&thresholds, "/retrieval/minimum_queries_with_hit")
    );
    assert_eq!(
        usize_at(&build_state, "/evidence/golden/queries"),
        usize_at(&thresholds, "/evaluation_set/query_count")
    );
}
