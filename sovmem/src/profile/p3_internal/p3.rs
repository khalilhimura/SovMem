#![allow(dead_code)]
// Private provisioning fixture for the internal P3 suite.

use super::p2::P2Fixture;
use crate::profile::transaction::seal_intake_submission_with_human;
use sovmem::profile::{
    build_judgment, build_p3_index_generation, inspect_store, load_p3_profile, load_policy,
    load_profile, parse_source_document, query_p3_generation, render_document, stage_generation,
    verdict_id, verify_bundle, verify_reindex_receipt, verify_store_level4, AppliedGenerationInput,
    CanonicalResult, ExpansionMode, GeneratedDocument, GeneratedMetadata, GenerationPlan,
    GenesisEvent, GenesisPayload, LedgerEvent, P3IndexEvidence, P3IndexRequest, ProposalIntent,
    ProposalMeta, ProposalOperation, ProposalSubmission, QueryResult, Reason, RewriteBytes,
    SourceDocument, Timestamp, VerdictEvent, VerdictIdentityInput, VerdictPayload, VerdictVerb,
    VerificationLevel, VerificationOptions, VerifiedGeneration, LEDGER_EVENT_SCHEMA,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

pub struct P3Fixture {
    p2: P2Fixture,
    pub profile_path: PathBuf,
    pub policy_template: PathBuf,
    pub store_root: PathBuf,
    candidate_path: PathBuf,
}

struct FixtureProposal {
    meta: ProposalMeta,
    meta_hash: String,
    candidate_hash: String,
    candidate_bytes: Vec<u8>,
    intake_sequence: u64,
    intake_head_hash: String,
}

impl P3Fixture {
    pub fn new(name: &str) -> Self {
        Self::new_with_p2(P2Fixture::new(name))
    }

    pub fn new_with_resolvable_wikilink(name: &str) -> Self {
        Self::new_with_p2(P2Fixture::new_with_resolvable_wikilink(name))
    }

    fn new_with_p2(p2: P2Fixture) -> Self {
        // NOTE: P3 fixtures build full Tantivy indexes and copy 5MB+ fixture
        // trees. These lib tests exhibit test pollution under cargo's concurrent
        // runner; run individually with `cargo test <exact-name>`. The external
        // integration tests (p3_cli.rs, p3_privacy.rs) cover the same P3
        // functionality reliably.
        let store_root = p2.index_root("p3-store");
        let profile_path = repository_path("profiles/audy-ssa0-p3-v2/profile.yaml");
        let policy_template = repository_path("profiles/audy-ssa0-p3-v2/policy.toml");
        provision_test_store(&p2, &profile_path, &policy_template, &store_root);
        let mut fixture = Self {
            p2,
            profile_path,
            policy_template,
            store_root,
            candidate_path: PathBuf::new(),
        };
        let target_path = "04-Research/CLI/README.md";
        let target_id = fixture_id(target_path);
        fixture.candidate_path = fixture.input_file(
            "cli-candidate",
            &created_target_bytes(&fixture, target_path, &target_id),
        );
        fixture
    }

    pub fn new_from_checked_in_seed(name: &str) -> Self {
        let fixture = Self::new(name);
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/p3-cli-store-v1/one-applied/store/seed");
        fs::remove_dir_all(fixture.store_root.join("seed"))
            .expect("remove synthetic seed before checked-in seed copy");
        copy_tree(&source, &fixture.store_root.join("seed"));
        fixture
    }

    pub fn profile_arg(&self) -> &str {
        self.profile_path
            .to_str()
            .expect("UTF-8 fixture profile path")
    }

    pub fn store_arg(&self) -> &str {
        self.store_root.to_str().expect("UTF-8 fixture store path")
    }

    pub fn candidate_arg(&self) -> &str {
        self.candidate_path
            .to_str()
            .expect("UTF-8 fixture candidate path")
    }

    pub fn p2_bundle_arg(&self) -> &str {
        self.p2
            .bundle_root
            .to_str()
            .expect("UTF-8 fixture P2 bundle path")
    }

    pub fn cli_candidate(&self, name: &str, target_path: &str) -> PathBuf {
        self.input_file(
            name,
            &created_target_bytes(self, target_path, &fixture_id(target_path)),
        )
    }

    pub fn parse_single_json_stdout(&self, output: &Output) -> serde_json::Value {
        let stdout = String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8");
        assert_eq!(stdout.lines().count(), 1, "stdout: {stdout:?}");
        assert!(stdout.ends_with('\n'), "JSON stdout must end in LF");
        serde_json::from_str(stdout.trim_end()).expect("stdout is one JSON object")
    }

    pub fn assert_machine_streams_redacted(&self, output: &Output) {
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for path in [&self.profile_path, &self.store_root, &self.candidate_path] {
            assert!(
                !combined.contains(path.to_string_lossy().as_ref()),
                "machine output leaked a fixture path: {combined}"
            );
        }
    }

    pub fn p2_bundle_hash(&self) -> String {
        self.p2.bundle_hash()
    }

    pub fn clone_store(&self, name: &str) -> PathBuf {
        let destination = self.p2.index_root(&format!("p3-store-{name}"));
        copy_tree(&self.store_root, &destination);
        destination
    }

    pub fn input_file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.p2.index_root(&format!("p3-input-{name}.md"));
        fs::write(&path, bytes).expect("write proposal input");
        path
    }

    pub fn stage_accept_create(&self) -> VerifiedGeneration {
        let profile = load_p3_profile(&self.profile_path).expect("load P3 profile");
        let policy = load_policy(&self.policy_template).expect("load P3 policy");
        let target_path = "04-Research/P3-Generated-Canon.md";
        let target_id = fixture_id(target_path);
        let target_bytes = created_target_bytes(self, target_path, &target_id);
        let verdict_id = fixture_hash_id("ver-", 'b');
        let reason = Reason::new(b"fixture create reason", policy.policy.max_reason_bytes)
            .expect("fixture reason");
        let ruled_at = fixture_timestamp();
        let judgment = build_judgment(
            &profile,
            &verdict_id,
            VerdictVerb::Accept,
            &target_id,
            &reason,
            &ruled_at,
        )
        .expect("build create judgment")
        .expect("ACCEPT creates judgment");
        let plan = GenerationPlan::from_seed(
            &profile,
            &self.store_root.join("seed"),
            AppliedGenerationInput {
                generation_id: "gen-0000000000000001".to_owned(),
                proposal_id: fixture_hash_id("prp-", 'a'),
                verdict_id,
                policy_hash: policy.hash,
                operation: ProposalOperation::Create,
                target_path: target_path.to_owned(),
                target_id,
                target_bytes,
                judgment,
            },
        )
        .expect("plan create generation");
        stage_generation(
            &profile,
            &plan,
            &self.p2.index_root(".p3-create-generation"),
        )
        .expect("stage create generation")
    }

    pub fn stage_rewrite_replace(&self, parent: &VerifiedGeneration) -> VerifiedGeneration {
        let profile = load_p3_profile(&self.profile_path).expect("load P3 profile");
        let policy = load_policy(&self.policy_template).expect("load P3 policy");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = fixture_id(target_path);
        let frozen_id = target_id.trim_end_matches(".md");
        let mut target_bytes = String::from_utf8(
            std::fs::read(self.store_root.join("seed/bundle").join(target_path))
                .expect("read seeded replacement target"),
        )
        .expect("seeded replacement target is UTF-8")
        .replace(frozen_id, &target_id)
        .into_bytes();
        target_bytes.extend_from_slice(b"\nP3 fixture rewritten exact bytes.\n");
        let verdict_id = fixture_hash_id("ver-", 'd');
        let reason = Reason::new(b"fixture rewrite reason", policy.policy.max_reason_bytes)
            .expect("fixture reason");
        let ruled_at = fixture_timestamp();
        let judgment = build_judgment(
            &profile,
            &verdict_id,
            VerdictVerb::Rewrite,
            &target_id,
            &reason,
            &ruled_at,
        )
        .expect("build rewrite judgment")
        .expect("REWRITE creates judgment");
        let plan = GenerationPlan::from_generation(
            &profile,
            parent,
            AppliedGenerationInput {
                generation_id: "gen-0000000000000002".to_owned(),
                proposal_id: fixture_hash_id("prp-", 'c'),
                verdict_id,
                policy_hash: policy.hash,
                operation: ProposalOperation::Replace,
                target_path: target_path.to_owned(),
                target_id,
                target_bytes,
                judgment,
            },
        )
        .expect("plan replacement generation");
        stage_generation(
            &profile,
            &plan,
            &self.p2.index_root(".p3-replace-generation"),
        )
        .expect("stage replacement generation")
    }

    pub fn target_bytes_equal_approved(&self, generation: &VerifiedGeneration) -> bool {
        let target = generation.manifest.documents.iter().find(|document| {
            document.proposal_id.as_deref()
                == Some(generation.manifest.applied_proposal_id.as_str())
                && document.verdict_id.as_deref()
                    == Some(generation.manifest.applied_verdict_id.as_str())
                && !document.canonical_path.starts_with("judgments/")
        });
        let Some(target) = target else {
            return false;
        };
        let expected = match generation.manifest.generation_id.as_str() {
            "gen-0000000000000001" => created_target_bytes(
                self,
                "04-Research/P3-Generated-Canon.md",
                &fixture_id("04-Research/P3-Generated-Canon.md"),
            ),
            "gen-0000000000000002" => {
                let mut bytes = match std::fs::read(self.store_root.join(
                    "seed/bundle/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                )) {
                    Ok(bytes) => bytes,
                    Err(_) => return false,
                };
                bytes = match String::from_utf8(bytes) {
                    Ok(text) => text
                        .replace(
                            "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog",
                            "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
                        )
                        .into_bytes(),
                    Err(_) => return false,
                };
                bytes.extend_from_slice(b"\nP3 fixture rewritten exact bytes.\n");
                bytes
            }
            _ => return false,
        };
        std::fs::read(generation.root.join(&target.output_path)).ok() == Some(expected)
    }

    pub fn judgment_bytes_equal_approved(&self, generation: &VerifiedGeneration) -> bool {
        let profile = match load_p3_profile(&self.profile_path) {
            Ok(profile) => profile,
            Err(_) => return false,
        };
        let target = match generation.manifest.documents.iter().find(|document| {
            document.proposal_id.as_deref()
                == Some(generation.manifest.applied_proposal_id.as_str())
                && document.verdict_id.as_deref()
                    == Some(generation.manifest.applied_verdict_id.as_str())
                && !document.canonical_path.starts_with("judgments/")
        }) {
            Some(target) => target,
            None => return false,
        };
        let (verb, reason) = match generation.manifest.generation_id.as_str() {
            "gen-0000000000000001" => (VerdictVerb::Accept, b"fixture create reason".as_slice()),
            "gen-0000000000000002" => (VerdictVerb::Rewrite, b"fixture rewrite reason".as_slice()),
            _ => return false,
        };
        let policy = match load_policy(&self.policy_template) {
            Ok(policy) => policy,
            Err(_) => return false,
        };
        let reason = match Reason::new(reason, policy.policy.max_reason_bytes) {
            Ok(reason) => reason,
            Err(_) => return false,
        };
        let judgment = match build_judgment(
            &profile,
            &generation.manifest.applied_verdict_id,
            verb,
            &target.id,
            &reason,
            &fixture_timestamp(),
        ) {
            Ok(Some(judgment)) => judgment,
            _ => return false,
        };
        std::fs::read(generation.root.join("bundle").join(&judgment.path)).ok()
            == Some(judgment.bytes)
    }

    pub fn seed_lineage_preserved(&self, generation: &VerifiedGeneration) -> bool {
        generation.manifest.documents.iter().any(|document| {
            document.canonical_path
                == "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md"
                && document.seed_source_path.as_deref()
                    == Some("04-Research/Agent-Loop-Engineering/Implementation-Backlog.md")
                && document.seed_source_hash.is_some()
                && document.previous_hash.is_some()
        })
    }

    pub fn build_p3_index(&self, generation: &VerifiedGeneration) -> P3IndexEvidence {
        let profile = load_p3_profile(&self.profile_path).expect("load P3 profile");
        build_p3_index_generation(P3IndexRequest {
            profile: &profile,
            generation,
            index_root: &self.p3_index_root(generation),
        })
        .expect("build P3 generation index")
    }

    pub fn query_p3(&self, generation: &VerifiedGeneration, query: &str) -> QueryResult {
        let profile = load_p3_profile(&self.profile_path).expect("load P3 profile");
        query_p3_generation(
            &profile,
            generation,
            &self.p3_index_root(generation),
            query.to_owned(),
            5,
            ExpansionMode::None,
        )
        .expect("query P3 generation")
    }

    pub fn verify_p3_receipt(
        &self,
        generation: &VerifiedGeneration,
        evidence: &P3IndexEvidence,
    ) -> bool {
        let Ok(profile) = load_p3_profile(&self.profile_path) else {
            return false;
        };
        verify_reindex_receipt(
            &profile,
            generation,
            &self.p3_index_root(generation),
            &evidence.receipt,
            &evidence.receipt_bytes,
        )
        .is_ok()
    }

    pub fn rejects_tampered_p3_receipt(
        &self,
        generation: &VerifiedGeneration,
        evidence: &P3IndexEvidence,
    ) -> bool {
        let Ok(profile) = load_p3_profile(&self.profile_path) else {
            return false;
        };
        let mut bytes = evidence.receipt_bytes.clone();
        bytes[0] ^= 1;
        verify_reindex_receipt(
            &profile,
            generation,
            &self.p3_index_root(generation),
            &evidence.receipt,
            &bytes,
        )
        .is_err()
    }

    fn p3_index_root(&self, generation: &VerifiedGeneration) -> PathBuf {
        self.p2
            .index_root(&format!(".p3-index-{}", generation.manifest.generation_id))
    }

    pub fn delete_p3_index(&self, generation: &VerifiedGeneration) {
        std::fs::remove_dir_all(self.p3_index_root(generation)).expect("delete P3 index");
    }

    pub fn rejects_p3_index_inside_generation(&self, generation: &VerifiedGeneration) -> bool {
        let Ok(profile) = load_p3_profile(&self.profile_path) else {
            return false;
        };
        build_p3_index_generation(P3IndexRequest {
            profile: &profile,
            generation,
            index_root: &generation.root.join(".forbidden-derived-index"),
        })
        .is_err()
    }

    pub fn fixture_genesis_and_reject(&self) -> Vec<LedgerEvent> {
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = fixture_id(target_path);
        let candidate_bytes = String::from_utf8(
            fs::read(self.store_root.join("seed/bundle").join(target_path))
                .expect("read fixture ledger candidate"),
        )
        .expect("fixture ledger candidate is UTF-8")
        .replace(target_id.trim_end_matches(".md"), &target_id)
        .into_bytes();
        let rejected = self.submit_fixture_proposal_from(
            "ledger-reject",
            "src:agent:fixture-ledger",
            ProposalOperation::Replace,
            ProposalIntent::Accept,
            target_path,
            &target_id,
            &candidate_bytes,
        );
        let profile = load_p3_profile(&self.profile_path).expect("load P3 profile");
        let store = inspect_store(&profile, &self.store_root).expect("inspect fixture store");
        sovmem::profile::archive_policy(&store.paths.policies, &store.policy)
            .expect("archive fixture ledger policy");
        let genesis = GenesisEvent::new(GenesisPayload {
            schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "genesis".to_owned(),
            sequence: 0,
            event_id: "evt-0000000000000000".to_owned(),
            created_at: fixture_timestamp().as_str().to_owned(),
            previous_event_hash: None,
            store_id: store.policy.policy.store_id.clone(),
            profile_version: profile.version,
            profile_hash: profile.hash.clone(),
            policy_hash: store.policy.hash.clone(),
            seed_manifest_hash: store.seed.manifest_hash.clone(),
            seed_corpus_report_hash: store.seed.corpus_report_hash.clone(),
            seed_bundle_hash: store.seed.bundle_hash.clone(),
            source_commit: store.seed.source_commit.clone(),
            initial_canonical_ref: "seed".to_owned(),
        })
        .expect("create fixture genesis");
        let reason = Reason::new(
            b"fixture ledger reject",
            store.policy.policy.max_reason_bytes,
        )
        .expect("build fixture ledger reason");
        let reject_verdict_id = verdict_id(&VerdictIdentityInput {
            proposal_id: &rejected.meta.proposal_id,
            verb: VerdictVerb::Reject,
            reason: &reason,
            rewrite: None,
            actor: &store.policy.policy.human_actor,
            ruled_at: &fixture_timestamp(),
            prior_ledger_head: Some(genesis.hash()),
            intake_sequence: rejected.intake_sequence,
            intake_head_hash: Some(&rejected.intake_head_hash),
            policy_hash: &store.policy.hash,
            profile_hash: &profile.hash,
        });
        let reject = VerdictEvent::new(VerdictPayload {
            schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "verdict".to_owned(),
            sequence: 1,
            event_id: "evt-0000000000000001".to_owned(),
            occurred_at: fixture_timestamp().as_str().to_owned(),
            previous_event_hash: Some(genesis.hash().to_owned()),
            store_id: store.policy.policy.store_id.clone(),
            transaction_id: fixture_hash_id("txn-", 'a'),
            proposal_id: rejected.meta.proposal_id,
            proposal_meta_hash: rejected.meta_hash,
            candidate_hash: rejected.candidate_hash,
            source_id: rejected.meta.source_id,
            verb: VerdictVerb::Reject,
            reason: reason.as_str().to_owned(),
            verdict_id: reject_verdict_id,
            actor: store.policy.policy.human_actor.clone(),
            policy_hash: store.policy.hash.clone(),
            profile_hash: profile.hash.clone(),
            confidence: rejected.meta.confidence,
            evaluator_meta_hash: None,
            base_kind: rejected.meta.base.kind,
            base_generation: rejected.meta.base.generation_id,
            base_manifest_hash: rejected.meta.base.manifest_hash,
            base_bundle_hash: rejected.meta.base.bundle_hash,
            base_target_hash: rejected.meta.base.target_hash,
            rewrite_hash: None,
            rewrite_bytes: None,
            canonical_result: CanonicalResult::none(),
            resulting_canonical_head: "seed".to_owned(),
            intake_sequence: rejected.intake_sequence,
            intake_head_hash: Some(rejected.intake_head_hash),
        })
        .expect("create fixture reject");
        vec![
            LedgerEvent::Genesis(Box::new(genesis)),
            LedgerEvent::Verdict(Box::new(reject)),
        ]
    }

    pub fn fixture_tp(&self) -> Vec<LedgerEvent> {
        let profile = load_p3_profile(&self.profile_path).expect("load P3 profile");
        let policy = load_policy(&self.policy_template).expect("load P3 policy");
        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let target_id = fixture_id(target_path);
        let mut candidate_bytes = String::from_utf8(
            fs::read(self.store_root.join("seed/bundle").join(target_path))
                .expect("read fixture TP candidate"),
        )
        .expect("fixture TP candidate is UTF-8")
        .replace(target_id.trim_end_matches(".md"), &target_id)
        .into_bytes();
        candidate_bytes.extend_from_slice(b"\nFixture TP candidate revision.\n");
        let proposal = self.submit_fixture_proposal_from(
            "ledger-tp",
            "src:agent:fixture-ledger",
            ProposalOperation::Replace,
            ProposalIntent::Accept,
            target_path,
            &target_id,
            &candidate_bytes,
        );
        let ledger = sovmem::profile::read_ledger(&self.store_root.join("ledger/verdicts.jsonl"))
            .expect("read fixture ledger");
        let prior_head = ledger.final_hash.expect("fixture final hash");
        let reason = Reason::new(
            b"fixture ledger true positive",
            policy.policy.max_reason_bytes,
        )
        .expect("build fixture TP reason");
        let fixture_verdict_id = verdict_id(&VerdictIdentityInput {
            proposal_id: &proposal.meta.proposal_id,
            verb: VerdictVerb::Tp,
            reason: &reason,
            rewrite: None,
            actor: &policy.policy.human_actor,
            ruled_at: &fixture_timestamp(),
            prior_ledger_head: Some(&prior_head),
            intake_sequence: proposal.intake_sequence,
            intake_head_hash: Some(&proposal.intake_head_hash),
            policy_hash: &policy.hash,
            profile_hash: &profile.hash,
        });
        let event = VerdictEvent::new(VerdictPayload {
            schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "verdict".to_owned(),
            sequence: 2,
            event_id: "evt-0000000000000002".to_owned(),
            occurred_at: fixture_timestamp().as_str().to_owned(),
            previous_event_hash: Some(prior_head),
            store_id: policy.policy.store_id.clone(),
            transaction_id: fixture_hash_id("txn-", 'b'),
            proposal_id: proposal.meta.proposal_id,
            proposal_meta_hash: proposal.meta_hash,
            candidate_hash: proposal.candidate_hash,
            source_id: proposal.meta.source_id,
            verb: VerdictVerb::Tp,
            reason: reason.as_str().to_owned(),
            verdict_id: fixture_verdict_id,
            actor: policy.policy.human_actor.clone(),
            policy_hash: policy.hash,
            profile_hash: profile.hash,
            confidence: proposal.meta.confidence,
            evaluator_meta_hash: None,
            base_kind: proposal.meta.base.kind,
            base_generation: proposal.meta.base.generation_id,
            base_manifest_hash: proposal.meta.base.manifest_hash,
            base_bundle_hash: proposal.meta.base.bundle_hash,
            base_target_hash: proposal.meta.base.target_hash,
            rewrite_hash: None,
            rewrite_bytes: None,
            canonical_result: CanonicalResult::none(),
            resulting_canonical_head: "seed".to_owned(),
            intake_sequence: proposal.intake_sequence,
            intake_head_hash: Some(proposal.intake_head_hash),
        })
        .expect("create fixture TP");
        vec![LedgerEvent::Verdict(Box::new(event))]
    }

    pub fn append_fixture_events(&self, events: &[LedgerEvent]) -> Vec<u8> {
        let ledger = self.ledger_path();
        let mut bytes = fs::read(&ledger).expect("read fixture ledger");
        for event in events {
            bytes.extend_from_slice(
                &event
                    .canonical_line_bytes()
                    .expect("render fixture ledger event"),
            );
        }
        fs::write(&ledger, &bytes).expect("write sealed fixture ledger");
        bytes
    }

    pub fn ledger_path(&self) -> PathBuf {
        self.store_root.join("ledger/verdicts.jsonl")
    }

    pub fn canonical_ledger_bytes(&self, events: &[LedgerEvent]) -> Vec<u8> {
        events
            .iter()
            .flat_map(|event| {
                event
                    .canonical_line_bytes()
                    .expect("render canonical fixture ledger event")
            })
            .collect()
    }

    pub fn install_fixture_chain_with_one_applied_generation(&self) {
        self.install_fixture_chain(false);
    }

    pub fn install_fixture_chain_with_warning_evidence(&self) {
        self.install_fixture_chain(true);
    }

    fn install_fixture_chain(&self, warning_evidence: bool) {
        let profile = load_p3_profile(&self.profile_path).expect("load P3 profile");
        let policy = load_policy(&self.policy_template).expect("load P3 policy");
        let replacement_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let replacement_id = fixture_id(replacement_path);
        let frozen_replacement_id = replacement_id.trim_end_matches(".md");
        let replacement_bytes = String::from_utf8(
            fs::read(self.store_root.join("seed/bundle").join(replacement_path))
                .expect("read fixture replacement candidate"),
        )
        .expect("fixture replacement candidate is UTF-8")
        .replace(frozen_replacement_id, &replacement_id)
        .into_bytes();
        let rejected = self.submit_fixture_proposal(
            "level-four-reject",
            ProposalOperation::Replace,
            ProposalIntent::Accept,
            replacement_path,
            &replacement_id,
            &replacement_bytes,
        );

        let create_path = "04-Research/Agent-Loop-Engineering/P3-Concept-Map.md";
        let create_id = fixture_id(create_path);
        let mut candidate_bytes = created_target_bytes(self, create_path, &create_id);
        if warning_evidence {
            let parsed = parse_source_document(SourceDocument {
                source_path: create_path.to_owned(),
                source_hash: blake3::hash(&candidate_bytes).to_hex().to_string(),
                bytes: candidate_bytes,
                created: String::new(),
                updated: String::new(),
            })
            .expect("parse warning fixture candidate");
            let mut metadata: GeneratedMetadata = serde_yaml::from_value(
                serde_yaml::to_value(parsed.metadata).expect("encode warning fixture metadata"),
            )
            .expect("decode warning fixture metadata");
            metadata.inferred_fields.push("description".to_owned());
            candidate_bytes = render_document(&GeneratedDocument {
                metadata,
                body: parsed.body,
                source_hash: String::new(),
            })
            .expect("render warning fixture candidate");
        }
        let applied = self.submit_fixture_proposal(
            "level-four-accept",
            ProposalOperation::Create,
            ProposalIntent::Accept,
            create_path,
            &create_id,
            &candidate_bytes,
        );

        let store = inspect_store(&profile, &self.store_root).expect("inspect unsealed store");
        let genesis = GenesisEvent::new(GenesisPayload {
            schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "genesis".to_owned(),
            sequence: 0,
            event_id: "evt-0000000000000000".to_owned(),
            created_at: fixture_timestamp().as_str().to_owned(),
            previous_event_hash: None,
            store_id: store.policy.policy.store_id.clone(),
            profile_version: profile.version.clone(),
            profile_hash: profile.hash.clone(),
            policy_hash: policy.hash.clone(),
            seed_manifest_hash: store.seed.manifest_hash.clone(),
            seed_corpus_report_hash: store.seed.corpus_report_hash.clone(),
            seed_bundle_hash: store.seed.bundle_hash.clone(),
            source_commit: store.seed.source_commit.clone(),
            initial_canonical_ref: "seed".to_owned(),
        })
        .expect("build Level 4 genesis");
        let reject_reason = Reason::new(b"fixture retained reject", policy.policy.max_reason_bytes)
            .expect("build fixture reject reason");
        let reject_verdict_id = verdict_id(&VerdictIdentityInput {
            proposal_id: &rejected.meta.proposal_id,
            verb: VerdictVerb::Reject,
            reason: &reject_reason,
            rewrite: None,
            actor: &policy.policy.human_actor,
            ruled_at: &fixture_timestamp(),
            prior_ledger_head: Some(genesis.hash()),
            intake_sequence: rejected.intake_sequence,
            intake_head_hash: Some(&rejected.intake_head_hash),
            policy_hash: &policy.hash,
            profile_hash: &profile.hash,
        });
        let reject = VerdictEvent::new(VerdictPayload {
            schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "verdict".to_owned(),
            sequence: 1,
            event_id: "evt-0000000000000001".to_owned(),
            occurred_at: fixture_timestamp().as_str().to_owned(),
            previous_event_hash: Some(genesis.hash().to_owned()),
            store_id: store.policy.policy.store_id.clone(),
            transaction_id: fixture_hash_id("txn-", '1'),
            proposal_id: rejected.meta.proposal_id.clone(),
            proposal_meta_hash: rejected.meta_hash,
            candidate_hash: rejected.candidate_hash,
            source_id: rejected.meta.source_id.clone(),
            verb: VerdictVerb::Reject,
            reason: reject_reason.as_str().to_owned(),
            verdict_id: reject_verdict_id,
            actor: policy.policy.human_actor.clone(),
            policy_hash: policy.hash.clone(),
            profile_hash: profile.hash.clone(),
            confidence: rejected.meta.confidence,
            evaluator_meta_hash: None,
            base_kind: rejected.meta.base.kind.clone(),
            base_generation: rejected.meta.base.generation_id.clone(),
            base_manifest_hash: rejected.meta.base.manifest_hash.clone(),
            base_bundle_hash: rejected.meta.base.bundle_hash.clone(),
            base_target_hash: rejected.meta.base.target_hash.clone(),
            rewrite_hash: None,
            rewrite_bytes: None,
            canonical_result: CanonicalResult::none(),
            resulting_canonical_head: "seed".to_owned(),
            intake_sequence: rejected.intake_sequence,
            intake_head_hash: Some(rejected.intake_head_hash),
        })
        .expect("build fixture reject event");

        let accept_reason = Reason::new(b"fixture applied accept", policy.policy.max_reason_bytes)
            .expect("build fixture accept reason");
        let accept_verdict_id = verdict_id(&VerdictIdentityInput {
            proposal_id: &applied.meta.proposal_id,
            verb: VerdictVerb::Accept,
            reason: &accept_reason,
            rewrite: None,
            actor: &policy.policy.human_actor,
            ruled_at: &fixture_timestamp(),
            prior_ledger_head: Some(reject.hash()),
            intake_sequence: applied.intake_sequence,
            intake_head_hash: Some(&applied.intake_head_hash),
            policy_hash: &policy.hash,
            profile_hash: &profile.hash,
        });
        let judgment = build_judgment(
            &profile,
            &accept_verdict_id,
            VerdictVerb::Accept,
            &create_id,
            &accept_reason,
            &fixture_timestamp(),
        )
        .expect("build fixture accept judgment")
        .expect("ACCEPT makes a fixture judgment");
        let plan = GenerationPlan::from_seed(
            &profile,
            &self.store_root.join("seed"),
            AppliedGenerationInput {
                generation_id: "gen-0000000000000001".to_owned(),
                proposal_id: applied.meta.proposal_id.clone(),
                verdict_id: accept_verdict_id.clone(),
                policy_hash: policy.hash.clone(),
                operation: applied.meta.operation,
                target_path: applied.meta.target_path.clone(),
                target_id: applied.meta.target_id.clone(),
                target_bytes: applied.candidate_bytes.clone(),
                judgment,
            },
        )
        .expect("plan fixture generation");
        let generation = stage_generation(
            &profile,
            &plan,
            &self.p2.index_root(".p3-level-four-generation"),
        )
        .expect("stage fixture generation");
        let evidence = self.build_p3_index(&generation);
        copy_tree(
            &generation.root,
            &self
                .store_root
                .join("canon/generations")
                .join(&generation.manifest.generation_id),
        );
        fs::write(
            self.store_root
                .join("evidence/reindex")
                .join(format!("{accept_verdict_id}.json")),
            &evidence.receipt_bytes,
        )
        .expect("publish fixture receipt");
        fs::write(
            self.store_root
                .join("policies")
                .join(format!("{}.toml", policy.hash)),
            &policy.bytes,
        )
        .expect("archive fixture policy");
        fs::write(
            self.store_root.join("canon/CURRENT"),
            b"gen-0000000000000001\n",
        )
        .expect("select fixture generation");
        let accept = VerdictEvent::new(VerdictPayload {
            schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "verdict".to_owned(),
            sequence: 2,
            event_id: "evt-0000000000000002".to_owned(),
            occurred_at: fixture_timestamp().as_str().to_owned(),
            previous_event_hash: Some(reject.hash().to_owned()),
            store_id: store.policy.policy.store_id,
            transaction_id: fixture_hash_id("txn-", '2'),
            proposal_id: applied.meta.proposal_id,
            proposal_meta_hash: applied.meta_hash,
            candidate_hash: applied.candidate_hash,
            source_id: applied.meta.source_id,
            verb: VerdictVerb::Accept,
            reason: accept_reason.as_str().to_owned(),
            verdict_id: accept_verdict_id.clone(),
            actor: policy.policy.human_actor,
            policy_hash: policy.hash,
            profile_hash: profile.hash,
            confidence: applied.meta.confidence,
            evaluator_meta_hash: None,
            base_kind: applied.meta.base.kind,
            base_generation: applied.meta.base.generation_id,
            base_manifest_hash: applied.meta.base.manifest_hash,
            base_bundle_hash: applied.meta.base.bundle_hash,
            base_target_hash: applied.meta.base.target_hash,
            rewrite_hash: None,
            rewrite_bytes: None,
            canonical_result: CanonicalResult {
                generation_id: Some(generation.manifest.generation_id.clone()),
                manifest_hash: Some(generation.manifest_hash.clone()),
                bundle_hash: Some(generation.bundle_hash.clone()),
                target_prior_hash: None,
                target_new_hash: Some(blake3::hash(&candidate_bytes).to_hex().to_string()),
                judgment_hash: Some(
                    generation
                        .manifest
                        .documents
                        .iter()
                        .find(|document| {
                            document.canonical_path == format!("judgments/{accept_verdict_id}.md")
                        })
                        .expect("fixture judgment manifest entry")
                        .hash
                        .clone(),
                ),
                reindex_receipt_hash: Some(evidence.receipt_hash),
            },
            resulting_canonical_head: generation.manifest.generation_id,
            intake_sequence: applied.intake_sequence,
            intake_head_hash: Some(applied.intake_head_hash),
        })
        .expect("build fixture accept event");
        self.append_fixture_events(&[
            LedgerEvent::Genesis(Box::new(genesis)),
            LedgerEvent::Verdict(Box::new(reject)),
            LedgerEvent::Verdict(Box::new(accept)),
        ]);
    }

    pub fn extend_fixture_chain_with_superseding_generation(&self) {
        let profile = load_p3_profile(&self.profile_path).expect("load P3 profile");
        let policy = load_policy(&self.policy_template).expect("load P3 policy");
        let generation_root = self
            .store_root
            .join("canon/generations/gen-0000000000000001");
        let parent = sovmem::profile::verify_generation(
            &profile,
            &generation_root,
            VerificationLevel::Three,
        )
        .expect("verify parent generation");
        let target_path = "04-Research/Agent-Loop-Engineering/P3-Concept-Map.md";
        let target_id = fixture_id(target_path);
        let mut candidate_bytes =
            fs::read(generation_root.join("bundle").join(target_path)).expect("read parent target");
        candidate_bytes.extend_from_slice(b"\nSuperseding fixture revision.\n");
        let proposal = self.submit_fixture_proposal(
            "level-four-supersede",
            ProposalOperation::Replace,
            ProposalIntent::Accept,
            target_path,
            &target_id,
            &candidate_bytes,
        );
        let ledger = sovmem::profile::read_ledger(&self.ledger_path()).expect("read ledger");
        let prior_head = ledger.final_hash.expect("sealed ledger head");
        let reason = Reason::new(
            b"fixture superseding accept",
            policy.policy.max_reason_bytes,
        )
        .expect("build superseding reason");
        let ruled_at = fixture_timestamp();
        let verdict_id = verdict_id(&VerdictIdentityInput {
            proposal_id: &proposal.meta.proposal_id,
            verb: VerdictVerb::Accept,
            reason: &reason,
            rewrite: None,
            actor: &policy.policy.human_actor,
            ruled_at: &ruled_at,
            prior_ledger_head: Some(&prior_head),
            intake_sequence: proposal.intake_sequence,
            intake_head_hash: Some(&proposal.intake_head_hash),
            policy_hash: &policy.hash,
            profile_hash: &profile.hash,
        });
        let judgment = build_judgment(
            &profile,
            &verdict_id,
            VerdictVerb::Accept,
            &target_id,
            &reason,
            &ruled_at,
        )
        .expect("build superseding judgment")
        .expect("accept has judgment");
        let plan = GenerationPlan::from_generation(
            &profile,
            &parent,
            AppliedGenerationInput {
                generation_id: "gen-0000000000000002".to_owned(),
                proposal_id: proposal.meta.proposal_id.clone(),
                verdict_id: verdict_id.clone(),
                policy_hash: policy.hash.clone(),
                operation: ProposalOperation::Replace,
                target_path: target_path.to_owned(),
                target_id,
                target_bytes: candidate_bytes.clone(),
                judgment,
            },
        )
        .expect("plan superseding generation");
        let generation = stage_generation(
            &profile,
            &plan,
            &self.p2.index_root(".p3-superseding-generation"),
        )
        .expect("stage superseding generation");
        let evidence = self.build_p3_index(&generation);
        copy_tree(
            &generation.root,
            &self
                .store_root
                .join("canon/generations/gen-0000000000000002"),
        );
        fs::write(
            self.store_root
                .join("evidence/reindex")
                .join(format!("{verdict_id}.json")),
            &evidence.receipt_bytes,
        )
        .expect("publish superseding receipt");
        fs::write(
            self.store_root.join("canon/CURRENT"),
            b"gen-0000000000000002\n",
        )
        .expect("select superseding generation");
        let event = VerdictEvent::new(VerdictPayload {
            schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
            event_type: "verdict".to_owned(),
            sequence: 3,
            event_id: "evt-0000000000000003".to_owned(),
            occurred_at: ruled_at.as_str().to_owned(),
            previous_event_hash: Some(prior_head),
            store_id: policy.policy.store_id,
            transaction_id: fixture_hash_id("txn-", '3'),
            proposal_id: proposal.meta.proposal_id,
            proposal_meta_hash: proposal.meta_hash,
            candidate_hash: proposal.candidate_hash,
            source_id: proposal.meta.source_id,
            verb: VerdictVerb::Accept,
            reason: reason.as_str().to_owned(),
            verdict_id: verdict_id.clone(),
            actor: policy.policy.human_actor,
            policy_hash: policy.hash,
            profile_hash: profile.hash.clone(),
            confidence: proposal.meta.confidence,
            evaluator_meta_hash: None,
            base_kind: proposal.meta.base.kind,
            base_generation: proposal.meta.base.generation_id,
            base_manifest_hash: proposal.meta.base.manifest_hash,
            base_bundle_hash: proposal.meta.base.bundle_hash,
            base_target_hash: proposal.meta.base.target_hash.clone(),
            rewrite_hash: None,
            rewrite_bytes: None,
            canonical_result: CanonicalResult {
                generation_id: Some(generation.manifest.generation_id.clone()),
                manifest_hash: Some(generation.manifest_hash.clone()),
                bundle_hash: Some(generation.bundle_hash.clone()),
                target_prior_hash: proposal.meta.base.target_hash,
                target_new_hash: Some(blake3::hash(&candidate_bytes).to_hex().to_string()),
                judgment_hash: generation
                    .manifest
                    .documents
                    .iter()
                    .find(|document| {
                        document.canonical_path == format!("judgments/{verdict_id}.md")
                    })
                    .map(|document| document.hash.clone()),
                reindex_receipt_hash: Some(evidence.receipt_hash),
            },
            resulting_canonical_head: generation.manifest.generation_id,
            intake_sequence: proposal.intake_sequence,
            intake_head_hash: Some(proposal.intake_head_hash),
        })
        .expect("build superseding event");
        self.append_fixture_events(&[LedgerEvent::Verdict(Box::new(event))]);
        verify_store_level4(&profile, &self.store_root).expect("verify superseding chain");
    }

    pub fn extend_fixture_chain_to_twenty_verdicts(&self) {
        self.install_fixture_chain_with_one_applied_generation();
        let profile = load_p3_profile(&self.profile_path).expect("load P3 profile");
        let policy = load_policy(&self.policy_template).expect("load P3 policy");
        let target_path = "04-Research/Agent-Loop-Engineering/P3-Concept-Map.md";
        let target_id = fixture_id(target_path);
        let verbs = [
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
        let mut generation_number = 1_u64;
        for (offset, verb) in verbs.into_iter().enumerate() {
            let sequence = offset as u64 + 3;
            let generation_root = self
                .store_root
                .join(format!("canon/generations/gen-{generation_number:016}"));
            let parent = sovmem::profile::verify_generation(
                &profile,
                &generation_root,
                VerificationLevel::Three,
            )
            .expect("verify 20-chain parent");
            let mut candidate_bytes = fs::read(parent.root.join("bundle").join(target_path))
                .expect("read 20-chain target");
            candidate_bytes.extend_from_slice(
                format!("\nCandidate revision for verdict {sequence}.\n").as_bytes(),
            );
            let proposal = self.submit_fixture_proposal_from(
                &format!("twenty-chain-{sequence}"),
                &format!("src:agent:twenty-{sequence}"),
                ProposalOperation::Replace,
                if verb == VerdictVerb::Rewrite {
                    ProposalIntent::Rewrite
                } else {
                    ProposalIntent::Accept
                },
                target_path,
                &target_id,
                &candidate_bytes,
            );
            let ledger = sovmem::profile::read_ledger(&self.ledger_path()).expect("read ledger");
            let prior_head = ledger.final_hash.expect("20-chain prior head");
            let reason = Reason::new(
                format!("fixture {} reason {sequence}", verb.as_str()).as_bytes(),
                policy.policy.max_reason_bytes,
            )
            .expect("build 20-chain reason");
            let mut rewrite_bytes = None;
            if verb == VerdictVerb::Rewrite {
                let mut bytes = fs::read(parent.root.join("bundle").join(target_path))
                    .expect("read rewrite parent target");
                bytes.extend_from_slice(
                    format!("\nRewritten revision for verdict {sequence}.\n").as_bytes(),
                );
                rewrite_bytes = Some(
                    RewriteBytes::new(bytes, policy.policy.max_rewrite_bytes)
                        .expect("valid 20-chain rewrite"),
                );
            }
            let ruled_at = fixture_timestamp();
            let verdict_id = verdict_id(&VerdictIdentityInput {
                proposal_id: &proposal.meta.proposal_id,
                verb,
                reason: &reason,
                rewrite: rewrite_bytes.as_ref(),
                actor: &policy.policy.human_actor,
                ruled_at: &ruled_at,
                prior_ledger_head: Some(&prior_head),
                intake_sequence: proposal.intake_sequence,
                intake_head_hash: Some(&proposal.intake_head_hash),
                policy_hash: &policy.hash,
                profile_hash: &profile.hash,
            });
            let mut canonical_result = CanonicalResult::none();
            let resulting_head;
            if verb.applies() {
                generation_number += 1;
                let target_bytes = rewrite_bytes
                    .as_ref()
                    .map(|rewrite| rewrite.bytes().to_vec())
                    .unwrap_or_else(|| proposal.candidate_bytes.clone());
                let judgment =
                    build_judgment(&profile, &verdict_id, verb, &target_id, &reason, &ruled_at)
                        .expect("build 20-chain judgment")
                        .expect("applied verdict has judgment");
                let generation_id = format!("gen-{generation_number:016}");
                let plan = GenerationPlan::from_generation(
                    &profile,
                    &parent,
                    AppliedGenerationInput {
                        generation_id: generation_id.clone(),
                        proposal_id: proposal.meta.proposal_id.clone(),
                        verdict_id: verdict_id.clone(),
                        policy_hash: policy.hash.clone(),
                        operation: proposal.meta.operation,
                        target_path: target_path.to_owned(),
                        target_id: target_id.clone(),
                        target_bytes: target_bytes.clone(),
                        judgment,
                    },
                )
                .expect("plan 20-chain generation");
                let generation = stage_generation(
                    &profile,
                    &plan,
                    &self
                        .p2
                        .index_root(&format!(".p3-twenty-generation-{sequence}")),
                )
                .expect("stage 20-chain generation");
                let evidence = self.build_p3_index(&generation);
                copy_tree(
                    &generation.root,
                    &self
                        .store_root
                        .join("canon/generations")
                        .join(&generation_id),
                );
                fs::write(
                    self.store_root
                        .join("evidence/reindex")
                        .join(format!("{verdict_id}.json")),
                    &evidence.receipt_bytes,
                )
                .expect("publish 20-chain receipt");
                fs::write(
                    self.store_root.join("canon/CURRENT"),
                    format!("{generation_id}\n"),
                )
                .expect("select 20-chain generation");
                canonical_result = CanonicalResult {
                    generation_id: Some(generation_id.clone()),
                    manifest_hash: Some(generation.manifest_hash.clone()),
                    bundle_hash: Some(generation.bundle_hash.clone()),
                    target_prior_hash: proposal.meta.base.target_hash.clone(),
                    target_new_hash: Some(blake3::hash(&target_bytes).to_hex().to_string()),
                    judgment_hash: generation
                        .manifest
                        .documents
                        .iter()
                        .find(|document| {
                            document.canonical_path == format!("judgments/{verdict_id}.md")
                        })
                        .map(|document| document.hash.clone()),
                    reindex_receipt_hash: Some(evidence.receipt_hash),
                };
                resulting_head = generation_id;
            } else {
                resulting_head = format!("gen-{generation_number:016}");
            }
            let event = VerdictEvent::new(VerdictPayload {
                schema_version: LEDGER_EVENT_SCHEMA.to_owned(),
                event_type: "verdict".to_owned(),
                sequence,
                event_id: format!("evt-{sequence:016}"),
                occurred_at: ruled_at.as_str().to_owned(),
                previous_event_hash: Some(prior_head),
                store_id: policy.policy.store_id.clone(),
                transaction_id: format!(
                    "txn-{}",
                    blake3::hash(format!("twenty-chain-{sequence}").as_bytes()).to_hex()
                ),
                proposal_id: proposal.meta.proposal_id,
                proposal_meta_hash: proposal.meta_hash,
                candidate_hash: proposal.candidate_hash,
                source_id: proposal.meta.source_id,
                verb,
                reason: reason.as_str().to_owned(),
                verdict_id,
                actor: policy.policy.human_actor.clone(),
                policy_hash: policy.hash.clone(),
                profile_hash: profile.hash.clone(),
                confidence: proposal.meta.confidence,
                evaluator_meta_hash: None,
                base_kind: proposal.meta.base.kind,
                base_generation: proposal.meta.base.generation_id,
                base_manifest_hash: proposal.meta.base.manifest_hash,
                base_bundle_hash: proposal.meta.base.bundle_hash,
                base_target_hash: proposal.meta.base.target_hash,
                rewrite_hash: rewrite_bytes
                    .as_ref()
                    .map(|rewrite| rewrite.hash().to_owned()),
                rewrite_bytes: rewrite_bytes
                    .as_ref()
                    .map(|rewrite| rewrite.bytes().len() as u64),
                canonical_result,
                resulting_canonical_head: resulting_head,
                intake_sequence: proposal.intake_sequence,
                intake_head_hash: Some(proposal.intake_head_hash),
            })
            .expect("build 20-chain event");
            self.append_fixture_events(&[LedgerEvent::Verdict(Box::new(event))]);
        }
        let summary = verify_store_level4(&profile, &self.store_root)
            .expect("verify actual genesis plus 20 verdict fixture");
        assert_eq!(summary.verdicts, 20);
        assert_eq!(summary.applied_generations, 8);
        assert_eq!(summary.current_generation, "gen-0000000000000008");
    }

    pub fn verify_level4(
        &self,
    ) -> Result<sovmem::profile::Level4Summary, sovmem::profile::Level4Failure> {
        let profile = load_p3_profile(&self.profile_path).expect("load P3 profile");
        verify_store_level4(&profile, &self.store_root)
    }

    pub fn verdict_counts(&self) -> [usize; 5] {
        let ledger = sovmem::profile::read_ledger(&self.ledger_path())
            .expect("read fixture ledger for verdict counts");
        [
            ledger.accept,
            ledger.rewrite,
            ledger.reject,
            ledger.tp,
            ledger.fp,
        ]
    }

    fn submit_fixture_proposal(
        &self,
        name: &str,
        operation: ProposalOperation,
        intent: ProposalIntent,
        target_path: &str,
        target_id: &str,
        candidate_bytes: &[u8],
    ) -> FixtureProposal {
        self.submit_fixture_proposal_from(
            name,
            "src:agent:fixture",
            operation,
            intent,
            target_path,
            target_id,
            candidate_bytes,
        )
    }

    #[allow(clippy::too_many_arguments)] // Fixture inputs model the independently bound proposal fields.
    fn submit_fixture_proposal_from(
        &self,
        name: &str,
        source_id: &str,
        operation: ProposalOperation,
        intent: ProposalIntent,
        target_path: &str,
        target_id: &str,
        candidate_bytes: &[u8],
    ) -> FixtureProposal {
        let candidate = self.input_file(name, candidate_bytes);
        let submitted = super::super::submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: self.profile_path.clone(),
                store_root: self.store_root.clone(),
                source_id: source_id.to_owned(),
                operation,
                intent,
                target_id: target_id.to_owned(),
                target_path: target_path.to_owned(),
                candidate_path: candidate,
                confidence: None,
                evaluator_path: None,
            },
            &super::super::p3_test_support::FixedClock(fixture_timestamp()),
        )
        .unwrap_or_else(|error| panic!("stage fixture proposal {name}: {error:?}"));
        assert!(
            submitted.proposal_id.is_empty(),
            "an untrusted inbox submission cannot return a protected proposal id"
        );
        let mut human = super::super::p3_test_support::ScriptedHuman::new(Some(format!(
            "SEAL INTAKE {}\n",
            submitted.submission_id
        )));
        let sealed = seal_intake_submission_with_human(
            &self.profile_path,
            &self.store_root,
            &submitted.submission_id,
            &mut human,
            &super::super::p3_test_support::FixedClock(fixture_timestamp()),
        )
        .unwrap_or_else(|error| panic!("seal fixture proposal {name}: {error:?}"));
        let proposal_id = sealed
            .proposal_id
            .expect("a policy-passing seal publishes a protected proposal id");
        let profile = load_p3_profile(&self.profile_path).expect("load sealed fixture profile");
        let store =
            inspect_store(&profile, &self.store_root).expect("inspect sealed fixture store");
        let pair = super::super::read_sealed_pair_structural(&store.paths, &proposal_id)
            .expect("read sealed fixture proposal pair");
        FixtureProposal {
            meta: pair.meta,
            meta_hash: pair.meta_hash,
            candidate_hash: pair.candidate_hash,
            candidate_bytes: pair.candidate_bytes,
            intake_sequence: sealed.intake_sequence,
            intake_head_hash: sealed.intake_head_hash,
        }
    }
}

