use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

use super::{parse_queries, validate_query_fields};

pub const THRESHOLDS_SCHEMA: &str = "sovmem-retrieval-thresholds-v1";
pub const EVALUATION_SET_ID: &str = "audy-sovmem-v0";
pub const GOLDEN_FILE: &str = "evals/audy-sovmem-v0/golden.jsonl";
pub const GOLDEN_BLAKE3: &str = "f36c55ee4b1772274e5ab5c22156e660bf5efa146726ac9677e54d81cc1c7175";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RetrievalThresholds {
    pub schema_version: String,
    pub evaluation_set: EvaluationSet,
    pub retrieval: RetrievalGate,
    pub approval: ThresholdApproval,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvaluationSet {
    pub id: String,
    pub golden_file: String,
    pub golden_blake3: String,
    pub query_count: usize,
    pub relevant_documents_per_query: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RetrievalGate {
    pub k: usize,
    pub macro_precision_at_k_minimum: String,
    pub macro_recall_at_k_minimum: String,
    pub minimum_queries_with_hit: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ThresholdApproval {
    pub human_approved: bool,
    pub authority: String,
    pub freeze_date: String,
    pub frozen_through_phase: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ThresholdError {
    #[error("threshold configuration could not be read")]
    Read,
    #[error("threshold configuration could not be parsed")]
    Parse,
    #[error("threshold schema is unsupported")]
    Schema,
    #[error("threshold evaluation-set identity is invalid")]
    EvaluationSet,
    #[error("golden query bytes do not match the frozen evaluation set")]
    GoldenHash,
    #[error("golden query structure does not match the frozen evaluation set")]
    GoldenContract,
    #[error("retrieval threshold contract is invalid")]
    Retrieval,
    #[error("threshold approval record is invalid")]
    Approval,
}

impl ThresholdError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Read => "thresholds.read",
            Self::Parse => "thresholds.parse",
            Self::Schema => "thresholds.schema",
            Self::EvaluationSet => "thresholds.evaluation_set",
            Self::GoldenHash => "thresholds.golden_hash",
            Self::GoldenContract => "thresholds.golden_contract",
            Self::Retrieval => "thresholds.retrieval",
            Self::Approval => "thresholds.approval",
        }
    }
}

impl RetrievalThresholds {
    pub const fn minimum_queries_with_hit(&self) -> usize {
        self.retrieval.minimum_queries_with_hit
    }

    pub fn golden_blake3(&self) -> &str {
        &self.evaluation_set.golden_blake3
    }
}

pub fn load_retrieval_thresholds(
    thresholds_path: &Path,
    golden_path: &Path,
) -> Result<RetrievalThresholds, ThresholdError> {
    let threshold_bytes = fs::read(thresholds_path).map_err(|_| ThresholdError::Read)?;
    let thresholds: RetrievalThresholds =
        serde_json::from_slice(&threshold_bytes).map_err(|_| ThresholdError::Parse)?;

    if thresholds.schema_version != THRESHOLDS_SCHEMA {
        return Err(ThresholdError::Schema);
    }
    if thresholds.evaluation_set.id != EVALUATION_SET_ID
        || thresholds.evaluation_set.golden_file != GOLDEN_FILE
        || thresholds.evaluation_set.golden_blake3 != GOLDEN_BLAKE3
        || thresholds.evaluation_set.query_count != 25
        || thresholds.evaluation_set.relevant_documents_per_query != 1
    {
        return Err(ThresholdError::EvaluationSet);
    }
    if thresholds.retrieval.k != 5
        || thresholds.retrieval.macro_precision_at_k_minimum != "0.16"
        || thresholds.retrieval.macro_recall_at_k_minimum != "0.80"
        || thresholds.retrieval.minimum_queries_with_hit != 20
    {
        return Err(ThresholdError::Retrieval);
    }
    if !thresholds.approval.human_approved
        || thresholds.approval.authority != "repository-owner"
        || thresholds.approval.freeze_date != "2026-07-12"
        || thresholds.approval.frozen_through_phase != "P2"
    {
        return Err(ThresholdError::Approval);
    }

    let golden_bytes = fs::read(golden_path).map_err(|_| ThresholdError::Read)?;
    let queries = parse_queries(&golden_bytes).map_err(|_| ThresholdError::GoldenContract)?;
    validate_query_fields(&queries).map_err(|_| ThresholdError::GoldenContract)?;
    if queries.len() != thresholds.evaluation_set.query_count
        || queries.iter().any(|query| {
            query.relevant_ids.len() != thresholds.evaluation_set.relevant_documents_per_query
        })
    {
        return Err(ThresholdError::GoldenContract);
    }

    let golden_hash = blake3::hash(&golden_bytes).to_hex().to_string();
    if golden_hash != thresholds.evaluation_set.golden_blake3 {
        return Err(ThresholdError::GoldenHash);
    }

    let precision_passes = thresholds.retrieval.minimum_queries_with_hit * 100
        >= 16 * thresholds.evaluation_set.query_count * thresholds.retrieval.k;
    let recall_passes = thresholds.retrieval.minimum_queries_with_hit * 100
        >= 80 * thresholds.evaluation_set.query_count;
    if !precision_passes || !recall_passes {
        return Err(ThresholdError::Retrieval);
    }

    Ok(thresholds)
}
