#![allow(dead_code)]

use super::p2_fixture;

use super::transaction::{
    apply_human_verdict, preflight_human_verdict_with_human, read_preflight_state,
    recover_store_with_human_for_test, seal_intake_submission_with_human, FaultInjector,
    FaultPoint, FilesystemOps, HumanPresence, HumanVerdictRequest, LeaseInspection, NoFaults,
    PreflightVerdict, RecoveryOutcome, RecoveryRequest, StdFilesystem, TransactionError,
    TransactionRuntime, VerdictLeaseReceipt,
};
use super::{
    archive_policy, evaluator_commitment, inspect_store, load_p3_profile, read_intake, read_ledger,
    read_sealed_pair_structural, submit_proposal_with_clock, verify_store_level4, CanonicalResult,
    GenesisEvent, GenesisPayload, LedgerEvent, ProposalIntent, ProposalOperation,
    ProposalSubmission, Reason, VerdictEvent, VerdictIdentityInput, VerdictPayload, VerdictVerb,
};
use super::{Clock, ClockError, Timestamp};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

pub fn p2_git_environment_snapshot() -> p2_fixture::GitEnvironmentSnapshot {
    p2_fixture::git_environment_snapshot()
}

pub struct ProvisionedP3Fixture {
    p2: p2_fixture::P2Fixture,
    pub profile_path: PathBuf,
    pub store_root: PathBuf,
    pub proposal_id: String,
}

impl ProvisionedP3Fixture {
    pub fn p2_git_environment_guard_id(&self) -> u64 {
        self.p2.git_environment_guard_id()
    }

    pub fn empty(name: &str) -> Self {
        let p2 = p2_fixture::P2Fixture::new(name);
        let store_root = p2.index_root("p3-preflight-store");
        let profile_directory = p2.index_root("audy-ssa0-p3-v2");
        fs::create_dir(&profile_directory).expect("create empty P3 fixture profile directory");
        let profile_path = profile_directory.join("profile.yaml");
        fs::copy(
            repository_path("profiles/audy-ssa0-p3-v2/profile.yaml"),
            &profile_path,
        )
        .expect("copy empty P3 fixture profile");
        let policy_path = repository_path("profiles/audy-ssa0-p3-v2/policy.toml");
        provision_store(&p2, &store_root, &policy_path);
        Self {
            p2,
            profile_path,
            store_root,
            proposal_id: String::new(),
        }
    }

    pub fn with_staged_reject(name: &str) -> Self {
        Self::with_staged_candidate(name, b"")
    }

    pub fn with_staged_accept_candidate_with_extra_raw_newline(name: &str) -> Self {
        Self::with_staged_candidate(name, b"\n")
    }

