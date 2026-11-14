use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

use super::{
    load_profile, load_retrieval_thresholds, open_current_index, parse_queries,
    search_opened_index, validate_golden_file, ExpansionMode, GoldenError, IndexError, QueryError,
    RetrievalThresholds, ThresholdError,
};

pub const BASELINE_SCHEMA: &str = "sovmem-retrieval-baseline-v1";
const GOLDEN_FILE: &str = "evals/audy-sovmem-v0/golden.jsonl";
const THRESHOLDS_FILE: &str = "evals/audy-sovmem-v0/thresholds.json";

static NEXT_BASELINE_ID: AtomicU64 = AtomicU64::new(0);

pub struct EvalRequest {
    pub profile_path: PathBuf,
    pub bundle_root: PathBuf,
    pub index_root: PathBuf,
    pub golden_path: PathBuf,
    pub thresholds_path: PathBuf,
    pub baseline_path: PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Q3Decision {
    Go,
    NoGo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvalOutcome {
    pub decision: Q3Decision,
    pub evaluation_set: String,
    pub queries: usize,
    pub k: usize,
    pub queries_with_hit: usize,
    pub required_queries_with_hit: usize,
    pub precision_numerator: usize,
    pub precision_denominator: usize,
    pub precision_decimal: String,
    pub recall_numerator: usize,
    pub recall_denominator: usize,
    pub recall_decimal: String,
    pub baseline_written: bool,
    pub baseline: PathBuf,
    pub missed_query_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvaluationBaseline {
    pub schema_version: String,
    pub evaluation_set: BaselineEvaluationSet,
    pub corpus: BaselineCorpus,
    pub retrieval: BaselineRetrieval,
    pub index: BaselineIndex,
    pub metrics: EvaluationMetrics,
    pub queries: Vec<EvaluationRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BaselineEvaluationSet {
    pub id: String,
    pub golden_file: String,
    pub golden_blake3: String,
    pub thresholds_file: String,
    pub thresholds_blake3: String,
    pub query_count: usize,
    pub relevant_documents_per_query: usize,
    pub k: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BaselineCorpus {
    pub profile: String,
    pub context_namespace: String,
    pub source_commit: String,
    pub bundle_hash: String,
    pub documents: usize,
    pub core: usize,
    pub research: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BaselineRetrieval {
    pub engine: String,
    pub sovmem_version: String,
    pub tantivy_version: String,
    pub petgraph_version: String,
    pub index_schema: String,
    pub tokenizer: String,
    pub ranking: String,
    pub index_config: String,
    pub graph_expansion: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BaselineIndex {
    pub documents: usize,
    pub documents_hash: String,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub external_typed_links: usize,
    pub graph_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvaluationMetrics {
    pub queries_with_hit: usize,
    pub queries_without_hit: usize,
    pub precision_at_5: MetricFraction,
    pub recall_at_5: MetricFraction,
    pub gate: EvaluationGate,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MetricFraction {
    pub numerator: usize,
    pub denominator: usize,
    pub decimal: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvaluationGate {
    pub minimum_queries_with_hit: usize,
    pub status: Q3Decision,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvaluationRecord {
    pub id: String,
    pub relevant_ids: Vec<String>,
    pub result_ids: Vec<String>,
    pub hit_rank: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HitEvaluation {
    pub decision: Q3Decision,
    pub precision_numerator: usize,
    pub precision_denominator: usize,
    pub precision_decimal: String,
    pub recall_numerator: usize,
    pub recall_denominator: usize,
    pub recall_decimal: String,
}

#[derive(Debug, Error)]
pub enum EvalError {
    #[error("index evaluation failed")]
    Index(#[from] IndexError),
    #[error("query evaluation failed")]
    Query(#[from] QueryError),
    #[error("golden evaluation input failed")]
    Golden(#[from] GoldenError),
    #[error("threshold evaluation input failed")]
    Threshold(#[from] ThresholdError),
    #[error("baseline output is invalid")]
    BaselineOutputInvalid,
    #[error("verified corpus split does not account for every document")]
    CorpusSplitInvalid,
    #[error("baseline write failed")]
    Write,
}

impl EvalError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Index(error) => error.code(),
            Self::Query(error) => error.code(),
            Self::Golden(error) => error.code(),
            Self::Threshold(error) => error.code(),
            Self::BaselineOutputInvalid => "eval.baseline_output_invalid",
            Self::CorpusSplitInvalid => "eval.corpus_split_invalid",
            Self::Write => "eval.write_failed",
        }
    }
}

pub fn evaluate(request: EvalRequest) -> Result<EvalOutcome, EvalError> {
    let destination = validate_baseline_destination(&request)?;
    let thresholds_bytes = fs::read(&request.thresholds_path)
        .map_err(|_| EvalError::Threshold(ThresholdError::Read))?;
    let golden_bytes =
        fs::read(&request.golden_path).map_err(|_| EvalError::Golden(GoldenError::Read))?;
    let profile = load_profile(&request.profile_path)
        .map_err(IndexError::Profile)
        .map_err(EvalError::Index)?;
    let thresholds = load_retrieval_thresholds(&request.thresholds_path, &request.golden_path)?;
    validate_golden_file(&profile, &request.bundle_root, &request.golden_path)?;
    ensure_inputs_unchanged(&request, &thresholds_bytes, &golden_bytes)?;

    let queries = parse_queries(&golden_bytes)?;
    let golden_blake3 = blake3::hash(&golden_bytes).to_hex().to_string();
    if golden_blake3 != thresholds.golden_blake3() {
        return Err(EvalError::Threshold(ThresholdError::GoldenHash));
    }
    let thresholds_blake3 = blake3::hash(&thresholds_bytes).to_hex().to_string();

    let opened = open_current_index(
        &request.profile_path,
        &request.bundle_root,
        &request.index_root,
    )?;
    // The baseline reports the measured core/research split. Fail closed if it
    // does not account for every verified document, so a drifting corpus
    // composition cannot be published as settled gate evidence.
    if opened.core + opened.research != opened.manifest.documents {
        return Err(EvalError::CorpusSplitInvalid);
    }
    let mut records = Vec::with_capacity(queries.len());
    for query in queries {
        let result = search_opened_index(
            &opened,
            &query.query,
            thresholds.retrieval.k,
            ExpansionMode::None,
        )?;
        let result_ids = result
            .results
            .into_iter()
            .map(|hit| hit.id)
            .collect::<Vec<_>>();
        let relevant_id = &query.relevant_ids[0];
        let hit_rank = result_ids
            .iter()
            .position(|id| id == relevant_id)
            .map(|position| position + 1);
        records.push(EvaluationRecord {
            id: query.id,
            relevant_ids: query.relevant_ids,
            result_ids,
            hit_rank,
        });
    }

    let metrics = score_evaluation(&records, &thresholds);
    let manifest = &opened.manifest;
    let baseline = EvaluationBaseline {
        schema_version: BASELINE_SCHEMA.to_owned(),
        evaluation_set: BaselineEvaluationSet {
            id: thresholds.evaluation_set.id.clone(),
            golden_file: GOLDEN_FILE.to_owned(),
            golden_blake3,
            thresholds_file: THRESHOLDS_FILE.to_owned(),
            thresholds_blake3,
            query_count: thresholds.evaluation_set.query_count,
            relevant_documents_per_query: thresholds.evaluation_set.relevant_documents_per_query,
            k: thresholds.retrieval.k,
        },
        corpus: BaselineCorpus {
            profile: manifest.profile.clone(),
            context_namespace: manifest.context_namespace.clone(),
            source_commit: manifest.source_commit.clone(),
            bundle_hash: manifest.bundle_hash.clone(),
            documents: manifest.documents,
            core: opened.core,
            research: opened.research,
        },
        retrieval: BaselineRetrieval {
            engine: "tantivy-bm25".to_owned(),
            sovmem_version: manifest.sovmem_version.clone(),
            tantivy_version: manifest.tantivy_version.clone(),
            petgraph_version: manifest.petgraph_version.clone(),
            index_schema: manifest.index_schema.clone(),
            tokenizer: manifest.tokenizer.clone(),
            ranking: manifest.ranking.clone(),
            index_config: manifest.config_hash.clone(),
            graph_expansion: "none".to_owned(),
        },
        index: BaselineIndex {
            documents: manifest.documents,
            documents_hash: manifest.documents_hash.clone(),
            graph_nodes: manifest.graph_nodes,
            graph_edges: manifest.graph_edges,
            external_typed_links: manifest.external_typed_links,
            graph_hash: manifest.graph_hash.clone(),
        },
        metrics: metrics.clone(),
        queries: records,
    };
    let mut bytes = serde_json::to_vec_pretty(&baseline).map_err(|_| EvalError::Write)?;
    bytes.push(b'\n');
    publish_baseline(&destination, &bytes)?;

    let missed_query_ids = baseline
        .queries
        .iter()
        .filter(|record| record.hit_rank.is_none())
        .map(|record| record.id.clone())
        .collect();
    Ok(EvalOutcome {
        decision: metrics.gate.status,
        evaluation_set: baseline.evaluation_set.id,
        queries: baseline.evaluation_set.query_count,
        k: baseline.evaluation_set.k,
        queries_with_hit: metrics.queries_with_hit,
        required_queries_with_hit: metrics.gate.minimum_queries_with_hit,
        precision_numerator: metrics.precision_at_5.numerator,
        precision_denominator: metrics.precision_at_5.denominator,
        precision_decimal: metrics.precision_at_5.decimal,
        recall_numerator: metrics.recall_at_5.numerator,
        recall_denominator: metrics.recall_at_5.denominator,
        recall_decimal: metrics.recall_at_5.decimal,
        baseline_written: true,
        baseline: request.baseline_path,
        missed_query_ids,
    })
}

pub(crate) fn score_evaluation(
    records: &[EvaluationRecord],
    thresholds: &RetrievalThresholds,
) -> EvaluationMetrics {
    let hit_queries = records
        .iter()
        .filter(|record| record.hit_rank.is_some())
        .count();
    metrics_from_hits(
        hit_queries,
        thresholds.evaluation_set.query_count,
        thresholds.retrieval.k,
        thresholds.minimum_queries_with_hit(),
    )
}

pub fn evaluate_hits_for_test(hit_queries: usize) -> HitEvaluation {
    let metrics = metrics_from_hits(hit_queries, 25, 5, 20);
    HitEvaluation {
        decision: metrics.gate.status,
        precision_numerator: metrics.precision_at_5.numerator,
        precision_denominator: metrics.precision_at_5.denominator,
        precision_decimal: metrics.precision_at_5.decimal,
        recall_numerator: metrics.recall_at_5.numerator,
        recall_denominator: metrics.recall_at_5.denominator,
        recall_decimal: metrics.recall_at_5.decimal,
    }
}

fn metrics_from_hits(
    hit_queries: usize,
    query_count: usize,
    k: usize,
    minimum_queries_with_hit: usize,
) -> EvaluationMetrics {
    assert!(hit_queries <= query_count);
    let status = if hit_queries >= minimum_queries_with_hit {
        Q3Decision::Go
    } else {
        Q3Decision::NoGo
    };
    let precision_denominator = query_count * k;
    EvaluationMetrics {
        queries_with_hit: hit_queries,
        queries_without_hit: query_count - hit_queries,
        precision_at_5: MetricFraction {
            numerator: hit_queries,
            denominator: precision_denominator,
            decimal: format!("{:.4}", hit_queries as f64 / precision_denominator as f64),
        },
        recall_at_5: MetricFraction {
            numerator: hit_queries,
            denominator: query_count,
            decimal: format!("{:.4}", hit_queries as f64 / query_count as f64),
        },
        gate: EvaluationGate {
            minimum_queries_with_hit,
            status,
        },
    }
}

fn validate_baseline_destination(request: &EvalRequest) -> Result<PathBuf, EvalError> {
    let file_name = request
        .baseline_path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or(EvalError::BaselineOutputInvalid)?;
    let requested_parent = request
        .baseline_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent =
        fs::canonicalize(requested_parent).map_err(|_| EvalError::BaselineOutputInvalid)?;
    let parent_metadata =
        fs::symlink_metadata(&parent).map_err(|_| EvalError::BaselineOutputInvalid)?;
    if !parent_metadata.file_type().is_dir() || parent_metadata.permissions().readonly() {
        return Err(EvalError::BaselineOutputInvalid);
    }
    let destination = parent.join(file_name);
    match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.file_type().is_file() => {
            File::open(&destination).map_err(|_| EvalError::BaselineOutputInvalid)?;
        }
        Ok(_) => return Err(EvalError::BaselineOutputInvalid),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(EvalError::BaselineOutputInvalid),
    }

    for input in [
        &request.profile_path,
        &request.golden_path,
        &request.thresholds_path,
    ] {
        if fs::canonicalize(input).is_ok_and(|path| path == destination) {
            return Err(EvalError::BaselineOutputInvalid);
        }
    }
    for protected_root in [&request.bundle_root, &request.index_root] {
        if fs::canonicalize(protected_root).is_ok_and(|root| destination.starts_with(root)) {
            return Err(EvalError::BaselineOutputInvalid);
        }
    }
    Ok(destination)
}

fn ensure_inputs_unchanged(
    request: &EvalRequest,
    thresholds_bytes: &[u8],
    golden_bytes: &[u8],
) -> Result<(), EvalError> {
    match fs::read(&request.thresholds_path) {
        Ok(current) if current == thresholds_bytes => {}
        _ => return Err(EvalError::Threshold(ThresholdError::Read)),
    }
    match fs::read(&request.golden_path) {
        Ok(current) if current == golden_bytes => {}
        _ => return Err(EvalError::Golden(GoldenError::Read)),
    }
    Ok(())
}

fn publish_baseline(destination: &Path, bytes: &[u8]) -> Result<(), EvalError> {
    let parent = destination
        .parent()
        .ok_or(EvalError::BaselineOutputInvalid)?;
    let mut temporary = None;
    for _ in 0..1_000 {
        let sequence = NEXT_BASELINE_ID.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".sovmem-baseline-{}-{sequence}.tmp",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                temporary = Some((path, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(EvalError::Write),
        }
    }
    let (temporary_path, mut file) = temporary.ok_or(EvalError::Write)?;
    let result = (|| {
        file.write_all(bytes).map_err(|_| EvalError::Write)?;
        file.flush().map_err(|_| EvalError::Write)?;
        file.sync_all().map_err(|_| EvalError::Write)?;
        drop(file);
        fs::rename(&temporary_path, destination).map_err(|_| EvalError::Write)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}
