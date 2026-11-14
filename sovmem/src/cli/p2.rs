use super::{emit_diagnostics, print_json, ExpansionArg, OutputFormat};
use crate::profile as sovmem_profile;
use serde::Serialize;
use sovmem_profile::{
    build_index, evaluate, query_index, Direction, EvalError, EvalOutcome, EvalRequest,
    ExpansionMode, GoldenError, IndexError, IndexRequest, IndexSummary, Q3Decision, QueryError,
    QueryRequest, QueryResult, ThresholdError, VerificationDiagnostic, INDEX_MANIFEST_SCHEMA,
};
use std::path::PathBuf;

#[derive(Serialize)]
struct IndexOutput<'a> {
    status: &'static str,
    command: &'static str,
    schema_version: &'static str,
    documents: usize,
    graph_nodes: usize,
    graph_edges: usize,
    external_typed_links: usize,
    bundle_hash: &'a str,
    index_config: &'a str,
    replaced_invalid_current: bool,
}

#[derive(Serialize)]
struct QueryOutput<'a> {
    status: &'static str,
    command: &'static str,
    top_k: usize,
    expansion: ExpansionMode,
    bundle_hash: &'a str,
    results: &'a [sovmem_profile::QueryHit],
    expanded: &'a [sovmem_profile::ExpandedHit],
}

#[derive(Serialize)]
struct EvalOutput<'a> {
    status: Q3Decision,
    command: &'static str,
    evaluation_set: &'a str,
    queries: usize,
    k: usize,
    queries_with_hit: usize,
    required_queries_with_hit: usize,
    precision_numerator: usize,
    precision_denominator: usize,
    recall_numerator: usize,
    recall_denominator: usize,
    baseline_written: bool,
}

pub(super) fn run_index(
    profile_path: PathBuf,
    bundle_root: PathBuf,
    index_root: PathBuf,
    format: OutputFormat,
) -> i32 {
    match build_index(IndexRequest {
        profile_path,
        bundle_root,
        index_root,
    }) {
        Ok(summary) => {
            match format {
                OutputFormat::Json => render_index_json(&summary),
                OutputFormat::Text => render_index_text(&summary),
            }
            0
        }
        Err(error) => {
            let exit = index_error_exit(&error);
            emit_diagnostics(index_error_diagnostics(&error));
            exit
        }
    }
}

