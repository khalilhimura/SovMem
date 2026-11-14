#[allow(dead_code)]
mod support;

use serde_json::Value;
use sovmem::profile::{
    build_index, query_index, ExpansionMode, GeneratedDocument, IndexRequest, QueryRequest,
};
use std::fs;
use std::path::{Path, PathBuf};
use support::p2::P2Fixture;

fn build_fixture_index(fixture: &P2Fixture) -> PathBuf {
    let index_root = fixture.index_root("index");
    build_index(IndexRequest {
        profile_path: fixture.profile_path.clone(),
        bundle_root: fixture.bundle_root.clone(),
        index_root: index_root.clone(),
    })
    .expect("build fixture index");
    index_root
}

fn query(
    fixture: &P2Fixture,
    index_root: &Path,
    text: &str,
    top_k: usize,
) -> sovmem::profile::QueryResult {
    query_with_expansion(fixture, index_root, text, top_k, ExpansionMode::None)
}

fn query_with_expansion(
    fixture: &P2Fixture,
    index_root: &Path,
    text: &str,
    top_k: usize,
    expansion: ExpansionMode,
) -> sovmem::profile::QueryResult {
    query_index(QueryRequest {
        profile_path: fixture.profile_path.clone(),
        bundle_root: fixture.bundle_root.clone(),
        index_root: index_root.to_owned(),
        query: text.into(),
        top_k,
        expansion,
    })
    .expect("query fixture index")
}

fn matching_id(
    documents: &[GeneratedDocument],
    predicate: impl Fn(&GeneratedDocument) -> bool,
) -> String {
    documents
        .iter()
        .find(|document| predicate(document))
        .expect("matching fixture document")
        .metadata
        .id
        .clone()
}

fn current_generation(index_root: &Path) -> PathBuf {
    let slot = fs::read_to_string(index_root.join("CURRENT"))
        .expect("read CURRENT")
        .trim()
        .to_owned();
    index_root.join(slot)
}

fn write_pretty_json(path: &Path, value: &Value) {
    let mut bytes = serde_json::to_vec_pretty(value).expect("serialize mutated JSON");
    bytes.push(b'\n');
    fs::write(path, bytes).expect("write mutated JSON");
}

fn replace_manifest_field(index_root: &Path, key: &str, value: Value) {
    let path = current_generation(index_root).join("index-manifest.json");
    let bytes = fs::read_to_string(&path).expect("read index manifest");
    let manifest: Value = serde_json::from_str(&bytes).expect("parse index manifest");
    let old_value = serde_json::to_string(&manifest[key]).expect("serialize old value");
    let new_value = serde_json::to_string(&value).expect("serialize new value");
    let old_field = format!("  \"{key}\": {old_value}");
    let new_field = format!("  \"{key}\": {new_value}");
    assert_eq!(bytes.matches(&old_field).count(), 1);
    fs::write(path, bytes.replacen(&old_field, &new_field, 1))
        .expect("write mutated index manifest");
}

fn query_error_code(fixture: &P2Fixture, index_root: &Path) -> &'static str {
    query_index(QueryRequest {
        profile_path: fixture.profile_path.clone(),
        bundle_root: fixture.bundle_root.clone(),
        index_root: index_root.to_owned(),
        query: "private-query-sentinel".into(),
        top_k: 5,
        expansion: ExpansionMode::OneHop,
    })
    .expect_err("corrupt index must block query")
    .code()
}

#[test]
fn tokenizes_case_punctuation_unicode_and_crisp_squared_deterministically() {
    assert_eq!(
        sovmem::profile::tokenize_query("CRISP², Agent-Work!"),
        vec!["crisp²", "agent", "work"]
    );
    assert_eq!(
        sovmem::profile::tokenize_query("Repeat repeat"),
        vec!["repeat", "repeat"]
    );
}