fn created_target_bytes(fixture: &P3Fixture, target_path: &str, target_id: &str) -> Vec<u8> {
    let seed_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
    let bytes = std::fs::read(fixture.store_root.join("seed/bundle").join(seed_path))
        .expect("read seeded template");
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let parsed = parse_source_document(SourceDocument {
        source_path: seed_path.to_owned(),
        bytes,
        source_hash: hash,
        created: String::new(),
        updated: String::new(),
    })
    .expect("parse seeded template");
    let mut metadata: GeneratedMetadata = serde_yaml::from_value(
        serde_yaml::to_value(parsed.metadata).expect("encode seeded template metadata"),
    )
    .expect("decode seeded template metadata");
    metadata.id = target_id.to_owned();
    metadata.source_path = target_path.to_owned();
    metadata.source = format!("vault://{target_id}");
    metadata.resource = format!("vault://{target_id}");
    metadata.title = "P3 generated canonical fixture".to_owned();
    metadata.description = "Deterministic P3 create fixture.".to_owned();
    if let Some(mapping) = load_p3_profile(&fixture.profile_path)
        .expect("load P3 profile for fixture candidate")
        .config
        .mapping_for(target_path)
    {
        metadata.core_type = mapping.core_type.clone();
        metadata.context_kind = mapping.context_kind.clone();
        metadata.layer = mapping.layer.clone();
        metadata.authority = mapping.authority.clone();
    }
    let document = GeneratedDocument {
        metadata,
        body: format!("{}\nP3 fixture create exact bytes.\n", parsed.body),
        source_hash: String::new(),
    };
    render_document(&document).expect("render create candidate")
}

