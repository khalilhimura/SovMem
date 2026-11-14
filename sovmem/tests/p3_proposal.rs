use sovmem::profile::p3_test_support::{FixedClock, ProvisionedP3Fixture, ScriptedHuman};
use sovmem::profile::proposal::{
    canonical_meta_bytes, parse_evaluator, prepare_proposal, proposal_list, proposal_record_order,
    proposal_status, publish_proposal_pair, publish_proposal_pair_with_failpoint,
    read_proposal_pair, release_intake_lease_with, submit_proposal_with_clock, verdict_list,
    Confidence, EvaluatorBlock, IntakeLease, IntakeLeaseReleaseOps, ProposalBase, ProposalDraft,
    ProposalError, ProposalFilter, ProposalIntent, ProposalOperation, ProposalPair,
    ProposalReadError, ProposalRecord, ProposalState, ProposalSubmission, PublicationFailpoint,
    SubmitProposalResult,
};
use sovmem::profile::transaction::{
    apply_human_verdict, preflight_human_verdict_with_human, read_preflight_state,
    seal_intake_submission_with_human, HumanVerdictRequest, IntakeSealOutcome, PreflightVerdict,
};
use sovmem::profile::{
    inspect_store, load_p3_profile, read_intake, store_stats, sync_directory, Reason, Timestamp,
    VerdictVerb,
};

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Task7ProjectionFixture {
    fixture: ProvisionedP3Fixture,
    submitted_id: String,
    staged_id: String,
    rejected_id: String,
    ruled_id: String,
}

fn replace_candidate(fixture: &ProvisionedP3Fixture, target_path: &str, name: &str) -> PathBuf {
    let identity = target_path.trim_end_matches(".md");
    let bytes = String::from_utf8(
        fs::read(fixture.store_root.join("seed/bundle").join(target_path))
            .expect("read Task 7 fixture candidate"),
    )
    .expect("Task 7 fixture candidate is UTF-8")
    .replace(
        &format!("ssa-0/{identity}"),
        &format!("ssa-0/{target_path}"),
    );
    let path = fixture
        .profile_path
        .parent()
        .expect("Task 7 fixture profile parent")
        .join(name);
    fs::write(&path, bytes).expect("write Task 7 fixture candidate");
    path
}

fn submit_at(
    fixture: &ProvisionedP3Fixture,
    source_id: &str,
    target_path: &str,
    candidate_path: PathBuf,
    evaluator_path: Option<PathBuf>,
    at: &str,
) -> SubmitProposalResult {
    submit_proposal_with_clock(
        ProposalSubmission {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
            source_id: source_id.to_owned(),
            operation: ProposalOperation::Replace,
            intent: ProposalIntent::Accept,
            target_id: format!("ssa-0/{target_path}"),
            target_path: target_path.to_owned(),
            candidate_path,
            confidence: None,
            evaluator_path,
        },
        &FixedClock(Timestamp::parse(at).unwrap()),
    )
    .expect("submit Task 7 fixture inbox item")
}

fn seal_at(fixture: &ProvisionedP3Fixture, submission_id: &str, at: &str) -> IntakeSealOutcome {
    let mut human = ScriptedHuman::new(Some(format!("SEAL INTAKE {submission_id}\n")));
    seal_intake_submission_with_human(
        &fixture.profile_path,
        &fixture.store_root,
        submission_id,
        &mut human,
        &FixedClock(Timestamp::parse(at).unwrap()),
    )
    .expect("seal Task 7 fixture inbox item")
}

fn apply_accept_at(fixture: &ProvisionedP3Fixture, proposal_id: &str, reason: &str, at: &str) {
    let request = HumanVerdictRequest {
        proposal_id: proposal_id.to_owned(),
        verb: VerdictVerb::Accept,
        reason: Reason::new(reason.as_bytes(), 4096).expect("construct Task 7 ACCEPT reason"),
        rewrite: None,
    };
    let initial = read_preflight_state(&fixture.profile_path, &fixture.store_root, &request, None)
        .expect("read Task 7 ACCEPT preflight");
    let runtime = fixture.runtime();
    let mut human = ScriptedHuman::new(Some(format!("APPLY ACCEPT {}\n", &proposal_id[..12])));
    let decision = match preflight_human_verdict_with_human(
        initial,
        request,
        &runtime,
        &mut human,
        &FixedClock(Timestamp::parse(at).unwrap()),
    )
    .expect("confirm Task 7 ACCEPT")
    {
        PreflightVerdict::Confirmed(decision) => decision,
        PreflightVerdict::Idempotent(_) => panic!("fresh Task 7 ACCEPT cannot be idempotent"),
    };
    let outcome = apply_human_verdict(decision, &runtime).expect("commit Task 7 ACCEPT");
    assert!(outcome.applied);
}