#[test]
fn rejects_blank_control_oversized_and_over_token_queries_without_echoing() {
    let cases = vec![
        ("   ".to_owned(), "query.empty"),
        ("private\u{0007}query".to_owned(), "query.invalid"),
        ("x".repeat(4097), "query.invalid"),
        (
            (0..65)
                .map(|number| format!("t{number}"))
                .collect::<Vec<_>>()
                .join(" "),
            "query.too_many_tokens",
        ),
        ("!!!".to_owned(), "query.empty"),
    ];
    for (text, code) in cases {
        let error = sovmem::profile::validate_query(&text, 5).unwrap_err();
        assert_eq!(error.code(), code);
        assert!(!error.to_string().contains(&text));
    }
    assert_eq!(
        sovmem::profile::validate_query("valid", 0)
            .unwrap_err()
            .code(),
        "query.invalid"
    );
    assert_eq!(
        sovmem::profile::validate_query("valid", 63)
            .unwrap_err()
            .code(),
        "query.invalid"
    );
    assert_eq!(
        sovmem::profile::validate_query(
            &(0..64)
                .map(|number| format!("t{number}"))
                .collect::<Vec<_>>()
                .join(" "),
            62,
        )
        .unwrap()
        .len(),
        64
    );
}

#[test]
fn title_tag_and_body_matches_follow_the_fixed_structural_boost_order() {
    let fixture = P2Fixture::new_with_query_cases("query-field-boosts");
    let documents = fixture.generated_documents();
    let title_id = matching_id(&documents, |document| {
        document.metadata.title.contains("fieldboostneedle")
    });
    let tag_id = matching_id(&documents, |document| {
        document
            .metadata
            .tags
            .iter()
            .any(|tag| tag == "fieldboostneedle")
    });
    let body_id = matching_id(&documents, |document| {
        document.body.contains("Fieldboostneedle body result")
    });
    let index_root = build_fixture_index(&fixture);

    let result = query(&fixture, &index_root, "fieldboostneedle", 5);
    assert_eq!(result.results.len(), 3);
    assert_eq!(
        result
            .results
            .iter()
            .map(|hit| hit.id.as_str())
            .collect::<Vec<_>>(),
        [title_id.as_str(), tag_id.as_str(), body_id.as_str()]
    );
    let scores = result
        .results
        .iter()
        .map(|hit| hit.score.parse::<f32>().expect("six-decimal score"))
        .collect::<Vec<_>>();
    assert!(scores[0] > scores[1]);
    assert!(scores[1] > scores[2]);
}

#[test]
fn equal_raw_bm25_scores_are_ordered_by_canonical_id_bytes() {
    let fixture = P2Fixture::new_with_query_cases("query-tie-order");
    let mut expected_ids = fixture
        .generated_documents()
        .into_iter()
        .filter(|document| document.body.contains("Equalrankneedle tie result"))
        .map(|document| document.metadata.id)
        .collect::<Vec<_>>();
    expected_ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    let index_root = build_fixture_index(&fixture);

    let result = query(&fixture, &index_root, "equalrankneedle", 5);
    assert_eq!(
        result
            .results
            .iter()
            .map(|hit| hit.id.as_str())
            .collect::<Vec<_>>(),
        expected_ids.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_eq!(result.results.len(), 2);
    assert_eq!(result.results[0].score, result.results[1].score);
}

#[test]
fn ranks_by_bm25_then_canonical_id_and_serializes_metadata_only() {
    let fixture = P2Fixture::new_with_query_cases("query-order");
    let index_root = build_fixture_index(&fixture);
    let result = query(&fixture, &index_root, "synthetic evidence", 5);

    assert!(result.results.windows(2).all(|pair| {
        let left = pair[0].score.parse::<f32>().unwrap();
        let right = pair[1].score.parse::<f32>().unwrap();
        left > right || (left == right && pair[0].id.as_bytes() < pair[1].id.as_bytes())
    }));
    assert_eq!(result.top_k, 5);
    assert_eq!(result.expansion, ExpansionMode::None);
    assert!(result.expanded.is_empty());
    let json = serde_json::to_string(&result).unwrap();
    assert!(!json.contains("body"));
    assert!(!json.contains("synthetic evidence"));
    assert!(!json.contains("source_path"));
    assert!(!json.contains(&index_root.to_string_lossy().to_string()));

    let value: Value = serde_json::from_str(&json).unwrap();
    let first = value["results"][0].as_object().unwrap();
    assert_eq!(
        first.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "id",
            "layer",
            "provenance",
            "rank",
            "score",
            "title",
            "type"
        ]
    );
}

