// Integration test file for p3_internal tests.
// These tests were extracted from sovmem/src/profile/p3_internal/ to run as
// a single integration test binary, avoiding concurrent filesystem/Tantivy
// pollution when running under cargo's default parallel test runner.
//
// Each source file is wrapped in its own module to prevent name collisions.
// Imports have been rewritten from crate-private paths to public integration-test paths.

/// Shared helper functions available to all sub-modules via `super::`.
#[allow(dead_code, unused_imports)]
fn repository_path(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative)
}

#[allow(dead_code)]
fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
    let metadata = std::fs::symlink_metadata(source).expect("read test source metadata");
    assert!(
        !metadata.file_type().is_symlink(),
        "test source cannot be a symlink"
    );
    if metadata.is_file() {
        std::fs::write(
            destination,
            std::fs::read(source).expect("read test source file"),
        )
        .expect("copy test source file");
        return;
    }
    assert!(metadata.is_dir(), "test source is a directory");
    std::fs::create_dir(destination).expect("create copied test directory");
    let mut entries = std::fs::read_dir(source)
        .expect("read test source directory")
        .collect::<Result<Vec<_>, _>>()
        .expect("read test source entries");
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        copy_tree(&entry.path(), &destination.join(entry.file_name()));
    }
}

pub mod advisory_erasure_tests {
    use sovmem::profile::p3_test_support::{FixedClock, ProvisionedP3Fixture, ScriptedHuman};
    use sovmem::profile::transaction::{
        apply_human_verdict, preflight_human_verdict_with_human, read_preflight_state,
        seal_intake_submission_with_human, HumanVerdictRequest, PreflightVerdict,
        TransactionRuntime,
    };
    use sovmem::profile::{
        evaluator_commitment, query_p3_generation, query_store, read_ledger, rebuild_store_index,
        verify_generation, verify_store_level4, ExpansionMode, StoreIndexRequest,
        StoreQueryRequest,
    };
    use sovmem::profile::{
        inspect_store, load_p3_profile, parse_source_document, prepare_proposal,
        read_sealed_pair_structural, render_document, submit_proposal_with_clock, Confidence,
        EvaluatorBlock, GeneratedDocument, GeneratedMetadata, IntakeHead, IntakePrefix,
        PolicyContext, PolicyEvaluator, ProposalBase, ProposalDraft, ProposalIntent,
        ProposalOperation, ProposalSubmission, RewriteBytes, SourceDocument, Timestamp,
        VerdictVerb, VerificationLevel,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::Path;

    const ADVISORY_SENTINEL: &str = "zztask17advisoryevidenceq9";
    const CONFIDENCE_LITERAL: &str = "0.963217";
    const CONFIDENCE_TOKEN: &str = "963217";
    const EVALUATOR_SOURCE: &str = "src:agent:reviewer963217";
    const EVALUATOR_SOURCE_TOKEN: &str = "reviewer963217";

    fn staged_pair(
        fixture: &ProvisionedP3Fixture,
        intent: ProposalIntent,
        confidence: Option<Confidence>,
        recommendation: Option<&str>,
    ) -> sovmem::profile::ProposalPair {
        let candidate_bytes = advisory_candidate_bytes(fixture);
        let candidate_path = fixture_input(fixture, "candidate", &candidate_bytes);
        let candidate_hash = blake3::hash(&candidate_bytes).to_hex().to_string();
        let evaluator_path = recommendation.map(|recommendation| {
            fixture_input(
                fixture,
                "structurally-valid-evaluator",
                &serde_json::to_vec(&EvaluatorBlock {
                    protocol: "adversarial-bug-finding-v1".to_owned(),
                    source_id: EVALUATOR_SOURCE.to_owned(),
                    evaluated_at: "2026-07-14T04:29:00.000000000Z".to_owned(),
                    candidate_hash,
                    recommendation: recommendation.to_owned(),
                    reasons: vec![ADVISORY_SENTINEL.to_owned()],
                })
                .unwrap(),
            )
        });
        let submitted = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:fixture".to_owned(),
                operation: ProposalOperation::Create,
                intent,
                target_id: "ssa-0/04-Research/CLI/README.md".to_owned(),
                target_path: "04-Research/CLI/README.md".to_owned(),
                candidate_path,
                confidence,
                evaluator_path,
            },
            &FixedClock(Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap()),
        )
        .expect("submit structurally valid proposal");
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let sealed = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-14T04:30:01.000000000Z").unwrap()),
        )
        .expect("seal structurally valid proposal");
        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
        let store = inspect_store(&profile, &fixture.store_root).expect("inspect fixture store");
        read_sealed_pair_structural(
            &store.paths,
            sealed.proposal_id.as_deref().expect("staged proposal id"),
        )
        .expect("reload canonically staged proposal")
    }

    fn protected_policy_pair(
        fixture: &ProvisionedP3Fixture,
        confidence: Option<Confidence>,
        recommendation: Option<&str>,
    ) -> sovmem::profile::ProposalPair {
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = inspect_store(&profile, &fixture.store_root).unwrap();
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target = store.paths.seed.join("bundle").join(target_path);
        let candidate_bytes = String::from_utf8(fs::read(&target).unwrap())
            .unwrap()
            .replace(
                "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog",
                "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
            )
            .into_bytes();
        let candidate_hash = blake3::hash(&candidate_bytes).to_hex().to_string();
        prepare_proposal(ProposalDraft {
            submitted_at: Timestamp::parse("2026-07-21T06:00:00.000000000Z").unwrap(),
            source_id: "src:agent:advisory-policy".to_owned(),
            operation: ProposalOperation::Replace,
            intent: ProposalIntent::Accept,
            target_id: "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"
                .to_owned(),
            target_path: target_path.to_owned(),
            base: ProposalBase {
                kind: "seed".to_owned(),
                generation_id: None,
                bundle_hash: store.seed.bundle_hash,
                manifest_hash: store.seed.manifest_hash,
                target_hash: Some(
                    blake3::hash(&fs::read(target).unwrap())
                        .to_hex()
                        .to_string(),
                ),
            },
            candidate_bytes,
            policy_hash: store.policy.hash,
            profile_hash: profile.hash,
            confidence,
            evaluator: recommendation.map(|recommendation| EvaluatorBlock {
                protocol: "adversarial-bug-finding-v1".to_owned(),
                source_id: EVALUATOR_SOURCE.to_owned(),
                evaluated_at: "2026-07-21T05:59:00.000000000Z".to_owned(),
                candidate_hash,
                recommendation: recommendation.to_owned(),
                reasons: vec![ADVISORY_SENTINEL.to_owned()],
            }),
        })
        .unwrap()
    }

    fn protected_policy_trace(
        fixture: &ProvisionedP3Fixture,
        proposal: &sovmem::profile::ProposalPair,
    ) -> (bool, Vec<u8>) {
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = inspect_store(&profile, &fixture.store_root).unwrap();
        let intake = IntakePrefix {
            head: IntakeHead {
                sequence: 0,
                event_hash: None,
            },
            staged: Vec::new(),
            rejected: Vec::new(),
        };
        let decision = PolicyEvaluator::evaluate(&PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal,
            intake: &intake,
        });
        (
            decision.passed,
            serde_json::to_vec(&decision.rule_codes).unwrap(),
        )
    }

    #[test]
    fn advisory_erasure_preserves_policy_trace_after_structural_validation() {
        let absent_fixture = ProvisionedP3Fixture::empty("task-17-advisory-trace-absent");
        let absent = protected_policy_pair(&absent_fixture, None, None);
        let expected = protected_policy_trace(&absent_fixture, &absent);
        assert!(
            expected.0,
            "the canonical absent-advisory pair must be accepted"
        );

        for (name, confidence, recommendation) in [
            (
                "looks-sound",
                Some(Confidence::parse(CONFIDENCE_LITERAL).unwrap()),
                Some("looks-sound"),
            ),
            (
                "needs-rewrite",
                Some(Confidence::parse("1").unwrap()),
                Some("needs-rewrite"),
            ),
            (
                "reject",
                Some(Confidence::parse("0").unwrap()),
                Some("reject"),
            ),
        ] {
            let fixture = ProvisionedP3Fixture::empty(&format!("task-17-advisory-trace-{name}"));
            let present = protected_policy_pair(&fixture, confidence, recommendation);
            assert_eq!(protected_policy_trace(&fixture, &present), expected);
        }

        let mut forged_confidence = absent.clone();
        forged_confidence.meta_bytes = forged_confidence
            .meta_bytes
            .windows(b"\"confidence\":null".len())
            .position(|window| window == b"\"confidence\":null")
            .map(|offset| {
                let mut bytes = forged_confidence.meta_bytes.clone();
                bytes.splice(
                    offset..offset + b"\"confidence\":null".len(),
                    b"\"confidence\":0.1234567".iter().copied(),
                );
                bytes
            })
            .expect("canonical absent confidence field");
        forged_confidence.meta_hash = blake3::hash(&forged_confidence.meta_bytes)
            .to_hex()
            .to_string();
        assert_eq!(
            protected_policy_trace(&absent_fixture, &forged_confidence),
            (false, br#"["proposal.meta_parse"]"#.to_vec())
        );

        let forged_fixture = ProvisionedP3Fixture::empty("task-17-advisory-forged-evaluator");
        let mut forged_evaluator = protected_policy_pair(
            &forged_fixture,
            Some(Confidence::parse(CONFIDENCE_LITERAL).unwrap()),
            Some("looks-sound"),
        );
        forged_evaluator.candidate_bytes = b"not a profile-valid candidate".to_vec();
        forged_evaluator.candidate_hash = blake3::hash(&forged_evaluator.candidate_bytes)
            .to_hex()
            .to_string();
        forged_evaluator.meta.candidate_bytes = forged_evaluator.candidate_bytes.len();
        forged_evaluator.meta.candidate_hash = forged_evaluator.candidate_hash.clone();
        forged_evaluator.meta.proposal_id = sovmem::profile::proposal_id(&forged_evaluator.meta);
        forged_evaluator.meta_bytes = serde_json::to_vec(&forged_evaluator.meta).unwrap();
        forged_evaluator.meta_bytes.push(b'\n');
        forged_evaluator.meta_hash = blake3::hash(&forged_evaluator.meta_bytes)
            .to_hex()
            .to_string();
        assert_eq!(
            protected_policy_trace(&forged_fixture, &forged_evaluator),
            (false, br#"["proposal.evaluator_hash_mismatch"]"#.to_vec())
        );
    }

    fn collect_hashes(root: &Path, path: &Path, hashes: &mut BTreeMap<String, String>) {
        let mut entries = fs::read_dir(path)
            .expect("read fixture artifact directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect fixture artifact entries");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("read fixture artifact metadata");
            if metadata.is_dir() {
                collect_hashes(root, &path, hashes);
            } else if metadata.is_file() {
                hashes.insert(
                    path.strip_prefix(root)
                        .expect("fixture artifact remains below root")
                        .to_string_lossy()
                        .replace('\\', "/"),
                    blake3::hash(&fs::read(path).expect("read fixture artifact"))
                        .to_hex()
                        .to_string(),
                );
            }
        }
    }

    fn artifact_hashes(root: &Path) -> BTreeMap<String, String> {
        let mut hashes = BTreeMap::new();
        collect_hashes(root, root, &mut hashes);
        hashes
    }

    fn apply_with_scripted_human(
        fixture: &ProvisionedP3Fixture,
        proposal_id: &str,
        verb: VerdictVerb,
    ) -> sovmem::profile::transaction::VerdictOutcome {
        let profile = load_p3_profile(&fixture.profile_path).expect("load advisory profile");
        let store = inspect_store(&profile, &fixture.store_root).expect("inspect advisory store");
        let pair = read_sealed_pair_structural(&store.paths, proposal_id)
            .expect("read exact advisory proposal for human verdict");
        let rewrite = (verb == VerdictVerb::Rewrite).then(|| {
            RewriteBytes::new(pair.candidate_bytes, store.policy.policy.max_rewrite_bytes)
                .expect("construct valid exact human rewrite")
        });
        let request = HumanVerdictRequest {
            proposal_id: proposal_id.to_owned(),
            verb,
            reason: sovmem::profile::Reason::new(
                b"task-17 advisory human reason",
                store.policy.policy.max_reason_bytes,
            )
            .expect("construct valid human reason"),
            rewrite,
        };
        let initial =
            read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
                .expect("read advisory preflight state");
        let runtime = TransactionRuntime::production(&profile, &store)
            .expect("construct production authority runtime");
        let confirmation = format!("APPLY {} {}\n", verb.as_str(), &proposal_id[..12]);
        let mut human = ScriptedHuman::new(Some(confirmation));
        let outcome = match preflight_human_verdict_with_human(
            initial,
            request,
            &runtime,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-14T04:30:02.000000000Z").unwrap()),
        )
        .expect("advisory must not reject human preflight")
        {
            PreflightVerdict::Confirmed(decision) => {
                apply_human_verdict(decision, &runtime).expect("apply human advisory verdict")
            }
            PreflightVerdict::Idempotent(_) => panic!("isolated advisory fixture cannot replay"),
        };
        assert_eq!(human.show_count(), 1, "{verb:?} must reach the human once");
        assert_eq!(human.read_count(), 1, "{verb:?} must confirm once");
        outcome
    }

    fn recommendation_results(
        fixture: &ProvisionedP3Fixture,
        generation_id: &str,
        recommendation: &str,
    ) -> Vec<(String, String, String, String, String)> {
        let profile = load_p3_profile(&fixture.profile_path).expect("load recommendation profile");
        let generation_root = fixture
            .store_root
            .join("canon/generations")
            .join(generation_id);
        let generation = verify_generation(&profile, &generation_root, VerificationLevel::Three)
            .expect("verify recommendation generation");
        query_p3_generation(
            &profile,
            &generation,
            &fixture
                .store_root
                .join("derived/indexes")
                .join(generation_id),
            recommendation.to_owned(),
            62,
            ExpansionMode::None,
        )
        .expect("query complete recommendation vector")
        .results
        .into_iter()
        .map(|hit| {
            (
                hit.title,
                hit.core_type,
                hit.layer,
                hit.provenance,
                hit.score,
            )
        })
        .collect()
    }

    type NonJudgmentDocumentInvariant = (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        bool,
        bool,
    );

    #[derive(Debug, PartialEq, Eq)]
    struct CanonAdvisoryInvariant {
        generation_id: String,
        parent_kind: String,
        policy_hash: String,
        non_judgment_documents: BTreeMap<String, NonJudgmentDocumentInvariant>,
        target_bytes: Vec<u8>,
        normalized_judgment: String,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct LedgerAdvisoryInvariant {
        events: usize,
        verdicts: usize,
        accept: usize,
        reject: usize,
        rewrite: usize,
        tp: usize,
        fp: usize,
        applied_heads: usize,
        schema_version: String,
        event_type: String,
        sequence: u64,
        occurred_at: String,
        store_id: String,
        candidate_hash: String,
        source_id: String,
        verb: VerdictVerb,
        reason: String,
        actor: String,
        policy_hash: String,
        profile_hash: String,
        base_kind: String,
        base_generation: Option<String>,
        base_manifest_hash: String,
        base_bundle_hash: String,
        base_target_hash: Option<String>,
        rewrite_hash: Option<String>,
        rewrite_bytes: Option<u64>,
        canonical_availability: (bool, bool, bool, bool, bool, bool, bool),
        target_prior_hash: Option<String>,
        target_new_hash: Option<String>,
        resulting_canonical_head: String,
        intake_sequence: u64,
        intake_head_available: bool,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Level4AdvisoryInvariant {
        status: &'static str,
        command: &'static str,
        level: u8,
        events: usize,
        verdicts: usize,
        accept: usize,
        reject: usize,
        rewrite: usize,
        tp: usize,
        fp: usize,
        applied_generations: usize,
        current_generation: String,
        errors: usize,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ReindexAdvisoryInvariant {
        documents: usize,
        graph_nodes: usize,
        graph_edges: usize,
        external_typed_links: usize,
        index_config: String,
        store_id: String,
        generation: String,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct TransactionAdvisoryInvariant {
        canon: CanonAdvisoryInvariant,
        ledger: LedgerAdvisoryInvariant,
        level4: Level4AdvisoryInvariant,
        reindex: ReindexAdvisoryInvariant,
        query: Vec<(usize, String, String, String, String, String, String)>,
    }

    fn level4_advisory_invariant(
        summary: &sovmem::profile::Level4Summary,
    ) -> Level4AdvisoryInvariant {
        Level4AdvisoryInvariant {
            status: summary.status,
            command: summary.command,
            level: summary.level,
            events: summary.events,
            verdicts: summary.verdicts,
            accept: summary.accept,
            reject: summary.reject,
            rewrite: summary.rewrite,
            tp: summary.tp,
            fp: summary.fp,
            applied_generations: summary.applied_generations,
            current_generation: summary.current_generation.clone(),
            errors: summary.errors,
        }
    }

    fn advisory_query_vector(
        fixture: &ProvisionedP3Fixture,
    ) -> Vec<(usize, String, String, String, String, String, String)> {
        query_store(StoreQueryRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            query: "Advisory fixture exact candidate bytes".to_owned(),
            top_k: 5,
            expansion: ExpansionMode::None,
        })
        .expect("query the advisory transaction result")
        .result
        .results
        .into_iter()
        .map(|hit| {
            let id = if hit.core_type == "judgment" {
                "<advisory-derived-verdict>".to_owned()
            } else {
                hit.id
            };
            (
                hit.rank,
                id,
                hit.title,
                hit.core_type,
                hit.layer,
                hit.provenance,
                hit.score,
            )
        })
        .collect()
    }

    fn transaction_advisory_invariant(
        fixture: &ProvisionedP3Fixture,
        pair: &sovmem::profile::ProposalPair,
        outcome: &sovmem::profile::transaction::VerdictOutcome,
    ) -> TransactionAdvisoryInvariant {
        let profile =
            load_p3_profile(&fixture.profile_path).expect("load transaction-matrix profile");
        let generation_id = outcome
            .generation
            .as_deref()
            .expect("constant ACCEPT verdict creates a generation");
        let generation_root = fixture
            .store_root
            .join("canon/generations")
            .join(generation_id);
        let generation = verify_generation(&profile, &generation_root, VerificationLevel::Three)
            .expect("verify transaction-matrix generation");
        let non_judgment_documents = generation
            .manifest
            .documents
            .iter()
            .filter(|document| !document.canonical_path.starts_with("judgments/"))
            .map(|document| {
                (
                    document.canonical_path.clone(),
                    (
                        document.output_path.clone(),
                        document.id.clone(),
                        document.resource.clone(),
                        document.hash.clone(),
                        document.seed_source_path.clone(),
                        document.seed_source_hash.clone(),
                        document.previous_hash.clone(),
                        document.proposal_id.is_some(),
                        document.verdict_id.is_some(),
                    ),
                )
            })
            .collect();
        let target_bytes = fs::read(generation_root.join("bundle/04-Research/CLI/README.md"))
            .expect("read exact advisory target bytes");
        let judgment = fs::read_to_string(
            generation_root
                .join("bundle/judgments")
                .join(format!("{}.md", outcome.verdict_id)),
        )
        .expect("read advisory Human Judgment");
        let normalized_judgment =
            judgment.replace(&outcome.verdict_id, "<advisory-derived-verdict>");
        for sentinel in [
            ADVISORY_SENTINEL,
            CONFIDENCE_LITERAL,
            EVALUATOR_SOURCE_TOKEN,
        ] {
            assert!(
                !normalized_judgment.contains(sentinel),
                "Human Judgment must erase advisory source material"
            );
        }

        let ledger = read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("read transaction-matrix ledger");
        let verdict = ledger
            .events
            .iter()
            .find_map(|event| match event {
                sovmem::profile::LedgerEvent::Verdict(event)
                    if event.payload().proposal_id == pair.meta.proposal_id =>
                {
                    Some(event.payload())
                }
                _ => None,
            })
            .expect("find transaction-matrix verdict");
        assert_eq!(verdict.confidence, pair.meta.confidence);
        assert_eq!(
            verdict.evaluator_meta_hash,
            pair.meta
                .evaluator
                .as_ref()
                .map(|evaluator| evaluator_commitment(evaluator).expect("commit evaluator"))
        );
        let canonical = &verdict.canonical_result;
        let ledger_invariant = LedgerAdvisoryInvariant {
            events: ledger.events.len(),
            verdicts: ledger.verdicts,
            accept: ledger.accept,
            reject: ledger.reject,
            rewrite: ledger.rewrite,
            tp: ledger.tp,
            fp: ledger.fp,
            applied_heads: ledger.applied_heads.len(),
            schema_version: verdict.schema_version.clone(),
            event_type: verdict.event_type.clone(),
            sequence: verdict.sequence,
            occurred_at: verdict.occurred_at.clone(),
            store_id: verdict.store_id.clone(),
            candidate_hash: verdict.candidate_hash.clone(),
            source_id: verdict.source_id.clone(),
            verb: verdict.verb,
            reason: verdict.reason.clone(),
            actor: verdict.actor.clone(),
            policy_hash: verdict.policy_hash.clone(),
            profile_hash: verdict.profile_hash.clone(),
            base_kind: verdict.base_kind.clone(),
            base_generation: verdict.base_generation.clone(),
            base_manifest_hash: verdict.base_manifest_hash.clone(),
            base_bundle_hash: verdict.base_bundle_hash.clone(),
            base_target_hash: verdict.base_target_hash.clone(),
            rewrite_hash: verdict.rewrite_hash.clone(),
            rewrite_bytes: verdict.rewrite_bytes,
            canonical_availability: (
                canonical.generation_id.is_some(),
                canonical.manifest_hash.is_some(),
                canonical.bundle_hash.is_some(),
                canonical.target_prior_hash.is_some(),
                canonical.target_new_hash.is_some(),
                canonical.judgment_hash.is_some(),
                canonical.reindex_receipt_hash.is_some(),
            ),
            target_prior_hash: canonical.target_prior_hash.clone(),
            target_new_hash: canonical.target_new_hash.clone(),
            resulting_canonical_head: verdict.resulting_canonical_head.clone(),
            intake_sequence: verdict.intake_sequence,
            intake_head_available: verdict.intake_head_hash.is_some(),
        };

        let level4 = verify_store_level4(&profile, &fixture.store_root)
            .expect("transaction matrix must verify at Level 4");
        let level4_invariant = level4_advisory_invariant(&level4);
        let query_before_rebuild = advisory_query_vector(fixture);
        let index_root = fixture
            .store_root
            .join("derived/indexes")
            .join(generation_id);
        fs::remove_dir_all(&index_root).expect("remove only the disposable transaction index");
        let rebuilt = rebuild_store_index(StoreIndexRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        })
        .expect("rebuild transaction index from canonical state");
        let query_after_rebuild = advisory_query_vector(fixture);
        assert_eq!(
            query_after_rebuild, query_before_rebuild,
            "rebuild must preserve the complete advisory query-result vector"
        );
        assert_eq!(
            level4_advisory_invariant(
                &verify_store_level4(&profile, &fixture.store_root)
                    .expect("rebuilt transaction matrix must remain Level 4"),
            ),
            level4_invariant,
            "disposable reindex must preserve Level 4 availability and shape"
        );

        TransactionAdvisoryInvariant {
            canon: CanonAdvisoryInvariant {
                generation_id: generation.manifest.generation_id,
                parent_kind: format!("{:?}", generation.manifest.parent_kind),
                policy_hash: generation.manifest.policy_hash,
                non_judgment_documents,
                target_bytes,
                normalized_judgment,
            },
            ledger: ledger_invariant,
            level4: level4_invariant,
            reindex: ReindexAdvisoryInvariant {
                documents: rebuilt.documents,
                graph_nodes: rebuilt.graph_nodes,
                graph_edges: rebuilt.graph_edges,
                external_typed_links: rebuilt.external_typed_links,
                index_config: rebuilt.index_config,
                store_id: rebuilt.store_id,
                generation: rebuilt.generation,
            },
            query: query_after_rebuild,
        }
    }

    #[test]
    fn confidence_absent_zero_one_and_evaluator_polarity_are_transaction_advisory_only() {
        let mut expected = None;
        let mut transaction_confidences = BTreeSet::new();
        let mut evaluator_recommendations = BTreeSet::new();
        let mut proposal_ids = BTreeSet::new();

        for (confidence_name, confidence) in [
            ("absent", None),
            ("zero", Some(Confidence::parse("0").unwrap())),
            ("one", Some(Confidence::parse("1").unwrap())),
        ] {
            for recommendation in ["looks-sound", "reject"] {
                let fixture = ProvisionedP3Fixture::empty(&format!(
                    "task-10-confidence-{confidence_name}-{recommendation}"
                ));
                let pair = staged_pair(
                    &fixture,
                    ProposalIntent::Accept,
                    confidence,
                    Some(recommendation),
                );
                transaction_confidences
                    .insert(pair.meta.confidence.map(|value| value.millionths()));
                evaluator_recommendations.insert(
                    pair.meta
                        .evaluator
                        .as_ref()
                        .expect("matrix evaluator")
                        .recommendation
                        .clone(),
                );
                assert!(proposal_ids.insert(pair.meta.proposal_id.clone()));
                let outcome = apply_with_scripted_human(
                    &fixture,
                    &pair.meta.proposal_id,
                    VerdictVerb::Accept,
                );
                let invariant = transaction_advisory_invariant(&fixture, &pair, &outcome);
                if let Some(expected) = expected.as_ref() {
                    assert_eq!(
                        &invariant, expected,
                        "only advisory commitments may differ for {confidence_name}/{recommendation}"
                    );
                } else {
                    expected = Some(invariant);
                }
            }
        }

        assert_eq!(
            transaction_confidences,
            BTreeSet::from([None, Some(0), Some(1_000_000)])
        );
        assert_eq!(
            evaluator_recommendations,
            BTreeSet::from(["looks-sound".to_owned(), "reject".to_owned()])
        );
        assert_eq!(proposal_ids.len(), 6);
    }

    #[test]
    fn advisory_recommendations_cannot_change_human_verb_or_canonical_availability() {
        let accept_control = ProvisionedP3Fixture::empty("task-17-advisory-control-accept");
        let accept_control_pair = staged_pair(
            &accept_control,
            ProposalIntent::Accept,
            Some(Confidence::parse(CONFIDENCE_LITERAL).unwrap()),
            None,
        );
        let accept_control_generation = apply_with_scripted_human(
            &accept_control,
            &accept_control_pair.meta.proposal_id,
            VerdictVerb::Accept,
        )
        .generation
        .expect("ACCEPT control generation");
        let rewrite_control = ProvisionedP3Fixture::empty("task-17-advisory-control-rewrite");
        let rewrite_control_pair = staged_pair(
            &rewrite_control,
            ProposalIntent::Rewrite,
            Some(Confidence::parse(CONFIDENCE_LITERAL).unwrap()),
            None,
        );
        let rewrite_control_generation = apply_with_scripted_human(
            &rewrite_control,
            &rewrite_control_pair.meta.proposal_id,
            VerdictVerb::Rewrite,
        )
        .generation
        .expect("REWRITE control generation");
        for recommendation in ["looks-sound", "needs-rewrite", "reject"] {
            for verb in [
                VerdictVerb::Accept,
                VerdictVerb::Reject,
                VerdictVerb::Rewrite,
                VerdictVerb::Tp,
                VerdictVerb::Fp,
            ] {
                let fixture = ProvisionedP3Fixture::empty(&format!(
                    "task-17-advisory-human-{}-{}",
                    recommendation,
                    verb.as_str().to_ascii_lowercase()
                ));
                let intent = if verb == VerdictVerb::Rewrite {
                    ProposalIntent::Rewrite
                } else {
                    ProposalIntent::Accept
                };
                let pair = staged_pair(
                    &fixture,
                    intent,
                    Some(Confidence::parse(CONFIDENCE_LITERAL).unwrap()),
                    Some(recommendation),
                );
                let canon_before = artifact_hashes(&fixture.store_root.join("canon"));
                let index_before = artifact_hashes(&fixture.store_root.join("derived/indexes"));
                let outcome = apply_with_scripted_human(&fixture, &pair.meta.proposal_id, verb);

                if !verb.applies() {
                    assert!(!outcome.applied);
                    assert!(outcome.generation.is_none());
                    assert_eq!(
                        artifact_hashes(&fixture.store_root.join("canon")),
                        canon_before
                    );
                    assert_eq!(
                        artifact_hashes(&fixture.store_root.join("derived/indexes")),
                        index_before
                    );
                    continue;
                }

                let generation_id = outcome.generation.expect("applying verdict has generation");
                let generation_root = fixture
                    .store_root
                    .join("canon/generations")
                    .join(&generation_id);
                let target = fs::read(generation_root.join("bundle/04-Research/CLI/README.md"))
                    .expect("read canonical target");
                let judgment = fs::read(
                    generation_root
                        .join("bundle/judgments")
                        .join(format!("{}.md", outcome.verdict_id)),
                )
                .expect("read canonical Human Judgment");
                let manifest = fs::read(generation_root.join("manifest.json"))
                    .expect("read canonical manifest");
                for bytes in [&target, &judgment, &manifest] {
                    let text = std::str::from_utf8(bytes).expect("canonical text evidence");
                    assert!(!text.contains(ADVISORY_SENTINEL));
                    assert!(!text.contains(CONFIDENCE_LITERAL));
                    assert!(!text.contains(EVALUATOR_SOURCE_TOKEN));
                    assert!(!text.contains(&format!("\"recommendation\":\"{recommendation}\"")));
                }
                let profile =
                    load_p3_profile(&fixture.profile_path).expect("reload applying profile");
                let generation =
                    verify_generation(&profile, &generation_root, VerificationLevel::Three)
                        .expect("verify applying generation");
                for (name, token) in [
                    ("evaluator reason", ADVISORY_SENTINEL),
                    ("confidence", CONFIDENCE_TOKEN),
                    ("evaluator source", EVALUATOR_SOURCE_TOKEN),
                ] {
                    let query = query_p3_generation(
                        &profile,
                        &generation,
                        &fixture
                            .store_root
                            .join("derived/indexes")
                            .join(&generation_id),
                        token.to_owned(),
                        5,
                        ExpansionMode::None,
                    )
                    .expect("query rebuilt index for advisory component");
                    assert!(query.results.is_empty(), "{name} must not be indexed");
                }
                let ledger = read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
                    .expect("read advisory evidence ledger");
                let verdict = ledger
                    .events
                    .iter()
                    .find_map(|event| match event {
                        sovmem::profile::LedgerEvent::Verdict(event)
                            if event.payload().proposal_id == pair.meta.proposal_id =>
                        {
                            Some(event.payload())
                        }
                        _ => None,
                    })
                    .expect("advisory verdict evidence");
                assert_eq!(verdict.confidence, pair.meta.confidence);
                assert_eq!(
                    verdict.evaluator_meta_hash.as_deref(),
                    Some(
                        evaluator_commitment(pair.meta.evaluator.as_ref().unwrap())
                            .expect("commit staged evaluator")
                            .as_str()
                    )
                );

                let (control, control_generation) = if verb == VerdictVerb::Accept {
                    (&accept_control, &accept_control_generation)
                } else {
                    (&rewrite_control, &rewrite_control_generation)
                };
                assert_eq!(
                    recommendation_results(&fixture, &generation_id, recommendation),
                    recommendation_results(control, control_generation, recommendation),
                    "recommendation must not change the complete rebuilt-index result vector"
                );
            }
        }
    }

    #[test]
    fn harness_captures_confidence_before_draft_and_the_binary_never_infers_it() {
        let fixture = ProvisionedP3Fixture::empty("task-17-advisory-harness-order");
        let mut trace = Vec::new();
        let confidence = capture_confidence(&mut trace, CONFIDENCE_LITERAL);
        let candidate = create_candidate_after_capture(&mut trace, &fixture);
        let request = ProposalSubmission {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            source_id: "src:agent:fixture".to_owned(),
            operation: ProposalOperation::Create,
            intent: ProposalIntent::Accept,
            target_id: "ssa-0/04-Research/CLI/README.md".to_owned(),
            target_path: "04-Research/CLI/README.md".to_owned(),
            candidate_path: candidate,
            confidence: Some(confidence),
            evaluator_path: None,
        };
        let submitted = submit_proposal_with_clock(
            request,
            &FixedClock(Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap()),
        )
        .expect("submit confidence captured before candidate draft");
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let sealed = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-14T04:30:01.000000000Z").unwrap()),
        )
        .expect("human-seal confidence-captured submission");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = inspect_store(&profile, &fixture.store_root).unwrap();
        let persisted = read_sealed_pair_structural(
            &store.paths,
            sealed.proposal_id.as_deref().expect("staged proposal id"),
        )
        .expect("read persisted protected proposal");
        assert_eq!(trace, ["confidence-captured", "candidate-created"]);
        assert_eq!(persisted.meta.confidence.unwrap().millionths(), 963_217);

        let absent_fixture = ProvisionedP3Fixture::empty("task-17-advisory-absent-confidence");
        let absent_candidate = advisory_candidate_bytes(&absent_fixture);
        let absent = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: absent_fixture.profile_path.clone(),
                store_root: absent_fixture.store_root.clone(),
                source_id: "src:agent:fixture".to_owned(),
                operation: ProposalOperation::Create,
                intent: ProposalIntent::Accept,
                target_id: "ssa-0/04-Research/CLI/README.md".to_owned(),
                target_path: "04-Research/CLI/README.md".to_owned(),
                candidate_path: fixture_input(
                    &absent_fixture,
                    "absent-confidence",
                    &absent_candidate,
                ),
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(Timestamp::parse("2026-07-14T04:31:00.000000000Z").unwrap()),
        )
        .expect("stage absent confidence without inference");
        let mut absent_human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", absent.submission_id)));
        let absent_sealed = seal_intake_submission_with_human(
            &absent_fixture.profile_path,
            &absent_fixture.store_root,
            &absent.submission_id,
            &mut absent_human,
            &FixedClock(Timestamp::parse("2026-07-14T04:31:01.000000000Z").unwrap()),
        )
        .expect("human-seal absent-confidence submission");
        let absent_profile = load_p3_profile(&absent_fixture.profile_path).unwrap();
        let absent_store = inspect_store(&absent_profile, &absent_fixture.store_root).unwrap();
        let absent_pair = read_sealed_pair_structural(
            &absent_store.paths,
            absent_sealed
                .proposal_id
                .as_deref()
                .expect("absent-confidence staged proposal id"),
        )
        .expect("read absent-confidence protected proposal");
        assert_eq!(absent_pair.meta.confidence, None);
    }

    fn capture_confidence(trace: &mut Vec<&'static str>, value: &str) -> Confidence {
        trace.push("confidence-captured");
        Confidence::parse(value).expect("capture canonical confidence")
    }

    fn create_candidate_after_capture(
        trace: &mut Vec<&'static str>,
        fixture: &ProvisionedP3Fixture,
    ) -> std::path::PathBuf {
        let candidate = fixture_input(
            fixture,
            "captured-after-confidence",
            &advisory_candidate_bytes(fixture),
        );
        trace.push("candidate-created");
        candidate
    }

    fn fixture_input(
        fixture: &ProvisionedP3Fixture,
        name: &str,
        bytes: &[u8],
    ) -> std::path::PathBuf {
        let path = fixture
            .profile_path
            .parent()
            .expect("fixture profile has parent")
            .join(format!("advisory-{name}.input"));
        fs::write(&path, bytes).expect("write advisory fixture input");
        path
    }

    fn advisory_candidate_bytes(fixture: &ProvisionedP3Fixture) -> Vec<u8> {
        let seed_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_path = "04-Research/CLI/README.md";
        let target_id = format!("ssa-0/{target_path}");
        let bytes = fs::read(fixture.store_root.join("seed/bundle").join(seed_path))
            .expect("read advisory candidate template");
        let parsed = parse_source_document(SourceDocument {
            source_path: seed_path.to_owned(),
            source_hash: blake3::hash(&bytes).to_hex().to_string(),
            bytes,
            created: String::new(),
            updated: String::new(),
        })
        .expect("parse advisory candidate template");
        let mut metadata: GeneratedMetadata = serde_yaml::from_value(
            serde_yaml::to_value(parsed.metadata).expect("encode advisory candidate metadata"),
        )
        .expect("decode advisory candidate metadata");
        metadata.id = target_id.clone();
        metadata.source_path = target_path.to_owned();
        metadata.source = format!("vault://{target_id}");
        metadata.resource = format!("vault://{target_id}");
        metadata.title = "P3 advisory erasure fixture".to_owned();
        metadata.description = "Deterministic advisory erasure candidate.".to_owned();
        if let Some(mapping) = load_p3_profile(&fixture.profile_path)
            .expect("load advisory fixture profile")
            .config
            .mapping_for(target_path)
        {
            metadata.core_type = mapping.core_type.clone();
            metadata.context_kind = mapping.context_kind.clone();
            metadata.layer = mapping.layer.clone();
            metadata.authority = mapping.authority.clone();
        }
        render_document(&GeneratedDocument {
            metadata,
            body: format!("{}\nAdvisory fixture exact candidate bytes.\n", parsed.body),
            source_hash: String::new(),
        })
        .expect("render advisory fixture candidate")
    }
}

pub mod canon_tests {

    use sovmem::profile::{
        build_judgment, load_p3_profile, load_policy, validate_decision, validate_document,
        validate_rewrite, verdict_id, verify_generation, Authority, CanonManifest, CoreType,
        JudgmentError, Layer, ProposalIntent, Provenance, Reason, RewriteBytes, Sensitivity,
        SourceKind, Status, Timestamp, VerdictIdentityInput, VerdictVerb, VerificationLevel,
    };
    use std::path::Path;

    fn assert_manifest_and_document_identity_match(
        generation: &sovmem::profile::VerifiedGeneration,
    ) {
        for entry in &generation.manifest.documents {
            let document = generation
                .generated_documents
                .iter()
                .find(|document| document.metadata.source_path == entry.canonical_path)
                .expect("manifest document has parsed bytes");
            assert_eq!(document.metadata.id, entry.id, "{}", entry.canonical_path);
            assert_eq!(
                document.metadata.resource, entry.resource,
                "{}",
                entry.canonical_path
            );
            assert_eq!(
                document.metadata.source, entry.resource,
                "{}",
                entry.canonical_path
            );
            assert_eq!(document.source_hash, entry.hash, "{}", entry.canonical_path);
        }
    }

    #[test]
    fn internal_verdict_grammar_and_reason_rules_are_exact() {
        for (source, verb) in [
            ("ACCEPT", VerdictVerb::Accept),
            ("REJECT", VerdictVerb::Reject),
            ("REWRITE", VerdictVerb::Rewrite),
            ("TP", VerdictVerb::Tp),
            ("FP", VerdictVerb::Fp),
        ] {
            assert_eq!(source.parse::<VerdictVerb>().unwrap(), verb);
            assert_eq!(verb.as_str(), source);
        }
        for invalid in ["accept", "", "TRUE_POSITIVE", "UNKNOWN"] {
            assert!(invalid.parse::<VerdictVerb>().is_err(), "{invalid}");
        }
        assert!(Reason::new(b"human reviewed evidence", 4096).is_ok());
        for invalid in [b"".as_slice(), b"   \n\t", b"contains\0nul"] {
            assert!(Reason::new(invalid, 4096).is_err());
        }
    }

    #[test]
    fn decision_binding_requires_the_exact_human_rewrite_shape() {
        let reason = Reason::new(b"Human reason.", 4096).unwrap();
        let ruled_at = Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap();
        let policy_hash = "b".repeat(64);
        let profile_hash = "c".repeat(64);
        let rewrite = RewriteBytes::new(b"---\nid: fixture\n---\n# Rewrite\n".to_vec(), 4096)
            .expect("rewrite bytes");
        assert!(RewriteBytes::new(b"valid\x01".to_vec(), 4096).is_err());
        let identity_without_rewrite = VerdictIdentityInput {
            proposal_id: "prp-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            verb: VerdictVerb::Rewrite,
            reason: &reason,
            rewrite: None,
            actor: "src:human:khalil-nooh",
            ruled_at: &ruled_at,
            prior_ledger_head: None,
            intake_sequence: 0,
            intake_head_hash: None,
            policy_hash: &policy_hash,
            profile_hash: &profile_hash,
        };
        assert_eq!(
            validate_decision(
                ProposalIntent::Rewrite,
                VerdictVerb::Rewrite,
                reason.clone(),
                None,
                &identity_without_rewrite,
            )
            .unwrap_err()
            .code(),
            JudgmentError::RewriteRequired.code()
        );
        let accept_identity = VerdictIdentityInput {
            verb: VerdictVerb::Accept,
            ..identity_without_rewrite.clone()
        };
        assert_eq!(
            validate_decision(
                ProposalIntent::Accept,
                VerdictVerb::Accept,
                reason.clone(),
                Some(rewrite.clone()),
                &accept_identity,
            )
            .unwrap_err()
            .code(),
            JudgmentError::RewriteForbidden.code()
        );
        assert_eq!(
            validate_decision(
                ProposalIntent::Rewrite,
                VerdictVerb::Rewrite,
                reason.clone(),
                Some(rewrite.clone()),
                &identity_without_rewrite,
            )
            .unwrap_err()
            .code(),
            JudgmentError::RewriteInvalid.code()
        );
        let identity_with_rewrite = VerdictIdentityInput {
            rewrite: Some(&rewrite),
            ..identity_without_rewrite
        };
        assert!(validate_decision(
            ProposalIntent::Rewrite,
            VerdictVerb::Rewrite,
            reason.clone(),
            Some(rewrite.clone()),
            &identity_with_rewrite,
        )
        .is_ok());
    }

    #[test]
    fn human_rewrite_must_be_a_complete_target_bound_p3_document() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("human-rewrite-validation");
        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
        let policy = load_policy(&fixture.policy_template).expect("load P3 policy");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let p2_target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let approved = String::from_utf8(
            std::fs::read(fixture.store_root.join("seed/bundle").join(target_path))
                .expect("read verified fixture document"),
        )
        .unwrap()
        .replace(p2_target_id, target_id)
        .into_bytes();
        let rewrite = RewriteBytes::new(approved, policy.policy.max_rewrite_bytes)
            .expect("approved rewrite bytes");
        let document = validate_rewrite(&rewrite, &profile, &policy.policy, target_id, target_path)
            .expect("complete target-bound P3 rewrite");
        assert_eq!(document.metadata.id, target_id);
        assert_eq!(document.source_hash, rewrite.hash());

        let missing_provenance = RewriteBytes::new(
            String::from_utf8(rewrite.bytes().to_vec())
                .expect("fixture document is UTF-8")
                .replacen("provenance: synthesis\n", "", 1)
                .into_bytes(),
            policy.policy.max_rewrite_bytes,
        )
        .expect("well-shaped but unprovenanced rewrite");
        assert_eq!(
            validate_rewrite(
                &missing_provenance,
                &profile,
                &policy.policy,
                target_id,
                target_path,
            )
            .unwrap_err()
            .code(),
            JudgmentError::RewriteInvalid.code()
        );
        assert_eq!(
            validate_rewrite(
                &rewrite,
                &profile,
                &policy.policy,
                "ssa-0/04-Research/Agent-Loop-Engineering/Other.md",
                target_path,
            )
            .unwrap_err()
            .code(),
            JudgmentError::RewriteInvalid.code()
        );
    }

    #[test]
    fn create_and_replace_reconstruct_exact_generation_evidence() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("canon-generation");
        let create = fixture.stage_accept_create();
        assert_eq!(create.manifest.schema_version, "sovmem-canon-manifest-v1");
        assert_eq!(create.manifest.store_id, "audy-sovmem-writeback-v2");
        assert_eq!(create.manifest.parent_kind.to_string(), "seed");
        assert_eq!(create.manifest.documents.len(), 64);
        assert!(create.manifest.documents.iter().all(|document| {
            document.id.ends_with(".md") && document.resource == format!("vault://{}", document.id)
        }));
        assert!(create.manifest.documents.iter().any(|document| {
            document.canonical_path == "04-Research/P3-Generated-Canon.md"
                && document.id == "ssa-0/04-Research/P3-Generated-Canon.md"
        }));
        assert!(fixture.target_bytes_equal_approved(&create));
        assert!(fixture.judgment_bytes_equal_approved(&create));
        assert_manifest_and_document_identity_match(&create);

        let replace = fixture.stage_rewrite_replace(&create);
        assert_eq!(replace.manifest.parent_kind.to_string(), "generation");
        assert_eq!(replace.manifest.documents.len(), 65);
        assert!(fixture.target_bytes_equal_approved(&replace));
        assert!(fixture.seed_lineage_preserved(&replace));
        assert_manifest_and_document_identity_match(&replace);
    }

    #[test]
    fn generation_verification_fails_closed_on_storage_and_manifest_corruption() {
        type CorruptionCase = (&'static str, &'static str, fn(&std::path::Path));
        let cases: [CorruptionCase; 5] = [
            ("extra-root-file", "canon.root_invalid", |root| {
                std::fs::write(root.join("corpus-report.json"), b"{}\n")
                    .expect("write forbidden corpus evidence");
            }),
            ("manifest-unknown", "canon.manifest_invalid", |root| {
                let mut manifest =
                    std::fs::read_to_string(root.join("manifest.json")).expect("read manifest");
                manifest = manifest.replacen("\n}\n", ",\n  \"unknown\": true\n}\n", 1);
                std::fs::write(root.join("manifest.json"), manifest).expect("write manifest");
            }),
            ("missing-document", "canon.hash_mismatch", |root| {
                std::fs::remove_file(root.join("bundle/04-Research/P3-Generated-Canon.md"))
                    .expect("remove documented file");
            }),
            ("index-mismatch", "canon.index_mismatch", |root| {
                std::fs::write(root.join("bundle/index.md"), b"# forged\n")
                    .expect("write forged index");
            }),
            ("parent-binding", "canon.parent_invalid", |root| {
                let bytes = std::fs::read(root.join("manifest.json")).expect("read manifest");
                let mut manifest: CanonManifest =
                    serde_json::from_slice(&bytes).expect("parse manifest");
                manifest.parent_generation_id = Some("gen-0000000000000009".to_owned());
                let mut bytes = serde_json::to_vec_pretty(&manifest).expect("render manifest");
                bytes.push(b'\n');
                std::fs::write(root.join("manifest.json"), bytes).expect("write manifest");
            }),
        ];

        for (name, expected_code, corrupt) in cases {
            let fixture =
                sovmem::profile::p3_internal::p3::P3Fixture::new(&format!("canon-corrupt-{name}"));
            let generation = fixture.stage_accept_create();
            corrupt(&generation.root);
            let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
            assert_eq!(
                verify_generation(&profile, &generation.root, VerificationLevel::Three)
                    .expect_err("corruption must fail verification")
                    .code(),
                expected_code,
                "{name}"
            );
        }
    }

    #[test]
    fn generation_verification_rejects_seed_lineage_without_a_previous_raw_hash() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("canon-seed-lineage");
        let create = fixture.stage_accept_create();
        let replace = fixture.stage_rewrite_replace(&create);
        let manifest_path = replace.root.join("manifest.json");
        let mut manifest: CanonManifest = serde_json::from_slice(
            &std::fs::read(&manifest_path).expect("read replacement manifest"),
        )
        .expect("parse replacement manifest");
        manifest
            .documents
            .iter_mut()
            .find(|document| {
                document.canonical_path
                    == "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"
            })
            .expect("replacement target")
            .previous_hash = None;
        let mut bytes = serde_json::to_vec_pretty(&manifest).expect("render replacement manifest");
        bytes.push(b'\n');
        std::fs::write(&manifest_path, bytes).expect("write forged replacement manifest");
        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
        assert_eq!(
            verify_generation(&profile, &replace.root, VerificationLevel::Three)
                .expect_err("lineage corruption must fail")
                .code(),
            "canon.lineage_invalid"
        );
    }

    #[test]
    fn verdict_identity_and_human_judgment_bytes_are_deterministic() {
        let profile = load_p3_profile(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-p3-v2/profile.yaml"),
        )
        .expect("load P3 profile");
        let reason = Reason::new(b"Human reviewed the exact candidate.", 4096).unwrap();
        let ruled_at = Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap();
        let policy_hash = "b".repeat(64);
        let identity = VerdictIdentityInput {
            proposal_id: "prp-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            verb: VerdictVerb::Accept,
            reason: &reason,
            rewrite: None,
            actor: "src:human:khalil-nooh",
            ruled_at: &ruled_at,
            prior_ledger_head: None,
            intake_sequence: 0,
            intake_head_hash: None,
            policy_hash: &policy_hash,
            profile_hash: profile.hash.as_str(),
        };
        let first = verdict_id(&identity);
        let second = verdict_id(&identity);
        assert_eq!(first, second);
        assert!(first.starts_with("ver-"));
        assert_eq!(first.len(), 68);

        let material = validate_decision(
            ProposalIntent::Accept,
            VerdictVerb::Accept,
            reason.clone(),
            None,
            &identity,
        )
        .expect("validate ACCEPT material");
        assert_eq!(material.verdict_id, first);
        let judgment = build_judgment(
            &profile,
            &first,
            VerdictVerb::Accept,
            "ssa-0/04-Research/AVM/AVM-OKF-Substrate-Investigation.md",
            &reason,
            &ruled_at,
        )
        .unwrap()
        .expect("ACCEPT creates a judgment");
        assert_eq!(judgment.path, format!("judgments/{first}.md"));
        assert_eq!(judgment.id, format!("ssa-0/judgments/{first}.md"));
        assert_eq!(judgment.document.metadata.core_type, CoreType::Judgment);
        assert_eq!(judgment.document.metadata.context_kind, "Human Judgment");
        assert_eq!(judgment.document.metadata.layer, Layer::Concept);
        assert_eq!(judgment.document.metadata.provenance, Provenance::Synthesis);
        assert_eq!(judgment.document.metadata.authority, Authority::Canonical);
        assert_eq!(
            judgment.document.metadata.sensitivity,
            Sensitivity::Internal
        );
        assert_eq!(judgment.document.metadata.status, Status::Active);
        assert_eq!(
            judgment.document.metadata.source_kind,
            SourceKind::GeneratedReport
        );
        assert_eq!(judgment.document.metadata.links.len(), 1);
        assert_eq!(
            judgment.document.metadata.reason.as_deref(),
            Some("Human reviewed the exact candidate.")
        );
        assert!(judgment.document.metadata.inferred_fields.is_empty());
        assert!(judgment
            .bytes
            .ends_with(b"Human reviewed the exact candidate."));
        assert_eq!(
            judgment.hash,
            blake3::hash(&judgment.bytes).to_hex().to_string()
        );
        let validation = validate_document(&profile.config, &judgment.document);
        assert!(
            !validation.has_errors(),
            "Judgment validation: {:#?}",
            validation.diagnostics()
        );
        let second_judgment = build_judgment(
            &profile,
            &first,
            VerdictVerb::Accept,
            "ssa-0/04-Research/AVM/AVM-OKF-Substrate-Investigation.md",
            &reason,
            &ruled_at,
        )
        .unwrap()
        .expect("second deterministic judgment");
        assert_eq!(second_judgment.bytes, judgment.bytes);
        assert_eq!(second_judgment.hash, judgment.hash);
        for verb in [VerdictVerb::Reject, VerdictVerb::Tp, VerdictVerb::Fp] {
            assert!(build_judgment(
                &profile,
                &first,
                verb,
                "ssa-0/04-Research/AVM/AVM-OKF-Substrate-Investigation.md",
                &reason,
                &ruled_at,
            )
            .unwrap()
            .is_none());
        }
    }

    #[test]
    fn non_applying_verbs_produce_no_human_judgment_document() {
        let profile = load_p3_profile(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-p3-v2/profile.yaml"),
        )
        .unwrap();
        let reason = Reason::new(b"human ledger-only classification", 4096).unwrap();
        let ruled_at = Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap();

        for verb in [VerdictVerb::Reject, VerdictVerb::Tp, VerdictVerb::Fp] {
            assert!(build_judgment(
                &profile,
                &format!("ver-{}", "a".repeat(64)),
                verb,
                "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                &reason,
                &ruled_at,
            )
            .unwrap()
            .is_none());
        }
    }
}

pub mod capability_tests {
    use sovmem::profile::p3_test_support::{FixedClock, ProvisionedP3Fixture};
    use sovmem::profile::{
        load_p3_profile, parse_source_document, render_document, seal_intake_submission,
        submit_proposal_with_clock, GeneratedDocument, GeneratedMetadata, ProposalIntent,
        ProposalOperation, ProposalSubmission, SourceDocument, Timestamp, TransactionError,
    };
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const PERMISSION_MODEL_UNSUPPORTED: &str = "store.permission_model_unsupported";
    const CHILD_TEST: &str = "profile::p3_internal::capability::restricted_proposer_child_entry";
    const CHILD_ACTION: &str = "SOVMEM_P3_CAPABILITY_ACTION";
    const CHILD_PROFILE: &str = "SOVMEM_P3_CAPABILITY_PROFILE";
    const CHILD_STORE: &str = "SOVMEM_P3_CAPABILITY_STORE";
    const CHILD_CANDIDATE: &str = "SOVMEM_P3_CAPABILITY_CANDIDATE";
    const CHILD_PROPOSAL: &str = "SOVMEM_P3_CAPABILITY_PROPOSAL";
    const CHILD_TARGET: &str = "SOVMEM_P3_CAPABILITY_TARGET";
    const TARGET_PATH: &str = "04-Research/CLI/README.md";
    const TARGET_ID: &str = "ssa-0/04-Research/CLI/README.md";
    const FIXED_TIME: &str = "2026-07-22T04:00:00.000000000Z";
    const PROTECTED_ROOTS: [&str; 11] = [
        "intake/events.jsonl",
        "intake/proposals",
        "intake/transactions",
        "locks",
        "ledger",
        "canon",
        "policies",
        "evidence",
        "derived",
        "transactions",
        "policy.toml",
    ];
    const PROTECTED_MATRIX_ACTIONS: [&str; 6] = [
        "create",
        "write",
        "rename",
        "remove",
        "hard-link",
        "symlink",
    ];
    const PROTECTED_ACTIONS: [&str; 11] = [
        "create",
        "write",
        "rename",
        "remove",
        "hard-link",
        "symlink",
        "hard-link-protected-into-own",
        "symlink-own-to-protected",
        "rename-protected-into-own",
        "rename-foreign-into-own",
        "rename-replace-protected",
    ];

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum RestrictedCapability {
        Supported,
    }

    struct ChildFixture<'a> {
        sandbox: &'a Path,
        profile: &'a Path,
        store: &'a Path,
        candidate: &'a Path,
        submission_id: &'a str,
    }

    impl ChildFixture<'_> {
        fn run(
            &self,
            action: &str,
            proposal_id: Option<&str>,
            target: Option<&str>,
            spoof: bool,
        ) -> bool {
            let profile = sandbox_profile(self.store, self.submission_id);
            let executable =
                std::env::current_exe().expect("locate capability child test executable");
            let mut command = Command::new(self.sandbox);
            command
                .arg("-p")
                .arg(profile)
                .arg(executable)
                .args(["--exact", CHILD_TEST, "--nocapture"])
                .env(CHILD_ACTION, action)
                .env(CHILD_PROFILE, self.profile)
                .env(CHILD_STORE, self.store)
                .env(CHILD_CANDIDATE, self.candidate)
                .env(CHILD_PROPOSAL, self.submission_id);
            if let Some(proposal_id) = proposal_id {
                command.env(CHILD_PROPOSAL, proposal_id);
            }
            if let Some(target) = target {
                command.env(CHILD_TARGET, target);
            }
            if spoof {
                command
                    .env("USER", "capability-spoofed-user")
                    .env("LOGNAME", "capability-spoofed-user")
                    .env("SOVMEM_ACTOR", "src:human:capability-spoofed");
            }
            command
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }
    }

    fn exercise_restricted_proposer_fixture() -> Result<RestrictedCapability, &'static str> {
        let Some(sandbox) = usable_sandbox() else {
            return Err(PERMISSION_MODEL_UNSUPPORTED);
        };
        let fixture = ProvisionedP3Fixture::empty("restricted-proposer-capability");
        let candidate = fixture
            .profile_path
            .parent()
            .expect("capability fixture profile has parent")
            .join("capability-candidate.md");
        fs::write(&candidate, capability_candidate_bytes(&fixture))
            .expect("write capability candidate");
        let predicted = submit_proposal_with_clock(
            capability_submission(&fixture, &candidate),
            &FixedClock(Timestamp::parse(FIXED_TIME).expect("fixed capability timestamp")),
        )
        .expect("derive the exact content-bound submission path");
        fs::remove_dir_all(
            fixture
                .store_root
                .join("staging/inbox")
                .join(&predicted.submission_id),
        )
        .expect("remove the unsandboxed path derivation request");
        let child = ChildFixture {
            sandbox: &sandbox,
            profile: &fixture.profile_path,
            store: &fixture.store_root,
            candidate: &candidate,
            submission_id: &predicted.submission_id,
        };
        let before = protected_fingerprints(&fixture.store_root);

        assert!(
            child.run("stage", None, None, false),
            "restricted proposer child must create its one exact untrusted inbox request"
        );
        let own_submission = fixture
            .store_root
            .join("staging/inbox")
            .join(&predicted.submission_id);
        assert!(own_submission.join("candidate.md").is_file());
        assert!(own_submission.join("submission.meta.json").is_file());
        assert!(
            !fixture.store_root.join("locks/intake.lock").exists(),
            "propose remains transport-only and cannot acquire protected intake authority"
        );
        assert_eq!(protected_fingerprints(&fixture.store_root), before);

        assert!(child.run("no-tty-seal", None, None, true));
        assert_eq!(protected_fingerprints(&fixture.store_root), before);

        let foreign_submission_id = format!("sub-{}", "f".repeat(64));
        let foreign_submission = fixture
            .store_root
            .join("staging/inbox")
            .join(&foreign_submission_id);
        fs::create_dir(&foreign_submission).expect("create foreign untrusted submission");
        fs::write(foreign_submission.join("candidate.md"), b"foreign\n")
            .expect("write foreign candidate");
        fs::write(foreign_submission.join("submission.meta.json"), b"{}")
            .expect("write foreign metadata");
        let foreign_before = fingerprint(&foreign_submission);

        for action in PROTECTED_MATRIX_ACTIONS {
            for target in PROTECTED_ROOTS
                .into_iter()
                .chain(std::iter::once(foreign_submission_id.as_str()))
            {
                let target = if target == foreign_submission_id {
                    format!("staging/inbox/{foreign_submission_id}")
                } else {
                    target.to_owned()
                };
                assert!(
                    child.run(action, None, Some(&target), false),
                    "restricted proposer child did not confirm {action} denial for {target}"
                );
                assert_eq!(protected_fingerprints(&fixture.store_root), before);
                assert_eq!(fingerprint(&foreign_submission), foreign_before);
            }
        }
        for (action, target) in [
            ("hard-link-protected-into-own", "policy.toml".to_owned()),
            ("symlink-own-to-protected", "policy.toml".to_owned()),
            ("rename-protected-into-own", "policy.toml".to_owned()),
            (
                "rename-foreign-into-own",
                format!("staging/inbox/{foreign_submission_id}/candidate.md"),
            ),
            ("rename-replace-protected", "policy.toml".to_owned()),
        ] {
            assert!(
                child.run(action, None, Some(&target), false),
                "restricted proposer child did not confirm {action} denial for {target}"
            );
            assert_eq!(protected_fingerprints(&fixture.store_root), before);
            assert_eq!(fingerprint(&foreign_submission), foreign_before);
        }
        assert!(
            child.run("write", None, Some("canon"), true),
            "spoofed environment, user, or actor expanded proposer authority"
        );
        assert_eq!(protected_fingerprints(&fixture.store_root), before);

        Ok(RestrictedCapability::Supported)
    }

    fn capability_candidate_bytes(fixture: &ProvisionedP3Fixture) -> Vec<u8> {
        let seed_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let bytes = fs::read(fixture.store_root.join("seed/bundle").join(seed_path))
            .expect("read seeded template");
        let parsed = parse_source_document(SourceDocument {
            source_path: seed_path.to_owned(),
            source_hash: blake3::hash(&bytes).to_hex().to_string(),
            bytes,
            created: String::new(),
            updated: String::new(),
        })
        .expect("parse seeded template");
        let mut metadata: GeneratedMetadata = serde_yaml::from_value(
            serde_yaml::to_value(parsed.metadata).expect("encode seeded template metadata"),
        )
        .expect("decode seeded template metadata");
        metadata.id = TARGET_ID.to_owned();
        metadata.source_path = TARGET_PATH.to_owned();
        metadata.source = format!("vault://{TARGET_ID}");
        metadata.resource = format!("vault://{TARGET_ID}");
        metadata.title = "P3 generated canonical fixture".to_owned();
        metadata.description = "Deterministic P3 create fixture.".to_owned();
        if let Some(mapping) = load_p3_profile(&fixture.profile_path)
            .expect("load P3 profile for fixture candidate")
            .config
            .mapping_for(TARGET_PATH)
        {
            metadata.core_type = mapping.core_type.clone();
            metadata.context_kind = mapping.context_kind.clone();
            metadata.layer = mapping.layer.clone();
            metadata.authority = mapping.authority.clone();
        }
        render_document(&GeneratedDocument {
            metadata,
            body: format!("{}\nP3 fixture create exact bytes.\n", parsed.body),
            source_hash: String::new(),
        })
        .expect("render create candidate")
    }

    fn capability_submission(
        fixture: &ProvisionedP3Fixture,
        candidate: &Path,
    ) -> ProposalSubmission {
        ProposalSubmission {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            source_id: "src:agent:capability-fixture".to_owned(),
            operation: ProposalOperation::Create,
            intent: ProposalIntent::Accept,
            target_id: TARGET_ID.to_owned(),
            target_path: TARGET_PATH.to_owned(),
            candidate_path: candidate.to_owned(),
            confidence: None,
            evaluator_path: None,
        }
    }

    fn usable_sandbox() -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            let path = PathBuf::from("/usr/bin/sandbox-exec");
            if !path.is_file() {
                return None;
            }
            let executable = std::env::current_exe().ok()?;
            let probe = Command::new(&path)
                .args([
                    "-p",
                    "(version 1) (deny default) (allow process*) (allow file-read*)",
                ])
                .arg(executable)
                .arg("--list")
                .output()
                .ok()?;
            probe.status.success().then_some(path)
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    fn sandbox_profile(store: &Path, submission_id: &str) -> String {
        let inbox = store.join("staging/inbox");
        let custody = inbox.join(format!(".submission-{submission_id}"));
        let submission = inbox.join(submission_id);
        format!(
            "(version 1)\n(deny default)\n(allow process*)\n(allow file-read*)\n(allow file-write-data (literal {}))\n(allow file-write-create (literal {}))\n(allow file-write* (subpath {}))\n(allow file-write-create (literal {}))\n(allow file-write* (subpath {}))\n",
            sandbox_literal(&inbox),
            sandbox_literal(&custody),
            sandbox_literal(&custody),
            sandbox_literal(&submission),
            sandbox_literal(&submission),
        )
    }

    fn expected_sandbox_profile(store: &Path, submission_id: &str) -> String {
        let inbox = store.join("staging/inbox");
        let custody = inbox.join(format!(".submission-{submission_id}"));
        let submission = inbox.join(submission_id);
        format!(
            "(version 1)\n(deny default)\n(allow process*)\n(allow file-read*)\n(allow file-write-data (literal {}))\n(allow file-write-create (literal {}))\n(allow file-write* (subpath {}))\n(allow file-write-create (literal {}))\n(allow file-write* (subpath {}))\n",
            sandbox_literal(&inbox),
            sandbox_literal(&custody),
            sandbox_literal(&custody),
            sandbox_literal(&submission),
            sandbox_literal(&submission),
        )
    }

    fn sandbox_profile_guard(profile: &str, store: &Path, submission_id: &str) -> bool {
        profile == expected_sandbox_profile(store, submission_id)
    }

    fn sandbox_literal(path: &Path) -> String {
        let path = path
            .to_str()
            .expect("capability fixture paths are UTF-8")
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        format!("\"{path}\"")
    }

    fn protected_fingerprints(store: &Path) -> BTreeMap<&'static str, String> {
        PROTECTED_ROOTS
            .into_iter()
            .map(|root| (root, fingerprint(&store.join(root))))
            .collect()
    }

    fn fingerprint(path: &Path) -> String {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return "absent".to_owned();
        };
        if metadata.is_file() {
            return blake3::hash(&fs::read(path).expect("read protected fixture file"))
                .to_hex()
                .to_string();
        }
        assert!(metadata.is_dir(), "protected fixture path must be regular");
        let mut entries = fs::read_dir(path)
            .expect("read protected fixture directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect protected fixture entries");
        entries.sort_by_key(|entry| entry.file_name());
        let mut bytes = Vec::new();
        for entry in entries {
            let name = entry.file_name();
            let child = entry.path();
            bytes.extend_from_slice(name.as_encoded_bytes());
            bytes.push(0);
            bytes.extend_from_slice(fingerprint(&child).as_bytes());
            bytes.push(b'\n');
        }
        blake3::hash(&bytes).to_hex().to_string()
    }

    fn child_value(key: &str) -> PathBuf {
        std::env::var_os(key)
            .map(PathBuf::from)
            .expect("restricted capability child received its private fixture value")
    }

    fn child_operation_is_denied(store: &Path, own_submission: &Path, action: &str, target: &str) {
        let target = store.join(target);
        let protected_destination = if target.is_dir() {
            target.join(format!("capability-{action}"))
        } else if target.exists() {
            target.with_extension(format!("capability-{action}"))
        } else {
            target.clone()
        };
        let source = own_submission.join(format!(
            "capability-source-{}",
            blake3::hash(format!("{action}:{}", target.display()).as_bytes()).to_hex()
        ));
        let own_destination = own_submission.join(format!(
            "capability-destination-{}",
            blake3::hash(format!("{action}:{}", target.display()).as_bytes()).to_hex()
        ));
        if matches!(
            action,
            "rename" | "hard-link" | "symlink" | "rename-replace-protected"
        ) {
            fs::write(&source, b"untrusted capability source\n")
                .expect("restricted proposer can prepare its own operation source");
        }
        let result = match action {
            "create" => fs::create_dir(&protected_destination),
            "write" => {
                if target.is_file() {
                    fs::OpenOptions::new().write(true).open(&target).map(|_| ())
                } else {
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&protected_destination)
                        .map(|_| ())
                }
            }
            "rename" => fs::rename(&source, &protected_destination),
            "remove" => {
                if target.is_dir() {
                    fs::remove_dir(&target)
                } else {
                    fs::remove_file(&target)
                }
            }
            "hard-link" => fs::hard_link(&source, &protected_destination),
            "symlink" => {
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&source, &protected_destination)
                }
                #[cfg(not(unix))]
                {
                    panic!("the approved real sandbox fixture is unavailable on this platform")
                }
            }
            "hard-link-protected-into-own" => fs::hard_link(&target, &own_destination)
                .and_then(|()| fs::OpenOptions::new().write(true).open(&own_destination))
                .and_then(|mut alias| alias.write_all(b"forbidden protected mutation\n")),
            "symlink-own-to-protected" => {
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&target, &own_destination).expect(
                        "restricted proposer can create a symlink inside its own submission",
                    );
                    fs::OpenOptions::new()
                        .write(true)
                        .open(&own_destination)
                        .and_then(|mut alias| alias.write_all(b"forbidden protected mutation\n"))
                }
                #[cfg(not(unix))]
                {
                    panic!("the approved real sandbox fixture is unavailable on this platform")
                }
            }
            "rename-protected-into-own" | "rename-foreign-into-own" => {
                fs::rename(&target, &own_destination)
            }
            "rename-replace-protected" => fs::rename(&source, &target),
            _ => panic!("restricted capability action is invalid"),
        };
        assert_eq!(
            result
                .expect_err("real sandbox must deny the protected operation")
                .kind(),
            io::ErrorKind::PermissionDenied,
            "protected operation must be denied by the real sandbox, not incidental filesystem state"
        );
        if fs::symlink_metadata(&own_destination).is_ok() {
            fs::remove_file(&own_destination).expect("remove own operation destination");
        }
        if source.exists() {
            fs::remove_file(source).expect("remove own operation source");
        }
    }

    #[test]
    fn restricted_proposer_child_entry() {
        let Some(action) = std::env::var_os(CHILD_ACTION) else {
            return;
        };
        let profile = child_value(CHILD_PROFILE);
        let store = child_value(CHILD_STORE);
        let candidate = child_value(CHILD_CANDIDATE);
        let submission_id = std::env::var(CHILD_PROPOSAL)
            .expect("restricted capability child received its own submission ID");
        match action.to_str() {
            Some("stage") => {
                submit_proposal_with_clock(
                    ProposalSubmission {
                        profile_path: profile,
                        store_root: store,
                        source_id: "src:agent:capability-fixture".to_owned(),
                        operation: ProposalOperation::Create,
                        intent: ProposalIntent::Accept,
                        target_id: TARGET_ID.to_owned(),
                        target_path: TARGET_PATH.to_owned(),
                        candidate_path: candidate,
                        confidence: None,
                        evaluator_path: None,
                    },
                    &FixedClock(Timestamp::parse(FIXED_TIME).expect("fixed capability timestamp")),
                )
                .expect("restricted proposer creates its exact untrusted request");
            }
            Some("no-tty-seal") => {
                assert!(matches!(
                    seal_intake_submission(&profile, &store, &submission_id),
                    Err(TransactionError::TtyRequired)
                ));
            }
            Some(action) if PROTECTED_ACTIONS.contains(&action) => {
                let target = std::env::var(CHILD_TARGET)
                    .expect("restricted capability child received protected target");
                child_operation_is_denied(
                    &store,
                    &store.join("staging/inbox").join(&submission_id),
                    action,
                    &target,
                );
            }
            _ => panic!("restricted capability child action is invalid"),
        }
    }

    #[test]
    fn fixture_reports_an_explicit_unsupported_result_without_a_usable_os_sandbox() {
        if usable_sandbox().is_none() {
            assert_eq!(
                exercise_restricted_proposer_fixture(),
                Err(PERMISSION_MODEL_UNSUPPORTED)
            );
        }
    }

    #[test]
    fn same_user_checks_are_supplementary_and_never_satisfy_strict_capability() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let root = std::env::temp_dir()
                .join(format!("sovmem-p3-same-user-mode-{}", std::process::id()));
            fs::create_dir(&root).expect("create same-user mode fixture");
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .expect("apply same-user mode fixture");
            if usable_sandbox().is_none() {
                assert_eq!(
                    exercise_restricted_proposer_fixture(),
                    Err(PERMISSION_MODEL_UNSUPPORTED),
                    "same-user mode bits cannot satisfy the strict capability gate"
                );
            }
            fs::remove_dir(&root).expect("remove same-user mode fixture");
        }
    }

    #[test]
    fn strict_capability_uses_a_real_sandbox_or_reports_the_approved_unsupported_result() {
        assert!(matches!(
            exercise_restricted_proposer_fixture(),
            Ok(RestrictedCapability::Supported) | Err(PERMISSION_MODEL_UNSUPPORTED)
        ));
    }

    #[test]
    fn proposer_sandbox_profile_is_scoped_to_exactly_one_inbox_submission() {
        let store = Path::new("/private/tmp/sovmem-capability-profile");
        let submission_id = format!("sub-{}", "a".repeat(64));
        let profile = sandbox_profile(store, &submission_id);

        assert!(sandbox_profile_guard(&profile, store, &submission_id));
    }

    #[test]
    fn proposer_sandbox_profile_guard_rejects_an_added_grant() {
        let store = Path::new("/private/tmp/sovmem-capability-profile");
        let submission_id = format!("sub-{}", "a".repeat(64));
        let mutated = format!(
            "{}(allow file-write-data (literal {}))\n",
            sandbox_profile(store, &submission_id),
            sandbox_literal(&store.join("ledger/verdicts.jsonl"))
        );

        assert!(
            !sandbox_profile_guard(&mutated, store, &submission_id),
            "an added protected-path grant must be rejected"
        );
    }

    #[test]
    fn proposer_protected_root_matrix_covers_the_complete_v2_authority() {
        assert_eq!(
            &PROTECTED_ROOTS[..],
            &[
                "intake/events.jsonl",
                "intake/proposals",
                "intake/transactions",
                "locks",
                "ledger",
                "canon",
                "policies",
                "evidence",
                "derived",
                "transactions",
                "policy.toml",
            ][..]
        );
    }

    #[test]
    fn proposer_capability_matrix_covers_inverse_links_and_exact_protected_replacement() {
        assert_eq!(
            &PROTECTED_ACTIONS[..],
            &[
                "create",
                "write",
                "rename",
                "remove",
                "hard-link",
                "symlink",
                "hard-link-protected-into-own",
                "symlink-own-to-protected",
                "rename-protected-into-own",
                "rename-foreign-into-own",
                "rename-replace-protected",
            ][..]
        );
    }
}

pub mod determinism_tests {
    #[allow(unused_imports)]
    use super::full_loop_tests;
    use super::full_loop_tests::{run_complete_loop, run_nonapplying_verdict};
    #[allow(unused_imports)]
    use super::repository_path;
    use serde::{Deserialize, Serialize};
    use sovmem::profile::p3_test_support::ProvisionedP3Fixture;
    use sovmem::profile::{
        inspect_store, load_p3_profile, query_store, verify_store_level4, ExpansionMode,
        StoreQueryRequest, VerdictVerb,
    };
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    const FIXTURE_SCHEMA: &str = "sovmem-p3-cli-store-fixture-v2";

    #[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
    #[serde(deny_unknown_fields)]
    struct FixtureManifest {
        schema: String,
        profile_hash: String,
        policy_hash: String,
        seed_bundle_hash: String,
        directories: Vec<String>,
        files: BTreeMap<String, String>,
        tree_hash: String,
        events: usize,
        verdicts: usize,
        applied_generations: usize,
        current_generation: String,
    }

    struct FixtureTree {
        directories: Vec<String>,
        files: BTreeMap<String, String>,
    }

    #[test]
    fn one_applied_cli_store_manifest_is_reproducible_from_private_provisioning() {
        let fixture = ProvisionedP3Fixture::empty("p3-v2-repro-one");
        run_nonapplying_verdict(&fixture, VerdictVerb::Reject);
        run_complete_loop(&fixture, false);
        clear_disposable_indexes(&fixture);
        assert_static_variant("one-applied", &fixture);
    }

    #[test]
    fn two_applied_cli_store_manifest_is_reproducible_from_private_provisioning() {
        let fixture = ProvisionedP3Fixture::empty("p3-v2-repro-two");
        run_nonapplying_verdict(&fixture, VerdictVerb::Reject);
        run_complete_loop(&fixture, false);
        run_complete_loop(&fixture, true);
        clear_disposable_indexes(&fixture);
        assert_static_variant("two-applied", &fixture);
    }

    #[test]
    #[ignore = "writes the reviewed Task 10 v2 fixture trees"]
    fn regenerate_v2_cli_store_fixtures_through_supported_transactions() {
        let fixture = ProvisionedP3Fixture::empty("p3-v2-regenerate");
        run_nonapplying_verdict(&fixture, VerdictVerb::Reject);
        run_complete_loop(&fixture, false);
        clear_disposable_indexes(&fixture);
        write_static_variant("one-applied", &fixture);
        run_complete_loop(&fixture, true);
        clear_disposable_indexes(&fixture);
        write_static_variant("two-applied", &fixture);
    }

    #[test]
    fn fault_free_transaction_sequences_are_byte_identical_across_separate_store_roots() {
        let left = ProvisionedP3Fixture::empty("task-10-two-store-left");
        run_complete_loop(&left, false);
        run_complete_loop(&left, true);
        let left_durable_files = durable_file_map(&left);
        let left_index_snapshot = rebuilt_index_semantic_snapshot(&left);
        drop(left);

        let right = ProvisionedP3Fixture::empty("task-10-two-store-right");
        run_complete_loop(&right, false);
        run_complete_loop(&right, true);

        assert_eq!(
            left_durable_files,
            durable_file_map(&right),
            "durable P3 bytes must not depend on the absolute fixture root"
        );
        assert_eq!(
            left_index_snapshot,
            rebuilt_index_semantic_snapshot(&right),
            "rebuilt P3 index semantics must not depend on the absolute fixture root"
        );
    }

    fn assert_static_variant(variant: &str, fixture: &ProvisionedP3Fixture) {
        let expected = fixture_manifest(fixture);
        let root = fixture_root(variant);
        let actual: FixtureManifest = serde_json::from_slice(
            &fs::read(root.join("fixture.json")).expect("read checked-in fixture manifest"),
        )
        .expect("parse checked-in fixture manifest");
        assert_eq!(actual, expected, "fixture manifest for {variant}");
    }

    fn durable_file_map(fixture: &ProvisionedP3Fixture) -> BTreeMap<String, Vec<u8>> {
        assert_disposable_state_absent(fixture);
        let mut files = BTreeMap::new();
        for relative in [
            "seed",
            "policy.toml",
            "policies",
            "intake/events.jsonl",
            "intake/proposals",
            "intake/transactions",
            "canon/CURRENT",
            "canon/generations",
            "evidence/reindex",
            "ledger",
        ] {
            collect_durable_files(&fixture.store_root.join(relative), relative, &mut files);
        }
        files
    }

    fn assert_disposable_state_absent(fixture: &ProvisionedP3Fixture) {
        for relative in ["locks", "transactions", "staging/inbox"] {
            let mut entries = fs::read_dir(fixture.store_root.join(relative))
                .expect("read disposable fixture directory");
            assert!(
                entries.next().is_none(),
                "fault-free sequence must leave no disposable {relative} entries"
            );
        }
    }

    fn clear_disposable_indexes(fixture: &ProvisionedP3Fixture) {
        let indexes = fixture.store_root.join("derived/indexes");
        fs::remove_dir_all(&indexes).expect("remove disposable fixture indexes");
        fs::create_dir(&indexes).expect("recreate empty fixture indexes directory");
    }

    fn collect_durable_files(root: &Path, relative: &str, files: &mut BTreeMap<String, Vec<u8>>) {
        let metadata = fs::symlink_metadata(root).expect("read durable fixture entry");
        assert!(
            !metadata.file_type().is_symlink(),
            "durable fixture entry cannot be a symlink: {relative}"
        );
        if metadata.is_file() {
            assert!(
                files
                    .insert(
                        relative.to_owned(),
                        fs::read(root).expect("read durable fixture file")
                    )
                    .is_none(),
                "duplicate durable fixture path: {relative}"
            );
            return;
        }
        assert!(metadata.is_dir(), "durable fixture entry must be regular");
        let mut entries = fs::read_dir(root)
            .expect("read durable fixture directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read durable fixture entry");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            let child = format!("{relative}/{}", name.to_string_lossy());
            collect_durable_files(&entry.path(), &child, files);
        }
    }

    fn rebuilt_index_semantic_snapshot(fixture: &ProvisionedP3Fixture) -> BTreeMap<String, String> {
        let queries = [
            "zztask17acceptphraseq7",
            "zztask17rewritephraseq7",
            "fixture applied accept",
            "fixture superseding accept",
        ];
        queries
            .into_iter()
            .map(|query| {
                let result = query_store(StoreQueryRequest {
                    profile_path: fixture.profile_path.clone(),
                    store_root: fixture.store_root.clone(),
                    query: query.to_owned(),
                    top_k: 5,
                    expansion: ExpansionMode::None,
                })
                .expect("query rebuilt deterministic P3 index")
                .result;
                (
                    query.to_owned(),
                    blake3::hash(
                        &serde_json::to_vec(&result).expect("serialize semantic P3 index snapshot"),
                    )
                    .to_hex()
                    .to_string(),
                )
            })
            .collect()
    }

    fn fixture_manifest(fixture: &ProvisionedP3Fixture) -> FixtureManifest {
        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 fixture profile");
        let store =
            inspect_store(&profile, &fixture.store_root).expect("inspect P3 v2 fixture store");
        let report =
            verify_store_level4(&profile, &fixture.store_root).expect("verify fixture Level 4");
        let tree = collect_tree(&fixture.store_root);
        FixtureManifest {
            schema: FIXTURE_SCHEMA.to_owned(),
            profile_hash: profile.hash,
            policy_hash: store.policy.hash,
            seed_bundle_hash: store.seed.bundle_hash,
            tree_hash: tree_hash(&tree),
            directories: tree.directories,
            files: tree.files,
            events: report.events,
            verdicts: report.verdicts,
            applied_generations: report.applied_generations,
            current_generation: report.current_generation,
        }
    }

    fn fixture_root(variant: &str) -> PathBuf {
        assert!(matches!(variant, "one-applied" | "two-applied"));
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/p3-cli-store-v2")
            .join(variant)
    }

    fn write_static_variant(variant: &str, fixture: &ProvisionedP3Fixture) {
        let root = fixture_root(variant);
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove prior generated v2 fixture root");
        }
        fs::create_dir_all(&root).expect("create generated v2 fixture root");
        copy_tree(&fixture.store_root, &root.join("store"));
        let mut manifest = serde_json::to_vec_pretty(&fixture_manifest(fixture))
            .expect("render v2 fixture manifest");
        manifest.push(b'\n');
        fs::write(root.join("fixture.json"), manifest).expect("write v2 fixture manifest");
    }

    fn copy_tree(source: &Path, destination: &Path) {
        let metadata =
            fs::symlink_metadata(source).expect("read generated fixture source metadata");
        assert!(!metadata.file_type().is_symlink());
        if metadata.is_file() {
            fs::write(
                destination,
                fs::read(source).expect("read generated fixture source file"),
            )
            .expect("write generated fixture destination file");
            return;
        }
        assert!(metadata.is_dir());
        fs::create_dir(destination).expect("create generated fixture destination directory");
        let mut entries = fs::read_dir(source)
            .expect("read generated fixture source directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect generated fixture source entries");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_tree(&entry.path(), &destination.join(entry.file_name()));
        }
    }

    fn collect_tree(root: &Path) -> FixtureTree {
        let metadata = fs::symlink_metadata(root).expect("read fixture tree root metadata");
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        let mut tree = FixtureTree {
            directories: Vec::new(),
            files: BTreeMap::new(),
        };
        collect_tree_entries(root, root, &mut tree);
        tree
    }

    fn collect_tree_entries(root: &Path, current: &Path, tree: &mut FixtureTree) {
        let mut entries = fs::read_dir(current)
            .expect("read fixture tree directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read fixture tree entry");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("read fixture entry metadata");
            assert!(
                !metadata.file_type().is_symlink(),
                "fixture tree cannot contain symlinks"
            );
            let relative = path
                .strip_prefix(root)
                .expect("fixture entry remains beneath root")
                .to_str()
                .expect("fixture path is UTF-8")
                .replace('\\', "/");
            if metadata.is_dir() {
                tree.directories.push(relative);
                collect_tree_entries(root, &path, tree);
            } else if metadata.is_file() {
                tree.files.insert(
                    relative,
                    blake3::hash(&fs::read(&path).expect("read fixture file"))
                        .to_hex()
                        .to_string(),
                );
            } else {
                panic!("fixture tree entry must be regular");
            }
        }
    }

    fn tree_hash(tree: &FixtureTree) -> String {
        let mut bytes = Vec::new();
        for directory in &tree.directories {
            bytes.extend_from_slice(b"D\0");
            bytes.extend_from_slice(directory.as_bytes());
            bytes.push(b'\n');
        }
        for (path, hash) in &tree.files {
            bytes.extend_from_slice(b"F\0");
            bytes.extend_from_slice(path.as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(hash.as_bytes());
            bytes.push(b'\n');
        }
        blake3::hash(&bytes).to_hex().to_string()
    }
}

pub mod fixture_concurrency_tests {
    use sovmem::profile::p3_test_support::{
        FaultPlan, FixedClock, ProvisionedP3Fixture, RecoveryLeasePublicationBarrier, ScriptedHuman,
    };
    use sovmem::profile::transaction::{
        install_intake_seal_after_candidate_cleanup_hook_for_test,
        recover_store_with_human_for_test, seal_intake_submission_with_human,
        seal_intake_submission_with_human_and_faults, FaultPoint, HumanPresence, RecoveryMode,
        RecoveryRequest, TransactionError,
    };
    use sovmem::profile::{
        inspect_store, load_p3_profile, submit_proposal_with_clock,
        submit_proposal_with_final_publication_hook, IntakeLeaseIdentity, IntakePublicationLease,
        ProposalIntent, ProposalOperation, ProposalSubmission, SubmitProposalResult, Timestamp,
        VerdictVerb,
    };
    use std::collections::BTreeMap;
    use std::fs;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::path::Path;
    use std::sync::{Arc, Condvar, Mutex};
    use std::thread;

    #[test]
    fn intake_matching_reseal_returns_the_same_outcome_without_duplicate_authority() {
        let fixture = ProvisionedP3Fixture::empty("intake-idempotent-reseal");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate_path = fixture.profile_path.parent().unwrap().join("reseal.md");
        let candidate = String::from_utf8(
            fs::read(fixture.store_root.join("seed/bundle").join(target_path)).unwrap(),
        )
        .unwrap()
        .replace(target_id.trim_end_matches(".md"), target_id)
            + "\nTask 8 idempotent reseal fixture\n";
        fs::write(&candidate_path, candidate).unwrap();
        let submitted = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:task8-reseal".to_owned(),
                operation: ProposalOperation::Replace,
                intent: ProposalIntent::Accept,
                target_id: target_id.to_owned(),
                target_path: target_path.to_owned(),
                candidate_path,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(Timestamp::parse("2026-07-22T01:00:00.000000000Z").unwrap()),
        )
        .unwrap();
        let mut first_human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let first = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut first_human,
            &FixedClock(Timestamp::parse("2026-07-22T01:00:01.000000000Z").unwrap()),
        )
        .unwrap();
        assert!(!first.idempotent);
        let events_before = fs::read(fixture.store_root.join("intake/events.jsonl")).unwrap();
        let pairs_before = fs::read_dir(fixture.store_root.join("intake/proposals"))
            .unwrap()
            .flat_map(|shard| fs::read_dir(shard.unwrap().path()).unwrap())
            .count();

        let mut replay_human = ScriptedHuman::new(None);
        let replay = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut replay_human,
            &FixedClock(Timestamp::parse("2026-07-22T01:00:02.000000000Z").unwrap()),
        )
        .expect("a matching committed intake seal must replay idempotently");

        assert_eq!(replay.submission_id, first.submission_id);
        assert_eq!(replay.proposal_id, first.proposal_id);
        assert_eq!(replay.state, first.state);
        assert_eq!(replay.intake_sequence, first.intake_sequence);
        assert_eq!(replay.intake_head_hash, first.intake_head_hash);
        assert_eq!(replay.policy_hash, first.policy_hash);
        assert_eq!(replay.profile_hash, first.profile_hash);
        assert!(replay.idempotent);
        assert_eq!(replay_human.show_count(), 0);
        assert_eq!(replay_human.read_count(), 0);
        assert_eq!(
            fs::read(fixture.store_root.join("intake/events.jsonl")).unwrap(),
            events_before
        );
        assert_eq!(
            fs::read_dir(fixture.store_root.join("intake/proposals"))
                .unwrap()
                .flat_map(|shard| fs::read_dir(shard.unwrap().path()).unwrap())
                .count(),
            pairs_before
        );
    }

    #[test]
    fn intake_matching_extant_submission_reseal_is_idempotent_without_duplicate_authority() {
        let (fixture, submitted) = task8_submission("intake-idempotent-extant-matching");
        let inbox = fixture
            .store_root
            .join("staging/inbox")
            .join(&submitted.submission_id);
        let candidate = fs::read(inbox.join("candidate.md")).unwrap();
        let meta = fs::read(inbox.join("submission.meta.json")).unwrap();
        let mut first_human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let first = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut first_human,
            &FixedClock(Timestamp::parse("2026-07-22T01:01:00.000000000Z").unwrap()),
        )
        .unwrap();
        fs::create_dir(&inbox).unwrap();
        fs::write(inbox.join("candidate.md"), &candidate).unwrap();
        fs::write(inbox.join("submission.meta.json"), &meta).unwrap();
        let authority_before = task8_authority_commitment(&fixture.store_root);
        let events_before = fs::read(fixture.store_root.join("intake/events.jsonl")).unwrap();
        let pairs_before = task8_pair_count(&fixture);

        let mut replay_human = ScriptedHuman::new(None);
        let replay = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut replay_human,
            &FixedClock(Timestamp::parse("2026-07-22T01:01:01.000000000Z").unwrap()),
        )
        .expect("an exact extant submission must replay its protected outcome");

        assert_eq!(replay.submission_id, first.submission_id);
        assert_eq!(replay.proposal_id, first.proposal_id);
        assert_eq!(replay.state, first.state);
        assert_eq!(replay.intake_sequence, first.intake_sequence);
        assert_eq!(replay.intake_head_hash, first.intake_head_hash);
        assert_eq!(replay.policy_hash, first.policy_hash);
        assert_eq!(replay.profile_hash, first.profile_hash);
        assert!(replay.idempotent);
        assert_eq!(replay_human.show_count(), 0);
        assert_eq!(replay_human.read_count(), 0);
        assert_eq!(
            fs::read(fixture.store_root.join("intake/events.jsonl")).unwrap(),
            events_before
        );
        assert_eq!(task8_pair_count(&fixture), pairs_before);
        assert_eq!(
            task8_authority_commitment(&fixture.store_root),
            authority_before
        );
    }

    #[test]
    fn intake_disagreeing_extant_submission_reseal_is_incomplete_without_mutation() {
        let (fixture, submitted) = task8_submission("intake-idempotent-extant-disagreeing");
        let inbox = fixture
            .store_root
            .join("staging/inbox")
            .join(&submitted.submission_id);
        let mut candidate = fs::read(inbox.join("candidate.md")).unwrap();
        let meta = fs::read(inbox.join("submission.meta.json")).unwrap();
        let mut first_human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut first_human,
            &FixedClock(Timestamp::parse("2026-07-22T01:02:00.000000000Z").unwrap()),
        )
        .unwrap();
        candidate.extend_from_slice(b"DISAGREEING_EXTANT_SUBMISSION\n");
        fs::create_dir(&inbox).unwrap();
        fs::write(inbox.join("candidate.md"), candidate).unwrap();
        fs::write(inbox.join("submission.meta.json"), meta).unwrap();
        let store_before = task8_authority_commitment(&fixture.store_root);
        let events_before = fs::read(fixture.store_root.join("intake/events.jsonl")).unwrap();
        let pairs_before = task8_pair_count(&fixture);

        let mut replay_human = ScriptedHuman::new(None);
        let error = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut replay_human,
            &FixedClock(Timestamp::parse("2026-07-22T01:02:01.000000000Z").unwrap()),
        )
        .unwrap_err();

        assert_eq!(error.code(), "intake.incomplete");
        assert_eq!(replay_human.show_count(), 0);
        assert_eq!(replay_human.read_count(), 0);
        assert_eq!(
            fs::read(fixture.store_root.join("intake/events.jsonl")).unwrap(),
            events_before
        );
        assert_eq!(task8_pair_count(&fixture), pairs_before);
        assert_eq!(
            task8_authority_commitment(&fixture.store_root),
            store_before
        );
    }

    #[test]
    fn intake_crash_boundaries_retain_protected_custody_or_recover_one_committed_outcome() {
        for point in [
            FaultPoint::AfterInboxRead,
            FaultPoint::AfterIntakeJournalSync,
            FaultPoint::AfterPairSync,
            FaultPoint::BeforeIntakeEventRename,
            FaultPoint::AfterIntakeEventRename,
            FaultPoint::BeforeInboxCleanup,
        ] {
            let (fixture, submitted) = task8_submission(&format!("intake-crash-{point:?}"));
            let mut human =
                ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
            let result = seal_intake_submission_with_human_and_faults(
                &fixture.profile_path,
                &fixture.store_root,
                &submitted.submission_id,
                &mut human,
                &FixedClock(Timestamp::parse("2026-07-22T01:10:01.000000000Z").unwrap()),
                &FaultPlan::at(point),
            );
            let inbox = fixture
                .store_root
                .join("staging/inbox")
                .join(&submitted.submission_id);
            let custody = fixture
                .store_root
                .join("intake/transactions")
                .join(&submitted.submission_id);
            let events = || {
                fs::read_to_string(fixture.store_root.join("intake/events.jsonl"))
                    .unwrap()
                    .lines()
                    .count()
            };
            assert_eq!(result.unwrap_err().code(), "intake.incomplete", "{point:?}");
            assert!(!inbox.exists(), "{point:?}");
            assert!(custody.is_dir(), "{point:?}");
            assert!(fixture.store_root.join("locks/intake.lock").is_dir());
            let profile = load_p3_profile(&fixture.profile_path).unwrap();
            assert_eq!(
                inspect_store(&profile, &fixture.store_root)
                    .unwrap_err()
                    .code(),
                "transaction.incomplete",
                "{point:?}"
            );

            if matches!(
                point,
                FaultPoint::AfterIntakeEventRename | FaultPoint::BeforeInboxCleanup
            ) {
                let recovered = fixture
                    .recover_intake_with_runtime(&fixture.runtime())
                    .unwrap_or_else(|error| panic!("{point:?} must recover: {error:?}"));
                assert_eq!(recovered.mode, RecoveryMode::IntakeOnly, "{point:?}");
                assert!(fixture.leases_absent(), "{point:?}");
                assert!(
                    inspect_store(&profile, &fixture.store_root).is_ok(),
                    "{point:?}"
                );
                assert_eq!(events(), 1, "{point:?}");
                assert_eq!(task8_pair_count(&fixture), 1, "{point:?}");
                assert!(!inbox.exists(), "{point:?}");
                assert!(custody.join("published.json").is_file(), "{point:?}");
            } else {
                let intake_before = task8_authority_commitment(&fixture.store_root.join("intake"));
                let recovery = fixture
                    .recover_intake_with_runtime(&fixture.runtime())
                    .unwrap_err();
                assert_eq!(recovery.code(), "intake.incomplete", "{point:?}");
                assert_eq!(
                    task8_authority_commitment(&fixture.store_root.join("intake")),
                    intake_before,
                    "{point:?}"
                );
                assert_eq!(events(), 0, "{point:?}");
                assert_eq!(
                    task8_pair_count(&fixture),
                    usize::from(matches!(
                        point,
                        FaultPoint::AfterPairSync | FaultPoint::BeforeIntakeEventRename
                    )),
                    "{point:?}"
                );
                assert!(!inbox.exists(), "{point:?}");
                assert!(custody.is_dir(), "{point:?}");
                assert_eq!(
                    inspect_store(&profile, &fixture.store_root)
                        .unwrap_err()
                        .code(),
                    "transaction.incomplete",
                    "{point:?}"
                );
            }
        }
    }

    #[test]
    fn intake_committed_cleanup_interruption_after_candidate_unlink_recovers_monotonically() {
        let (fixture, submitted) = task8_submission("intake-committed-cleanup-interruption");
        install_intake_seal_after_candidate_cleanup_hook_for_test(|| {
            panic!("fixture interruption after committed candidate cleanup")
        });
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));

        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            seal_intake_submission_with_human(
                &fixture.profile_path,
                &fixture.store_root,
                &submitted.submission_id,
                &mut human,
                &FixedClock(Timestamp::parse("2026-07-22T01:19:00.000000000Z").unwrap()),
            )
        }));
        assert!(
            interrupted.is_err(),
            "the cleanup interruption must execute"
        );

        let custody = fixture
            .store_root
            .join("intake/transactions")
            .join(&submitted.submission_id);
        assert!(!custody.join("candidate.md").exists());
        assert!(custody.join("submission.meta.json").is_file());
        assert!(custody.join("prepared.json").is_file());
        assert!(custody.join("committed.json").is_file());
        assert!(custody.join("published.json").is_file());
        assert_eq!(task8_pair_count(&fixture), 1);
        assert_eq!(
            fs::read_to_string(fixture.store_root.join("intake/events.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );

        let recovered = fixture
            .recover_intake_with_runtime(&fixture.runtime())
            .expect("committed cleanup recovery must tolerate the missing candidate");
        assert_eq!(recovered.mode, RecoveryMode::IntakeOnly);
        let mut receipt_names = fs::read_dir(&custody)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        receipt_names.sort();
        assert_eq!(
            receipt_names,
            ["committed.json", "prepared.json", "published.json"]
        );
        assert!(fixture.leases_absent());
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        assert!(inspect_store(&profile, &fixture.store_root).is_ok());

        let mut replay_human = ScriptedHuman::new(None);
        let replay = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut replay_human,
            &FixedClock(Timestamp::parse("2026-07-22T01:19:01.000000000Z").unwrap()),
        )
        .expect("the verified completed receipt must replay idempotently");
        assert!(replay.idempotent);
        assert_eq!(replay_human.show_count(), 0);
        assert_eq!(replay_human.read_count(), 0);
    }

    #[test]
    fn intake_committed_cleanup_tail_accepts_either_or_both_raw_inputs_absent() {
        for (case, remove_candidate, remove_meta) in [
            ("candidate-absent", true, false),
            ("metadata-absent", false, true),
            ("both-absent", true, true),
        ] {
            let (fixture, submitted) = task8_submission(&format!("intake-cleanup-{case}"));
            let mut human =
                ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
            let error = seal_intake_submission_with_human_and_faults(
                &fixture.profile_path,
                &fixture.store_root,
                &submitted.submission_id,
                &mut human,
                &FixedClock(Timestamp::parse("2026-07-22T01:19:10.000000000Z").unwrap()),
                &FaultPlan::at(FaultPoint::BeforeInboxCleanup),
            )
            .unwrap_err();
            assert_eq!(error.code(), "intake.incomplete", "{case}");
            let custody = fixture
                .store_root
                .join("intake/transactions")
                .join(&submitted.submission_id);
            if remove_candidate {
                fs::remove_file(custody.join("candidate.md")).unwrap();
            }
            if remove_meta {
                fs::remove_file(custody.join("submission.meta.json")).unwrap();
            }

            let recovered = fixture
                .recover_intake_with_runtime(&fixture.runtime())
                .unwrap_or_else(|error| panic!("{case} committed tail must recover: {error:?}"));
            assert_eq!(recovered.mode, RecoveryMode::IntakeOnly, "{case}");
            let mut names = fs::read_dir(&custody)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect::<Vec<_>>();
            names.sort();
            assert_eq!(
                names,
                ["committed.json", "prepared.json", "published.json"],
                "{case}"
            );
            let profile = load_p3_profile(&fixture.profile_path).unwrap();
            assert!(
                inspect_store(&profile, &fixture.store_root).is_ok(),
                "{case}"
            );
        }
    }

    #[test]
    fn intake_committed_cleanup_tail_rejects_a_corrupted_survivor_without_deleting_its_peer() {
        let (fixture, submitted) = task8_submission("intake-cleanup-corrupted-survivor");
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        seal_intake_submission_with_human_and_faults(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-22T01:19:20.000000000Z").unwrap()),
            &FaultPlan::at(FaultPoint::BeforeInboxCleanup),
        )
        .unwrap_err();
        let custody = fixture
            .store_root
            .join("intake/transactions")
            .join(&submitted.submission_id);
        let candidate_before = fs::read(custody.join("candidate.md")).unwrap();
        fs::write(
            custody.join("submission.meta.json"),
            b"corrupted cleanup tail",
        )
        .unwrap();
        let intake_before = task8_authority_commitment(&fixture.store_root.join("intake"));

        let error = fixture
            .recover_intake_with_runtime(&fixture.runtime())
            .unwrap_err();
        assert_eq!(error.code(), "intake.incomplete");
        assert_eq!(
            fs::read(custody.join("candidate.md")).unwrap(),
            candidate_before,
            "validation must finish before deleting any surviving raw input"
        );
        assert_eq!(
            task8_authority_commitment(&fixture.store_root.join("intake")),
            intake_before,
            "corrupted committed cleanup input must not mutate intake authority"
        );
    }

    #[test]
    fn intake_uncommitted_missing_raw_input_still_fails_closed_without_mutation() {
        let (fixture, submitted) = task8_submission("intake-uncommitted-missing-raw");
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        seal_intake_submission_with_human_and_faults(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-22T01:19:30.000000000Z").unwrap()),
            &FaultPlan::at(FaultPoint::AfterPairSync),
        )
        .unwrap_err();
        let custody = fixture
            .store_root
            .join("intake/transactions")
            .join(&submitted.submission_id);
        fs::remove_file(custody.join("candidate.md")).unwrap();
        let intake_before = task8_authority_commitment(&fixture.store_root.join("intake"));

        let error = fixture
            .recover_intake_with_runtime(&fixture.runtime())
            .unwrap_err();
        assert_eq!(error.code(), "intake.incomplete");
        assert_eq!(
            task8_authority_commitment(&fixture.store_root.join("intake")),
            intake_before
        );
        assert!(!custody.join("candidate.md").exists());
        assert!(custody.join("submission.meta.json").is_file());
        assert!(!custody.join("committed.json").exists());
        assert!(!custody.join("published.json").exists());
    }

    #[test]
    fn intake_concurrent_seal_seal_commits_one_event_and_returns_typed_lock() {
        let (fixture, submitted) = task8_submission("intake-concurrent-seal-seal");
        let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let first_gate = Arc::clone(&gate);
        let profile_path = fixture.profile_path.clone();
        let store_root = fixture.store_root.clone();
        let submission_id = submitted.submission_id.clone();
        let first = thread::spawn(move || {
            let mut human = BlockingSealHuman {
                confirmation: format!("SEAL INTAKE {submission_id}\n"),
                gate: first_gate,
            };
            seal_intake_submission_with_human(
                &profile_path,
                &store_root,
                &submission_id,
                &mut human,
                &FixedClock(Timestamp::parse("2026-07-22T01:20:00.000000000Z").unwrap()),
            )
        });
        wait_until_seal_is_prompting(&gate);

        let mut second_human = ScriptedHuman::new(None);
        let second = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut second_human,
            &FixedClock(Timestamp::parse("2026-07-22T01:20:01.000000000Z").unwrap()),
        )
        .unwrap_err();
        assert_eq!(second.code(), "intake.locked");
        assert_eq!(second_human.show_count(), 0);
        assert_eq!(second_human.read_count(), 0);

        release_blocked_seal(&gate);
        let winner = first.join().unwrap().unwrap();
        assert!(!winner.idempotent);
        assert_eq!(
            fs::read_to_string(fixture.store_root.join("intake/events.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert_eq!(task8_pair_count(&fixture), 1);
    }

    #[test]
    fn intake_concurrent_seal_verdict_preserves_lock_order_and_one_new_intake_event() {
        let fixture = ProvisionedP3Fixture::with_staged_reject("intake-concurrent-seal-verdict");
        let submitted =
            task8_submit_existing(&fixture, "seal-verdict", "2026-07-22T01:21:00.000000000Z");
        let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let first_gate = Arc::clone(&gate);
        let profile_path = fixture.profile_path.clone();
        let store_root = fixture.store_root.clone();
        let submission_id = submitted.submission_id.clone();
        let seal = thread::spawn(move || {
            let mut human = BlockingSealHuman {
                confirmation: format!("SEAL INTAKE {submission_id}\n"),
                gate: first_gate,
            };
            seal_intake_submission_with_human(
                &profile_path,
                &store_root,
                &submission_id,
                &mut human,
                &FixedClock(Timestamp::parse("2026-07-22T01:21:01.000000000Z").unwrap()),
            )
        });
        wait_until_seal_is_prompting(&gate);

        let verdict = fixture
            .apply(
                VerdictVerb::Reject,
                "concurrent verdict loses to intake seal",
            )
            .unwrap_err();
        assert_eq!(verdict.code(), "store.locked");

        release_blocked_seal(&gate);
        assert!(seal.join().unwrap().is_ok());
        assert_eq!(
            fs::read_to_string(fixture.store_root.join("intake/events.jsonl"))
                .unwrap()
                .lines()
                .count(),
            2
        );
        assert_eq!(fixture.verdict_count(VerdictVerb::Reject), 0);
    }

    #[test]
    fn intake_concurrent_propose_seal_returns_typed_protected_lock() {
        let (fixture, submitted) = task8_submission("intake-concurrent-propose-seal");
        let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let first_gate = Arc::clone(&gate);
        let profile_path = fixture.profile_path.clone();
        let store_root = fixture.store_root.clone();
        let submission_id = submitted.submission_id.clone();
        let seal = thread::spawn(move || {
            let mut human = BlockingSealHuman {
                confirmation: format!("SEAL INTAKE {submission_id}\n"),
                gate: first_gate,
            };
            seal_intake_submission_with_human(
                &profile_path,
                &store_root,
                &submission_id,
                &mut human,
                &FixedClock(Timestamp::parse("2026-07-22T01:22:01.000000000Z").unwrap()),
            )
        });
        wait_until_seal_is_prompting(&gate);

        let proposal = submit_proposal_with_clock(
            task8_proposal_submission(&fixture, "propose-seal-loser"),
            &FixedClock(Timestamp::parse("2026-07-22T01:22:02.000000000Z").unwrap()),
        )
        .unwrap_err();
        assert_eq!(proposal.code(), "store.locked");

        release_blocked_seal(&gate);
        assert!(seal.join().unwrap().is_ok());
    }

    #[test]
    fn intake_proposal_final_publication_during_unrelated_seal_remains_untrusted_until_future_seal()
    {
        let (fixture, first_submitted) =
            task8_submission("intake-final-publication-untrusted-transport");
        let publication_gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let proposer_gate = Arc::clone(&publication_gate);
        let proposal_request = task8_proposal_submission(&fixture, "late-untrusted-publication");
        let proposer = thread::spawn(move || {
            submit_proposal_with_final_publication_hook(proposal_request, || {
                let (state, changed) = &*proposer_gate;
                let mut state = state.lock().unwrap();
                state.0 = true;
                changed.notify_all();
                while !state.1 {
                    state = changed.wait(state).unwrap();
                }
                Ok(())
            })
        });
        wait_until_seal_is_prompting(&publication_gate);

        let seal_gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let blocked_seal_gate = Arc::clone(&seal_gate);
        let profile_path = fixture.profile_path.clone();
        let store_root = fixture.store_root.clone();
        let submission_id = first_submitted.submission_id.clone();
        let seal = thread::spawn(move || {
            let mut human = BlockingSealHuman {
                confirmation: format!("SEAL INTAKE {submission_id}\n"),
                gate: blocked_seal_gate,
            };
            seal_intake_submission_with_human(
                &profile_path,
                &store_root,
                &submission_id,
                &mut human,
                &FixedClock(Timestamp::parse("2026-07-22T01:22:03.000000000Z").unwrap()),
            )
        });
        wait_until_seal_is_prompting(&seal_gate);

        let intake_before = task8_authority_commitment(&fixture.store_root.join("intake"));
        let canon_before = task8_authority_commitment(&fixture.store_root.join("canon"));
        let ledger_before = task8_authority_commitment(&fixture.store_root.join("ledger"));
        release_blocked_seal(&publication_gate);
        let late_submission = proposer
            .join()
            .unwrap()
            .expect("the proposer may finish only its untrusted inbox publication");
        assert!(fixture.store_root.join("locks/intake.lock").is_dir());
        assert_eq!(
            task8_authority_commitment(&fixture.store_root.join("intake")),
            intake_before,
            "late untrusted transport must not mutate protected intake state"
        );
        assert_eq!(
            task8_authority_commitment(&fixture.store_root.join("canon")),
            canon_before,
            "late untrusted transport must not mutate canon"
        );
        assert_eq!(
            task8_authority_commitment(&fixture.store_root.join("ledger")),
            ledger_before,
            "late untrusted transport must not mutate the authority ledger"
        );
        assert!(fixture
            .store_root
            .join("staging/inbox")
            .join(&late_submission.submission_id)
            .is_dir());
        assert_eq!(task8_pair_count(&fixture), 0);
        assert!(fs::read(fixture.store_root.join("intake/events.jsonl"))
            .unwrap()
            .is_empty());

        release_blocked_seal(&seal_gate);
        let first_outcome = seal.join().unwrap().unwrap();
        assert!(!first_outcome.idempotent);
        assert_eq!(
            fs::read_to_string(fixture.store_root.join("intake/events.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert_eq!(task8_pair_count(&fixture), 1);

        let mut future_human = ScriptedHuman::new(Some(format!(
            "SEAL INTAKE {}\n",
            late_submission.submission_id
        )));
        let future_outcome = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &late_submission.submission_id,
            &mut future_human,
            &FixedClock(Timestamp::parse("2026-07-22T01:22:04.000000000Z").unwrap()),
        )
        .expect("a later human seal must handle the untrusted submission normally");
        assert!(!future_outcome.idempotent);
        assert_eq!(future_human.show_count(), 1);
        assert_eq!(future_human.read_count(), 1);
        assert_eq!(
            fs::read_to_string(fixture.store_root.join("intake/events.jsonl"))
                .unwrap()
                .lines()
                .count(),
            2
        );
        assert_eq!(task8_pair_count(&fixture), 2);
    }

    #[test]
    fn intake_concurrent_recovery_seal_returns_typed_lock_and_retains_unpublished_custody() {
        let (fixture, submitted) = task8_submission("intake-concurrent-recovery-seal");
        let mut crash_human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let faults = FaultPlan::at(FaultPoint::AfterIntakeJournalSync);
        assert_eq!(
            seal_intake_submission_with_human_and_faults(
                &fixture.profile_path,
                &fixture.store_root,
                &submitted.submission_id,
                &mut crash_human,
                &FixedClock(Timestamp::parse("2026-07-22T01:23:00.000000000Z").unwrap()),
                &faults,
            )
            .unwrap_err()
            .code(),
            "intake.incomplete"
        );
        let owner: serde_json::Value = serde_json::from_slice(
            &fs::read(fixture.store_root.join("locks/intake.lock/owner.json")).unwrap(),
        )
        .unwrap();
        let owner_id = owner["owner_id"].as_str().unwrap().to_owned();
        let protected_lease = fs::canonicalize(&fixture.store_root)
            .unwrap()
            .join("locks/verdict.lock");
        let publication = RecoveryLeasePublicationBarrier::after_sync_of(protected_lease);
        let runtime = fixture.runtime_with_ops(publication.clone());
        let profile_path = fixture.profile_path.clone();
        let store_root = fixture.store_root.clone();
        let recovery = thread::spawn(move || {
            let mut human = ScriptedHuman::new(Some(format!("RECOVER {owner_id}\n")));
            recover_store_with_human_for_test(
                RecoveryRequest {
                    profile_path,
                    store_root,
                },
                &runtime,
                &mut human,
                &FixedClock(Timestamp::parse("2026-07-22T01:23:01.000000000Z").unwrap()),
            )
        });
        publication.wait_until_published();

        let mut seal_human = ScriptedHuman::new(None);
        let loser = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut seal_human,
            &FixedClock(Timestamp::parse("2026-07-22T01:23:02.000000000Z").unwrap()),
        )
        .unwrap_err();
        assert_eq!(loser.code(), "intake.locked");
        assert_eq!(seal_human.show_count(), 0);
        assert_eq!(seal_human.read_count(), 0);

        publication.release();
        let recovery_error = recovery.join().unwrap().unwrap_err();
        assert_eq!(recovery_error.code(), "intake.incomplete");
        assert!(!fixture
            .store_root
            .join("staging/inbox")
            .join(&submitted.submission_id)
            .exists());
        assert!(fixture
            .store_root
            .join("intake/transactions")
            .join(&submitted.submission_id)
            .is_dir());
        assert_eq!(task8_pair_count(&fixture), 0);
        assert!(fs::read(fixture.store_root.join("intake/events.jsonl"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn intake_release_has_no_fallible_post_removal_parent_sync() {
        let fixture = ProvisionedP3Fixture::empty("intake-release-parent-sync");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = inspect_store(&profile, &fixture.store_root).unwrap();
        let identity = IntakeLeaseIdentity::new(
            "task8-parent-sync",
            "intake-seal",
            &Timestamp::parse("2026-07-22T01:24:00.000000000Z").unwrap(),
            std::process::id(),
        )
        .unwrap();
        let lease = IntakePublicationLease::acquire(&store.paths, identity).unwrap();
        lease
            .release(&store.paths)
            .expect("remove_dir must be the terminal liveness-only release action");
        assert!(!store.paths.locks.join("intake.lock").exists());
    }

    struct BlockingSealHuman {
        confirmation: String,
        gate: Arc<(Mutex<(bool, bool)>, Condvar)>,
    }

    impl HumanPresence for BlockingSealHuman {
        fn show(&mut self, _bytes: &[u8]) -> Result<(), TransactionError> {
            let (state, changed) = &*self.gate;
            let mut state = state.lock().unwrap();
            state.0 = true;
            changed.notify_all();
            while !state.1 {
                state = changed.wait(state).unwrap();
            }
            Ok(())
        }

        fn read_confirmation(&mut self) -> Result<String, TransactionError> {
            Ok(self.confirmation.clone())
        }
    }

    fn wait_until_seal_is_prompting(gate: &Arc<(Mutex<(bool, bool)>, Condvar)>) {
        let (state, changed) = &**gate;
        let mut state = state.lock().unwrap();
        while !state.0 {
            state = changed.wait(state).unwrap();
        }
    }

    fn release_blocked_seal(gate: &Arc<(Mutex<(bool, bool)>, Condvar)>) {
        let (state, changed) = &**gate;
        let mut state = state.lock().unwrap();
        state.1 = true;
        changed.notify_all();
    }

    fn task8_submission(name: &str) -> (ProvisionedP3Fixture, SubmitProposalResult) {
        let fixture = ProvisionedP3Fixture::empty(name);
        let submitted = task8_submit_existing(&fixture, "crash", "2026-07-22T01:10:00.000000000Z");
        (fixture, submitted)
    }

    fn task8_submit_existing(
        fixture: &ProvisionedP3Fixture,
        marker: &str,
        submitted_at: &str,
    ) -> SubmitProposalResult {
        submit_proposal_with_clock(
            task8_proposal_submission(fixture, marker),
            &FixedClock(Timestamp::parse(submitted_at).unwrap()),
        )
        .unwrap()
    }

    fn task8_proposal_submission(
        fixture: &ProvisionedP3Fixture,
        marker: &str,
    ) -> ProposalSubmission {
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate_path = fixture
            .profile_path
            .parent()
            .unwrap()
            .join(format!("task8-{marker}.md"));
        let candidate = String::from_utf8(
            fs::read(fixture.store_root.join("seed/bundle").join(target_path)).unwrap(),
        )
        .unwrap()
        .replace(target_id.trim_end_matches(".md"), target_id)
            + &format!("\nTask 8 {marker} fixture\n");
        fs::write(&candidate_path, candidate).unwrap();
        ProposalSubmission {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            source_id: format!("src:agent:task8-{marker}"),
            operation: ProposalOperation::Replace,
            intent: ProposalIntent::Accept,
            target_id: target_id.to_owned(),
            target_path: target_path.to_owned(),
            candidate_path,
            confidence: None,
            evaluator_path: None,
        }
    }

    fn task8_pair_count(fixture: &ProvisionedP3Fixture) -> usize {
        fs::read_dir(fixture.store_root.join("intake/proposals"))
            .unwrap()
            .flat_map(|shard| fs::read_dir(shard.unwrap().path()).unwrap())
            .count()
    }

    fn task8_authority_commitment(root: &Path) -> String {
        fn collect(root: &Path, current: &Path, entries: &mut BTreeMap<String, String>) {
            let mut children = fs::read_dir(current)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            children.sort_by_key(|entry| entry.file_name());
            for entry in children {
                let entry_path = entry.path();
                let metadata = fs::symlink_metadata(&entry_path).unwrap();
                let relative = entry_path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if metadata.is_dir() {
                    entries.insert(relative, "directory".to_owned());
                    collect(root, &entry_path, entries);
                } else if metadata.file_type().is_symlink() {
                    entries.insert(
                        relative,
                        format!("symlink:{}", fs::read_link(&entry_path).unwrap().display()),
                    );
                } else {
                    entries.insert(
                        relative,
                        blake3::hash(&fs::read(&entry_path).unwrap())
                            .to_hex()
                            .to_string(),
                    );
                }
            }
        }

        let mut entries = BTreeMap::new();
        collect(root, root, &mut entries);
        let mut bytes = Vec::new();
        for (entry_path, commitment) in entries {
            bytes.extend_from_slice(entry_path.as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(commitment.as_bytes());
            bytes.push(b'\n');
        }
        blake3::hash(&bytes).to_hex().to_string()
    }

    #[test]
    fn p2_fixture_access_paths_share_state_shim_and_release_each_environment_guard() {
        let internal_fixture =
            sovmem::profile::p2_fixture::P2Fixture::new("internal-p2-shim-allocation");
        let internal_guard_id = internal_fixture.git_environment_guard_id();
        let internal = sovmem::profile::p2_fixture::git_environment_snapshot();
        assert!(
            internal.active_guard_ids.contains(&internal_guard_id),
            "internal fixture environment guard is active"
        );
        drop(internal_fixture);

        let released = sovmem::profile::p3_test_support::p2_git_environment_snapshot();
        assert!(
            !released.active_guard_ids.contains(&internal_guard_id),
            "dropping the internal fixture releases its environment guard"
        );
        assert_eq!(released.state_address, internal.state_address);
        assert_eq!(released.shim_directory, internal.shim_directory);

        let preflight_fixture = sovmem::profile::p3_test_support::ProvisionedP3Fixture::empty(
            "preflight-p2-shim-allocation",
        );
        let preflight_guard_id = preflight_fixture.p2_git_environment_guard_id();
        let preflight = sovmem::profile::p3_test_support::p2_git_environment_snapshot();
        assert!(
            preflight.active_guard_ids.contains(&preflight_guard_id),
            "preflight fixture environment guard is active"
        );
        assert_eq!(preflight.state_address, internal.state_address);
        assert_eq!(preflight.shim_directory, internal.shim_directory);
        drop(preflight_fixture);

        let released_again = sovmem::profile::p2_fixture::git_environment_snapshot();
        assert!(
            !released_again
                .active_guard_ids
                .contains(&preflight_guard_id),
            "dropping the preflight fixture releases its environment guard"
        );
        assert_eq!(released_again.state_address, internal.state_address);
        assert_eq!(released_again.shim_directory, internal.shim_directory);
    }
}

pub mod full_loop_tests {
    use sovmem::profile::p3_test_support::{FixedClock, ProvisionedP3Fixture, ScriptedHuman};
    use sovmem::profile::transaction::{
        apply_human_verdict, preflight_human_verdict_with_human, read_preflight_state,
        seal_intake_submission_with_human, HumanVerdictRequest, PreflightVerdict,
        TransactionRuntime,
    };
    use sovmem::profile::{
        build_index, build_judgment, inspect_store, load_p3_profile, parse_source_document,
        query_index, query_store, read_intake, read_sealed_pair_structural, rebuild_store_index,
        render_document, submit_proposal_with_clock, EvaluatorBlock, ExpansionMode,
        GeneratedDocument, GeneratedMetadata, IndexRequest, ProposalIntent, ProposalOperation,
        ProposalSubmission, QueryRequest, Reason, RewriteBytes, SourceDocument, StoreIndexRequest,
        StoreQueryRequest, Timestamp, VerdictVerb,
    };
    use std::fs;
    use std::path::{Path, PathBuf};

    const ACCEPT_PHRASE: &str = "zztask17acceptphraseq7";
    const ACCEPT_REASON: &str = "fixture applied accept";
    const ACCEPT_ADVICE: &str = "zztask17acceptadviceq7";
    const REWRITE_PHRASE: &str = "zztask17rewritephraseq7";
    const REWRITE_REASON: &str = "fixture superseding accept";
    const REWRITE_ADVICE: &str = "zztask17rewriteadviceq7";
    #[test]
    fn accept_create_reindexes_and_requeries_the_exact_human_reviewed_loop() {
        let fixture = ProvisionedP3Fixture::empty("task-10-full-loop-accept");
        run_complete_loop(&fixture, false);
    }

    #[test]
    fn rewrite_replace_reindexes_and_requeries_the_exact_human_reviewed_loop() {
        let fixture = ProvisionedP3Fixture::empty("task-10-full-loop-rewrite");
        run_complete_loop(&fixture, false);
        run_complete_loop(&fixture, true);
    }

    #[test]
    fn reject_tp_and_fp_leave_canon_and_index_bytes_unchanged() {
        for verb in [VerdictVerb::Reject, VerdictVerb::Tp, VerdictVerb::Fp] {
            let fixture = ProvisionedP3Fixture::empty(&format!(
                "task-10-full-loop-nonapplying-{}",
                verb.as_str().to_ascii_lowercase()
            ));
            run_nonapplying_verdict(&fixture, verb);
        }
    }

    pub(crate) fn run_complete_loop(fixture: &ProvisionedP3Fixture, rewrite: bool) {
        let slot = current_intake_sequence(fixture);
        let (phrase, reason_text, advice, operation, intent, target_path, candidate_bytes) =
            proposal_inputs(fixture, rewrite);
        assert_seed_has_no_hit(fixture, phrase);

        let candidate_path = fixture_input(
            fixture,
            &format!(
                "task-10-full-loop-{}-{slot}-candidate",
                if rewrite { "rewrite" } else { "accept" }
            ),
            &candidate_bytes,
        );
        let parsed = parse_source_document(SourceDocument {
            source_path: target_path.to_owned(),
            bytes: candidate_bytes.clone(),
            source_hash: blake3::hash(&candidate_bytes).to_hex().to_string(),
            created: String::new(),
            updated: String::new(),
        })
        .expect("fixture candidate parses");
        assert!(
            parsed.metadata.contains_key("provenance"),
            "the proposal must carry explicit provenance"
        );

        let evaluator_path = fixture_input(
            fixture,
            &format!(
                "task-10-full-loop-{}-{slot}-advice",
                if rewrite { "rewrite" } else { "accept" }
            ),
            &serde_json::to_vec(&EvaluatorBlock {
                protocol: "adversarial-bug-finding-v1".to_owned(),
                source_id: "src:agent:task10-reviewer".to_owned(),
                evaluated_at: fixture_time(slot, 0).as_str().to_owned(),
                candidate_hash: blake3::hash(&candidate_bytes).to_hex().to_string(),
                recommendation: "looks-sound".to_owned(),
                reasons: vec![advice.to_owned()],
            })
            .expect("render fixture advisory"),
        );
        let target_id = format!("ssa-0/{target_path}");
        let submitted = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:fixture".to_owned(),
                operation,
                intent,
                target_id: target_id.clone(),
                target_path: target_path.to_owned(),
                candidate_path,
                confidence: None,
                evaluator_path: Some(evaluator_path),
            },
            &FixedClock(fixture_time(slot, 1)),
        )
        .expect("stage complete-loop proposal");
        assert!(
            !submitted.submission_id.is_empty(),
            "the untrusted transport must return a submission identity"
        );
        assert!(
            fixture
                .store_root
                .join("staging/inbox")
                .join(&submitted.submission_id)
                .is_dir(),
            "propose must publish only the untrusted v2 inbox pair"
        );
        let proposal_id = seal_submission(fixture, &submitted.submission_id, slot);

        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
        let store = inspect_store(&profile, &fixture.store_root).expect("inspect fixture store");
        let pair = read_sealed_pair_structural(&store.paths, &proposal_id)
            .expect("inspect exact protected proposal");
        assert!(
            pair.candidate_bytes == candidate_bytes,
            "the staged pair must retain the exact candidate bytes"
        );
        assert_eq!(pair.candidate_hash, submitted.candidate_hash);
        assert_eq!(pair.meta.target_id, target_id);
        assert_eq!(pair.meta.target_path, target_path);
        assert!(
            pair.meta
                .evaluator
                .as_ref()
                .expect("advisory is retained for human inspection")
                .reasons
                == vec![advice.to_owned()],
            "the staged advisory must retain the exact human-reviewed reason"
        );

        let reason = Reason::new(reason_text.as_bytes(), store.policy.policy.max_reason_bytes)
            .expect("construct exact human reason");
        let human_rewrite_bytes = rewrite.then(|| {
            let mut bytes = candidate_bytes.clone();
            bytes.extend_from_slice(b"\nSuperseding fixture revision.\n");
            bytes
        });
        let exact_rewrite = human_rewrite_bytes.as_ref().map(|bytes| {
            RewriteBytes::new(bytes.clone(), store.policy.policy.max_rewrite_bytes)
                .expect("construct exact human rewrite")
        });
        if let Some(rewrite) = exact_rewrite.as_ref() {
            assert!(
                rewrite.bytes() != candidate_bytes.as_slice(),
                "the human rewrite must be independent from the staged candidate"
            );
        }
        let request = HumanVerdictRequest {
            proposal_id: proposal_id.clone(),
            verb: if rewrite {
                VerdictVerb::Rewrite
            } else {
                VerdictVerb::Accept
            },
            reason: reason.clone(),
            rewrite: exact_rewrite.clone(),
        };
        let initial =
            read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
                .expect("bind staged candidate, reason, and advisory before human confirmation");
        let runtime = TransactionRuntime::production(&profile, &store)
            .expect("construct private transaction runtime");
        let confirmation = format!("APPLY {} {}\n", request.verb.as_str(), &proposal_id[..12]);
        let mut human = ScriptedHuman::new(Some(confirmation));
        let outcome = match preflight_human_verdict_with_human(
            initial,
            request,
            &runtime,
            &mut human,
            &FixedClock(fixture_time(slot, 3)),
        )
        .expect("human preflight accepts exact fixture inputs")
        {
            PreflightVerdict::Confirmed(decision) => {
                apply_human_verdict(decision, &runtime).expect("apply complete-loop human verdict")
            }
            PreflightVerdict::Idempotent(_) => panic!("isolated full-loop fixture cannot replay"),
        };
        assert_eq!(human.show_count(), 1);
        assert_eq!(human.read_count(), 1);
        assert!(human
            .displayed()
            .windows(candidate_bytes.len())
            .any(|bytes| bytes == candidate_bytes));
        assert!(human
            .displayed()
            .windows(reason_text.len())
            .any(|bytes| bytes == reason_text.as_bytes()));
        assert!(human
            .displayed()
            .windows(advice.len())
            .any(|bytes| bytes == advice.as_bytes()));

        let generation_id = outcome
            .generation
            .clone()
            .expect("applying verdict has generation");
        let level4 = sovmem::profile::verify_store_level4(&profile, &fixture.store_root)
            .expect("Level 4 verifies the applied full loop");
        assert_eq!(level4.current_generation, generation_id);
        let generation_root = fixture
            .store_root
            .join("canon/generations")
            .join(&generation_id);
        let expected_target = human_rewrite_bytes.as_deref().unwrap_or(&candidate_bytes);
        let applied_target = fs::read(generation_root.join("bundle").join(target_path))
            .expect("read applied target");
        assert!(
            applied_target == expected_target,
            "ACCEPT applies the candidate and REWRITE applies the selected human rewrite"
        );
        if rewrite {
            assert!(
                applied_target != candidate_bytes,
                "REWRITE must not apply the staged candidate"
            );
        }
        let judgment = build_judgment(
            &profile,
            &outcome.verdict_id,
            if rewrite {
                VerdictVerb::Rewrite
            } else {
                VerdictVerb::Accept
            },
            &target_id,
            &reason,
            &fixture_time(slot, 3),
        )
        .expect("build deterministic Judgment")
        .expect("applying verdict creates Judgment");
        assert!(
            fs::read(generation_root.join("bundle").join(&judgment.path))
                .expect("read deterministic Judgment")
                == judgment.bytes,
            "the stored Judgment must equal its deterministic exact bytes"
        );
        if let Some(rewrite) = exact_rewrite.as_ref() {
            let ledger =
                sovmem::profile::read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
                    .expect("read applied REWRITE ledger event");
            let verdict = ledger
                .events
                .iter()
                .find_map(|event| match event {
                    sovmem::profile::LedgerEvent::Verdict(event)
                        if event.payload().proposal_id == proposal_id =>
                    {
                        Some(event.payload())
                    }
                    _ => None,
                })
                .expect("locate applied REWRITE ledger event");
            assert_eq!(verdict.rewrite_hash.as_deref(), Some(rewrite.hash()));
            assert_eq!(verdict.rewrite_bytes, Some(rewrite.bytes().len() as u64));
            assert_eq!(
                verdict.canonical_result.target_new_hash.as_deref(),
                Some(blake3::hash(expected_target).to_hex().as_str())
            );
        }
        assert_queryable(fixture, phrase, &target_id, reason_text, &judgment.id);

        let current_index = fixture
            .store_root
            .join("derived/indexes")
            .join(&generation_id);
        assert!(current_index.is_dir(), "the current P3 index is disposable");
        fs::remove_dir_all(&current_index).expect("delete only the current disposable P3 index");
        assert!(!current_index.exists());
        let rebuilt = rebuild_store_index(StoreIndexRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        })
        .expect("full rebuild from the ledger-selected generation");
        assert_eq!(rebuilt.generation, generation_id);
        assert_queryable(fixture, phrase, &target_id, reason_text, &judgment.id);
    }

    pub(crate) fn run_nonapplying_verdict(fixture: &ProvisionedP3Fixture, verb: VerdictVerb) {
        assert!(matches!(
            verb,
            VerdictVerb::Reject | VerdictVerb::Tp | VerdictVerb::Fp
        ));
        let slot = current_intake_sequence(fixture);
        let target_path = "04-Research/CLI/README.md";
        let target_id = format!("ssa-0/{target_path}");
        let mut candidate_bytes = create_candidate_bytes(fixture, target_path, &target_id);
        candidate_bytes.extend_from_slice(
            format!("\nTask 10 nonapplying {} marker {slot}.\n", verb.as_str()).as_bytes(),
        );
        let candidate_path = fixture_input(
            fixture,
            &format!("task-10-nonapplying-{}-{slot}", verb.as_str()),
            &candidate_bytes,
        );
        let submitted = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:fixture".to_owned(),
                operation: ProposalOperation::Create,
                intent: ProposalIntent::Accept,
                target_id,
                target_path: target_path.to_owned(),
                candidate_path,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(fixture_time(slot, 1)),
        )
        .expect("submit nonapplying v2 proposal");
        let proposal_id = seal_submission(fixture, &submitted.submission_id, slot);
        let canon_before = fixture.canon_hashes();
        let index_before = fixture.index_hashes();
        let profile = load_p3_profile(&fixture.profile_path).expect("load nonapplying profile");
        let store =
            inspect_store(&profile, &fixture.store_root).expect("inspect nonapplying store");
        let reason_text = if verb == VerdictVerb::Reject {
            "fixture retained reject".to_owned()
        } else {
            format!("task 10 {} human reason", verb.as_str())
        };
        let request = HumanVerdictRequest {
            proposal_id: proposal_id.clone(),
            verb,
            reason: Reason::new(reason_text.as_bytes(), store.policy.policy.max_reason_bytes)
                .expect("construct nonapplying reason"),
            rewrite: None,
        };
        let initial =
            read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
                .expect("bind protected nonapplying proposal");
        let runtime = TransactionRuntime::production(&profile, &store)
            .expect("construct nonapplying transaction runtime");
        let mut human = ScriptedHuman::new(Some(format!(
            "APPLY {} {}\n",
            verb.as_str(),
            &proposal_id[..12]
        )));
        let outcome = match preflight_human_verdict_with_human(
            initial,
            request,
            &runtime,
            &mut human,
            &FixedClock(fixture_time(slot, 3)),
        )
        .expect("confirm nonapplying human verdict")
        {
            PreflightVerdict::Confirmed(decision) => {
                apply_human_verdict(decision, &runtime).expect("apply nonapplying human verdict")
            }
            PreflightVerdict::Idempotent(_) => panic!("isolated nonapplying fixture cannot replay"),
        };
        assert!(!outcome.applied);
        assert!(outcome.generation.is_none());
        assert_eq!(fixture.canon_hashes(), canon_before);
        assert_eq!(fixture.index_hashes(), index_before);
        let summary = sovmem::profile::verify_store_level4(&profile, &fixture.store_root)
            .expect("Level 4 verifies nonapplying v2 verdict");
        assert_eq!(summary.level, 4);
        let ledger =
            sovmem::profile::read_ledger(&store.paths.ledger).expect("read nonapplying v2 ledger");
        let verdict = ledger
            .events
            .iter()
            .find_map(|event| match event {
                sovmem::profile::LedgerEvent::Verdict(event)
                    if event.payload().proposal_id == proposal_id =>
                {
                    Some(event.payload())
                }
                _ => None,
            })
            .expect("nonapplying verdict is committed");
        assert_eq!(verdict.verb, verb);
        assert_eq!(
            verdict.canonical_result,
            sovmem::profile::CanonicalResult::none()
        );
    }

    fn proposal_inputs(
        fixture: &ProvisionedP3Fixture,
        rewrite: bool,
    ) -> (
        &'static str,
        &'static str,
        &'static str,
        ProposalOperation,
        ProposalIntent,
        &'static str,
        Vec<u8>,
    ) {
        if !rewrite {
            let target_path = "04-Research/CLI/README.md";
            let target_id = format!("ssa-0/{target_path}");
            let mut candidate = create_candidate_bytes(fixture, target_path, &target_id);
            candidate.extend_from_slice(format!("\n{ACCEPT_PHRASE}\n").as_bytes());
            return (
                ACCEPT_PHRASE,
                ACCEPT_REASON,
                ACCEPT_ADVICE,
                ProposalOperation::Create,
                ProposalIntent::Accept,
                "04-Research/CLI/README.md",
                candidate,
            );
        }
        let target_path = "04-Research/CLI/README.md";
        let current = fs::read_to_string(fixture.store_root.join("canon/CURRENT"))
            .expect("read selected canonical generation");
        let current = current.trim();
        assert_ne!(current, "seed", "REWRITE fixture requires the prior ACCEPT");
        let mut candidate = String::from_utf8(
            fs::read(
                fixture
                    .store_root
                    .join("canon/generations")
                    .join(current)
                    .join("bundle")
                    .join(target_path),
            )
            .expect("read accepted rewrite target"),
        )
        .expect("accepted rewrite target is UTF-8")
        .into_bytes();
        candidate.extend_from_slice(format!("\n{REWRITE_PHRASE}\n").as_bytes());
        (
            REWRITE_PHRASE,
            REWRITE_REASON,
            REWRITE_ADVICE,
            ProposalOperation::Replace,
            ProposalIntent::Rewrite,
            target_path,
            candidate,
        )
    }

    fn assert_seed_has_no_hit(fixture: &ProvisionedP3Fixture, phrase: &str) {
        let index_root = fixture
            .profile_path
            .parent()
            .expect("fixture profile has parent")
            .join(format!("seed-index-{phrase}"));
        build_index(IndexRequest {
            profile_path: repository_path("profiles/audy-ssa0-v0/profile.yaml"),
            bundle_root: fixture.store_root.join("seed"),
            index_root: index_root.clone(),
        })
        .expect("build disposable seed index");
        let result = query_index(QueryRequest {
            profile_path: repository_path("profiles/audy-ssa0-v0/profile.yaml"),
            bundle_root: fixture.store_root.join("seed"),
            index_root,
            query: phrase.to_owned(),
            top_k: 5,
            expansion: ExpansionMode::None,
        })
        .expect("query seed for unique phrase");
        assert!(result.results.is_empty(), "unique phrase has no seed hit");
    }

    fn assert_queryable(
        fixture: &ProvisionedP3Fixture,
        phrase: &str,
        target_id: &str,
        reason: &str,
        judgment_id: &str,
    ) {
        let query = |value: &str| {
            query_store(StoreQueryRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                query: value.to_owned(),
                top_k: 5,
                expansion: ExpansionMode::None,
            })
            .expect("query ledger-selected rebuilt P3 index")
            .result
        };
        assert!(
            query(phrase)
                .results
                .iter()
                .position(|hit| hit.id == target_id)
                .is_some_and(|position| position < 5),
            "the applied target is queryable"
        );
        assert!(
            query(reason)
                .results
                .iter()
                .any(|hit| hit.id == judgment_id && hit.core_type == "judgment"),
            "the exact deterministic Judgment and its reason are queryable"
        );
    }

    fn seal_submission(fixture: &ProvisionedP3Fixture, submission_id: &str, slot: u64) -> String {
        let mut human = ScriptedHuman::new(Some(format!("SEAL INTAKE {submission_id}\n")));
        let sealed = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            submission_id,
            &mut human,
            &FixedClock(fixture_time(slot, 2)),
        )
        .expect("seal complete-loop inbox submission");
        assert_eq!(human.show_count(), 1);
        assert_eq!(human.read_count(), 1);
        assert_eq!(sealed.state, "staged");
        assert_eq!(sealed.intake_sequence, slot + 1);
        sealed
            .proposal_id
            .expect("policy-passing seal publishes protected proposal")
    }

    fn current_intake_sequence(fixture: &ProvisionedP3Fixture) -> u64 {
        let profile = load_p3_profile(&fixture.profile_path).expect("load v2 fixture profile");
        let store = inspect_store(&profile, &fixture.store_root).expect("inspect v2 fixture store");
        read_intake(&store.paths)
            .expect("read protected intake")
            .head()
            .sequence
    }

    fn fixture_input(fixture: &ProvisionedP3Fixture, name: &str, bytes: &[u8]) -> PathBuf {
        let path = fixture
            .profile_path
            .parent()
            .expect("fixture profile has parent")
            .join(format!("{name}.md"));
        fs::write(&path, bytes).expect("write Task 10 fixture input");
        path
    }

    fn create_candidate_bytes(
        fixture: &ProvisionedP3Fixture,
        target_path: &str,
        target_id: &str,
    ) -> Vec<u8> {
        let seed_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let bytes = fs::read(fixture.store_root.join("seed/bundle").join(seed_path))
            .expect("read seeded candidate template");
        let parsed = parse_source_document(SourceDocument {
            source_path: seed_path.to_owned(),
            source_hash: blake3::hash(&bytes).to_hex().to_string(),
            bytes,
            created: String::new(),
            updated: String::new(),
        })
        .expect("parse seeded candidate template");
        let mut metadata: GeneratedMetadata = serde_yaml::from_value(
            serde_yaml::to_value(parsed.metadata).expect("encode seeded candidate metadata"),
        )
        .expect("decode seeded candidate metadata");
        metadata.id = target_id.to_owned();
        metadata.source_path = target_path.to_owned();
        metadata.source = format!("vault://{target_id}");
        metadata.resource = format!("vault://{target_id}");
        metadata.title = "P3 v2 generated canonical fixture".to_owned();
        metadata.description = "Deterministic P3 v2 create fixture.".to_owned();
        if let Some(mapping) = load_p3_profile(&fixture.profile_path)
            .expect("load P3 v2 profile for fixture candidate")
            .config
            .mapping_for(target_path)
        {
            metadata.core_type = mapping.core_type.clone();
            metadata.context_kind = mapping.context_kind.clone();
            metadata.layer = mapping.layer.clone();
            metadata.authority = mapping.authority.clone();
        }
        render_document(&GeneratedDocument {
            metadata,
            body: format!("{}\nP3 v2 fixture create exact bytes.\n", parsed.body),
            source_hash: String::new(),
        })
        .expect("render P3 v2 create candidate")
    }

    fn fixture_time(slot: u64, phase: u64) -> Timestamp {
        let minute = 20_u64
            .checked_add(slot)
            .expect("fixture minute remains bounded");
        Timestamp::parse(&format!("2026-07-22T04:{minute:02}:{phase:02}.000000000Z"))
            .expect("parse fixed Task 10 timestamp")
    }

    fn repository_path(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(relative)
    }
}

pub mod index_tests {

    #[test]
    fn full_p3_rebuild_binds_generation_and_is_queryable() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("p3-index");
        let generation = fixture.stage_accept_create();
        let built = fixture.build_p3_index(&generation);

        assert_eq!(built.input_kind, "p3-canon");
        assert_eq!(built.store_id.as_deref(), Some("audy-sovmem-writeback-v2"));
        assert_eq!(
            built.generation_id.as_deref(),
            Some(generation.manifest.generation_id.as_str())
        );
        assert_eq!(
            built.manifest_hash.as_deref(),
            Some(generation.manifest_hash.as_str())
        );
        assert_eq!(built.receipt.schema_version, "sovmem-reindex-receipt-v1");
        assert!(fixture.verify_p3_receipt(&generation, &built));
        assert!(fixture.rejects_tampered_p3_receipt(&generation, &built));
        assert!(fixture.rejects_p3_index_inside_generation(&generation));
        assert!(fixture
            .query_p3(&generation, "P3 fixture create exact bytes")
            .results
            .iter()
            .any(|hit| hit.id == generation.applied_target_id().expect("applied target")));
        let inherited = generation
            .generated_documents
            .iter()
            .find(|document| {
                document.metadata.source_path
                    == "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"
            })
            .expect("inherited seed document");
        assert_eq!(
            inherited.metadata.id,
            "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"
        );
        assert!(fixture
            .query_p3(&generation, &inherited.metadata.title)
            .results
            .iter()
            .any(|hit| {
                hit.id == "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"
            }));

        fixture.delete_p3_index(&generation);
        let rebuilt = fixture.build_p3_index(&generation);
        assert_eq!(rebuilt.receipt_bytes, built.receipt_bytes);
        assert_eq!(rebuilt.documents_hash, built.documents_hash);
        assert_eq!(rebuilt.graph_hash, built.graph_hash);
    }
}

pub mod ledger_tests {

    use sovmem::profile::{
        append_intake_event, archive_policy, evaluator_commitment, intake_prefix, load_p3_profile,
        proposal_list, read_intake, read_ledger, read_sealed_pair_structural,
        reindex_receipt_bytes, verify_store_level4, CanonicalResult, GenesisEvent, GenesisPayload,
        IntakeEvent, IntakeHead, IntakeLeaseIdentity, IntakeOutcome, IntakePayload,
        IntakePublicationLease, LedgerEvent, ProposalFilter, ProposalIntent, ProposalOperation,
        ProposalState, ProposalSubmission, Reason, ReindexReceipt, RewriteBytes, Timestamp,
        VerdictEvent, VerdictIdentityInput, VerdictPayload, VerdictVerb,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;

    use sovmem::profile::p3_test_support::{FixedClock, ProvisionedP3Fixture, ScriptedHuman};
    use sovmem::profile::transaction::{
        apply_human_verdict, install_verdict_late_intake_check_hook_for_test,
        install_verdict_pre_append_hook_for_test, preflight_human_verdict_with_human,
        read_preflight_state, seal_intake_submission_with_human, HumanPresence, PreflightVerdict,
        TransactionError, TransactionRuntime, VerdictLeaseReceipt,
    };

    fn captured_reindex_receipts(fixture: &ProvisionedP3Fixture) -> BTreeMap<String, Vec<u8>> {
        fs::read_dir(fixture.store_root.join("evidence/reindex"))
            .expect("read fixture reindex evidence")
            .map(|entry| {
                let path = entry.expect("read reindex evidence entry").path();
                (
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .expect("UTF-8 receipt name")
                        .to_owned(),
                    fs::read(path).expect("read reindex evidence bytes"),
                )
            })
            .collect()
    }

    fn intake_binding_fixture(name: &str) -> ProvisionedP3Fixture {
        intake_binding_fixture_with_prior_rejection(name, false)
    }

    fn intake_binding_fixture_with_prior_rejection(
        name: &str,
        include_prior_rejection: bool,
    ) -> ProvisionedP3Fixture {
        let mut fixture = ProvisionedP3Fixture::empty(name);
        if include_prior_rejection {
            append_visible_intake_rejection_at(&fixture, "2026-07-21T06:59:59.000000000Z");
        }
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate_path = fixture
            .profile_path
            .parent()
            .expect("fixture profile parent")
            .join("intake-binding-candidate.md");
        let candidate = String::from_utf8(
            fs::read(fixture.store_root.join("seed/bundle").join(target_path))
                .expect("read fixture candidate"),
        )
        .expect("fixture candidate is UTF-8")
        .replace(
            "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog",
            "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
        );
        fs::write(&candidate_path, candidate).expect("write fixture candidate");
        let submitted = sovmem::profile::submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:intake-binding".to_owned(),
                operation: ProposalOperation::Replace,
                intent: ProposalIntent::Accept,
                target_id: "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"
                    .to_owned(),
                target_path: target_path.to_owned(),
                candidate_path,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-21T07:00:00.000000000Z")
                    .expect("fixed submit time"),
            ),
        )
        .expect("submit untrusted fixture candidate");
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let sealed = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-21T07:00:01.000000000Z")
                    .expect("fixed seal time"),
            ),
        )
        .expect("seal fixture candidate");
        fixture.proposal_id = sealed.proposal_id.expect("policy-passing fixture proposal");
        fixture
    }

    fn v2_event_hash(payload: &[u8]) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sovmem.ledger-event.v2");
        hasher.update(&(payload.len() as u64).to_le_bytes());
        hasher.update(payload);
        hasher.finalize().to_hex().to_string()
    }

    fn reframe_as_v2(line: &[u8], intake_head: Option<(u64, Option<&str>)>) -> Vec<u8> {
        let source = std::str::from_utf8(line).expect("canonical ledger line is UTF-8");
        let event_hash = source
            .rfind(",\"event_hash\":")
            .expect("canonical ledger line contains an event hash");
        let payload_end = if intake_head.is_some() {
            source[..event_hash]
                .rfind(",\"intake_sequence\":")
                .unwrap_or(event_hash)
        } else {
            event_hash
        };
        let schema = format!(
            "\"schema_version\":\"{}\"",
            sovmem::profile::LEDGER_EVENT_SCHEMA
        );
        let mut payload_body = source[..payload_end].replacen(
            &schema,
            "\"schema_version\":\"sovmem-ledger-event-v2\"",
            1,
        );
        if let Some((sequence, hash)) = intake_head {
            payload_body.push_str(&format!(
                ",\"intake_sequence\":{sequence},\"intake_head_hash\":"
            ));
            match hash {
                Some(hash) => payload_body.push_str(&format!("\"{hash}\"")),
                None => payload_body.push_str("null"),
            }
        }
        let payload = format!("{payload_body}}}");
        let hash = v2_event_hash(payload.as_bytes());
        format!("{payload_body},\"event_hash\":\"{hash}\"}}\n").into_bytes()
    }

    fn write_v2_reject_verdict(
        fixture: &ProvisionedP3Fixture,
        intake_sequence: u64,
        intake_head_hash: Option<&str>,
    ) {
        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
        let store = sovmem::profile::inspect_store(&profile, &fixture.store_root)
            .expect("inspect unsealed fixture store");
        let pair = read_sealed_pair_structural(&store.paths, &fixture.proposal_id)
            .expect("read fixture sealed pair");
        let actual_intake = read_intake(&store.paths).expect("read fixture intake");
        let actual_intake_sequence = actual_intake.head().sequence;
        let actual_intake_head_hash = actual_intake
            .head()
            .event_hash
            .clone()
            .expect("sealed fixture has a nonempty intake head");
        archive_policy(&store.paths.policies, &store.policy).expect("archive fixture policy");
        let ruled_at =
            Timestamp::parse("2026-07-21T07:00:02.000000000Z").expect("fixed verdict time");
        let genesis = GenesisEvent::new(GenesisPayload {
            schema_version: sovmem::profile::LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "genesis".to_owned(),
            sequence: 0,
            event_id: "evt-0000000000000000".to_owned(),
            created_at: ruled_at.as_str().to_owned(),
            previous_event_hash: None,
            store_id: store.policy.policy.store_id.clone(),
            profile_version: profile.version.clone(),
            profile_hash: profile.hash.clone(),
            policy_hash: store.policy.hash.clone(),
            seed_manifest_hash: store.seed.manifest_hash.clone(),
            seed_corpus_report_hash: store.seed.corpus_report_hash.clone(),
            seed_bundle_hash: store.seed.bundle_hash.clone(),
            source_commit: store.seed.source_commit.clone(),
            initial_canonical_ref: "seed".to_owned(),
        })
        .expect("build fixture genesis");
        let reason = Reason::new(b"intake binding fixture reason", 4096).expect("fixture reason");
        let identity = VerdictIdentityInput {
            proposal_id: &pair.meta.proposal_id,
            verb: VerdictVerb::Reject,
            reason: &reason,
            rewrite: None,
            actor: &store.policy.policy.human_actor,
            ruled_at: &ruled_at,
            prior_ledger_head: Some(genesis.hash()),
            intake_sequence: actual_intake_sequence,
            intake_head_hash: Some(&actual_intake_head_hash),
            policy_hash: &store.policy.hash,
            profile_hash: &profile.hash,
        };
        let verdict = VerdictEvent::new(VerdictPayload {
            schema_version: sovmem::profile::LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "verdict".to_owned(),
            sequence: 1,
            event_id: "evt-0000000000000001".to_owned(),
            occurred_at: ruled_at.as_str().to_owned(),
            previous_event_hash: Some(genesis.hash().to_owned()),
            store_id: store.policy.policy.store_id.clone(),
            transaction_id: format!("txn-{}", "a".repeat(64)),
            proposal_id: pair.meta.proposal_id.clone(),
            proposal_meta_hash: pair.meta_hash.clone(),
            candidate_hash: pair.candidate_hash.clone(),
            source_id: pair.meta.source_id.clone(),
            verb: VerdictVerb::Reject,
            reason: reason.as_str().to_owned(),
            verdict_id: sovmem::profile::verdict_id(&identity),
            actor: store.policy.policy.human_actor.clone(),
            policy_hash: store.policy.hash.clone(),
            profile_hash: profile.hash.clone(),
            confidence: pair.meta.confidence,
            evaluator_meta_hash: pair
                .meta
                .evaluator
                .as_ref()
                .map(evaluator_commitment)
                .transpose()
                .expect("fixture evaluator commitment"),
            base_kind: pair.meta.base.kind.clone(),
            base_generation: pair.meta.base.generation_id.clone(),
            base_manifest_hash: pair.meta.base.manifest_hash.clone(),
            base_bundle_hash: pair.meta.base.bundle_hash.clone(),
            base_target_hash: pair.meta.base.target_hash.clone(),
            rewrite_hash: None,
            rewrite_bytes: None,
            canonical_result: CanonicalResult::none(),
            resulting_canonical_head: store.head.as_str().to_owned(),
            intake_sequence: actual_intake_sequence,
            intake_head_hash: Some(actual_intake_head_hash),
        })
        .expect("build fixture reject verdict");
        let mut bytes = reframe_as_v2(
            &LedgerEvent::Genesis(Box::new(genesis))
                .canonical_line_bytes()
                .expect("render fixture genesis"),
            None,
        );
        bytes.extend_from_slice(&reframe_as_v2(
            &LedgerEvent::Verdict(Box::new(verdict))
                .canonical_line_bytes()
                .expect("render fixture verdict"),
            Some((intake_sequence, intake_head_hash)),
        ));
        fs::write(&store.paths.ledger, bytes).expect("write fixture v2 ledger");
    }

    fn append_visible_intake_rejection(fixture: &ProvisionedP3Fixture) {
        append_visible_intake_rejection_at(fixture, "2026-07-21T07:00:02.000000000Z");
    }

    fn append_visible_intake_rejection_at(fixture: &ProvisionedP3Fixture, occurred_at: &str) {
        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
        let store = sovmem::profile::inspect_store(&profile, &fixture.store_root)
            .expect("inspect unsealed fixture store");
        let timestamp = Timestamp::parse(occurred_at).expect("fixed rejection time");
        let identity = IntakeLeaseIdentity::new("task5-visible-rejection", "test", &timestamp, 7)
            .expect("construct intake test lease identity");
        let mut lease = IntakePublicationLease::acquire(&store.paths, identity)
            .expect("acquire protected intake lease");
        let view = read_intake(&store.paths).expect("read fixture intake");
        let sequence = view
            .head()
            .sequence
            .checked_add(1)
            .expect("sequence remains bounded");
        let event = IntakeEvent::new(IntakePayload {
            schema_version: sovmem::profile::INTAKE_EVENT_SCHEMA.to_owned(),
            event_type: "intake".to_owned(),
            sequence,
            event_id: format!("int-{sequence:016}"),
            occurred_at: timestamp.as_str().to_owned(),
            previous_event_hash: view.head().event_hash.clone(),
            outcome: IntakeOutcome::REJECTED,
            source_id: None,
            proposal_id: None,
            proposal_meta_hash: None,
            candidate_hash: None,
            policy_hash: store.policy.hash.clone(),
            profile_hash: profile.hash.clone(),
            rule_codes: vec!["policy.fixture_rejected".to_owned()],
        })
        .expect("build visible fixture rejection");
        append_intake_event(&store.paths, &mut lease, &event).expect("append fixture rejection");
        lease
            .release(&store.paths)
            .expect("release fixture intake lease");
    }

    fn append_untrusted_intake_rejection(profile_path: PathBuf, store_root: PathBuf) {
        let profile = load_p3_profile(&profile_path).expect("load fixture profile");
        let paths = sovmem::profile::StorePaths::from_root(
            fs::canonicalize(store_root).expect("canonicalize fixture store"),
        );
        let policy = sovmem::profile::load_policy(&paths.policy).expect("load fixture policy");
        let view = read_intake(&paths).expect("read fixture intake");
        let sequence = view
            .head()
            .sequence
            .checked_add(1)
            .expect("sequence remains bounded");
        let timestamp =
            Timestamp::parse("2026-07-21T07:00:03.000000000Z").expect("fixed forged intake time");
        let event = IntakeEvent::new(IntakePayload {
            schema_version: sovmem::profile::INTAKE_EVENT_SCHEMA.to_owned(),
            event_type: "intake".to_owned(),
            sequence,
            event_id: format!("int-{sequence:016}"),
            occurred_at: timestamp.as_str().to_owned(),
            previous_event_hash: view.head().event_hash.clone(),
            outcome: IntakeOutcome::REJECTED,
            source_id: None,
            proposal_id: None,
            proposal_meta_hash: None,
            candidate_hash: None,
            policy_hash: policy.hash,
            profile_hash: profile.hash.clone(),
            rule_codes: vec!["policy.fixture_rejected".to_owned()],
        })
        .expect("build forged intake event");
        let mut events = fs::OpenOptions::new()
            .append(true)
            .open(&paths.intake_events)
            .expect("open protected intake events for adversarial fixture");
        events
            .write_all(&event.canonical_line_bytes())
            .and_then(|_| events.sync_all())
            .expect("append forged intake event");
    }

    struct IntakeDriftHuman {
        confirmation: String,
        mutate: Option<Box<dyn FnOnce()>>,
        confirmations: usize,
    }

    impl IntakeDriftHuman {
        fn new(confirmation: String, mutate: impl FnOnce() + 'static) -> Self {
            Self {
                confirmation,
                mutate: Some(Box::new(mutate)),
                confirmations: 0,
            }
        }
    }

    impl HumanPresence for IntakeDriftHuman {
        fn show(&mut self, _bytes: &[u8]) -> Result<(), TransactionError> {
            Ok(())
        }

        fn read_confirmation(&mut self) -> Result<String, TransactionError> {
            self.confirmations += 1;
            self.mutate
                .take()
                .expect("fixture mutates intake exactly once")();
            Ok(self.confirmation.clone())
        }
    }

    fn fixture_intake_head(fixture: &ProvisionedP3Fixture) -> (u64, String) {
        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
        let store = sovmem::profile::inspect_store(&profile, &fixture.store_root)
            .expect("inspect fixture store");
        let intake = read_intake(&store.paths).expect("read fixture intake");
        (
            intake.head().sequence,
            intake
                .head()
                .event_hash
                .clone()
                .expect("sealed fixture has a nonempty intake head"),
        )
    }

    fn assert_level4_intake_binding_failure(fixture: &ProvisionedP3Fixture) {
        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
        let before = fixture.durable_hashes();
        assert_eq!(
            verify_store_level4(&profile, &fixture.store_root)
                .expect_err("forged intake high-water must fail Level 4")
                .code(),
            "ledger.intake_binding"
        );
        assert_eq!(fixture.durable_hashes(), before);
    }

    #[test]
    fn intake_binding_valid_v2_ledger_replays_the_exact_observed_high_water() {
        let fixture = intake_binding_fixture("ledger-intake-binding-v2-valid");
        let (sequence, hash) = fixture_intake_head(&fixture);
        write_v2_reject_verdict(&fixture, sequence, Some(&hash));
        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");

        verify_store_level4(&profile, &fixture.store_root)
            .expect("valid v2 intake-bound ledger must verify");
    }

    #[test]
    fn intake_binding_rejects_a_wrong_sequence_without_mutating_durable_state() {
        let fixture = intake_binding_fixture("ledger-intake-binding-wrong-sequence");
        let (sequence, hash) = fixture_intake_head(&fixture);
        write_v2_reject_verdict(&fixture, sequence + 1, Some(&hash));

        assert_level4_intake_binding_failure(&fixture);
    }

    #[test]
    fn intake_binding_rejects_a_wrong_hash_without_mutating_durable_state() {
        let fixture = intake_binding_fixture("ledger-intake-binding-wrong-hash");
        let (sequence, _) = fixture_intake_head(&fixture);
        let forged = "a".repeat(64);
        write_v2_reject_verdict(&fixture, sequence, Some(&forged));

        assert_level4_intake_binding_failure(&fixture);
    }

    #[test]
    fn intake_binding_rejects_zero_sequence_with_a_hash_without_mutating_durable_state() {
        let fixture = intake_binding_fixture("ledger-intake-binding-zero-with-hash");
        let (_, hash) = fixture_intake_head(&fixture);
        write_v2_reject_verdict(&fixture, 0, Some(&hash));

        assert_level4_intake_binding_failure(&fixture);
    }

    #[test]
    fn intake_binding_rejects_nonzero_sequence_without_a_hash_without_mutating_durable_state() {
        let fixture = intake_binding_fixture("ledger-intake-binding-nonzero-without-hash");
        let (sequence, _) = fixture_intake_head(&fixture);
        write_v2_reject_verdict(&fixture, sequence, None);

        assert_level4_intake_binding_failure(&fixture);
    }

    #[test]
    fn intake_binding_rejects_a_head_that_omits_a_visible_rejection() {
        let fixture = intake_binding_fixture_with_prior_rejection(
            "ledger-intake-binding-omitted-rejection",
            true,
        );
        let subject_head = fixture_intake_head(&fixture);
        assert!(
            subject_head.0 > 1,
            "fixture must stage the subject after the visible rejection"
        );
        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
        let store = sovmem::profile::inspect_store(&profile, &fixture.store_root)
            .expect("inspect fixture store");
        let intake = read_intake(&store.paths).expect("read fixture intake");
        let prior = intake.events().first().expect("fixture rejection is first");
        assert_eq!(prior.payload().outcome, IntakeOutcome::REJECTED);
        write_v2_reject_verdict(&fixture, prior.payload().sequence, Some(prior.hash()));

        assert_level4_intake_binding_failure(&fixture);
    }

    #[test]
    fn intake_binding_allows_a_later_intake_event_after_the_observed_high_water() {
        let fixture = intake_binding_fixture("ledger-intake-binding-later-event");
        let subject_head = fixture_intake_head(&fixture);
        append_visible_intake_rejection(&fixture);
        write_v2_reject_verdict(&fixture, subject_head.0, Some(&subject_head.1));
        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");

        verify_store_level4(&profile, &fixture.store_root)
            .expect("later intake records must not invalidate the recorded high-water");
    }

    #[test]
    fn level_four_intake_replays_archived_policy_after_current_meta_limit_rotates_lower() {
        let fixture = intake_binding_fixture("level-four-intake-historical-policy");
        let request = fixture
            .request_for(VerdictVerb::Reject, "historical intake policy fixture")
            .expect("construct historical-policy verdict request");
        fixture.commit_exact_reject(&request);

        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
        let store = sovmem::profile::inspect_store(&profile, &fixture.store_root)
            .expect("inspect committed fixture store");
        let pair = read_sealed_pair_structural(&store.paths, &fixture.proposal_id)
            .expect("read historical sealed pair");
        let rotated = String::from_utf8(store.policy.bytes.clone())
            .expect("fixture policy is UTF-8")
            .replacen(
                "max_meta_bytes = 65536",
                &format!("max_meta_bytes = {}", pair.meta_bytes.len() - 1),
                1,
            )
            .into_bytes();
        fs::write(&store.paths.policy, rotated)
            .expect("rotate current policy below historical meta");
        append_visible_intake_rejection(&fixture);

        let current =
            sovmem::profile::load_policy(&store.paths.policy).expect("load rotated policy");
        assert!(pair.meta_bytes.len() > current.policy.max_meta_bytes);
        verify_store_level4(&profile, &fixture.store_root)
            .expect("Level 4 must replay the verdict's archived policy and exact intake prefix");
    }

    #[test]
    fn intake_binding_valid_v2_verdict_commits_the_observed_high_water() {
        let fixture = intake_binding_fixture("ledger-intake-binding-valid");
        let request = fixture
            .request_for(VerdictVerb::Reject, "intake binding fixture reason")
            .expect("construct fixture verdict request");
        let initial =
            read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
                .expect("read fixture verdict preflight");
        let runtime = fixture.runtime();
        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture profile");
        let store = sovmem::profile::inspect_store(&profile, &fixture.store_root)
            .expect("inspect fixture store");
        let receipt = VerdictLeaseReceipt::new(
            &store.policy.policy.store_id,
            Some(store.head.as_str()),
            &profile.hash,
            &store.policy.hash,
            &request.proposal_id,
        )
        .expect("bind fixture lease receipt");
        let leases = runtime
            .acquire_verdict_leases(&receipt)
            .expect("acquire fixture verdict leases");
        let allowance = sovmem::profile::VerificationAllowance {
            verdict_lease_token: Some(leases.verdict_token().to_owned()),
            intake_lease_token: Some(leases.intake_token().to_owned()),
            recovery_verdict_lease_token: None,
        };
        if let Err(error) =
            sovmem::profile::verify_preflight_transient_state(&store.paths, &allowance)
        {
            let _ = runtime.release_verdict_leases(&leases);
            panic!("held transient verification must succeed: {}", error.code());
        }
        let held = read_preflight_state(
            &fixture.profile_path,
            &fixture.store_root,
            &request,
            Some(&leases),
        );
        runtime
            .release_verdict_leases(&leases)
            .expect("release fixture verdict leases");
        if let Err(error) = held {
            panic!("held preflight must succeed: {}", error.code());
        }
        let mut human = ScriptedHuman::new(Some(format!(
            "APPLY REJECT {}\n",
            &fixture.proposal_id[..12]
        )));
        let decision = match preflight_human_verdict_with_human(
            initial,
            request,
            &runtime,
            &mut human,
            &FixedClock(
                sovmem::profile::Timestamp::parse("2026-07-21T07:00:02.000000000Z")
                    .expect("fixed verdict time"),
            ),
        )
        .expect("confirm fixture verdict")
        {
            PreflightVerdict::Confirmed(decision) => decision,
            PreflightVerdict::Idempotent(_) => panic!("new fixture verdict must not be idempotent"),
        };
        apply_human_verdict(decision, &runtime).expect("commit fixture verdict");
        let ledger = read_ledger(&fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("read fixture ledger");
        let LedgerEvent::Verdict(event) = &ledger.events[1] else {
            panic!("fixture must contain a verdict after genesis")
        };

        let line = String::from_utf8(
            LedgerEvent::Verdict(event.clone())
                .canonical_line_bytes()
                .expect("render fixture verdict line"),
        )
        .expect("fixture line is UTF-8");
        assert_eq!(event.payload().schema_version, "sovmem-ledger-event-v2");
        assert!(line.contains("\"intake_sequence\":1"));
        assert!(line.contains("\"intake_head_hash\":\""));
    }

    #[test]
    fn intake_head_drift_before_policy_archive_blocks_the_verdict_without_ledger_or_canon_mutation()
    {
        let fixture = intake_binding_fixture("ledger-intake-head-drift-pre-archive");
        let request = fixture
            .request_for(VerdictVerb::Reject, "intake drift before archive")
            .expect("construct fixture verdict request");
        let initial =
            read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
                .expect("read fixture verdict preflight");
        let ledger_before = fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("capture ledger bytes");
        let canon_before = fixture.canon_hashes();
        let profile_path = fixture.profile_path.clone();
        let store_root = fixture.store_root.clone();
        let mut human = IntakeDriftHuman::new(
            format!("APPLY REJECT {}\n", &fixture.proposal_id[..12]),
            move || append_untrusted_intake_rejection(profile_path, store_root),
        );

        let error = match preflight_human_verdict_with_human(
            initial,
            request,
            &fixture.runtime(),
            &mut human,
            &FixedClock(
                Timestamp::parse("2026-07-21T07:00:02.000000000Z").expect("fixed verdict time"),
            ),
        ) {
            Err(error) => error,
            Ok(_) => panic!("intake drift before policy archive must fail closed"),
        };

        assert_eq!(error.code(), "intake.head_mismatch");
        assert_eq!(human.confirmations, 1);
        assert_eq!(
            fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).unwrap(),
            ledger_before
        );
        assert_eq!(fixture.canon_hashes(), canon_before);
        assert!(fixture.leases_absent());
    }

    #[test]
    fn intake_head_drift_before_ledger_append_blocks_the_verdict_without_ledger_or_canon_mutation()
    {
        let fixture = intake_binding_fixture("ledger-intake-head-drift-pre-append");
        let request = fixture
            .request_for(VerdictVerb::Reject, "intake drift before append")
            .expect("construct fixture verdict request");
        let initial =
            read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
                .expect("read fixture verdict preflight");
        let runtime = fixture.runtime();
        let mut human = ScriptedHuman::new(Some(format!(
            "APPLY REJECT {}\n",
            &fixture.proposal_id[..12]
        )));
        let decision = match preflight_human_verdict_with_human(
            initial,
            request,
            &runtime,
            &mut human,
            &FixedClock(
                Timestamp::parse("2026-07-21T07:00:02.000000000Z").expect("fixed verdict time"),
            ),
        )
        .expect("confirm fixture verdict")
        {
            PreflightVerdict::Confirmed(decision) => decision,
            PreflightVerdict::Idempotent(_) => panic!("new fixture verdict must not be idempotent"),
        };
        let ledger_before = fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("capture ledger bytes");
        let canon_before = fixture.canon_hashes();
        let profile_path = fixture.profile_path.clone();
        let store_root = fixture.store_root.clone();
        install_verdict_pre_append_hook_for_test(move || {
            append_untrusted_intake_rejection(profile_path, store_root)
        });

        let error = apply_human_verdict(decision, &runtime)
            .expect_err("intake drift before ledger append must fail closed");

        assert_eq!(error.code(), "intake.head_mismatch");
        assert_eq!(
            fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).unwrap(),
            ledger_before
        );
        assert_eq!(fixture.canon_hashes(), canon_before);
        assert_eq!(fixture.verdict_count(VerdictVerb::Reject), 0);
    }

    #[test]
    fn applying_intake_head_drift_at_the_late_check_leaves_no_canonical_artifacts() {
        let fixture = intake_binding_fixture("ledger-applying-intake-head-drift-late");
        let request = fixture
            .request_for(VerdictVerb::Accept, "applying intake drift at late check")
            .expect("construct applying fixture verdict request");
        let initial =
            read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
                .expect("read fixture verdict preflight");
        let runtime = fixture.runtime();
        let mut human = ScriptedHuman::new(Some(format!(
            "APPLY ACCEPT {}\n",
            &fixture.proposal_id[..12]
        )));
        let decision = match preflight_human_verdict_with_human(
            initial,
            request,
            &runtime,
            &mut human,
            &FixedClock(
                Timestamp::parse("2026-07-21T07:00:02.000000000Z").expect("fixed verdict time"),
            ),
        )
        .expect("confirm fixture applying verdict")
        {
            PreflightVerdict::Confirmed(decision) => decision,
            PreflightVerdict::Idempotent(_) => panic!("new fixture verdict must not be idempotent"),
        };
        let canon_before = fixture.canon_hashes();
        let indexes_before = fixture.index_hashes();
        let receipts_before = captured_reindex_receipts(&fixture);
        let current_before = fs::read(fixture.store_root.join("canon/CURRENT"))
            .expect("capture current pointer bytes");
        let ledger_before = fs::read(fixture.store_root.join("ledger/verdicts.jsonl"))
            .expect("capture ledger bytes");
        let profile_path = fixture.profile_path.clone();
        let store_root = fixture.store_root.clone();
        install_verdict_late_intake_check_hook_for_test(move || {
            append_untrusted_intake_rejection(profile_path, store_root)
        });

        let error = apply_human_verdict(decision, &runtime)
            .expect_err("intake drift at the late applying boundary must fail closed");

        assert_eq!(error.code(), "intake.head_mismatch");
        assert_eq!(fixture.canon_hashes(), canon_before);
        assert_eq!(fixture.index_hashes(), indexes_before);
        assert_eq!(captured_reindex_receipts(&fixture), receipts_before);
        assert_eq!(
            fs::read(fixture.store_root.join("canon/CURRENT")).unwrap(),
            current_before
        );
        assert_eq!(
            fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).unwrap(),
            ledger_before
        );
    }

    #[test]
    fn fixture_genesis_and_first_verdict_use_canonical_ledger_lines() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("ledger-genesis");
        let events = fixture.fixture_genesis_and_reject();
        let bytes = fixture.append_fixture_events(&events);
        let lines = bytes
            .split_inclusive(|byte| *byte == b'\n')
            .collect::<Vec<_>>();

        assert_eq!(lines.len(), 2);
        assert!(std::str::from_utf8(lines[0])
            .unwrap()
            .contains("\"event_id\":\"evt-0000000000000000\""));
        assert!(std::str::from_utf8(lines[1])
            .unwrap()
            .contains("\"event_id\":\"evt-0000000000000001\""));
        assert!(events[0].previous_hash().is_none());
        assert_eq!(events[1].previous_hash(), Some(events[0].hash()));
    }

    #[test]
    fn ledger_only_fixture_verdict_keeps_the_seed_head_and_has_no_canonical_result() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("ledger-only-seed-head");
        let events = fixture.fixture_genesis_and_reject();
        let LedgerEvent::Verdict(verdict) = &events[1] else {
            panic!("fixture must contain a verdict after genesis")
        };

        assert_eq!(verdict.payload().resulting_canonical_head, "seed");
        assert_eq!(
            verdict.payload().canonical_result,
            sovmem::profile::CanonicalResult::none()
        );
        assert!(verdict.payload().rewrite_hash.is_none());
        assert!(verdict.payload().rewrite_bytes.is_none());
    }

    #[test]
    fn fixture_successors_preserve_the_prior_ledger_prefix() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("ledger-prefix");
        let first = fixture.append_fixture_events(&fixture.fixture_genesis_and_reject());
        let second = fixture.append_fixture_events(&fixture.fixture_tp());

        assert!(second.starts_with(&first));
        assert!(second.len() > first.len());
    }

    #[test]
    fn level_four_walks_every_artifact_to_the_ledger_head() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("level-four");
        fixture.install_fixture_chain_with_one_applied_generation();
        let summary = fixture.verify_level4().expect("verify fixture Level 4");

        assert_eq!(summary.level, 4);
        assert_eq!(summary.events, 3);
        assert_eq!(summary.verdicts, 2);
        assert_eq!(summary.applied_generations, 1);
        assert_eq!(summary.current_generation, "gen-0000000000000001");
        assert_eq!(summary.errors, 0);
    }

    fn apply_mixed_v2_verdict(fixture: &ProvisionedP3Fixture, sequence: u64, verb: VerdictVerb) {
        let profile = load_p3_profile(&fixture.profile_path).expect("load mixed-chain profile");
        let store = sovmem::profile::inspect_store(&profile, &fixture.store_root)
            .expect("inspect mixed-chain store");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = format!("ssa-0/{target_path}");
        let source_root = if store.head.as_str() == "seed" {
            store.paths.seed.join("bundle")
        } else {
            store
                .paths
                .canon
                .join("generations")
                .join(store.head.as_str())
                .join("bundle")
        };
        let current_bytes =
            fs::read(source_root.join(target_path)).expect("read mixed-chain current target bytes");
        let frozen_id = target_id.trim_end_matches(".md");
        let normalized = String::from_utf8(current_bytes.clone())
            .expect("mixed-chain current target is UTF-8")
            .replace(
                &format!("\nid: {frozen_id}\n"),
                &format!("\nid: {target_id}\n"),
            )
            .replace(
                &format!("\nsource: vault://{frozen_id}\n"),
                &format!("\nsource: vault://{target_id}\n"),
            )
            .replace(
                &format!("\nresource: vault://{frozen_id}\n"),
                &format!("\nresource: vault://{target_id}\n"),
            )
            .into_bytes();
        let mut candidate_bytes = normalized.clone();
        candidate_bytes.extend_from_slice(
            format!("\nTask 10 candidate revision for verdict {sequence}.\n").as_bytes(),
        );
        let candidate_path = fixture
            .profile_path
            .parent()
            .expect("mixed-chain profile has parent")
            .join(format!("mixed-chain-candidate-{sequence}.md"));
        fs::write(&candidate_path, &candidate_bytes).expect("write mixed-chain candidate");
        let submitted = sovmem::profile::submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: format!("src:agent:task10-mixed-{sequence:02}"),
                operation: ProposalOperation::Replace,
                intent: if verb == VerdictVerb::Rewrite {
                    ProposalIntent::Rewrite
                } else {
                    ProposalIntent::Accept
                },
                target_id: target_id.clone(),
                target_path: target_path.to_owned(),
                candidate_path,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(mixed_chain_time(sequence, 0)),
        )
        .expect("submit mixed-chain inbox item");
        let mut sealer =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let sealed = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut sealer,
            &FixedClock(mixed_chain_time(sequence, 1)),
        )
        .expect("seal mixed-chain inbox item");
        assert_eq!(sealer.show_count(), 1);
        assert_eq!(sealer.read_count(), 1);
        if sealed.state != "staged" {
            let profile =
                load_p3_profile(&fixture.profile_path).expect("load rejected seal profile");
            let store = sovmem::profile::inspect_store(&profile, &fixture.store_root)
                .expect("inspect rejected seal store");
            let intake = read_intake(&store.paths).expect("read rejected seal intake");
            panic!(
                "mixed-chain seal {sequence} rejected with rules {:?}",
                intake
                    .events()
                    .last()
                    .expect("rejection event")
                    .payload()
                    .rule_codes
            );
        }
        assert_eq!(sealed.intake_sequence, sequence);
        let proposal_id = sealed
            .proposal_id
            .expect("mixed-chain seal publishes protected proposal");

        let profile = load_p3_profile(&fixture.profile_path).expect("reload mixed-chain profile");
        let store = sovmem::profile::inspect_store(&profile, &fixture.store_root)
            .expect("inspect sealed mixed-chain store");
        let rewrite = (verb == VerdictVerb::Rewrite).then(|| {
            let mut bytes = normalized;
            bytes.extend_from_slice(
                format!("\nTask 10 human rewrite for verdict {sequence}.\n").as_bytes(),
            );
            RewriteBytes::new(bytes, store.policy.policy.max_rewrite_bytes)
                .expect("construct mixed-chain human rewrite")
        });
        let request = sovmem::profile::transaction::HumanVerdictRequest {
            proposal_id: proposal_id.clone(),
            verb,
            reason: Reason::new(
                format!("task 10 {} reason {sequence}", verb.as_str()).as_bytes(),
                store.policy.policy.max_reason_bytes,
            )
            .expect("construct mixed-chain reason"),
            rewrite,
        };
        let initial =
            read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
                .expect("bind mixed-chain protected proposal");
        let runtime = TransactionRuntime::production(&profile, &store)
            .expect("construct mixed-chain transaction runtime");
        let mut human = ScriptedHuman::new(Some(format!(
            "APPLY {} {}\n",
            verb.as_str(),
            &proposal_id[..12]
        )));
        let outcome = match preflight_human_verdict_with_human(
            initial,
            request,
            &runtime,
            &mut human,
            &FixedClock(mixed_chain_time(sequence, 2)),
        )
        .expect("confirm mixed-chain human verdict")
        {
            PreflightVerdict::Confirmed(decision) => {
                apply_human_verdict(decision, &runtime).expect("apply mixed-chain human verdict")
            }
            PreflightVerdict::Idempotent(_) => panic!("mixed-chain verdict cannot replay"),
        };
        assert_eq!(human.show_count(), 1);
        assert_eq!(human.read_count(), 1);
        assert_eq!(outcome.applied, verb.applies());
        assert_eq!(outcome.generation.is_some(), verb.applies());
    }

    fn mixed_chain_time(sequence: u64, phase: u64) -> Timestamp {
        Timestamp::parse(&format!(
            "2026-07-22T08:{sequence:02}:{phase:02}.000000000Z"
        ))
        .expect("parse fixed mixed-chain timestamp")
    }

    #[test]
    fn twenty_mixed_verdict_chain_preserves_exact_ledger_artifacts_and_terminal_lifecycles() {
        let fixture = ProvisionedP3Fixture::empty("task-10-mixed-verdict-chain");
        assert!(
            fixture.store_root.join("intake/events.jsonl").is_file(),
            "the mixed chain must begin from the protected v2 intake layout"
        );
        let expected_verbs = [
            VerdictVerb::Reject,
            VerdictVerb::Accept,
            VerdictVerb::Reject,
            VerdictVerb::Tp,
            VerdictVerb::Fp,
            VerdictVerb::Rewrite,
            VerdictVerb::Accept,
            VerdictVerb::Reject,
            VerdictVerb::Tp,
            VerdictVerb::Fp,
            VerdictVerb::Rewrite,
            VerdictVerb::Accept,
            VerdictVerb::Reject,
            VerdictVerb::Tp,
            VerdictVerb::Fp,
            VerdictVerb::Rewrite,
            VerdictVerb::Accept,
            VerdictVerb::Tp,
            VerdictVerb::Fp,
            VerdictVerb::Rewrite,
        ];
        for (offset, verb) in expected_verbs.into_iter().enumerate() {
            apply_mixed_v2_verdict(&fixture, offset as u64 + 1, verb);
        }

        let profile = load_p3_profile(&fixture.profile_path).expect("load mixed-chain profile");
        let report =
            verify_store_level4(&profile, &fixture.store_root).expect("verify 20-verdict fixture");
        assert_eq!(report.level, 4);
        assert_eq!(report.events, 21);
        assert_eq!(report.verdicts, 20);
        assert_eq!(report.applied_generations, 8);
        assert_eq!(report.current_generation, "gen-0000000000000008");
        let ledger_path = fixture.store_root.join("ledger/verdicts.jsonl");
        let ledger_bytes = fs::read(&ledger_path).expect("read 20-verdict ledger bytes");
        let ledger = read_ledger(&ledger_path).expect("read 20-verdict ledger");
        assert_eq!(
            [
                ledger.accept,
                ledger.rewrite,
                ledger.reject,
                ledger.tp,
                ledger.fp,
            ],
            [4, 4, 4, 4, 4]
        );
        assert_eq!(ledger.events.len(), 21);
        let mut prefix = Vec::new();
        let mut previous_hash = None;
        for (sequence, event) in ledger.events.iter().enumerate() {
            let sequence = sequence as u64;
            assert_eq!(event.sequence(), sequence);
            assert_eq!(event.event_id(), format!("evt-{sequence:016}"));
            assert_eq!(event.previous_hash(), previous_hash.as_deref());
            let canonical = event.canonical_line_bytes().expect("render exact event");
            prefix.extend_from_slice(&canonical);
            assert_eq!(&ledger_bytes[..prefix.len()], prefix.as_slice());
            previous_hash = Some(event.hash().to_owned());
        }
        let actual_verbs = ledger
            .events
            .iter()
            .skip(1)
            .map(|event| match event {
                LedgerEvent::Genesis(_) => panic!("only the first event may be genesis"),
                LedgerEvent::Verdict(event) => event.payload().verb,
            })
            .collect::<Vec<_>>();
        assert_eq!(actual_verbs, expected_verbs);
        assert_eq!(report.ledger_head, ledger.final_hash.clone().unwrap());

        let store = sovmem::profile::inspect_store(&profile, &fixture.store_root)
            .expect("inspect verified mixed-chain store");
        let intake = read_intake(&store.paths).expect("read mixed-chain protected intake");
        assert_eq!(intake.events().len(), 20);
        for (offset, event) in ledger.events.iter().skip(1).enumerate() {
            let LedgerEvent::Verdict(verdict) = event else {
                panic!("only the first ledger event may be genesis");
            };
            let sequence = offset as u64 + 1;
            let intake_event = &intake.events()[offset];
            assert_eq!(intake_event.payload().sequence, sequence);
            assert_eq!(verdict.payload().intake_sequence, sequence);
            assert_eq!(
                verdict.payload().intake_head_hash.as_deref(),
                Some(intake_event.hash())
            );
            let prefix = intake_prefix(
                &intake,
                &IntakeHead {
                    sequence,
                    event_hash: Some(intake_event.hash().to_owned()),
                },
            )
            .expect("replay exact mixed-chain intake prefix");
            assert_eq!(prefix.head.sequence, sequence);
            assert_eq!(prefix.head.event_hash.as_deref(), Some(intake_event.hash()));
            assert!(prefix
                .staged
                .iter()
                .any(|summary| summary.proposal_id == verdict.payload().proposal_id));
        }

        let expected_generations = (1..=8)
            .map(|number| format!("gen-{number:016}"))
            .collect::<Vec<_>>();
        assert_eq!(
            directory_names(&fixture.store_root.join("canon/generations")),
            expected_generations
        );

        let applied_verdict_ids = ledger
            .events
            .iter()
            .skip(1)
            .filter_map(|event| match event {
                LedgerEvent::Genesis(_) => panic!("only the first event may be genesis"),
                LedgerEvent::Verdict(event) if event.payload().verb.applies() => {
                    Some(event.payload().verdict_id.clone())
                }
                LedgerEvent::Verdict(_) => None,
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(applied_verdict_ids.len(), 8);
        assert_eq!(
            regular_file_stems(
                &fixture
                    .store_root
                    .join("canon/generations/gen-0000000000000008/bundle/judgments"),
                ".md",
            ),
            applied_verdict_ids
        );
        assert_eq!(
            regular_file_stems(&fixture.store_root.join("evidence/reindex"), ".json"),
            applied_verdict_ids
        );

        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
        let records = proposal_list(
            &profile,
            &fixture.store_root,
            ProposalFilter { state: None },
        )
        .expect("list retained proposals");
        assert_eq!(records.len(), 20);
        let terminal = ledger
            .events
            .iter()
            .skip(1)
            .enumerate()
            .map(|(offset, event)| match event {
                LedgerEvent::Genesis(_) => panic!("only the first event may be genesis"),
                LedgerEvent::Verdict(event) => (
                    event.payload().proposal_id.clone(),
                    (
                        offset as u64 + 1,
                        event.payload().verb,
                        event.payload().verdict_id.clone(),
                    ),
                ),
            })
            .collect::<BTreeMap<_, _>>();
        let mut ordered = records.iter().collect::<Vec<_>>();
        ordered.sort_by_key(|record| terminal[&record.proposal_id].0);
        let mut final_applied_by_target = BTreeMap::new();
        for record in &ordered {
            let (_, verb, _) = &terminal[&record.proposal_id];
            if verb.applies() {
                final_applied_by_target.insert(
                    record
                        .target_id
                        .as_ref()
                        .expect("retained target ID")
                        .clone(),
                    record.proposal_id.clone(),
                );
            }
        }
        let mut ruled = 0;
        let mut superseded = 0;
        let mut applied = 0;
        for record in &ordered {
            let (_, verb, verdict_id) = &terminal[&record.proposal_id];
            assert!(record.retained);
            assert_eq!(record.terminal_verb, Some(*verb));
            assert_eq!(record.verdict_id.as_deref(), Some(verdict_id.as_str()));
            let expected_state = if !verb.applies() {
                ProposalState::Ruled
            } else if final_applied_by_target
                [record.target_id.as_ref().expect("retained target ID")]
                == record.proposal_id
            {
                ProposalState::Applied
            } else {
                ProposalState::Superseded
            };
            assert_eq!(record.state, expected_state, "{}", record.proposal_id);
            match record.state {
                ProposalState::Ruled => ruled += 1,
                ProposalState::Superseded => superseded += 1,
                ProposalState::Applied => applied += 1,
                other => panic!("unexpected terminal lifecycle state: {}", other.as_str()),
            }
        }
        assert_eq!(ruled, 12);
        assert_eq!(superseded, 7);
        assert_eq!(applied, 1);
    }

    fn directory_names(path: &std::path::Path) -> Vec<String> {
        let mut names = fs::read_dir(path)
            .expect("read deterministic generation directory")
            .map(|entry| entry.expect("read deterministic generation entry"))
            .filter(|entry| {
                entry
                    .file_type()
                    .expect("read generation entry type")
                    .is_dir()
            })
            .map(|entry| {
                entry
                    .file_name()
                    .into_string()
                    .expect("UTF-8 generation name")
            })
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn regular_file_stems(path: &std::path::Path, suffix: &str) -> BTreeSet<String> {
        fs::read_dir(path)
            .expect("read deterministic artifact directory")
            .map(|entry| entry.expect("read deterministic artifact entry"))
            .filter(|entry| {
                entry
                    .file_type()
                    .expect("read artifact entry type")
                    .is_file()
            })
            .map(|entry| {
                entry
                    .file_name()
                    .into_string()
                    .expect("UTF-8 artifact name")
                    .strip_suffix(suffix)
                    .expect("expected deterministic artifact suffix")
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn level_four_verifies_durable_receipt_evidence_without_opening_disposable_index() {
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("level-four-durable-receipt");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).unwrap();

        assert!(
            !fixture
                .store_root
                .join("derived/indexes/gen-0000000000000001")
                .exists(),
            "the pre-Task-13 fixture deliberately retains durable receipt evidence only"
        );
        assert_eq!(
            verify_store_level4(&profile, &fixture.store_root)
                .expect("Level 4 verifies durable canonical evidence")
                .current_generation,
            "gen-0000000000000001"
        );
    }

    #[test]
    fn level_four_preserves_policy_binding_precedence_over_disposable_index_absence() {
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("level-four-policy-precedence");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = fixture.clone_store("policy-precedence");
        let policy_path = fs::read_dir(store.join("policies"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(policy_path, b"not a policy\n").unwrap();

        assert_eq!(
            verify_store_level4(&profile, &store).unwrap_err().code(),
            "ledger.proposal_binding"
        );
    }

    #[test]
    fn level_four_preserves_current_pointer_code_over_disposable_index_absence() {
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("level-four-current-precedence");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = fixture.clone_store("current-precedence");
        fs::write(store.join("canon/CURRENT"), b"seed\n").unwrap();

        assert_eq!(
            verify_store_level4(&profile, &store).unwrap_err().code(),
            "ledger.current_mismatch"
        );
    }

    #[test]
    fn level_four_fails_closed_on_pointer_receipt_policy_generation_and_lease_tampering() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("level-four-corruption");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).unwrap();

        let current = fixture.clone_store("current");
        fs::write(current.join("canon/CURRENT"), b"seed\n").unwrap();
        assert_eq!(
            verify_store_level4(&profile, &current).unwrap_err().code(),
            "ledger.current_mismatch"
        );

        let receipt = fixture.clone_store("receipt");
        let path = fs::read_dir(receipt.join("evidence/reindex"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut bytes = fs::read(&path).unwrap();
        bytes[0] ^= 1;
        fs::write(path, bytes).unwrap();
        assert_eq!(
            verify_store_level4(&profile, &receipt).unwrap_err().code(),
            "ledger.reindex_receipt_mismatch"
        );

        let policy = fixture.clone_store("policy");
        let path = fs::read_dir(policy.join("policies"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(path, b"not a policy\n").unwrap();
        assert_eq!(
            verify_store_level4(&profile, &policy).unwrap_err().code(),
            "ledger.proposal_binding"
        );

        let generation = fixture.clone_store("generation");
        fs::create_dir(generation.join("canon/generations/gen-0000000000000002")).unwrap();
        assert_eq!(
            verify_store_level4(&profile, &generation)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );

        let lease = fixture.clone_store("lease");
        fs::create_dir(lease.join("locks/verdict.lock")).unwrap();
        fs::write(lease.join("locks/verdict.lock/owner.json"), b"fixture\n").unwrap();
        assert_eq!(
            verify_store_level4(&profile, &lease).unwrap_err().code(),
            "store.locked"
        );

        let temporary = fixture.clone_store("lease-temporary");
        fs::create_dir(
            temporary
                .join("locks")
                .join(format!(".lease-verdict-{}", "a".repeat(64))),
        )
        .unwrap();
        assert_eq!(
            verify_store_level4(&profile, &temporary)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
    }

    #[test]
    fn level_four_rejects_unknown_extra_policy_snapshots() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("level-four-unknown-policy");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = fixture.clone_store("unknown-policy-snapshot");
        fs::write(
            store
                .join("policies")
                .join(format!("{}.toml", "f".repeat(64))),
            fs::read(&fixture.policy_template).unwrap(),
        )
        .unwrap();

        assert_eq!(
            verify_store_level4(&profile, &store).unwrap_err().code(),
            "ledger.proposal_binding"
        );
    }

    #[test]
    fn level_four_rejects_a_tampered_current_policy_snapshot() {
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("level-four-tampered-current-policy");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = fixture.clone_store("tampered-current-policy-snapshot");
        let rotated = String::from_utf8(fs::read(store.join("policy.toml")).unwrap())
            .unwrap()
            .replacen("max_open_total = 50", "max_open_total = 51", 1)
            .into_bytes();
        let rotated_hash = blake3::hash(&rotated).to_hex().to_string();
        fs::write(store.join("policy.toml"), &rotated).unwrap();
        fs::write(
            store.join("policies").join(format!("{rotated_hash}.toml")),
            fs::read(&fixture.policy_template).unwrap(),
        )
        .unwrap();

        assert_eq!(
            verify_store_level4(&profile, &store).unwrap_err().code(),
            "ledger.proposal_binding"
        );
    }

    #[test]
    fn level_four_recomputes_receipt_commitments_without_a_disposable_index() {
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("level-four-receipt-commitments");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = fixture.clone_store("forged-receipt");
        let receipt_path = fs::read_dir(store.join("evidence/reindex"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut receipt: ReindexReceipt =
            serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
        receipt.documents_hash = "a".repeat(64);
        let receipt_bytes = reindex_receipt_bytes(&receipt).unwrap();
        fs::write(&receipt_path, &receipt_bytes).unwrap();

        let mut events = read_ledger(&store.join("ledger/verdicts.jsonl"))
            .unwrap()
            .events;
        let LedgerEvent::Verdict(event) = events.pop().unwrap() else {
            panic!("fixture must end in an applied verdict");
        };
        let mut payload = event.payload().clone();
        payload.canonical_result.reindex_receipt_hash =
            Some(blake3::hash(&receipt_bytes).to_hex().to_string());
        events.push(LedgerEvent::Verdict(Box::new(
            VerdictEvent::new(payload).unwrap(),
        )));
        fs::write(
            store.join("ledger/verdicts.jsonl"),
            fixture.canonical_ledger_bytes(&events),
        )
        .unwrap();

        assert_eq!(
            verify_store_level4(&profile, &store).unwrap_err().code(),
            "ledger.reindex_receipt_mismatch"
        );
    }

    #[test]
    fn level_four_binds_every_verdict_to_the_genesis_store() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("level-four-store-binding");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = fixture.clone_store("foreign-event-store");
        let mut events = read_ledger(&store.join("ledger/verdicts.jsonl"))
            .unwrap()
            .events;
        let LedgerEvent::Verdict(event) = events.pop().unwrap() else {
            panic!("fixture must end in a verdict");
        };
        let mut payload = event.payload().clone();
        payload.store_id = "foreign-store".to_owned();
        events.push(LedgerEvent::Verdict(Box::new(
            VerdictEvent::new(payload).unwrap(),
        )));
        fs::write(
            store.join("ledger/verdicts.jsonl"),
            fixture.canonical_ledger_bytes(&events),
        )
        .unwrap();

        assert_eq!(
            verify_store_level4(&profile, &store).unwrap_err().code(),
            "ledger.proposal_binding"
        );
    }

    #[test]
    fn ledger_rejects_noncanonical_and_tampered_chain_evidence_with_stable_codes() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("ledger-corruption");
        let events = fixture.fixture_genesis_and_reject();
        let baseline = fixture.append_fixture_events(&events);
        let baseline_text = std::str::from_utf8(&baseline).unwrap();
        let lines = baseline_text.lines().collect::<Vec<_>>();

        let mut pretty = serde_json::from_str::<serde_json::Value>(lines[0]).unwrap();
        let reordered = {
            let bytes = serde_json::to_vec(&pretty).unwrap();
            let mut bytes = bytes;
            bytes.push(b'\n');
            bytes.extend_from_slice(lines[1].as_bytes());
            bytes.push(b'\n');
            bytes
        };
        pretty.as_object_mut().unwrap().remove("store_id");
        let mut missing_field = serde_json::to_vec(&pretty).unwrap();
        missing_field.push(b'\n');
        missing_field.extend_from_slice(lines[1].as_bytes());
        missing_field.push(b'\n');

        let candidate_hash = match &events[1] {
            LedgerEvent::Verdict(event) => event.payload().candidate_hash.clone(),
            LedgerEvent::Genesis(_) => panic!("fixture verdict expected"),
        };
        let alternate_first = if candidate_hash.starts_with('a') {
            'b'
        } else {
            'a'
        };
        let tampered_candidate_hash = format!("{alternate_first}{}", &candidate_hash[1..]);
        let mut invalid_hash = baseline_text.replacen(
            &format!("\"candidate_hash\":\"{candidate_hash}\""),
            &format!("\"candidate_hash\":\"{tampered_candidate_hash}\""),
            1,
        );
        let mut empty_reason =
            baseline_text.replacen("\"reason\":\"fixture ledger reject\"", "\"reason\":\"\"", 1);
        let mut invalid_evaluator = baseline_text.replacen(
            "\"evaluator_meta_hash\":null",
            "\"evaluator_meta_hash\":\"not-a-hash\"",
            1,
        );
        let mut invalid_verb =
            baseline_text.replacen("\"verb\":\"REJECT\"", "\"verb\":\"ACCEPT\"", 1);
        let mut changed_head = baseline_text.replacen(
            "\"resulting_canonical_head\":\"seed\"",
            "\"resulting_canonical_head\":\"gen-0000000000000001\"",
            1,
        );
        for bytes in [
            &mut invalid_hash,
            &mut empty_reason,
            &mut invalid_evaluator,
            &mut invalid_verb,
            &mut changed_head,
        ] {
            assert!(bytes.ends_with('\n'));
        }

        let mut duplicate = baseline.clone();
        duplicate.extend_from_slice(lines[1].as_bytes());
        duplicate.push(b'\n');
        let reordered_events = format!("{}\n{}\n", lines[1], lines[0]).into_bytes();
        let deleted_genesis = format!("{}\n", lines[1]).into_bytes();
        let mut no_final_lf = baseline.clone();
        no_final_lf.pop();
        let mut extra_final_lf = baseline.clone();
        extra_final_lf.push(b'\n');
        let unknown_field = baseline_text.replacen("}\n", ",\"unknown\":true}\n", 1);

        let mut sequence_payload = match &events[1] {
            LedgerEvent::Verdict(event) => event.payload().clone(),
            LedgerEvent::Genesis(_) => panic!("fixture verdict expected"),
        };
        sequence_payload.sequence = 2;
        sequence_payload.event_id = "evt-0000000000000002".to_owned();
        let wrong_sequence = fixture.canonical_ledger_bytes(&[
            events[0].clone(),
            LedgerEvent::Verdict(Box::new(VerdictEvent::new(sequence_payload).unwrap())),
        ]);

        let mut id_payload = match &events[1] {
            LedgerEvent::Verdict(event) => event.payload().clone(),
            LedgerEvent::Genesis(_) => panic!("fixture verdict expected"),
        };
        id_payload.event_id = "evt-0000000000000002".to_owned();
        let wrong_event_id = fixture.canonical_ledger_bytes(&[
            events[0].clone(),
            LedgerEvent::Verdict(Box::new(VerdictEvent::new(id_payload).unwrap())),
        ]);

        let mut previous_payload = match &events[1] {
            LedgerEvent::Verdict(event) => event.payload().clone(),
            LedgerEvent::Genesis(_) => panic!("fixture verdict expected"),
        };
        previous_payload.previous_event_hash = Some("f".repeat(64));
        let wrong_previous = fixture.canonical_ledger_bytes(&[
            events[0].clone(),
            LedgerEvent::Verdict(Box::new(VerdictEvent::new(previous_payload).unwrap())),
        ]);

        let mut transaction_payload = fixture.fixture_tp().remove(0);
        let LedgerEvent::Verdict(ref mut transaction_event) = transaction_payload else {
            panic!("fixture TP verdict expected");
        };
        let mut transaction_payload = transaction_event.payload().clone();
        transaction_payload.transaction_id = match &events[1] {
            LedgerEvent::Verdict(event) => event.payload().transaction_id.clone(),
            LedgerEvent::Genesis(_) => panic!("fixture verdict expected"),
        };
        let duplicate_transaction = fixture.canonical_ledger_bytes(&[
            events[0].clone(),
            events[1].clone(),
            LedgerEvent::Verdict(Box::new(VerdictEvent::new(transaction_payload).unwrap())),
        ]);

        let cases: Vec<(&str, Vec<u8>, &str)> = vec![
            ("invalid UTF-8", vec![0xff, b'\n'], "ledger.utf8"),
            ("missing final LF", no_final_lf, "ledger.line_encoding"),
            ("extra final LF", extra_final_lf, "ledger.line_encoding"),
            ("reordered fields", reordered, "ledger.line_encoding"),
            ("unknown field", unknown_field.into_bytes(), "ledger.schema"),
            ("missing field", missing_field, "ledger.schema"),
            ("truncated line", b"{\n".to_vec(), "ledger.parse"),
            ("duplicated event", duplicate, "ledger.sequence"),
            (
                "reordered events",
                reordered_events,
                "ledger.genesis_missing",
            ),
            ("deleted genesis", deleted_genesis, "ledger.genesis_missing"),
            ("sequence mismatch", wrong_sequence, "ledger.sequence"),
            ("event ID mismatch", wrong_event_id, "ledger.event_id"),
            (
                "previous hash mismatch",
                wrong_previous,
                "ledger.previous_hash",
            ),
            (
                "event hash mismatch",
                invalid_hash.into_bytes(),
                "ledger.event_hash",
            ),
            (
                "duplicate transaction",
                duplicate_transaction,
                "ledger.duplicate_id",
            ),
            (
                "empty reason",
                empty_reason.into_bytes(),
                "ledger.reason_empty",
            ),
            (
                "invalid evaluator commitment",
                invalid_evaluator.into_bytes(),
                "ledger.evaluator_binding",
            ),
            (
                "invalid verb semantics",
                invalid_verb.into_bytes(),
                "ledger.verb_semantics",
            ),
            (
                "non-applying head change",
                changed_head.into_bytes(),
                "ledger.head_transition",
            ),
        ];

        for (name, bytes, expected) in cases {
            fs::write(fixture.ledger_path(), bytes).unwrap();
            let error = read_ledger(&fixture.ledger_path()).unwrap_err();
            assert_eq!(error.code(), expected, "{name}");
        }
    }
}

pub mod poisoning_matrix_tests {
    #[allow(unused_imports)]
    use super::full_loop_tests;
    use sovmem::profile::p3_test_support::{
        FaultPlan, FixedClock, ProvisionedP3Fixture, RecoveryLeasePublicationBarrier, ScriptedHuman,
    };
    use sovmem::profile::transaction::{
        apply_human_verdict, preflight_human_verdict_from_store_with_human,
        seal_intake_submission_with_human, seal_intake_submission_with_human_and_faults,
        FaultPoint, PreflightVerdict,
    };

    use sovmem::profile::{
        append_intake_event, inspect_store, load_p3_profile, parse_policy, read_intake,
        read_ledger, read_sealed_pair, recover_incomplete_intake_transaction, submit_proposal,
        submit_proposal_with_clock, validate_decision, verify_store_level4, Confidence,
        EvaluatorBlock, GenesisEvent, IntakeEvent, IntakeHead, IntakeLeaseIdentity, IntakeOutcome,
        IntakePayload, IntakePrefix, IntakePublicationLease, LedgerEvent, PolicyContext,
        PolicyEvaluator, ProposalIntent, ProposalOperation, ProposalSubmission, ProposalSummary,
        Reason, RewriteBytes, StorePaths, SubmissionMeta, Timestamp, VerdictEvent,
        VerdictIdentityInput, VerdictPayload, VerdictVerb,
    };
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    type MatrixRun = fn() -> MatrixObservation;

    #[derive(Clone, Copy)]
    enum ExpectedOutcome {
        StableCode(&'static str),
    }

    #[cfg(unix)]
    const SYMLINK_EXPECTED: ExpectedOutcome = ExpectedOutcome::StableCode("store.symlink");

    #[cfg(not(unix))]
    const SYMLINK_EXPECTED: ExpectedOutcome =
        ExpectedOutcome::StableCode("proposal.confidence_invalid");

    struct MatrixCase {
        family: &'static str,
        first_stage: &'static str,
        expected: ExpectedOutcome,
        cli_exit: Option<i32>,
        root_change: RootChange,
        redaction_sentinel: Option<&'static str>,
        run: MatrixRun,
    }

    #[derive(Clone, Copy)]
    enum RootChange {
        None,
        RedactedIntakeReceipt,
    }

    struct MatrixObservation {
        code: Option<String>,
        cli_exit: Option<i32>,
        before: DurableSnapshot,
        after: DurableSnapshot,
        redacted_input: Option<String>,
        redacted_projection: Option<String>,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct DurableSnapshot {
        root_hash: String,
        ledger_hash: String,
    }

    #[test]
    fn intake_crash_prepared_only_staged_event_hash_poison_is_retained_byte_exact() {
        let (fixture, submitted) =
            prepared_only_intake_fixture("matrix-intake-prepared-only-staged-event-hash", false);
        let prepared = fixture
            .store_root
            .join("intake/transactions")
            .join(&submitted.submission_id)
            .join("prepared.json");
        let mut bytes = fs::read(&prepared).unwrap();
        let prefix = b"\"event_hash\":\"";
        let offset = bytes
            .windows(prefix.len())
            .position(|window| window == prefix)
            .unwrap()
            + prefix.len();
        bytes[offset] = if bytes[offset] == b'0' { b'1' } else { b'0' };
        fs::write(&prepared, bytes).unwrap();
        let intake_before = durable_tree_hash(&fixture.store_root.join("intake"));

        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
        assert_eq!(
            fixture
                .recover_intake_with_runtime(&fixture.runtime())
                .unwrap_err()
                .code(),
            "intake.incomplete"
        );
        assert_eq!(
            durable_tree_hash(&fixture.store_root.join("intake")),
            intake_before
        );
    }

    #[test]
    fn intake_crash_prepared_only_rejected_outcome_poison_is_retained_byte_exact() {
        let (fixture, submitted) =
            prepared_only_intake_fixture("matrix-intake-prepared-only-rejected-outcome", true);
        let prepared = fixture
            .store_root
            .join("intake/transactions")
            .join(&submitted.submission_id)
            .join("prepared.json");
        let bytes = fs::read(&prepared).unwrap();
        let poisoned = String::from_utf8(bytes)
            .unwrap()
            .replacen("\"outcome\":\"REJECTED\"", "\"outcome\":\"STAGED\"", 1)
            .into_bytes();
        fs::write(&prepared, poisoned).unwrap();
        let intake_before = durable_tree_hash(&fixture.store_root.join("intake"));

        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
        assert_eq!(
            fixture
                .recover_intake_with_runtime(&fixture.runtime())
                .unwrap_err()
                .code(),
            "intake.incomplete"
        );
        assert_eq!(
            durable_tree_hash(&fixture.store_root.join("intake")),
            intake_before
        );
    }

    fn prepared_only_intake_fixture(
        name: &str,
        rejected: bool,
    ) -> (ProvisionedP3Fixture, sovmem::profile::SubmitProposalResult) {
        let fixture = ProvisionedP3Fixture::empty(name);
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate_path = fixture
            .profile_path
            .parent()
            .unwrap()
            .join("prepared-only.md");
        let mut candidate = String::from_utf8(
            fs::read(fixture.store_root.join("seed/bundle").join(target_path)).unwrap(),
        )
        .unwrap();
        if rejected {
            candidate = candidate.replacen("provenance: synthesis\n", "", 1);
        } else {
            candidate.push_str("\nTask 8 prepared-only STAGED fixture\n");
        }
        fs::write(&candidate_path, candidate).unwrap();
        let submitted = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:task8-prepared-only".to_owned(),
                operation: ProposalOperation::Replace,
                intent: ProposalIntent::Accept,
                target_id: target_id.to_owned(),
                target_path: target_path.to_owned(),
                candidate_path,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(Timestamp::parse("2026-07-22T02:00:00.000000000Z").unwrap()),
        )
        .unwrap();
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        assert_eq!(
            seal_intake_submission_with_human_and_faults(
                &fixture.profile_path,
                &fixture.store_root,
                &submitted.submission_id,
                &mut human,
                &FixedClock(Timestamp::parse("2026-07-22T02:00:01.000000000Z").unwrap()),
                &FaultPlan::at(FaultPoint::AfterIntakeJournalSync),
            )
            .unwrap_err()
            .code(),
            "intake.incomplete"
        );
        (fixture, submitted)
    }

    #[cfg(unix)]
    #[test]
    fn intake_crash_recovery_parent_replacement_after_validation_retains_protected_custody() {
        use std::cell::Cell;
        use std::os::unix::fs::symlink;

        let fixture = ProvisionedP3Fixture::empty("matrix-intake-recovery-parent-replacement");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate_path = fixture
            .profile_path
            .parent()
            .unwrap()
            .join("parent-race.md");
        let candidate = fs::read(fixture.store_root.join("seed/bundle").join(target_path)).unwrap();
        fs::write(&candidate_path, candidate).unwrap();
        let submitted = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:task8-parent-race".to_owned(),
                operation: ProposalOperation::Replace,
                intent: ProposalIntent::Accept,
                target_id: target_id.to_owned(),
                target_path: target_path.to_owned(),
                candidate_path,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(Timestamp::parse("2026-07-22T02:01:00.000000000Z").unwrap()),
        )
        .unwrap();
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = inspect_store(&profile, &fixture.store_root).unwrap();
        let custody = store
            .paths
            .intake_transactions
            .join(&submitted.submission_id);
        fs::rename(store.paths.inbox.join(&submitted.submission_id), &custody).unwrap();
        let intake_before = durable_tree_hash(&store.paths.intake);
        let original_inbox = store.paths.staging.join("inbox-before-parent-race");
        let attacker_inbox = fixture.store_root.join("attacker-controlled-inbox");
        fs::create_dir(&attacker_inbox).unwrap();
        let raced = Cell::new(false);

        let error = recover_incomplete_intake_transaction(
            &store.paths,
            store.policy.policy.max_candidate_bytes,
            || {
                fs::rename(&store.paths.inbox, &original_inbox).unwrap();
                symlink(&attacker_inbox, &store.paths.inbox).unwrap();
                raced.set(true);
                Ok(())
            },
        )
        .unwrap_err();

        assert!(
            raced.get(),
            "the parent replacement must occur after validation"
        );
        assert_eq!(error.code(), "intake.record_invalid");
        assert_eq!(durable_tree_hash(&store.paths.intake), intake_before);
        assert!(custody.is_dir());
        assert!(!attacker_inbox.join(&submitted.submission_id).exists());
    }

    #[test]
    fn intake_crash_journal_hash_disagreement_is_retained_fail_closed() {
        let fixture = ProvisionedP3Fixture::empty("matrix-intake-crash-journal-mismatch");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate_path = fixture
            .profile_path
            .parent()
            .unwrap()
            .join("matrix-task8.md");
        let candidate = String::from_utf8(
            fs::read(fixture.store_root.join("seed/bundle").join(target_path)).unwrap(),
        )
        .unwrap()
            + "\nTask 8 poisoning fixture\n";
        fs::write(&candidate_path, candidate).unwrap();
        let submitted = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:task8-poisoning".to_owned(),
                operation: ProposalOperation::Replace,
                intent: ProposalIntent::Accept,
                target_id: target_id.to_owned(),
                target_path: target_path.to_owned(),
                candidate_path,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(Timestamp::parse("2026-07-22T01:40:00.000000000Z").unwrap()),
        )
        .unwrap();
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-22T01:40:01.000000000Z").unwrap()),
        )
        .unwrap();

        let prepared = fixture
            .store_root
            .join("intake/transactions")
            .join(&submitted.submission_id)
            .join("prepared.json");
        let mut bytes = fs::read(&prepared).unwrap();
        let prefix = b"\"event_hash\":\"";
        let offset = bytes
            .windows(prefix.len())
            .position(|window| window == prefix)
            .unwrap()
            + prefix.len();
        bytes[offset] = if bytes[offset] == b'0' { b'1' } else { b'0' };
        fs::write(&prepared, bytes).unwrap();
        let poisoned = snapshot(&fixture.store_root);
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
        let mut reseal_human = ScriptedHuman::new(None);
        let reseal = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut reseal_human,
            &FixedClock(Timestamp::parse("2026-07-22T01:40:02.000000000Z").unwrap()),
        )
        .unwrap_err();
        assert_eq!(reseal.code(), "intake.incomplete");
        assert_eq!(reseal_human.show_count(), 0);
        assert_eq!(reseal_human.read_count(), 0);
        assert_eq!(snapshot(&fixture.store_root), poisoned);
    }

    #[test]
    fn intake_crash_published_prefix_without_committed_is_not_repaired() {
        let fixture = ProvisionedP3Fixture::empty("matrix-intake-crash-prefix-order");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate_path = fixture
            .profile_path
            .parent()
            .unwrap()
            .join("matrix-task8-prefix.md");
        let candidate = String::from_utf8(
            fs::read(fixture.store_root.join("seed/bundle").join(target_path)).unwrap(),
        )
        .unwrap()
            + "\nTask 8 prefix-order fixture\n";
        fs::write(&candidate_path, candidate).unwrap();
        let submitted = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:task8-prefix".to_owned(),
                operation: ProposalOperation::Replace,
                intent: ProposalIntent::Accept,
                target_id: target_id.to_owned(),
                target_path: target_path.to_owned(),
                candidate_path,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(Timestamp::parse("2026-07-22T01:41:00.000000000Z").unwrap()),
        )
        .unwrap();
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        assert_eq!(
            sovmem::profile::transaction::seal_intake_submission_with_human_and_faults(
                &fixture.profile_path,
                &fixture.store_root,
                &submitted.submission_id,
                &mut human,
                &FixedClock(Timestamp::parse("2026-07-22T01:41:01.000000000Z").unwrap()),
                &FaultPlan::at(FaultPoint::AfterIntakeEventRename),
            )
            .unwrap_err()
            .code(),
            "intake.incomplete"
        );
        let transaction = fixture
            .store_root
            .join("intake/transactions")
            .join(&submitted.submission_id);
        assert!(!transaction.join("committed.json").exists());
        fs::write(transaction.join("published.json"), b"{}\n").unwrap();
        let intake_before = durable_tree_hash(&fixture.store_root.join("intake"));

        let recovery = fixture
            .recover_intake_with_runtime(&fixture.runtime())
            .unwrap_err();
        assert_eq!(recovery.code(), "intake.incomplete");
        assert_eq!(
            durable_tree_hash(&fixture.store_root.join("intake")),
            intake_before
        );
        assert!(!transaction.join("committed.json").exists());
    }

    #[test]
    fn forged_evaluator_candidate_hash_at_human_seal_writes_only_redacted_receipt_without_prompt() {
        let fixture = ProvisionedP3Fixture::empty("round2-forged-evaluator-seal-receipt");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate = intake_v2_candidate(&fixture, target_path, |mut bytes| {
            bytes.extend_from_slice(b"\nMATRIX_PRIVATE_CANDIDATE_SENTINEL\n");
            bytes
        });
        let candidate_hash = blake3::hash(&fs::read(&candidate).unwrap())
            .to_hex()
            .to_string();
        let evaluator_text = String::from_utf8(
            serde_json::to_vec(&EvaluatorBlock {
                protocol: "adversarial-bug-finding-v1".to_owned(),
                source_id: "src:human:matrix-private-evaluator".to_owned(),
                evaluated_at: "2026-07-26T08:12:00.000000000Z".to_owned(),
                candidate_hash,
                recommendation: "reject".to_owned(),
                reasons: vec!["MATRIX_PRIVATE_EVALUATOR_SENTINEL".to_owned()],
            })
            .unwrap(),
        )
        .unwrap();
        let evaluator = fixture
            .profile_path
            .parent()
            .unwrap()
            .join("round2-forged-evaluator.json");
        fs::write(&evaluator, &evaluator_text).unwrap();
        let submitted = submit_proposal_with_clock(
            intake_v2_submission(
                &fixture,
                &candidate,
                "src:agent:matrix-private-source",
                target_path,
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                Some(evaluator),
            ),
            &intake_v2_clock("2026-07-26T08:12:01.000000000Z"),
        )
        .expect("a structurally valid evaluator must submit before seal-time tampering");
        let metadata = fixture
            .store_root
            .join("staging/inbox")
            .join(&submitted.submission_id)
            .join("submission.meta.json");
        let mut forged: SubmissionMeta =
            serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
        forged
            .evaluator
            .as_mut()
            .expect("submitted evaluator")
            .candidate_hash = "0".repeat(64);
        fs::write(&metadata, serde_json::to_vec(&forged).unwrap()).unwrap();

        let before = snapshot(&fixture.store_root);
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let outcome = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &intake_v2_clock("2026-07-26T08:12:02.000000000Z"),
        )
        .expect("a forged evaluator binding must be converted into a redacted intake rejection");
        let after = snapshot(&fixture.store_root);
        assert_eq!(outcome.state, "rejected-at-intake");
        assert_eq!(
            human.show_count(),
            0,
            "forgery must stop before human/policy authority"
        );
        assert_eq!(
            human.read_count(),
            0,
            "forgery must not request human confirmation"
        );
        assert_ne!(after.root_hash, before.root_hash);
        assert_eq!(after.ledger_hash, before.ledger_hash);
        let receipt =
            String::from_utf8(fs::read(fixture.store_root.join("intake/events.jsonl")).unwrap())
                .unwrap();
        assert!(receipt.contains("proposal.evaluator_hash_mismatch"));
        assert!(!receipt.contains("MATRIX_PRIVATE_CANDIDATE_SENTINEL"));
        assert!(!receipt.contains("MATRIX_PRIVATE_EVALUATOR_SENTINEL"));
    }

    fn intake_v2_attack_families() -> Vec<&'static str> {
        let mut families = Vec::new();

        let forged_source = ProvisionedP3Fixture::empty("matrix-intake-v2-forged-source");
        let forged_source_candidate = intake_v2_candidate(
            &forged_source,
            "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
            |bytes| bytes,
        );
        let before = snapshot(&forged_source.store_root);
        let error = submit_proposal_with_clock(
            intake_v2_submission(
                &forged_source,
                &forged_source_candidate,
                "src:agent:MATRIX_PRIVATE_SOURCE_9304",
                "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                None,
            ),
            &intake_v2_clock("2026-07-22T04:20:00.000000000Z"),
        )
        .expect_err("forged source must fail transport framing");
        assert_intake_v2_immutable_failure(
            "forged_source",
            "proposal.path_unsafe",
            error.code(),
            before,
            snapshot(&forged_source.store_root),
            &forged_source,
        );
        families.push("forged_source");

        let forged_evaluator = ProvisionedP3Fixture::empty("matrix-intake-v2-forged-evaluator");
        let forged_evaluator_candidate = intake_v2_candidate(
            &forged_evaluator,
            "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
            |bytes| bytes,
        );
        let evaluator = forged_evaluator
            .profile_path
            .parent()
            .unwrap()
            .join("MATRIX_PRIVATE_ADVICE_9303-evaluator.json");
        fs::write(
            &evaluator,
            serde_json::to_vec(&EvaluatorBlock {
                protocol: "adversarial-bug-finding-v1".to_owned(),
                source_id: "src:human:matrix-private-evaluator".to_owned(),
                evaluated_at: "2026-07-22T04:20:01.000000000Z".to_owned(),
                candidate_hash: blake3::hash(&fs::read(&forged_evaluator_candidate).unwrap())
                    .to_hex()
                    .to_string(),
                recommendation: "reject".to_owned(),
                reasons: vec!["MATRIX_PRIVATE_REASON_9302".to_owned()],
            })
            .unwrap(),
        )
        .unwrap();
        let submitted = submit_proposal_with_clock(
            intake_v2_submission(
                &forged_evaluator,
                &forged_evaluator_candidate,
                "src:agent:matrix-private-source",
                "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                Some(evaluator),
            ),
            &intake_v2_clock("2026-07-22T04:20:02.000000000Z"),
        )
        .expect("a structurally valid evaluator must submit before seal-time tampering");
        let metadata = forged_evaluator
            .store_root
            .join("staging/inbox")
            .join(&submitted.submission_id)
            .join("submission.meta.json");
        let mut forged: SubmissionMeta =
            serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
        forged
            .evaluator
            .as_mut()
            .expect("submitted evaluator")
            .candidate_hash = "0".repeat(64);
        fs::write(&metadata, serde_json::to_vec(&forged).unwrap()).unwrap();
        let before = snapshot(&forged_evaluator.store_root);
        let mut human = ScriptedHuman::new(None);
        let outcome = seal_intake_submission_with_human(
            &forged_evaluator.profile_path,
            &forged_evaluator.store_root,
            &submitted.submission_id,
            &mut human,
            &intake_v2_clock("2026-07-22T04:20:03.000000000Z"),
        )
        .expect("forged evaluator must create a redacted pre-policy intake rejection");
        assert_eq!(outcome.state, "rejected-at-intake");
        assert_eq!(human.show_count(), 0);
        assert_eq!(human.read_count(), 0);
        let receipt = String::from_utf8(
            fs::read(forged_evaluator.store_root.join("intake/events.jsonl")).unwrap(),
        )
        .unwrap();
        assert_intake_v2_rejected_commit(
            "forged_evaluator",
            "proposal.evaluator_hash_mismatch",
            before,
            snapshot(&forged_evaluator.store_root),
            &receipt,
            &forged_evaluator,
        );
        families.push("forged_evaluator");

        let judgment = ProvisionedP3Fixture::empty("matrix-intake-v2-judgment");
        let judgment_candidate = intake_v2_candidate(
            &judgment,
            "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
            |bytes| {
                String::from_utf8(bytes)
                    .unwrap()
                    .replacen("type: concept", "type: judgment", 1)
                    .replacen(
                        "context_kind: Research Brief",
                        "context_kind: Human Judgment",
                        1,
                    )
                    .into_bytes()
            },
        );
        let (before, after, preview) = intake_v2_rejected_seal(
            &judgment,
            intake_v2_submission(
                &judgment,
                &judgment_candidate,
                "src:agent:matrix-private-source",
                "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                None,
            ),
            "2026-07-22T04:21:00.000000000Z",
            "2026-07-22T04:21:01.000000000Z",
            &["proposal.judgment_forbidden"],
        );
        assert_intake_v2_rejected_commit(
            "forged_judgment",
            "proposal.judgment_forbidden",
            before,
            after,
            &preview,
            &judgment,
        );
        families.push("forged_judgment");

        let duplicate_submission =
            ProvisionedP3Fixture::empty("matrix-intake-v2-duplicate-submission");
        let duplicate_candidate = intake_v2_candidate(
            &duplicate_submission,
            "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
            |bytes| bytes,
        );
        let submit_time = "2026-07-22T04:22:00.000000000Z";
        let first = submit_proposal_with_clock(
            intake_v2_submission(
                &duplicate_submission,
                &duplicate_candidate,
                "src:agent:matrix-private-source",
                "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                None,
            ),
            &intake_v2_clock(submit_time),
        )
        .unwrap();
        assert_eq!(first.state, "submitted");
        let before = snapshot(&duplicate_submission.store_root);
        let error = submit_proposal_with_clock(
            intake_v2_submission(
                &duplicate_submission,
                &duplicate_candidate,
                "src:agent:matrix-private-source",
                "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                None,
            ),
            &intake_v2_clock(submit_time),
        )
        .expect_err("duplicate submission IDs must fail at the public transport boundary");
        let after = snapshot(&duplicate_submission.store_root);
        assert_eq!(error.code(), "proposal.duplicate_id");
        assert_ne!(
            after.root_hash, before.root_hash,
            "loser temporary custody is retained"
        );
        assert_eq!(
            after.ledger_hash, before.ledger_hash,
            "transport cannot mutate verdict authority"
        );
        assert_intake_v2_private(error.code(), &duplicate_submission);
        assert!(duplicate_submission
            .store_root
            .join("staging/inbox")
            .join(&first.submission_id)
            .is_dir());
        assert!(duplicate_submission
            .store_root
            .join("staging/inbox")
            .join(format!(".submission-{}", first.submission_id))
            .is_dir());
        families.push("duplicate_submission");

        let duplicate_proposal =
            ProvisionedP3Fixture::with_staged_reject("matrix-intake-v2-duplicate-proposal");
        let profile = load_p3_profile(&duplicate_proposal.profile_path).unwrap();
        let store = inspect_store(&profile, &duplicate_proposal.store_root).unwrap();
        let event_bytes = fs::read(&store.paths.intake_events).unwrap();
        let mut duplicate_bytes = event_bytes.clone();
        duplicate_bytes.extend_from_slice(&event_bytes);
        fs::write(&store.paths.intake_events, duplicate_bytes).unwrap();
        let before = snapshot(&duplicate_proposal.store_root);
        let error = read_intake(&store.paths).expect_err("duplicate proposal event must fail");
        assert_intake_v2_immutable_failure(
            "duplicate_proposal",
            "intake.chain_invalid",
            error.code(),
            before,
            snapshot(&duplicate_proposal.store_root),
            &duplicate_proposal,
        );
        families.push("duplicate_proposal");

        let duplicate_content = ProvisionedP3Fixture::empty("matrix-intake-v2-duplicate-content");
        let duplicate_content_candidate = intake_v2_candidate(
            &duplicate_content,
            "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
            |bytes| bytes,
        );
        intake_v2_staged_seal(
            &duplicate_content,
            intake_v2_submission(
                &duplicate_content,
                &duplicate_content_candidate,
                "src:agent:first-private-source",
                "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                None,
            ),
            "2026-07-22T04:23:00.000000000Z",
            "2026-07-22T04:23:01.000000000Z",
        );
        let (before, after, preview) = intake_v2_rejected_seal(
            &duplicate_content,
            intake_v2_submission(
                &duplicate_content,
                &duplicate_content_candidate,
                "src:agent:second-private-source",
                "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                None,
            ),
            "2026-07-22T04:23:02.000000000Z",
            "2026-07-22T04:23:03.000000000Z",
            &["proposal.duplicate_content"],
        );
        assert_intake_v2_rejected_commit(
            "duplicate_target_candidate",
            "proposal.duplicate_content",
            before,
            after,
            &preview,
            &duplicate_content,
        );
        families.push("duplicate_target_candidate");

        let unsafe_path = ProvisionedP3Fixture::empty("matrix-intake-v2-unsafe-path");
        let unsafe_candidate = intake_v2_candidate(
            &unsafe_path,
            "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
            |bytes| bytes,
        );
        let before = snapshot(&unsafe_path.store_root);
        let error = submit_proposal_with_clock(
            intake_v2_submission(
                &unsafe_path,
                &unsafe_candidate,
                "src:agent:matrix-private-source",
                "../MATRIX_PRIVATE_TARGET_9305.md",
                ProposalOperation::Create,
                ProposalIntent::Accept,
                None,
            ),
            &intake_v2_clock("2026-07-22T04:24:00.000000000Z"),
        )
        .expect_err("unsafe target path must fail before publication");
        assert_intake_v2_immutable_failure(
            "unsafe_path",
            "proposal.path_unsafe",
            error.code(),
            before,
            snapshot(&unsafe_path.store_root),
            &unsafe_path,
        );
        families.push("unsafe_path");

        let provenance = ProvisionedP3Fixture::empty("matrix-intake-v2-provenance");
        let provenance_candidate = intake_v2_candidate(
            &provenance,
            "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
            |bytes| {
                String::from_utf8(bytes)
                    .unwrap()
                    .replacen("provenance: synthesis\n", "", 1)
                    .into_bytes()
            },
        );
        let (before, after, preview) = intake_v2_rejected_seal(
            &provenance,
            intake_v2_submission(
                &provenance,
                &provenance_candidate,
                "src:agent:matrix-private-source",
                "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                None,
            ),
            "2026-07-22T04:25:00.000000000Z",
            "2026-07-22T04:25:01.000000000Z",
            &["proposal.provenance_required"],
        );
        assert_intake_v2_rejected_commit(
            "missing_provenance",
            "proposal.provenance_required",
            before,
            after,
            &preview,
            &provenance,
        );
        families.push("missing_provenance");

        let synthesis = ProvisionedP3Fixture::empty("matrix-intake-v2-synthesis");
        let synthesis_candidate =
            intake_v2_candidate(&synthesis, "01-Soul/Audy Himura.md", |bytes| bytes);
        let (before, after, preview) = intake_v2_rejected_seal(
            &synthesis,
            intake_v2_submission(
                &synthesis,
                &synthesis_candidate,
                "src:agent:matrix-private-source",
                "01-Soul/Audy Himura.md",
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                None,
            ),
            "2026-07-22T04:26:00.000000000Z",
            "2026-07-22T04:26:01.000000000Z",
            &["proposal.synthesis_rewrite_required"],
        );
        assert_intake_v2_rejected_commit(
            "protected_synthesis",
            "proposal.synthesis_rewrite_required",
            before,
            after,
            &preview,
            &synthesis,
        );
        families.push("protected_synthesis");

        let candidate_limit = ProvisionedP3Fixture::empty("matrix-intake-v2-candidate-limit");
        let candidate_limit_valid = intake_v2_candidate(
            &candidate_limit,
            "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
            |bytes| bytes,
        );
        let oversized = candidate_limit
            .profile_path
            .parent()
            .unwrap()
            .join("MATRIX_PRIVATE_SAFE_CLONE_ROOT_9309-MATRIX_PRIVATE_PATH_9306.md");
        fs::write(&oversized, vec![b'x'; 262_145]).unwrap();
        let before = snapshot(&candidate_limit.store_root);
        let error = submit_proposal_with_clock(
            intake_v2_submission(
                &candidate_limit,
                &oversized,
                "src:agent:matrix-private-source",
                "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                None,
            ),
            &intake_v2_clock("2026-07-22T04:27:00.000000000Z"),
        )
        .expect_err("candidate limit must fail before publication");
        assert_intake_v2_immutable_failure(
            "candidate_limit",
            "proposal.read",
            error.code(),
            before,
            snapshot(&candidate_limit.store_root),
            &candidate_limit,
        );
        let oversized_evaluator = candidate_limit
            .profile_path
            .parent()
            .unwrap()
            .join("MATRIX_PRIVATE_ADVICE_9303-oversized-evaluator.json");
        fs::write(&oversized_evaluator, vec![b'x'; 65_537]).unwrap();
        let before = snapshot(&candidate_limit.store_root);
        let error = submit_proposal_with_clock(
            intake_v2_submission(
                &candidate_limit,
                &candidate_limit_valid,
                "src:agent:matrix-private-source",
                "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                Some(oversized_evaluator),
            ),
            &intake_v2_clock("2026-07-22T04:27:01.000000000Z"),
        )
        .expect_err("evaluator metadata limit must fail before publication");
        assert_intake_v2_immutable_failure(
            "candidate_limit",
            "proposal.read",
            error.code(),
            before,
            snapshot(&candidate_limit.store_root),
            &candidate_limit,
        );
        families.push("candidate_limit");

        let malformed_receipt = ProvisionedP3Fixture::empty("matrix-intake-v2-malformed-receipt");
        let malformed_candidate = intake_v2_candidate(
            &malformed_receipt,
            "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
            |bytes| {
                String::from_utf8(bytes)
                    .unwrap()
                    .replacen("provenance: synthesis\n", "", 1)
                    .into_bytes()
            },
        );
        intake_v2_rejected_seal(
            &malformed_receipt,
            intake_v2_submission(
                &malformed_receipt,
                &malformed_candidate,
                "src:agent:matrix-private-source",
                "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                ProposalOperation::Replace,
                ProposalIntent::Accept,
                None,
            ),
            "2026-07-22T04:28:00.000000000Z",
            "2026-07-22T04:28:01.000000000Z",
            &["proposal.provenance_required"],
        );
        let profile = load_p3_profile(&malformed_receipt.profile_path).unwrap();
        let store = inspect_store(&profile, &malformed_receipt.store_root).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(
            fs::read(&store.paths.intake_events)
                .unwrap()
                .strip_suffix(b"\n")
                .unwrap(),
        )
        .unwrap();
        value["target_id"] = serde_json::json!("MATRIX_PRIVATE_TARGET_9305");
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        fs::write(&store.paths.intake_events, bytes).unwrap();
        let before = snapshot(&malformed_receipt.store_root);
        let error = read_intake(&store.paths).expect_err("malformed rejection receipt must fail");
        assert_intake_v2_immutable_failure(
            "malformed_receipt",
            "intake.record_invalid",
            error.code(),
            before,
            snapshot(&malformed_receipt.store_root),
            &malformed_receipt,
        );
        families.push("malformed_receipt");

        let quota = ProvisionedP3Fixture::with_staged_reject("matrix-intake-v2-quota");
        let profile = load_p3_profile(&quota.profile_path).unwrap();
        let store = inspect_store(&profile, &quota.store_root).unwrap();
        let pair = read_sealed_pair(&store.paths, &quota.proposal_id).unwrap();
        let retained = (0..store.policy.policy.max_open_per_source)
            .map(|index| ProposalSummary {
                proposal_id: format!("prp-open-{index}"),
                state: "staged".to_owned(),
                submitted_at: "2026-07-22T04:00:00.000000000Z".to_owned(),
                source_id: pair.meta.source_id.clone(),
                target_id: format!("ssa-0/MATRIX_PRIVATE_TARGET_9305-{index}"),
                target_path: format!("04-Research/MATRIX_PRIVATE_TARGET_9305-{index}.md"),
                candidate_hash: format!("{index:064x}"),
            })
            .collect();
        let intake = IntakePrefix {
            head: IntakeHead {
                sequence: 0,
                event_hash: None,
            },
            staged: retained,
            rejected: Vec::new(),
        };
        let before = snapshot(&quota.store_root);
        let decision = PolicyEvaluator::evaluate(&PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &pair,
            intake: &intake,
        });
        assert_eq!(decision.rule_codes, ["proposal.quota_source_open"]);
        assert_intake_v2_immutable_failure(
            "quota_breach",
            "proposal.quota_source_open",
            &decision.rule_codes[0],
            before,
            snapshot(&quota.store_root),
            &quota,
        );
        families.push("quota_breach");

        let authoritative =
            ProvisionedP3Fixture::with_staged_reject("matrix-intake-v2-authority-tamper");
        authoritative.commit_exact_reject(&authoritative.request(b"MATRIX_PRIVATE_REASON_9302"));
        let profile = load_p3_profile(&authoritative.profile_path).unwrap();

        let forged_highwater_store = clone_matrix_store(
            &authoritative.store_root,
            "matrix-intake-v2-forged-highwater",
        );
        replace_applied_event(&forged_highwater_store, |payload| {
            payload.intake_sequence = 0;
            payload.intake_head_hash = None;
        });
        let before = snapshot(&forged_highwater_store);
        let error = verify_store_level4(&profile, &forged_highwater_store)
            .expect_err("forged intake high-water must fail Level 4");
        assert_intake_v2_store_failure(
            "forged_highwater",
            "ledger.intake_binding",
            error.code(),
            before,
            snapshot(&forged_highwater_store),
            &forged_highwater_store,
        );
        families.push("forged_highwater");

        let ledger_store =
            clone_matrix_store(&authoritative.store_root, "matrix-intake-v2-ledger-tamper");
        corrupt_ledger_event_hash(&ledger_store);
        let before = snapshot(&ledger_store);
        let error = verify_store_level4(&profile, &ledger_store)
            .expect_err("tampered verdict ledger must fail Level 4");
        assert_intake_v2_store_failure(
            "ledger_tamper",
            "ledger.event_hash",
            error.code(),
            before,
            snapshot(&ledger_store),
            &ledger_store,
        );
        families.push("ledger_tamper");

        let partial_journal = ProvisionedP3Fixture::empty("matrix-intake-v2-partial-journal");
        let transaction = partial_journal
            .store_root
            .join("intake/transactions")
            .join(format!("sub-{}", "a".repeat(64)));
        fs::create_dir(&transaction).unwrap();
        fs::write(
            transaction.join("prepared.json"),
            b"MATRIX_PRIVATE_TEMP_ROOT_9308\n",
        )
        .unwrap();
        let before = snapshot(&partial_journal.store_root);
        let profile = load_p3_profile(&partial_journal.profile_path).unwrap();
        let error = inspect_store(&profile, &partial_journal.store_root)
            .expect_err("partial intake journal must fail closed");
        assert_intake_v2_immutable_failure(
            "partial_journal",
            "transaction.incomplete",
            error.code(),
            before,
            snapshot(&partial_journal.store_root),
            &partial_journal,
        );
        families.push("partial_journal");

        families
    }

    fn intake_v2_clock(value: &str) -> FixedClock {
        FixedClock(Timestamp::parse(value).expect("valid Task 9 matrix timestamp"))
    }

    fn intake_v2_submission(
        fixture: &ProvisionedP3Fixture,
        candidate: &Path,
        source_id: &str,
        target_path: &str,
        operation: ProposalOperation,
        intent: ProposalIntent,
        evaluator_path: Option<std::path::PathBuf>,
    ) -> ProposalSubmission {
        let target_id = sovmem::profile::p3_canonical_document_id("ssa-0", target_path)
            .unwrap_or_else(|| format!("ssa-0/{target_path}"));
        ProposalSubmission {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            source_id: source_id.to_owned(),
            operation,
            intent,
            target_id,
            target_path: target_path.to_owned(),
            candidate_path: candidate.to_owned(),
            confidence: None,
            evaluator_path,
        }
    }

    fn intake_v2_candidate(
        fixture: &ProvisionedP3Fixture,
        target_path: &str,
        mutate: impl FnOnce(Vec<u8>) -> Vec<u8>,
    ) -> std::path::PathBuf {
        let bytes = fs::read(fixture.store_root.join("seed/bundle").join(target_path)).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let target_id = sovmem::profile::p3_canonical_document_id("ssa-0", target_path).unwrap();
        let mut normalized = text
            .lines()
            .map(|line| {
                if line.starts_with("id: ") {
                    format!("id: {target_id}")
                } else if line.starts_with("source: ") {
                    format!("source: vault://{target_id}")
                } else if line.starts_with("source_path: ") {
                    format!("source_path: {target_path}")
                } else if line.starts_with("resource: ") {
                    format!("resource: vault://{target_id}")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        normalized.push_str(
            "\nMATRIX_PRIVATE_CANDIDATE_9301\nMATRIX_PRIVATE_REASON_9302\n\
             MATRIX_PRIVATE_ADVICE_9303\nMATRIX_PRIVATE_SOURCE_9304\n\
             MATRIX_PRIVATE_TARGET_9305\nMATRIX_PRIVATE_OS_IDENTITY_9307\n\
             MATRIX_PRIVATE_TEMP_ROOT_9308\nMATRIX_PRIVATE_SAFE_CLONE_ROOT_9309\n",
        );
        let path = fixture.profile_path.parent().unwrap().join(
            "MATRIX_PRIVATE_SAFE_CLONE_ROOT_9309-MATRIX_PRIVATE_TEMP_ROOT_9308-MATRIX_PRIVATE_PATH_9306.md",
        );
        fs::write(&path, mutate(normalized.into_bytes())).unwrap();
        path
    }

    fn intake_v2_staged_seal(
        fixture: &ProvisionedP3Fixture,
        submission: ProposalSubmission,
        submit_time: &str,
        seal_time: &str,
    ) {
        let submitted =
            submit_proposal_with_clock(submission, &intake_v2_clock(submit_time)).unwrap();
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let outcome = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &intake_v2_clock(seal_time),
        )
        .unwrap();
        assert_eq!(
            outcome.state,
            "staged",
            "unexpected policy rejection: {}",
            String::from_utf8_lossy(human.displayed())
        );
        assert_intake_v2_private(&String::from_utf8_lossy(human.displayed()), fixture);
    }

    fn intake_v2_rejected_seal(
        fixture: &ProvisionedP3Fixture,
        submission: ProposalSubmission,
        submit_time: &str,
        seal_time: &str,
        expected_codes: &[&str],
    ) -> (DurableSnapshot, DurableSnapshot, String) {
        let submitted =
            submit_proposal_with_clock(submission, &intake_v2_clock(submit_time)).unwrap();
        let before = snapshot(&fixture.store_root);
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let outcome = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &intake_v2_clock(seal_time),
        )
        .unwrap();
        assert_eq!(outcome.state, "rejected-at-intake");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = inspect_store(&profile, &fixture.store_root).unwrap();
        let intake = read_intake(&store.paths).unwrap();
        assert_eq!(
            intake.events().last().unwrap().payload().rule_codes,
            expected_codes
        );
        (
            before,
            snapshot(&fixture.store_root),
            String::from_utf8(human.displayed().to_vec()).unwrap(),
        )
    }

    fn assert_intake_v2_immutable_failure(
        family: &str,
        expected: &str,
        actual: &str,
        before: DurableSnapshot,
        after: DurableSnapshot,
        fixture: &ProvisionedP3Fixture,
    ) {
        assert_eq!(actual, expected, "{family}");
        assert_eq!(after, before, "{family} mutated durable state");
        assert_intake_v2_private(actual, fixture);
    }

    fn assert_intake_v2_rejected_commit(
        family: &str,
        expected: &str,
        before: DurableSnapshot,
        after: DurableSnapshot,
        preview: &str,
        fixture: &ProvisionedP3Fixture,
    ) {
        assert_ne!(
            after.root_hash, before.root_hash,
            "{family} omitted its rejection record"
        );
        assert_eq!(
            after.ledger_hash, before.ledger_hash,
            "{family} mutated verdict authority"
        );
        assert_intake_v2_private(expected, fixture);
        assert_intake_v2_private(preview, fixture);
    }

    fn assert_intake_v2_store_failure(
        family: &str,
        expected: &str,
        actual: &str,
        before: DurableSnapshot,
        after: DurableSnapshot,
        store_root: &Path,
    ) {
        assert_eq!(actual, expected, "{family}");
        assert_eq!(after, before, "{family} repaired poisoned durable state");
        assert_intake_v2_private_value(actual, store_root);
    }

    fn assert_intake_v2_private(value: &str, fixture: &ProvisionedP3Fixture) {
        assert_intake_v2_private_value(value, &fixture.store_root);
        assert!(!value.contains(fixture.profile_path.to_string_lossy().as_ref()));
    }

    fn assert_intake_v2_private_value(value: &str, store_root: &Path) {
        for sentinel in [
            "MATRIX_PRIVATE_CANDIDATE_9301",
            "MATRIX_PRIVATE_REASON_9302",
            "MATRIX_PRIVATE_ADVICE_9303",
            "MATRIX_PRIVATE_SOURCE_9304",
            "MATRIX_PRIVATE_TARGET_9305",
            "MATRIX_PRIVATE_PATH_9306",
            "MATRIX_PRIVATE_OS_IDENTITY_9307",
            "MATRIX_PRIVATE_TEMP_ROOT_9308",
            "MATRIX_PRIVATE_SAFE_CLONE_ROOT_9309",
            store_root.to_string_lossy().as_ref(),
        ] {
            assert!(
                !value.contains(sentinel),
                "Task 9 diagnostic leaked {sentinel}"
            );
        }
        if let Ok(user) = std::env::var("USER") {
            assert_intake_v2_os_identity(value, &user);
        }
        assert!(!value.contains(std::env::temp_dir().to_string_lossy().as_ref()));
    }

    // Only the role in a valid top-level receipt source_id is public schema.
    // Keep all other raw bytes, including unrelated fields and malformed IDs.
    fn assert_intake_v2_os_identity(value: &str, user: &str) {
        let mut without_public_role = value.to_owned();
        if let Ok(receipt) = serde_json::from_str::<serde_json::Value>(value) {
            if let Some(id) = receipt.get("source_id").and_then(|id| id.as_str()) {
                if let Some(name) = id.strip_prefix("src:agent:") {
                    let bytes = name.as_bytes();
                    let valid_name = (1..=64).contains(&bytes.len())
                        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
                        && bytes[1..].iter().all(|byte| {
                            byte.is_ascii_lowercase()
                                || byte.is_ascii_digit()
                                || matches!(byte, b'.' | b'_' | b'-')
                        });
                    let field = format!("\"source_id\":{}", serde_json::to_string(id).unwrap());
                    if valid_name && value.matches(&field).count() == 1 {
                        let suffix =
                            format!("\"source_id\":{}", serde_json::to_string(name).unwrap());
                        without_public_role = value.replacen(&field, &suffix, 1);
                    }
                }
            }
        }
        assert!(
            !without_public_role.contains(user),
            "Task 9 diagnostic leaked OS username"
        );
    }

    #[test]
    fn os_identity_redaction_allows_the_public_agent_source_kind() {
        assert_intake_v2_os_identity(
            r#"{"source_id":"src:agent:matrix-private-source"}"#,
            "agent",
        );
    }

    #[test]
    #[should_panic(expected = "Task 9 diagnostic leaked OS username")]
    fn os_identity_redaction_still_rejects_identity_in_a_source_id() {
        assert_intake_v2_os_identity(r#"{"source_id":"src:agent:agent"}"#, "agent");
    }

    #[test]
    #[should_panic(expected = "Task 9 diagnostic leaked OS username")]
    fn os_identity_redaction_still_rejects_identity_in_other_data() {
        assert_intake_v2_os_identity(r#"{"message":"private user agent"}"#, "agent");
    }

    #[test]
    #[should_panic(expected = "Task 9 diagnostic leaked OS username")]
    fn os_identity_redaction_does_not_exempt_markers_in_unrelated_fields() {
        assert_intake_v2_os_identity(r#"{"message":"src:agent:"}"#, "agent");
    }

    #[test]
    #[should_panic(expected = "Task 9 diagnostic leaked OS username")]
    fn os_identity_redaction_does_not_exempt_malformed_source_ids() {
        assert_intake_v2_os_identity(r#"{"source_id":"src:agent:src:agent:"}"#, "agent");
    }

    #[test]
    fn intake_v2_poisoning_matrix_executes_every_required_attack_family() {
        assert_eq!(
            intake_v2_attack_families(),
            [
                "forged_source",
                "forged_evaluator",
                "forged_judgment",
                "duplicate_submission",
                "duplicate_proposal",
                "duplicate_target_candidate",
                "unsafe_path",
                "missing_provenance",
                "protected_synthesis",
                "candidate_limit",
                "malformed_receipt",
                "quota_breach",
                "forged_highwater",
                "ledger_tamper",
                "partial_journal",
            ]
        );
    }

    #[test]
    fn poisoning_and_policy_matrix_preserves_first_failure_durable_state_and_redaction() {
        // Each row names only the direct corruption or rejection that it executes. The named owner
        // supplies deeper permutation coverage without widening this matrix row's claim.
        let cases = [
            MatrixCase {
                family: "malformed_profile_bytes",
                first_stage: "profile.parse",
                expected: ExpectedOutcome::StableCode("profile.parse"),
                cli_exit: None,
                root_change: RootChange::None,
                redaction_sentinel: None,
                run: malformed_profile_row,
            },
            MatrixCase {
                family: "malformed_policy_bytes",
                first_stage: "policy.utf8",
                expected: ExpectedOutcome::StableCode("policy.utf8"),
                cli_exit: None,
                root_change: RootChange::None,
                redaction_sentinel: None,
                run: malformed_policy_row,
            },
            MatrixCase {
                family: "malformed_proposal_meta_precedes_later_rules",
                first_stage: "proposal.meta_schema",
                expected: ExpectedOutcome::StableCode("proposal.meta_schema"),
                cli_exit: None,
                root_change: RootChange::None,
                redaction_sentinel: None,
                run: pair_precedence_row,
            },
            MatrixCase {
                family: "malformed_evaluator_json",
                first_stage: "proposal.meta_parse",
                expected: ExpectedOutcome::StableCode("proposal.meta_parse"),
                cli_exit: None,
                root_change: RootChange::None,
                redaction_sentinel: None,
                run: malformed_evaluator_row,
            },
            MatrixCase {
                family: "symlinked_store_entry",
                first_stage: "store.symlink",
                expected: SYMLINK_EXPECTED,
                cli_exit: None,
                root_change: RootChange::None,
                redaction_sentinel: None,
                run: symlink_row,
            },
            MatrixCase {
                family: "missing_provenance_retains_only_redacted_intake_receipt",
                first_stage: "proposal.provenance_required",
                expected: ExpectedOutcome::StableCode("proposal.provenance_required"),
                cli_exit: None,
                root_change: RootChange::RedactedIntakeReceipt,
                redaction_sentinel: Some("MATRIX_PRIVATE_CANDIDATE_SENTINEL"),
                run: provenance_row,
            },
            MatrixCase {
                family: "confidence_extra_precision",
                first_stage: "proposal.confidence_invalid",
                expected: ExpectedOutcome::StableCode("proposal.confidence_invalid"),
                cli_exit: None,
                root_change: RootChange::None,
                redaction_sentinel: None,
                run: confidence_row,
            },
            MatrixCase {
                family: "forged_evaluator_candidate_binding_retains_redacted_receipt",
                first_stage: "proposal.evaluator_hash_mismatch",
                expected: ExpectedOutcome::StableCode("proposal.evaluator_hash_mismatch"),
                cli_exit: None,
                root_change: RootChange::RedactedIntakeReceipt,
                redaction_sentinel: Some("MATRIX_PRIVATE_EVALUATOR_SENTINEL"),
                run: forged_evaluator_row,
            },
            MatrixCase {
                family: "rewrite_requires_exact_human_bytes",
                first_stage: "human.rewrite_required",
                expected: ExpectedOutcome::StableCode("human.rewrite_required"),
                cli_exit: None,
                root_change: RootChange::None,
                redaction_sentinel: None,
                run: rewrite_required_row,
            },
            MatrixCase {
                family: "level4_stale_current_pointer_binding",
                first_stage: "ledger.current_mismatch",
                expected: ExpectedOutcome::StableCode("ledger.current_mismatch"),
                cli_exit: None,
                root_change: RootChange::None,
                redaction_sentinel: None,
                run: current_pointer_row,
            },
            MatrixCase {
                family: "visible_incomplete_transaction",
                first_stage: "transaction.incomplete",
                expected: ExpectedOutcome::StableCode("transaction.incomplete"),
                cli_exit: None,
                root_change: RootChange::None,
                redaction_sentinel: None,
                run: incomplete_transaction_row,
            },
            MatrixCase {
                family: "visible_protected_verdict_lease",
                first_stage: "store.locked",
                expected: ExpectedOutcome::StableCode("store.locked"),
                cli_exit: None,
                root_change: RootChange::None,
                redaction_sentinel: None,
                run: protected_lease_row,
            },
        ];

        assert!(
            cases
                .iter()
                .all(|case| matches!(case.expected, ExpectedOutcome::StableCode(_))),
            "every poisoning row must execute a production rejection with a stable code; \
             static fixture labels are not executable attack evidence"
        );

        for case in cases {
            let observation = (case.run)();
            match case.expected {
                ExpectedOutcome::StableCode(code) => {
                    assert_eq!(observation.code.as_deref(), Some(code), "{}", case.family);
                }
            }
            assert_eq!(observation.cli_exit, case.cli_exit, "{}", case.family);
            assert_eq!(
                observation.before.ledger_hash, observation.after.ledger_hash,
                "{} mutated the verdict ledger while proving {}",
                case.family, case.first_stage
            );
            match case.root_change {
                RootChange::None => assert_eq!(
                    observation.before.root_hash, observation.after.root_hash,
                    "{} mutated the durable root while proving {}",
                    case.family, case.first_stage
                ),
                RootChange::RedactedIntakeReceipt => assert_ne!(
                    observation.before.root_hash, observation.after.root_hash,
                    "{} must persist its redacted intake receipt",
                    case.family
                ),
            }
            if let Some(sentinel) = case.redaction_sentinel {
                let redacted_input = observation
                    .redacted_input
                    .as_deref()
                    .expect("redaction row supplies its rejected input");
                let projection = observation
                    .redacted_projection
                    .as_deref()
                    .expect("redaction row supplies its retained receipt projection");
                assert!(
                    redacted_input.contains(sentinel),
                    "{} must inject its asserted sentinel",
                    case.family
                );
                assert!(
                    !projection.contains(sentinel),
                    "{} leaked {sentinel}",
                    case.family
                );
            }
        }
    }

    #[test]
    fn task17_last_findings_matrix_has_one_real_route_family_per_required_gap() {
        assert_eq!(
            task17_last_findings_route_families(),
            [
                "race_and_recovery",
                "level4_durable_corruption",
                "concrete_schema_poisoning",
                "special_file_snapshot_kind",
            ]
        );
    }

    fn task17_last_findings_route_families() -> Vec<&'static str> {
        vec![
            "race_and_recovery",
            "level4_durable_corruption",
            "concrete_schema_poisoning",
            "special_file_snapshot_kind",
        ]
    }

    #[test]
    fn matrix_race_rows_use_concurrent_public_routes_and_preserve_redacted_durable_evidence() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-concurrent-propose");
        let before = snapshot(&fixture.store_root);
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for index in 0..2 {
            let barrier = Arc::clone(&barrier);
            let request = matrix_submission(
                &fixture,
                fixture.input_file(
                    &format!("matrix-concurrent-propose-{index}"),
                    format!(
                        "{}\nMATRIX_PRIVATE_CONCURRENT_PROPOSER_{index}\n",
                        String::from_utf8(fs::read(fixture.candidate_arg()).unwrap()).unwrap()
                    )
                    .as_bytes(),
                ),
                None,
            );
            handles.push(thread::spawn(move || {
                barrier.wait();
                submit_proposal(request)
                    .map(|_| "ok".to_owned())
                    .unwrap_or_else(|error| error.code().to_owned())
            }));
        }
        barrier.wait();
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().expect("concurrent proposer thread"))
            .collect::<Vec<_>>();
        assert!(outcomes.iter().all(|outcome| {
            matches!(
                outcome.as_str(),
                "ok" | "proposal.locked" | "proposal.rejected"
            )
        }));
        let after = snapshot(&fixture.store_root);
        assert!(outcomes.iter().any(|outcome| outcome == "ok"));
        assert_ne!(before.root_hash, after.root_hash);
        assert_eq!(before.ledger_hash, after.ledger_hash);
        assert_no_partial_submission_or_protected_pairs(&fixture.store_root, "concurrent_proposer");
        assert_no_diagnostic_sentinel(&outcomes.join("\n"), "MATRIX_PRIVATE_CONCURRENT_PROPOSER");

        let verdict = ProvisionedP3Fixture::with_staged_reject("matrix-concurrent-verdict");
        let protected_lease = fs::canonicalize(&verdict.store_root)
            .expect("canonicalize concurrent verdict store")
            .join("locks/verdict.lock");
        let publication = RecoveryLeasePublicationBarrier::after_sync_of(protected_lease);
        let winner_runtime = verdict.runtime_with_ops(publication.clone());
        let winner_request = verdict
            .request_for(
                VerdictVerb::Reject,
                "MATRIX_PRIVATE_CONCURRENT_VERDICT_WINNER",
            )
            .expect("construct concurrent winner request");
        let winner_profile = verdict.profile_path.clone();
        let winner_store = verdict.store_root.clone();
        let winner = thread::spawn(move || {
            let confirmation = format!("APPLY REJECT {}\n", &winner_request.proposal_id[..12]);
            let mut human = ScriptedHuman::new(Some(confirmation));
            match preflight_human_verdict_from_store_with_human(
                &winner_profile,
                &winner_store,
                winner_request,
                &winner_runtime,
                &mut human,
                &FixedClock(Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap()),
            )
            .expect("winner retains the published verdict lease")
            {
                PreflightVerdict::Confirmed(decision) => {
                    apply_human_verdict(decision, &winner_runtime)
                }
                PreflightVerdict::Idempotent(_) => panic!("fresh concurrent winner cannot replay"),
            }
        });
        publication.wait_until_published();

        // The loser must observe the real, fsynced lease before it may read the proposal or prompt.
        // Its durable snapshot and stable code prove that the protected authority boundary fails
        // closed without exposing either private human reason.
        let before_loser = snapshot(&verdict.store_root);
        let loser_request = verdict
            .request_for(
                VerdictVerb::Reject,
                "MATRIX_PRIVATE_CONCURRENT_VERDICT_LOSER",
            )
            .expect("construct concurrent loser request");
        let mut loser_human = ScriptedHuman::new(None);
        let loser = match preflight_human_verdict_from_store_with_human(
            &verdict.profile_path,
            &verdict.store_root,
            loser_request,
            &verdict.runtime(),
            &mut loser_human,
            &FixedClock(Timestamp::parse("2026-07-14T04:30:01.000000000Z").unwrap()),
        ) {
            Err(error) => error,
            Ok(_) => panic!("concurrent loser must fail before proposal read or prompt"),
        };
        assert_eq!(loser.code(), "store.locked");
        assert_eq!(snapshot(&verdict.store_root), before_loser);
        assert_eq!(loser_human.show_count(), 0);
        assert_eq!(loser_human.read_count(), 0);
        assert_no_diagnostic_sentinel(loser.code(), "MATRIX_PRIVATE_CONCURRENT_VERDICT_LOSER");

        publication.release();
        let winner = winner
            .join()
            .expect("concurrent verdict winner thread")
            .expect("released winner must apply the real verdict");
        assert!(!winner.idempotent);
        let after = snapshot(&verdict.store_root);
        assert_ne!(before_loser.root_hash, after.root_hash);
        assert_ne!(before_loser.ledger_hash, after.ledger_hash);
        assert_eq!(verdict.verdict_count(VerdictVerb::Reject), 1);

        let recovery = Arc::new(ProvisionedP3Fixture::with_staged_reject(
            "matrix-concurrent-recovery",
        ));
        let interrupted =
            recovery.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
        let interrupted_error = recovery
            .apply_with_runtime(
                VerdictVerb::Reject,
                "MATRIX_PRIVATE_CONCURRENT_RECOVERY",
                &interrupted,
            )
            .expect_err("prepare durable interrupted recovery state");
        assert_eq!(interrupted_error.code(), "transaction.prepare_failed");
        let before = snapshot(&recovery.store_root);
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let fixture = Arc::clone(&recovery);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                let runtime = fixture.runtime();
                fixture
                    .recover_with_runtime(&runtime)
                    .map(|_| "ok".to_owned())
                    .unwrap_or_else(|error| error.code().to_owned())
            }));
        }
        barrier.wait();
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().expect("concurrent recovery thread"))
            .collect::<Vec<_>>();
        assert!(outcomes.iter().any(|outcome| outcome == "ok"));
        assert!(outcomes.iter().all(|outcome| {
            matches!(
                outcome.as_str(),
                "ok" | "store.locked" | "transaction.recovery_ambiguous" | "transaction.incomplete"
            )
        }));
        let after = snapshot(&recovery.store_root);
        assert_ne!(before.root_hash, after.root_hash);
        assert_no_diagnostic_sentinel(&outcomes.join("\n"), "MATRIX_PRIVATE_CONCURRENT_RECOVERY");
    }

    #[test]
    fn matrix_stale_intake_lease_and_ambiguous_recovery_rows_execute_the_real_fail_closed_routes() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-stale-intake-lease");
        let submitted = submit_proposal(matrix_submission(&fixture, fixture.candidate_arg(), None))
            .expect("public submit writes only the untrusted inbox request");
        let paths = StorePaths::from_root(fs::canonicalize(&fixture.store_root).unwrap());
        let held = IntakePublicationLease::acquire(
            &paths,
            IntakeLeaseIdentity::new(
                "MATRIX_PRIVATE_STALE_INTAKE",
                "intake-seal",
                &Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap(),
                4242,
            )
            .unwrap(),
        )
        .expect("materialize the protected v2 intake lease");
        let before = snapshot(&fixture.store_root);
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let error = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-14T04:30:01.000000000Z").unwrap()),
        )
        .expect_err("a held protected intake lease blocks human seal");
        assert_eq!(error.code(), "intake.locked");
        assert_eq!(before, snapshot(&fixture.store_root));
        assert_no_diagnostic_sentinel(error.code(), "MATRIX_PRIVATE_STALE_INTAKE");
        held.release(&paths).unwrap();

        let fixture = ProvisionedP3Fixture::with_staged_reject("matrix-ambiguous-recovery");
        let faulted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
        fixture
            .apply_with_runtime(
                VerdictVerb::Reject,
                "MATRIX_PRIVATE_AMBIGUOUS_RECOVERY",
                &faulted,
            )
            .expect_err("prepare interrupted evidence");
        let journal = only_transaction_journal(&fixture.store_root);
        fs::write(&journal, b"MATRIX_PRIVATE_AMBIGUOUS_RECOVERY\n")
            .expect("corrupt recovery journal");
        let before = snapshot(&fixture.store_root);
        let runtime = fixture.runtime();
        let error = fixture
            .recover_with_runtime(&runtime)
            .expect_err("ambiguous recovery must fail closed");
        assert_eq!(error.code(), "transaction.recovery_ambiguous");
        assert_eq!(before, snapshot(&fixture.store_root));
        assert_no_diagnostic_sentinel(error.code(), "MATRIX_PRIVATE_AMBIGUOUS_RECOVERY");
    }

    #[test]
    fn matrix_level4_rows_directly_reject_target_candidate_rewrite_index_and_policy_history_tampering(
    ) {
        type StoreCorruption = fn(&Path);
        let rows: [(&str, StoreCorruption, &str, &str); 4] = [
            (
                "target",
                corrupt_canonical_target,
                "ledger.generation_missing",
                "MATRIX_PRIVATE_TARGET",
            ),
            (
                "candidate",
                corrupt_retained_candidate,
                "transaction.incomplete",
                "MATRIX_PRIVATE_CANDIDATE",
            ),
            (
                "derived_index",
                corrupt_derived_index,
                "store.layout_invalid",
                "MATRIX_PRIVATE_INDEX",
            ),
            (
                "policy_history",
                corrupt_policy_archive,
                "ledger.proposal_binding",
                "MATRIX_PRIVATE_POLICY",
            ),
        ];
        for (family, corrupt, expected, sentinel) in rows {
            let fixture = ProvisionedP3Fixture::empty(&format!("matrix-level4-direct-{family}"));
            super::full_loop_tests::run_nonapplying_verdict(&fixture, VerdictVerb::Reject);
            super::full_loop_tests::run_complete_loop(&fixture, false);
            let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
            corrupt(&fixture.store_root);
            let before = snapshot(&fixture.store_root);
            let error = verify_store_level4(&profile, &fixture.store_root).expect_err(family);
            assert_eq!(error.code(), expected, "{family}");
            assert_eq!(
                before,
                snapshot(&fixture.store_root),
                "{family} verifier mutated durable corruption"
            );
            assert_no_diagnostic_sentinel(error.code(), sentinel);
        }

        let rewrite = ProvisionedP3Fixture::empty("matrix-level4-direct-rewrite");
        super::full_loop_tests::run_complete_loop(&rewrite, false);
        super::full_loop_tests::run_complete_loop(&rewrite, true);
        let profile = load_p3_profile(&rewrite.profile_path).expect("load rewrite profile");
        corrupt_rewrite_binding(&rewrite.store_root);
        let before = snapshot(&rewrite.store_root);
        let error = verify_store_level4(&profile, &rewrite.store_root)
            .expect_err("rewrite commitment corruption must fail Level 4");
        assert_eq!(error.code(), "ledger.verdict_binding");
        assert_eq!(before, snapshot(&rewrite.store_root));
        assert_no_diagnostic_sentinel(error.code(), "MATRIX_PRIVATE_REWRITE");
    }

    #[test]
    fn matrix_concrete_schema_rows_execute_profile_proposal_evaluator_manifest_receipt_and_journal_variants(
    ) {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-concrete-schema");
        let profile_bytes = fs::read(&fixture.profile_path).expect("read fixture profile");
        let unknown_profile = fixture
            .store_root
            .join("profiles/audy-ssa0-p3-v2/profile.yaml");
        fs::create_dir_all(unknown_profile.parent().unwrap())
            .expect("create unknown profile parent");
        fs::write(
            &unknown_profile,
            [
                profile_bytes.clone(),
                b"matrix_unknown: MATRIX_PRIVATE_PROFILE_UNKNOWN\n".to_vec(),
            ]
            .concat(),
        )
        .expect("write concrete unknown profile poison");
        let before = snapshot(&fixture.store_root);
        let error = load_p3_profile(&unknown_profile)
            .expect_err("unknown P3 profile fields must fail closed before store access");
        assert_eq!(error.code(), "profile.parse");
        assert_eq!(before, snapshot(&fixture.store_root));
        assert_no_diagnostic_sentinel(error.code(), "MATRIX_PRIVATE_PROFILE_UNKNOWN");

        let noncanonical_profile = fixture
            .store_root
            .join("profiles/noncanonical/profile.yaml");
        fs::create_dir_all(noncanonical_profile.parent().unwrap())
            .expect("create noncanonical profile parent");
        fs::write(&noncanonical_profile, profile_bytes).expect("write noncanonical profile copy");
        let before = snapshot(&fixture.store_root);
        let error = load_p3_profile(&noncanonical_profile)
            .expect_err("noncanonical profile version path must fail");
        assert_eq!(error.code(), "profile.identifier");
        assert_eq!(before, snapshot(&fixture.store_root));
        assert_no_diagnostic_sentinel(error.code(), "MATRIX_PRIVATE_PROFILE_PATH");

        let staged = ProvisionedP3Fixture::with_staged_reject("matrix-concrete-proposal-evaluator");
        let proposal_root = staged_proposal_root(&staged.store_root);
        let meta = proposal_root.join("proposal.meta.json");
        let candidate = proposal_root.join("candidate.md");
        let profile = load_p3_profile(&staged.profile_path).expect("load profile");
        let store_snapshot =
            inspect_store(&profile, &staged.store_root).expect("inspect staged store");
        for (family, path, bytes, expected, sentinel) in [
            (
                "proposal_unknown",
                meta.clone(),
                b"{\"unknown\":\"MATRIX_PRIVATE_PROPOSAL_UNKNOWN\"}\n".to_vec(),
                "proposal.meta_parse",
                "MATRIX_PRIVATE_PROPOSAL_UNKNOWN",
            ),
            (
                "proposal_noncanonical",
                meta.clone(),
                [fs::read(&meta).unwrap(), b" \n".to_vec()].concat(),
                "proposal.meta_noncanonical",
                "MATRIX_PRIVATE_PROPOSAL_NONCANONICAL",
            ),
            (
                "candidate_hash_binding",
                candidate.clone(),
                b"MATRIX_PRIVATE_CANDIDATE_UNKNOWN\n".to_vec(),
                "proposal.content_size_mismatch",
                "MATRIX_PRIVATE_CANDIDATE_UNKNOWN",
            ),
        ] {
            let store = staged.store_root.clone();
            let copied = clone_matrix_store(&staged.store_root, family);
            let relative = path.strip_prefix(&store).unwrap();
            let path = copied.join(relative);
            fs::write(&path, bytes).expect("write concrete proposal poison");
            let before = snapshot(&copied);
            let error = sovmem::profile::read_proposal_pair(
                &copied.join("intake/proposals"),
                &staged.proposal_id,
                store_snapshot.policy.policy.max_meta_bytes,
            )
            .expect_err(family);
            assert_eq!(error.code(), expected, "{family}");
            assert_eq!(before, snapshot(&copied), "{family}");
            assert_no_diagnostic_sentinel(error.code(), sentinel);
        }

        let evaluator_fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-concrete-evaluator");
        let candidate_hash = blake3::hash(&fs::read(evaluator_fixture.candidate_arg()).unwrap())
            .to_hex()
            .to_string();
        let canonical = serde_json::to_vec(&serde_json::json!({
            "protocol": "adversarial-bug-finding-v1",
            "source_id": "src:agent:reviewer",
            "evaluated_at": "2026-07-14T04:29:00.000000000Z",
            "candidate_hash": candidate_hash,
            "recommendation": "looks-sound",
            "reasons": ["MATRIX_PRIVATE_EVALUATOR_REASON"]
        }))
        .expect("serialize evaluator base");
        for (family, bytes, sentinel) in [
            (
                "evaluator_unknown",
                unknown_json_field(canonical.clone()),
                "MATRIX_PRIVATE_UNKNOWN",
            ),
            (
                "evaluator_noncanonical",
                noncanonical_json(canonical.clone()),
                "MATRIX_PRIVATE_NONCANONICAL",
            ),
        ] {
            let evaluator = evaluator_fixture.input_file(&format!("matrix-{family}"), &bytes);
            let before = snapshot(&evaluator_fixture.store_root);
            let error = submit_proposal(matrix_submission(
                &evaluator_fixture,
                evaluator_fixture.candidate_arg(),
                Some(evaluator),
            ))
            .expect_err(family);
            assert_eq!(error.code(), "proposal.meta_parse", "{family}");
            assert_eq!(before, snapshot(&evaluator_fixture.store_root), "{family}");
            assert_no_diagnostic_sentinel(error.code(), sentinel);
        }
    }

    #[test]
    fn matrix_manifest_receipt_and_journal_rows_execute_malformed_noncanonical_and_unknown_field_variants(
    ) {
        type BytesMutation = fn(Vec<u8>) -> Vec<u8>;
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-durable-schema-rows");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).expect("load Level 4 profile");
        let manifest = fixture
            .store_root
            .join("canon/generations/gen-0000000000000001/manifest.json");
        let receipt = fs::read_dir(fixture.store_root.join("evidence/reindex"))
            .expect("read receipt evidence")
            .flatten()
            .next()
            .expect("fixture receipt")
            .path();
        for (kind, source, expected) in [
            ("manifest", manifest, "ledger.generation_missing"),
            ("receipt", receipt, "ledger.reindex_receipt_mismatch"),
        ] {
            for (variant, mutate, sentinel) in [
                (
                    "malformed",
                    malformed_json as BytesMutation,
                    "MATRIX_PRIVATE_MALFORMED",
                ),
                (
                    "noncanonical",
                    noncanonical_json as BytesMutation,
                    "MATRIX_PRIVATE_NONCANONICAL",
                ),
                (
                    "unknown_field",
                    unknown_json_field as BytesMutation,
                    "MATRIX_PRIVATE_UNKNOWN",
                ),
            ] {
                let store = fixture.clone_store(&format!("matrix-{kind}-{variant}"));
                let path = store.join(source.strip_prefix(&fixture.store_root).unwrap());
                fs::write(
                    &path,
                    mutate(fs::read(&path).expect("read durable schema bytes")),
                )
                .expect("write durable schema poison");
                let before = snapshot(&store);
                let error = verify_store_level4(&profile, &store).expect_err(kind);
                assert_eq!(error.code(), expected, "{kind}_{variant}");
                assert_eq!(before, snapshot(&store), "{kind}_{variant}");
                assert_no_diagnostic_sentinel(error.code(), sentinel);
            }
        }

        for (variant, mutate, sentinel) in [
            (
                "malformed",
                malformed_json as BytesMutation,
                "MATRIX_PRIVATE_MALFORMED",
            ),
            (
                "noncanonical",
                noncanonical_json as BytesMutation,
                "MATRIX_PRIVATE_NONCANONICAL",
            ),
            (
                "unknown_field",
                unknown_json_field as BytesMutation,
                "MATRIX_PRIVATE_UNKNOWN",
            ),
        ] {
            let fixture =
                ProvisionedP3Fixture::with_staged_reject(&format!("matrix-journal-{variant}"));
            let faulted =
                fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
            fixture
                .apply_with_runtime(VerdictVerb::Reject, "MATRIX_PRIVATE_JOURNAL", &faulted)
                .expect_err("prepare journal schema poison evidence");
            let journal = only_transaction_journal(&fixture.store_root);
            fs::write(
                &journal,
                mutate(fs::read(&journal).expect("read journal bytes")),
            )
            .expect("write journal schema poison");
            let before = snapshot(&fixture.store_root);
            let runtime = fixture.runtime();
            let error = fixture
                .recover_with_runtime(&runtime)
                .expect_err("poisoned journal must fail closed");
            assert_eq!(
                error.code(),
                "transaction.recovery_ambiguous",
                "journal_{variant}"
            );
            assert_eq!(before, snapshot(&fixture.store_root), "journal_{variant}");
            assert_no_diagnostic_sentinel(error.code(), sentinel);
        }
    }

    #[test]
    fn policy_poisoning_rows_execute_noncanonical_unknown_version_and_invariant_attacks() {
        let fixture = ProvisionedP3Fixture::empty("matrix-policy-poisoning");
        let reviewed =
            fs::read(fixture.store_root.join("policy.toml")).expect("read reviewed policy bytes");
        let reviewed_text = String::from_utf8(reviewed).expect("reviewed policy is UTF-8");
        let rows = [
            (
                "policy_raw_utf8",
                vec![0xff, b'\n'],
                "policy.utf8",
                "MATRIX_PRIVATE_POLICY_UTF8",
            ),
            (
                "policy_raw_crlf",
                reviewed_text.replacen('\n', "\r\n", 1).into_bytes(),
                "policy.line_endings",
                "MATRIX_PRIVATE_POLICY_CRLF",
            ),
            (
                "policy_noncanonical_comment",
                reviewed_text
                    .replacen(
                        "schema_version = \"sovmem-policy-v1\"",
                        "schema_version = \"sovmem-policy-v1\" # private",
                        1,
                    )
                    .into_bytes(),
                "policy.syntax",
                "MATRIX_PRIVATE_POLICY_COMMENT",
            ),
            (
                "policy_unknown_field",
                format!("unknown = \"MATRIX_PRIVATE_POLICY_UNKNOWN\"\n{reviewed_text}")
                    .into_bytes(),
                "policy.unknown_field",
                "MATRIX_PRIVATE_POLICY_UNKNOWN",
            ),
            (
                "policy_key_order",
                policy_key_order_attack(&reviewed_text),
                "policy.key_order",
                "MATRIX_PRIVATE_POLICY_ORDER",
            ),
            (
                "policy_duplicate_key",
                policy_duplicate_key_attack(&reviewed_text),
                "policy.duplicate_key",
                "MATRIX_PRIVATE_POLICY_DUPLICATE",
            ),
            (
                "policy_missing_field",
                reviewed_text
                    .replacen("human_verdict_cost = 1\n", "", 1)
                    .into_bytes(),
                "policy.missing_field",
                "MATRIX_PRIVATE_POLICY_MISSING",
            ),
            (
                "policy_unsupported_version",
                reviewed_text
                    .replacen("sovmem-policy-v1", "sovmem-policy-v2", 1)
                    .into_bytes(),
                "policy.schema_unsupported",
                "MATRIX_PRIVATE_POLICY_VERSION",
            ),
            (
                "policy_invalid_target_invariant",
                reviewed_text
                    .replacen("queue_alarm_at = 25", "queue_alarm_at = 51", 1)
                    .into_bytes(),
                "policy.invariant_invalid",
                "MATRIX_PRIVATE_POLICY_INVARIANT",
            ),
        ];

        for (family, bytes, expected, sentinel) in rows {
            let before = snapshot(&fixture.store_root);
            let error = parse_policy(&bytes).expect_err(family);
            assert_eq!(error.code(), expected, "{family}");
            assert!(!error.code().contains(sentinel), "{family} leaked input");
            assert_eq!(
                before,
                snapshot(&fixture.store_root),
                "{family} mutated the store"
            );
        }
    }

    fn policy_key_order_attack(reviewed_text: &str) -> Vec<u8> {
        let schema = reviewed_text
            .lines()
            .find(|line| line.starts_with("schema_version = "))
            .expect("reviewed policy has a schema-version line");
        let store = reviewed_text
            .lines()
            .find(|line| line.starts_with("store_id = "))
            .expect("reviewed policy has a store-id line");
        let needle = format!("{schema}\n{store}");
        let replacement = format!("{store}\n{schema}");
        let attacked = reviewed_text
            .replacen(&needle, &replacement, 1)
            .into_bytes();
        assert_ne!(
            attacked,
            reviewed_text.as_bytes(),
            "policy key-order attack must mutate its reviewed fixture"
        );
        attacked
    }

    fn policy_duplicate_key_attack(reviewed_text: &str) -> Vec<u8> {
        let schema = reviewed_text
            .lines()
            .find(|line| line.starts_with("schema_version = "))
            .expect("reviewed policy has a schema-version line");
        let store = reviewed_text
            .lines()
            .find(|line| line.starts_with("store_id = "))
            .expect("reviewed policy has a store-id line");
        let attacked = reviewed_text.replacen(store, schema, 1).into_bytes();
        assert_ne!(
            attacked,
            reviewed_text.as_bytes(),
            "policy duplicate-key attack must mutate its reviewed fixture"
        );
        attacked
    }

    #[test]
    fn human_reason_and_rewrite_poisoning_rows_execute_without_durable_mutation() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-human-poisoning");
        let reason_rows: [(&str, Vec<u8>, &str, &str); 4] = [
            (
                "reason_empty",
                Vec::new(),
                "human.reason_invalid",
                "MATRIX_PRIVATE_EMPTY",
            ),
            (
                "reason_whitespace",
                b" \t\n".to_vec(),
                "human.reason_invalid",
                "MATRIX_PRIVATE_SPACE",
            ),
            (
                "reason_nul",
                b"MATRIX_PRIVATE_REASON\0".to_vec(),
                "human.reason_invalid",
                "MATRIX_PRIVATE_REASON",
            ),
            (
                "reason_over_limit",
                vec![b'x'; 4097],
                "human.reason_invalid",
                "MATRIX_PRIVATE_LONG",
            ),
        ];
        for (family, bytes, expected, sentinel) in reason_rows {
            let before = snapshot(&fixture.store_root);
            let error = Reason::new(&bytes, 4096).expect_err(family);
            assert_eq!(error.code(), expected, "{family}");
            assert!(!error.code().contains(sentinel), "{family} leaked input");
            assert_eq!(
                before,
                snapshot(&fixture.store_root),
                "{family} mutated the store"
            );
        }
        let rewrite_rows: [(&str, Vec<u8>, &str, &str); 4] = [
            (
                "rewrite_empty",
                Vec::new(),
                "human.rewrite_invalid",
                "MATRIX_PRIVATE_EMPTY",
            ),
            (
                "rewrite_nul",
                b"MATRIX_PRIVATE_REWRITE\0".to_vec(),
                "human.rewrite_invalid",
                "MATRIX_PRIVATE_REWRITE",
            ),
            (
                "rewrite_control",
                b"MATRIX_PRIVATE_REWRITE\x01".to_vec(),
                "human.rewrite_invalid",
                "MATRIX_PRIVATE_CONTROL",
            ),
            (
                "rewrite_over_limit",
                vec![b'x'; 4097],
                "human.rewrite_invalid",
                "MATRIX_PRIVATE_LONG",
            ),
        ];
        for (family, bytes, expected, sentinel) in rewrite_rows {
            let before = snapshot(&fixture.store_root);
            let error = RewriteBytes::new(bytes, 4096).expect_err(family);
            assert_eq!(error.code(), expected, "{family}");
            assert!(!error.code().contains(sentinel), "{family} leaked input");
            assert_eq!(
                before,
                snapshot(&fixture.store_root),
                "{family} mutated the store"
            );
        }
    }

    #[test]
    fn ledger_poisoning_rows_execute_each_encoding_and_schema_attack_without_repair() {
        let fixture = ProvisionedP3Fixture::empty("matrix-ledger-poisoning");
        super::full_loop_tests::run_nonapplying_verdict(&fixture, VerdictVerb::Reject);
        let ledger = fixture.store_root.join("ledger/verdicts.jsonl");
        let baseline = fs::read_to_string(&ledger).expect("fixture ledger is UTF-8");
        let rows: [(&str, Vec<u8>, &str, &str); 4] = [
            (
                "ledger_utf8",
                vec![0xff, b'\n'],
                "ledger.utf8",
                "MATRIX_PRIVATE_LEDGER_UTF8",
            ),
            (
                "ledger_missing_lf",
                b"MATRIX_PRIVATE_LEDGER_NO_LF".to_vec(),
                "ledger.line_encoding",
                "MATRIX_PRIVATE_LEDGER_NO_LF",
            ),
            (
                "ledger_extra_lf",
                b"\n\n".to_vec(),
                "ledger.line_encoding",
                "MATRIX_PRIVATE_LEDGER_EXTRA",
            ),
            (
                "ledger_parse",
                b"{\n".to_vec(),
                "ledger.parse",
                "MATRIX_PRIVATE_LEDGER_PARSE",
            ),
        ];
        for (family, bytes, expected, sentinel) in rows {
            fs::write(&ledger, bytes).expect("inject ledger attack");
            let before = snapshot(&fixture.store_root);
            let error = read_ledger(&ledger).expect_err(family);
            assert_eq!(error.code(), expected, "{family}");
            assert!(!error.code().contains(sentinel), "{family} leaked input");
            assert_eq!(
                before,
                snapshot(&fixture.store_root),
                "{family} repaired poisoned ledger"
            );
        }

        let structured_rows = [
            (
                "ledger_unknown_field",
                baseline
                    .replacen(
                        "}\n",
                        ",\"unknown\":\"MATRIX_PRIVATE_LEDGER_UNKNOWN\"}\n",
                        1,
                    )
                    .into_bytes(),
                "ledger.schema",
                "MATRIX_PRIVATE_LEDGER_UNKNOWN",
            ),
            (
                "ledger_missing_field",
                ledger_missing_field_attack(&baseline),
                "ledger.schema",
                "MATRIX_PRIVATE_LEDGER_MISSING",
            ),
        ];
        for (family, bytes, expected, sentinel) in structured_rows {
            fs::write(&ledger, bytes).expect("inject structured ledger attack");
            let before = snapshot(&fixture.store_root);
            let error = read_ledger(&ledger).expect_err(family);
            assert_eq!(error.code(), expected, "{family}");
            assert!(!error.code().contains(sentinel), "{family} leaked input");
            assert_eq!(
                before,
                snapshot(&fixture.store_root),
                "{family} repaired poisoned ledger"
            );
        }
    }

    fn ledger_missing_field_attack(baseline: &str) -> Vec<u8> {
        let first: serde_json::Value = serde_json::from_str(
            baseline
                .lines()
                .next()
                .expect("fixture ledger has a genesis line"),
        )
        .expect("fixture genesis is JSON");
        let store_id = first["store_id"]
            .as_str()
            .expect("fixture genesis has a store ID");
        let needle = format!("\"store_id\":\"{store_id}\",");
        let attacked = baseline.replacen(&needle, "", 1).into_bytes();
        assert_ne!(
            attacked,
            baseline.as_bytes(),
            "ledger missing-field attack must mutate its fixture"
        );
        attacked
    }

    #[test]
    fn policy_and_ledger_schema_attack_builders_mutate_the_v1_fixture_bytes() {
        let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/p3-cli-store-v1/one-applied/store");
        let policy = fs::read_to_string(fixture_root.join("policy.toml"))
            .expect("read the v1 fixture policy");
        assert_ne!(
            policy_key_order_attack(&policy),
            policy.as_bytes(),
            "policy key-order row must mutate the fixture it claims to poison"
        );
        assert_ne!(
            policy_duplicate_key_attack(&policy),
            policy.as_bytes(),
            "policy duplicate-key row must mutate the fixture it claims to poison"
        );

        let ledger = fs::read_to_string(fixture_root.join("ledger/verdicts.jsonl"))
            .expect("read the v1 fixture ledger");
        assert_ne!(
            ledger_missing_field_attack(&ledger),
            ledger.as_bytes(),
            "ledger missing-field row must mutate the fixture it claims to poison"
        );
    }

    #[test]
    fn level4_corruption_rows_reject_each_durable_tamper_without_repair_or_diagnostic_leakage() {
        type StoreCorruption = fn(&Path);
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-level4-corruptions");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
        let rows: [(&str, StoreCorruption, &str, &str); 7] = [
            (
                "current_pointer",
                corrupt_current_pointer,
                "ledger.current_mismatch",
                "seed",
            ),
            (
                "receipt",
                corrupt_reindex_receipt,
                "ledger.reindex_receipt_mismatch",
                "MATRIX_PRIVATE_LEVEL4_RECEIPT",
            ),
            (
                "policy_archive",
                corrupt_policy_archive,
                "ledger.proposal_binding",
                "MATRIX_PRIVATE_LEVEL4_POLICY",
            ),
            (
                "extra_generation",
                corrupt_extra_generation,
                "transaction.incomplete",
                "MATRIX_PRIVATE_LEVEL4_GENERATION",
            ),
            (
                "verdict_lease",
                corrupt_verdict_lease,
                "store.locked",
                "MATRIX_PRIVATE_LEVEL4_LEASE",
            ),
            (
                "lease_preparation",
                corrupt_lease_preparation,
                "transaction.incomplete",
                "MATRIX_PRIVATE_LEVEL4_PREPARATION",
            ),
            (
                "ledger_event_hash",
                corrupt_ledger_event_hash,
                "ledger.event_hash",
                "MATRIX_PRIVATE_LEVEL4_LEDGER",
            ),
        ];

        for (family, corrupt, expected, sentinel) in rows {
            let store = fixture.clone_store(&format!("matrix-level4-{family}"));
            corrupt(&store);
            let before_verify = snapshot(&store);
            let error = verify_store_level4(&profile, &store).expect_err(family);
            assert_eq!(error.code(), expected, "{family}");
            assert!(
                !error.code().contains(sentinel),
                "{family} diagnostic leaked corruption bytes"
            );
            assert_eq!(
                before_verify,
                snapshot(&store),
                "{family} verification repaired durable corruption"
            );
        }
    }

    #[test]
    fn level4_ledger_rows_rebuild_canonical_events_for_each_chain_and_binding_rejection() {
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-level4-ledger-bindings");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
        type PoisonRow = (&'static str, fn(&Path), &'static str);
        let rows: [PoisonRow; 12] = [
            ("genesis_missing", remove_genesis, "ledger.genesis_missing"),
            (
                "genesis_duplicate",
                duplicate_genesis,
                "ledger.genesis_duplicate",
            ),
            (
                "duplicate_id",
                duplicate_terminal_proposal,
                "ledger.duplicate_id",
            ),
            (
                "confidence_binding",
                corrupt_confidence_binding,
                "ledger.confidence_binding",
            ),
            (
                "evaluator_binding",
                corrupt_evaluator_binding,
                "ledger.evaluator_binding",
            ),
            (
                "head_transition",
                corrupt_head_transition,
                "ledger.head_transition",
            ),
            (
                "verb_semantics",
                corrupt_verb_semantics,
                "ledger.verb_semantics",
            ),
            ("reason", corrupt_reason, "ledger.reason_empty"),
            (
                "judgment",
                corrupt_judgment_binding,
                "ledger.judgment_mismatch",
            ),
            (
                "manifest",
                corrupt_manifest_binding,
                "ledger.manifest_mismatch",
            ),
            ("bundle", corrupt_bundle_binding, "ledger.bundle_mismatch"),
            (
                "generation_chain",
                corrupt_generation_chain,
                "ledger.generation_chain",
            ),
        ];

        for (family, corrupt, expected) in rows {
            let store = fixture.clone_store(&format!("matrix-level4-ledger-{family}"));
            corrupt(&store);
            let before = snapshot(&store);
            let error = verify_store_level4(&profile, &store).expect_err(family);
            assert_eq!(error.code(), expected, "{family}");
            assert!(!error.code().contains("MATRIX_PRIVATE"), "{family}");
            assert_eq!(before, snapshot(&store), "{family} repaired corruption");
        }
    }

    #[cfg(unix)]
    #[test]
    fn unix_hard_link_and_special_file_rows_are_materialized_and_rejected_without_repair() {
        use std::os::unix::fs::FileTypeExt;
        use std::os::unix::fs::MetadataExt;
        use std::process::Command;

        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-unix-link-special");
        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");

        let hard_link_store = fixture.clone_store("matrix-hard-link");
        let source = hard_link_store.join("ledger/verdicts.jsonl");
        let hard_link = hard_link_store.join("ledger/.verdicts-0");
        fs::hard_link(&source, &hard_link).expect("materialize observable hard link");
        assert!(
            fs::metadata(&source)
                .expect("read hard-link source metadata")
                .nlink()
                >= 2,
            "the hard-link row must prove link-count observability"
        );
        let hard_before = snapshot(&hard_link_store);
        let hard_error =
            inspect_store(&profile, &hard_link_store).expect_err("hard link is transient");
        assert_eq!(hard_error.code(), "transaction.incomplete");
        assert!(!hard_error.code().contains("MATRIX_PRIVATE_HARD_LINK"));
        assert_eq!(hard_before, snapshot(&hard_link_store));

        let special_store = fixture.clone_store("matrix-special-file");
        let fifo = special_store.join("intake/events.jsonl");
        fs::remove_file(&fifo).expect("replace protected intake event log with FIFO");
        let materialized = Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        assert!(
            materialized,
            "special-file row must materialize its FIFO before asserting rejection"
        );
        assert!(
            fs::symlink_metadata(&fifo)
                .expect("read materialized FIFO metadata")
                .file_type()
                .is_fifo(),
            "special-file row must exercise the FIFO rejection route"
        );
        let special_before = snapshot(&special_store);
        let special_error =
            inspect_store(&profile, &special_store).expect_err("FIFO must be rejected");
        assert_eq!(special_error.code(), "store.layout_invalid");
        assert!(!special_error.code().contains("MATRIX_PRIVATE_SPECIAL_FILE"));
        assert_eq!(special_before, snapshot(&special_store));
    }

    #[derive(Clone, Copy)]
    enum CrashDurability {
        SeedHead,
        LedgerCommitted,
    }

    #[test]
    fn transaction_crash_rows_inject_each_apply_boundary_and_leave_a_classifiable_durable_state() {
        // These are the apply-route boundaries. Proposal publication and recovery-route boundaries
        // have different entry points and remain separately exercised by their own matrix rows.
        let rows = [
            (
                "lease_creation",
                FaultPoint::AfterLeaseCreation,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "plan_prep_sync",
                FaultPoint::AfterPlanPrepSync,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "plan_final_directory_sync",
                FaultPoint::AfterPlanFinalDirectorySync,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "plan_hard_link_sync",
                FaultPoint::AfterPlanHardLinkSync,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "plan_prep_removal",
                FaultPoint::AfterPlanPrepRemoval,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "plan_sync",
                FaultPoint::AfterPlanSync,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "journal_creation",
                FaultPoint::AfterTransactionJournalCreation,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "target_staged",
                FaultPoint::AfterTargetBytesStaged,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "judgment_staged",
                FaultPoint::AfterJudgmentBytesStaged,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "manifest_sync",
                FaultPoint::AfterManifestSync,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "level_three",
                FaultPoint::AfterLevelThreeSelfCheck,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "index_build",
                FaultPoint::DuringIndexBuild,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "index_self_check",
                FaultPoint::AfterIndexSelfCheck,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "receipt_sync",
                FaultPoint::AfterReindexReceiptSync,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "generation_rename",
                FaultPoint::AfterGenerationRename,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "index_rename",
                FaultPoint::AfterDerivedIndexRename,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "prepared_marker",
                FaultPoint::AfterPreparedMarker,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "ledger_temporary_sync",
                FaultPoint::AfterLedgerTemporarySync,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "ledger_before_rename",
                FaultPoint::BeforeLedgerRename,
                "transaction.prepare_failed",
                CrashDurability::SeedHead,
            ),
            (
                "ledger_after_rename",
                FaultPoint::AfterLedgerRename,
                "transaction.incomplete",
                CrashDurability::LedgerCommitted,
            ),
            (
                "current_before_rename",
                FaultPoint::BeforeCurrentRename,
                "transaction.incomplete",
                CrashDurability::LedgerCommitted,
            ),
            (
                "current_after_rename",
                FaultPoint::AfterCurrentRename,
                "transaction.incomplete",
                CrashDurability::LedgerCommitted,
            ),
            (
                "published_marker",
                FaultPoint::BeforePublishedMarker,
                "transaction.incomplete",
                CrashDurability::LedgerCommitted,
            ),
            (
                "cleanup",
                FaultPoint::DuringCleanup,
                "transaction.incomplete",
                CrashDurability::LedgerCommitted,
            ),
            (
                "journal_cleanup",
                FaultPoint::AfterJournalCleanup,
                "transaction.incomplete",
                CrashDurability::LedgerCommitted,
            ),
        ];

        assert_eq!(
            rows.len(),
            25,
            "the matrix must execute every apply-route FaultPoint; proposal-publication and recovery \
             points have no ProvisionedP3Fixture route"
        );

        for (family, point, expected_code, durability) in rows {
            let fixture =
                ProvisionedP3Fixture::with_staged_reject(&format!("matrix-crash-{family}"));
            let before = snapshot(&fixture.store_root);
            let runtime = fixture.runtime_with_fault(FaultPlan::at(point));
            let error = fixture
                .apply_with_runtime(VerdictVerb::Accept, "MATRIX_PRIVATE_CRASH_REASON", &runtime)
                .expect_err(family);
            assert_eq!(error.code(), expected_code, "{family}");
            assert!(
                !error.code().contains("MATRIX_PRIVATE_CRASH_REASON"),
                "{family} leaked human input"
            );
            let after = snapshot(&fixture.store_root);
            assert_ne!(
                before.root_hash, after.root_hash,
                "{family} must retain recovery evidence"
            );
            match durability {
                CrashDurability::SeedHead => assert_eq!(
                    before.ledger_hash, after.ledger_hash,
                    "{family} committed a ledger event before the ledger boundary"
                ),
                CrashDurability::LedgerCommitted => assert_ne!(
                    before.ledger_hash, after.ledger_hash,
                    "{family} lost the committed ledger event"
                ),
            }
        }
    }

    #[test]
    fn proposal_publication_crash_rows_execute_the_real_staging_path_without_partial_pairs() {
        let rows = [
            (
                "proposal_candidate_sync",
                sovmem::profile::PublicationFaultPoint::AfterCandidateSync,
            ),
            (
                "proposal_metadata_sync",
                sovmem::profile::PublicationFaultPoint::AfterMetadataSync,
            ),
            (
                "proposal_directory_rename",
                sovmem::profile::PublicationFaultPoint::BeforeDirectoryRename,
            ),
        ];

        for (family, point) in rows {
            let fixture =
                sovmem::profile::p3_internal::p3::P3Fixture::new(&format!("matrix-{family}"));
            let before = snapshot(&fixture.store_root);
            let error = sovmem::profile::submit_proposal_with_publication_fault(
                matrix_submission(&fixture, fixture.candidate_arg(), None),
                point,
            )
            .expect_err(family);
            assert_eq!(error.code(), "proposal.write", "{family}");
            let after = snapshot(&fixture.store_root);
            assert_eq!(before.ledger_hash, after.ledger_hash, "{family}");
            assert_ne!(before.root_hash, after.root_hash, "{family}");
            let snapshot_difference = format!(
                "{family} changed durable root {} -> {}",
                before.root_hash, after.root_hash
            );
            assert_no_partial_submission_or_protected_pairs(
                &fixture.store_root,
                &snapshot_difference,
            );
        }
    }

    #[test]
    fn publication_atomicity_assertion_rejects_visible_inbox_and_protected_pair_partials() {
        use std::panic::{catch_unwind, AssertUnwindSafe};

        fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
            if let Some(message) = payload.downcast_ref::<String>() {
                return message.clone();
            }
            if let Some(message) = payload.downcast_ref::<&str>() {
                return (*message).to_owned();
            }
            "non-string panic payload".to_owned()
        }

        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new(
            "matrix-publication-atomicity-assertion",
        );
        let hidden = fixture
            .store_root
            .join("staging/inbox/.submission-review-residue");
        fs::create_dir(&hidden).expect("materialize permitted hidden recovery residue");
        fs::write(hidden.join("candidate.md"), b"partial\n")
            .expect("write hidden recovery residue");
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                assert_no_partial_submission_or_protected_pairs(
                    &fixture.store_root,
                    "hidden_submission_residue",
                );
            }))
            .is_ok(),
            "hidden .submission-* recovery residue remains permitted"
        );

        let visible = fixture
            .store_root
            .join(format!("staging/inbox/sub-{}", "a".repeat(64)));
        fs::create_dir(&visible).expect("materialize visible inbox submission");
        fs::write(visible.join("candidate.md"), b"partial\n")
            .expect("write partial visible inbox submission");
        let visible_panic = catch_unwind(AssertUnwindSafe(|| {
            assert_no_partial_submission_or_protected_pairs(
                &fixture.store_root,
                "visible_submission",
            );
        }))
        .expect_err("visible sub-* partial submission must fail the atomicity assertion");
        assert!(
            panic_message(visible_panic)
                .contains("visible_submission exposed a partial visible inbox submission"),
            "visible inbox assertion must identify its exact failing family"
        );
        fs::remove_dir_all(&visible).expect("remove visible inbox assertion fixture");

        let protected = fixture
            .store_root
            .join(format!("intake/proposals/aa/prp-{}", "a".repeat(64)));
        fs::create_dir_all(&protected).expect("materialize protected proposal pair");
        fs::write(protected.join("candidate.md"), b"partial\n")
            .expect("write partial protected proposal pair");
        let protected_panic = catch_unwind(AssertUnwindSafe(|| {
            assert_no_partial_submission_or_protected_pairs(&fixture.store_root, "protected_pair");
        }))
        .expect_err("partial protected pair must fail the atomicity assertion");
        assert!(
            panic_message(protected_panic)
                .contains("protected_pair exposed a partial visible proposal pair"),
            "protected pair assertion must identify its exact failing family"
        );
    }

    #[test]
    fn recovery_crash_rows_execute_every_recovery_boundary_against_real_interrupted_evidence() {
        let rows = [
            ("recovery_lease", FaultPoint::AfterRecoveryLeaseAcquisition),
            ("recovery_journal", FaultPoint::AfterRecoveryJournalCreation),
            (
                "recovery_original_witness",
                FaultPoint::AfterOriginalCleanupWitness,
            ),
            (
                "recovery_published",
                FaultPoint::AfterRecoveryPublishedMarker,
            ),
            (
                "recovery_original_cleanup",
                FaultPoint::AfterOriginalCleanup,
            ),
            (
                "recovery_cleanup_witness",
                FaultPoint::AfterRecoveryCleanupWitness,
            ),
            (
                "recovery_verification",
                FaultPoint::AfterRecoveryVerification,
            ),
        ];

        for (family, point) in rows {
            let fixture = ProvisionedP3Fixture::with_staged_reject(&format!("matrix-{family}"));
            let interrupted =
                fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
            fixture
                .apply_with_runtime(
                    VerdictVerb::Accept,
                    "MATRIX_PRIVATE_INTERRUPTED",
                    &interrupted,
                )
                .expect_err("prepare real recovery evidence");
            let before = snapshot(&fixture.store_root);
            let runtime = fixture.runtime_with_fault(FaultPlan::at(point));
            let error = fixture.recover_with_runtime(&runtime).expect_err(family);
            assert_eq!(error.code(), "transaction.prepare_failed", "{family}");
            assert!(
                !error.code().contains("MATRIX_PRIVATE_INTERRUPTED"),
                "{family}"
            );
            let after = snapshot(&fixture.store_root);
            assert_ne!(before.root_hash, after.root_hash, "{family}");
            assert_eq!(before.ledger_hash, after.ledger_hash, "{family}");
        }
    }

    #[test]
    fn stale_verdict_lease_blocks_a_second_verdict_attempt_without_repairing_recovery_evidence() {
        let fixture = ProvisionedP3Fixture::with_staged_reject("matrix-stale-verdict-lease");
        let faulted = fixture.runtime_with_fault(FaultPlan::at(FaultPoint::AfterPreparedMarker));
        let first = fixture
            .apply_with_runtime(
                VerdictVerb::Reject,
                "MATRIX_PRIVATE_STALE_LEASE_REASON",
                &faulted,
            )
            .expect_err("interrupted first verdict must retain its recovery state");
        assert_eq!(first.code(), "transaction.prepare_failed");
        assert!(!first.code().contains("MATRIX_PRIVATE_STALE_LEASE_REASON"));
        let before_second = snapshot(&fixture.store_root);

        let second = fixture
            .apply(VerdictVerb::Reject, "MATRIX_PRIVATE_SECOND_VERDICT")
            .expect_err("second verdict attempt must observe the stale protected lease");
        assert_eq!(
            second.code(),
            "store.locked",
            "the public second verdict path must fail closed before it can alter a stale transaction"
        );
        assert!(!second.code().contains("MATRIX_PRIVATE_SECOND_VERDICT"));
        assert_eq!(
            before_second,
            snapshot(&fixture.store_root),
            "a blocked competing verdict must not repair or mutate the first attempt's durable evidence"
        );
    }

    fn malformed_profile_row() -> MatrixObservation {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-malformed-profile");
        let profile = fixture
            .store_root
            .join("matrix-profile/audy-ssa0-p3-v2/profile.yaml");
        fs::create_dir_all(profile.parent().expect("profile has parent")).unwrap();
        fs::write(&profile, b"MATRIX_PRIVATE_PROFILE_SENTINEL: \xff\n").unwrap();
        let before = snapshot(&fixture.store_root);
        let code = load_p3_profile(&profile)
            .expect_err("malformed profile must fail")
            .code()
            .to_owned();
        MatrixObservation::code(code, before, snapshot(&fixture.store_root))
    }

    fn malformed_policy_row() -> MatrixObservation {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-malformed-policy");
        let before = snapshot(&fixture.store_root);
        let code = parse_policy(b"\xff\n")
            .expect_err("malformed policy must fail")
            .code()
            .to_owned();
        MatrixObservation::code(code, before, snapshot(&fixture.store_root))
    }

    fn pair_precedence_row() -> MatrixObservation {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-pair-precedence");
        let before = snapshot(&fixture.store_root);
        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
        let store = inspect_store(&profile, &fixture.store_root).expect("inspect P3 store");
        let proposal = sovmem::profile::ProposalPair {
            meta: sovmem::profile::ProposalMeta {
                schema_version: "unsupported".to_owned(),
                proposal_id: "not-a-proposal-id".to_owned(),
                submitted_at: "invalid".to_owned(),
                source_id: "MATRIX_PRIVATE_STALE_SENTINEL".to_owned(),
                operation: ProposalOperation::Create,
                intent: ProposalIntent::Accept,
                target_id: "unsafe target".to_owned(),
                target_path: "../outside.md".to_owned(),
                base: sovmem::profile::ProposalBase {
                    kind: "invalid".to_owned(),
                    generation_id: None,
                    bundle_hash: "invalid".to_owned(),
                    manifest_hash: "invalid".to_owned(),
                    target_hash: None,
                },
                candidate_hash: "invalid".to_owned(),
                candidate_bytes: 0,
                policy_hash: "invalid".to_owned(),
                profile_hash: "invalid".to_owned(),
                confidence: None,
                evaluator: None,
            },
            meta_bytes: b"{}\n".to_vec(),
            candidate_bytes: b"MATRIX_PRIVATE_STALE_SENTINEL".to_vec(),
            meta_hash: "invalid".to_owned(),
            candidate_hash: "invalid".to_owned(),
        };
        let intake = sovmem::profile::IntakePrefix {
            head: sovmem::profile::IntakeHead {
                sequence: 0,
                event_hash: None,
            },
            staged: Vec::new(),
            rejected: Vec::new(),
        };
        let context = sovmem::profile::PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &intake,
        };
        let decision = sovmem::profile::PolicyEvaluator::evaluate(&context);
        assert_eq!(decision.rule_codes, ["proposal.meta_schema"]);
        MatrixObservation::code(
            decision.rule_codes[0].clone(),
            before,
            snapshot(&fixture.store_root),
        )
    }

    fn malformed_evaluator_row() -> MatrixObservation {
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-malformed-evaluator");
        let evaluator = fixture.input_file(
            "matrix-malformed-evaluator",
            b"{MATRIX_PRIVATE_EVALUATOR_SENTINEL\n",
        );
        let before = snapshot(&fixture.store_root);
        let code = submit_proposal(matrix_submission(
            &fixture,
            fixture.candidate_arg(),
            Some(evaluator),
        ))
        .expect_err("malformed evaluator must fail before proposal preparation")
        .code()
        .to_owned();
        MatrixObservation::code(code, before, snapshot(&fixture.store_root))
    }

    #[cfg(unix)]
    fn symlink_row() -> MatrixObservation {
        use std::os::unix::fs::symlink;

        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-symlink");
        let store = fixture.clone_store("matrix-symlink-copy");
        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
        symlink(
            store.join("seed"),
            store.join("MATRIX_PRIVATE_PATH_SENTINEL"),
        )
        .unwrap();
        let before = snapshot(&store);
        let code = inspect_store(&profile, &store)
            .expect_err("symlink must fail before layout evaluation")
            .code()
            .to_owned();
        MatrixObservation::code(code, before, snapshot(&store))
    }

    #[cfg(not(unix))]
    fn symlink_row() -> MatrixObservation {
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-symlink-unsupported");
        let before = snapshot(&fixture.store_root);
        let code = sovmem::profile::Confidence::parse("0.1234567")
            .expect_err("portable matrix fallback must execute a real parser rejection")
            .code()
            .to_owned();
        MatrixObservation::code(code, before, snapshot(&fixture.store_root))
    }

    fn provenance_row() -> MatrixObservation {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-provenance");
        let candidate = fs::read(fixture.candidate_arg()).expect("read fixture candidate");
        let candidate_text = String::from_utf8(candidate)
            .expect("fixture candidate is UTF-8")
            .replacen("provenance: synthesis\n", "", 1)
            + "\nMATRIX_PRIVATE_CANDIDATE_SENTINEL\n";
        let candidate = fixture.input_file("matrix-missing-provenance", candidate_text.as_bytes());
        let before = snapshot(&fixture.store_root);
        let submitted = submit_proposal(matrix_submission(&fixture, candidate, None))
            .expect("untrusted transport does not decide missing provenance");
        assert!(submitted.proposal_id.is_empty());
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let sealed = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-21T02:20:00.000000000Z").unwrap()),
        )
        .expect("human seal decides missing provenance");
        assert_eq!(sealed.state, "rejected-at-intake");
        assert!(sealed.proposal_id.is_none());
        let profile = load_p3_profile(&fixture.profile_path).expect("load provenance profile");
        let store = inspect_store(&profile, &fixture.store_root).expect("inspect provenance store");
        let intake = read_intake(&store.paths).expect("read provenance intake rejection");
        let code = intake
            .events()
            .last()
            .expect("protected rejection event")
            .payload()
            .rule_codes[0]
            .clone();
        let after = snapshot(&fixture.store_root);
        assert_eq!(
            before.ledger_hash, after.ledger_hash,
            "intake rejection cannot mutate ledger"
        );
        MatrixObservation::redacted(
            code,
            before,
            after,
            candidate_text,
            receipt_projection(&fixture),
        )
    }

    fn confidence_row() -> MatrixObservation {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-confidence");
        let before = snapshot(&fixture.store_root);
        let code = sovmem::profile::Confidence::parse("0.1234567")
            .expect_err("extra precision must be rejected")
            .code()
            .to_owned();
        MatrixObservation::code(code, before, snapshot(&fixture.store_root))
    }

    fn forged_evaluator_row() -> MatrixObservation {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-forged-evaluator");
        let candidate = fixture.candidate_arg();
        let candidate_hash = blake3::hash(&fs::read(candidate).unwrap())
            .to_hex()
            .to_string();
        let evaluator_bytes = serde_json::to_vec(&EvaluatorBlock {
            protocol: "adversarial-bug-finding-v1".to_owned(),
            source_id: "src:human:matrix-private-evaluator".to_owned(),
            evaluated_at: "2026-07-14T04:29:00.000000000Z".to_owned(),
            candidate_hash,
            recommendation: "looks-sound".to_owned(),
            reasons: vec!["MATRIX_PRIVATE_EVALUATOR_SENTINEL".to_owned()],
        })
        .unwrap();
        let evaluator = fixture.input_file("matrix-forged-evaluator", &evaluator_bytes);
        let evaluator_text = String::from_utf8(fs::read(&evaluator).unwrap()).unwrap();
        let submitted = submit_proposal(matrix_submission(&fixture, candidate, Some(evaluator)))
            .expect("a structurally valid evaluator must submit before seal-time tampering");
        let metadata = fixture
            .store_root
            .join("staging/inbox")
            .join(&submitted.submission_id)
            .join("submission.meta.json");
        let mut forged: SubmissionMeta =
            serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
        forged
            .evaluator
            .as_mut()
            .expect("submitted evaluator")
            .candidate_hash = "0".repeat(64);
        fs::write(&metadata, serde_json::to_vec(&forged).unwrap()).unwrap();
        let before = snapshot(&fixture.store_root);
        let mut human = ScriptedHuman::new(None);
        let outcome = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-26T08:20:00.000000000Z").unwrap()),
        )
        .expect("forged evaluator must create a redacted pre-policy intake rejection");
        assert_eq!(outcome.state, "rejected-at-intake");
        assert_eq!(human.show_count(), 0);
        assert_eq!(human.read_count(), 0);
        let after = snapshot(&fixture.store_root);
        let receipt =
            String::from_utf8(fs::read(fixture.store_root.join("intake/events.jsonl")).unwrap())
                .unwrap();
        MatrixObservation::redacted(
            "proposal.evaluator_hash_mismatch".to_owned(),
            before,
            after,
            evaluator_text,
            receipt,
        )
    }

    fn rewrite_required_row() -> MatrixObservation {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-rewrite-required");
        let before = snapshot(&fixture.store_root);
        let reason = Reason::new(b"MATRIX_PRIVATE_REASON_SENTINEL", 4096).unwrap();
        let ruled_at = Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap();
        let policy_hash = "b".repeat(64);
        let profile_hash = "c".repeat(64);
        let code = validate_decision(
            ProposalIntent::Rewrite,
            VerdictVerb::Rewrite,
            reason.clone(),
            None,
            &VerdictIdentityInput {
                proposal_id: "prp-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                verb: VerdictVerb::Rewrite,
                reason: &reason,
                rewrite: None,
                actor: "src:human:fixture",
                ruled_at: &ruled_at,
                prior_ledger_head: None,
                intake_sequence: 0,
                intake_head_hash: None,
                policy_hash: &policy_hash,
                profile_hash: &profile_hash,
            },
        )
        .expect_err("REWRITE must bind exact human rewrite bytes")
        .code()
        .to_owned();
        MatrixObservation::code(code, before, snapshot(&fixture.store_root))
    }

    fn current_pointer_row() -> MatrixObservation {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-current-pointer");
        fixture.install_fixture_chain_with_one_applied_generation();
        let store = fixture.clone_store("matrix-current-pointer-copy");
        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
        fs::write(store.join("canon/CURRENT"), b"seed\n").unwrap();
        let before = snapshot(&store);
        let code = verify_store_level4(&profile, &store)
            .expect_err("stale current pointer must fail Level 4")
            .code()
            .to_owned();
        MatrixObservation::code(code, before, snapshot(&store))
    }

    fn incomplete_transaction_row() -> MatrixObservation {
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-incomplete-transaction");
        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
        fs::create_dir(
            fixture
                .store_root
                .join("transactions")
                .join(format!("txn-{}", "a".repeat(64))),
        )
        .unwrap();
        let before = snapshot(&fixture.store_root);
        let code = inspect_store(&profile, &fixture.store_root)
            .expect_err("visible transaction must fail closed")
            .code()
            .to_owned();
        MatrixObservation::code(code, before, snapshot(&fixture.store_root))
    }

    fn protected_lease_row() -> MatrixObservation {
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-incomplete-transaction");
        fixture.install_fixture_chain_with_one_applied_generation();
        let store = fixture.clone_store("matrix-incomplete-transaction-copy");
        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
        fs::create_dir(store.join("locks/verdict.lock")).unwrap();
        fs::write(
            store.join("locks/verdict.lock/owner.json"),
            b"MATRIX_PRIVATE_RECOVERY_SENTINEL\n",
        )
        .unwrap();
        let before = snapshot(&store);
        let code = verify_store_level4(&profile, &store)
            .expect_err("visible protected lease must fail closed")
            .code()
            .to_owned();
        MatrixObservation::code(code, before, snapshot(&store))
    }

    type IntakeCorruptionCase = (&'static str, &'static str, fn(&Path));

    const INTAKE_CORRUPTION_CASES: [IntakeCorruptionCase; 14] = [
        (
            "event_deletion",
            "intake.chain_invalid",
            intake_delete_first_event,
        ),
        (
            "event_insertion",
            "intake.chain_invalid",
            intake_insert_first_event,
        ),
        (
            "event_reordering",
            "intake.chain_invalid",
            intake_reorder_events,
        ),
        (
            "duplicate_event_id",
            "intake.chain_invalid",
            intake_duplicate_event_id,
        ),
        (
            "previous_hash_alteration",
            "intake.chain_invalid",
            intake_alter_previous_hash,
        ),
        (
            "event_hash_alteration",
            "intake.chain_invalid",
            intake_alter_event_hash,
        ),
        (
            "sealed_candidate_hash_mismatch",
            "intake.pair_binding",
            intake_alter_candidate,
        ),
        (
            "sealed_meta_hash_mismatch",
            "intake.pair_binding",
            intake_alter_meta,
        ),
        ("missing_pair", "intake.pair_binding", intake_remove_pair),
        ("extra_pair", "intake.pair_binding", intake_add_extra_pair),
        (
            "event_pair_cross_binding",
            "intake.pair_binding",
            intake_cross_bind_event,
        ),
        (
            "noncanonical_line",
            "intake.record_invalid",
            intake_make_line_noncanonical,
        ),
        (
            "rule_code_disorder",
            "intake.record_invalid",
            intake_disorder_rule_codes,
        ),
        (
            "backward_time",
            "intake.clock_regression",
            intake_move_time_backward,
        ),
    ];

    #[test]
    fn level4_intake_corruption_matrix_returns_first_invalid_layer_without_repair() {
        let fixture = task6_level4_intake_fixture("level4-intake-corruption-matrix");
        let profile = load_p3_profile(&fixture.profile_path).expect("load Task 6 fixture profile");

        for (family, _, corrupt) in INTAKE_CORRUPTION_CASES {
            let store = clone_task6_store(&fixture.store_root, family);
            corrupt(&store);
            let before = snapshot(&store);
            let error = match verify_store_level4(&profile, &store) {
                Ok(_) => panic!("{family} must fail Level 4"),
                Err(error) => error,
            };
            assert_eq!(
                error.code(),
                "transaction.incomplete",
                "{family} returned the wrong first-invalid-layer code"
            );
            assert_eq!(
                snapshot(&store),
                before,
                "{family} triggered verifier repair"
            );
        }

        let store = clone_task6_store(&fixture.store_root, "high-water-mismatch");
        replace_canonical_event_field(
            &store,
            1,
            b"\"intake_sequence\":2",
            b"\"intake_sequence\":3",
        );
        let before = snapshot(&store);
        assert_eq!(
            verify_store_level4(&profile, &store).unwrap_err().code(),
            "ledger.intake_binding"
        );
        assert_eq!(
            snapshot(&store),
            before,
            "high-water mismatch triggered repair"
        );
    }

    #[test]
    fn read_intake_corruption_matrix_preserves_detailed_codes_without_repair() {
        let fixture = task6_level4_intake_fixture("read-intake-corruption-matrix");
        let profile = load_p3_profile(&fixture.profile_path).expect("load Task 6 fixture profile");
        verify_store_level4(&profile, &fixture.store_root)
            .expect("the direct-parser matrix must start from a valid Level 4 fixture");

        for (family, expected, corrupt) in INTAKE_CORRUPTION_CASES {
            let store = clone_task6_store(&fixture.store_root, &format!("read-intake-{family}"));
            corrupt(&store);
            let before = snapshot(&store);
            let paths = sovmem::profile::StorePaths::from_root(store.clone());
            let error = match read_intake(&paths) {
                Ok(_) => panic!("{family} must fail direct intake replay"),
                Err(error) => error,
            };
            assert_eq!(
                error.code(),
                expected,
                "{family} returned the wrong direct intake code"
            );
            assert_eq!(
                snapshot(&store),
                before,
                "{family} triggered direct intake repair"
            );
        }
    }

    #[test]
    fn level4_intake_transaction_journal_is_incomplete_without_repair() {
        assert_level4_intake_transient(
            "level4-intake-transaction-journal",
            intake_add_transaction_journal,
        );
    }

    #[test]
    fn level4_intake_event_temporary_is_incomplete_without_repair() {
        assert_level4_intake_transient("level4-intake-event-temporary", intake_add_event_temporary);
    }

    #[test]
    fn level4_intake_trusted_lease_temporary_is_incomplete_without_repair() {
        assert_level4_intake_transient(
            "level4-intake-trusted-lease-temporary",
            intake_add_lease_temporary,
        );
    }

    #[test]
    fn level4_intake_malformed_lease_temporary_remains_layout_invalid_without_repair() {
        let fixture = task6_level4_intake_fixture("level4-intake-malformed-lease-temporary");
        let profile = load_p3_profile(&fixture.profile_path).expect("load Task 6 fixture profile");
        let lease = fixture
            .store_root
            .join("locks")
            .join(format!(".lease-intake-{}", "a".repeat(63)));
        fs::create_dir(&lease).expect("create malformed trusted intake lease temporary");
        let before = snapshot(&fixture.store_root);
        assert_eq!(
            verify_store_level4(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "store.layout_invalid"
        );
        assert_eq!(snapshot(&fixture.store_root), before);
    }

    fn assert_level4_intake_transient(name: &str, interrupt: fn(&Path)) {
        let fixture = task6_level4_intake_fixture(name);
        let profile = load_p3_profile(&fixture.profile_path).expect("load Task 6 fixture profile");
        interrupt(&fixture.store_root);
        let before = snapshot(&fixture.store_root);
        let error = match verify_store_level4(&profile, &fixture.store_root) {
            Ok(_) => panic!("{name} must fail closed as an interrupted intake transaction"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "transaction.incomplete");
        assert_eq!(
            snapshot(&fixture.store_root),
            before,
            "{name} triggered verifier repair"
        );
    }

    fn task6_level4_intake_fixture(name: &str) -> ProvisionedP3Fixture {
        let fixture = ProvisionedP3Fixture::with_staged_reject(name);
        let profile = load_p3_profile(&fixture.profile_path).expect("load Task 6 fixture profile");
        let store =
            inspect_store(&profile, &fixture.store_root).expect("inspect Task 6 fixture store");
        let view = read_intake(&store.paths).expect("read Task 6 fixture intake");
        let timestamp = Timestamp::parse("2026-07-21T06:30:02.000000000Z")
            .expect("fixed Task 6 rejection time");
        let identity = IntakeLeaseIdentity::new("task6-level4", "test", &timestamp, 7)
            .expect("construct Task 6 intake identity");
        let mut lease = IntakePublicationLease::acquire(&store.paths, identity)
            .expect("acquire Task 6 intake lease");
        let event = IntakeEvent::new(IntakePayload {
            schema_version: sovmem::profile::INTAKE_EVENT_SCHEMA.to_owned(),
            event_type: "intake".to_owned(),
            sequence: 2,
            event_id: "int-0000000000000002".to_owned(),
            occurred_at: timestamp.as_str().to_owned(),
            previous_event_hash: view.head().event_hash.clone(),
            outcome: IntakeOutcome::REJECTED,
            source_id: Some("src:agent:task6-rejection".to_owned()),
            proposal_id: None,
            proposal_meta_hash: None,
            candidate_hash: None,
            policy_hash: store.policy.hash.clone(),
            profile_hash: profile.hash.clone(),
            rule_codes: vec!["policy.fixture_rejected".to_owned()],
        })
        .expect("construct Task 6 intake rejection");
        append_intake_event(&store.paths, &mut lease, &event)
            .expect("append Task 6 intake rejection");
        lease
            .release(&store.paths)
            .expect("release Task 6 intake lease");
        let request = fixture.request(b"Task 6 exact-prefix fixture reason");
        fixture.commit_exact_reject(&request);
        fixture
    }

    fn clone_task6_store(source: &Path, label: &str) -> std::path::PathBuf {
        let destination = source
            .parent()
            .expect("fixture store has a parent")
            .join(format!("task6-{label}"));
        copy_task6_tree(source, &destination);
        destination
    }

    fn copy_task6_tree(source: &Path, destination: &Path) {
        let metadata = fs::symlink_metadata(source).expect("read Task 6 source metadata");
        if metadata.is_file() {
            fs::write(
                destination,
                fs::read(source).expect("read Task 6 source file"),
            )
            .expect("copy Task 6 source file");
            return;
        }
        fs::create_dir(destination).expect("create Task 6 destination directory");
        let mut entries = fs::read_dir(source)
            .expect("read Task 6 source directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect Task 6 source entries");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_task6_tree(&entry.path(), &destination.join(entry.file_name()));
        }
    }

    fn intake_events(store: &Path) -> Vec<IntakeEvent> {
        let paths = sovmem::profile::StorePaths::from_root(store.to_owned());
        read_intake(&paths)
            .expect("read canonical Task 6 intake")
            .events()
            .to_vec()
    }

    fn write_intake_events(store: &Path, events: &[IntakeEvent]) {
        let bytes = events
            .iter()
            .flat_map(IntakeEvent::canonical_line_bytes)
            .collect::<Vec<_>>();
        fs::write(store.join("intake/events.jsonl"), bytes).expect("write Task 6 intake tamper");
    }

    fn intake_delete_first_event(store: &Path) {
        let events = intake_events(store);
        write_intake_events(store, &events[1..]);
    }

    fn intake_insert_first_event(store: &Path) {
        let events = intake_events(store);
        write_intake_events(
            store,
            &[events[0].clone(), events[0].clone(), events[1].clone()],
        );
    }

    fn intake_reorder_events(store: &Path) {
        let events = intake_events(store);
        write_intake_events(store, &[events[1].clone(), events[0].clone()]);
    }

    fn intake_duplicate_event_id(store: &Path) {
        let events = intake_events(store);
        let mut duplicate = events[1].payload().clone();
        duplicate.event_id = events[0].payload().event_id.clone();
        let duplicate = IntakeEvent::new(duplicate).expect("rebuild duplicate intake ID");
        write_intake_events(store, &[events[0].clone(), duplicate]);
    }

    fn intake_alter_previous_hash(store: &Path) {
        let events = intake_events(store);
        let mut altered = events[1].payload().clone();
        altered.previous_event_hash = Some("a".repeat(64));
        let altered = IntakeEvent::new(altered).expect("rebuild altered previous hash");
        write_intake_events(store, &[events[0].clone(), altered]);
    }

    fn intake_alter_event_hash(store: &Path) {
        let path = store.join("intake/events.jsonl");
        let mut bytes = fs::read(&path).expect("read intake for event-hash alteration");
        let needle = b"\"event_hash\":\"";
        let offset = bytes
            .windows(needle.len())
            .position(|window| window == needle)
            .expect("intake event contains hash")
            + needle.len();
        bytes[offset] = if bytes[offset] == b'a' { b'b' } else { b'a' };
        fs::write(path, bytes).expect("write event-hash alteration");
    }

    fn intake_alter_candidate(store: &Path) {
        let proposal = task6_pair_root(store);
        let candidate = proposal.join("candidate.md");
        let mut bytes = fs::read(&candidate).expect("read sealed candidate");
        bytes.extend_from_slice(b"\nTask 6 candidate tamper.\n");
        fs::write(candidate, bytes).expect("write sealed candidate tamper");
    }

    fn intake_alter_meta(store: &Path) {
        let meta = task6_pair_root(store).join("proposal.meta.json");
        let mut bytes = fs::read(&meta).expect("read sealed proposal meta");
        bytes.extend_from_slice(b" \n");
        fs::write(meta, bytes).expect("write sealed proposal meta tamper");
    }

    fn intake_remove_pair(store: &Path) {
        fs::remove_dir_all(task6_pair_root(store)).expect("remove sealed proposal pair");
    }

    fn intake_add_extra_pair(store: &Path) {
        let source = task6_pair_root(store);
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .expect("UTF-8 proposal ID");
        let mut extra = name.as_bytes().to_vec();
        let last = extra.last_mut().expect("proposal ID is nonempty");
        *last = if *last == b'a' { b'b' } else { b'a' };
        let extra = source
            .parent()
            .expect("proposal pair has shard parent")
            .join(String::from_utf8(extra).expect("altered proposal ID remains UTF-8"));
        copy_task6_tree(&source, &extra);
    }

    fn intake_cross_bind_event(store: &Path) {
        let events = intake_events(store);
        let mut staged = events[0].payload().clone();
        staged.proposal_meta_hash = Some("a".repeat(64));
        let staged = IntakeEvent::new(staged).expect("rebuild cross-bound staged event");
        let mut rejected = events[1].payload().clone();
        rejected.previous_event_hash = Some(staged.hash().to_owned());
        let rejected = IntakeEvent::new(rejected).expect("rechain successor rejection");
        write_intake_events(store, &[staged, rejected]);
    }

    fn intake_make_line_noncanonical(store: &Path) {
        let path = store.join("intake/events.jsonl");
        let bytes = fs::read(&path).expect("read canonical intake line");
        let mut noncanonical = Vec::with_capacity(bytes.len() + 1);
        noncanonical.extend_from_slice(b"{ ");
        noncanonical.extend_from_slice(&bytes[1..]);
        fs::write(path, noncanonical).expect("write noncanonical intake line");
    }

    fn intake_disorder_rule_codes(store: &Path) {
        let path = store.join("intake/events.jsonl");
        let bytes = fs::read(&path).expect("read intake for rule-code disorder");
        let mut lines = bytes
            .strip_suffix(b"\n")
            .expect("canonical intake has terminal LF")
            .split(|byte| *byte == b'\n')
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let mut rejected: serde_json::Value =
            serde_json::from_slice(&lines[1]).expect("parse rejected intake line");
        rejected["rule_codes"] = serde_json::json!(["policy.z", "policy.a"]);
        lines[1] = serde_json::to_vec(&rejected).expect("serialize disordered rule codes");
        let mut rewritten = lines.join(&b'\n');
        rewritten.push(b'\n');
        fs::write(path, rewritten).expect("write rule-code disorder");
    }

    fn intake_move_time_backward(store: &Path) {
        let events = intake_events(store);
        let mut backward = events[1].payload().clone();
        backward.occurred_at = "2026-07-21T06:29:59.000000000Z".to_owned();
        let backward = IntakeEvent::new(backward).expect("rebuild backward intake event");
        write_intake_events(store, &[events[0].clone(), backward]);
    }

    fn task6_pair_root(store: &Path) -> std::path::PathBuf {
        let paths = sovmem::profile::StorePaths::from_root(store.to_owned());
        let view = read_intake(&paths).expect("read Task 6 intake for sealed pair");
        let proposal_id = view.events()[0]
            .payload()
            .proposal_id
            .as_deref()
            .expect("first Task 6 event stages a proposal");
        paths
            .intake_proposals
            .join(&proposal_id[4..6])
            .join(proposal_id)
    }

    fn intake_add_transaction_journal(store: &Path) {
        fs::create_dir(
            store
                .join("intake/transactions")
                .join(format!("txn-{}", "a".repeat(64))),
        )
        .expect("create interrupted intake transaction journal");
    }

    fn intake_add_event_temporary(store: &Path) {
        fs::write(
            store.join("intake/.events-3"),
            b"MATRIX_PRIVATE_INTAKE_TEMPORARY\n",
        )
        .expect("create interrupted intake event temporary");
    }

    fn intake_add_lease_temporary(store: &Path) {
        let lease = store
            .join("locks")
            .join(format!(".lease-intake-{}", "a".repeat(64)));
        fs::create_dir(&lease).expect("create trusted intake lease temporary");
        fs::write(lease.join("owner.json"), b"MATRIX_PRIVATE_INTAKE_LEASE\n")
            .expect("write trusted intake lease temporary owner");
    }

    fn corrupt_current_pointer(store: &Path) {
        fs::write(store.join("canon/CURRENT"), b"seed\n").expect("corrupt CURRENT binding");
    }

    fn corrupt_canonical_target(store: &Path) {
        let generation = store.join("canon/generations/gen-0000000000000001");
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(generation.join("manifest.json")).expect("read canonical manifest"),
        )
        .expect("parse canonical manifest");
        let output_path = manifest["documents"]
            .as_array()
            .expect("canonical manifest documents")
            .iter()
            .find(|document| {
                document["proposal_id"].is_string()
                    && document["verdict_id"].is_string()
                    && !document["output_path"]
                        .as_str()
                        .unwrap_or_default()
                        .starts_with("judgments/")
            })
            .and_then(|document| document["output_path"].as_str())
            .expect("canonical manifest has applied target");
        let target = generation.join(output_path);
        let mut bytes = fs::read(&target)
            .unwrap_or_else(|error| panic!("read canonical target {}: {error}", target.display()));
        bytes.extend_from_slice(b"\nMATRIX_PRIVATE_TARGET\n");
        fs::write(target, bytes).expect("corrupt canonical target bytes");
    }

    fn corrupt_retained_candidate(store: &Path) {
        let candidate = staged_proposal_root(store).join("candidate.md");
        let mut bytes = fs::read(&candidate).expect("read retained candidate");
        bytes.extend_from_slice(b"\nMATRIX_PRIVATE_CANDIDATE\n");
        fs::write(candidate, bytes).expect("corrupt retained candidate bytes");
    }

    fn corrupt_rewrite_binding(store: &Path) {
        replace_applied_event(store, |payload| {
            payload.rewrite_hash = Some("a".repeat(64));
            payload.rewrite_bytes = Some(1);
        });
    }

    fn corrupt_derived_index(store: &Path) {
        let root = store.join("derived/indexes/MATRIX_PRIVATE_INDEX");
        fs::create_dir(&root).expect("materialize forged disposable index");
        let index = root.join("index-manifest.json");
        fs::write(index, b"MATRIX_PRIVATE_INDEX\n").expect("corrupt disposable derived index");
    }

    fn staged_proposal_root(store: &Path) -> std::path::PathBuf {
        fs::read_dir(store.join("intake/proposals"))
            .expect("read staged proposal shards")
            .flatten()
            .find_map(|shard| {
                fs::read_dir(shard.path())
                    .ok()
                    .and_then(|entries| entries.flatten().next().map(|entry| entry.path()))
            })
            .expect("fixture retains one staged proposal")
    }

    fn only_transaction_journal(store: &Path) -> std::path::PathBuf {
        fs::read_dir(store.join("transactions"))
            .expect("read transaction evidence")
            .flatten()
            .find_map(|entry| {
                fs::read_dir(entry.path())
                    .ok()
                    .and_then(|entries| entries.flatten().next().map(|entry| entry.path()))
            })
            .expect("interrupted transaction retains a journal")
    }

    fn clone_matrix_store(source: &Path, name: &str) -> std::path::PathBuf {
        let destination = source.with_file_name(format!("matrix-clone-{name}"));
        assert!(
            !destination.exists(),
            "matrix clone destination must be unique: {}",
            destination.display()
        );
        copy_matrix_tree(source, &destination);
        destination
    }

    fn copy_matrix_tree(source: &Path, destination: &Path) {
        let metadata = fs::symlink_metadata(source).expect("read matrix clone source metadata");
        assert!(
            !metadata.file_type().is_symlink(),
            "matrix clone source cannot contain a symlink"
        );
        if metadata.is_file() {
            fs::write(
                destination,
                fs::read(source).expect("read matrix clone file"),
            )
            .expect("write matrix clone file");
            return;
        }
        fs::create_dir(destination).expect("create matrix clone directory");
        let mut entries = fs::read_dir(source)
            .expect("read matrix clone directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect matrix clone entries");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_matrix_tree(&entry.path(), &destination.join(entry.file_name()));
        }
    }

    fn assert_no_diagnostic_sentinel(value: &str, sentinel: &str) {
        assert!(
            !value.contains(sentinel),
            "stable matrix diagnostic leaked a private sentinel"
        );
    }

    fn malformed_json(_bytes: Vec<u8>) -> Vec<u8> {
        b"{MATRIX_PRIVATE_MALFORMED\n".to_vec()
    }

    fn noncanonical_json(mut bytes: Vec<u8>) -> Vec<u8> {
        bytes.extend_from_slice(b" \n");
        bytes
    }

    fn unknown_json_field(bytes: Vec<u8>) -> Vec<u8> {
        let mut text = String::from_utf8(bytes).expect("fixture schema bytes are UTF-8");
        let closing = text.rfind('}').expect("fixture JSON has closing object");
        text.insert_str(closing, ",\"matrix_unknown\":\"MATRIX_PRIVATE_UNKNOWN\"");
        text.into_bytes()
    }

    fn corrupt_reindex_receipt(store: &Path) {
        let path = fs::read_dir(store.join("evidence/reindex"))
            .expect("read reindex evidence")
            .next()
            .expect("fixture reindex receipt")
            .expect("read receipt entry")
            .path();
        let mut bytes = fs::read(&path).expect("read receipt bytes");
        bytes[0] ^= 1;
        bytes.extend_from_slice(b"MATRIX_PRIVATE_LEVEL4_RECEIPT");
        fs::write(path, bytes).expect("corrupt receipt bytes");
    }

    fn corrupt_policy_archive(store: &Path) {
        let path = fs::read_dir(store.join("policies"))
            .expect("read policy archives")
            .next()
            .expect("fixture policy archive")
            .expect("read policy archive entry")
            .path();
        fs::write(path, b"MATRIX_PRIVATE_LEVEL4_POLICY\n").expect("corrupt policy archive");
    }

    fn corrupt_extra_generation(store: &Path) {
        fs::create_dir(store.join("canon/generations/gen-0000000000000002"))
            .expect("create incomplete extra generation");
        fs::write(
            store.join("canon/generations/gen-0000000000000002/MATRIX_PRIVATE_LEVEL4_GENERATION"),
            b"corruption",
        )
        .expect("write extra generation sentinel");
    }

    fn corrupt_verdict_lease(store: &Path) {
        let lease = store.join("locks/verdict.lock");
        fs::create_dir(&lease).expect("create visible verdict lease");
        fs::write(lease.join("owner.json"), b"MATRIX_PRIVATE_LEVEL4_LEASE\n")
            .expect("write visible verdict lease");
    }

    fn corrupt_lease_preparation(store: &Path) {
        fs::create_dir(
            store
                .join("locks")
                .join(format!(".lease-verdict-{}", "a".repeat(64))),
        )
        .expect("create visible lease preparation");
        fs::write(
            store
                .join("locks")
                .join(format!(".lease-verdict-{}", "a".repeat(64)))
                .join("owner.json"),
            b"MATRIX_PRIVATE_LEVEL4_PREPARATION\n",
        )
        .expect("write visible lease preparation");
    }

    fn corrupt_ledger_event_hash(store: &Path) {
        let ledger = store.join("ledger/verdicts.jsonl");
        let mut bytes = fs::read(&ledger).expect("read fixture ledger");
        let prefix = b"\"candidate_hash\":\"";
        let offset = bytes
            .windows(prefix.len())
            .position(|window| window == prefix)
            .expect("fixture ledger contains candidate hash")
            + prefix.len();
        bytes[offset] = if bytes[offset] == b'a' { b'b' } else { b'a' };
        fs::write(ledger, bytes).expect("corrupt committed ledger event");
    }

    fn read_canonical_events(store: &Path) -> Vec<LedgerEvent> {
        read_ledger(&store.join("ledger/verdicts.jsonl"))
            .expect("read canonical fixture ledger")
            .events
    }

    fn write_canonical_events(store: &Path, events: &[LedgerEvent]) {
        let bytes = events
            .iter()
            .flat_map(|event| {
                event
                    .canonical_line_bytes()
                    .expect("render canonical event")
            })
            .collect::<Vec<_>>();
        fs::write(store.join("ledger/verdicts.jsonl"), bytes)
            .expect("write canonical ledger tamper");
    }

    fn replace_applied_event(store: &Path, edit: impl FnOnce(&mut VerdictPayload)) {
        let mut events = read_canonical_events(store);
        let LedgerEvent::Verdict(event) = events
            .last()
            .expect("fixture chain has an applied terminal event")
        else {
            panic!("fixture chain ends in a verdict");
        };
        let mut payload = event.payload().clone();
        edit(&mut payload);
        *events.last_mut().expect("replace applied event") = LedgerEvent::Verdict(Box::new(
            VerdictEvent::new(payload).expect("rebuild canonical applied tamper"),
        ));
        write_canonical_events(store, &events);
    }

    fn remove_genesis(store: &Path) {
        let events = read_canonical_events(store);
        write_canonical_events(store, &events[1..]);
    }

    fn duplicate_genesis(store: &Path) {
        let events = read_canonical_events(store);
        let LedgerEvent::Genesis(genesis) = &events[0] else {
            panic!("fixture chain begins with genesis");
        };
        let mut duplicate = genesis.payload().clone();
        duplicate.sequence = 1;
        duplicate.event_id = "evt-0000000000000001".to_owned();
        duplicate.previous_event_hash = Some(genesis.hash().to_owned());
        let duplicate = LedgerEvent::Genesis(Box::new(
            GenesisEvent::new(duplicate).expect("rebuild canonical duplicate genesis"),
        ));
        write_canonical_events(store, &[events[0].clone(), duplicate]);
    }

    fn duplicate_terminal_proposal(store: &Path) {
        let mut events = read_canonical_events(store);
        let LedgerEvent::Verdict(last) = events.last().expect("fixture chain has terminal verdict")
        else {
            panic!("fixture chain ends in a verdict");
        };
        let mut duplicate = last.payload().clone();
        duplicate.sequence = 3;
        duplicate.event_id = "evt-0000000000000003".to_owned();
        duplicate.previous_event_hash = Some(last.hash().to_owned());
        events.push(LedgerEvent::Verdict(Box::new(
            VerdictEvent::new(duplicate).expect("rebuild canonical duplicate verdict"),
        )));
        write_canonical_events(store, &events);
    }

    fn corrupt_confidence_binding(store: &Path) {
        replace_applied_event(store, |payload| {
            payload.confidence =
                Some(Confidence::parse("0.500000").expect("valid test confidence"));
        });
    }

    fn corrupt_evaluator_binding(store: &Path) {
        replace_applied_event(store, |payload| {
            payload.evaluator_meta_hash = Some("a".repeat(64));
        });
    }

    fn corrupt_head_transition(store: &Path) {
        let prior_reject_hash = read_canonical_events(store)[1].hash().to_owned();
        let replacement = replace_canonical_event_field(
            store,
            1,
            b"\"resulting_canonical_head\":\"seed\"",
            b"\"resulting_canonical_head\":\"gen-0000000000000001\"",
        );
        replace_canonical_event_field(
            store,
            2,
            format!("\"previous_event_hash\":\"{prior_reject_hash}\"").as_bytes(),
            format!("\"previous_event_hash\":\"{replacement}\"").as_bytes(),
        );
    }

    fn corrupt_verb_semantics(store: &Path) {
        let prior_reject_hash = read_canonical_events(store)[1].hash().to_owned();
        let replacement =
            replace_canonical_event_field(store, 1, b"\"verb\":\"REJECT\"", b"\"verb\":\"ACCEPT\"");
        replace_canonical_event_field(
            store,
            2,
            format!("\"previous_event_hash\":\"{prior_reject_hash}\"").as_bytes(),
            format!("\"previous_event_hash\":\"{replacement}\"").as_bytes(),
        );
    }

    fn corrupt_reason(store: &Path) {
        replace_canonical_event_field(
            store,
            2,
            b"\"reason\":\"fixture applied accept\"",
            b"\"reason\":\" \\t\\n\"",
        );
    }

    fn corrupt_judgment_binding(store: &Path) {
        replace_applied_event(store, |payload| {
            payload.canonical_result.judgment_hash = Some("b".repeat(64));
        });
    }

    fn corrupt_manifest_binding(store: &Path) {
        replace_applied_event(store, |payload| {
            payload.canonical_result.manifest_hash = Some("c".repeat(64));
        });
    }

    fn corrupt_bundle_binding(store: &Path) {
        replace_applied_event(store, |payload| {
            payload.canonical_result.bundle_hash = Some("d".repeat(64));
        });
    }

    fn corrupt_generation_chain(store: &Path) {
        replace_applied_event(store, |payload| {
            payload.canonical_result.target_prior_hash = Some("e".repeat(64));
        });
    }

    fn replace_canonical_event_field(
        store: &Path,
        line_index: usize,
        needle: &[u8],
        replacement: &[u8],
    ) -> String {
        let ledger = store.join("ledger/verdicts.jsonl");
        let bytes = fs::read(&ledger).expect("read ledger for canonical tamper");
        let mut lines = bytes
            .strip_suffix(b"\n")
            .expect("canonical ledger has terminal LF")
            .split(|byte| *byte == b'\n')
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let line = lines
            .get_mut(line_index)
            .expect("fixture ledger contains requested event line");
        let event_hash_start = line
            .windows(b",\"event_hash\":\"".len())
            .rposition(|window| window == b",\"event_hash\":\"")
            .expect("canonical event has a terminal event hash");
        let mut payload = line[..event_hash_start].to_vec();
        let offset = payload
            .windows(needle.len())
            .position(|window| window == needle)
            .expect("canonical event contains asserted field");
        payload.splice(offset..offset + needle.len(), replacement.iter().copied());
        payload.push(b'}');
        let hash = sovmem::profile::event_hash(&payload);
        payload.pop();
        payload.extend_from_slice(b",\"event_hash\":\"");
        payload.extend_from_slice(hash.as_bytes());
        payload.extend_from_slice(b"\"}");
        *line = payload;
        let mut rewritten = lines.join(&b'\n');
        rewritten.push(b'\n');
        fs::write(ledger, rewritten).expect("write canonical event tamper");
        hash
    }

    impl MatrixObservation {
        fn code(code: String, before: DurableSnapshot, after: DurableSnapshot) -> Self {
            Self {
                code: Some(code),
                cli_exit: None,
                before,
                after,
                redacted_input: None,
                redacted_projection: None,
            }
        }

        fn redacted(
            code: String,
            before: DurableSnapshot,
            after: DurableSnapshot,
            redacted_input: String,
            redacted_projection: String,
        ) -> Self {
            Self {
                code: Some(code),
                cli_exit: None,
                before,
                after,
                redacted_input: Some(redacted_input),
                redacted_projection: Some(redacted_projection),
            }
        }
    }

    fn matrix_submission(
        fixture: &sovmem::profile::p3_internal::p3::P3Fixture,
        candidate_path: impl AsRef<Path>,
        evaluator_path: Option<std::path::PathBuf>,
    ) -> ProposalSubmission {
        ProposalSubmission {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            source_id: "src:agent:fixture".to_owned(),
            operation: ProposalOperation::Create,
            intent: ProposalIntent::Accept,
            target_id: "ssa-0/04-Research/CLI/README.md".to_owned(),
            target_path: "04-Research/CLI/README.md".to_owned(),
            candidate_path: candidate_path.as_ref().to_owned(),
            confidence: None,
            evaluator_path,
        }
    }

    fn receipt_projection(fixture: &sovmem::profile::p3_internal::p3::P3Fixture) -> String {
        let events = fs::read_to_string(fixture.store_root.join("intake/events.jsonl"))
            .expect("read protected intake rejection projection");
        assert_eq!(
            events.lines().count(),
            1,
            "matrix rejection retains one protected event"
        );
        events
    }

    fn assert_no_partial_submission_or_protected_pairs(store_root: &Path, family: &str) {
        let inbox = store_root.join("staging/inbox");
        for entry in fs::read_dir(&inbox)
            .expect("read untrusted inbox")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect untrusted inbox")
        {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with(".submission-") {
                continue;
            }
            let submission = entry.path();
            let mut names = fs::read_dir(&submission)
                .expect("read visible inbox submission")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect visible inbox submission")
                .into_iter()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            names.sort();
            assert_eq!(
                names,
                ["candidate.md", "submission.meta.json"],
                "{family} exposed a partial visible inbox submission at {}",
                submission.display()
            );
        }

        assert_no_partial_protected_pairs(store_root, family);
    }

    fn assert_no_partial_protected_pairs(store_root: &Path, family: &str) {
        let proposals = store_root.join("intake/proposals");
        for shard in fs::read_dir(&proposals)
            .expect("read proposal shards")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect proposal shards")
        {
            let path = shard.path();
            let name = shard.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".proposal-") {
                continue;
            }
            if !path.is_dir() {
                continue;
            }
            for candidate in fs::read_dir(&path)
                .expect("read proposal shard")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect proposal shard")
            {
                let pair = candidate.path();
                let names = fs::read_dir(&pair)
                    .expect("read visible proposal pair")
                    .collect::<Result<Vec<_>, _>>()
                    .expect("collect visible proposal pair")
                    .into_iter()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                assert_eq!(
                    names.len(),
                    2,
                    "{family} exposed a partial visible proposal pair at {}",
                    pair.display()
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn durable_snapshot_commits_empty_directories_link_kind_and_link_identity() {
        use std::os::unix::fs::symlink;
        use std::os::unix::net::UnixListener;
        use std::process::Command;

        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("matrix-durable-snapshot-topology");
        let baseline = snapshot(&fixture.store_root);

        fs::create_dir(
            fixture
                .store_root
                .join("transactions/empty-recovery-evidence"),
        )
        .expect("create empty durable directory");
        let with_empty_directory = snapshot(&fixture.store_root);
        assert_ne!(
            baseline, with_empty_directory,
            "empty directories are durable state"
        );

        let symlink_path = fixture.store_root.join("transactions/link-evidence");
        symlink("../ledger/verdicts.jsonl", &symlink_path).expect("create first symlink");
        let first_link = snapshot(&fixture.store_root);
        fs::remove_file(&symlink_path).expect("remove first symlink");
        symlink("../canon/CURRENT", &symlink_path).expect("create second symlink");
        let second_link = snapshot(&fixture.store_root);
        assert_ne!(
            first_link, second_link,
            "symlink target identity is committed"
        );

        fs::remove_file(&symlink_path).expect("remove second symlink");
        let original = fixture.store_root.join("ledger/verdicts.jsonl");
        let hard_link = fixture.store_root.join("ledger/verdicts-hard-link");
        fs::hard_link(&original, &hard_link).expect("create hard link");
        let with_hard_link = snapshot(&fixture.store_root);
        assert_ne!(
            with_empty_directory, with_hard_link,
            "hard-link multiplicity and entry identity are committed"
        );

        let fifo = fixture.store_root.join("transactions/fifo-evidence");
        let fifo_created = Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if fifo_created {
            let fifo_snapshot = snapshot(&fixture.store_root);
            assert_ne!(
                with_hard_link, fifo_snapshot,
                "FIFO kind is committed distinctly"
            );
            assert_eq!(
                special_file_commitment(&fs::symlink_metadata(&fifo).unwrap()),
                "special:fifo"
            );
            fs::remove_file(&fifo).expect("remove FIFO evidence");
        }

        // Darwin's `sun_path` is shorter than this fixture's intentionally long isolated root.
        // The snapshot uses this exact file-type commitment when a socket is encountered below a
        // store; materialize the socket under the short system temporary root to exercise its kind.
        let socket =
            std::env::temp_dir().join(format!("sovmem-p3-matrix-{}-socket", std::process::id()));
        match UnixListener::bind(&socket) {
            Ok(listener) => {
                assert_eq!(
                    special_file_commitment(&fs::symlink_metadata(&socket).unwrap()),
                    "special:socket"
                );
                drop(listener);
                fs::remove_file(socket).expect("remove socket evidence");
            }
            Err(error) => assert!(
                matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied
                        | std::io::ErrorKind::InvalidInput
                        | std::io::ErrorKind::Unsupported
                ),
                "socket fixture must either materialize or report an explicit unsupported capability: {error}"
            ),
        }
    }

    fn snapshot(root: &Path) -> DurableSnapshot {
        let mut entries = BTreeMap::new();
        collect_durable_entries(root, root, &mut entries);
        let mut root_bytes = Vec::new();
        for (path, commitment) in entries {
            root_bytes.extend_from_slice(path.as_bytes());
            root_bytes.push(0);
            root_bytes.extend_from_slice(commitment.as_bytes());
            root_bytes.push(b'\n');
        }
        DurableSnapshot {
            root_hash: blake3::hash(&root_bytes).to_hex().to_string(),
            ledger_hash: blake3::hash(&fs::read(root.join("ledger/verdicts.jsonl")).unwrap())
                .to_hex()
                .to_string(),
        }
    }

    fn durable_tree_hash(root: &Path) -> String {
        let mut entries = BTreeMap::new();
        collect_durable_entries(root, root, &mut entries);
        let mut bytes = Vec::new();
        for (path, commitment) in entries {
            bytes.extend_from_slice(path.as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(commitment.as_bytes());
            bytes.push(b'\n');
        }
        blake3::hash(&bytes).to_hex().to_string()
    }

    fn collect_durable_entries(
        root: &Path,
        current: &Path,
        durable_entries: &mut BTreeMap<String, String>,
    ) {
        let mut children = fs::read_dir(current)
            .expect("read durable snapshot directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read durable snapshot entry");
        children.sort_by_key(|entry| entry.file_name());
        for entry in children {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("read durable snapshot metadata");
            let relative = path
                .strip_prefix(root)
                .expect("fixture path stays beneath root")
                .to_string_lossy()
                .replace('\\', "/");
            if metadata.is_dir() {
                // Empty directories, including recovery witnesses, are durable evidence too.
                durable_entries.insert(relative, "directory".to_owned());
                collect_durable_entries(root, &path, durable_entries);
            } else if metadata.file_type().is_symlink() {
                let target = fs::read_link(&path)
                    .expect("read durable snapshot symlink")
                    .to_string_lossy()
                    .replace('\\', "/");
                durable_entries.insert(relative, format!("symlink:{target}"));
            } else if metadata.is_file() {
                let bytes = fs::read(&path).expect("read durable snapshot file");
                let content_hash = blake3::hash(&bytes).to_hex().to_string();
                #[cfg(unix)]
                let commitment = {
                    use std::os::unix::fs::MetadataExt;
                    format!("regular:nlink={}:blake3={content_hash}", metadata.nlink())
                };
                #[cfg(not(unix))]
                let commitment = format!("regular:blake3={content_hash}");
                durable_entries.insert(relative, commitment);
            } else {
                durable_entries.insert(relative, special_file_commitment(&metadata));
            }
        }
    }

    #[cfg(unix)]
    fn special_file_commitment(metadata: &fs::Metadata) -> String {
        use std::os::unix::fs::FileTypeExt;

        let file_type = metadata.file_type();
        let kind = if file_type.is_fifo() {
            "fifo"
        } else if file_type.is_socket() {
            "socket"
        } else if file_type.is_char_device() {
            "char-device"
        } else if file_type.is_block_device() {
            "block-device"
        } else {
            "special-unknown"
        };
        format!("special:{kind}")
    }

    #[cfg(not(unix))]
    fn special_file_commitment(_metadata: &fs::Metadata) -> String {
        "special:unsupported-platform".to_owned()
    }
}

pub mod policy_tests {

    use sovmem::profile::p3_test_support::ProvisionedP3Fixture;
    use sovmem::profile::{
        append_intake_event, archive_policy, inspect_store, intake_prefix, load_p3_profile,
        load_policy, prepare_proposal, proposal_status, publish_sealed_pair, read_intake,
        read_sealed_pair_structural, verdict_id, verify_store_level4, CanonicalResult,
        GenesisEvent, GenesisPayload, IntakeEvent, IntakeHead, IntakeLeaseIdentity, IntakeOutcome,
        IntakePayload, IntakePrefix, IntakePublicationLease, IntakeRejection, LedgerEvent,
        PolicyContext, PolicyEvaluator, ProposalBase, ProposalDraft, ProposalIntent, ProposalMeta,
        ProposalOperation, ProposalPair, ProposalState, ProposalSummary, Reason, StorePaths,
        Timestamp, VerdictEvent, VerdictIdentityInput, VerdictPayload, VerdictVerb,
        INTAKE_EVENT_SCHEMA, LEDGER_EVENT_SCHEMA, P3_PROFILE_VERSION, P3_STORE_ID,
    };
    use std::fs;
    use std::path::Path;

    fn repository_path(relative: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(relative)
    }

    #[test]
    fn loads_the_exact_versioned_profile_and_policy() {
        let profile = load_p3_profile(&repository_path("profiles/audy-ssa0-p3-v2/profile.yaml"))
            .expect("load P3 profile");
        let policy = load_policy(&repository_path("profiles/audy-ssa0-p3-v2/policy.toml"))
            .expect("load P3 policy");

        assert_eq!(profile.version, P3_PROFILE_VERSION);
        assert_eq!(policy.policy.store_id, P3_STORE_ID);
        assert_eq!(policy.policy.profile_version, P3_PROFILE_VERSION);
        assert_eq!(profile.hash.len(), 64);
        assert_eq!(policy.hash.len(), 64);
        assert!(profile.config.context_kinds.contains_key("Human Judgment"));
        assert_eq!(
            policy.policy.provenance_values,
            ["tier1", "secondary", "synthesis", "unknown"]
        );
    }

    #[test]
    fn rejects_noncanonical_policy_bytes_before_evaluation() {
        for (name, bytes, code) in [
            (
                "unknown",
                b"unknown = true\n".as_slice(),
                "policy.unknown_field",
            ),
            (
                "crlf",
                b"schema_version = \"sovmem-policy-v1\"\r\n".as_slice(),
                "policy.line_endings",
            ),
            (
                "comment",
                b"schema_version = \"sovmem-policy-v1\" # no\n".as_slice(),
                "policy.syntax",
            ),
            (
                "no-final-lf",
                b"schema_version = \"sovmem-policy-v1\"".as_slice(),
                "policy.line_endings",
            ),
        ] {
            let error = sovmem::profile::parse_policy(bytes).expect_err(name);
            assert_eq!(error.code(), code, "{name}");
        }
    }

    #[test]
    fn rejects_key_order_and_policy_invariant_violations() {
        let policy = std::fs::read(repository_path("profiles/audy-ssa0-p3-v2/policy.toml"))
            .expect("read reviewed policy");
        let policy = String::from_utf8(policy).expect("reviewed policy is UTF-8");
        let cases = [
            (
                "key order",
                policy.replacen(
                    "schema_version = \"sovmem-policy-v1\"\nstore_id = \"audy-sovmem-writeback-v2\"",
                    "store_id = \"audy-sovmem-writeback-v2\"\nschema_version = \"sovmem-policy-v1\"",
                    1,
                ),
                "policy.key_order",
            ),
            (
                "duplicate key",
                policy.replacen(
                    "store_id = \"audy-sovmem-writeback-v2\"",
                    "schema_version = \"sovmem-policy-v1\"",
                    1,
                ),
                "policy.duplicate_key",
            ),
            (
                "missing key",
                policy.replacen("human_verdict_cost = 1\n", "", 1),
                "policy.missing_field",
            ),
            (
                "unsupported schema",
                policy.replacen("sovmem-policy-v1", "sovmem-policy-v2", 1),
                "policy.schema_unsupported",
            ),
            (
                "invalid policy target glob",
                policy.replacen(
                    "[\"01-Soul/*.md\", \"03-Protocols/*.md\", \"04-Research/**/*.md\"]",
                    "[\"../outside/*.md\"]",
                    1,
                ),
                "policy.value_invalid",
            ),
            (
                "queue beyond limit",
                policy.replacen("queue_alarm_at = 25", "queue_alarm_at = 51", 1),
                "policy.invariant_invalid",
            ),
        ];

        for (name, bytes, code) in cases {
            let error = sovmem::profile::parse_policy(bytes.as_bytes()).expect_err(name);
            assert_eq!(error.code(), code, "{name}");
        }

        let error = sovmem::profile::parse_policy(b"\xff\n").expect_err("non UTF-8 is rejected");
        assert_eq!(error.code(), "policy.utf8");
    }

    #[test]
    fn pair_structure_stops_evaluation_before_source_and_target_rules() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("policy-stage-order");
        let profile = load_p3_profile(&fixture.profile_path).expect("load P3 profile");
        let store = inspect_store(&profile, &fixture.store_root).expect("inspect P3 store");
        let proposal = ProposalPair {
            meta: ProposalMeta {
                schema_version: "unsupported".to_owned(),
                proposal_id: "not-a-proposal-id".to_owned(),
                submitted_at: "invalid".to_owned(),
                source_id: "unsafe source".to_owned(),
                operation: ProposalOperation::Create,
                intent: ProposalIntent::Accept,
                target_id: "unsafe target".to_owned(),
                target_path: "../outside.md".to_owned(),
                base: ProposalBase {
                    kind: "invalid".to_owned(),
                    generation_id: None,
                    bundle_hash: "invalid".to_owned(),
                    manifest_hash: "invalid".to_owned(),
                    target_hash: None,
                },
                candidate_hash: "invalid".to_owned(),
                candidate_bytes: 0,
                policy_hash: "invalid".to_owned(),
                profile_hash: "invalid".to_owned(),
                confidence: None,
                evaluator: None,
            },
            meta_bytes: b"{}\n".to_vec(),
            candidate_bytes: b"not markdown".to_vec(),
            meta_hash: "invalid".to_owned(),
            candidate_hash: "invalid".to_owned(),
        };
        let context = PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &proposal,
            intake: &policy_prefix(Vec::new(), Vec::new()),
        };

        assert_eq!(
            PolicyEvaluator::evaluate(&context).rule_codes,
            ["proposal.meta_schema"]
        );
    }

    #[test]
    fn intake_v1_receipt_removal_cannot_change_a_quota_decision() {
        let fixture = ProvisionedP3Fixture::empty("policy-v1-receipt-removal");
        let (profile, store, pair) = valid_policy_pair(&fixture, "2026-07-21T04:30:00.000000000Z");
        let receipts_root = store.paths.staging.join("intake-rejections");
        fs::create_dir_all(&receipts_root).unwrap();
        let mut protected_rejections = Vec::new();
        for index in 0..store.policy.policy.max_submissions_per_source_utc_day {
            let receipt = IntakeRejection {
                schema_version: "sovmem-intake-rejection-v1".to_owned(),
                receipt_id: format!("rec-{index:064x}"),
                received_at: "2026-07-21T03:00:00.000000000Z".to_owned(),
                source_id: Some(pair.meta.source_id.clone()),
                rule_codes: vec!["proposal.provenance_required".to_owned()],
            };
            let mut bytes = serde_json::to_vec_pretty(&receipt).unwrap();
            bytes.push(b'\n');
            fs::write(
                receipts_root.join(format!("{}.json", receipt.receipt_id)),
                bytes,
            )
            .unwrap();
            protected_rejections.push(receipt);
        }
        let protected = policy_prefix(Vec::new(), protected_rejections);
        let before = PolicyEvaluator::evaluate(&PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &pair,
            intake: &protected,
        });
        assert_eq!(before.rule_codes, ["proposal.quota_source_daily"]);

        for entry in fs::read_dir(&receipts_root).unwrap() {
            fs::remove_file(entry.unwrap().path()).unwrap();
        }
        assert!(fs::read_dir(&receipts_root).unwrap().next().is_none());
        let after = PolicyEvaluator::evaluate(&PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &pair,
            intake: &protected,
        });

        assert_eq!(
            after, before,
            "deleting mutable v1 receipts reset the quota"
        );
    }

    #[test]
    fn intake_later_sealed_item_cannot_change_an_earlier_prefix_decision() {
        let fixture = ProvisionedP3Fixture::empty("policy-earlier-prefix");
        let (profile, store, pair) = valid_policy_pair(&fixture, "2026-07-21T04:30:00.000000000Z");
        let before = read_intake(&store.paths).unwrap();
        let earlier_head = before.head().clone();
        let earlier_prefix = intake_prefix(&before, &earlier_head).unwrap();
        let earlier = PolicyEvaluator::evaluate(&PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &pair,
            intake: &earlier_prefix,
        });
        let mut later = pair.clone();
        later.meta.submitted_at = "2026-07-21T04:31:00.000000000Z".to_owned();
        later.meta.source_id = "src:agent:later-sealed".to_owned();
        later.meta.proposal_id = sovmem::profile::proposal_id(&later.meta);
        later.meta_bytes = serde_json::to_vec(&later.meta).unwrap();
        later.meta_bytes.push(b'\n');
        later.meta_hash = blake3::hash(&later.meta_bytes).to_hex().to_string();
        let mut lease = intake_lease(&store.paths, "policy-earlier-prefix");
        publish_sealed_pair(&store.paths, &mut lease, &later).unwrap();
        let event = IntakeEvent::new(staged_payload(&later, 1, None)).unwrap();
        append_intake_event(&store.paths, &mut lease, &event).unwrap();
        lease.release(&store.paths).unwrap();
        let after = read_intake(&store.paths).unwrap();
        let full_prefix = intake_prefix(&after, after.head()).unwrap();
        let replayed_prefix = intake_prefix(&after, &earlier_head).unwrap();
        assert_eq!(full_prefix.staged.len(), 1);
        assert_eq!(
            PolicyEvaluator::evaluate(&PolicyContext {
                policy: &store.policy,
                profile: &profile,
                store: &store,
                proposal: &pair,
                intake: &full_prefix,
            })
            .rule_codes,
            ["proposal.duplicate_content"]
        );
        let replayed = PolicyEvaluator::evaluate(&PolicyContext {
            policy: &store.policy,
            profile: &profile,
            store: &store,
            proposal: &pair,
            intake: &replayed_prefix,
        });

        assert_eq!(replayed, earlier, "a later mutable pair changed replay");
    }

    #[test]
    fn intake_forged_rejected_event_fails_before_policy_evaluation() {
        let fixture = ProvisionedP3Fixture::empty("policy-forged-rejection");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = inspect_store(&profile, &fixture.store_root).unwrap();
        let event = IntakeEvent::new(rejected_payload(
            &store.paths,
            &profile.hash,
            &store.policy.hash,
            1,
            "2026-07-21T05:00:00.000000000Z",
            None,
        ))
        .unwrap();
        let mut bytes = event.canonical_line_bytes();
        let offset = bytes
            .windows(b"proposal.provenance_required".len())
            .position(|window| window == b"proposal.provenance_required")
            .unwrap();
        bytes[offset] = b'P';
        fs::write(&store.paths.intake_events, bytes).unwrap();

        let error = read_intake(&store.paths).unwrap_err();
        assert_eq!(error.code(), "intake.record_invalid");
    }

    #[test]
    fn intake_equal_timestamps_use_sequence_for_policy_prefix_order() {
        let fixture = ProvisionedP3Fixture::empty("policy-equal-timestamps");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = inspect_store(&profile, &fixture.store_root).unwrap();
        let mut lease = intake_lease(&store.paths, "policy-equal-timestamps");
        let occurred_at = "2026-07-21T05:00:00.000000000Z";
        let first = IntakeEvent::new(rejected_payload(
            &store.paths,
            &profile.hash,
            &store.policy.hash,
            1,
            occurred_at,
            None,
        ))
        .unwrap();
        append_intake_event(&store.paths, &mut lease, &first).unwrap();
        let second = IntakeEvent::new(rejected_payload(
            &store.paths,
            &profile.hash,
            &store.policy.hash,
            2,
            occurred_at,
            Some(first.hash().to_owned()),
        ))
        .unwrap();
        append_intake_event(&store.paths, &mut lease, &second).unwrap();

        let view = read_intake(&store.paths).unwrap();
        let prefix = intake_prefix(&view, view.head()).unwrap();
        assert_eq!(
            prefix
                .rejected
                .iter()
                .map(|receipt| receipt.receipt_id.as_str())
                .collect::<Vec<_>>(),
            ["int-0000000000000001", "int-0000000000000002"]
        );
    }

    #[test]
    fn intake_backward_time_creates_no_policy_event() {
        let fixture = ProvisionedP3Fixture::empty("policy-backward-time");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = inspect_store(&profile, &fixture.store_root).unwrap();
        let mut lease = intake_lease(&store.paths, "policy-backward-time");
        let first = IntakeEvent::new(rejected_payload(
            &store.paths,
            &profile.hash,
            &store.policy.hash,
            1,
            "2026-07-21T05:00:00.000000000Z",
            None,
        ))
        .unwrap();
        append_intake_event(&store.paths, &mut lease, &first).unwrap();
        let before = fs::read(&store.paths.intake_events).unwrap();
        let backward = IntakeEvent::new(rejected_payload(
            &store.paths,
            &profile.hash,
            &store.policy.hash,
            2,
            "2026-07-21T04:59:59.999999999Z",
            Some(first.hash().to_owned()),
        ))
        .unwrap();
        assert_eq!(
            append_intake_event(&store.paths, &mut lease, &backward)
                .unwrap_err()
                .code(),
            "intake.clock_regression"
        );
        assert_eq!(fs::read(&store.paths.intake_events).unwrap(), before);
    }

    #[test]
    fn intake_historical_pair_replays_under_archived_meta_limit_after_policy_rotation() {
        let fixture = ProvisionedP3Fixture::empty("policy-historical-meta-limit");
        let (profile, store, ruled) = valid_policy_pair(&fixture, "2026-07-21T05:10:00.000000000Z");
        let (_, _, mut open) = valid_policy_pair(&fixture, "2026-07-21T05:11:00.000000000Z");
        open.meta.source_id = "src:agent:policy-open".to_owned();
        open.candidate_bytes
            .extend_from_slice(b"\nTask 4 open policy-rotation marker.\n");
        open.candidate_hash = blake3::hash(&open.candidate_bytes).to_hex().to_string();
        open.meta.candidate_hash = open.candidate_hash.clone();
        open.meta.candidate_bytes = open.candidate_bytes.len();
        open.meta.proposal_id = sovmem::profile::proposal_id(&open.meta);
        open.meta_bytes = serde_json::to_vec(&open.meta).unwrap();
        open.meta_bytes.push(b'\n');
        open.meta_hash = blake3::hash(&open.meta_bytes).to_hex().to_string();

        let mut lease = intake_lease(&store.paths, "policy-historical-meta-limit");
        publish_sealed_pair(&store.paths, &mut lease, &ruled).unwrap();
        let first = IntakeEvent::new(staged_payload(&ruled, 1, None)).unwrap();
        append_intake_event(&store.paths, &mut lease, &first).unwrap();
        publish_sealed_pair(&store.paths, &mut lease, &open).unwrap();
        let second =
            IntakeEvent::new(staged_payload(&open, 2, Some(first.hash().to_owned()))).unwrap();
        append_intake_event(&store.paths, &mut lease, &second).unwrap();
        lease.release(&store.paths).unwrap();
        let intake = read_intake(&store.paths).unwrap();
        let intake_sequence = intake.head().sequence;
        let intake_head_hash = intake
            .head()
            .event_hash
            .clone()
            .expect("staged fixture has a nonempty intake head");

        archive_policy(&store.paths.policies, &store.policy).unwrap();
        let genesis = GenesisEvent::new(GenesisPayload {
            schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "genesis".to_owned(),
            sequence: 0,
            event_id: "evt-0000000000000000".to_owned(),
            created_at: "2026-07-21T05:12:00.000000000Z".to_owned(),
            previous_event_hash: None,
            store_id: store.policy.policy.store_id.clone(),
            profile_version: profile.version.clone(),
            profile_hash: profile.hash.clone(),
            policy_hash: store.policy.hash.clone(),
            seed_manifest_hash: store.seed.manifest_hash.clone(),
            seed_corpus_report_hash: store.seed.corpus_report_hash.clone(),
            seed_bundle_hash: store.seed.bundle_hash.clone(),
            source_commit: store.seed.source_commit.clone(),
            initial_canonical_ref: "seed".to_owned(),
        })
        .unwrap();
        let reason = Reason::new(b"Task 4 archived policy replay.", 4096).unwrap();
        let ruled_at = Timestamp::parse("2026-07-21T05:12:00.000000000Z").unwrap();
        let identity = VerdictIdentityInput {
            proposal_id: &ruled.meta.proposal_id,
            verb: VerdictVerb::Reject,
            reason: &reason,
            rewrite: None,
            actor: &store.policy.policy.human_actor,
            ruled_at: &ruled_at,
            prior_ledger_head: Some(genesis.hash()),
            intake_sequence,
            intake_head_hash: Some(&intake_head_hash),
            policy_hash: &store.policy.hash,
            profile_hash: &profile.hash,
        };
        let verdict = VerdictEvent::new(VerdictPayload {
            schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "verdict".to_owned(),
            sequence: 1,
            event_id: "evt-0000000000000001".to_owned(),
            occurred_at: ruled_at.as_str().to_owned(),
            previous_event_hash: Some(genesis.hash().to_owned()),
            store_id: store.policy.policy.store_id.clone(),
            transaction_id: format!("txn-{}", "a".repeat(64)),
            proposal_id: ruled.meta.proposal_id.clone(),
            proposal_meta_hash: ruled.meta_hash.clone(),
            candidate_hash: ruled.candidate_hash.clone(),
            source_id: ruled.meta.source_id.clone(),
            verb: VerdictVerb::Reject,
            reason: reason.as_str().to_owned(),
            verdict_id: verdict_id(&identity),
            actor: store.policy.policy.human_actor.clone(),
            policy_hash: store.policy.hash.clone(),
            profile_hash: profile.hash.clone(),
            confidence: None,
            evaluator_meta_hash: None,
            base_kind: ruled.meta.base.kind.clone(),
            base_generation: ruled.meta.base.generation_id.clone(),
            base_manifest_hash: ruled.meta.base.manifest_hash.clone(),
            base_bundle_hash: ruled.meta.base.bundle_hash.clone(),
            base_target_hash: ruled.meta.base.target_hash.clone(),
            rewrite_hash: None,
            rewrite_bytes: None,
            canonical_result: CanonicalResult::none(),
            resulting_canonical_head: "seed".to_owned(),
            intake_sequence,
            intake_head_hash: Some(intake_head_hash),
        })
        .unwrap();
        let mut ledger = LedgerEvent::Genesis(Box::new(genesis))
            .canonical_line_bytes()
            .unwrap();
        ledger.extend_from_slice(
            &LedgerEvent::Verdict(Box::new(verdict))
                .canonical_line_bytes()
                .unwrap(),
        );
        fs::write(&store.paths.ledger, ledger).unwrap();

        let rotated_bytes = String::from_utf8(store.policy.bytes.clone())
            .unwrap()
            .replacen(
                "max_meta_bytes = 65536",
                &format!("max_meta_bytes = {}", open.meta_bytes.len() - 1),
                1,
            )
            .into_bytes();
        fs::write(&store.paths.policy, rotated_bytes).unwrap();
        let current = load_policy(&store.paths.policy).unwrap();
        let historical =
            read_sealed_pair_structural(&store.paths, &ruled.meta.proposal_id).unwrap();
        assert!(historical.meta_bytes.len() > current.policy.max_meta_bytes);
        if let Err(error) = verify_store_level4(&profile, &fixture.store_root) {
            panic!("historical Level 4 replay failed with {}", error.code());
        }
        let record =
            proposal_status(&profile, &fixture.store_root, &open.meta.proposal_id).unwrap();
        assert_eq!(record.state, ProposalState::Staged);
        assert_eq!(
            serde_json::to_value(&record).unwrap()["policy_stale"],
            serde_json::Value::Bool(true)
        );
    }

    fn valid_policy_pair(
        fixture: &ProvisionedP3Fixture,
        occurred_at: &str,
    ) -> (
        sovmem::profile::P3Profile,
        sovmem::profile::StoreSnapshot,
        ProposalPair,
    ) {
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = inspect_store(&profile, &fixture.store_root).unwrap();
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target = store.paths.seed.join("bundle").join(target_path);
        let candidate = String::from_utf8(fs::read(&target).unwrap())
            .unwrap()
            .replace(
                "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog",
                "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
            )
            .into_bytes();
        let pair = prepare_proposal(ProposalDraft {
            submitted_at: Timestamp::parse(occurred_at).unwrap(),
            source_id: "src:agent:policy-fixture".to_owned(),
            operation: ProposalOperation::Replace,
            intent: ProposalIntent::Accept,
            target_id: "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"
                .to_owned(),
            target_path: target_path.to_owned(),
            base: ProposalBase {
                kind: "seed".to_owned(),
                generation_id: None,
                bundle_hash: store.seed.bundle_hash.clone(),
                manifest_hash: store.seed.manifest_hash.clone(),
                target_hash: Some(
                    blake3::hash(&fs::read(target).unwrap())
                        .to_hex()
                        .to_string(),
                ),
            },
            candidate_bytes: candidate,
            policy_hash: store.policy.hash.clone(),
            profile_hash: profile.hash.clone(),
            confidence: None,
            evaluator: None,
        })
        .unwrap();
        (profile, store, pair)
    }

    fn intake_lease(paths: &StorePaths, owner: &str) -> IntakePublicationLease {
        IntakePublicationLease::acquire(
            paths,
            IntakeLeaseIdentity::new(
                owner,
                "policy-test",
                &Timestamp::parse("2026-07-21T05:00:00.000000000Z").unwrap(),
                std::process::id(),
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn rejected_payload(
        _paths: &StorePaths,
        profile_hash: &str,
        policy_hash: &str,
        sequence: u64,
        occurred_at: &str,
        previous_event_hash: Option<String>,
    ) -> IntakePayload {
        IntakePayload {
            schema_version: INTAKE_EVENT_SCHEMA.to_owned(),
            event_type: "intake".to_owned(),
            sequence,
            event_id: format!("int-{sequence:016}"),
            occurred_at: occurred_at.to_owned(),
            previous_event_hash,
            outcome: IntakeOutcome::REJECTED,
            source_id: Some("src:agent:policy-fixture".to_owned()),
            proposal_id: None,
            proposal_meta_hash: None,
            candidate_hash: None,
            policy_hash: policy_hash.to_owned(),
            profile_hash: profile_hash.to_owned(),
            rule_codes: vec!["proposal.provenance_required".to_owned()],
        }
    }

    fn staged_payload(
        pair: &ProposalPair,
        sequence: u64,
        previous_event_hash: Option<String>,
    ) -> IntakePayload {
        IntakePayload {
            schema_version: INTAKE_EVENT_SCHEMA.to_owned(),
            event_type: "intake".to_owned(),
            sequence,
            event_id: format!("int-{sequence:016}"),
            occurred_at: pair.meta.submitted_at.clone(),
            previous_event_hash,
            outcome: IntakeOutcome::STAGED,
            source_id: Some(pair.meta.source_id.clone()),
            proposal_id: Some(pair.meta.proposal_id.clone()),
            proposal_meta_hash: Some(pair.meta_hash.clone()),
            candidate_hash: Some(pair.candidate_hash.clone()),
            policy_hash: pair.meta.policy_hash.clone(),
            profile_hash: pair.meta.profile_hash.clone(),
            rule_codes: Vec::new(),
        }
    }

    fn policy_prefix(staged: Vec<ProposalSummary>, rejected: Vec<IntakeRejection>) -> IntakePrefix {
        IntakePrefix {
            head: IntakeHead {
                sequence: 0,
                event_hash: None,
            },
            staged,
            rejected,
        }
    }
}

pub mod proposal_tests {
    use sovmem::profile::p3_test_support::{FixedClock, ProvisionedP3Fixture, ScriptedHuman};
    use sovmem::profile::transaction::{
        install_intake_seal_post_precheck_hook_for_test,
        install_intake_seal_pre_cleanup_hook_for_test, seal_intake_submission_with_human,
    };

    use sovmem::profile::{
        append_intake_event, inspect_store, install_intake_event_post_rename_sync_failure_for_test,
        install_intake_event_pre_rename_sync_failure_for_test,
        install_intake_lease_owner_publication_failure_for_test,
        install_intake_lease_release_finalization_failure_for_test,
        install_intake_pair_post_rename_sync_failure_for_test,
        install_intake_promotion_hook_for_test, intake_prefix, load_p3_profile, prepare_proposal,
        proposal_list, proposal_status, publish_sealed_pair, read_inbox_submission, read_intake,
        read_sealed_pair, submit_proposal, submit_proposal_with_clock,
        submit_proposal_with_final_publication_hook, submit_proposal_with_publication_fault,
        verdict_list, Confidence, IntakeEvent, IntakeHead, IntakeLeaseIdentity, IntakeOutcome,
        IntakePayload, IntakePublicationLease, ProposalBase, ProposalDraft, ProposalFilter,
        ProposalIntent, ProposalOperation, ProposalPair, ProposalState, ProposalSubmission,
        PublicationFaultPoint, StorePaths, Timestamp, INTAKE_EVENT_SCHEMA,
    };
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    #[test]
    fn intake_chain_rejects_corruption_and_prefix_mismatches() {
        let fixture = ProvisionedP3Fixture::empty("intake-chain-corruption");
        let paths = intake_paths(&fixture);
        let occurred_at = "2026-07-21T01:02:03.000000000Z";
        let pair = intake_pair(&fixture, "src:agent:first", occurred_at, "first");
        let mut lease = intake_lease(&paths, "chain-corruption");
        publish_sealed_pair(&paths, &mut lease, &pair).expect("publish first sealed pair");
        let first = IntakeEvent::new(staged_intake_payload(&pair, 1, occurred_at, None)).unwrap();
        append_intake_event(&paths, &mut lease, &first).expect("append first intake event");
        let second = IntakeEvent::new(rejected_intake_payload(
            2,
            occurred_at,
            Some(first.hash().to_owned()),
            Some("src:agent:rejected"),
            vec![
                "proposal.duplicate_content".to_owned(),
                "proposal.quota_source_open".to_owned(),
            ],
            pair.meta.policy_hash.clone(),
            pair.meta.profile_hash.clone(),
        ))
        .unwrap();
        append_intake_event(&paths, &mut lease, &second).expect("append rejected intake event");

        let view = read_intake(&paths).expect("read canonical intake chain");
        assert_eq!(view.head().sequence, 2);
        assert_eq!(view.head().event_hash.as_deref(), Some(second.hash()));
        let sealed = read_sealed_pair(&paths, &pair.meta.proposal_id).unwrap();
        assert_eq!(sealed.meta, pair.meta);
        assert_eq!(sealed.meta_bytes, pair.meta_bytes);
        assert_eq!(sealed.candidate_bytes, pair.candidate_bytes);
        assert_eq!(sealed.meta_hash, pair.meta_hash);
        assert_eq!(sealed.candidate_hash, pair.candidate_hash);
        let full = intake_prefix(&view, view.head()).expect("materialize full prefix");
        assert_eq!(full.staged.len(), 1);
        assert_eq!(full.staged[0].proposal_id, pair.meta.proposal_id);
        assert_eq!(full.rejected.len(), 1);
        assert_eq!(full.rejected[0].receipt_id, second.payload().event_id);
        assert_eq!(
            intake_prefix(
                &view,
                &IntakeHead {
                    sequence: 1,
                    event_hash: Some("f".repeat(64)),
                },
            )
            .unwrap_err()
            .code(),
            "intake.head_mismatch"
        );
        lease
            .release(&paths)
            .expect("release source lease before cloning");

        let pair_collision = corruption_store(&fixture, "pair-collision");
        let pair_collision_paths = paths_for_store(&fixture, &pair_collision);
        let mut pair_collision_lease = intake_lease(&pair_collision_paths, "pair-collision");
        let pair_root = sealed_pair_root(&pair_collision_paths, &pair.meta.proposal_id);
        let candidate_before = fs::read(pair_root.join("candidate.md")).unwrap();
        let meta_before = fs::read(pair_root.join("proposal.meta.json")).unwrap();
        assert_eq!(
            publish_sealed_pair(&pair_collision_paths, &mut pair_collision_lease, &pair)
                .unwrap_err()
                .code(),
            "intake.record_invalid"
        );
        assert_eq!(
            fs::read(pair_root.join("candidate.md")).unwrap(),
            candidate_before
        );
        assert_eq!(
            fs::read(pair_root.join("proposal.meta.json")).unwrap(),
            meta_before
        );

        let event_collision = corruption_store(&fixture, "event-collision");
        let event_collision_paths = paths_for_store(&fixture, &event_collision);
        let mut event_collision_lease = intake_lease(&event_collision_paths, "event-collision");
        fs::write(
            event_collision_paths.intake.join(".events-3"),
            b"collision\n",
        )
        .unwrap();
        let intake_before = fs::read(&event_collision_paths.intake_events).unwrap();
        let third = IntakeEvent::new(rejected_intake_payload(
            3,
            occurred_at,
            Some(second.hash().to_owned()),
            None,
            vec!["proposal.collision".to_owned()],
            pair.meta.policy_hash.clone(),
            pair.meta.profile_hash.clone(),
        ))
        .unwrap();
        assert_eq!(
            append_intake_event(&event_collision_paths, &mut event_collision_lease, &third)
                .unwrap_err()
                .code(),
            "intake.record_invalid"
        );
        assert_eq!(
            fs::read(&event_collision_paths.intake_events).unwrap(),
            intake_before
        );

        let sequence = corruption_store(&fixture, "sequence");
        rewrite_canonical_event(&sequence.join("intake/events.jsonl"), 0, |payload| {
            payload.sequence = 2;
        });
        assert_intake_error(&fixture, &sequence, "intake.chain_invalid");

        let event_id = corruption_store(&fixture, "event-id");
        rewrite_canonical_event(&event_id.join("intake/events.jsonl"), 0, |payload| {
            payload.event_id = "int-0000000000000002".to_owned();
        });
        assert_intake_error(&fixture, &event_id, "intake.chain_invalid");

        let previous = corruption_store(&fixture, "previous-hash");
        rewrite_canonical_event(&previous.join("intake/events.jsonl"), 1, |payload| {
            payload.previous_event_hash = Some("a".repeat(64));
        });
        assert_intake_error(&fixture, &previous, "intake.chain_invalid");

        let event_hash = corruption_store(&fixture, "event-hash");
        rewrite_raw_event(&event_hash.join("intake/events.jsonl"), 0, |value| {
            value["event_hash"] = serde_json::Value::String("b".repeat(64));
        });
        assert_intake_error(&fixture, &event_hash, "intake.chain_invalid");

        let schema = corruption_store(&fixture, "schema");
        rewrite_raw_event(&schema.join("intake/events.jsonl"), 0, |value| {
            value["schema_version"] =
                serde_json::Value::String("sovmem-intake-event-v0".to_owned());
        });
        assert_intake_error(&fixture, &schema, "intake.record_invalid");

        let newline = corruption_store(&fixture, "newline");
        let mut bytes = fs::read(newline.join("intake/events.jsonl")).unwrap();
        assert_eq!(bytes.pop(), Some(b'\n'));
        fs::write(newline.join("intake/events.jsonl"), bytes).unwrap();
        assert_intake_error(&fixture, &newline, "intake.record_invalid");

        let unknown = corruption_store(&fixture, "unknown-field");
        rewrite_raw_event(&unknown.join("intake/events.jsonl"), 0, |value| {
            value["private_body"] = serde_json::Value::String("must-not-parse".to_owned());
        });
        assert_intake_error(&fixture, &unknown, "intake.record_invalid");

        let staged_shape = corruption_store(&fixture, "staged-shape");
        rewrite_raw_event(&staged_shape.join("intake/events.jsonl"), 0, |value| {
            value["rule_codes"] = serde_json::json!(["proposal.unexpected"]);
        });
        assert_intake_error(&fixture, &staged_shape, "intake.record_invalid");

        let invalid_source = corruption_store(&fixture, "invalid-source");
        rewrite_raw_event(&invalid_source.join("intake/events.jsonl"), 1, |value| {
            value["source_id"] = serde_json::Value::String("PRIVATE\nSOURCE".to_owned());
        });
        assert_intake_error(&fixture, &invalid_source, "intake.record_invalid");

        let rule_order = corruption_store(&fixture, "rule-order");
        rewrite_raw_event(&rule_order.join("intake/events.jsonl"), 1, |value| {
            value["rule_codes"] =
                serde_json::json!(["proposal.quota_source_open", "proposal.duplicate_content"]);
        });
        assert_intake_error(&fixture, &rule_order, "intake.record_invalid");

        for (name, field) in [
            ("rejected-candidate", "candidate_hash"),
            ("rejected-meta", "proposal_meta_hash"),
            ("rejected-proposal", "proposal_id"),
        ] {
            let store = corruption_store(&fixture, name);
            rewrite_raw_event(&store.join("intake/events.jsonl"), 1, |value| {
                value[field] = serde_json::Value::String("c".repeat(64));
            });
            assert_intake_error(&fixture, &store, "intake.record_invalid");
        }

        let rejected_target = corruption_store(&fixture, "rejected-target");
        rewrite_raw_event(&rejected_target.join("intake/events.jsonl"), 1, |value| {
            value["target_id"] = serde_json::Value::String("private-target".to_owned());
        });
        assert_intake_error(&fixture, &rejected_target, "intake.record_invalid");

        let missing_pair = corruption_store(&fixture, "missing-pair");
        let missing_paths = paths_for_store(&fixture, &missing_pair);
        fs::remove_dir_all(sealed_pair_root(&missing_paths, &pair.meta.proposal_id)).unwrap();
        assert_eq!(
            read_intake(&missing_paths).unwrap_err().code(),
            "intake.pair_binding"
        );

        let extra_file = corruption_store(&fixture, "extra-pair-file");
        let extra_paths = paths_for_store(&fixture, &extra_file);
        fs::write(
            sealed_pair_root(&extra_paths, &pair.meta.proposal_id).join("unexpected.txt"),
            b"must fail closed\n",
        )
        .unwrap();
        assert_eq!(
            read_sealed_pair(&extra_paths, &pair.meta.proposal_id)
                .unwrap_err()
                .code(),
            "intake.pair_binding"
        );
        assert_eq!(
            read_intake(&extra_paths).unwrap_err().code(),
            "intake.pair_binding"
        );

        let pair_hash = corruption_store(&fixture, "pair-hash");
        let pair_hash_paths = paths_for_store(&fixture, &pair_hash);
        fs::write(
            sealed_pair_root(&pair_hash_paths, &pair.meta.proposal_id).join("candidate.md"),
            b"mutated sealed candidate\n",
        )
        .unwrap();
        assert_eq!(
            read_sealed_pair(&pair_hash_paths, &pair.meta.proposal_id)
                .unwrap_err()
                .code(),
            "intake.pair_binding"
        );

        let event_pair_binding = corruption_store(&fixture, "event-pair-binding");
        rewrite_canonical_event(
            &event_pair_binding.join("intake/events.jsonl"),
            0,
            |payload| payload.candidate_hash = Some("d".repeat(64)),
        );
        let rewritten_first_hash =
            recorded_event_hash(&intake_lines(&event_pair_binding.join("intake/events.jsonl"))[0]);
        rewrite_canonical_event(
            &event_pair_binding.join("intake/events.jsonl"),
            1,
            |payload| payload.previous_event_hash = Some(rewritten_first_hash),
        );
        assert_intake_error(&fixture, &event_pair_binding, "intake.pair_binding");
    }

    #[test]
    fn intake_chain_orders_equal_timestamps_and_rejects_clock_regression() {
        let fixture = ProvisionedP3Fixture::empty("intake-order-and-clock");
        let paths = intake_paths(&fixture);
        let occurred_at = "2026-07-21T03:04:05.000000000Z";
        let left = intake_pair(&fixture, "src:agent:left", occurred_at, "left");
        let right = intake_pair(&fixture, "src:agent:right", occurred_at, "right");
        let (first_pair, second_pair) = if left.meta.proposal_id > right.meta.proposal_id {
            (left, right)
        } else {
            (right, left)
        };
        assert!(first_pair.meta.proposal_id > second_pair.meta.proposal_id);
        let mut lease = intake_lease(&paths, "order-and-clock");

        publish_sealed_pair(&paths, &mut lease, &first_pair).unwrap();
        let first =
            IntakeEvent::new(staged_intake_payload(&first_pair, 1, occurred_at, None)).unwrap();
        append_intake_event(&paths, &mut lease, &first).unwrap();
        publish_sealed_pair(&paths, &mut lease, &second_pair).unwrap();
        let second = IntakeEvent::new(staged_intake_payload(
            &second_pair,
            2,
            occurred_at,
            Some(first.hash().to_owned()),
        ))
        .unwrap();
        append_intake_event(&paths, &mut lease, &second).unwrap();

        let view = read_intake(&paths).unwrap();
        let prefix = intake_prefix(&view, view.head()).unwrap();
        assert_eq!(
            prefix
                .staged
                .iter()
                .map(|summary| summary.proposal_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                first_pair.meta.proposal_id.as_str(),
                second_pair.meta.proposal_id.as_str(),
            ]
        );

        let before = fs::read(&paths.intake_events).unwrap();
        let backward = IntakeEvent::new(rejected_intake_payload(
            3,
            "2026-07-21T03:04:04.999999999Z",
            Some(second.hash().to_owned()),
            None,
            vec!["proposal.clock_regression".to_owned()],
            first_pair.meta.policy_hash.clone(),
            first_pair.meta.profile_hash.clone(),
        ))
        .unwrap();
        assert_eq!(
            append_intake_event(&paths, &mut lease, &backward)
                .unwrap_err()
                .code(),
            "intake.clock_regression"
        );
        assert_eq!(fs::read(&paths.intake_events).unwrap(), before);
    }

    #[test]
    fn intake_pair_publication_serializes_trusted_publishers_without_overwrite() {
        let fixture = ProvisionedP3Fixture::empty("intake-pair-unverified-lease");
        let paths = intake_paths(&fixture);
        let pair = intake_pair(
            &fixture,
            "src:agent:lease-pair",
            "2026-07-21T04:05:06.000000000Z",
            "lease-pair",
        );
        let mut first = intake_lease(&paths, "first-pair-publisher");
        let before = sealed_pair_root(&paths, &pair.meta.proposal_id);
        assert!(!before.exists());

        assert_eq!(
            IntakePublicationLease::acquire(&paths, intake_lease_identity("second-pair-publisher"),)
                .unwrap_err()
                .code(),
            "intake.locked"
        );
        publish_sealed_pair(&paths, &mut first, &pair).unwrap();
        let pair_root = sealed_pair_root(&paths, &pair.meta.proposal_id);
        let candidate_before = fs::read(pair_root.join("candidate.md")).unwrap();
        assert_eq!(
            publish_sealed_pair(&paths, &mut first, &pair)
                .unwrap_err()
                .code(),
            "intake.record_invalid"
        );
        assert_eq!(
            fs::read(pair_root.join("candidate.md")).unwrap(),
            candidate_before
        );
    }

    const APPROVED_RUSTIX_MANIFEST_LINE: &str =
        "rustix = { version = \"=1.1.4\", features = [\"fs\"] }";

    fn production_profile_sources(root: &Path) -> BTreeMap<String, String> {
        fn collect(root: &Path, path: &Path, sources: &mut BTreeMap<String, String>) {
            let mut entries = fs::read_dir(path)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|name| name == "p3_internal") {
                        continue;
                    }
                    collect(root, &path, sources);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    let relative = path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    sources.insert(relative, fs::read_to_string(path).unwrap());
                }
            }
        }

        let mut sources = BTreeMap::new();
        collect(root, root, &mut sources);
        sources
    }

    fn publish_inbox_submission_body(source: &str) -> Option<(usize, usize)> {
        let function_start = source.find("fn publish_inbox_submission<")?;
        let body_start = function_start + source[function_start..].find('{')?;
        let mut depth = 0usize;
        for (offset, byte) in source[body_start..].bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some((body_start + 1, body_start + offset));
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn rustix_boundary_guard(manifest: &str, sources: &BTreeMap<String, String>) -> bool {
        let manifest_lines = manifest
            .lines()
            .filter(|line| line.contains("rustix"))
            .collect::<Vec<_>>();
        if manifest_lines != [APPROVED_RUSTIX_MANIFEST_LINE] {
            return false;
        }

        let Some(proposal) = sources.get("proposal.rs") else {
            return false;
        };
        let Some((body_start, body_end)) = publish_inbox_submission_body(proposal) else {
            return false;
        };
        let body = &proposal[body_start..body_end];
        let allowed = [
            "rustix::fs::renameat_with",
            "rustix::fs::RenameFlags::NOREPLACE",
            "rustix::io::Errno::EXIST",
        ];
        let mut remaining = body.to_owned();
        for reference in allowed {
            if remaining.matches(reference).count() != 1 {
                return false;
            }
            remaining = remaining.replacen(reference, "", 1);
        }
        if remaining.contains("rustix") {
            return false;
        }
        if proposal[..body_start].contains("rustix") || proposal[body_end..].contains("rustix") {
            return false;
        }

        sources
            .iter()
            .filter(|(relative, _)| relative.as_str() != "proposal.rs")
            .all(|(_, source)| !source.contains("rustix"))
    }

    #[test]
    fn inbox_publication_dependency_surface_is_narrow_and_intake_leases_stay_std_only() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let sources = production_profile_sources(&root.join("src/profile"));

        assert!(rustix_boundary_guard(&manifest, &sources));
    }

    #[test]
    fn inbox_publication_dependency_guard_rejects_synthetic_rustix_mutations() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let sources = production_profile_sources(&root.join("src/profile"));

        let mut nested_intake_rustix = sources.clone();
        nested_intake_rustix.insert(
            "intake/nested_lease.rs".to_owned(),
            "fn lease() { rustix::fs::renameat_with; }".to_owned(),
        );
        assert!(
            !rustix_boundary_guard(&manifest, &nested_intake_rustix),
            "a nested intake-lease Rustix use must be rejected"
        );

        let nonexact_manifest = format!("{manifest}\nrustix = \"1.1.4\"\n");
        assert!(
            !rustix_boundary_guard(&nonexact_manifest, &sources),
            "a second non-exact Rustix manifest line must be rejected"
        );

        let mut duplicate_root_reference = sources.clone();
        duplicate_root_reference
            .get_mut("proposal.rs")
            .unwrap()
            .push_str("\nrustix::fs::renameat_with\n");
        assert!(
            !rustix_boundary_guard(&manifest, &duplicate_root_reference),
            "a second valid-looking Rustix call in root proposal.rs must be rejected"
        );

        let mut prefix_helper_relocation = sources.clone();
        let proposal = prefix_helper_relocation.get_mut("proposal.rs").unwrap();
        let mut relocated = proposal.clone();
        for (reference, replacement) in [
            ("rustix::fs::renameat_with", "moved_renameat_with"),
            (
                "rustix::fs::RenameFlags::NOREPLACE",
                "moved_rename_flags_noreplace",
            ),
            ("rustix::io::Errno::EXIST", "moved_errno_exist"),
        ] {
            relocated = relocated.replacen(reference, replacement, 1);
        }
        *proposal = format!(
            "fn publish_inbox_submission_helper() {{ rustix::fs::renameat_with; rustix::fs::RenameFlags::NOREPLACE; rustix::io::Errno::EXIST; }}\n{relocated}"
        );
        assert!(
            !rustix_boundary_guard(&manifest, &prefix_helper_relocation),
            "a prefix-named helper containing all Rustix references must not select as the publisher"
        );

        let mut relocated_root_reference = sources;
        let proposal = relocated_root_reference.get_mut("proposal.rs").unwrap();
        *proposal = proposal.replacen("rustix::fs::renameat_with", "renamed_renameat_with", 1);
        proposal.push_str("\nfn unrelated() { rustix::fs::renameat_with; }\n");
        assert!(
            !rustix_boundary_guard(&manifest, &relocated_root_reference),
            "a permitted Rustix reference relocated outside publish_inbox_submission must be rejected"
        );
    }

    #[test]
    fn intake_lease_acquisition_is_no_replace_and_preserves_existing_owner_bytes() {
        let fixture = ProvisionedP3Fixture::empty("intake-lease-no-replace");
        let paths = intake_paths(&fixture);
        let first = intake_lease(&paths, "first-trusted-writer");
        let before = fs::read(paths.locks.join("intake.lock/owner.json")).unwrap();

        assert_eq!(
            IntakePublicationLease::acquire(&paths, intake_lease_identity("second-trusted-writer"))
                .unwrap_err()
                .code(),
            "intake.locked"
        );
        assert_eq!(
            fs::read(paths.locks.join("intake.lock/owner.json")).unwrap(),
            before
        );
        first.release(&paths).unwrap();
    }

    #[test]
    fn intake_lease_nonce_is_portable_canonical_and_unique() {
        let first_fixture = ProvisionedP3Fixture::empty("intake-lease-portable-nonce-first");
        let first_paths = intake_paths(&first_fixture);
        let first = intake_lease(&first_paths, "portable-nonce-first");
        let first_owner = fs::read(first_paths.locks.join("intake.lock/owner.json")).unwrap();
        first.release(&first_paths).unwrap();

        let second_fixture = ProvisionedP3Fixture::empty("intake-lease-portable-nonce-second");
        let second_paths = intake_paths(&second_fixture);
        let second = intake_lease(&second_paths, "portable-nonce-second");
        let second_owner = fs::read(second_paths.locks.join("intake.lock/owner.json")).unwrap();
        second.release(&second_paths).unwrap();

        let nonce = |owner: &[u8]| {
            serde_json::from_slice::<serde_json::Value>(owner).unwrap()["acquisition_nonce"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        let first_nonce = nonce(&first_owner);
        let second_nonce = nonce(&second_owner);
        let is_lower_hex = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };

        assert!(is_lower_hex(&first_nonce));
        assert!(is_lower_hex(&second_nonce));
        assert_ne!(first_nonce, second_nonce);
        assert!(!include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/profile/intake.rs"
        ))
        .contains("/dev/urandom"));
    }

    #[test]
    fn intake_lease_owner_publication_failure_leaves_visible_incomplete_evidence() {
        let fixture = ProvisionedP3Fixture::empty("intake-lease-owner-failure");
        let paths = intake_paths(&fixture);
        install_intake_lease_owner_publication_failure_for_test();

        assert_eq!(
            IntakePublicationLease::acquire(&paths, intake_lease_identity("trusted-writer"))
                .unwrap_err()
                .code(),
            "intake.write"
        );
        assert!(paths.locks.join("intake.lock").is_dir());
        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture P3 profile");
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
    }

    #[test]
    fn intake_lease_release_failure_leaves_visible_incomplete_evidence() {
        let fixture = ProvisionedP3Fixture::empty("intake-lease-release-incomplete");
        let paths = intake_paths(&fixture);
        let lease = intake_lease(&paths, "trusted-writer");
        install_intake_lease_release_finalization_failure_for_test();

        assert_eq!(lease.release(&paths).unwrap_err().code(), "intake.write");
        assert!(paths.locks.join("intake.lock").is_dir());
        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture P3 profile");
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
    }

    #[cfg(unix)]
    #[test]
    fn intake_lease_rejects_unsafe_observed_owner_without_publication() {
        let fixture = ProvisionedP3Fixture::empty("intake-unsafe-owner");
        let paths = intake_paths(&fixture);
        let pair = intake_pair(
            &fixture,
            "src:agent:unsafe",
            "2026-07-21T06:00:00.000000000Z",
            "unsafe",
        );
        let mut lease = intake_lease(&paths, "trusted-writer");
        let owner = paths.locks.join("intake.lock/owner.json");
        let owner_bytes = fs::read(&owner).unwrap();
        fs::remove_file(&owner).unwrap();
        std::os::unix::fs::symlink("missing-owner.json", &owner).unwrap();

        assert_eq!(
            publish_sealed_pair(&paths, &mut lease, &pair)
                .unwrap_err()
                .code(),
            "intake.locked"
        );
        assert!(!sealed_pair_root(&paths, &pair.meta.proposal_id).exists());
        let profile = load_p3_profile(&fixture.profile_path).expect("load fixture P3 profile");
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "store.symlink"
        );

        fs::remove_file(&owner).unwrap();
        fs::write(&owner, owner_bytes).unwrap();
        lease.release(&paths).unwrap();
    }

    #[test]
    fn intake_event_publication_serializes_trusted_publishers_without_stale_successor() {
        let fixture = ProvisionedP3Fixture::empty("intake-event-unverified-lease");
        let paths = intake_paths(&fixture);
        let pair = intake_pair(
            &fixture,
            "src:agent:lease-event",
            "2026-07-21T04:05:07.000000000Z",
            "lease-event",
        );
        let mut first = intake_lease(&paths, "first-event-publisher");
        publish_sealed_pair(&paths, &mut first, &pair).unwrap();
        let event = IntakeEvent::new(staged_intake_payload(
            &pair,
            1,
            "2026-07-21T04:05:07.000000000Z",
            None,
        ))
        .unwrap();
        let before = fs::read(&paths.intake_events).unwrap();

        assert_eq!(
            IntakePublicationLease::acquire(
                &paths,
                intake_lease_identity("second-event-publisher"),
            )
            .unwrap_err()
            .code(),
            "intake.locked"
        );
        append_intake_event(&paths, &mut first, &event).unwrap();
        assert_ne!(fs::read(&paths.intake_events).unwrap(), before);
        let committed = fs::read(&paths.intake_events).unwrap();
        assert_eq!(
            append_intake_event(&paths, &mut first, &event)
                .unwrap_err()
                .code(),
            "intake.pair_binding"
        );
        assert_eq!(fs::read(&paths.intake_events).unwrap(), committed);
    }

    #[test]
    fn intake_lease_reacquisition_invalidates_stale_pair_capability_without_overwrite() {
        let fixture = ProvisionedP3Fixture::empty("intake-stale-pair-lease");
        let paths = intake_paths(&fixture);
        let identity = intake_lease_identity("same-pair-publisher");
        let baseline_pair = intake_pair(
            &fixture,
            "src:agent:lease-pair-baseline",
            "2026-07-21T04:06:00.000000000Z",
            "lease-pair-baseline",
        );
        let attempted_pair = intake_pair(
            &fixture,
            "src:agent:lease-pair-attempt",
            "2026-07-21T04:06:01.000000000Z",
            "lease-pair-attempt",
        );
        let mut original = IntakePublicationLease::acquire(&paths, identity.clone()).unwrap();
        publish_sealed_pair(&paths, &mut original, &baseline_pair).unwrap();
        let baseline_event = IntakeEvent::new(staged_intake_payload(
            &baseline_pair,
            1,
            "2026-07-21T04:06:00.000000000Z",
            None,
        ))
        .unwrap();
        append_intake_event(&paths, &mut original, &baseline_event).unwrap();
        let mut stale = original.forged_stale_capability_for_test();
        original.release(&paths).unwrap();
        let reacquired = IntakePublicationLease::acquire(&paths, identity).unwrap();

        let baseline_root = sealed_pair_root(&paths, &baseline_pair.meta.proposal_id);
        let baseline_candidate = fs::read(baseline_root.join("candidate.md")).unwrap();
        let baseline_meta = fs::read(baseline_root.join("proposal.meta.json")).unwrap();
        let intake_before = fs::read(&paths.intake_events).unwrap();
        let head_before = read_intake(&paths).unwrap().head().clone();

        assert_eq!(
            publish_sealed_pair(&paths, &mut stale, &attempted_pair)
                .unwrap_err()
                .code(),
            "intake.locked"
        );
        assert!(!sealed_pair_root(&paths, &attempted_pair.meta.proposal_id).exists());
        assert_eq!(
            fs::read(baseline_root.join("candidate.md")).unwrap(),
            baseline_candidate
        );
        assert_eq!(
            fs::read(baseline_root.join("proposal.meta.json")).unwrap(),
            baseline_meta
        );
        assert_eq!(fs::read(&paths.intake_events).unwrap(), intake_before);
        assert_eq!(read_intake(&paths).unwrap().head(), &head_before);
        reacquired.release(&paths).unwrap();
    }

    #[test]
    fn intake_lease_reacquisition_invalidates_stale_event_capability_without_replacement() {
        let fixture = ProvisionedP3Fixture::empty("intake-stale-event-lease");
        let paths = intake_paths(&fixture);
        let identity = intake_lease_identity("same-event-publisher");
        let pair = intake_pair(
            &fixture,
            "src:agent:lease-event-baseline",
            "2026-07-21T04:07:00.000000000Z",
            "lease-event-baseline",
        );
        let mut original = IntakePublicationLease::acquire(&paths, identity.clone()).unwrap();
        publish_sealed_pair(&paths, &mut original, &pair).unwrap();
        let first = IntakeEvent::new(staged_intake_payload(
            &pair,
            1,
            "2026-07-21T04:07:00.000000000Z",
            None,
        ))
        .unwrap();
        append_intake_event(&paths, &mut original, &first).unwrap();
        let mut stale = original.forged_stale_capability_for_test();
        original.release(&paths).unwrap();
        let reacquired = IntakePublicationLease::acquire(&paths, identity).unwrap();
        let second = IntakeEvent::new(rejected_intake_payload(
            2,
            "2026-07-21T04:07:01.000000000Z",
            Some(first.hash().to_owned()),
            None,
            vec!["proposal.stale_lease".to_owned()],
            pair.meta.policy_hash.clone(),
            pair.meta.profile_hash.clone(),
        ))
        .unwrap();
        let intake_before = fs::read(&paths.intake_events).unwrap();
        let head_before = read_intake(&paths).unwrap().head().clone();

        assert_eq!(
            append_intake_event(&paths, &mut stale, &second)
                .unwrap_err()
                .code(),
            "intake.locked"
        );
        assert_eq!(fs::read(&paths.intake_events).unwrap(), intake_before);
        assert_eq!(read_intake(&paths).unwrap().head(), &head_before);
        reacquired.release(&paths).unwrap();
    }

    #[test]
    fn intake_pair_promotion_rechecks_rotated_lease_before_rename() {
        let fixture = ProvisionedP3Fixture::empty("intake-pair-promotion-rotation");
        let paths = intake_paths(&fixture);
        let identity = intake_lease_identity("pair-promotion-publisher");
        let pair = intake_pair(
            &fixture,
            "src:agent:pair-promotion-rotation",
            "2026-07-21T04:08:00.000000000Z",
            "pair-promotion-rotation",
        );
        let mut lease = IntakePublicationLease::acquire(&paths, identity.clone()).unwrap();
        let stale = lease.forged_stale_capability_for_test();
        let replacement = install_lease_rotation_hook(&paths, stale, identity);
        let destination = sealed_pair_root(&paths, &pair.meta.proposal_id);
        let intake_before = fs::read(&paths.intake_events).unwrap();

        let result = publish_sealed_pair(&paths, &mut lease, &pair);
        release_rotated_lease(&paths, &replacement);

        assert_eq!(result.unwrap_err().code(), "intake.locked");
        assert!(!destination.exists());
        assert_eq!(fs::read(&paths.intake_events).unwrap(), intake_before);
    }

    #[test]
    fn intake_event_promotion_rechecks_rotated_lease_before_replacement() {
        let fixture = ProvisionedP3Fixture::empty("intake-event-promotion-rotation");
        let paths = intake_paths(&fixture);
        let identity = intake_lease_identity("event-promotion-publisher");
        let pair = intake_pair(
            &fixture,
            "src:agent:event-promotion-rotation",
            "2026-07-21T04:09:00.000000000Z",
            "event-promotion-rotation",
        );
        let mut lease = IntakePublicationLease::acquire(&paths, identity.clone()).unwrap();
        publish_sealed_pair(&paths, &mut lease, &pair).unwrap();
        let first = IntakeEvent::new(staged_intake_payload(
            &pair,
            1,
            "2026-07-21T04:09:00.000000000Z",
            None,
        ))
        .unwrap();
        append_intake_event(&paths, &mut lease, &first).unwrap();
        let second = IntakeEvent::new(rejected_intake_payload(
            2,
            "2026-07-21T04:09:01.000000000Z",
            Some(first.hash().to_owned()),
            None,
            vec!["proposal.promotion_rotation".to_owned()],
            pair.meta.policy_hash.clone(),
            pair.meta.profile_hash.clone(),
        ))
        .unwrap();
        let stale = lease.forged_stale_capability_for_test();
        let replacement = install_lease_rotation_hook(&paths, stale, identity);
        let intake_before = fs::read(&paths.intake_events).unwrap();
        let head_before = read_intake(&paths).unwrap().head().clone();

        let result = append_intake_event(&paths, &mut lease, &second);
        release_rotated_lease(&paths, &replacement);

        assert_eq!(result.unwrap_err().code(), "intake.locked");
        assert_eq!(fs::read(&paths.intake_events).unwrap(), intake_before);
        assert_eq!(read_intake(&paths).unwrap().head(), &head_before);
    }

    #[test]
    fn intake_publication_lease_supports_sequential_pair_then_event_and_release() {
        let fixture = ProvisionedP3Fixture::empty("intake-sequential-live-lease");
        let paths = intake_paths(&fixture);
        let pair = intake_pair(
            &fixture,
            "src:agent:sequential-live-lease",
            "2026-07-21T04:10:00.000000000Z",
            "sequential-live-lease",
        );
        let mut lease = intake_lease(&paths, "sequential-publisher");

        publish_sealed_pair(&paths, &mut lease, &pair).unwrap();
        let event = IntakeEvent::new(staged_intake_payload(
            &pair,
            1,
            "2026-07-21T04:10:00.000000000Z",
            None,
        ))
        .unwrap();
        let view = append_intake_event(&paths, &mut lease, &event).unwrap();
        assert_eq!(view.head().sequence, 1);
        assert_eq!(view.head().event_hash.as_deref(), Some(event.hash()));
        assert_eq!(
            read_sealed_pair(&paths, &pair.meta.proposal_id)
                .unwrap()
                .candidate_bytes,
            pair.candidate_bytes
        );

        lease.release(&paths).unwrap();
        assert!(!paths.locks.join("intake.lock").exists());
    }

    #[cfg(unix)]
    #[test]
    fn intake_pair_publication_rejects_symlinked_replayed_lease_owner_without_durable_pair() {
        let fixture = ProvisionedP3Fixture::empty("intake-symlinked-owner-pair");
        let paths = intake_paths(&fixture);
        let pair = intake_pair(
            &fixture,
            "src:agent:symlinked-owner-pair",
            "2026-07-21T04:11:00.000000000Z",
            "symlinked-owner-pair",
        );
        let mut lease = intake_lease(&paths, "symlinked-owner-pair-publisher");
        let lock = paths.locks.join("intake.lock");
        let owner = lock.join("owner.json");
        let owner_bytes = fs::read(&owner).unwrap();
        let replayed_owner = paths.locks.join("replayed-owner.json");
        fs::write(&replayed_owner, &owner_bytes).unwrap();
        fs::remove_file(&owner).unwrap();
        symlink(&replayed_owner, &owner).unwrap();

        let result = publish_sealed_pair(&paths, &mut lease, &pair);
        let destination = sealed_pair_root(&paths, &pair.meta.proposal_id);
        let shard = paths.intake_proposals.join(&pair.meta.proposal_id[4..6]);
        let temporary_prefix = format!(".proposal-{}", pair.meta.proposal_id);

        fs::remove_file(&owner).unwrap();
        fs::write(&owner, &owner_bytes).unwrap();
        fs::remove_file(&replayed_owner).unwrap();

        assert_eq!(result.unwrap_err().code(), "intake.locked");
        assert!(!destination.exists());
        assert!(
            !shard.exists()
                || !fs::read_dir(shard).unwrap().any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&temporary_prefix))
        );
        lease.release(&paths).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn intake_event_publication_rejects_symlinked_replayed_lock_without_replacement() {
        let fixture = ProvisionedP3Fixture::empty("intake-symlinked-lock-event");
        let paths = intake_paths(&fixture);
        let pair = intake_pair(
            &fixture,
            "src:agent:symlinked-lock-event",
            "2026-07-21T04:12:00.000000000Z",
            "symlinked-lock-event",
        );
        let mut lease = intake_lease(&paths, "symlinked-lock-event-publisher");
        publish_sealed_pair(&paths, &mut lease, &pair).unwrap();
        let event = IntakeEvent::new(staged_intake_payload(
            &pair,
            1,
            "2026-07-21T04:12:00.000000000Z",
            None,
        ))
        .unwrap();
        let before = fs::read(&paths.intake_events).unwrap();
        let lock = paths.locks.join("intake.lock");
        let owner_bytes = fs::read(lock.join("owner.json")).unwrap();
        let replayed_lock = paths.locks.join("replayed-intake.lock");
        fs::create_dir(&replayed_lock).unwrap();
        fs::write(replayed_lock.join("owner.json"), &owner_bytes).unwrap();
        fs::remove_dir_all(&lock).unwrap();
        symlink(&replayed_lock, &lock).unwrap();

        let result = append_intake_event(&paths, &mut lease, &event);
        let temporary = paths.intake.join(".events-1");

        fs::remove_file(&lock).unwrap();
        fs::create_dir(&lock).unwrap();
        fs::write(lock.join("owner.json"), &owner_bytes).unwrap();
        fs::remove_dir_all(&replayed_lock).unwrap();

        assert_eq!(result.unwrap_err().code(), "intake.locked");
        assert_eq!(fs::read(&paths.intake_events).unwrap(), before);
        assert!(!temporary.exists());
        lease.release(&paths).unwrap();
    }

    #[test]
    fn intake_event_pre_rename_directory_sync_failure_removes_successor() {
        let fixture = ProvisionedP3Fixture::empty("intake-event-pre-rename-sync-failure");
        let paths = intake_paths(&fixture);
        let pair = intake_pair(
            &fixture,
            "src:agent:event-pre-rename-sync-failure",
            "2026-07-21T04:13:00.000000000Z",
            "event-pre-rename-sync-failure",
        );
        let mut lease = intake_lease(&paths, "event-pre-rename-sync-failure");
        let event = IntakeEvent::new(rejected_intake_payload(
            1,
            "2026-07-21T04:13:00.000000000Z",
            None,
            None,
            vec!["proposal.pre_rename_sync_failure".to_owned()],
            pair.meta.policy_hash,
            pair.meta.profile_hash,
        ))
        .unwrap();
        let before = fs::read(&paths.intake_events).unwrap();
        let temporary = paths.intake.join(".events-1");
        install_intake_event_pre_rename_sync_failure_for_test();

        let result = append_intake_event(&paths, &mut lease, &event);

        assert_eq!(result.unwrap_err().code(), "intake.write");
        assert_eq!(fs::read(&paths.intake_events).unwrap(), before);
        assert!(!temporary.exists());
        lease.release(&paths).unwrap();
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        assert!(inspect_store(&profile, &fixture.store_root).is_ok());
    }

    fn install_lease_rotation_hook(
        paths: &StorePaths,
        stale: IntakePublicationLease,
        identity: IntakeLeaseIdentity,
    ) -> Rc<RefCell<Option<IntakePublicationLease>>> {
        let replacement = Rc::new(RefCell::new(None));
        let replacement_for_hook = Rc::clone(&replacement);
        let paths = paths.clone();
        install_intake_promotion_hook_for_test(move || {
            stale.release(&paths).unwrap();
            let reacquired = IntakePublicationLease::acquire(&paths, identity).unwrap();
            *replacement_for_hook.borrow_mut() = Some(reacquired);
        });
        replacement
    }

    fn release_rotated_lease(
        paths: &StorePaths,
        replacement: &Rc<RefCell<Option<IntakePublicationLease>>>,
    ) {
        replacement
            .borrow_mut()
            .take()
            .expect("promotion hook reacquired intake lease")
            .release(paths)
            .unwrap();
    }

    fn intake_paths(fixture: &ProvisionedP3Fixture) -> StorePaths {
        paths_for_store(fixture, &fixture.store_root)
    }

    fn intake_lease_identity(owner_id: &str) -> IntakeLeaseIdentity {
        IntakeLeaseIdentity::new(
            owner_id,
            "intake-test",
            &Timestamp::parse("2026-07-21T04:05:00.000000000Z").unwrap(),
            42,
        )
        .unwrap()
    }

    fn intake_lease(paths: &StorePaths, owner_id: &str) -> IntakePublicationLease {
        IntakePublicationLease::acquire(paths, intake_lease_identity(owner_id)).unwrap()
    }

    fn paths_for_store(fixture: &ProvisionedP3Fixture, store_root: &Path) -> StorePaths {
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        inspect_store(&profile, store_root).unwrap().paths
    }

    fn intake_pair(
        fixture: &ProvisionedP3Fixture,
        source_id: &str,
        occurred_at: &str,
        marker: &str,
    ) -> ProposalPair {
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = inspect_store(&profile, &fixture.store_root).unwrap();
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_bytes = fs::read(store.paths.seed.join("bundle").join(target_path)).unwrap();
        prepare_proposal(ProposalDraft {
            submitted_at: Timestamp::parse(occurred_at).unwrap(),
            source_id: source_id.to_owned(),
            operation: ProposalOperation::Replace,
            intent: ProposalIntent::Accept,
            target_id: "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"
                .to_owned(),
            target_path: target_path.to_owned(),
            base: ProposalBase {
                kind: "seed".to_owned(),
                generation_id: None,
                bundle_hash: store.seed.bundle_hash,
                manifest_hash: store.seed.manifest_hash,
                target_hash: Some(blake3::hash(&target_bytes).to_hex().to_string()),
            },
            candidate_bytes: format!("sealed candidate {marker}\n").into_bytes(),
            policy_hash: store.policy.hash,
            profile_hash: profile.hash,
            confidence: None,
            evaluator: None,
        })
        .unwrap()
    }

    fn staged_intake_payload(
        pair: &ProposalPair,
        sequence: u64,
        occurred_at: &str,
        previous_event_hash: Option<String>,
    ) -> IntakePayload {
        IntakePayload {
            schema_version: INTAKE_EVENT_SCHEMA.to_owned(),
            event_type: "intake".to_owned(),
            sequence,
            event_id: format!("int-{sequence:016}"),
            occurred_at: occurred_at.to_owned(),
            previous_event_hash,
            outcome: IntakeOutcome::STAGED,
            source_id: Some(pair.meta.source_id.clone()),
            proposal_id: Some(pair.meta.proposal_id.clone()),
            proposal_meta_hash: Some(pair.meta_hash.clone()),
            candidate_hash: Some(pair.candidate_hash.clone()),
            policy_hash: pair.meta.policy_hash.clone(),
            profile_hash: pair.meta.profile_hash.clone(),
            rule_codes: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rejected_intake_payload(
        sequence: u64,
        occurred_at: &str,
        previous_event_hash: Option<String>,
        source_id: Option<&str>,
        rule_codes: Vec<String>,
        policy_hash: String,
        profile_hash: String,
    ) -> IntakePayload {
        IntakePayload {
            schema_version: INTAKE_EVENT_SCHEMA.to_owned(),
            event_type: "intake".to_owned(),
            sequence,
            event_id: format!("int-{sequence:016}"),
            occurred_at: occurred_at.to_owned(),
            previous_event_hash,
            outcome: IntakeOutcome::REJECTED,
            source_id: source_id.map(str::to_owned),
            proposal_id: None,
            proposal_meta_hash: None,
            candidate_hash: None,
            policy_hash,
            profile_hash,
            rule_codes,
        }
    }

    fn corruption_store(fixture: &ProvisionedP3Fixture, name: &str) -> PathBuf {
        let destination = fixture
            .store_root
            .with_file_name(format!("task-2-corruption-{name}"));
        copy_intake_tree(&fixture.store_root, &destination);
        destination
    }

    fn copy_intake_tree(source: &Path, destination: &Path) {
        let metadata = fs::symlink_metadata(source).unwrap();
        assert!(!metadata.file_type().is_symlink());
        if metadata.is_file() {
            fs::write(destination, fs::read(source).unwrap()).unwrap();
            return;
        }
        fs::create_dir(destination).unwrap();
        let mut entries = fs::read_dir(source)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_intake_tree(&entry.path(), &destination.join(entry.file_name()));
        }
    }

    fn rewrite_canonical_event(path: &Path, index: usize, mutate: impl FnOnce(&mut IntakePayload)) {
        let mut lines = intake_lines(path);
        let mut payload = payload_from_line(&lines[index]);
        mutate(&mut payload);
        lines[index] = IntakeEvent::new(payload).unwrap().canonical_line_bytes();
        write_intake_lines(path, &lines);
    }

    fn rewrite_raw_event(path: &Path, index: usize, mutate: impl FnOnce(&mut serde_json::Value)) {
        let mut lines = intake_lines(path);
        let mut value: serde_json::Value = serde_json::from_slice(&lines[index]).unwrap();
        mutate(&mut value);
        let mut line = serde_json::to_vec(&value).unwrap();
        line.push(b'\n');
        lines[index] = line;
        write_intake_lines(path, &lines);
    }

    fn payload_from_line(line: &[u8]) -> IntakePayload {
        let mut value: serde_json::Value = serde_json::from_slice(line).unwrap();
        value.as_object_mut().unwrap().remove("event_hash");
        serde_json::from_value(value).unwrap()
    }

    fn recorded_event_hash(line: &[u8]) -> String {
        let value: serde_json::Value = serde_json::from_slice(line).unwrap();
        value["event_hash"].as_str().unwrap().to_owned()
    }

    fn intake_lines(path: &Path) -> Vec<Vec<u8>> {
        fs::read(path)
            .unwrap()
            .split_inclusive(|byte| *byte == b'\n')
            .map(Vec::from)
            .collect()
    }

    fn write_intake_lines(path: &Path, lines: &[Vec<u8>]) {
        fs::write(path, lines.concat()).unwrap();
    }

    fn assert_intake_error(fixture: &ProvisionedP3Fixture, store_root: &Path, expected: &str) {
        let paths = paths_for_store(fixture, store_root);
        assert_eq!(
            read_intake(&paths).unwrap_err().code(),
            expected,
            "corruption fixture {}",
            store_root.display()
        );
    }

    fn sealed_pair_root(paths: &StorePaths, proposal_id: &str) -> PathBuf {
        paths
            .intake_proposals
            .join(&proposal_id[4..6])
            .join(proposal_id)
    }

    #[test]
    fn confidence_accepts_only_canonical_millionths() {
        for (source, millionths) in [
            ("0", 0),
            ("0.000001", 1),
            ("0.75", 750_000),
            ("1", 1_000_000),
        ] {
            assert_eq!(Confidence::parse(source).unwrap().millionths(), millionths);
        }
        for invalid in ["-0.1", "1.000001", ".5", "0.", "0.1234567", "NaN", "inf"] {
            assert!(Confidence::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn confidence_is_a_bounded_json_number_not_a_string() {
        let confidence = Confidence::parse("0.000001").unwrap();
        assert_eq!(serde_json::to_string(&confidence).unwrap(), "0.000001");
        assert_eq!(
            serde_json::from_str::<Confidence>("0.75")
                .unwrap()
                .millionths(),
            750_000
        );
        for invalid in ["\"0.75\"", "1.000001", "0.1234567", "1e-6", "-0.1"] {
            assert!(
                serde_json::from_str::<Confidence>(invalid).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn proposal_enums_are_frozen() {
        assert_eq!(ProposalOperation::Create.as_str(), "create");
        assert_eq!(ProposalOperation::Replace.as_str(), "replace");
        assert_eq!(ProposalIntent::Accept.as_str(), "accept");
        assert_eq!(ProposalIntent::Rewrite.as_str(), "rewrite");
        assert_eq!(
            Timestamp::parse("2026-07-14T04:30:00.000000000Z")
                .unwrap()
                .as_str(),
            "2026-07-14T04:30:00.000000000Z"
        );
    }

    #[test]
    fn public_submission_creates_only_a_strict_untrusted_inbox_item() {
        let fixture = ProvisionedP3Fixture::empty("submit-valid");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate = fixture.profile_path.parent().unwrap().join("candidate.md");
        fs::write(
            &candidate,
            provisioned_candidate(&fixture.store_root, target_path, target_id),
        )
        .unwrap();

        let request = || ProposalSubmission {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            source_id: "src:agent:fixture".to_owned(),
            operation: ProposalOperation::Replace,
            intent: ProposalIntent::Accept,
            target_id: target_id.to_owned(),
            target_path: target_path.to_owned(),
            candidate_path: candidate.clone(),
            confidence: Some(Confidence::parse("0.75").expect("confidence")),
            evaluator_path: None,
        };
        let submitted = submit_proposal(request()).expect("submit a valid proposal request");

        assert_eq!(submitted.state, "submitted");
        assert!(submitted.submission_id.starts_with("sub-"));
        assert!(submitted.proposal_id.is_empty());
        let inbox = fixture
            .store_root
            .join("staging/inbox")
            .join(&submitted.submission_id);
        assert!(inbox.join("candidate.md").is_file());
        assert!(inbox.join("submission.meta.json").is_file());
        assert!(
            std::fs::read(fixture.store_root.join("intake/events.jsonl"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            std::fs::read(fixture.store_root.join("ledger/verdicts.jsonl")).unwrap(),
            b""
        );
        assert_eq!(
            std::fs::read(fixture.store_root.join("canon/CURRENT")).unwrap(),
            b"seed\n"
        );

        let store = inspect_store(
            &load_p3_profile(&fixture.profile_path).unwrap(),
            &fixture.store_root,
        )
        .unwrap();
        let read = read_inbox_submission(
            &store.paths.inbox,
            &submitted.submission_id,
            store.policy.policy.max_candidate_bytes,
        )
        .unwrap();
        assert_eq!(read.meta.submission_id, submitted.submission_id);
        std::fs::write(inbox.join("extra"), b"poison").unwrap();
        assert_eq!(
            read_inbox_submission(
                &store.paths.inbox,
                &submitted.submission_id,
                store.policy.policy.max_candidate_bytes,
            )
            .unwrap_err()
            .code(),
            "proposal.pair_incomplete"
        );
        std::fs::remove_file(inbox.join("extra")).unwrap();
        std::fs::remove_file(inbox.join("candidate.md")).unwrap();
        #[cfg(unix)]
        {
            symlink(&candidate, inbox.join("candidate.md")).unwrap();
            assert_eq!(
                read_inbox_submission(
                    &store.paths.inbox,
                    &submitted.submission_id,
                    store.policy.policy.max_candidate_bytes,
                )
                .unwrap_err()
                .code(),
                "proposal.path_unsafe"
            );
            std::fs::remove_file(inbox.join("candidate.md")).unwrap();
        }
        std::fs::write(inbox.join("candidate.md"), b"changed raw bytes").unwrap();
        assert_eq!(
            read_inbox_submission(
                &store.paths.inbox,
                &submitted.submission_id,
                store.policy.policy.max_candidate_bytes,
            )
            .unwrap_err()
            .code(),
            "proposal.meta_noncanonical"
        );
    }

    #[test]
    fn raw_submission_faults_leave_only_hidden_recovery_residue() {
        for (family, fault) in [
            ("candidate-sync", PublicationFaultPoint::AfterCandidateSync),
            ("metadata-sync", PublicationFaultPoint::AfterMetadataSync),
            (
                "directory-rename",
                PublicationFaultPoint::BeforeDirectoryRename,
            ),
        ] {
            let fixture = ProvisionedP3Fixture::empty(&format!("raw-submission-{family}"));
            let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
            let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
            let candidate = fixture.profile_path.parent().unwrap().join("candidate.md");
            fs::write(
                &candidate,
                provisioned_candidate(&fixture.store_root, target_path, target_id),
            )
            .unwrap();
            let protected_before = fixture
                .durable_hashes()
                .into_iter()
                .filter(|(relative, _)| !relative.starts_with("staging/inbox/"))
                .collect::<Vec<_>>();

            let error = submit_proposal_with_publication_fault(
                ProposalSubmission {
                    profile_path: fixture.profile_path.clone(),
                    store_root: fixture.store_root.clone(),
                    source_id: "src:agent:fixture".to_owned(),
                    operation: ProposalOperation::Replace,
                    intent: ProposalIntent::Accept,
                    target_id: target_id.to_owned(),
                    target_path: target_path.to_owned(),
                    candidate_path: candidate,
                    confidence: None,
                    evaluator_path: None,
                },
                fault,
            )
            .expect_err(family);

            assert_eq!(error, sovmem::profile::ProposalError::Write, "{family}");
            let inbox = fixture.store_root.join("staging/inbox");
            let entries = fs::read_dir(&inbox)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(entries.len(), 1, "{family}");
            assert!(
                entries[0]
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".submission-"),
                "{family} exposed a visible partial raw submission at {}",
                entries[0].path().display()
            );
            assert_eq!(
                fixture
                    .durable_hashes()
                    .into_iter()
                    .filter(|(relative, _)| !relative.starts_with("staging/inbox/"))
                    .collect::<Vec<_>>(),
                protected_before,
                "{family} changed protected state"
            );
            let profile = load_p3_profile(&fixture.profile_path).unwrap();
            assert!(
                proposal_list(
                    &profile,
                    &fixture.store_root,
                    ProposalFilter { state: None }
                )
                .expect("hidden raw-submission residue must not poison queue projection")
                .is_empty(),
                "{family} projected hidden recovery residue"
            );
        }
    }

    #[test]
    fn raw_submission_final_collision_retains_exact_hidden_custody() {
        let fixture = ProvisionedP3Fixture::empty("raw-submission-no-replace");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate = fixture.profile_path.parent().unwrap().join("candidate.md");
        fs::write(
            &candidate,
            provisioned_candidate(&fixture.store_root, target_path, target_id),
        )
        .unwrap();
        let request = || ProposalSubmission {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            source_id: "src:agent:fixture".to_owned(),
            operation: ProposalOperation::Replace,
            intent: ProposalIntent::Accept,
            target_id: target_id.to_owned(),
            target_path: target_path.to_owned(),
            candidate_path: candidate.clone(),
            confidence: None,
            evaluator_path: None,
        };
        let inbox = fixture.store_root.join("staging/inbox");
        let final_submission = Rc::new(RefCell::new(None));
        let collision_target = Rc::clone(&final_submission);
        let error = submit_proposal_with_final_publication_hook(request(), || {
            let hidden_name = fs::read_dir(&inbox)
                .map_err(|_| sovmem::profile::ProposalError::Write)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| sovmem::profile::ProposalError::Write)?
                .into_iter()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .find(|name| name.starts_with(".submission-"))
                .ok_or(sovmem::profile::ProposalError::Write)?;
            let submission_id = hidden_name
                .get(".submission-".len()..".submission-".len() + 68)
                .ok_or(sovmem::profile::ProposalError::Write)?;
            let destination = inbox.join(submission_id);
            fs::create_dir(&destination).map_err(|_| sovmem::profile::ProposalError::Write)?;
            *collision_target.borrow_mut() = Some(destination);
            Ok(())
        })
        .expect_err("a final directory inserted before promotion must win without replacement");
        let final_submission = final_submission
            .borrow_mut()
            .take()
            .expect("the hook inserted a final directory");
        let submission_id = final_submission.file_name().unwrap().to_string_lossy();
        let expected_custody = inbox.join(format!(".submission-{submission_id}"));

        assert_eq!(error.code(), "proposal.duplicate_id");
        assert!(
            final_submission.is_dir(),
            "the colliding final directory remains intact"
        );
        assert!(
            fs::read_dir(&final_submission).unwrap().next().is_none(),
            "the colliding empty final directory must not be replaced"
        );
        assert!(
            expected_custody.join("candidate.md").is_file()
                && expected_custody.join("submission.meta.json").is_file(),
            "the complete raw pair remains in its deterministic hidden custody"
        );
    }

    #[test]
    fn raw_submission_duplicate_keeps_one_final_pair_and_loser_custody() {
        let fixture = ProvisionedP3Fixture::empty("raw-submission-duplicate-custody");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate = fixture.profile_path.parent().unwrap().join("candidate.md");
        fs::write(
            &candidate,
            provisioned_candidate(&fixture.store_root, target_path, target_id),
        )
        .unwrap();
        let request = || ProposalSubmission {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            source_id: "src:agent:fixture".to_owned(),
            operation: ProposalOperation::Replace,
            intent: ProposalIntent::Accept,
            target_id: target_id.to_owned(),
            target_path: target_path.to_owned(),
            candidate_path: candidate.clone(),
            confidence: None,
            evaluator_path: None,
        };
        let clock = FixedClock(Timestamp::parse("2026-07-26T01:02:04.000000000Z").unwrap());
        let first = submit_proposal_with_clock(request(), &clock).unwrap();
        let inbox = fixture.store_root.join("staging/inbox");
        let final_submission = inbox.join(&first.submission_id);
        let candidate_before = fs::read(final_submission.join("candidate.md")).unwrap();
        let meta_before = fs::read(final_submission.join("submission.meta.json")).unwrap();

        let loser = submit_proposal_with_clock(request(), &clock).unwrap_err();

        assert_eq!(loser.code(), "proposal.duplicate_id");
        assert_eq!(
            fs::read(final_submission.join("candidate.md")).unwrap(),
            candidate_before
        );
        assert_eq!(
            fs::read(final_submission.join("submission.meta.json")).unwrap(),
            meta_before
        );
        let custody = inbox.join(format!(".submission-{}", first.submission_id));
        assert!(custody.join("candidate.md").is_file());
        assert!(custody.join("submission.meta.json").is_file());
        assert_eq!(
            fs::read_dir(&inbox)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy() == first.submission_id)
                .count(),
            1,
            "exactly one final submission directory is published"
        );
    }

    #[cfg(unix)]
    #[test]
    fn intake_seal_cleanup_never_follows_a_swapped_inbox_into_protected_evidence() {
        let fixture = ProvisionedP3Fixture::empty("intake-seal-cleanup-custody");
        let submitted = submit_valid_intake_request(&fixture, "cleanup-custody");
        let inbox_root = fixture
            .store_root
            .join("staging/inbox")
            .join(&submitted.submission_id);
        let displaced = fixture.store_root.join("staging/displaced-submission");
        let events = fixture.store_root.join("intake/events.jsonl");
        let proposals = fixture.store_root.join("intake/proposals");
        let inbox_for_hook = inbox_root.clone();
        let displaced_for_hook = displaced.clone();
        install_intake_seal_pre_cleanup_hook_for_test(move || {
            if fs::symlink_metadata(&inbox_for_hook).is_ok() {
                fs::rename(&inbox_for_hook, &displaced_for_hook).unwrap();
            }
            let line = fs::read_to_string(&events).unwrap();
            let event: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
            let proposal_id = event["proposal_id"].as_str().unwrap();
            let protected = proposals.join(&proposal_id[4..6]).join(proposal_id);
            symlink(protected, &inbox_for_hook).unwrap();
        });
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));

        let result = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-21T02:11:00.000000000Z").unwrap()),
        );
        let line = fs::read_to_string(fixture.store_root.join("intake/events.jsonl")).unwrap();
        let event: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        let proposal_id = event["proposal_id"].as_str().unwrap();
        let protected_candidate = fixture
            .store_root
            .join("intake/proposals")
            .join(&proposal_id[4..6])
            .join(proposal_id)
            .join("candidate.md");

        assert!(
            protected_candidate.is_file(),
            "mutable inbox cleanup deleted protected candidate evidence"
        );
        assert!(result.is_ok(), "custodied cleanup must complete safely");
        fs::remove_file(inbox_root).unwrap();
        if displaced.exists() {
            fs::remove_dir_all(displaced).unwrap();
        }
    }

    #[test]
    fn intake_seal_rechecks_protected_locks_after_acquiring_its_lease() {
        let fixture = ProvisionedP3Fixture::empty("intake-seal-lock-recheck");
        let submitted = submit_valid_intake_request(&fixture, "lock-recheck");
        let conflicting_lock = fixture.store_root.join("locks/verdict.lock");
        install_intake_seal_post_precheck_hook_for_test(move || {
            fs::create_dir(&conflicting_lock).unwrap();
        });
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));

        let error = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-21T02:12:00.000000000Z").unwrap()),
        )
        .unwrap_err();

        assert_eq!(error.code(), "intake.locked");
        assert_eq!(human.show_count(), 0);
        assert!(fixture
            .store_root
            .join("staging/inbox")
            .join(submitted.submission_id)
            .is_dir());
        assert!(fs::read(fixture.store_root.join("intake/events.jsonl"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn intake_seal_release_failure_retains_custody_after_failed_confirmation() {
        let fixture = ProvisionedP3Fixture::empty("intake-seal-release-masks-confirmation");
        let submitted = submit_valid_intake_request(&fixture, "release-masks-confirmation");
        install_intake_lease_release_finalization_failure_for_test();
        let mut human = ScriptedHuman::new(Some("NOT THE CONFIRMATION\n".to_owned()));

        let error = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-21T02:13:00.000000000Z").unwrap()),
        )
        .unwrap_err();

        assert_eq!(error.code(), "intake.incomplete");
        assert!(fixture.store_root.join("locks/intake.lock").is_dir());
        assert!(!fixture
            .store_root
            .join("staging/inbox")
            .join(&submitted.submission_id)
            .exists());
        assert!(fixture
            .store_root
            .join("intake/transactions")
            .join(submitted.submission_id)
            .is_dir());
    }

    #[test]
    fn intake_seal_retains_protected_custody_after_pair_publication_without_event_commit() {
        let fixture = ProvisionedP3Fixture::empty("intake-seal-pair-without-event");
        let submitted = submit_valid_intake_request(&fixture, "pair-without-event");
        install_intake_event_pre_rename_sync_failure_for_test();
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));

        let error = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-21T02:14:00.000000000Z").unwrap()),
        )
        .unwrap_err();

        assert_eq!(error.code(), "intake.incomplete");
        assert!(fixture
            .store_root
            .join("intake/transactions")
            .join(&submitted.submission_id)
            .is_dir());
        assert!(!fixture
            .store_root
            .join("staging/inbox")
            .join(&submitted.submission_id)
            .exists());
        assert!(fs::read(fixture.store_root.join("intake/events.jsonl"))
            .unwrap()
            .is_empty());
        assert!(fs::read_dir(fixture.store_root.join("intake/proposals"))
            .unwrap()
            .any(|entry| entry.unwrap().path().read_dir().unwrap().next().is_some()));
    }

    #[test]
    fn intake_seal_pair_post_rename_failure_retains_claimed_custody() {
        let fixture = ProvisionedP3Fixture::empty("intake-seal-pair-post-rename-failure");
        let submitted = submit_valid_intake_request(&fixture, "pair-post-rename-failure");
        install_intake_pair_post_rename_sync_failure_for_test();
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));

        let error = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-21T02:15:00.000000000Z").unwrap()),
        )
        .unwrap_err();

        assert_eq!(error.code(), "intake.incomplete");
        assert!(fixture
            .store_root
            .join("intake/transactions")
            .join(&submitted.submission_id)
            .is_dir());
        assert!(!fixture
            .store_root
            .join("staging/inbox")
            .join(&submitted.submission_id)
            .exists());
        assert!(fs::read_dir(fixture.store_root.join("intake/proposals"))
            .unwrap()
            .any(|entry| entry.unwrap().path().read_dir().unwrap().next().is_some()));
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
    }

    #[test]
    fn intake_seal_rejected_event_post_rename_failure_retains_claimed_custody() {
        let fixture = ProvisionedP3Fixture::empty("intake-seal-rejected-event-post-rename-failure");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate = String::from_utf8(provisioned_candidate(
            &fixture.store_root,
            target_path,
            target_id,
        ))
        .unwrap()
        .replacen("provenance: synthesis\n", "", 1);
        let candidate_path = fixture
            .profile_path
            .parent()
            .unwrap()
            .join("rejected-event-post-rename-failure.md");
        fs::write(&candidate_path, candidate).unwrap();
        let submitted = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:rejected-event-post-rename-failure".to_owned(),
                operation: ProposalOperation::Replace,
                intent: ProposalIntent::Accept,
                target_id: target_id.to_owned(),
                target_path: target_path.to_owned(),
                candidate_path,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(Timestamp::parse("2026-07-21T02:10:00.000000000Z").unwrap()),
        )
        .unwrap();
        install_intake_event_post_rename_sync_failure_for_test();
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));

        let error = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-21T02:16:00.000000000Z").unwrap()),
        )
        .unwrap_err();

        assert_eq!(error.code(), "intake.incomplete");
        assert!(fixture
            .store_root
            .join("intake/transactions")
            .join(&submitted.submission_id)
            .is_dir());
        assert!(!fixture
            .store_root
            .join("staging/inbox")
            .join(&submitted.submission_id)
            .exists());
        assert_eq!(
            fs::read_to_string(fixture.store_root.join("intake/events.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
        let mut retry_human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        assert!(seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut retry_human,
            &FixedClock(Timestamp::parse("2026-07-21T02:17:00.000000000Z").unwrap()),
        )
        .is_err());
        assert_eq!(retry_human.show_count(), 0);
        assert_eq!(retry_human.read_count(), 0);
        assert_eq!(
            fs::read_to_string(fixture.store_root.join("intake/events.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }

    #[test]
    fn design_valid_p3_target_id_retains_the_markdown_extension() {
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("submit-p3-id-with-extension");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let p2_target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog";
        let p3_target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate = String::from_utf8(
            std::fs::read(fixture.store_root.join("seed/bundle").join(target_path)).unwrap(),
        )
        .unwrap()
        .replace(p2_target_id, p3_target_id);
        let candidate = fixture.input_file("p3-id-with-extension", candidate.as_bytes());

        let staged = submit_proposal(ProposalSubmission {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            source_id: "src:agent:fixture".to_owned(),
            operation: ProposalOperation::Replace,
            intent: ProposalIntent::Rewrite,
            target_id: p3_target_id.to_owned(),
            target_path: target_path.to_owned(),
            candidate_path: candidate,
            confidence: None,
            evaluator_path: None,
        })
        .expect("approved P3 IDs retain the canonical .md path");

        assert_eq!(staged.target_id, p3_target_id);
    }

    #[test]
    fn rejected_intake_is_created_only_by_a_confirmed_human_seal() {
        let fixture = ProvisionedP3Fixture::empty("submit-rejected");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate = String::from_utf8(provisioned_candidate(
            &fixture.store_root,
            target_path,
            "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
        ))
        .unwrap()
        .replacen("provenance: synthesis\n", "", 1)
            + "\nP3_PRIVATE_CANDIDATE_SENTINEL\n";
        let candidate_path = fixture.profile_path.parent().unwrap().join("rejected.md");
        fs::write(&candidate_path, candidate.as_bytes()).unwrap();

        let submitted = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:fixture".to_owned(),
                operation: ProposalOperation::Replace,
                intent: ProposalIntent::Accept,
                target_id: "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"
                    .to_owned(),
                target_path: target_path.to_owned(),
                candidate_path,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(Timestamp::parse("2026-07-21T02:00:00.000000000Z").unwrap()),
        )
        .expect("untrusted transport does not decide policy");
        assert!(
            std::fs::read(fixture.store_root.join("intake/events.jsonl"))
                .unwrap()
                .is_empty()
        );
        let mut human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let outcome = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-21T02:01:00.000000000Z").unwrap()),
        )
        .expect("human seals deterministic rejection");
        assert_eq!(outcome.state, "rejected-at-intake");
        assert!(outcome.proposal_id.is_none());
        assert_eq!(outcome.intake_sequence, 1);
        let event =
            std::fs::read_to_string(fixture.store_root.join("intake/events.jsonl")).unwrap();
        assert!(event.contains("proposal.provenance_required"));
        assert!(!event.contains("P3_PRIVATE_CANDIDATE_SENTINEL"));
        assert!(!fixture
            .store_root
            .join("staging/inbox")
            .join(submitted.submission_id)
            .exists());
    }

    #[test]
    fn proposal_and_verdict_views_project_only_verified_redacted_evidence() {
        let fixture = sovmem::profile::p3_internal::p3::P3Fixture::new("proposal-views");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = sovmem::profile::load_p3_profile(&fixture.profile_path).unwrap();

        let all = proposal_list(
            &profile,
            &fixture.store_root,
            ProposalFilter { state: None },
        )
        .expect("list verified proposal records");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].state, ProposalState::Ruled);
        assert_eq!(all[1].state, ProposalState::Applied);
        assert!(all.iter().all(|record| record.target_path.is_none()));
        assert!(all.iter().all(|record| record.reason.is_none()));

        let retained = proposal_list(
            &profile,
            &fixture.store_root,
            ProposalFilter {
                state: Some(ProposalState::Retained),
            },
        )
        .expect("list retained proposal records");
        assert_eq!(retained.len(), 2);
        assert!(retained.iter().all(|record| record.retained));

        let verdicts = verdict_list(&profile, &fixture.store_root, false, false)
            .expect("list verified verdict records");
        assert_eq!(verdicts.len(), 2);
        assert!(verdicts.iter().all(|record| record.reason.is_none()));
        assert!(verdicts
            .iter()
            .all(|record| record.evaluator_reasons.is_none()));
    }

    #[test]
    fn proposal_views_cover_staged_rejected_and_status_states_before_genesis() {
        let fixture =
            sovmem::profile::p3_internal::p3::P3Fixture::new("proposal-pre-genesis-views");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate_bytes = p3_seed_candidate(&fixture, target_path, target_id);
        let candidate = fixture.input_file("pre-genesis-candidate", &candidate_bytes);
        let submitted = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:fixture".to_owned(),
                operation: ProposalOperation::Replace,
                intent: ProposalIntent::Accept,
                target_id: target_id.to_owned(),
                target_path: target_path.to_owned(),
                candidate_path: candidate,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(Timestamp::parse("2026-07-21T02:10:00.000000000Z").unwrap()),
        )
        .expect("submit pre-genesis staged fixture");
        assert!(submitted.proposal_id.is_empty());
        let mut staged_human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let staged = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &submitted.submission_id,
            &mut staged_human,
            &FixedClock(Timestamp::parse("2026-07-21T02:10:01.000000000Z").unwrap()),
        )
        .expect("seal pre-genesis staged fixture");
        let staged_id = staged
            .proposal_id
            .expect("policy-passing seal publishes a protected proposal id");

        let rejected_bytes = String::from_utf8(candidate_bytes)
            .unwrap()
            .replacen("provenance: synthesis\n", "", 1)
            .into_bytes();
        let rejected_candidate = fixture.input_file("pre-genesis-rejected", &rejected_bytes);
        let rejected = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: "src:agent:fixture-rejected".to_owned(),
                operation: ProposalOperation::Replace,
                intent: ProposalIntent::Accept,
                target_id: target_id.to_owned(),
                target_path: target_path.to_owned(),
                candidate_path: rejected_candidate,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(Timestamp::parse("2026-07-21T02:10:02.000000000Z").unwrap()),
        )
        .expect("submit pre-genesis rejected fixture");
        assert!(rejected.proposal_id.is_empty());
        let mut rejected_human =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", rejected.submission_id)));
        let rejected = seal_intake_submission_with_human(
            &fixture.profile_path,
            &fixture.store_root,
            &rejected.submission_id,
            &mut rejected_human,
            &FixedClock(Timestamp::parse("2026-07-21T02:10:03.000000000Z").unwrap()),
        )
        .expect("seal pre-genesis rejected fixture");
        assert_eq!(rejected.state, "rejected-at-intake");
        assert!(rejected.proposal_id.is_none());
        let profile = sovmem::profile::load_p3_profile(&fixture.profile_path).unwrap();

        let all = proposal_list(
            &profile,
            &fixture.store_root,
            ProposalFilter { state: None },
        )
        .unwrap();
        assert_eq!(all.len(), 2);
        let rejected_record = all
            .iter()
            .find(|record| record.state == ProposalState::RejectedAtIntake)
            .expect("project protected intake rejection");
        assert!(rejected_record.receipt_id.is_some());
        assert!(rejected_record.source_id.is_some());
        let staged_record = all
            .iter()
            .find(|record| record.state == ProposalState::Staged)
            .expect("project protected staged pair");
        assert_eq!(staged_record.proposal_id, staged_id);
        assert!(all.iter().all(|record| record.target_path.is_none()));

        let staged_only = proposal_list(
            &profile,
            &fixture.store_root,
            ProposalFilter {
                state: Some(ProposalState::Staged),
            },
        )
        .unwrap();
        assert_eq!(staged_only.len(), 1);
        assert_eq!(staged_only[0].proposal_id, staged_id);
        assert_eq!(
            proposal_status(&profile, &fixture.store_root, &staged_id)
                .unwrap()
                .state,
            ProposalState::Staged
        );
    }

    fn p3_seed_candidate(
        fixture: &sovmem::profile::p3_internal::p3::P3Fixture,
        target_path: &str,
        target_id: &str,
    ) -> Vec<u8> {
        let frozen_id = target_id.trim_end_matches(".md");
        String::from_utf8(
            std::fs::read(fixture.store_root.join("seed/bundle").join(target_path)).unwrap(),
        )
        .unwrap()
        .replace(frozen_id, target_id)
        .into_bytes()
    }

    fn provisioned_candidate(store_root: &Path, target_path: &str, target_id: &str) -> Vec<u8> {
        let frozen_id = target_id.trim_end_matches(".md");
        String::from_utf8(fs::read(store_root.join("seed/bundle").join(target_path)).unwrap())
            .unwrap()
            .replace(frozen_id, target_id)
            .into_bytes()
    }

    fn submit_valid_intake_request(
        fixture: &ProvisionedP3Fixture,
        marker: &str,
    ) -> sovmem::profile::SubmitProposalResult {
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let candidate_path = fixture
            .profile_path
            .parent()
            .unwrap()
            .join(format!("{marker}.md"));
        let mut candidate = provisioned_candidate(&fixture.store_root, target_path, target_id);
        candidate.extend_from_slice(format!("\n{marker}\n").as_bytes());
        fs::write(&candidate_path, candidate).unwrap();
        submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
                source_id: format!("src:agent:{marker}"),
                operation: ProposalOperation::Replace,
                intent: ProposalIntent::Accept,
                target_id: target_id.to_owned(),
                target_path: target_path.to_owned(),
                candidate_path,
                confidence: None,
                evaluator_path: None,
            },
            &FixedClock(Timestamp::parse("2026-07-21T02:10:00.000000000Z").unwrap()),
        )
        .unwrap()
    }
}

pub mod store_tests {
    use sovmem::profile::p3_test_support::ProvisionedP3Fixture;

    use sovmem::profile::p3_internal::p3::P3Fixture;
    use sovmem::profile::{
        inspect_store, load_p3_profile, store_stats, validate_bundle, verify_generation,
        verify_store_level4, verify_unsealed_store, Timestamp, VerificationLevel,
        P3_PROFILE_VERSION,
    };
    use std::fs;
    use std::path::Path;

    type StoreCorruption = (&'static str, fn(&Path));

    #[test]
    fn p3_v2_store_rejects_v1_layout_and_removed_staging_roots() {
        let fixture = ProvisionedP3Fixture::empty("v2-store-layout");
        let v2_profile = load_p3_profile(&repository_path("profiles/audy-ssa0-p3-v2/profile.yaml"))
            .expect("load reviewed v2 profile");
        let v1_store = fixture.store_root.with_file_name("p3-v1-store");
        copy_tree(
            &repository_path("sovmem/tests/fixtures/p3-cli-store-v1/one-applied/store"),
            &v1_store,
        );

        assert_eq!(
            inspect_store(&v2_profile, &v1_store).unwrap_err().code(),
            "store.layout_invalid"
        );

        let expected_roots = [
            "policy.toml",
            "policies",
            "seed",
            "staging",
            "intake",
            "ledger",
            "canon",
            "evidence",
            "derived",
            "transactions",
            "locks",
        ];
        let mut actual_roots = fs::read_dir(&fixture.store_root)
            .expect("read v2 store root")
            .map(|entry| {
                entry
                    .expect("read v2 store root entry")
                    .file_name()
                    .into_string()
                    .expect("v2 store root name is UTF-8")
            })
            .collect::<Vec<_>>();
        actual_roots.sort();
        let mut expected_roots = expected_roots.map(str::to_owned).to_vec();
        expected_roots.sort();
        assert_eq!(actual_roots, expected_roots);
        assert!(inspect_store(&v2_profile, &fixture.store_root).is_ok());

        for forbidden in ["staging/proposals", "staging/intake-rejections"] {
            fs::create_dir_all(fixture.store_root.join(forbidden)).unwrap();
            assert_eq!(
                inspect_store(&v2_profile, &fixture.store_root)
                    .unwrap_err()
                    .code(),
                "store.layout_invalid"
            );
            fs::remove_dir_all(fixture.store_root.join(forbidden)).unwrap();
        }
    }

    #[test]
    fn p3_v2_profile_isolated_from_v1() {
        let v1 = repository_path("profiles/audy-ssa0-p3-v1/profile.yaml");
        assert_eq!(
            load_p3_profile(&v1).unwrap_err().code(),
            "profile.identifier"
        );

        let v2 = load_p3_profile(&repository_path("profiles/audy-ssa0-p3-v2/profile.yaml"))
            .expect("load reviewed v2 profile");
        assert_eq!(v2.version, P3_PROFILE_VERSION);
        assert_eq!(
            v2.hash,
            "66aa2c017c04a968a67aa314ce132cb7467b0382cac7585b3f83e5524941b59f"
        );
    }

    #[test]
    fn p3_v2_intake_transaction_residue_fails_closed() {
        let fixture = ProvisionedP3Fixture::empty("v2-intake-transaction-residue");
        let profile = load_p3_profile(&fixture.profile_path).expect("load v2 fixture profile");

        let intake_transaction = fixture
            .store_root
            .join("intake/transactions")
            .join(format!("txn-{}", "a".repeat(64)));
        fs::create_dir(&intake_transaction).expect("materialize intake transaction");
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
    }

    #[test]
    fn p3_v2_intake_lock_fails_closed() {
        let fixture = ProvisionedP3Fixture::empty("v2-intake-lock");
        let profile = load_p3_profile(&fixture.profile_path).expect("load v2 fixture profile");
        let intake_lock = fixture.store_root.join("locks/intake.lock");
        fs::create_dir(&intake_lock).expect("materialize intake lock");
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
    }

    #[test]
    fn p3_v2_intake_events_temporary_fails_closed() {
        let fixture = ProvisionedP3Fixture::empty("v2-intake-events-temporary");
        let profile = load_p3_profile(&fixture.profile_path).expect("load v2 fixture profile");
        let temporary = fixture.store_root.join("intake/.events-0");
        fs::write(&temporary, b"pending\n").expect("materialize intake temporary");
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
    }

    #[test]
    fn p3_v2_intake_pair_temporaries_fail_closed_with_exact_grammar() {
        let fixture = ProvisionedP3Fixture::empty("v2-intake-pair-temporary");
        let profile = load_p3_profile(&fixture.profile_path).expect("load v2 fixture profile");
        let proposal_id = format!("prp-{}", "a".repeat(64));
        let shard = fixture.store_root.join("intake/proposals/aa");
        fs::create_dir(&shard).expect("materialize intake proposal shard");
        let temporary = shard.join(format!(".proposal-{proposal_id}-1-0"));
        fs::create_dir(&temporary).expect("materialize intake pair temporary");
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );

        fs::remove_dir(&temporary).unwrap();
        let malformed = shard.join(format!(".proposal-{proposal_id}-01-0"));
        fs::create_dir(&malformed).expect("materialize malformed intake pair temporary");
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "store.layout_invalid"
        );

        fs::remove_dir(&malformed).unwrap();
        let cross_shard = fixture.store_root.join("intake/proposals/bb");
        fs::create_dir(&cross_shard).expect("materialize mismatched intake proposal shard");
        let mismatched = cross_shard.join(format!(".proposal-{proposal_id}-1-0"));
        fs::create_dir(&mismatched).expect("materialize cross-shard intake pair temporary");
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "store.layout_invalid"
        );
    }

    #[test]
    fn p3_v2_unicode_intake_pair_temporary_fails_closed_without_panicking() {
        let fixture = ProvisionedP3Fixture::empty("v2-unicode-intake-pair-temporary");
        let profile = load_p3_profile(&fixture.profile_path).expect("load v2 fixture profile");
        let shard = fixture.store_root.join("intake/proposals/aa");
        fs::create_dir(&shard).expect("materialize intake proposal shard");
        let malformed = shard.join(format!(".proposal-prp-{}é-1-0", "a".repeat(63)));
        fs::create_dir(&malformed).expect("materialize Unicode intake pair temporary");

        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "store.layout_invalid"
        );
    }

    #[test]
    fn p3_v2_malformed_intake_temporary_is_rejected() {
        let fixture = ProvisionedP3Fixture::empty("v2-malformed-intake-temporary");
        let profile = load_p3_profile(&fixture.profile_path).expect("load v2 fixture profile");
        let malformed = fixture.store_root.join("intake/.events-00");
        fs::write(&malformed, b"pending\n").expect("materialize malformed intake temporary");
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "store.layout_invalid"
        );
    }

    #[test]
    fn timestamp_contract_is_exact_and_calendar_valid() {
        for valid in [
            "1970-01-01T00:00:00.000000000Z",
            "2024-02-29T23:59:59.999999999Z",
            "2026-07-14T04:30:00.000000000Z",
        ] {
            assert_eq!(Timestamp::parse(valid).unwrap().as_str(), valid);
        }
        for invalid in [
            "2026-07-14T04:30:00Z",
            "2026-02-29T04:30:00.000000000Z",
            "2026-07-14T24:00:00.000000000Z",
            "2026-07-14T04:30:60.000000000Z",
            "2026-07-14T04:30:00.000000000+00:00",
        ] {
            assert!(Timestamp::parse(invalid).is_err(), "{invalid}");
        }
    }

    fn repository_path(relative: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(relative)
    }

    fn copy_tree(source: &Path, destination: &Path) {
        let metadata = fs::symlink_metadata(source).expect("read test source metadata");
        assert!(
            !metadata.file_type().is_symlink(),
            "test source cannot be a symlink"
        );
        if metadata.is_file() {
            fs::write(
                destination,
                fs::read(source).expect("read test source file"),
            )
            .expect("copy test source file");
            return;
        }
        assert!(metadata.is_dir(), "test source is a directory");
        fs::create_dir(destination).expect("create copied test directory");
        let mut entries = fs::read_dir(source)
            .expect("read test source directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read test source entries");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_tree(&entry.path(), &destination.join(entry.file_name()));
        }
    }

    #[test]
    fn unsealed_store_is_a_verified_seed_not_level_four() {
        let fixture = P3Fixture::new("unsealed-store");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store = inspect_store(&profile, &fixture.store_root).unwrap();

        assert_eq!(store.head.as_str(), "seed");
        assert_eq!(store.seed.documents, 62);
        assert_eq!(store.seed.bundle_hash, fixture.p2_bundle_hash());
        assert!(!store.sealed);
        assert_eq!(std::fs::read(&store.paths.current).unwrap(), b"seed\n");
        assert_eq!(std::fs::read(&store.paths.ledger).unwrap(), b"");
        assert!(verify_unsealed_store(&profile, &fixture.store_root).is_ok());
    }

    #[test]
    fn stats_distinguish_verified_seed_and_sealed_chain_state() {
        let fixture = P3Fixture::new("store-stats");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let unsealed = store_stats(&profile, &fixture.store_root).expect("unsealed stats");
        assert!(!unsealed.sealed);
        assert_eq!(unsealed.events, 0);
        assert_eq!(unsealed.ledger_head, None);
        assert_eq!(unsealed.current_generation, "seed");
        assert_eq!(unsealed.queue.open, 0);
        assert!(!unsealed.queue.alarm);

        fixture.install_fixture_chain_with_warning_evidence();
        let sealed = store_stats(&profile, &fixture.store_root).expect("sealed stats");
        assert!(sealed.sealed);
        assert_eq!(sealed.events, 3);
        assert_eq!(sealed.verdicts, 2);
        assert_eq!(sealed.applied_generations, 1);
        assert_eq!(sealed.current_generation, "gen-0000000000000001");
        assert!(sealed.ledger_head.is_some());
    }

    #[test]
    fn sealed_stats_preserve_seed_exclusions_and_current_generation_warnings() {
        let fixture = P3Fixture::new("store-stats-warning-evidence");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let seed = store_stats(&profile, &fixture.store_root).unwrap();
        assert!(
            seed.bundle.warnings > 0,
            "live seed fixture must carry warning evidence"
        );
        assert!(
            seed.bundle.exclusions > 0,
            "live seed fixture must carry exclusions"
        );

        fixture.install_fixture_chain_with_warning_evidence();
        let generation = verify_generation(
            &profile,
            &fixture
                .store_root
                .join("canon/generations/gen-0000000000000001"),
            VerificationLevel::Three,
        )
        .unwrap();
        let expected_warnings = validate_bundle(&profile.config, &generation.generated_documents)
            .warnings()
            .len();
        assert!(
            expected_warnings > 0,
            "live generation fixture must carry warnings"
        );

        let sealed = store_stats(&profile, &fixture.store_root).unwrap();
        assert_eq!(sealed.bundle.warnings, expected_warnings);
        assert_eq!(sealed.bundle.exclusions, seed.bundle.exclusions);
    }

    #[test]
    fn unsealed_public_reads_fail_closed_when_a_transaction_is_visible() {
        let fixture = P3Fixture::new("unsealed-transaction-residue");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        fs::create_dir(
            fixture
                .store_root
                .join("transactions")
                .join(format!("txn-{}", "a".repeat(64))),
        )
        .unwrap();

        assert_eq!(
            verify_unsealed_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
        assert_eq!(
            store_stats(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );

        fs::remove_dir(
            fixture
                .store_root
                .join("transactions")
                .join(format!("txn-{}", "a".repeat(64))),
        )
        .unwrap();
        fs::create_dir(
            fixture
                .store_root
                .join("locks")
                .join(format!(".lease-verdict-{}", "b".repeat(64))),
        )
        .unwrap();
        assert_eq!(
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
        assert_eq!(
            verify_unsealed_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );

        fs::remove_dir(
            fixture
                .store_root
                .join("locks")
                .join(format!(".lease-verdict-{}", "b".repeat(64))),
        )
        .unwrap();
        fs::create_dir(
            fixture
                .store_root
                .join("locks")
                .join(format!(".lease-intake-{}", "c".repeat(64))),
        )
        .unwrap();
        assert_eq!(
            verify_unsealed_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            "transaction.incomplete"
        );
    }

    #[test]
    fn ledger_temporary_grammar_is_exact_for_public_and_level_four_inspection() {
        let fixture = P3Fixture::new("ledger-temporary-grammar");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        for name in [
            ".verdicts-00",
            ".verdicts-18446744073709551616",
            ".verdicts-not-a-sequence",
        ] {
            let invalid = fixture.clone_store(name);
            fs::write(invalid.join("ledger").join(name), b"pending\n").unwrap();
            assert_eq!(
                inspect_store(&profile, &invalid).unwrap_err().code(),
                "store.layout_invalid",
                "public inspection: {name}"
            );
            assert_eq!(
                verify_store_level4(&profile, &invalid).unwrap_err().code(),
                "store.layout_invalid",
                "Level 4: {name}"
            );
        }

        fs::write(fixture.store_root.join("ledger/.verdicts-0"), b"pending\n").unwrap();
        for result in [
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            verify_store_level4(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
        ] {
            assert_eq!(result, "transaction.incomplete");
        }
    }

    #[test]
    fn current_temporary_grammar_is_exact_and_recognized_temps_fail_closed() {
        let fixture = P3Fixture::new("current-temporary-grammar");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).unwrap();

        for name in [
            ".current-not-a-transaction",
            ".current-txn-too-short",
            ".current-txn-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            let invalid = fixture.clone_store(name);
            fs::write(invalid.join("canon").join(name), b"seed\n").unwrap();
            assert_eq!(
                inspect_store(&profile, &invalid).unwrap_err().code(),
                "store.layout_invalid",
                "public inspection: {name}"
            );
            assert_eq!(
                verify_store_level4(&profile, &invalid).unwrap_err().code(),
                "store.layout_invalid",
                "Level 4: {name}"
            );
        }

        fs::write(
            fixture
                .store_root
                .join("canon")
                .join(format!(".current-txn-{}", "a".repeat(64))),
            b"gen-0000000000000001\n",
        )
        .unwrap();
        for result in [
            inspect_store(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
            verify_store_level4(&profile, &fixture.store_root)
                .unwrap_err()
                .code(),
        ] {
            assert_eq!(result, "transaction.incomplete");
        }
    }

    #[test]
    fn generation_and_index_staging_grammar_is_exact_and_fail_closed() {
        let fixture = P3Fixture::new("generation-index-staging-grammar");
        fixture.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&fixture.profile_path).unwrap();

        for (index, relative) in [
            "canon/generations/.txn-not-a-transaction",
            "derived/indexes/.txn-txn-too-short",
            "canon/generations/.txn-txn-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ]
        .into_iter()
        .enumerate()
        {
            let invalid = fixture.clone_store(&format!("invalid-staging-{index}"));
            fs::create_dir_all(invalid.join(relative)).unwrap();
            assert_eq!(
                inspect_store(&profile, &invalid).unwrap_err().code(),
                "store.layout_invalid",
                "public inspection: {relative}"
            );
            assert_eq!(
                verify_store_level4(&profile, &invalid).unwrap_err().code(),
                "store.layout_invalid",
                "Level 4: {relative}"
            );
        }

        for (index, relative) in [
            format!("canon/generations/.txn-txn-{}", "a".repeat(64)),
            format!("derived/indexes/.txn-txn-{}", "b".repeat(64)),
        ]
        .into_iter()
        .enumerate()
        {
            let staged = fixture.clone_store(&format!("recognized-staging-{index}"));
            fs::create_dir_all(staged.join(&relative)).unwrap();
            for result in [
                inspect_store(&profile, &staged).unwrap_err().code(),
                verify_store_level4(&profile, &staged).unwrap_err().code(),
            ] {
                assert_eq!(result, "transaction.incomplete", "{relative}");
            }
        }
    }

    #[test]
    fn corrupt_store_shapes_fail_closed_with_stable_codes() {
        let fixture = P3Fixture::new("store-corruption");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();

        let cases: [StoreCorruption; 6] = [
            ("extra root entry", |root| {
                fs::write(root.join("unexpected"), b"x").unwrap();
            }),
            ("missing durable directory", |root| {
                fs::remove_dir(root.join("locks")).unwrap();
            }),
            ("malformed current", |root| {
                fs::write(root.join("canon/CURRENT"), b"gen-01\n").unwrap();
            }),
            ("wrong policy identity", |root| {
                let policy = fs::read_to_string(root.join("policy.toml")).unwrap();
                fs::write(
                    root.join("policy.toml"),
                    policy.replacen("audy-sovmem-writeback-v2", "foreign-store", 1),
                )
                .unwrap();
            }),
            ("changed seed bytes", |root| {
                fs::write(root.join("seed/manifest.json"), b"{}\n").unwrap();
            }),
            ("missing level three evidence", |root| {
                fs::remove_file(root.join("seed/corpus-report.json")).unwrap();
            }),
        ];

        for (name, corrupt) in cases {
            let store_root = fixture.clone_store(name);
            corrupt(&store_root);
            let error = inspect_store(&profile, &store_root).expect_err(name);
            assert_eq!(error.code(), "store.layout_invalid", "{name}");
        }

        let nested = fixture.store_root.join("seed");
        let error = inspect_store(&profile, &nested).expect_err("store root inside seed");
        assert_eq!(error.code(), "store.layout_invalid");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_store_entries_are_rejected_before_layout_evaluation() {
        use std::os::unix::fs::symlink;

        let fixture = P3Fixture::new("store-symlink");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store_root = fixture.clone_store("symlink");
        symlink(store_root.join("seed"), store_root.join("link")).unwrap();

        let error = inspect_store(&profile, &store_root).expect_err("symlink is rejected");
        assert_eq!(error.code(), "store.symlink");
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_store_names_are_rejected_before_layout_evaluation() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let fixture = P3Fixture::new("store-non-utf8");
        let profile = load_p3_profile(&fixture.profile_path).unwrap();
        let store_root = fixture.clone_store("non-utf8");
        match fs::write(store_root.join(OsString::from_vec(vec![0xff])), b"x") {
            Ok(()) => {}
            Err(error) => {
                let generic_unmaterializable = matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::InvalidInput
                );
                #[cfg(target_os = "macos")]
                let platform_unmaterializable = error.raw_os_error() == Some(92);
                #[cfg(not(target_os = "macos"))]
                let platform_unmaterializable = false;
                if !generic_unmaterializable && !platform_unmaterializable {
                    panic!("write non-UTF-8 fixture entry: {error}");
                }
                // Permission/input errors are generic; macOS alone may report raw error 92 here.
                return;
            }
        }

        let error = inspect_store(&profile, &store_root).expect_err("non-UTF-8 entry is rejected");
        assert_eq!(error.code(), "store.path_unsafe");
    }
}