fn current_candidate(fixture: &ProvisionedP3Fixture, target_path: &str, name: &str) -> PathBuf {
    let generation = fs::read_to_string(fixture.store_root.join("canon/CURRENT"))
        .expect("read Task 7 current generation")
        .trim()
        .to_owned();
    let mut bytes = fs::read(
        fixture
            .store_root
            .join("canon/generations")
            .join(generation)
            .join("bundle")
            .join(target_path),
    )
    .expect("read Task 7 current candidate");
    bytes.push(b'\n');
    let path = fixture
        .profile_path
        .parent()
        .expect("Task 7 fixture profile parent")
        .join(name);
    fs::write(&path, bytes).expect("write Task 7 superseding candidate");
    path
}

fn add_task7_applied_generations(projection: &Task7ProjectionFixture) -> (String, String) {
    apply_accept_at(
        &projection.fixture,
        &projection.staged_id,
        "PRIVATE_TASK7_FIRST_ACCEPT_REASON_7411",
        "2026-07-22T01:05:00.000000000Z",
    );

    let target = "04-Research/Agent-Loop-Engineering/Daily-Court-Deep-Gap-Notes.md";
    let superseding = submit_at(
        &projection.fixture,
        "src:agent:task7-superseding",
        target,
        current_candidate(
            &projection.fixture,
            target,
            "PRIVATE_TASK7_SUPERSEDING_PATH_7412.md",
        ),
        None,
        "2026-07-22T01:06:00.000000000Z",
    );
    let applied_id = seal_at(
        &projection.fixture,
        &superseding.submission_id,
        "2026-07-22T01:06:01.000000000Z",
    )
    .proposal_id
    .expect("Task 7 superseding submission stages");
    apply_accept_at(
        &projection.fixture,
        &applied_id,
        "PRIVATE_TASK7_SECOND_ACCEPT_REASON_7413",
        "2026-07-22T01:06:02.000000000Z",
    );
    (projection.staged_id.clone(), applied_id)
}

fn task7_projection_fixture(name: &str) -> Task7ProjectionFixture {
    let mut fixture = ProvisionedP3Fixture::empty(name);
    let policy_path = fixture.store_root.join("policy.toml");
    let policy = fs::read_to_string(&policy_path).unwrap().replacen(
        "queue_alarm_at = 25",
        "queue_alarm_at = 1",
        1,
    );
    fs::write(&policy_path, policy).unwrap();

    let staged_target = "04-Research/Agent-Loop-Engineering/Daily-Court-Deep-Gap-Notes.md";
    let staged_candidate =
        replace_candidate(&fixture, staged_target, "PRIVATE_TASK7_STAGED_PATH_7402.md");
    let evaluator_path = fixture
        .profile_path
        .parent()
        .unwrap()
        .join("PRIVATE_TASK7_EVALUATOR_PATH_7403.json");
    fs::write(
        &evaluator_path,
        serde_json::to_vec(&EvaluatorBlock {
            protocol: "adversarial-bug-finding-v1".to_owned(),
            source_id: "src:agent:task7-evaluator".to_owned(),
            evaluated_at: "2026-07-22T01:00:00.000000000Z".to_owned(),
            candidate_hash: blake3::hash(&fs::read(&staged_candidate).unwrap())
                .to_hex()
                .to_string(),
            recommendation: "looks-sound".to_owned(),
            reasons: vec!["PRIVATE_TASK7_EVALUATOR_REASON_7404".to_owned()],
        })
        .unwrap(),
    )
    .unwrap();
    let staged = submit_at(
        &fixture,
        "src:agent:task7-staged",
        staged_target,
        staged_candidate,
        Some(evaluator_path),
        "2026-07-22T01:00:00.000000000Z",
    );
    let staged_id = seal_at(
        &fixture,
        &staged.submission_id,
        "2026-07-22T01:00:01.000000000Z",
    )
    .proposal_id
    .expect("Task 7 open fixture stages");

    let rejected_target = "04-Research/AVM/AVM-OKF-Substrate-Investigation.md";
    let rejected_candidate = fixture
        .profile_path
        .parent()
        .unwrap()
        .join("PRIVATE_TASK7_REJECTED_PATH_7405.md");
    fs::write(
        &rejected_candidate,
        b"PRIVATE_TASK7_REJECTED_CANDIDATE_7406\n",
    )
    .unwrap();
    let rejected = submit_at(
        &fixture,
        "src:agent:task7-rejected",
        rejected_target,
        rejected_candidate,
        None,
        "2026-07-22T01:01:00.000000000Z",
    );
    let rejected_outcome = seal_at(
        &fixture,
        &rejected.submission_id,
        "2026-07-22T01:01:01.000000000Z",
    );
    assert_eq!(rejected_outcome.state, "rejected-at-intake");
    let rejected_id = format!("int-{:016}", rejected_outcome.intake_sequence);

    let ruled_target = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
    let ruled = submit_at(
        &fixture,
        "src:agent:task7-ruled",
        ruled_target,
        replace_candidate(&fixture, ruled_target, "PRIVATE_TASK7_RULED_PATH_7401.md"),
        None,
        "2026-07-22T01:02:00.000000000Z",
    );
    let ruled_id = seal_at(
        &fixture,
        &ruled.submission_id,
        "2026-07-22T01:02:01.000000000Z",
    )
    .proposal_id
    .expect("Task 7 ruled fixture stages");

    fixture.proposal_id = ruled_id.clone();
    let request = fixture.request(b"PRIVATE_TASK7_VERDICT_REASON_7407");
    fixture.commit_exact_reject_at(
        &request,
        Timestamp::parse("2026-07-22T01:02:02.000000000Z").unwrap(),
    );

    let submitted_target = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
    let submitted_candidate = replace_candidate(
        &fixture,
        submitted_target,
        "PRIVATE_TASK7_SUBMITTED_PATH_7408.md",
    );
    let submitted = submit_at(
        &fixture,
        "src:agent:task7-submitted",
        submitted_target,
        submitted_candidate,
        None,
        "2026-07-22T01:03:00.000000000Z",
    );
    load_p3_profile(&fixture.profile_path).expect("Task 7 fixture profile remains valid");

    Task7ProjectionFixture {
        fixture,
        submitted_id: submitted.submission_id,
        staged_id,
        rejected_id,
        ruled_id,
    }
}