#[test]
fn no_match_is_a_success_with_empty_result_arrays() {
    let fixture = P2Fixture::new_with_query_cases("query-no-match");
    let index_root = build_fixture_index(&fixture);
    let result = query(&fixture, &index_root, "termabsentfromtheentirefixture", 62);

    assert!(result.results.is_empty());
    assert!(result.expanded.is_empty());
    assert_eq!(result.top_k, 62);
}

#[test]
fn one_hop_expansion_is_separate_and_cannot_change_bm25_top_five() {
    let fixture = P2Fixture::new_with_query_cases("query-expansion");
    let documents = fixture.generated_documents();
    let index_root = build_fixture_index(&fixture);
    let base = query_with_expansion(
        &fixture,
        &index_root,
        "deterministic retrieval evidence",
        5,
        ExpansionMode::None,
    );
    let expanded = query_with_expansion(
        &fixture,
        &index_root,
        "deterministic retrieval evidence",
        5,
        ExpansionMode::OneHop,
    );

    assert_eq!(base.results, expanded.results);
    assert!(!expanded.expanded.is_empty());
    assert!(expanded
        .expanded
        .iter()
        .all(|hit| { !expanded.results.iter().any(|seed| seed.id == hit.id) }));
    let first = &expanded.expanded[0];
    assert_eq!(first.rank, 1);
    assert_eq!(first.via_id, expanded.results[0].id);
    let metadata = &documents
        .iter()
        .find(|document| document.metadata.id == first.id)
        .expect("expanded metadata fixture")
        .metadata;
    assert_eq!(first.title, metadata.title);
    assert_eq!(
        first.core_type,
        serde_json::to_value(&metadata.core_type)
            .unwrap()
            .as_str()
            .unwrap()
    );
    assert_eq!(
        first.layer,
        serde_json::to_value(&metadata.layer)
            .unwrap()
            .as_str()
            .unwrap()
    );
    assert_eq!(
        first.provenance,
        serde_json::to_value(&metadata.provenance)
            .unwrap()
            .as_str()
            .unwrap()
    );

    let expanded_json = serde_json::to_string(&expanded.expanded).unwrap();
    assert!(!expanded_json.contains("body"));
    assert!(!expanded_json.contains("source_path"));
    assert!(!expanded_json.contains("score"));
    assert!(!expanded_json.contains("deterministic retrieval evidence"));
    assert!(!expanded_json.contains(&index_root.to_string_lossy().to_string()));
    let json = serde_json::to_string(&expanded).unwrap();
    let value: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        value["expanded"][0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "direction",
            "id",
            "layer",
            "provenance",
            "rank",
            "relation",
            "title",
            "type",
            "via_id",
        ]
    );
}