fn fixture_id(path: &str) -> String {
    format!("ssa-0/{path}")
}

fn fixture_hash_id(prefix: &str, character: char) -> String {
    format!("{prefix}{}", character.to_string().repeat(64))
}

fn fixture_timestamp() -> Timestamp {
    Timestamp::parse("2026-07-14T04:30:00.000000000Z").expect("fixture timestamp")
}

fn provision_test_store(
    p2: &P2Fixture,
    profile_path: &Path,
    policy_template: &Path,
    store_root: &Path,
) {
    let frozen_profile = load_profile(&p2.profile_path).expect("load frozen P2 profile");
    let p2_bundle = fs::canonicalize(&p2.bundle_root).expect("canonicalize frozen P2 bundle");
    let frozen = verify_bundle(
        &frozen_profile,
        &p2_bundle,
        VerificationOptions::level(VerificationLevel::Three),
    );
    assert!(
        !frozen.has_errors(),
        "frozen fixture Level 3: {:#?}",
        frozen.diagnostics()
    );

    fs::create_dir(store_root).expect("create temporary P3 store");
    copy_tree(&p2_bundle, &store_root.join("seed"));
    for directory in [
        "policies",
        "staging/inbox",
        "intake/proposals",
        "intake/transactions",
        "ledger",
        "canon/generations",
        "evidence/reindex",
        "derived/indexes",
        "transactions",
        "locks",
    ] {
        fs::create_dir_all(store_root.join(directory)).expect("create P3 store directory");
    }
    fs::write(
        store_root.join("policy.toml"),
        fs::read(policy_template).expect("read P3 policy template"),
    )
    .expect("write P3 policy");
    fs::write(store_root.join("ledger/verdicts.jsonl"), b"").expect("write zero ledger");
    fs::write(store_root.join("intake/events.jsonl"), b"").expect("write zero intake");
    fs::write(store_root.join("canon/CURRENT"), b"seed\n").expect("write seed current");

    load_p3_profile(profile_path).expect("load P3 profile");
    let seed = fs::canonicalize(store_root.join("seed")).expect("canonicalize copied seed");
    let copied = verify_bundle(
        &frozen_profile,
        &seed,
        VerificationOptions::level(VerificationLevel::Three),
    );
    assert!(
        !copied.has_errors(),
        "copied fixture Level 3: {:#?}",
        copied.diagnostics()
    );
}