#[test]
fn task7_projection_all_states_use_complete_inbox_and_verified_protected_evidence() {
    let projection = task7_projection_fixture("task7-projection-all-states");
    let profile = load_p3_profile(&projection.fixture.profile_path).unwrap();
    let list = proposal_list(
        &profile,
        &projection.fixture.store_root,
        ProposalFilter { state: None },
    )
    .expect("Task 7 list projects verified evidence");
    assert_eq!(list.len(), 4);
    assert_eq!(list[0].state.as_str(), "submitted");
    assert_eq!(
        list[0].submission_id.as_deref(),
        Some(projection.submitted_id.as_str())
    );
    assert_eq!(list[1].state, ProposalState::Staged);
    assert_eq!(list[1].proposal_id, projection.staged_id);
    assert_eq!(list[2].state, ProposalState::RejectedAtIntake);
    assert_eq!(
        list[2].receipt_id.as_deref(),
        Some(projection.rejected_id.as_str())
    );
    assert_eq!(list[3].state, ProposalState::Ruled);
    assert_eq!(list[3].proposal_id, projection.ruled_id);
    for (id, state) in [
        (projection.submitted_id.as_str(), "submitted"),
        (projection.staged_id.as_str(), "staged"),
        (projection.rejected_id.as_str(), "rejected-at-intake"),
        (projection.ruled_id.as_str(), "ruled"),
    ] {
        assert_eq!(
            proposal_status(&profile, &projection.fixture.store_root, id)
                .unwrap()
                .state
                .as_str(),
            state,
        );
    }
    let verdicts = verdict_list(&profile, &projection.fixture.store_root, false, false)
        .expect("Task 7 verdicts project the Level 4 ledger");
    assert_eq!(verdicts.len(), 1);
    assert_eq!(verdicts[0].proposal_id, projection.ruled_id);
    assert!(verdicts[0].reason.is_none());
    assert!(verdicts[0].evaluator_reasons.is_none());
}

#[test]
fn task7_projection_store_stats_count_each_queue_state_without_private_data() {
    let projection = task7_projection_fixture("task7-projection-store-stats");
    let profile = load_p3_profile(&projection.fixture.profile_path).unwrap();
    let stats = store_stats(&profile, &projection.fixture.store_root)
        .expect("Task 7 stats project verified evidence");
    let queue = serde_json::to_value(&stats.queue).unwrap();
    assert_eq!(queue["submitted"], 1);
    assert_eq!(queue["staged_open"], 1);
    assert_eq!(queue["rejected_at_intake"], 1);
    assert_eq!(queue["open"], 1);
    assert_eq!(queue["alarm_at"], 1);
    assert_eq!(queue["alarm"], true);
}