    fn with_staged_candidate(name: &str, candidate_suffix: &[u8]) -> Self {
        let p2 = p2_fixture::P2Fixture::new(name);
        let store_root = p2.index_root("p3-preflight-store");
        let profile_directory = p2.index_root("audy-ssa0-p3-v2");
        fs::create_dir(&profile_directory).expect("create P3 fixture profile directory");
        let profile_path = profile_directory.join("profile.yaml");
        fs::copy(
            repository_path("profiles/audy-ssa0-p3-v2/profile.yaml"),
            &profile_path,
        )
        .expect("copy P3 fixture profile");
        let policy_path = repository_path("profiles/audy-ssa0-p3-v2/policy.toml");
        provision_store(&p2, &store_root, &policy_path);

        let target_path = "04-Research/Agent-Loop-Engineering/Implementation-Backlog.md";
        let mut candidate_bytes = String::from_utf8(
            fs::read(store_root.join("seed/bundle").join(target_path))
                .expect("read exact fixture candidate"),
        )
        .expect("fixture candidate is UTF-8")
        .replace(
            "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog",
            "ssa-0/04-Research/Agent-Loop-Engineering/Implementation-Backlog.md",
        )
        .into_bytes();
        candidate_bytes.extend_from_slice(candidate_suffix);
        let candidate_path = p2.index_root("p3-preflight-candidate.md");
        fs::write(&candidate_path, candidate_bytes).expect("write exact fixture candidate");
        let submitted = submit_proposal_with_clock(
            ProposalSubmission {
                profile_path: profile_path.clone(),
                store_root: store_root.clone(),
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
            &FixedClock(Timestamp::parse("2026-07-21T06:30:00.000000000Z").unwrap()),
        )
        .expect("submit valid P3 fixture inbox item");
        let mut sealer =
            ScriptedHuman::new(Some(format!("SEAL INTAKE {}\n", submitted.submission_id)));
        let sealed = seal_intake_submission_with_human(
            &profile_path,
            &store_root,
            &submitted.submission_id,
            &mut sealer,
            &FixedClock(Timestamp::parse("2026-07-21T06:30:01.000000000Z").unwrap()),
        )
        .expect("human-seal valid P3 fixture proposal");
        Self {
            p2,
            profile_path,
            store_root,
            proposal_id: sealed
                .proposal_id
                .expect("policy-passing fixture seal must publish a proposal id"),
        }
    }

    pub fn request(&self, reason: &[u8]) -> super::transaction::HumanVerdictRequest {
        super::transaction::HumanVerdictRequest {
            proposal_id: self.proposal_id.clone(),
            verb: VerdictVerb::Reject,
            reason: Reason::new(reason, 4096).expect("construct fixture reason"),
            rewrite: None,
        }
    }

    pub fn request_for(
        &self,
        verb: VerdictVerb,
        reason: &str,
    ) -> Result<HumanVerdictRequest, TransactionError> {
        Ok(HumanVerdictRequest {
            proposal_id: self.proposal_id.clone(),
            verb,
            reason: Reason::new(reason.as_bytes(), 4096)
                .map_err(|_| TransactionError::ReasonInvalid)?,
            rewrite: None,
        })
    }

    pub fn runtime(&self) -> TransactionRuntime<StdFilesystem, NoFaults> {
        self.runtime_with_parts(StdFilesystem, NoFaults)
    }

    pub fn runtime_with_fault(
        &self,
        faults: FaultPlan,
    ) -> TransactionRuntime<StdFilesystem, FaultPlan> {
        self.runtime_with_parts(StdFilesystem, faults)
    }

    pub fn runtime_with_ops<O: FilesystemOps>(&self, ops: O) -> TransactionRuntime<O, NoFaults> {
        self.runtime_with_parts(ops, NoFaults)
    }

    fn runtime_with_parts<O: FilesystemOps, F: FaultInjector>(
        &self,
        ops: O,
        faults: F,
    ) -> TransactionRuntime<O, F> {
        TransactionRuntime::with_parts(
            fs::canonicalize(&self.store_root).expect("canonicalize provisioned fixture store"),
            LeaseInspection::fixed(
                Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap(),
                7,
            ),
            ops,
            faults,
        )
    }

    pub fn apply(
        &self,
        verb: VerdictVerb,
        reason: &str,
    ) -> Result<super::transaction::VerdictOutcome, TransactionError> {
        let runtime = self.runtime();
        self.apply_with_runtime(verb, reason, &runtime)
    }

    pub fn apply_with_runtime<O, F>(
        &self,
        verb: VerdictVerb,
        reason: &str,
        runtime: &TransactionRuntime<O, F>,
    ) -> Result<super::transaction::VerdictOutcome, TransactionError>
    where
        O: FilesystemOps,
        F: FaultInjector,
    {
        let request = self.request_for(verb, reason)?;
        let initial = read_preflight_state(&self.profile_path, &self.store_root, &request, None)?;
        let confirmation = format!("APPLY {} {}\n", verb.as_str(), &self.proposal_id[..12]);
        let mut human = ScriptedHuman::new(Some(confirmation));
        match preflight_human_verdict_with_human(
            initial,
            request,
            runtime,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap()),
        )? {
            PreflightVerdict::Confirmed(decision) => apply_human_verdict(decision, runtime),
            PreflightVerdict::Idempotent(replay) => Ok(replay.outcome),
        }
    }