fn copy_tree(source: &Path, destination: &Path) {
    let metadata = fs::symlink_metadata(source).expect("read source metadata");
    assert!(
        !metadata.file_type().is_symlink(),
        "fixture source must not be a symlink"
    );
    if metadata.is_file() {
        fs::write(
            destination,
            fs::read(source).expect("read fixture source file"),
        )
        .expect("write fixture destination file");
        return;
    }
    assert!(
        metadata.is_dir(),
        "fixture source must be a file or directory"
    );
    fs::create_dir(destination).expect("create fixture destination directory");
    let mut entries = fs::read_dir(source)
        .expect("read fixture source directory")
        .collect::<Result<Vec<_>, _>>()
        .expect("read fixture source entry");
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        copy_tree(&entry.path(), &destination.join(entry.file_name()));
    }
}

fn repository_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative)
}

#[test]
fn p3_fixture_provisions_the_v2_store_layout() {
    let fixture = P3Fixture::new("p3-v2-layout");
    let profile = load_p3_profile(&fixture.profile_path).expect("load v2 fixture profile");

    inspect_store(&profile, &fixture.store_root).expect("inspect v2 fixture store");
    assert!(!fixture.store_root.join("staging/proposals").exists());
    assert!(!fixture
        .store_root
        .join("staging/intake-rejections")
        .exists());
    assert!(fixture.store_root.join("staging/inbox").is_dir());
    assert!(fixture.store_root.join("intake/proposals").is_dir());
    assert!(fixture.store_root.join("intake/transactions").is_dir());
    assert_eq!(
        fs::read(fixture.store_root.join("intake/events.jsonl"))
            .expect("read protected fixture intake"),
        b""
    );
}