#[test]
fn task7_real_lifecycle_projection_covers_applied_superseded_retained_and_nonseed_stats() {
    let projection = task7_projection_fixture("task7-projection-real-lifecycle");
    let (superseded_id, applied_id) = add_task7_applied_generations(&projection);
    let profile = load_p3_profile(&projection.fixture.profile_path).unwrap();
    let records = proposal_list(
        &profile,
        &projection.fixture.store_root,
        ProposalFilter { state: None },
    )
    .expect("Task 7 lifecycle projection uses verified protected evidence");
    assert_eq!(
        records
            .iter()
            .map(|record| record.state)
            .collect::<Vec<_>>(),
        vec![
            ProposalState::Submitted,
            ProposalState::Superseded,
            ProposalState::RejectedAtIntake,
            ProposalState::Ruled,
            ProposalState::Applied,
        ],
        "protected terminal states stay in intake sequence order"
    );
    assert_eq!(records[1].proposal_id, superseded_id);
    assert_eq!(records[4].proposal_id, applied_id);
    let public_records = serde_json::to_string(&records).unwrap();
    for private in [
        "PRIVATE_TASK7_FIRST_ACCEPT_REASON_7411",
        "PRIVATE_TASK7_SECOND_ACCEPT_REASON_7413",
        "PRIVATE_TASK7_SUPERSEDING_PATH_7412",
    ] {
        assert!(
            !public_records.contains(private),
            "projection leaked {private}"
        );
    }

    for (state, expected_id) in [
        (ProposalState::Superseded, superseded_id.as_str()),
        (ProposalState::Applied, applied_id.as_str()),
    ] {
        let filtered = proposal_list(
            &profile,
            &projection.fixture.store_root,
            ProposalFilter { state: Some(state) },
        )
        .expect("terminal filter uses verified protected evidence");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].proposal_id, expected_id);
    }

    assert_eq!(
        proposal_status(&profile, &projection.fixture.store_root, &superseded_id)
            .unwrap()
            .state,
        ProposalState::Superseded
    );
    assert_eq!(
        proposal_status(&profile, &projection.fixture.store_root, &applied_id)
            .unwrap()
            .state,
        ProposalState::Applied
    );
    let retained = proposal_list(
        &profile,
        &projection.fixture.store_root,
        ProposalFilter {
            state: Some(ProposalState::Retained),
        },
    )
    .expect("retained filter uses verified protected evidence");
    assert_eq!(retained.len(), 3);
    assert!(retained.iter().all(|record| record.retained));
    assert_eq!(
        retained
            .iter()
            .map(|record| record.proposal_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            superseded_id.as_str(),
            projection.ruled_id.as_str(),
            applied_id.as_str(),
        ]
    );

    let stats = store_stats(&profile, &projection.fixture.store_root)
        .expect("Task 7 lifecycle stats use the applied canonical generation");
    assert_eq!(stats.applied_generations, 2);
    assert_eq!(stats.current_generation, "gen-0000000000000002");
    assert!(stats.bundle.documents > 0);
    let public_stats = serde_json::to_string(&stats).unwrap();
    assert!(!public_stats.contains("PRIVATE_TASK7_"));
}

#[test]
fn task7_protected_order_uses_intake_sequence_across_terminal_states() {
    let record = |proposal_id: &str, state| ProposalRecord {
        submission_id: None,
        proposal_id: proposal_id.to_owned(),
        receipt_id: None,
        state,
        submitted_at: "2026-07-22T01:00:00.000000000Z".to_owned(),
        source_id: None,
        operation: None,
        intent: None,
        target_id: None,
        candidate_hash: None,
        confidence: None,
        evaluator: None,
        terminal_verb: None,
        verdict_id: None,
        retained: true,
        policy_stale: false,
        base_kind: None,
        base_generation: None,
        base_bundle_hash: None,
        base_manifest_hash: None,
        base_target_hash: None,
        policy_hash: None,
        profile_hash: None,
        metadata_hash: None,
        target_path: None,
        reason: None,
        evaluator_reasons: None,
    };
    let ruled_first = record("prp-ruled", ProposalState::Ruled);
    let staged_second = record("prp-staged", ProposalState::Staged);
    assert!(
        proposal_record_order(&ruled_first, Some(1))
            < proposal_record_order(&staged_second, Some(2))
    );
}

