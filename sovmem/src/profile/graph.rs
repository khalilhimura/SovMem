use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::{Directed, Direction as PetDirection};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

use super::{GeneratedDocument, LinkRelation};

pub const GRAPH_SCHEMA: &str = "sovmem-derived-graph-v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct GraphNode {
    pub id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct GraphEdge {
    pub source: String,
    pub relation: String,
    pub target: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphProjection {
    pub schema_version: String,
    pub bundle_hash: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub external_typed_links: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GraphError {
    #[error("graph schema is invalid")]
    Schema,
    #[error("graph node set is invalid")]
    Node,
    #[error("graph edge is duplicated")]
    DuplicateEdge,
    #[error("graph edge target is invalid")]
    Edge,
    #[error("graph JSON is invalid")]
    Parse,
}

impl GraphError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Schema => "graph.schema",
            Self::Node => "graph.node",
            Self::DuplicateEdge => "graph.duplicate_edge",
            Self::Edge => "graph.edge",
            Self::Parse => "graph.parse",
        }
    }
}

#[derive(Debug)]
pub struct GraphAdjacency {
    graph: Graph<String, String, Directed>,
    by_id: BTreeMap<String, NodeIndex>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Outgoing,
    Incoming,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExpansionWitness {
    pub id: String,
    pub via_id: String,
    pub direction: Direction,
    pub relation: String,
}

pub fn derive_graph(
    bundle_hash: &str,
    documents: &[GeneratedDocument],
) -> Result<GraphProjection, GraphError> {
    if !lowercase_hash(bundle_hash) {
        return Err(GraphError::Schema);
    }

    let mut ordered_documents: Vec<_> = documents.iter().collect();
    ordered_documents.sort_by(|left, right| {
        left.metadata
            .source_path
            .as_bytes()
            .cmp(right.metadata.source_path.as_bytes())
            .then_with(|| {
                left.metadata
                    .id
                    .as_bytes()
                    .cmp(right.metadata.id.as_bytes())
            })
    });

    let mut ids = BTreeSet::new();
    let mut nodes = Vec::with_capacity(ordered_documents.len());
    for document in &ordered_documents {
        if document.metadata.id.is_empty() || !ids.insert(document.metadata.id.clone()) {
            return Err(GraphError::Node);
        }
        nodes.push(GraphNode {
            id: document.metadata.id.clone(),
        });
    }

    let mut edges = Vec::new();
    let mut external_typed_links = 0;
    for document in ordered_documents {
        for link in &document.metadata.links {
            let target = normalized_target(&link.target);
            if ids.contains(target) {
                edges.push(GraphEdge {
                    source: document.metadata.id.clone(),
                    relation: relation_name(&link.rel).into(),
                    target: target.into(),
                });
            } else {
                external_typed_links += 1;
            }
        }
    }
    edges.sort_by(compare_edges);
    if edges
        .windows(2)
        .any(|pair| compare_edges(&pair[0], &pair[1]) == Ordering::Equal)
    {
        return Err(GraphError::DuplicateEdge);
    }

    let projection = GraphProjection {
        schema_version: GRAPH_SCHEMA.into(),
        bundle_hash: bundle_hash.into(),
        nodes,
        edges,
        external_typed_links,
    };
    projection.validate()?;
    Ok(projection)
}

impl GraphProjection {
    pub fn to_bytes(&self) -> Result<Vec<u8>, GraphError> {
        self.validate()?;
        serialize(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, GraphError> {
        let graph: Self = serde_json::from_slice(bytes).map_err(|_| GraphError::Parse)?;
        graph.validate()?;
        if graph.to_bytes()? != bytes {
            return Err(GraphError::Parse);
        }
        Ok(graph)
    }

    pub fn blake3(&self) -> Result<String, GraphError> {
        let bytes = self.to_bytes()?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }

    pub fn adjacency(&self) -> GraphAdjacency {
        let mut graph = Graph::<String, String, Directed>::new();
        let mut by_id = BTreeMap::new();
        for node in &self.nodes {
            let index = graph.add_node(node.id.clone());
            by_id.insert(node.id.clone(), index);
        }
        for edge in &self.edges {
            let source = by_id[&edge.source];
            let target = by_id[&edge.target];
            graph.add_edge(source, target, edge.relation.clone());
        }
        GraphAdjacency { graph, by_id }
    }

    fn validate(&self) -> Result<(), GraphError> {
        if self.schema_version != GRAPH_SCHEMA || !lowercase_hash(&self.bundle_hash) {
            return Err(GraphError::Schema);
        }
        let mut ids = BTreeSet::new();
        for node in &self.nodes {
            if node.id.is_empty() || !ids.insert(node.id.as_str()) {
                return Err(GraphError::Node);
            }
        }
        let mut unique_edges = BTreeSet::new();
        for edge in &self.edges {
            if !unique_edges.insert((
                edge.source.as_str(),
                edge.relation.as_str(),
                edge.target.as_str(),
            )) {
                return Err(GraphError::DuplicateEdge);
            }
        }
        for pair in self.edges.windows(2) {
            match compare_edges(&pair[0], &pair[1]) {
                Ordering::Equal => return Err(GraphError::DuplicateEdge),
                Ordering::Greater => return Err(GraphError::Edge),
                Ordering::Less => {}
            }
        }
        if self.edges.iter().any(|edge| {
            !known_relation(&edge.relation)
                || !ids.contains(edge.source.as_str())
                || !ids.contains(edge.target.as_str())
        }) {
            return Err(GraphError::Edge);
        }
        Ok(())
    }
}

impl GraphAdjacency {
    pub fn expand_one_hop(&self, seed_ids: &[String]) -> Vec<ExpansionWitness> {
        let seeds: BTreeSet<_> = seed_ids.iter().map(String::as_str).collect();
        let mut seen = BTreeSet::new();
        let mut witnesses = Vec::new();

        for seed_id in seed_ids {
            let Some(&seed) = self.by_id.get(seed_id) else {
                continue;
            };
            for (direction, pet_direction) in [
                (Direction::Outgoing, PetDirection::Outgoing),
                (Direction::Incoming, PetDirection::Incoming),
            ] {
                let mut candidates: Vec<_> = self
                    .graph
                    .edges_directed(seed, pet_direction)
                    .map(|edge| {
                        let neighbor = match direction {
                            Direction::Outgoing => edge.target(),
                            Direction::Incoming => edge.source(),
                        };
                        (edge.weight().clone(), self.graph[neighbor].clone())
                    })
                    .collect();
                candidates.sort_by(|left, right| {
                    left.0
                        .as_bytes()
                        .cmp(right.0.as_bytes())
                        .then_with(|| left.1.as_bytes().cmp(right.1.as_bytes()))
                });
                for (relation, neighbor) in candidates {
                    if seeds.contains(neighbor.as_str()) || !seen.insert(neighbor.clone()) {
                        continue;
                    }
                    witnesses.push(ExpansionWitness {
                        id: neighbor,
                        via_id: seed_id.clone(),
                        direction: direction.clone(),
                        relation,
                    });
                }
            }
        }
        witnesses
    }
}

fn serialize(graph: &GraphProjection) -> Result<Vec<u8>, GraphError> {
    let mut bytes = serde_json::to_vec_pretty(graph).map_err(|_| GraphError::Parse)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn normalized_target(target: &str) -> &str {
    let target = target.strip_prefix("vault://").unwrap_or(target);
    target
        .split_once('#')
        .map_or(target, |(identity, _)| identity)
}

fn relation_name(relation: &LinkRelation) -> &'static str {
    match relation {
        LinkRelation::Cites => "cites",
        LinkRelation::DerivedFrom => "derived_from",
        LinkRelation::Supersedes => "supersedes",
        LinkRelation::DependsOn => "depends_on",
        LinkRelation::References => "references",
    }
}

fn known_relation(relation: &str) -> bool {
    matches!(
        relation,
        "cites" | "derived_from" | "supersedes" | "depends_on" | "references"
    )
}

fn compare_edges(left: &GraphEdge, right: &GraphEdge) -> Ordering {
    left.source
        .as_bytes()
        .cmp(right.source.as_bytes())
        .then_with(|| left.relation.as_bytes().cmp(right.relation.as_bytes()))
        .then_with(|| left.target.as_bytes().cmp(right.target.as_bytes()))
}

fn lowercase_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