    pub fn recover_with_runtime<O, F>(
        &self,
        runtime: &TransactionRuntime<O, F>,
    ) -> Result<RecoveryOutcome, TransactionError>
    where
        O: FilesystemOps,
        F: FaultInjector,
    {
        let owner_bytes = fs::read(self.store_root.join("locks/verdict.lock/owner.json"))
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        let owner: serde_json::Value = serde_json::from_slice(&owner_bytes)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        let owner_id = owner
            .get("owner_id")
            .and_then(serde_json::Value::as_str)
            .ok_or(TransactionError::RecoveryAmbiguous)?;
        let mut human = ScriptedHuman::new(Some(format!("RECOVER {owner_id}\n")));
        recover_store_with_human_for_test(
            RecoveryRequest {
                profile_path: self.profile_path.clone(),
                store_root: self.store_root.clone(),
            },
            runtime,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-15T04:31:00.000000000Z").unwrap()),
        )
    }

    pub fn recover_intake_with_runtime<O, F>(
        &self,
        runtime: &TransactionRuntime<O, F>,
    ) -> Result<RecoveryOutcome, TransactionError>
    where
        O: FilesystemOps,
        F: FaultInjector,
    {
        let owner_bytes = fs::read(self.store_root.join("locks/intake.lock/owner.json"))
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        let owner: serde_json::Value = serde_json::from_slice(&owner_bytes)
            .map_err(|_| TransactionError::RecoveryAmbiguous)?;
        let owner_id = owner
            .get("owner_id")
            .and_then(serde_json::Value::as_str)
            .ok_or(TransactionError::RecoveryAmbiguous)?;
        let mut human = ScriptedHuman::new(Some(format!("RECOVER {owner_id}\n")));
        recover_store_with_human_for_test(
            RecoveryRequest {
                profile_path: self.profile_path.clone(),
                store_root: self.store_root.clone(),
            },
            runtime,
            &mut human,
            &FixedClock(Timestamp::parse("2026-07-22T01:30:00.000000000Z").unwrap()),
        )
    }

    pub fn canon_hashes(&self) -> BTreeMap<String, String> {
        let root = self.store_root.join("canon");
        let mut hashes = BTreeMap::new();
        collect_hashes(&root, &root, &mut hashes);
        hashes
    }

    pub fn index_hashes(&self) -> BTreeMap<String, String> {
        let root = self.store_root.join("derived/indexes");
        let mut hashes = BTreeMap::new();
        collect_hashes(&root, &root, &mut hashes);
        hashes
    }

    pub fn verdict_count(&self, verb: VerdictVerb) -> usize {
        read_ledger(&self.store_root.join("ledger/verdicts.jsonl"))
            .expect("read fixture ledger")
            .events
            .iter()
            .filter(|event| {
                matches!(event, LedgerEvent::Verdict(verdict) if verdict.payload().verb == verb)
            })
            .count()
    }

    #[allow(clippy::result_unit_err)]
    pub fn verify_level4(&self) -> Result<(), ()> {
        let profile = load_p3_profile(&self.profile_path).map_err(|_| ())?;
        verify_store_level4(&profile, &self.store_root)
            .map(|_| ())
            .map_err(|_| ())
    }