pub(super) fn run_query(
    profile_path: PathBuf,
    bundle_root: PathBuf,
    index_root: PathBuf,
    query: String,
    top_k: usize,
    expansion: ExpansionArg,
    format: OutputFormat,
) -> i32 {
    let expansion = match expansion {
        ExpansionArg::None => ExpansionMode::None,
        ExpansionArg::OneHop => ExpansionMode::OneHop,
    };
    match query_index(QueryRequest {
        profile_path,
        bundle_root,
        index_root,
        query,
        top_k,
        expansion,
    }) {
        Ok(result) => {
            match format {
                OutputFormat::Json => render_query_json(&result),
                OutputFormat::Text => render_query_text(&result),
            }
            0
        }
        Err(error) => {
            let exit = query_error_exit(&error);
            emit_diagnostics(query_error_diagnostics(&error));
            exit
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_eval(
    profile_path: PathBuf,
    bundle_root: PathBuf,
    index_root: PathBuf,
    golden_path: PathBuf,
    thresholds_path: PathBuf,
    baseline_path: PathBuf,
    format: OutputFormat,
) -> i32 {
    match evaluate(EvalRequest {
        profile_path,
        bundle_root,
        index_root,
        golden_path,
        thresholds_path,
        baseline_path,
    }) {
        Ok(outcome) => {
            match format {
                OutputFormat::Json => render_eval_json(&outcome),
                OutputFormat::Text => render_eval_text(&outcome),
            }
            match outcome.decision {
                Q3Decision::Go => 0,
                Q3Decision::NoGo => {
                    emit_diagnostics(vec![VerificationDiagnostic::error(
                        "eval.q3_no_go",
                        "frozen Q3 retrieval gate did not pass",
                    )]);
                    1
                }
            }
        }
        Err(error) => {
            let exit = eval_error_exit(&error);
            emit_diagnostics(eval_error_diagnostics(&error));
            exit
        }
    }
}

fn render_index_json(summary: &IndexSummary) {
    print_json(&IndexOutput {
        status: "ok",
        command: "index",
        schema_version: INDEX_MANIFEST_SCHEMA,
        documents: summary.documents,
        graph_nodes: summary.graph_nodes,
        graph_edges: summary.graph_edges,
        external_typed_links: summary.external_typed_links,
        bundle_hash: &summary.bundle_hash,
        index_config: &summary.index_config,
        replaced_invalid_current: summary.replaced_invalid_current,
    });
}

fn render_index_text(summary: &IndexSummary) {
    println!("status: ok");
    println!("command: index");
    println!("documents: {}", summary.documents);
    println!("graph-nodes: {}", summary.graph_nodes);
    println!("graph-edges: {}", summary.graph_edges);
    println!("external-typed-links: {}", summary.external_typed_links);
    println!("bundle-hash: {}", summary.bundle_hash);
    println!("index-config: {}", summary.index_config);
    println!(
        "replaced-invalid-current: {}",
        summary.replaced_invalid_current
    );
}

fn render_query_json(result: &QueryResult) {
    print_json(&QueryOutput {
        status: "ok",
        command: "query",
        top_k: result.top_k,
        expansion: result.expansion,
        bundle_hash: &result.bundle_hash,
        results: &result.results,
        expanded: &result.expanded,
    });
}

fn render_query_text(result: &QueryResult) {
    println!("status: ok");
    println!("command: query");
    println!("top-k: {}", result.top_k);
    println!("expansion: {}", expansion_label(result.expansion));
    println!("bundle-hash: {}", result.bundle_hash);
    for hit in &result.results {
        println!(
            "bm25[{}]: id={} score={} title={}",
            hit.rank,
            hit.id,
            hit.score,
            serde_json::to_string(&hit.title).expect("title JSON")
        );
    }
    for hit in &result.expanded {
        println!(
            "expanded[{}]: id={} via={} direction={} relation={} title={}",
            hit.rank,
            hit.id,
            hit.via_id,
            direction_label(&hit.direction),
            hit.relation,
            serde_json::to_string(&hit.title).expect("title JSON")
        );
    }
}

fn render_eval_json(outcome: &EvalOutcome) {
    print_json(&EvalOutput {
        status: outcome.decision,
        command: "eval",
        evaluation_set: &outcome.evaluation_set,
        queries: outcome.queries,
        k: outcome.k,
        queries_with_hit: outcome.queries_with_hit,
        required_queries_with_hit: outcome.required_queries_with_hit,
        precision_numerator: outcome.precision_numerator,
        precision_denominator: outcome.precision_denominator,
        recall_numerator: outcome.recall_numerator,
        recall_denominator: outcome.recall_denominator,
        baseline_written: outcome.baseline_written,
    });
}

fn render_eval_text(outcome: &EvalOutcome) {
    let decision = match outcome.decision {
        Q3Decision::Go => "GO",
        Q3Decision::NoGo => "NO-GO",
    };
    println!("Q3: {decision}");
    println!("evaluation-set: {}", outcome.evaluation_set);
    println!("queries: {}", outcome.queries);
    println!("k: {}", outcome.k);
    println!("hits: {}/{}", outcome.queries_with_hit, outcome.queries);
    println!(
        "required-hits: {}/{}",
        outcome.required_queries_with_hit, outcome.queries
    );
    println!(
        "macro-precision-at-5: {}/{}",
        outcome.precision_numerator, outcome.precision_denominator
    );
    println!(
        "macro-recall-at-5: {}/{}",
        outcome.recall_numerator, outcome.recall_denominator
    );
    println!("baseline-written: {}", outcome.baseline_written);
    if outcome.missed_query_ids.is_empty() {
        println!("missed-query-ids: none");
    } else {
        println!("missed-query-ids: {}", outcome.missed_query_ids.join(","));
    }
}

fn expansion_label(expansion: ExpansionMode) -> &'static str {
    match expansion {
        ExpansionMode::None => "none",
        ExpansionMode::OneHop => "one-hop",
    }
}

fn direction_label(direction: &Direction) -> &'static str {
    match direction {
        Direction::Outgoing => "outgoing",
        Direction::Incoming => "incoming",
    }
}

fn index_error_exit(error: &IndexError) -> i32 {
    match error {
        IndexError::Profile(_)
        | IndexError::BundleRead
        | IndexError::OutputInvalid
        | IndexError::Build
        | IndexError::PublishUnsupported
        | IndexError::Publish => 2,
        IndexError::BundleUnverified(_)
        | IndexError::SelfCheck
        | IndexError::CurrentInvalid
        | IndexError::ManifestInvalid
        | IndexError::BundleMismatch
        | IndexError::SchemaMismatch
        | IndexError::DocumentSetInvalid
        | IndexError::GraphInvalid
        | IndexError::TantivyInvalid
        | IndexError::P3CanonUnverified
        | IndexError::InputMismatch
        | IndexError::ReceiptInvalid
        | IndexError::StoreUnverified(_)
        | IndexError::InputStale => 1,
    }
}

fn query_error_exit(error: &QueryError) -> i32 {
    match error {
        QueryError::Index(error) => index_error_exit(error),
        QueryError::Invalid | QueryError::Empty | QueryError::TooManyTokens => 2,
        QueryError::Execute => 1,
    }
}

fn eval_error_exit(error: &EvalError) -> i32 {
    match error {
        EvalError::Index(error) => index_error_exit(error),
        EvalError::Query(error) => query_error_exit(error),
        EvalError::Golden(GoldenError::Read) | EvalError::Threshold(ThresholdError::Read) => 2,
        EvalError::Golden(_) | EvalError::Threshold(_) => 1,
        EvalError::BaselineOutputInvalid | EvalError::Write => 2,
        // Verified data that failed a gate, not a usage or input-loading fault.
        EvalError::CorpusSplitInvalid => 1,
    }
}

fn index_error_diagnostics(error: &IndexError) -> Vec<VerificationDiagnostic> {
    let mut diagnostics = vec![VerificationDiagnostic::error(
        error.code(),
        "derived index operation failed",
    )];
    if let IndexError::BundleUnverified(verification) = error {
        diagnostics.extend(verification.iter().cloned());
    }
    diagnostics
}

fn query_error_diagnostics(error: &QueryError) -> Vec<VerificationDiagnostic> {
    let mut diagnostics = vec![VerificationDiagnostic::error(
        error.code(),
        "query operation failed",
    )];
    if let QueryError::Index(IndexError::BundleUnverified(verification)) = error {
        diagnostics.extend(verification.iter().cloned());
    }
    diagnostics
}

fn eval_error_diagnostics(error: &EvalError) -> Vec<VerificationDiagnostic> {
    let mut diagnostics = vec![VerificationDiagnostic::error(
        error.code(),
        "evaluation operation failed",
    )];
    match error {
        EvalError::Index(IndexError::BundleUnverified(verification))
        | EvalError::Query(QueryError::Index(IndexError::BundleUnverified(verification))) => {
            diagnostics.extend(verification.iter().cloned());
        }
        _ => {}
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{EvalError, GoldenError, IndexError, QueryError, ThresholdError};

    #[test]
    fn every_index_code_has_the_frozen_exit_class() {
        let cases = [
            (IndexError::BundleUnverified(Vec::new()), 1),
            (IndexError::OutputInvalid, 2),
            (IndexError::Build, 2),
            (IndexError::SelfCheck, 1),
            (IndexError::PublishUnsupported, 2),
            (IndexError::Publish, 2),
            (IndexError::CurrentInvalid, 1),
            (IndexError::ManifestInvalid, 1),
            (IndexError::BundleMismatch, 1),
            (IndexError::SchemaMismatch, 1),
            (IndexError::DocumentSetInvalid, 1),
            (IndexError::GraphInvalid, 1),
            (IndexError::TantivyInvalid, 1),
        ];
        for (error, exit) in cases {
            assert_eq!(index_error_exit(&error), exit, "{}", error.code());
        }
    }

    #[test]
    fn every_query_code_has_the_frozen_exit_class() {
        let cases = [
            (QueryError::Invalid, 2),
            (QueryError::Empty, 2),
            (QueryError::TooManyTokens, 2),
            (QueryError::Execute, 1),
            (QueryError::Index(IndexError::CurrentInvalid), 1),
            (QueryError::Index(IndexError::OutputInvalid), 2),
        ];
        for (error, exit) in cases {
            assert_eq!(query_error_exit(&error), exit, "{}", error.code());
        }
    }

    #[test]
    fn every_eval_code_has_the_frozen_exit_class() {
        let cases = [
            (EvalError::Index(IndexError::CurrentInvalid), 1),
            (EvalError::Index(IndexError::OutputInvalid), 2),
            (EvalError::Query(QueryError::Invalid), 2),
            (EvalError::Query(QueryError::Execute), 1),
            (EvalError::Golden(GoldenError::Read), 2),
            (EvalError::Golden(GoldenError::Parse), 1),
            (EvalError::Threshold(ThresholdError::Read), 2),
            (EvalError::Threshold(ThresholdError::Parse), 1),
            (EvalError::BaselineOutputInvalid, 2),
            (EvalError::Write, 2),
            (EvalError::CorpusSplitInvalid, 1),
        ];
        for (error, exit) in cases {
            assert_eq!(eval_error_exit(&error), exit, "{}", error.code());
        }
    }
}