#[test]
fn task7_projection_rejects_valid_intake_append_after_level4_without_repair() {
    let projection = task7_projection_fixture("task7-projection-intake-race");
    let late_target = "04-Research/AVM/AVM-OKF-Substrate-Investigation.md";
    let late = submit_at(
        &projection.fixture,
        "src:agent:task7-late-intake",
        late_target,
        replace_candidate(
            &projection.fixture,
            late_target,
            "PRIVATE_TASK7_LATE_PATH_7410.md",
        ),
        None,
        "2026-07-22T01:04:00.000000000Z",
    );
    let ledger_before = fs::read(projection.fixture.store_root.join("ledger/verdicts.jsonl"))
        .expect("read ledger before projection race");
    let current_before = fs::read(projection.fixture.store_root.join("canon/CURRENT"))
        .expect("read current before projection race");
    let profile_path = projection.fixture.profile_path.clone();
    let store_root = projection.fixture.store_root.clone();
    let submission_id = late.submission_id.clone();
    let post_append_intake = Rc::new(RefCell::new(None));
    let post_append_intake_from_hook = Rc::clone(&post_append_intake);
    let late_proposal_id = Rc::new(RefCell::new(None));
    let late_proposal_id_from_hook = Rc::clone(&late_proposal_id);
    sovmem::profile::proposal::install_projection_post_level4_hook_for_test(move || {
        let mut human = ScriptedHuman::new(Some(format!("SEAL INTAKE {submission_id}\n")));
        let outcome = seal_intake_submission_with_human(
            &profile_path,
            &store_root,
            &submission_id,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-22T01:04:01.000000000Z").unwrap()),
        )
        .expect("append a valid protected intake event after Level 4");
        *late_proposal_id_from_hook.borrow_mut() = Some(
            outcome
                .proposal_id
                .expect("late valid intake event publishes a protected pair"),
        );
        *post_append_intake_from_hook.borrow_mut() = Some(
            fs::read(store_root.join("intake/events.jsonl"))
                .expect("capture protected intake after valid append"),
        );
    });

    let profile = load_p3_profile(&projection.fixture.profile_path).unwrap();
    let result = proposal_list(
        &profile,
        &projection.fixture.store_root,
        ProposalFilter { state: None },
    );
    assert_eq!(
        result.as_ref().err().map(ProposalReadError::code),
        Some("transaction.incomplete"),
        "a projection must not mix a post-Level-4 intake append into verified rows"
    );

    let protected_intake = post_append_intake
        .borrow()
        .clone()
        .expect("the post-Level-4 hook ran");
    assert_eq!(
        fs::read(projection.fixture.store_root.join("intake/events.jsonl")).unwrap(),
        protected_intake,
        "the failed projection must not repair protected intake"
    );
    assert_eq!(
        fs::read(projection.fixture.store_root.join("ledger/verdicts.jsonl")).unwrap(),
        ledger_before,
        "the failed projection must not mutate the verdict ledger"
    );
    assert_eq!(
        fs::read(projection.fixture.store_root.join("canon/CURRENT")).unwrap(),
        current_before,
        "the failed projection must not mutate canon"
    );
    let intake = read_intake(
        &inspect_store(&profile, &projection.fixture.store_root)
            .unwrap()
            .paths,
    )
    .expect("the appended protected intake remains valid");
    let late_id = late_proposal_id
        .borrow()
        .clone()
        .expect("late proposal id captured");
    assert!(intake
        .events()
        .iter()
        .any(|event| { event.payload().proposal_id.as_deref() == Some(late_id.as_str()) }));
}

#[test]
fn post_level4_barrier_rejects_concurrently_committed_h2() {
    let error = sovmem::profile::proposal::require_verified_ledger_head(
        &"a".repeat(64),
        Some(&"b".repeat(64)),
    )
    .expect_err("H2 was not part of the verified H snapshot");
    assert_eq!(error.code(), "transaction.incomplete");
}

#[derive(Clone)]
struct Draft {
    submitted_at: Timestamp,
    source_id: String,
    operation: ProposalOperation,
    intent: ProposalIntent,
    target_id: String,
    target_path: String,
    base: ProposalBase,
    candidate: Vec<u8>,
    policy_hash: String,
    profile_hash: String,
    confidence: Option<Confidence>,
    evaluator: Option<EvaluatorBlock>,
}

fn draft() -> Draft {
    let candidate = b"---\nid: ssa-0/04-Research/Example.md\n---\n# Example\n".to_vec();
    let candidate_hash = blake3::hash(&candidate).to_hex().to_string();
    Draft {
        submitted_at: Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap(),
        source_id: "src:agent:example".to_owned(),
        operation: ProposalOperation::Create,
        intent: ProposalIntent::Accept,
        target_id: "ssa-0/04-Research/Example.md".to_owned(),
        target_path: "04-Research/Example.md".to_owned(),
        base: ProposalBase {
            kind: "seed".to_owned(),
            generation_id: None,
            bundle_hash: "a".repeat(64),
            manifest_hash: "b".repeat(64),
            target_hash: None,
        },
        candidate,
        policy_hash: "c".repeat(64),
        profile_hash: "d".repeat(64),
        confidence: Some(Confidence::parse("0.75").unwrap()),
        evaluator: Some(EvaluatorBlock {
            protocol: "adversarial-bug-finding-v1".to_owned(),
            source_id: "src:agent:skeptic".to_owned(),
            evaluated_at: "2026-07-14T04:29:00.000000000Z".to_owned(),
            candidate_hash,
            recommendation: "looks-sound".to_owned(),
            reasons: vec!["No policy conflict found.".to_owned()],
        }),
    }
}