    pub fn transactions_empty(&self) -> bool {
        fs::read_dir(self.store_root.join("transactions"))
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false)
    }

    pub fn durable_hashes(&self) -> BTreeMap<String, String> {
        let mut hashes = BTreeMap::new();
        collect_hashes(&self.store_root, &self.store_root, &mut hashes);
        hashes
    }

    pub fn leases_absent(&self) -> bool {
        !self.store_root.join("locks/verdict.lock").exists()
            && !self.store_root.join("locks/intake.lock").exists()
    }

    pub fn commit_exact_reject(&self, request: &super::transaction::HumanVerdictRequest) {
        self.commit_exact_reject_at(
            request,
            Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap(),
        );
    }

    pub fn commit_exact_reject_at(
        &self,
        request: &super::transaction::HumanVerdictRequest,
        ruled_at: Timestamp,
    ) {
        self.commit_exact_reject_at_with_level4(request, ruled_at, true);
    }

    pub fn commit_exact_reject_at_without_level4(
        &self,
        request: &super::transaction::HumanVerdictRequest,
        ruled_at: Timestamp,
    ) {
        self.commit_exact_reject_at_with_level4(request, ruled_at, false);
    }

    fn commit_exact_reject_at_with_level4(
        &self,
        request: &super::transaction::HumanVerdictRequest,
        ruled_at: Timestamp,
        verify_level4_after_write: bool,
    ) {
        let profile = load_p3_profile(&self.profile_path).expect("load fixture P3 profile");
        let store = inspect_store(&profile, &self.store_root).expect("inspect unsealed fixture");
        let pair = read_sealed_pair_structural(&store.paths, &request.proposal_id)
            .expect("read fixture sealed pair");
        let intake = read_intake(&store.paths).expect("read fixture intake");
        let intake_sequence = intake.head().sequence;
        let intake_head_hash = intake
            .head()
            .event_hash
            .clone()
            .expect("sealed fixture has a nonempty intake head");
        archive_policy(&store.paths.policies, &store.policy).expect("archive fixture policy");
        let genesis = GenesisEvent::new(GenesisPayload {
            schema_version: super::LEDGER_EVENT_SCHEMA.to_owned(),
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
        let identity = VerdictIdentityInput {
            proposal_id: &pair.meta.proposal_id,
            verb: request.verb,
            reason: &request.reason,
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
            schema_version: super::LEDGER_EVENT_SCHEMA.to_owned(),
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
            verb: request.verb,
            reason: request.reason.as_str().to_owned(),
            verdict_id: super::verdict_id(&identity),
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
                .unwrap(),
            base_kind: pair.meta.base.kind.clone(),
            base_generation: pair.meta.base.generation_id.clone(),
            base_manifest_hash: pair.meta.base.manifest_hash.clone(),
            base_bundle_hash: pair.meta.base.bundle_hash.clone(),
            base_target_hash: pair.meta.base.target_hash.clone(),
            rewrite_hash: None,
            rewrite_bytes: None,
            canonical_result: CanonicalResult::none(),
            resulting_canonical_head: store.head.as_str().to_owned(),
            intake_sequence,
            intake_head_hash: Some(intake_head_hash),
        })
        .expect("build fixture reject");
        let genesis = LedgerEvent::Genesis(Box::new(genesis));
        let verdict = LedgerEvent::Verdict(Box::new(verdict));
        let mut bytes = genesis.canonical_line_bytes().unwrap();
        bytes.extend_from_slice(&verdict.canonical_line_bytes().unwrap());
        fs::write(&store.paths.ledger, bytes).expect("commit fixture terminal event");
        if verify_level4_after_write {
            verify_store_level4(&profile, &self.store_root)
                .expect("verify committed fixture chain");
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
#[test]
fn fixture_commit_exact_reject_binds_the_human_sealed_pair_and_intake_head() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("sealed-pair-fixture-commit");
    let request = fixture.request(b"sealed pair fixture commit reason");

    fixture.commit_exact_reject(&request);

    let profile = load_p3_profile(&fixture.profile_path).expect("load fixture P3 profile");
    let store = inspect_store(&profile, &fixture.store_root).expect("inspect committed fixture");
    let intake = super::read_intake(&store.paths).expect("read fixture intake");
    let ledger = read_ledger(&store.paths.ledger).expect("read committed fixture ledger");
    let verdict = ledger
        .events
        .iter()
        .find_map(|event| match event {
            LedgerEvent::Verdict(event) => Some(event.payload()),
            LedgerEvent::Genesis(_) => None,
        })
        .expect("fixture ledger contains a verdict");

    assert_eq!(verdict.proposal_id, fixture.proposal_id);
    assert_eq!(verdict.intake_sequence, intake.head().sequence);
    assert_eq!(
        verdict.intake_head_hash.as_deref(),
        intake.head().event_hash.as_deref()
    );
}

fn provision_store(
    p2: &p2_fixture::P2Fixture,
    root: &std::path::Path,
    policy_path: &std::path::Path,
) {
    fs::create_dir(root).expect("create P3 fixture store root");
    copy_tree(&p2.bundle_root, &root.join("seed"));
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
        fs::create_dir_all(root.join(directory)).expect("create P3 fixture directory");
    }
    fs::write(root.join("policy.toml"), fs::read(policy_path).unwrap()).unwrap();
    fs::write(root.join("ledger/verdicts.jsonl"), b"").unwrap();
    fs::write(root.join("intake/events.jsonl"), b"").unwrap();
    fs::write(root.join("canon/CURRENT"), b"seed\n").unwrap();
}

fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
    let metadata = fs::symlink_metadata(source).expect("read fixture source metadata");
    assert!(
        !metadata.file_type().is_symlink(),
        "fixture source cannot be a symlink"
    );
    if metadata.is_file() {
        fs::write(destination, fs::read(source).unwrap()).expect("copy fixture file");
        return;
    }
    fs::create_dir(destination).expect("create fixture destination directory");
    let mut entries = fs::read_dir(source)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        copy_tree(&entry.path(), &destination.join(entry.file_name()));
    }
}

