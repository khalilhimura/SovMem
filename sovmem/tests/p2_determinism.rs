#[allow(dead_code)]
mod support;

use support::p2::{P2Fixture, SemanticSnapshot};

fn assert_complete_frozen_snapshot(snapshot: &SemanticSnapshot) {
    assert_eq!(snapshot.counts.documents, 62);
    assert_eq!(snapshot.counts.graph_nodes, 62);
    assert_eq!(snapshot.counts.graph_edges, 19);
    assert_eq!(snapshot.counts.external_typed_links, 81);
    assert_eq!(snapshot.bm25_ids.len(), 25);
    assert_eq!(snapshot.expansion_witnesses.len(), 25);
    assert!(snapshot.bm25_ids.iter().all(|ids| ids.len() <= 5));
    for bytes in [
        &snapshot.manifest_bytes,
        &snapshot.graph_bytes,
        &snapshot.baseline_bytes,
    ] {
        assert!(bytes.ends_with(b"\n"));
        assert!(!bytes.windows(b"slot-a".len()).any(|part| part == b"slot-a"));
        assert!(!bytes.windows(b"slot-b".len()).any(|part| part == b"slot-b"));
        assert!(!bytes
            .windows(b".building-".len())
            .any(|part| part == b".building-"));
    }
}

#[test]
fn two_clean_builds_and_deleted_rebuild_are_semantically_identical() {
    let fixture = P2Fixture::new_frozen_eval("two-build-proof");
    let root_a = fixture.index_root("index-a");
    let root_b = fixture.index_root("index-b");

    fixture.build_index(&root_a);
    let first_a = fixture.semantic_snapshot(&root_a);
    assert_complete_frozen_snapshot(&first_a);

    fixture.build_index(&root_b);
    let first_b = fixture.semantic_snapshot(&root_b);
    assert_eq!(first_a, first_b);

    std::fs::remove_dir_all(&root_a).expect("delete derived index A");
    assert!(!root_a.exists());
    fixture.build_index(&root_a);
    let rebuilt_a = fixture.semantic_snapshot(&root_a);
    assert_eq!(first_a, rebuilt_a);
}

#[test]
fn same_root_slot_switch_preserves_every_semantic_output() {
    let fixture = P2Fixture::new_frozen_eval("slot-proof");
    let root = fixture.index_root("index");
    fixture.build_index(&root);
    let before = fixture.semantic_snapshot(&root);
    assert_complete_frozen_snapshot(&before);
    assert_eq!(
        std::fs::read_to_string(root.join("CURRENT")).unwrap(),
        "slot-a\n"
    );
    fixture.build_index(&root);
    assert_eq!(
        std::fs::read_to_string(root.join("CURRENT")).unwrap(),
        "slot-b\n"
    );
    assert_eq!(before, fixture.semantic_snapshot(&root));
}