fn prepare_from(draft: &Draft) -> ProposalPair {
    prepare_proposal(ProposalDraft {
        submitted_at: draft.submitted_at.clone(),
        source_id: draft.source_id.clone(),
        operation: draft.operation,
        intent: draft.intent,
        target_id: draft.target_id.clone(),
        target_path: draft.target_path.clone(),
        base: draft.base.clone(),
        candidate_bytes: draft.candidate.clone(),
        policy_hash: draft.policy_hash.clone(),
        profile_hash: draft.profile_hash.clone(),
        confidence: draft.confidence,
        evaluator: draft.evaluator.clone(),
    })
    .unwrap()
}

fn prepare(draft: &Draft) -> String {
    prepare_from(draft).meta.proposal_id
}

struct ProposalFixture {
    root: PathBuf,
    proposals: PathBuf,
    lease: IntakeLease,
}

impl ProposalFixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-proposal-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let staging = root.join("staging");
        let proposals = staging.join("proposals");
        fs::create_dir(&staging).unwrap();
        fs::create_dir(&proposals).unwrap();
        let lease = IntakeLease::for_test(&staging).unwrap();
        Self {
            root,
            proposals,
            lease,
        }
    }

    fn visible_pair(&self, proposal_id: &str) -> PathBuf {
        self.proposals.join(&proposal_id[4..6]).join(proposal_id)
    }
}

impl Drop for ProposalFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct DenyTerminalIntakeParentSync {
    lock_removed: Cell<bool>,
    pre_delete_syncs: Cell<usize>,
    denied_attempts: Cell<usize>,
}

impl DenyTerminalIntakeParentSync {
    const fn new() -> Self {
        Self {
            lock_removed: Cell::new(false),
            pre_delete_syncs: Cell::new(0),
            denied_attempts: Cell::new(0),
        }
    }
}

impl IntakeLeaseReleaseOps for DenyTerminalIntakeParentSync {
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        fs::remove_dir(path)?;
        if path.file_name() == Some(std::ffi::OsStr::new(".intake-lock"))
            && path.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new("staging"))
        {
            self.lock_removed.set(true);
        }
        Ok(())
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        if path.file_name() != Some(std::ffi::OsStr::new("staging")) {
            return sync_directory(path);
        }
        if self.lock_removed.get() {
            self.denied_attempts
                .set(self.denied_attempts.get().saturating_add(1));
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        self.pre_delete_syncs
            .set(self.pre_delete_syncs.get().saturating_add(1));
        sync_directory(path)
    }
}

#[test]
fn standalone_intake_release_terminal_parent_sync_is_liveness_only() {
    let fixture = ProposalFixture::new();
    let ops = DenyTerminalIntakeParentSync::new();

    release_intake_lease_with(&fixture.lease, &ops)
        .expect("standalone intake release must not perform a post-delete parent sync");

    assert_eq!(ops.denied_attempts.get(), 0);
    assert_eq!(ops.pre_delete_syncs.get(), 1);
    assert!(!fixture
        .proposals
        .parent()
        .unwrap()
        .join(".intake-lock")
        .exists());
}

