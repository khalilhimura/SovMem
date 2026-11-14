use serde::Serialize;

use super::{
    ledger::{LedgerEvent, LedgerView, VerdictPayload},
    proposal::Confidence,
    Policy, VerdictVerb,
};

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PerSourceMetrics {
    pub source_id: String,
    pub proposals: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub rewritten: usize,
    pub tp: usize,
    pub fp: usize,
    pub accept_rate: f64,
    pub rewrite_rate: f64,
    /// TP rate among tp+fp observations. `None` when there is no tp/fp data
    /// (F08 — missing evidence cannot produce an apparently confident success claim).
    pub tp_rate: Option<f64>,
    /// FP rate among tp+fp observations. `None` when there is no tp/fp data.
    pub fp_rate: Option<f64>,
    pub mean_confidence: f64,
    /// Coverage: how many of the verdicts had confidence data.
    pub confidence_coverage: usize,
    pub calibration_gap: f64,
    pub optimal_implied_threshold: f64,
    pub implicit_threshold: f64,
    pub threshold_gap: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct BlindspotCell {
    pub source_id: String,
    pub layer: String,
    pub doc_type: String,
    pub verdicts: usize,
    pub mean_confidence: f64,
    pub accept_rate: f64,
    pub fp_rate: f64,
    pub blindspot: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct QuotaRecommendation {
    pub source_id: String,
    pub verdicts: usize,
    pub accept_rate: f64,
    pub recommendation: String,
    pub small_n: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TrustReport {
    pub store_id: String,
    pub window_size: usize,
    pub total_verdicts: usize,
    pub sources: Vec<PerSourceMetrics>,
    pub blindspots: Vec<BlindspotCell>,
    pub quota_recommendations: Vec<QuotaRecommendation>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn optimal_threshold(policy: &Policy) -> f64 {
    let total = policy.bad_accept_cost as f64 + policy.human_verdict_cost as f64;
    if total == 0.0 {
        return 0.0;
    }
    policy.bad_accept_cost as f64 / total
}

fn confidence_as_f64(confidence: &Confidence) -> f64 {
    let millionths = confidence.millionths();
    millionths as f64 / 1_000_000.0
}

// ---------------------------------------------------------------------------
// Core computation
// ---------------------------------------------------------------------------

pub fn compute_trust(ledger: &LedgerView, policy: &Policy, window: usize) -> TrustReport {
    let threshold = optimal_threshold(policy);

    // Grab the store_id from the policy (consistent with how the rest of the
    // codebase uses it).
    let store_id = policy.store_id.clone();

    // Collect last `window` verdict events.
    let verdict_events: Vec<&VerdictPayload> = ledger
        .events
        .iter()
        .rev()
        .filter_map(|event| match event {
            LedgerEvent::Verdict(ve) => Some(ve.payload()),
            LedgerEvent::Genesis(_) => None,
        })
        .take(window)
        .collect();

    let total_verdicts = verdict_events.len();

    // Group payloads by source_id.
    let mut by_source: std::collections::BTreeMap<&str, Vec<&VerdictPayload>> =
        std::collections::BTreeMap::new();
    for payload in &verdict_events {
        by_source
            .entry(&payload.source_id)
            .or_default()
            .push(payload);
    }

    // Compute per-source metrics.
    let mut sources: Vec<PerSourceMetrics> = by_source
        .into_iter()
        .map(|(source_id, payloads)| {
            let proposals = payloads.len();
            let accepted = payloads
                .iter()
                .filter(|p| p.verb == VerdictVerb::Accept)
                .count();
            let rejected = payloads
                .iter()
                .filter(|p| p.verb == VerdictVerb::Reject)
                .count();
            let rewritten = payloads
                .iter()
                .filter(|p| p.verb == VerdictVerb::Rewrite)
                .count();
            let tp = payloads
                .iter()
                .filter(|p| p.verb == VerdictVerb::Tp)
                .count();
            let fp = payloads
                .iter()
                .filter(|p| p.verb == VerdictVerb::Fp)
                .count();

            let accept_rate = if proposals > 0 {
                accepted as f64 / proposals as f64
            } else {
                0.0
            };

            let rewrite_rate = if proposals > 0 {
                rewritten as f64 / proposals as f64
            } else {
                0.0
            };

            let tp_fp_sum = tp + fp;
            let (tp_rate, fp_rate) = if tp_fp_sum > 0 {
                (
                    Some(tp as f64 / tp_fp_sum as f64),
                    Some(fp as f64 / tp_fp_sum as f64),
                )
            } else {
                (None, None)
            };

            let conf_values: Vec<f64> = payloads
                .iter()
                .filter_map(|p| p.confidence.as_ref().map(confidence_as_f64))
                .collect();
            let confidence_coverage = conf_values.len();
            let mean_confidence = if conf_values.is_empty() {
                0.0
            } else {
                conf_values.iter().sum::<f64>() / conf_values.len() as f64
            };

            let optimal_implied_threshold = threshold;
            let implicit_threshold = accept_rate;
            let threshold_gap = optimal_implied_threshold - accept_rate;

            // Calibration gap uses accept_rate (the rate the source was
            // accepted by the human), not tp_rate (which measures accuracy
            // and may be unknown — F08).
            // When there is no confidence data, calibration gap is set to 0.0
            // to avoid producing a misleading `-accept_rate` value.
            let calibration_gap = if confidence_coverage == 0 {
                0.0
            } else {
                mean_confidence - accept_rate
            };

            PerSourceMetrics {
                source_id: source_id.to_string(),
                proposals,
                accepted,
                rejected,
                rewritten,
                tp,
                fp,
                accept_rate,
                rewrite_rate,
                tp_rate,
                fp_rate,
                mean_confidence,
                confidence_coverage,
                calibration_gap,
                optimal_implied_threshold,
                implicit_threshold,
                threshold_gap,
            }
        })
        .collect();

    // Sort by source_id for deterministic output.
    sources.sort_by(|a, b| a.source_id.cmp(&b.source_id));

    // Compute quota recommendations.
    let quota_recommendations: Vec<QuotaRecommendation> = sources
        .iter()
        .map(|s| {
            let n = s.proposals;
            let accept_rate = s.accept_rate;
            let (recommendation, small_n) = if n < 30 {
                ("insufficient-data".to_string(), true)
            } else if accept_rate > threshold {
                ("reduce-quota".to_string(), false)
            } else {
                ("maintain-quota".to_string(), false)
            };

            QuotaRecommendation {
                source_id: s.source_id.clone(),
                verdicts: n,
                accept_rate,
                recommendation,
                small_n,
            }
        })
        .collect();

    // Blindspots are deferred to a later task.
    let blindspots: Vec<BlindspotCell> = Vec::new();

    TrustReport {
        store_id,
        window_size: window,
        total_verdicts,
        sources,
        blindspots,
        quota_recommendations,
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub fn render_trust_text(report: &TrustReport) -> String {
    use std::fmt::Write;

    let mut out = String::new();

    let _ = writeln!(
        out,
        "Trust Report for store: {} (window: {})",
        report.store_id, report.window_size
    );
    let _ = writeln!(out, "Total verdicts in window: {}", report.total_verdicts);
    let _ = writeln!(out);

    if report.sources.is_empty() {
        let _ = writeln!(out, "No sources in window.\n");
        return out;
    }

    for s in &report.sources {
        let _ = writeln!(out, "--- {} ---", s.source_id);
        let _ = writeln!(out, "  proposals:        {}", s.proposals);
        let _ = writeln!(
            out,
            "  accepted:         {} (rate: {:.4})",
            s.accepted, s.accept_rate
        );
        let _ = writeln!(out, "  rejected:         {}", s.rejected);
        let _ = writeln!(
            out,
            "  rewritten:        {} (rate: {:.4})",
            s.rewritten, s.rewrite_rate
        );
        match s.tp_rate {
            Some(rate) => {
                let _ = writeln!(out, "  tp:               {} (rate: {:.4})", s.tp, rate);
            }
            None => {
                let _ = writeln!(
                    out,
                    "  tp:               {} (rate: unknown — no tp/fp data)",
                    s.tp
                );
            }
        }
        match s.fp_rate {
            Some(rate) => {
                let _ = writeln!(out, "  fp:               {} (rate: {:.4})", s.fp, rate);
            }
            None => {
                let _ = writeln!(
                    out,
                    "  fp:               {} (rate: unknown — no tp/fp data)",
                    s.fp
                );
            }
        }
        let _ = writeln!(out, "  mean_confidence:  {:.4}", s.mean_confidence);
        let _ = writeln!(
            out,
            "  confidence_coverage: {} / {}",
            s.confidence_coverage, s.proposals
        );
        let _ = writeln!(out, "  calibration_gap:  {:.4}", s.calibration_gap);
        let _ = writeln!(
            out,
            "  optimal_threshold: {:.4}",
            s.optimal_implied_threshold
        );
        let _ = writeln!(out, "  threshold_gap:    {:.4}", s.threshold_gap);
        let _ = writeln!(out);
    }

    // Quota recommendations section.
    let _ = writeln!(out, "Quota Recommendations:");
    for q in &report.quota_recommendations {
        let _ = writeln!(
            out,
            "  {}: verdicts={}, accept_rate={:.4}, recommendation={}, small_n={}",
            q.source_id, q.verdicts, q.accept_rate, q.recommendation, q.small_n
        );
    }
    let _ = writeln!(out);

    // Blindspots section (empty for now).
    let _ = writeln!(out, "Blindspots: (none — not yet implemented)");
    let _ = writeln!(out);

    out
}

pub fn render_trust_json(report: &TrustReport) -> serde_json::Value {
    serde_json::to_value(report).expect("TrustReport serialization should never fail")
}
