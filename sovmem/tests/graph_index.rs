#[allow(dead_code)]
mod support;

use serde_json::Value;
use sovmem::profile::{
    derive_graph, Authority, CoreType, Direction, GeneratedDocument, GeneratedMetadata, GraphError,
    GraphProjection, Layer, LinkRelation, Provenance, Sensitivity, SourceKind, Status, TypedLink,
};
use std::collections::BTreeMap;
use support::p2::P2Fixture;

fn document(id: &str, source_path: &str, links: Vec<TypedLink>) -> GeneratedDocument {
    GeneratedDocument {
        metadata: GeneratedMetadata {
            okf_version: "0.1".into(),
            profile: "nous-sovmem-v0".into(),
            nous_schema: "nous-v0.3".into(),
            ssa_id: "ssa-0".into(),
            ssa_profile: "audy-ssa0-v0".into(),
            familiar_id: "audy-sovmem-v0".into(),
            context_namespace: "ssa-0".into(),
            id: id.into(),
            core_type: CoreType::Concept,
            context_kind: "Synthetic".into(),
            title: format!("Synthetic {id}"),
            description: "Synthetic graph fixture.".into(),
            layer: Layer::Analysis,
            source: format!("vault://{id}"),
            source_path: source_path.into(),
            resource: format!("vault://{id}"),
            source_kind: SourceKind::VaultNote,
            provenance: Provenance::Synthesis,
            status: Status::Active,
            authority: Authority::Reviewed,
            sensitivity: Sensitivity::Internal,
            created: "2026-01-02".into(),
            updated: "2026-01-05".into(),
            tags: vec!["graph-fixture".into()],
            links,
            inferred_fields: Vec::new(),
            owners: None,
            valid_from: None,
            valid_to: None,
            supersedes: None,
            superseded_by: None,
            verdict: None,
            reason: None,
            subject: None,
            extensions: BTreeMap::new(),
        },
        body: format!("# Synthetic {id}\n"),
        source_hash: "d".repeat(64),
    }
}

fn graph_documents(alpha_links: Vec<TypedLink>) -> Vec<GeneratedDocument> {
    vec![
        document("ssa-0/beta", "beta.md", Vec::new()),
        document("ssa-0/alpha", "alpha.md", alpha_links),
    ]
}

fn one_internal_link() -> Vec<TypedLink> {
    vec![TypedLink {
        rel: LinkRelation::References,
        target: "vault://ssa-0/beta#section".into(),
    }]
}

fn canonical_json(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).expect("serialize graph JSON");
    bytes.push(b'\n');
    bytes
}

fn duplicate_first_edge(bytes: &[u8]) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(bytes).expect("parse graph JSON");
    let edges = value["edges"].as_array_mut().expect("graph edges array");
    edges.push(edges[0].clone());
    canonical_json(&value)
}

fn graph_for_expansion(documents: Vec<GeneratedDocument>) -> GraphProjection {
    derive_graph(&"e".repeat(64), &documents).expect("derive expansion graph")
}

#[test]
fn derives_only_internal_typed_frontmatter_edges() {
    let documents = graph_documents(vec![
        TypedLink {
            rel: LinkRelation::References,
            target: "ssa-0/beta#section".into(),
        },
        TypedLink {
            rel: LinkRelation::Cites,
            target: "https://example.com/source".into(),
        },
    ]);
    let graph = derive_graph(&"a".repeat(64), &documents).expect("derive graph");
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.nodes[0].id, "ssa-0/alpha");
    assert_eq!(graph.nodes[1].id, "ssa-0/beta");
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.external_typed_links, 1);
    assert_eq!(graph.edges[0].source, "ssa-0/alpha");
    assert_eq!(graph.edges[0].relation, "references");
    assert_eq!(graph.edges[0].target, "ssa-0/beta");
}

#[test]
fn body_links_titles_tags_and_paths_never_create_graph_edges() {
    let mut documents = graph_documents(Vec::new());
    documents[0].body = "[[alpha]] [alpha](alpha.md)".into();
    documents[0].metadata.title = "ssa-0/alpha".into();
    documents[0].metadata.tags = vec!["ssa-0/alpha".into()];
    documents[0].metadata.source_path = "alpha.md/beta.md".into();
    let graph = derive_graph(&"b".repeat(64), &documents).expect("derive graph");
    assert!(graph.edges.is_empty());
    assert_eq!(graph.external_typed_links, 0);
}