#[test]
fn equal_semantic_drafts_have_one_canonical_content_bound_identity() {
    let draft = draft();
    let first = prepare_from(&draft);
    let second = prepare_from(&draft);

    assert_eq!(first.meta.proposal_id, second.meta.proposal_id);
    assert_eq!(first.meta_bytes, second.meta_bytes);
    assert!(first.meta.proposal_id.starts_with("prp-"));
    assert_eq!(first.meta.proposal_id.len(), 68);
    assert!(first.meta.proposal_id[4..]
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    assert!(first.meta_bytes.ends_with(b"\n"));
    let metadata = std::str::from_utf8(&first.meta_bytes).unwrap();
    assert!(
        metadata.starts_with("{\"schema_version\":\"sovmem-proposal-v1\",\"proposal_id\":\"prp-")
    );
    assert!(metadata.contains("\"confidence\":0.75,\"evaluator\":{"));
    assert!(!metadata[..metadata.len() - 1].ends_with('\n'));
}

#[test]
fn every_semantic_proposal_binding_changes_the_identity() {
    let original = draft();
    let expected = prepare(&original);

    let mut changed = original.clone();
    changed.submitted_at = Timestamp::parse("2026-07-14T04:30:01.000000000Z").unwrap();
    assert_ne!(prepare(&changed), expected, "timestamp");

    let mut changed = original.clone();
    changed.source_id = "src:agent:other".to_owned();
    assert_ne!(prepare(&changed), expected, "source");

    let mut changed = original.clone();
    changed.operation = ProposalOperation::Replace;
    assert_ne!(prepare(&changed), expected, "operation");

    let mut changed = original.clone();
    changed.intent = ProposalIntent::Rewrite;
    assert_ne!(prepare(&changed), expected, "intent");

    let mut changed = original.clone();
    changed.target_id = "ssa-0/04-Research/Other.md".to_owned();
    assert_ne!(prepare(&changed), expected, "target id");

    let mut changed = original.clone();
    changed.target_path = "04-Research/Other.md".to_owned();
    assert_ne!(prepare(&changed), expected, "target path");

    let mut changed = original.clone();
    changed.base.kind = "generation".to_owned();
    changed.base.generation_id = Some("gen-0000000000000001".to_owned());
    assert_ne!(prepare(&changed), expected, "base kind and generation");

    let mut changed = original.clone();
    changed.base.bundle_hash = "e".repeat(64);
    assert_ne!(prepare(&changed), expected, "bundle hash");

    let mut changed = original.clone();
    changed.base.manifest_hash = "f".repeat(64);
    assert_ne!(prepare(&changed), expected, "manifest hash");

    let mut changed = original.clone();
    changed.base.target_hash = Some("0".repeat(64));
    assert_ne!(prepare(&changed), expected, "target hash");

    let mut changed = original.clone();
    changed.policy_hash = "1".repeat(64);
    assert_ne!(prepare(&changed), expected, "policy hash");

    let mut changed = original.clone();
    changed.profile_hash = "2".repeat(64);
    assert_ne!(prepare(&changed), expected, "profile hash");

    let mut changed = original.clone();
    changed.candidate.push(b'!');
    assert_ne!(prepare(&changed), expected, "candidate bytes");

    let mut changed = original.clone();
    changed.confidence = None;
    assert_ne!(prepare(&changed), expected, "confidence presence");

    let mut changed = original.clone();
    changed.confidence = Some(Confidence::parse("0.5").unwrap());
    assert_ne!(prepare(&changed), expected, "confidence value");

    let mut changed = original;
    changed
        .evaluator
        .as_mut()
        .unwrap()
        .reasons
        .push("A distinct evaluator commitment.".to_owned());
    assert_ne!(prepare(&changed), expected, "evaluator commitment");
}

#[test]
fn evaluator_parser_rejects_noncanonical_json_bytes() {
    let fixture = ProposalFixture::new();
    let evaluator = draft().evaluator.expect("fixture evaluator");
    let canonical = serde_json::to_vec(&evaluator).expect("serialize evaluator");
    let canonical_path = fixture.root.join("canonical-evaluator.json");
    fs::write(&canonical_path, &canonical).expect("write canonical evaluator");
    assert_eq!(
        parse_evaluator(&canonical_path, 65_536).expect("canonical evaluator parses"),
        evaluator
    );

    let mut noncanonical = canonical;
    noncanonical.extend_from_slice(b" \n");
    let path = fixture.root.join("noncanonical-evaluator.json");
    fs::write(&path, noncanonical).expect("write evaluator");

    assert_eq!(
        parse_evaluator(&path, 65_536)
            .expect_err("noncanonical evaluator bytes must fail closed")
            .code(),
        "proposal.meta_parse"
    );
}

#[test]
fn published_pairs_are_complete_immutable_and_raw_byte_bound() {
    let fixture = ProposalFixture::new();
    let source = draft();
    let pair = prepare_from(&source);

    let published = publish_proposal_pair(&fixture.proposals, &fixture.lease, &pair).unwrap();
    assert_eq!(published, fixture.visible_pair(&pair.meta.proposal_id));
    let loaded = read_proposal_pair(&fixture.proposals, &pair.meta.proposal_id, 65_536).unwrap();
    assert_eq!(loaded.candidate_bytes, pair.candidate_bytes);
    assert_eq!(loaded.meta_bytes, pair.meta_bytes);
    assert_eq!(loaded.candidate_hash, pair.candidate_hash);
    assert_eq!(loaded.meta_hash, pair.meta_hash);

    let mut tampered = pair.candidate_bytes.clone();
    tampered[0] = b'!';
    fs::write(published.join("candidate.md"), tampered).unwrap();
    assert_eq!(
        read_proposal_pair(&fixture.proposals, &pair.meta.proposal_id, 65_536)
            .unwrap_err()
            .code(),
        "proposal.content_hash_mismatch"
    );
}

#[test]
fn publication_failpoints_never_expose_a_partial_proposal_pair() {
    for failpoint in [
        PublicationFailpoint::AfterCandidateSync,
        PublicationFailpoint::AfterMetaSync,
        PublicationFailpoint::BeforeDirectoryRename,
    ] {
        let fixture = ProposalFixture::new();
        let source = draft();
        let pair = prepare_from(&source);

        assert_eq!(
            publish_proposal_pair_with_failpoint(
                &fixture.proposals,
                &fixture.lease,
                &pair,
                failpoint,
            )
            .unwrap_err(),
            ProposalError::Write
        );
        assert!(!fixture.visible_pair(&pair.meta.proposal_id).exists());
    }

    let fixture = ProposalFixture::new();
    let source = draft();
    let pair = prepare_from(&source);
    let outcome = publish_proposal_pair_with_failpoint(
        &fixture.proposals,
        &fixture.lease,
        &pair,
        PublicationFailpoint::AfterDirectoryRename,
    )
    .unwrap_err();
    assert_eq!(outcome, ProposalError::Write);
    let loaded = read_proposal_pair(&fixture.proposals, &pair.meta.proposal_id, 65_536).unwrap();
    assert_eq!(loaded.meta_hash, pair.meta_hash);
}

#[test]
fn pair_reader_rejects_structural_and_byte_binding_corruption() {
    let fixture = ProposalFixture::new();
    let pair = prepare_from(&draft());
    let published = publish_proposal_pair(&fixture.proposals, &fixture.lease, &pair).unwrap();
    assert_eq!(
        read_proposal_pair(&fixture.proposals, &pair.meta.proposal_id, 0)
            .unwrap_err()
            .code(),
        "proposal.meta_too_large"
    );
    let mut noncanonical = pair.meta_bytes.clone();
    noncanonical.pop();
    noncanonical.extend_from_slice(b" \n");
    fs::write(published.join("proposal.meta.json"), noncanonical).unwrap();
    assert_eq!(
        read_proposal_pair(&fixture.proposals, &pair.meta.proposal_id, 65_536)
            .unwrap_err()
            .code(),
        "proposal.meta_noncanonical"
    );

    let fixture = ProposalFixture::new();
    let pair = prepare_from(&draft());
    let published = publish_proposal_pair(&fixture.proposals, &fixture.lease, &pair).unwrap();
    let mut mismatched_id = pair.meta.clone();
    mismatched_id.source_id = "src:agent:changed".to_owned();
    fs::write(
        published.join("proposal.meta.json"),
        canonical_meta_bytes(&mismatched_id).unwrap(),
    )
    .unwrap();
    assert_eq!(
        read_proposal_pair(&fixture.proposals, &pair.meta.proposal_id, 65_536)
            .unwrap_err()
            .code(),
        "proposal.id_mismatch"
    );

    let fixture = ProposalFixture::new();
    let pair = prepare_from(&draft());
    let published = publish_proposal_pair(&fixture.proposals, &fixture.lease, &pair).unwrap();
    let mut larger_candidate = pair.candidate_bytes.clone();
    larger_candidate.push(b'!');
    fs::write(published.join("candidate.md"), larger_candidate).unwrap();
    assert_eq!(
        read_proposal_pair(&fixture.proposals, &pair.meta.proposal_id, 65_536)
            .unwrap_err()
            .code(),
        "proposal.content_size_mismatch"
    );

    let fixture = ProposalFixture::new();
    let pair = prepare_from(&draft());
    let published = publish_proposal_pair(&fixture.proposals, &fixture.lease, &pair).unwrap();
    fs::write(published.join("unexpected.txt"), b"unexpected\n").unwrap();
    assert_eq!(
        read_proposal_pair(&fixture.proposals, &pair.meta.proposal_id, 65_536)
            .unwrap_err()
            .code(),
        "proposal.pair_incomplete"
    );

    let fixture = ProposalFixture::new();
    let pair = prepare_from(&draft());
    let published = publish_proposal_pair(&fixture.proposals, &fixture.lease, &pair).unwrap();
    fs::remove_file(published.join("candidate.md")).unwrap();
    fs::create_dir(published.join("candidate.md")).unwrap();
    assert_eq!(
        read_proposal_pair(&fixture.proposals, &pair.meta.proposal_id, 65_536)
            .unwrap_err()
            .code(),
        "proposal.pair_incomplete"
    );
}

#[cfg(unix)]
#[test]
fn pair_reader_rejects_symlinked_candidate() {
    use std::os::unix::fs::symlink;

    let fixture = ProposalFixture::new();
    let pair = prepare_from(&draft());
    let published = publish_proposal_pair(&fixture.proposals, &fixture.lease, &pair).unwrap();
    fs::remove_file(published.join("candidate.md")).unwrap();
    symlink("proposal.meta.json", published.join("candidate.md")).unwrap();
    assert_eq!(
        read_proposal_pair(&fixture.proposals, &pair.meta.proposal_id, 65_536)
            .unwrap_err()
            .code(),
        "proposal.path_unsafe"
    );
}

#[test]
fn duplicate_publication_preserves_the_original_pair() {
    let fixture = ProposalFixture::new();
    let pair = prepare_from(&draft());
    let first = publish_proposal_pair(&fixture.proposals, &fixture.lease, &pair).unwrap();
    assert_eq!(
        publish_proposal_pair(&fixture.proposals, &fixture.lease, &pair).unwrap_err(),
        ProposalError::DuplicateId
    );
    assert_eq!(
        read_proposal_pair(&fixture.proposals, &pair.meta.proposal_id, 65_536)
            .unwrap()
            .meta_bytes,
        pair.meta_bytes
    );
    assert!(first.is_dir());
}
