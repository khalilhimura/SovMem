use std::collections::BTreeMap;

use sovmem::profile::trust::{compute_trust, render_trust_json, render_trust_text};
use sovmem::profile::{
    CanonicalResult, Confidence, GenesisEvent, GenesisPayload, LedgerEvent, LedgerView, Policy,
    VerdictEvent, VerdictPayload, VerdictVerb,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn genesis_event() -> LedgerEvent {
    let payload = GenesisPayload {
        schema_version: "sovmem-ledger-event-v2".to_owned(),
        event_type: "genesis".to_owned(),
        sequence: 0,
        event_id: "evt-0000000000000000".to_owned(),
        created_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
        previous_event_hash: None,
        store_id: "audy-sovmem-writeback-v2".to_owned(),
        profile_version: "audy-ssa0-p3-v2".to_owned(),
        profile_hash: "a".repeat(64),
        policy_hash: "b".repeat(64),
        seed_manifest_hash: "c".repeat(64),
        seed_corpus_report_hash: "d".repeat(64),
        seed_bundle_hash: "e".repeat(64),
        source_commit: "f".repeat(40),
        initial_canonical_ref: "seed".to_owned(),
    };
    LedgerEvent::Genesis(Box::new(GenesisEvent::new(payload).unwrap()))
}

fn accept_verdict(source: &str, sequence: u64, prev_hash: &str) -> LedgerEvent {
    let gen_id = format!("gen-{:016}", sequence);
    let txn_id = format!("txn-{}", hex64(sequence * 3));
    let prp_id = format!("prp-{}", hex64(sequence * 5));
    let ver_id = format!("ver-{}", hex64(sequence * 7));
    let new_head = gen_id.clone();
    let payload = VerdictPayload {
        schema_version: "sovmem-ledger-event-v2".to_owned(),
        event_type: "verdict".to_owned(),
        sequence,
        event_id: format!("evt-{sequence:016}"),
        occurred_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
        previous_event_hash: Some(prev_hash.to_owned()),
        store_id: "audy-sovmem-writeback-v2".to_owned(),
        transaction_id: txn_id,
        proposal_id: prp_id,
        proposal_meta_hash: "c".repeat(64),
        candidate_hash: "d".repeat(64),
        source_id: source.to_owned(),
        verb: VerdictVerb::Accept,
        reason: "test acceptance".to_owned(),
        verdict_id: ver_id,
        actor: "src:human:khalil".to_owned(),
        policy_hash: "b".repeat(64),
        profile_hash: "a".repeat(64),
        confidence: None,
        evaluator_meta_hash: None,
        base_kind: "seed".to_owned(),
        base_generation: None,
        base_manifest_hash: "c".repeat(64),
        base_bundle_hash: "e".repeat(64),
        base_target_hash: None,
        rewrite_hash: None,
        rewrite_bytes: None,
        canonical_result: CanonicalResult {
            generation_id: Some(gen_id.clone()),
            manifest_hash: Some("a".repeat(64)),
            bundle_hash: Some("b".repeat(64)),
            target_prior_hash: None,
            target_new_hash: Some("c".repeat(64)),
            judgment_hash: Some("d".repeat(64)),
            reindex_receipt_hash: Some("e".repeat(64)),
        },
        resulting_canonical_head: new_head,
        intake_sequence: 0,
        intake_head_hash: None,
    };
    LedgerEvent::Verdict(Box::new(VerdictEvent::new(payload).unwrap()))
}

fn reject_verdict(source: &str, sequence: u64, prev_hash: &str) -> LedgerEvent {
    let txn_id = format!("txn-{}", hex64(sequence * 3));
    let prp_id = format!("prp-{}", hex64(sequence * 5));
    let ver_id = format!("ver-{}", hex64(sequence * 7));
    let payload = VerdictPayload {
        schema_version: "sovmem-ledger-event-v2".to_owned(),
        event_type: "verdict".to_owned(),
        sequence,
        event_id: format!("evt-{sequence:016}"),
        occurred_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
        previous_event_hash: Some(prev_hash.to_owned()),
        store_id: "audy-sovmem-writeback-v2".to_owned(),
        transaction_id: txn_id,
        proposal_id: prp_id,
        proposal_meta_hash: "c".repeat(64),
        candidate_hash: "d".repeat(64),
        source_id: source.to_owned(),
        verb: VerdictVerb::Reject,
        reason: "test rejection".to_owned(),
        verdict_id: ver_id,
        actor: "src:human:khalil".to_owned(),
        policy_hash: "b".repeat(64),
        profile_hash: "a".repeat(64),
        confidence: None,
        evaluator_meta_hash: None,
        base_kind: "seed".to_owned(),
        base_generation: None,
        base_manifest_hash: "c".repeat(64),
        base_bundle_hash: "e".repeat(64),
        base_target_hash: None,
        rewrite_hash: None,
        rewrite_bytes: None,
        canonical_result: CanonicalResult::none(),
        resulting_canonical_head: "seed".to_owned(),
        intake_sequence: 0,
        intake_head_hash: None,
    };
    LedgerEvent::Verdict(Box::new(VerdictEvent::new(payload).unwrap()))
}

fn accept_verdict_with_confidence(
    source: &str,
    confidence: Confidence,
    sequence: u64,
    prev_hash: &str,
) -> LedgerEvent {
    let gen_id = format!("gen-{:016}", sequence);
    let txn_id = format!("txn-{}", hex64(sequence * 3));
    let prp_id = format!("prp-{}", hex64(sequence * 5));
    let ver_id = format!("ver-{}", hex64(sequence * 7));
    let new_head = gen_id.clone();
    let payload = VerdictPayload {
        schema_version: "sovmem-ledger-event-v2".to_owned(),
        event_type: "verdict".to_owned(),
        sequence,
        event_id: format!("evt-{sequence:016}"),
        occurred_at: "2026-07-14T04:30:00.000000000Z".to_owned(),
        previous_event_hash: Some(prev_hash.to_owned()),
        store_id: "audy-sovmem-writeback-v2".to_owned(),
        transaction_id: txn_id,
        proposal_id: prp_id,
        proposal_meta_hash: "c".repeat(64),
        candidate_hash: "d".repeat(64),
        source_id: source.to_owned(),
        verb: VerdictVerb::Accept,
        reason: "test acceptance with confidence".to_owned(),
        verdict_id: ver_id,
        actor: "src:human:khalil".to_owned(),
        policy_hash: "b".repeat(64),
        profile_hash: "a".repeat(64),
        confidence: Some(confidence),
        evaluator_meta_hash: None,
        base_kind: "seed".to_owned(),
        base_generation: None,
        base_manifest_hash: "c".repeat(64),
        base_bundle_hash: "e".repeat(64),
        base_target_hash: None,
        rewrite_hash: None,
        rewrite_bytes: None,
        canonical_result: CanonicalResult {
            generation_id: Some(gen_id.clone()),
            manifest_hash: Some("a".repeat(64)),
            bundle_hash: Some("b".repeat(64)),
            target_prior_hash: None,
            target_new_hash: Some("c".repeat(64)),
            judgment_hash: Some("d".repeat(64)),
            reindex_receipt_hash: Some("e".repeat(64)),
        },
        resulting_canonical_head: new_head,
        intake_sequence: 0,
        intake_head_hash: None,
    };
    LedgerEvent::Verdict(Box::new(VerdictEvent::new(payload).unwrap()))
}

fn ledger_from_events(events: Vec<LedgerEvent>) -> LedgerView {
    let verdicts = events.len().saturating_sub(1);
    let final_hash = events.last().map(|e| e.hash().to_owned());
    let (accept, reject, rewrite, tp, fp) = events
        .iter()
        .filter_map(|e| {
            if let LedgerEvent::Verdict(v) = e {
                Some(v.payload())
            } else {
                None
            }
        })
        .fold((0, 0, 0, 0, 0), |(a, r, w, t, f), v| match v.verb {
            VerdictVerb::Accept => (a + 1, r, w, t, f),
            VerdictVerb::Reject => (a, r + 1, w, t, f),
            VerdictVerb::Rewrite => (a, r, w + 1, t, f),
            VerdictVerb::Tp => (a, r, w, t + 1, f),
            VerdictVerb::Fp => (a, r, w, t, f + 1),
        });
    let mut terminal = BTreeMap::new();
    for e in &events {
        if let LedgerEvent::Verdict(v) = e {
            terminal.insert(
                v.payload().proposal_id.clone(),
                v.payload().verdict_id.clone(),
            );
        }
    }
    let applied_heads: Vec<String> = events
        .iter()
        .filter_map(|e| {
            if let LedgerEvent::Verdict(v) = e {
                if v.payload().verb.applies() {
                    return Some(v.payload().resulting_canonical_head.clone());
                }
            }
            None
        })
        .collect();

    LedgerView {
        events,
        terminal_proposals: terminal,
        verdicts,
        accept,
        reject,
        rewrite,
        tp,
        fp,
        applied_heads,
        final_hash,
    }
}

fn policy() -> Policy {
    Policy {
        schema_version: "sovmem-policy-v1".to_owned(),
        store_id: "audy-sovmem-writeback-v2".to_owned(),
        profile_version: "audy-ssa0-p3-v2".to_owned(),
        human_actor: "src:human:khalil".to_owned(),
        allowed_proposer_prefixes: vec!["src:human:".to_owned(), "src:agent:".to_owned()],
        allowed_evaluator_prefixes: vec!["src:agent:".to_owned()],
        evaluator_protocol: "evaluator-v2".to_owned(),
        allowed_targets: vec!["**/*.md".to_owned()],
        protected_layers: vec!["synthesis".to_owned()],
        forbidden_proposal_types: vec!["judgment".to_owned()],
        provenance_values: vec![
            "tier1".to_owned(),
            "secondary".to_owned(),
            "synthesis".to_owned(),
            "unknown".to_owned(),
        ],
        max_candidate_bytes: 100_000,
        max_meta_bytes: 10_000,
        max_rewrite_bytes: 50_000,
        max_reason_bytes: 5_000,
        max_evaluator_reasons: 5,
        max_evaluator_reason_bytes: 2_000,
        max_open_per_source: 5,
        max_submissions_per_source_utc_day: 20,
        max_open_total: 50,
        queue_alarm_at: 40,
        bad_accept_cost: 100,
        human_verdict_cost: 1,
    }
}

/// Produce a 64-hex-char string from a u64 value.
fn hex64(value: u64) -> String {
    format!("{:064x}", value)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn empty_ledger_produces_empty_trust_report() {
    let events: Vec<LedgerEvent> = Vec::new();
    let view = ledger_from_events(events);
    let p = policy();
    let report = compute_trust(&view, &p, 50);

    assert!(report.sources.is_empty());
    assert_eq!(report.total_verdicts, 0);
    assert!(report.blindspots.is_empty());
    assert!(report.quota_recommendations.is_empty());
}

#[test]
fn single_source_single_verdict_produces_correct_rates() {
    let genesis = genesis_event();
    let reject = reject_verdict("src:human:khalil", 1, genesis.hash());
    let events = vec![genesis, reject];
    let view = ledger_from_events(events);
    let p = policy();
    let report = compute_trust(&view, &p, 50);

    assert_eq!(report.total_verdicts, 1);
    assert_eq!(report.sources.len(), 1);
    let metrics = &report.sources[0];
    assert_eq!(metrics.source_id, "src:human:khalil");
    assert_eq!(metrics.proposals, 1);
    assert_eq!(metrics.accepted, 0);
    assert_eq!(metrics.rejected, 1);
    assert_eq!(metrics.accept_rate, 0.0);
    // No tp/fp data → rates are None (F08)
    assert_eq!(metrics.fp_rate, None);
    assert_eq!(metrics.tp_rate, None);
    assert_eq!(metrics.confidence_coverage, 0);
}

#[test]
fn multiple_sources_have_independent_metrics() {
    let genesis = genesis_event();
    let mut prev = genesis.hash().to_owned();
    let mut events = vec![genesis];

    // Source A: 2 accepts at sequences 1, 3
    for seq in [1u64, 3] {
        let evt = accept_verdict("src:agent:a", seq, &prev);
        prev = evt.hash().to_owned();
        events.push(evt);
    }
    // Source B: 1 reject at sequence 2
    {
        let evt = reject_verdict("src:agent:b", 2, &prev);
        prev = evt.hash().to_owned();
        events.push(evt);
    }
    // Source A: 1 accept at sequence 4
    let evt = accept_verdict("src:agent:a", 4, &prev);
    events.push(evt);

    let view = ledger_from_events(events);
    let p = policy();
    let report = compute_trust(&view, &p, 50);

    // Sorted by source_id: "src:agent:a" < "src:agent:b"
    assert_eq!(report.sources.len(), 2);
    let a = &report.sources[0];
    let b = &report.sources[1];

    assert_eq!(a.source_id, "src:agent:a");
    assert_eq!(a.proposals, 3);
    assert_eq!(a.accepted, 3);
    assert_eq!(a.rejected, 0);
    assert!((a.accept_rate - 1.0).abs() < 1e-12);

    assert_eq!(b.source_id, "src:agent:b");
    assert_eq!(b.proposals, 1);
    assert_eq!(b.accepted, 0);
    assert_eq!(b.rejected, 1);
    assert!((b.accept_rate - 0.0).abs() < 1e-12);
}

#[test]
fn calibration_gap_is_confidence_minus_accept_rate() {
    let genesis = genesis_event();
    let confidence = Confidence::parse("0.8").unwrap();
    let evt = accept_verdict_with_confidence("src:human:khalil", confidence, 1, genesis.hash());
    let events = vec![genesis, evt];
    let view = ledger_from_events(events);
    let p = policy();
    let report = compute_trust(&view, &p, 50);

    assert_eq!(report.sources.len(), 1);
    let metrics = &report.sources[0];
    // 1 accept / 1 proposal = 1.0 accept_rate
    assert!((metrics.accept_rate - 1.0).abs() < 1e-12);
    // Confidence 0.8, mean_confidence 0.8
    assert!((metrics.mean_confidence - 0.8).abs() < 1e-12);
    // calibration_gap = mean_confidence - accept_rate = 0.8 - 1.0 = -0.2
    assert!((metrics.calibration_gap - (-0.2)).abs() < 1e-12);
}

#[test]
fn optimal_threshold_matches_d11_ratio() {
    let genesis = genesis_event();
    let evt = accept_verdict("src:human:khalil", 1, genesis.hash());
    let events = vec![genesis, evt];
    let view = ledger_from_events(events);
    let p = policy();
    let report = compute_trust(&view, &p, 50);

    assert_eq!(report.sources.len(), 1);
    let expected = 100.0 / (100.0 + 1.0); // bad_accept_cost / (bad_accept_cost + human_verdict_cost)
    let actual = report.sources[0].optimal_implied_threshold;
    assert!((actual - expected).abs() < 1e-12);
}

#[test]
fn rolling_window_truncates_older_verdicts() {
    let window = 50;
    let genesis = genesis_event();
    let mut events = vec![genesis];
    let mut prev = events[0].hash().to_owned();
    // Create 60 verdicts (sequences 1..=60)
    for seq in 1u64..=60 {
        let evt = reject_verdict("src:human:khalil", seq, &prev);
        prev = evt.hash().to_owned();
        events.push(evt);
    }
    let view = ledger_from_events(events);
    let p = policy();
    let report = compute_trust(&view, &p, window);

    // Only the last `window` verdicts should be tracked
    assert_eq!(report.total_verdicts, window);
    assert_eq!(report.window_size, window);
    assert_eq!(report.sources.len(), 1);
    let metrics = &report.sources[0];
    assert_eq!(metrics.proposals, window);
}

#[test]
fn small_n_guard_suppresses_recommendation_below_30() {
    let genesis = genesis_event();
    let mut events = vec![genesis];
    let mut prev = events[0].hash().to_owned();
    // 29 verdicts (sequences 1..=29)
    for seq in 1u64..=29 {
        let evt = reject_verdict("src:human:khalil", seq, &prev);
        prev = evt.hash().to_owned();
        events.push(evt);
    }
    let view = ledger_from_events(events);
    let p = policy();
    let report = compute_trust(&view, &p, 50);

    assert_eq!(report.sources.len(), 1);
    assert_eq!(report.quota_recommendations.len(), 1);
    let rec = &report.quota_recommendations[0];
    assert!(rec.small_n);
    assert_eq!(rec.recommendation, "insufficient-data");
    assert_eq!(rec.verdicts, 29);
}

#[test]
fn small_n_guard_lifts_at_30_verdicts() {
    let genesis = genesis_event();
    let mut events = vec![genesis];
    let mut prev = events[0].hash().to_owned();
    // 30 verdicts (sequences 1..=30)
    for seq in 1u64..=30 {
        let evt = reject_verdict("src:human:khalil", seq, &prev);
        prev = evt.hash().to_owned();
        events.push(evt);
    }
    let view = ledger_from_events(events);
    let p = policy();
    let report = compute_trust(&view, &p, 50);

    assert_eq!(report.sources.len(), 1);
    assert_eq!(report.quota_recommendations.len(), 1);
    let rec = &report.quota_recommendations[0];
    // accept_rate is 0.0 (all rejects), so threshold check: 0.0 <= threshold -> maintain-quota
    assert!(!rec.small_n);
    assert_eq!(rec.recommendation, "maintain-quota");
    assert_eq!(rec.verdicts, 30);
}

#[test]
fn trust_report_renders_as_text() {
    let genesis = genesis_event();
    let evt = accept_verdict("src:human:khalil", 1, genesis.hash());
    let events = vec![genesis, evt];
    let view = ledger_from_events(events);
    let p = policy();
    let report = compute_trust(&view, &p, 50);
    let text = render_trust_text(&report);

    assert!(text.contains("src:human:khalil"));
    assert!(text.contains("proposals:"));
    assert!(text.contains("accept_rate"));
    assert!(text.contains("Trust Report for store:"));
}

#[test]
fn trust_report_renders_as_json() {
    let genesis = genesis_event();
    let evt = accept_verdict("src:human:khalil", 1, genesis.hash());
    let events = vec![genesis, evt];
    let view = ledger_from_events(events);
    let p = policy();
    let report = compute_trust(&view, &p, 50);
    let json = render_trust_json(&report);

    assert!(json.is_object());
    assert!(json.get("sources").unwrap().is_array());
    assert!(json.get("store_id").is_some());
    assert!(json.get("window_size").is_some());
}