#[test]
fn preserves_every_known_relation_and_sorts_edge_keys_by_raw_bytes() {
    let links = [
        LinkRelation::Supersedes,
        LinkRelation::References,
        LinkRelation::DerivedFrom,
        LinkRelation::DependsOn,
        LinkRelation::Cites,
    ]
    .into_iter()
    .map(|rel| TypedLink {
        rel,
        target: "ssa-0/beta".into(),
    })
    .collect();
    let graph = derive_graph(&"c".repeat(64), &graph_documents(links)).expect("derive graph");
    assert_eq!(
        graph
            .edges
            .iter()
            .map(|edge| edge.relation.as_str())
            .collect::<Vec<_>>(),
        [
            "cites",
            "depends_on",
            "derived_from",
            "references",
            "supersedes",
        ]
    );
}

#[test]
fn rejects_duplicate_derived_edges_instead_of_collapsing_them() {
    let duplicate = TypedLink {
        rel: LinkRelation::References,
        target: "ssa-0/beta".into(),
    };
    let error = derive_graph(
        &"c".repeat(64),
        &graph_documents(vec![duplicate.clone(), duplicate]),
    )
    .expect_err("duplicate typed edges must fail");
    assert_eq!(error.code(), "graph.duplicate_edge");
}

#[test]
fn graph_json_is_canonical_and_rejects_unknown_or_duplicate_edges() {
    let graph =
        derive_graph(&"c".repeat(64), &graph_documents(one_internal_link())).expect("derive graph");
    let bytes = graph.to_bytes().expect("serialize graph");
    let expected = format!(
        concat!(
            "{{\n",
            "  \"schema_version\": \"sovmem-derived-graph-v1\",\n",
            "  \"bundle_hash\": \"{}\",\n",
            "  \"nodes\": [\n",
            "    {{\n",
            "      \"id\": \"ssa-0/alpha\"\n",
            "    }},\n",
            "    {{\n",
            "      \"id\": \"ssa-0/beta\"\n",
            "    }}\n",
            "  ],\n",
            "  \"edges\": [\n",
            "    {{\n",
            "      \"source\": \"ssa-0/alpha\",\n",
            "      \"relation\": \"references\",\n",
            "      \"target\": \"ssa-0/beta\"\n",
            "    }}\n",
            "  ],\n",
            "  \"external_typed_links\": 0\n",
            "}}\n"
        ),
        "c".repeat(64)
    );
    assert_eq!(bytes, expected.as_bytes());
    assert_eq!(GraphProjection::from_bytes(&bytes).unwrap(), graph);
    assert_eq!(graph.blake3().expect("hash canonical graph").len(), 64);

    let duplicated = duplicate_first_edge(&bytes);
    assert_eq!(
        GraphProjection::from_bytes(&duplicated).unwrap_err().code(),
        "graph.duplicate_edge"
    );

    let mut unknown: Value = serde_json::from_slice(&bytes).unwrap();
    unknown["unexpected"] = Value::Bool(true);
    assert_eq!(
        GraphProjection::from_bytes(&canonical_json(&unknown))
            .unwrap_err()
            .code(),
        "graph.parse"
    );
}

#[test]
fn graph_parser_rejects_invalid_hash_nodes_relations_and_endpoints() {
    let graph = derive_graph(&"f".repeat(64), &graph_documents(one_internal_link())).unwrap();
    let value: Value = serde_json::from_slice(&graph.to_bytes().unwrap()).unwrap();

    let mut invalid_hash = value.clone();
    invalid_hash["bundle_hash"] = Value::String("F".repeat(64));
    assert_eq!(
        GraphProjection::from_bytes(&canonical_json(&invalid_hash))
            .unwrap_err()
            .code(),
        "graph.schema"
    );

    let mut duplicate_node = value.clone();
    let nodes = duplicate_node["nodes"].as_array_mut().unwrap();
    nodes.push(nodes[0].clone());
    assert_eq!(
        GraphProjection::from_bytes(&canonical_json(&duplicate_node))
            .unwrap_err()
            .code(),
        "graph.node"
    );

    let mut unknown_relation = value.clone();
    unknown_relation["edges"][0]["relation"] = Value::String("similar_to".into());
    assert_eq!(
        GraphProjection::from_bytes(&canonical_json(&unknown_relation))
            .unwrap_err()
            .code(),
        "graph.edge"
    );

    let mut external_endpoint = value;
    external_endpoint["edges"][0]["target"] = Value::String("ssa-0/missing".into());
    assert_eq!(
        GraphProjection::from_bytes(&canonical_json(&external_endpoint))
            .unwrap_err()
            .code(),
        "graph.edge"
    );
}