fn repository_path(relative: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative)
}

pub struct P3TestFixture {
    root: PathBuf,
    inspection: LeaseInspection,
}

impl P3TestFixture {
    pub fn with_staged_proposal(name: &str, _verb: VerdictVerb) -> ProvisionedP3Fixture {
        ProvisionedP3Fixture::with_staged_reject(name)
    }

    pub fn new(name: &str) -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-p3-transaction-{name}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create transaction fixture root");
        for path in ["locks", "staging", "transactions"] {
            fs::create_dir(root.join(path)).expect("create transaction fixture directory");
        }
        Self {
            root,
            inspection: LeaseInspection::fixed(
                Timestamp::parse("2026-07-14T04:30:00.000000000Z").unwrap(),
                7,
            ),
        }
    }

    pub fn runtime(&self) -> TransactionRuntime<StdFilesystem, NoFaults> {
        TransactionRuntime::with_parts(
            self.root.clone(),
            self.inspection.clone(),
            StdFilesystem,
            NoFaults,
        )
    }

    pub fn runtime_with_fault(
        &self,
        faults: FaultPlan,
    ) -> TransactionRuntime<StdFilesystem, FaultPlan> {
        TransactionRuntime::with_parts(
            self.root.clone(),
            self.inspection.clone(),
            StdFilesystem,
            faults,
        )
    }

    pub fn runtime_with_ops<O: FilesystemOps>(&self, ops: O) -> TransactionRuntime<O, NoFaults> {
        TransactionRuntime::with_parts(self.root.clone(), self.inspection.clone(), ops, NoFaults)
    }

    pub fn replace_verdict_owner(&self) {
        fs::write(self.root.join("locks/verdict.lock/owner.json"), b"{}\n")
            .expect("replace fixture verdict owner");
    }

    pub fn verdict_lease_exists(&self) -> bool {
        self.root.join("locks/verdict.lock").exists()
    }

    pub fn transaction_exists(&self, transaction_id: &str) -> bool {
        self.root.join("transactions").join(transaction_id).exists()
    }

    pub fn create_empty_verdict_lock(&self) {
        fs::create_dir(self.root.join("locks/verdict.lock"))
            .expect("create fixture empty verdict lock");
    }

    pub fn create_empty_cleanup(&self, transaction_id: &str) {
        fs::create_dir(
            self.root
                .join("transactions")
                .join(format!(".cleanup-{transaction_id}")),
        )
        .expect("create fixture empty cleanup directory");
    }

    pub fn remove_empty_cleanup(&self, transaction_id: &str) {
        fs::remove_dir(
            self.root
                .join("transactions")
                .join(format!(".cleanup-{transaction_id}")),
        )
        .expect("remove fixture empty cleanup directory");
    }

    pub fn verdict_lease_receipt(&self, label: &str) -> VerdictLeaseReceipt {
        VerdictLeaseReceipt::new(
            "fixture-store",
            Some("seed"),
            &"a".repeat(64),
            &"b".repeat(64),
            &format!("prp-{}", blake3::hash(label.as_bytes()).to_hex()),
        )
        .expect("create fixture verdict receipt")
    }

    pub fn durable_hashes(&self) -> BTreeMap<String, String> {
        let mut hashes = BTreeMap::new();
        collect_hashes(&self.root, &self.root, &mut hashes);
        hashes
    }
}