#[test]
fn p3_fixture_public_submit_and_seal_bind_exact_protected_evidence() {
    let fixture = P3Fixture::new("p3-v2-protected-evidence");
    fixture.install_fixture_chain_with_one_applied_generation();
    let profile = load_p3_profile(&fixture.profile_path).expect("load v2 fixture profile");
    let store = inspect_store(&profile, &fixture.store_root).expect("inspect v2 fixture store");
    let intake = sovmem::profile::read_intake(&store.paths).expect("read protected intake");
    let ledger = sovmem::profile::read_ledger(&store.paths.ledger).expect("read fixture ledger");
    let verdicts = ledger
        .events
        .iter()
        .filter_map(|event| match event {
            LedgerEvent::Genesis(_) => None,
            LedgerEvent::Verdict(event) => Some(event.payload()),
        })
        .collect::<Vec<_>>();

    assert_eq!(intake.events().len(), 2);
    assert_eq!(verdicts.len(), 2);
    for verdict in verdicts {
        let pair = sovmem::profile::read_sealed_pair_structural(&store.paths, &verdict.proposal_id)
            .expect("read exact protected proposal pair");
        let event = &intake.events()[verdict.intake_sequence as usize - 1];
        assert_eq!(
            event.payload().proposal_id.as_deref(),
            Some(verdict.proposal_id.as_str())
        );
        assert_eq!(
            event.payload().proposal_meta_hash,
            Some(pair.meta_hash.clone())
        );
        assert_eq!(
            event.payload().candidate_hash,
            Some(pair.candidate_hash.clone())
        );
        assert_eq!(verdict.proposal_meta_hash, pair.meta_hash);
        assert_eq!(verdict.candidate_hash, pair.candidate_hash);
        assert_eq!(verdict.intake_head_hash.as_deref(), Some(event.hash()));
    }

    verify_store_level4(&profile, &fixture.store_root).expect("verify v2 fixture at Level 4");
}