#[test]
fn graph_parser_reports_nonadjacent_duplicate_edges_as_duplicates() {
    let documents = graph_documents(vec![
        TypedLink {
            rel: LinkRelation::Cites,
            target: "ssa-0/beta".into(),
        },
        TypedLink {
            rel: LinkRelation::References,
            target: "ssa-0/beta".into(),
        },
    ]);
    let graph = derive_graph(&"f".repeat(64), &documents).unwrap();
    let duplicated = duplicate_first_edge(&graph.to_bytes().unwrap());
    assert_eq!(
        GraphProjection::from_bytes(&duplicated).unwrap_err().code(),
        "graph.duplicate_edge"
    );
}

#[test]
fn graph_hash_rejects_a_malformed_public_projection() {
    let mut graph = derive_graph(&"0".repeat(64), &graph_documents(one_internal_link())).unwrap();
    graph.edges[0].target = "ssa-0/missing".into();
    assert_eq!(graph.blake3().unwrap_err().code(), "graph.edge");
}

#[test]
fn graph_parser_rejects_compact_json() {
    let graph = derive_graph(&"1".repeat(64), &graph_documents(one_internal_link())).unwrap();
    let compact = serde_json::to_vec(&graph).expect("serialize compact graph JSON");
    assert_eq!(
        GraphProjection::from_bytes(&compact).unwrap_err().code(),
        "graph.parse"
    );
}

#[test]
fn graph_parser_rejects_a_missing_canonical_newline() {
    let graph = derive_graph(&"2".repeat(64), &graph_documents(one_internal_link())).unwrap();
    let mut bytes = graph.to_bytes().unwrap();
    assert_eq!(bytes.pop(), Some(b'\n'));
    assert_eq!(
        GraphProjection::from_bytes(&bytes).unwrap_err().code(),
        "graph.parse"
    );
}

#[test]
fn graph_parser_rejects_an_extra_newline() {
    let graph = derive_graph(&"3".repeat(64), &graph_documents(one_internal_link())).unwrap();
    let mut bytes = graph.to_bytes().unwrap();
    bytes.push(b'\n');
    assert_eq!(
        GraphProjection::from_bytes(&bytes).unwrap_err().code(),
        "graph.parse"
    );
}

#[test]
fn canonical_graph_preserves_source_path_order_when_encoded_ids_sort_differently() {
    let documents = vec![
        document("ssa-0/a%3A", "a:.md", Vec::new()),
        document("ssa-0/a-", "a-.md", Vec::new()),
    ];
    let graph = derive_graph(&"4".repeat(64), &documents).expect("derive canonical graph");
    assert_eq!(
        graph
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        ["ssa-0/a-", "ssa-0/a%3A"]
    );
    assert!(graph.nodes[0].id.as_bytes() > graph.nodes[1].id.as_bytes());
    let bytes = graph.to_bytes().expect("serialize canonical graph");
    assert_eq!(GraphProjection::from_bytes(&bytes).unwrap(), graph);
}

#[test]
fn expansion_keeps_the_earlier_seed_for_a_shared_neighbor() {
    let graph = graph_for_expansion(vec![
        document(
            "ssa-0/seed-a",
            "seed-a.md",
            vec![TypedLink {
                rel: LinkRelation::References,
                target: "ssa-0/neighbor".into(),
            }],
        ),
        document(
            "ssa-0/seed-b",
            "seed-b.md",
            vec![TypedLink {
                rel: LinkRelation::Cites,
                target: "ssa-0/neighbor".into(),
            }],
        ),
        document("ssa-0/neighbor", "neighbor.md", Vec::new()),
    ]);

    let witnesses = graph
        .adjacency()
        .expand_one_hop(&["ssa-0/seed-b".into(), "ssa-0/seed-a".into()]);
    assert_eq!(witnesses.len(), 1);
    assert_eq!(witnesses[0].id, "ssa-0/neighbor");
    assert_eq!(witnesses[0].via_id, "ssa-0/seed-b");
    assert_eq!(witnesses[0].direction, Direction::Outgoing);
    assert_eq!(witnesses[0].relation, "cites");
}