fn collect_hashes(
    root: &std::path::Path,
    directory: &std::path::Path,
    hashes: &mut BTreeMap<String, String>,
) {
    let mut entries = fs::read_dir(directory)
        .expect("read fixture directory")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect fixture entries");
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).expect("read fixture entry metadata");
        if metadata.is_dir() {
            collect_hashes(root, &path, hashes);
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .expect("fixture entry below root")
                .to_string_lossy()
                .replace('\\', "/");
            hashes.insert(
                relative,
                blake3::hash(&fs::read(&path).unwrap()).to_hex().to_string(),
            );
        }
    }
}

impl Drop for P3TestFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub struct FixedClock(pub Timestamp);

impl Clock for FixedClock {
    fn now(&self) -> Result<Timestamp, ClockError> {
        Ok(self.0.clone())
    }
}

pub struct ScriptedHuman {
    response: Option<String>,
    displayed: Vec<u8>,
    show_count: usize,
    read_count: usize,
}

impl ScriptedHuman {
    pub fn new(response: Option<String>) -> Self {
        Self {
            response,
            displayed: Vec::new(),
            show_count: 0,
            read_count: 0,
        }
    }

    pub fn displayed(&self) -> &[u8] {
        &self.displayed
    }

    pub const fn show_count(&self) -> usize {
        self.show_count
    }

    pub const fn read_count(&self) -> usize {
        self.read_count
    }
}

impl HumanPresence for ScriptedHuman {
    fn show(&mut self, bytes: &[u8]) -> Result<(), TransactionError> {
        self.show_count += 1;
        self.displayed.extend_from_slice(bytes);
        Ok(())
    }

    fn read_confirmation(&mut self) -> Result<String, TransactionError> {
        self.read_count += 1;
        self.response
            .take()
            .ok_or(TransactionError::ConfirmationFailed)
    }
}

pub struct FaultPlan {
    fail_at: Option<FaultPoint>,
    reached: RefCell<Vec<FaultPoint>>,
}

pub struct DenyOwnerRead;

#[derive(Clone)]
pub struct RecoveryLeasePublicationBarrier {
    protected_lease: PathBuf,
    state: Arc<(Mutex<RecoveryLeasePublicationState>, Condvar)>,
}

#[derive(Default)]
struct RecoveryLeasePublicationState {
    published: bool,
    released: bool,
}

impl RecoveryLeasePublicationBarrier {
    pub fn after_sync_of(protected_lease: PathBuf) -> Self {
        Self {
            protected_lease,
            state: Arc::new((
                Mutex::new(RecoveryLeasePublicationState::default()),
                Condvar::new(),
            )),
        }
    }

    pub fn wait_until_published(&self) {
        let (lock, ready) = &*self.state;
        let state = lock.lock().expect("lock recovery publication barrier");
        let (state, timeout) = ready
            .wait_timeout_while(state, Duration::from_secs(5), |state| !state.published)
            .expect("wait for recovery publication barrier");
        assert!(
            state.published && !timeout.timed_out(),
            "recovery did not publish its protected lease"
        );
    }