#[test]
fn graph_corruption_blocks_queries_with_one_stable_code() {
    type Mutation = fn(&mut Value);
    let cases: &[(&str, Mutation)] = &[
        ("duplicate-edge", |graph| {
            let duplicate = graph["edges"][0].clone();
            graph["edges"].as_array_mut().unwrap().insert(0, duplicate);
        }),
        ("unbundled-valid-id-edge", |graph| {
            let nodes = graph["nodes"].as_array().expect("graph nodes");
            let source = nodes.last().unwrap()["id"].clone();
            let target = nodes.first().unwrap()["id"].clone();
            let edge = serde_json::json!({
                "source": source,
                "relation": "references",
                "target": target,
            });
            assert!(
                !graph["edges"]
                    .as_array()
                    .expect("graph edges")
                    .contains(&edge),
                "chosen edge must be absent before injection"
            );
            graph["edges"].as_array_mut().unwrap().push(edge);
        }),
    ];

    for (name, mutate) in cases {
        let fixture = P2Fixture::new_with_query_cases(&format!("query-graph-{name}"));
        let root = build_fixture_index(&fixture);
        assert!(
            query(&fixture, &root, "synthetic evidence", 5)
                .results
                .len()
                > 1
        );
        let graph_path = current_generation(&root).join("graph.json");
        let mut graph: Value = serde_json::from_slice(&fs::read(&graph_path).unwrap()).unwrap();
        mutate(&mut graph);
        write_pretty_json(&graph_path, &graph);

        assert_eq!(
            query_error_code(&fixture, &root),
            "index.graph_invalid",
            "case {name}"
        );
    }
}

#[test]
fn manifest_corruption_blocks_queries_with_exact_binding_codes() {
    let unknown = P2Fixture::new_with_query_cases("query-manifest-unknown-field");
    let unknown_root = build_fixture_index(&unknown);
    let manifest_path = current_generation(&unknown_root).join("index-manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["private_unknown_sentinel"] = serde_json::json!(true);
    write_pretty_json(&manifest_path, &manifest);
    assert_eq!(
        query_error_code(&unknown, &unknown_root),
        "index.manifest_invalid"
    );

    let cases: &[(&str, &str, Value, &str)] = &[
        (
            "bundle-binding",
            "bundle_hash",
            serde_json::json!("0".repeat(64)),
            "index.bundle_mismatch",
        ),
        (
            "config-binding",
            "config_hash",
            serde_json::json!("0".repeat(64)),
            "index.schema_mismatch",
        ),
        (
            "version-binding",
            "sovmem_version",
            serde_json::json!("private-version"),
            "index.schema_mismatch",
        ),
    ];

    for (name, key, value, expected) in cases {
        let fixture = P2Fixture::new_with_query_cases(&format!("query-manifest-{name}"));
        let root = build_fixture_index(&fixture);
        assert!(
            query(&fixture, &root, "synthetic evidence", 5)
                .results
                .len()
                > 1
        );
        replace_manifest_field(&root, key, value.clone());

        assert_eq!(query_error_code(&fixture, &root), *expected, "case {name}");
    }
}

#[test]
fn missing_tantivy_file_blocks_query_with_stable_code() {
    let fixture = P2Fixture::new_with_query_cases("query-missing-tantivy-file");
    let root = build_fixture_index(&fixture);
    assert!(
        query(&fixture, &root, "synthetic evidence", 5)
            .results
            .len()
            > 1
    );
    fs::remove_file(current_generation(&root).join("tantivy/meta.json"))
        .expect("remove Tantivy metadata file");
    assert_eq!(query_error_code(&fixture, &root), "index.tantivy_invalid");
}

#[test]
fn traversing_current_blocks_query_with_stable_code() {
    let fixture = P2Fixture::new_with_query_cases("query-traversing-current");
    let root = build_fixture_index(&fixture);
    fs::write(root.join("CURRENT"), b"../slot-a\n").expect("write traversing CURRENT");
    assert_eq!(query_error_code(&fixture, &root), "index.current_invalid");
}

#[cfg(unix)]
#[test]
fn symlink_current_blocks_query_without_following_target() {
    use std::os::unix::fs::symlink;

    let fixture = P2Fixture::new_with_query_cases("query-symlink-current");
    let root = build_fixture_index(&fixture);
    let target = fixture.index_root("external-current");
    fs::write(&target, b"slot-a\n").expect("write external CURRENT target");
    fs::remove_file(root.join("CURRENT")).expect("remove real CURRENT");
    symlink(&target, root.join("CURRENT")).expect("symlink CURRENT");

    assert_eq!(query_error_code(&fixture, &root), "index.current_invalid");
    assert_eq!(fs::read(target).unwrap(), b"slot-a\n");
}