#[test]
fn expansion_prefers_outgoing_before_incoming_for_the_same_neighbor() {
    let graph = graph_for_expansion(vec![
        document(
            "ssa-0/seed",
            "seed.md",
            vec![TypedLink {
                rel: LinkRelation::References,
                target: "ssa-0/neighbor".into(),
            }],
        ),
        document(
            "ssa-0/neighbor",
            "neighbor.md",
            vec![TypedLink {
                rel: LinkRelation::Cites,
                target: "ssa-0/seed".into(),
            }],
        ),
    ]);

    let witnesses = graph.adjacency().expand_one_hop(&["ssa-0/seed".into()]);
    assert_eq!(witnesses.len(), 1);
    assert_eq!(witnesses[0].direction, Direction::Outgoing);
    assert_eq!(witnesses[0].relation, "references");
}

#[test]
fn expansion_orders_relation_bytes_before_neighbor_id_bytes() {
    let graph = graph_for_expansion(vec![
        document(
            "ssa-0/seed",
            "seed.md",
            vec![
                TypedLink {
                    rel: LinkRelation::References,
                    target: "ssa-0/alpha".into(),
                },
                TypedLink {
                    rel: LinkRelation::Cites,
                    target: "ssa-0/zulu".into(),
                },
                TypedLink {
                    rel: LinkRelation::Cites,
                    target: "ssa-0/bravo".into(),
                },
            ],
        ),
        document("ssa-0/alpha", "alpha.md", Vec::new()),
        document("ssa-0/bravo", "bravo.md", Vec::new()),
        document("ssa-0/zulu", "zulu.md", Vec::new()),
    ]);

    let witnesses = graph.adjacency().expand_one_hop(&["ssa-0/seed".into()]);
    assert_eq!(
        witnesses
            .iter()
            .map(|witness| (witness.relation.as_str(), witness.id.as_str()))
            .collect::<Vec<_>>(),
        [
            ("cites", "ssa-0/bravo"),
            ("cites", "ssa-0/zulu"),
            ("references", "ssa-0/alpha"),
        ]
    );
}

#[test]
fn expansion_excludes_all_seeds_and_never_expands_witnesses() {
    let graph = graph_for_expansion(vec![
        document(
            "ssa-0/seed",
            "seed.md",
            vec![
                TypedLink {
                    rel: LinkRelation::References,
                    target: "ssa-0/other-seed".into(),
                },
                TypedLink {
                    rel: LinkRelation::References,
                    target: "ssa-0/middle".into(),
                },
            ],
        ),
        document("ssa-0/other-seed", "other-seed.md", Vec::new()),
        document(
            "ssa-0/middle",
            "middle.md",
            vec![TypedLink {
                rel: LinkRelation::References,
                target: "ssa-0/leaf".into(),
            }],
        ),
        document("ssa-0/leaf", "leaf.md", Vec::new()),
    ]);

    let witnesses = graph
        .adjacency()
        .expand_one_hop(&["ssa-0/seed".into(), "ssa-0/other-seed".into()]);
    assert_eq!(
        witnesses
            .iter()
            .map(|witness| witness.id.as_str())
            .collect::<Vec<_>>(),
        ["ssa-0/middle"]
    );
}

#[test]
fn graph_error_codes_are_stable() {
    assert_eq!(GraphError::Schema.code(), "graph.schema");
    assert_eq!(GraphError::Node.code(), "graph.node");
    assert_eq!(GraphError::DuplicateEdge.code(), "graph.duplicate_edge");
    assert_eq!(GraphError::Edge.code(), "graph.edge");
    assert_eq!(GraphError::Parse.code(), "graph.parse");
}

#[test]
fn verified_offline_fixture_projects_the_exact_p2_graph_shape() {
    let fixture = P2Fixture::new("graph-projection");
    let documents = fixture.generated_documents();
    let graph = derive_graph(&fixture.bundle_hash(), &documents).expect("derive verified graph");
    assert_eq!(graph.nodes.len(), 62);
    assert_eq!(graph.edges.len(), 19);
    assert_eq!(graph.external_typed_links, 81);
}