    pub fn release(&self) {
        let (lock, ready) = &*self.state;
        let mut state = lock.lock().expect("lock recovery publication barrier");
        state.released = true;
        ready.notify_all();
    }

    fn pause_after_publication(&self, path: &Path) {
        if path != self.protected_lease {
            return;
        }
        let (lock, ready) = &*self.state;
        let mut state = lock.lock().expect("lock recovery publication barrier");
        if state.published {
            return;
        }
        state.published = true;
        ready.notify_all();
        while !state.released {
            state = ready
                .wait(state)
                .expect("wait for recovery publication release");
        }
    }
}

impl FilesystemOps for RecoveryLeasePublicationBarrier {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        StdFilesystem.read(path)
    }

    fn write_new_synced(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)?;
        self.pause_after_publication(path);
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

impl FilesystemOps for DenyOwnerRead {
    fn create_dir(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &std::path::Path) -> std::io::Result<Vec<u8>> {
        if path.file_name().is_some_and(|name| name == "owner.json") {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        StdFilesystem.read(path)
    }

    fn write_new_synced(&self, path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &std::path::Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

pub struct RejectRename;

impl FilesystemOps for RejectRename {
    fn create_dir(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &std::path::Path) -> std::io::Result<Vec<u8>> {
        StdFilesystem.read(path)
    }

    fn write_new_synced(&self, path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, _from: &std::path::Path, _to: &std::path::Path) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    }

    fn hard_link(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &std::path::Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TerminalParentSync {
    TransactionCleanup,
    VerdictLease,
}

#[derive(Clone)]
pub struct DenyTerminalParentSync {
    target: TerminalParentSync,
    state: Rc<RefCell<TerminalParentSyncState>>,
}

#[derive(Default)]
struct TerminalParentSyncState {
    armed_parent: Option<PathBuf>,
    denied_attempts: usize,
}

impl DenyTerminalParentSync {
    pub fn new(target: TerminalParentSync) -> Self {
        Self {
            target,
            state: Rc::new(RefCell::new(TerminalParentSyncState::default())),
        }
    }

    pub fn denied_attempts(&self) -> usize {
        self.state.borrow().denied_attempts
    }

    fn is_terminal_directory(&self, path: &std::path::Path) -> bool {
        match self.target {
            TerminalParentSync::TransactionCleanup => {
                path.file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|name| name.starts_with(".cleanup-"))
                    && path.parent().and_then(std::path::Path::file_name)
                        == Some(std::ffi::OsStr::new("transactions"))
            }
            TerminalParentSync::VerdictLease => {
                path.file_name() == Some(std::ffi::OsStr::new("verdict.lock"))
                    && path.parent().and_then(std::path::Path::file_name)
                        == Some(std::ffi::OsStr::new("locks"))
            }
        }
    }

    fn denied() -> std::io::Error {
        std::io::Error::from(std::io::ErrorKind::PermissionDenied)
    }
}

impl FilesystemOps for DenyTerminalParentSync {
    fn create_dir(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &std::path::Path) -> std::io::Result<Vec<u8>> {
        StdFilesystem.read(path)
    }

    fn write_new_synced(&self, path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut state = self.state.borrow_mut();
        if state.armed_parent.as_deref() == Some(path) {
            state.denied_attempts += 1;
            return Err(Self::denied());
        }
        drop(state);
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)?;
        if self.is_terminal_directory(path) {
            self.state.borrow_mut().armed_parent = path.parent().map(PathBuf::from);
        }
        Ok(())
    }

    fn metadata(&self, path: &std::path::Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JournalFilesystemFailure {
    LedgerCommittedMarker,
    CleanupHardLink,
    CleanupRemove,
}

pub struct FailJournalFilesystem {
    failure: JournalFilesystemFailure,
}

impl FailJournalFilesystem {
    pub const fn new(failure: JournalFilesystemFailure) -> Self {
        Self { failure }
    }

    fn is_cleanup_marker(path: &std::path::Path) -> bool {
        path.parent()
            .and_then(std::path::Path::file_name)
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| name.starts_with(".cleanup-"))
    }

    fn is_transaction_marker(path: &std::path::Path) -> bool {
        path.file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| {
                matches!(
                    name,
                    "prepared.json" | "ledger-committed.json" | "published.json"
                )
            })
            && path
                .parent()
                .and_then(std::path::Path::parent)
                .and_then(std::path::Path::file_name)
                == Some(std::ffi::OsStr::new("transactions"))
    }

    fn denied() -> std::io::Error {
        std::io::Error::from(std::io::ErrorKind::PermissionDenied)
    }
}

impl FilesystemOps for FailJournalFilesystem {
    fn create_dir(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &std::path::Path) -> std::io::Result<Vec<u8>> {
        StdFilesystem.read(path)
    }

    fn write_new_synced(&self, path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
        if self.failure == JournalFilesystemFailure::LedgerCommittedMarker
            && path.file_name() == Some(std::ffi::OsStr::new("ledger-committed.json"))
        {
            return Err(Self::denied());
        }
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        if self.failure == JournalFilesystemFailure::CleanupHardLink && Self::is_cleanup_marker(to)
        {
            return Err(Self::denied());
        }
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        if self.failure == JournalFilesystemFailure::CleanupRemove
            && Self::is_transaction_marker(path)
        {
            return Err(Self::denied());
        }
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &std::path::Path) -> std::io::Result<()> {
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &std::path::Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

pub struct RejectProtectedWrites {
    root: PathBuf,
}

impl RejectProtectedWrites {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn rejects(&self, path: &std::path::Path) -> bool {
        ["canon", "derived", "evidence/reindex"]
            .iter()
            .any(|segment| path.starts_with(self.root.join(segment)))
    }
}

impl FilesystemOps for RejectProtectedWrites {
    fn create_dir(&self, path: &std::path::Path) -> std::io::Result<()> {
        if self.rejects(path) {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        StdFilesystem.create_dir(path)
    }

    fn read(&self, path: &std::path::Path) -> std::io::Result<Vec<u8>> {
        StdFilesystem.read(path)
    }

    fn write_new_synced(&self, path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
        if self.rejects(path) {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        StdFilesystem.write_new_synced(path, bytes)
    }

    fn sync_directory(&self, path: &std::path::Path) -> std::io::Result<()> {
        if self.rejects(path) {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        StdFilesystem.sync_directory(path)
    }

    fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        if self.rejects(from) || self.rejects(to) {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        StdFilesystem.rename(from, to)
    }

    fn hard_link(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        if self.rejects(from) || self.rejects(to) {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        StdFilesystem.hard_link(from, to)
    }

    fn remove_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        if self.rejects(path) {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        StdFilesystem.remove_file(path)
    }

    fn remove_dir(&self, path: &std::path::Path) -> std::io::Result<()> {
        if self.rejects(path) {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
        StdFilesystem.remove_dir(path)
    }

    fn metadata(&self, path: &std::path::Path) -> std::io::Result<std::fs::Metadata> {
        StdFilesystem.metadata(path)
    }
}

impl FaultPlan {
    pub fn none() -> Self {
        Self {
            fail_at: None,
            reached: RefCell::new(Vec::new()),
        }
    }

    pub fn at(point: FaultPoint) -> Self {
        Self {
            fail_at: Some(point),
            reached: RefCell::new(Vec::new()),
        }
    }

    pub fn reached(&self) -> Vec<FaultPoint> {
        self.reached.borrow().clone()
    }
}

impl FaultInjector for FaultPlan {
    fn check(&self, point: FaultPoint) -> Result<(), TransactionError> {
        self.reached.borrow_mut().push(point);
        if self.fail_at == Some(point) {
            return Err(TransactionError::FaultInjected);
        }
        Ok(())
    }
}
