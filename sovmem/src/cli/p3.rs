use serde::Serialize;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::profile::{
    commit_human_verdict, inspect_store, load_p3_profile, p3_canonical_document_id, proposal_list,
    proposal_status_with_advice, query_store, rebuild_store_index, recover_store_human,
    safe_target_path, seal_intake_submission, store_stats, submit_proposal, verdict_list,
    verify_store_level4, BundleStats, Confidence, EvaluatorSummary, HumanVerdictRequest,
    ProposalError, ProposalFilter, ProposalIntent, ProposalOperation, ProposalReadError,
    ProposalRecord, ProposalState, ProposalSubmission, QueueStats, Reason, RecoveryRequest,
    RewriteBytes, StoreIndexRequest, StoreQueryRequest, StoreStatsError, TransactionError,
    VerdictRecord, VerdictVerb, VerificationDiagnostic, INDEX_MANIFEST_SCHEMA_V2, P3_STORE_ID,
};

use super::{
    emit_diagnostics, print_json, ExpansionArg, OutputFormat, ProposalIntentArg,
    ProposalOperationArg, ProposalStateArg,
};

pub(super) struct VerdictArgs {
    pub proposal_id: String,
    pub verb: String,
    pub profile: PathBuf,
    pub store: PathBuf,
    pub reason: Option<String>,
    pub reason_file: Option<PathBuf>,
    pub rewrite_file: Option<PathBuf>,
    pub format: OutputFormat,
}

#[derive(Serialize)]
struct VerdictOutput {
    status: &'static str,
    command: &'static str,
    proposal_id: String,
    verdict_id: String,
    event_id: String,
    verb: VerdictVerb,
    applied: bool,
    generation: Option<String>,
    bundle_hash: Option<String>,
    manifest_hash: Option<String>,
    target_hash: Option<String>,
    judgment_hash: Option<String>,
    ledger_head: String,
    reindex_receipt_hash: Option<String>,
    idempotent: bool,
}

pub(super) fn run_verdict(args: VerdictArgs) -> i32 {
    let verb = match VerdictVerb::from_str(&args.verb) {
        Ok(verb) => verb,
        Err(_) => return fail_read("verdict.verb_invalid", "verdict verb is invalid", 2),
    };
    if verb == VerdictVerb::Rewrite && args.rewrite_file.is_none() {
        return fail_read("human.rewrite_required", "human rewrite is required", 2);
    }
    if verb != VerdictVerb::Rewrite && args.rewrite_file.is_some() {
        return fail_read("human.rewrite_forbidden", "human rewrite is forbidden", 2);
    }
    let profile = match load_p3_profile(&args.profile) {
        Ok(profile) => profile,
        Err(error) => return fail_read(error.code(), "P3 profile could not be loaded", 2),
    };
    let snapshot = match inspect_store(&profile, &args.store) {
        Ok(snapshot) => snapshot,
        Err(error) => return fail_code(error.code(), "store could not be read"),
    };
    let reason_bytes = match (args.reason, args.reason_file) {
        (Some(reason), None) => reason.into_bytes(),
        (None, Some(path)) => match read_regular_input(&path) {
            Ok(bytes) => bytes,
            Err(()) => return fail_read("human.reason_invalid", "human reason is invalid", 2),
        },
        _ => return fail_usage("verdict reason input is invalid"),
    };
    let reason = match Reason::new(&reason_bytes, snapshot.policy.policy.max_reason_bytes) {
        Ok(reason) => reason,
        Err(_) => return fail_read("human.reason_invalid", "human reason is invalid", 2),
    };
    let rewrite = match args.rewrite_file {
        Some(path) => match RewriteBytes::read(&path, snapshot.policy.policy.max_rewrite_bytes) {
            Ok(rewrite) => Some(rewrite),
            Err(_) => return fail_read("human.rewrite_invalid", "human rewrite is invalid", 2),
        },
        None => None,
    };
    let outcome = match commit_human_verdict(
        &args.profile,
        &args.store,
        HumanVerdictRequest {
            proposal_id: args.proposal_id,
            verb,
            reason,
            rewrite,
        },
    ) {
        Ok(outcome) => outcome,
        Err(error) => return fail_transaction(&error),
    };
    let level4 = match verify_store_level4(&profile, &args.store) {
        Ok(summary) => summary,
        Err(error) => return fail_code(error.code(), "store verification failed"),
    };
    let record = match verdict_list(&profile, &args.store, false, false)
        .ok()
        .and_then(|records| {
            records
                .into_iter()
                .find(|record| record.verdict_id == outcome.verdict_id)
        }) {
        Some(record) => record,
        None => {
            return fail_read(
                "ledger.verdict_binding",
                "verdict result could not be verified",
                1,
            )
        }
    };
    let output = VerdictOutput {
        status: "ok",
        command: "verdict",
        proposal_id: outcome.proposal_id,
        verdict_id: outcome.verdict_id,
        event_id: outcome.event_id,
        verb,
        applied: outcome.applied,
        generation: outcome.generation,
        bundle_hash: record.bundle_hash,
        manifest_hash: record.manifest_hash,
        target_hash: record.target_new_hash,
        judgment_hash: record.judgment_hash,
        ledger_head: level4.ledger_head,
        reindex_receipt_hash: record.reindex_receipt_hash,
        idempotent: outcome.idempotent,
    };
    match args.format {
        OutputFormat::Json => print_json(&output),
        OutputFormat::Text => print_verdict_text(&output),
    }
    0
}

pub(super) struct ProposeArgs {
    pub profile: PathBuf,
    pub store: PathBuf,
    pub source: String,
    pub operation: ProposalOperationArg,
    pub intent: ProposalIntentArg,
    pub target_id: String,
    pub target_path: String,
    pub candidate: PathBuf,
    pub confidence: Option<String>,
    pub evaluator: Option<PathBuf>,
    pub format: OutputFormat,
}

#[derive(Serialize)]
struct ProposeOutput {
    status: &'static str,
    command: &'static str,
    submission_id: String,
    state: String,
    candidate_hash: String,
    policy_hash: String,
    profile_hash: String,
    submitted_at: String,
}

pub(super) fn run_propose(args: ProposeArgs) -> i32 {
    let confidence = match args
        .confidence
        .as_deref()
        .map(Confidence::parse)
        .transpose()
    {
        Ok(confidence) => confidence,
        Err(error) => return fail_proposal(&error),
    };
    let operation = match args.operation {
        ProposalOperationArg::Create => ProposalOperation::Create,
        ProposalOperationArg::Replace => ProposalOperation::Replace,
    };
    let intent = match args.intent {
        ProposalIntentArg::Accept => ProposalIntent::Accept,
        ProposalIntentArg::Rewrite => ProposalIntent::Rewrite,
    };
    let profile = match load_p3_profile(&args.profile) {
        Ok(profile) => profile,
        Err(error) => return fail_read(error.code(), "P3 profile could not be loaded", 2),
    };
    if !safe_target_path(&args.target_path)
        || p3_canonical_document_id(&profile.config.context_namespace, &args.target_path).as_deref()
            != Some(args.target_id.as_str())
    {
        return fail_read(
            "proposal.target_unsafe",
            "proposal target path is unsafe",
            2,
        );
    }
    match submit_proposal(ProposalSubmission {
        profile_path: args.profile,
        store_root: args.store,
        source_id: args.source,
        operation,
        intent,
        target_id: args.target_id,
        target_path: args.target_path,
        candidate_path: args.candidate,
        confidence,
        evaluator_path: args.evaluator,
    }) {
        Ok(result) => {
            let output = ProposeOutput {
                status: "ok",
                command: "propose",
                submission_id: result.submission_id,
                state: result.state,
                candidate_hash: result.candidate_hash,
                policy_hash: result.policy_hash,
                profile_hash: result.profile_hash,
                submitted_at: result.submitted_at,
            };
            match args.format {
                OutputFormat::Json => print_json(&output),
                OutputFormat::Text => print_propose_text(&output),
            }
            0
        }
        Err(error) => fail_proposal(&error),
    }
}

fn print_propose_text(output: &ProposeOutput) {
    println!("status={}", output.status);
    println!("command={}", output.command);
    println!("submission_id={}", output.submission_id);
    println!("state={}", output.state);
    println!("candidate_hash={}", output.candidate_hash);
    println!("policy_hash={}", output.policy_hash);
    println!("profile_hash={}", output.profile_hash);
    println!("submitted_at={}", output.submitted_at);
}

pub(super) struct IntakeSealArgs {
    pub submission_id: String,
    pub profile: PathBuf,
    pub store: PathBuf,
    pub format: OutputFormat,
}

#[derive(Serialize)]
struct IntakeSealOutput {
    status: &'static str,
    command: &'static str,
    submission_id: String,
    proposal_id: Option<String>,
    state: String,
    intake_sequence: u64,
    intake_head_hash: String,
    policy_hash: String,
    profile_hash: String,
}

pub(super) fn run_intake_seal(args: IntakeSealArgs) -> i32 {
    let outcome = match seal_intake_submission(&args.profile, &args.store, &args.submission_id) {
        Ok(outcome) => outcome,
        Err(error) => return fail_transaction(&error),
    };
    let output = IntakeSealOutput {
        status: "ok",
        command: "intake seal",
        submission_id: outcome.submission_id,
        proposal_id: outcome.proposal_id,
        state: outcome.state,
        intake_sequence: outcome.intake_sequence,
        intake_head_hash: outcome.intake_head_hash,
        policy_hash: outcome.policy_hash,
        profile_hash: outcome.profile_hash,
    };
    match args.format {
        OutputFormat::Json => print_json(&output),
        OutputFormat::Text => print_intake_seal_text(&output),
    }
    0
}

fn print_intake_seal_text(output: &IntakeSealOutput) {
    println!("status={}", output.status);
    println!("command={}", output.command);
    println!("submission_id={}", output.submission_id);
    println!(
        "proposal_id={}",
        output.proposal_id.as_deref().unwrap_or("null")
    );
    println!("state={}", output.state);
    println!("intake_sequence={}", output.intake_sequence);
    println!("intake_head_hash={}", output.intake_head_hash);
    println!("policy_hash={}", output.policy_hash);
    println!("profile_hash={}", output.profile_hash);
}

fn fail_proposal(error: &ProposalError) -> i32 {
    if let Some(rule_codes) = error.rule_codes() {
        let diagnostics = rule_codes
            .iter()
            .map(|code| {
                VerificationDiagnostic::error(code.clone(), "proposal was rejected by policy")
            })
            .collect();
        emit_diagnostics(diagnostics);
        return if matches!(error, ProposalError::ReceiptWrite { .. }) {
            2
        } else {
            1
        };
    }
    let (message, exit) = match error {
        ProposalError::ConfidenceInvalid => ("proposal confidence is invalid", 2),
        ProposalError::PathUnsafe => ("proposal target path is unsafe", 2),
        ProposalError::Read => ("proposal input could not be read", 2),
        ProposalError::Write | ProposalError::ReceiptWrite { .. } => {
            ("proposal could not be written", 2)
        }
        ProposalError::Locked | ProposalError::ProtectedLocked => ("proposal intake is locked", 1),
        ProposalError::Rejected { .. } | ProposalError::DuplicateId => ("proposal was rejected", 1),
        _ => ("proposal input is invalid", 2),
    };
    emit_diagnostics(vec![VerificationDiagnostic::error(error.code(), message)]);
    exit
}

#[derive(Serialize)]
struct ProposalListOutput {
    status: &'static str,
    command: &'static str,
    proposals: Vec<ProposalListItem>,
}

#[derive(Serialize)]
struct ProposalListItem {
    submission_id: Option<String>,
    proposal_id: Option<String>,
    receipt_id: Option<String>,
    state: &'static str,
    submitted_at: Option<String>,
    received_at: Option<String>,
    source_id: Option<String>,
    operation: Option<ProposalOperation>,
    intent: Option<ProposalIntent>,
    target_id: Option<String>,
    candidate_hash: Option<String>,
    confidence: Option<Confidence>,
    evaluator: Option<EvaluatorSummary>,
    terminal_verb: Option<VerdictVerb>,
    verdict_id: Option<String>,
    retained: bool,
}

impl From<ProposalRecord> for ProposalListItem {
    fn from(record: ProposalRecord) -> Self {
        let rejected = record.state == ProposalState::RejectedAtIntake;
        Self {
            submission_id: record.submission_id,
            proposal_id: (!record.proposal_id.is_empty()).then_some(record.proposal_id),
            receipt_id: record.receipt_id,
            state: record.state.as_str(),
            submitted_at: (!rejected).then_some(record.submitted_at.clone()),
            received_at: rejected.then_some(record.submitted_at),
            source_id: record.source_id,
            operation: record.operation,
            intent: record.intent,
            target_id: record.target_id,
            candidate_hash: record.candidate_hash,
            confidence: record.confidence,
            evaluator: record.evaluator,
            terminal_verb: record.terminal_verb,
            verdict_id: record.verdict_id,
            retained: record.retained,
        }
    }
}

pub(super) fn run_proposals_list(
    profile_path: PathBuf,
    store: PathBuf,
    state: Option<ProposalStateArg>,
    format: OutputFormat,
) -> i32 {
    let profile = match load_p3_profile(&profile_path) {
        Ok(profile) => profile,
        Err(error) => return fail_read(error.code(), "P3 profile could not be loaded", 2),
    };
    let state = state.map(map_state);
    let records = match proposal_list(&profile, &store, ProposalFilter { state }) {
        Ok(records) => records.into_iter().map(ProposalListItem::from).collect(),
        Err(error) => return fail_proposal_read(&error),
    };
    let output = ProposalListOutput {
        status: "ok",
        command: "proposals.list",
        proposals: records,
    };
    match format {
        OutputFormat::Json => print_json(&output),
        OutputFormat::Text => print_list_text(&output),
    }
    0
}

#[derive(Serialize)]
struct ProposalStatusOutput {
    status: &'static str,
    command: &'static str,
    proposal: ProposalStatusItem,
}

#[derive(Serialize)]
struct ProposalStatusItem {
    submission_id: Option<String>,
    proposal_id: Option<String>,
    receipt_id: Option<String>,
    state: &'static str,
    submitted_at: Option<String>,
    received_at: Option<String>,
    source_id: Option<String>,
    operation: Option<ProposalOperation>,
    intent: Option<ProposalIntent>,
    target_id: Option<String>,
    candidate_hash: Option<String>,
    confidence: Option<Confidence>,
    evaluator: Option<EvaluatorSummary>,
    terminal_verb: Option<VerdictVerb>,
    verdict_id: Option<String>,
    retained: bool,
    base_kind: Option<String>,
    base_generation: Option<String>,
    base_bundle_hash: Option<String>,
    base_manifest_hash: Option<String>,
    base_target_hash: Option<String>,
    policy_hash: Option<String>,
    profile_hash: Option<String>,
    metadata_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    evaluator_reasons: Option<Vec<String>>,
}

impl From<ProposalRecord> for ProposalStatusItem {
    fn from(record: ProposalRecord) -> Self {
        let rejected = record.state == ProposalState::RejectedAtIntake;
        Self {
            submission_id: record.submission_id,
            proposal_id: (!record.proposal_id.is_empty()).then_some(record.proposal_id),
            receipt_id: record.receipt_id,
            state: record.state.as_str(),
            submitted_at: (!rejected).then_some(record.submitted_at.clone()),
            received_at: rejected.then_some(record.submitted_at),
            source_id: record.source_id,
            operation: record.operation,
            intent: record.intent,
            target_id: record.target_id,
            candidate_hash: record.candidate_hash,
            confidence: record.confidence,
            evaluator: record.evaluator,
            terminal_verb: record.terminal_verb,
            verdict_id: record.verdict_id,
            retained: record.retained,
            base_kind: record.base_kind,
            base_generation: record.base_generation,
            base_bundle_hash: record.base_bundle_hash,
            base_manifest_hash: record.base_manifest_hash,
            base_target_hash: record.base_target_hash,
            policy_hash: record.policy_hash,
            profile_hash: record.profile_hash,
            metadata_hash: record.metadata_hash,
            evaluator_reasons: record.evaluator_reasons,
        }
    }
}

pub(super) fn run_proposal_status(
    profile_path: PathBuf,
    store: PathBuf,
    proposal_id: String,
    show_advice: bool,
    format: OutputFormat,
) -> i32 {
    let profile = match load_p3_profile(&profile_path) {
        Ok(profile) => profile,
        Err(error) => return fail_read(error.code(), "P3 profile could not be loaded", 2),
    };
    let record = match proposal_status_with_advice(&profile, &store, &proposal_id, show_advice) {
        Ok(record) => record,
        Err(error) => return fail_proposal_read(&error),
    };
    let output = ProposalStatusOutput {
        status: "ok",
        command: "proposals.status",
        proposal: record.into(),
    };
    match format {
        OutputFormat::Json => print_json(&output),
        OutputFormat::Text => print_status_text(&output),
    }
    0
}

#[derive(Serialize)]
struct StoreStatsOutput {
    status: &'static str,
    command: &'static str,
    sealed: bool,
    #[serde(flatten)]
    bundle: BundleStats,
    queue: QueueStats,
    ledger: LedgerStatsOutput,
    store_id: &'static str,
    profile_version: String,
    current_generation: String,
}

#[derive(Serialize)]
struct LedgerStatsOutput {
    events: usize,
    verdicts: usize,
    applied_generations: usize,
    head: Option<String>,
}

pub(super) fn run_store_stats(profile_path: PathBuf, store: PathBuf, format: OutputFormat) -> i32 {
    let profile = match load_p3_profile(&profile_path) {
        Ok(profile) => profile,
        Err(error) => return fail_read(error.code(), "P3 profile could not be loaded", 2),
    };
    let stats = match store_stats(&profile, &store) {
        Ok(stats) => stats,
        Err(error) => return fail_store_stats(&error),
    };
    let output = StoreStatsOutput {
        status: stats.status,
        command: stats.command,
        sealed: stats.sealed,
        bundle: stats.bundle,
        queue: stats.queue,
        ledger: LedgerStatsOutput {
            events: stats.events,
            verdicts: stats.verdicts,
            applied_generations: stats.applied_generations,
            head: stats.ledger_head,
        },
        store_id: P3_STORE_ID,
        profile_version: profile.version,
        current_generation: stats.current_generation,
    };
    match format {
        OutputFormat::Json => print_json(&output),
        OutputFormat::Text => print_store_stats_text(&output),
    }
    0
}

fn map_state(state: ProposalStateArg) -> ProposalState {
    match state {
        ProposalStateArg::Submitted => ProposalState::Submitted,
        ProposalStateArg::Staged => ProposalState::Staged,
        ProposalStateArg::Ruled => ProposalState::Ruled,
        ProposalStateArg::Applied => ProposalState::Applied,
        ProposalStateArg::Superseded => ProposalState::Superseded,
        ProposalStateArg::Retained => ProposalState::Retained,
        ProposalStateArg::RejectedAtIntake => ProposalState::RejectedAtIntake,
    }
}

fn fail_proposal_read(error: &ProposalReadError) -> i32 {
    let message = match error {
        ProposalReadError::NotFound => "proposal record was not found",
        ProposalReadError::Verification { .. } => "proposal records could not be verified",
    };
    fail_read(error.code(), message, 1)
}

fn fail_store_stats(error: &StoreStatsError) -> i32 {
    fail_read(error.code(), "store statistics could not be verified", 1)
}

fn fail_read(code: impl Into<String>, message: &'static str, exit: i32) -> i32 {
    emit_diagnostics(vec![VerificationDiagnostic::error(code, message)]);
    exit
}

fn print_list_text(output: &ProposalListOutput) {
    println!("status={}", output.status);
    println!("command={}", output.command);
    println!("count={}", output.proposals.len());
    for (index, proposal) in output.proposals.iter().enumerate() {
        let prefix = format!("proposal[{index}]");
        print_proposal_text(&prefix, proposal);
    }
}

fn print_status_text(output: &ProposalStatusOutput) {
    println!("status={}", output.status);
    println!("command={}", output.command);
    print_proposal_text("proposal", &output.proposal);
    print_optional("proposal.base_kind", output.proposal.base_kind.as_deref());
    print_optional(
        "proposal.base_generation",
        output.proposal.base_generation.as_deref(),
    );
    print_optional(
        "proposal.base_bundle_hash",
        output.proposal.base_bundle_hash.as_deref(),
    );
    print_optional(
        "proposal.base_manifest_hash",
        output.proposal.base_manifest_hash.as_deref(),
    );
    print_optional(
        "proposal.base_target_hash",
        output.proposal.base_target_hash.as_deref(),
    );
    print_optional(
        "proposal.policy_hash",
        output.proposal.policy_hash.as_deref(),
    );
    print_optional(
        "proposal.profile_hash",
        output.proposal.profile_hash.as_deref(),
    );
    print_optional(
        "proposal.metadata_hash",
        output.proposal.metadata_hash.as_deref(),
    );
    if let Some(reasons) = &output.proposal.evaluator_reasons {
        println!("proposal.evaluator_reason_count={}", reasons.len());
        for (index, reason) in reasons.iter().enumerate() {
            println!("proposal.evaluator_reason[{index}]={reason}");
        }
    }
}

trait ProposalTextRecord {
    fn submission_id(&self) -> Option<&str>;
    fn proposal_id(&self) -> Option<&str>;
    fn receipt_id(&self) -> Option<&str>;
    fn state(&self) -> &str;
    fn submitted_at(&self) -> Option<&str>;
    fn received_at(&self) -> Option<&str>;
    fn source_id(&self) -> Option<&str>;
    fn operation(&self) -> Option<ProposalOperation>;
    fn intent(&self) -> Option<ProposalIntent>;
    fn target_id(&self) -> Option<&str>;
    fn candidate_hash(&self) -> Option<&str>;
    fn confidence(&self) -> Option<&Confidence>;
    fn evaluator(&self) -> Option<&EvaluatorSummary>;
    fn terminal_verb(&self) -> Option<VerdictVerb>;
    fn verdict_id(&self) -> Option<&str>;
    fn retained(&self) -> bool;
}

macro_rules! impl_proposal_text_record {
    ($type:ty) => {
        impl ProposalTextRecord for $type {
            fn submission_id(&self) -> Option<&str> {
                self.submission_id.as_deref()
            }
            fn proposal_id(&self) -> Option<&str> {
                self.proposal_id.as_deref()
            }
            fn receipt_id(&self) -> Option<&str> {
                self.receipt_id.as_deref()
            }
            fn state(&self) -> &str {
                self.state
            }
            fn submitted_at(&self) -> Option<&str> {
                self.submitted_at.as_deref()
            }
            fn received_at(&self) -> Option<&str> {
                self.received_at.as_deref()
            }
            fn source_id(&self) -> Option<&str> {
                self.source_id.as_deref()
            }
            fn operation(&self) -> Option<ProposalOperation> {
                self.operation
            }
            fn intent(&self) -> Option<ProposalIntent> {
                self.intent
            }
            fn target_id(&self) -> Option<&str> {
                self.target_id.as_deref()
            }
            fn candidate_hash(&self) -> Option<&str> {
                self.candidate_hash.as_deref()
            }
            fn confidence(&self) -> Option<&Confidence> {
                self.confidence.as_ref()
            }
            fn evaluator(&self) -> Option<&EvaluatorSummary> {
                self.evaluator.as_ref()
            }
            fn terminal_verb(&self) -> Option<VerdictVerb> {
                self.terminal_verb
            }
            fn verdict_id(&self) -> Option<&str> {
                self.verdict_id.as_deref()
            }
            fn retained(&self) -> bool {
                self.retained
            }
        }
    };
}

impl_proposal_text_record!(ProposalListItem);
impl_proposal_text_record!(ProposalStatusItem);

fn print_proposal_text(prefix: &str, proposal: &impl ProposalTextRecord) {
    print_optional(&format!("{prefix}.submission_id"), proposal.submission_id());
    print_optional(&format!("{prefix}.proposal_id"), proposal.proposal_id());
    print_optional(&format!("{prefix}.receipt_id"), proposal.receipt_id());
    println!("{prefix}.state={}", proposal.state());
    print_optional(&format!("{prefix}.submitted_at"), proposal.submitted_at());
    print_optional(&format!("{prefix}.received_at"), proposal.received_at());
    print_optional(&format!("{prefix}.source_id"), proposal.source_id());
    println!(
        "{prefix}.operation={}",
        proposal
            .operation()
            .map(|value| value.as_str())
            .unwrap_or("null")
    );
    println!(
        "{prefix}.intent={}",
        proposal
            .intent()
            .map(|value| value.as_str())
            .unwrap_or("null")
    );
    print_optional(&format!("{prefix}.target_id"), proposal.target_id());
    print_optional(
        &format!("{prefix}.candidate_hash"),
        proposal.candidate_hash(),
    );
    println!(
        "{prefix}.confidence={}",
        proposal
            .confidence()
            .map(|value| serde_json::to_string(value).expect("confidence serializes"))
            .unwrap_or_else(|| "null".to_string())
    );
    if let Some(evaluator) = proposal.evaluator() {
        println!("{prefix}.evaluator.source_id={}", evaluator.source_id);
        println!(
            "{prefix}.evaluator.recommendation={}",
            evaluator.recommendation.as_str()
        );
        println!("{prefix}.evaluator.reason_count={}", evaluator.reason_count);
    } else {
        println!("{prefix}.evaluator=null");
    }
    println!(
        "{prefix}.terminal_verb={}",
        proposal
            .terminal_verb()
            .map(|value| value.as_str())
            .unwrap_or("null")
    );
    print_optional(&format!("{prefix}.verdict_id"), proposal.verdict_id());
    println!("{prefix}.retained={}", proposal.retained());
}

fn print_optional(key: &str, value: Option<&str>) {
    println!("{key}={}", value.unwrap_or("null"));
}

fn print_store_stats_text(output: &StoreStatsOutput) {
    println!("status={}", output.status);
    println!("command={}", output.command);
    println!("sealed={}", output.sealed);
    println!("documents={}", output.bundle.documents);
    for (key, value) in &output.bundle.by_provenance {
        println!("by_provenance.{key}={value}");
    }
    for (key, value) in &output.bundle.by_type {
        println!("by_type.{key}={value}");
    }
    for (key, value) in &output.bundle.by_layer {
        println!("by_layer.{key}={value}");
    }
    println!("warnings={}", output.bundle.warnings);
    println!("exclusions={}", output.bundle.exclusions);
    println!("bundle_hash={}", output.bundle.bundle_hash);
    println!("queue.submitted={}", output.queue.submitted);
    println!("queue.staged_open={}", output.queue.staged_open);
    println!(
        "queue.rejected_at_intake={}",
        output.queue.rejected_at_intake
    );
    println!("queue.open={}", output.queue.open);
    println!("queue.alarm_at={}", output.queue.alarm_at);
    println!("queue.alarm={}", output.queue.alarm);
    println!("ledger.events={}", output.ledger.events);
    println!("ledger.verdicts={}", output.ledger.verdicts);
    println!(
        "ledger.applied_generations={}",
        output.ledger.applied_generations
    );
    println!(
        "ledger.head={}",
        output.ledger.head.as_deref().unwrap_or("null")
    );
    println!("store_id={}", output.store_id);
    println!("profile_version={}", output.profile_version);
    println!("current_generation={}", output.current_generation);
}

#[derive(Serialize)]
struct VerdictListOutput {
    status: &'static str,
    command: &'static str,
    verdicts: Vec<VerdictRecord>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_verdicts_list(
    profile_path: PathBuf,
    store: PathBuf,
    verb: Option<String>,
    source: Option<String>,
    show_reasons: bool,
    show_advice: bool,
    format: OutputFormat,
) -> i32 {
    let verb = match verb {
        Some(value) => match VerdictVerb::from_str(&value) {
            Ok(verb) => Some(verb),
            Err(_) => return fail_read("verdict.verb_invalid", "verdict verb is invalid", 2),
        },
        None => None,
    };
    let profile = match load_p3_profile(&profile_path) {
        Ok(profile) => profile,
        Err(error) => return fail_read(error.code(), "P3 profile could not be loaded", 2),
    };
    let mut records = match verdict_list(&profile, &store, show_reasons, show_advice) {
        Ok(records) => records,
        Err(error) => return fail_proposal_read(&error),
    };
    records.retain(|record| {
        verb.is_none_or(|verb| record.verb == verb)
            && source
                .as_deref()
                .is_none_or(|source| record.source_id == source)
    });
    let output = VerdictListOutput {
        status: "ok",
        command: "verdicts.list",
        verdicts: records,
    };
    match format {
        OutputFormat::Json => print_json(&output),
        OutputFormat::Text => print_verdicts_text(&output),
    }
    0
}

#[derive(Serialize)]
struct RecoveryOutput {
    status: &'static str,
    command: &'static str,
    rolled_forward: bool,
    idempotent: bool,
    mode: crate::profile::RecoveryMode,
}

pub(super) fn run_recover(profile: PathBuf, store: PathBuf, format: OutputFormat) -> i32 {
    let outcome = match recover_store_human(RecoveryRequest {
        profile_path: profile,
        store_root: store,
    }) {
        Ok(outcome) => outcome,
        Err(error) => return fail_transaction(&error),
    };
    let output = RecoveryOutput {
        status: "ok",
        command: "recover",
        rolled_forward: outcome.rolled_forward,
        idempotent: outcome.idempotent,
        mode: outcome.mode,
    };
    match format {
        OutputFormat::Json => print_json(&output),
        OutputFormat::Text => {
            println!("status=ok");
            println!("command=recover");
            println!("rolled_forward={}", output.rolled_forward);
            println!("idempotent={}", output.idempotent);
            println!(
                "mode={}",
                serde_json::to_string(&output.mode)
                    .expect("recovery mode serializes")
                    .trim_matches('"')
            );
        }
    }
    0
}

pub(super) fn run_verify_store(
    profile_path: PathBuf,
    store: PathBuf,
    level: Option<u8>,
    require_corpus: bool,
) -> i32 {
    if level != Some(4) || require_corpus {
        return fail_usage("store verification requires exactly level 4");
    }
    let profile = match load_p3_profile(&profile_path) {
        Ok(profile) => profile,
        Err(error) => return fail_read(error.code(), "P3 profile could not be loaded", 2),
    };
    match verify_store_level4(&profile, &store) {
        Ok(summary) => {
            print_json(&summary);
            0
        }
        Err(error) => fail_code(error.code(), "store verification failed"),
    }
}

#[derive(Serialize)]
struct StoreIndexOutput<'a> {
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
    store_id: &'a str,
    generation: &'a str,
    manifest_hash: &'a str,
    receipt_hash: &'a str,
}

pub(super) fn run_store_index(profile_path: PathBuf, store: PathBuf, format: OutputFormat) -> i32 {
    let summary = match rebuild_store_index(StoreIndexRequest {
        profile_path,
        store_root: store,
    }) {
        Ok(summary) => summary,
        Err(error) => return fail_index(&error),
    };
    let output = StoreIndexOutput {
        status: "ok",
        command: "index",
        schema_version: INDEX_MANIFEST_SCHEMA_V2,
        documents: summary.documents,
        graph_nodes: summary.graph_nodes,
        graph_edges: summary.graph_edges,
        external_typed_links: summary.external_typed_links,
        bundle_hash: &summary.bundle_hash,
        index_config: &summary.index_config,
        replaced_invalid_current: false,
        store_id: &summary.store_id,
        generation: &summary.generation,
        manifest_hash: &summary.manifest_hash,
        receipt_hash: &summary.receipt_hash,
    };
    match format {
        OutputFormat::Json => print_json(&output),
        OutputFormat::Text => {
            println!("status: ok");
            println!("command: index");
            println!("documents: {}", output.documents);
            println!("graph-nodes: {}", output.graph_nodes);
            println!("graph-edges: {}", output.graph_edges);
            println!("external-typed-links: {}", output.external_typed_links);
            println!("bundle-hash: {}", output.bundle_hash);
            println!("index-config: {}", output.index_config);
            println!("replaced-invalid-current: false");
            println!("store-id: {}", output.store_id);
            println!("generation: {}", output.generation);
            println!("manifest-hash: {}", output.manifest_hash);
            println!("receipt-hash: {}", output.receipt_hash);
        }
    }
    0
}

#[derive(Serialize)]
struct StoreQueryOutput<'a> {
    status: &'static str,
    command: &'static str,
    top_k: usize,
    expansion: crate::profile::ExpansionMode,
    bundle_hash: &'a str,
    store_id: &'static str,
    generation: &'a str,
    manifest_hash: &'a str,
    results: &'a [crate::profile::QueryHit],
    expanded: &'a [crate::profile::ExpandedHit],
}

pub(super) fn run_store_query(
    profile_path: PathBuf,
    store: PathBuf,
    query: String,
    top_k: usize,
    expand: ExpansionArg,
    format: OutputFormat,
) -> i32 {
    let expansion = match expand {
        ExpansionArg::None => crate::profile::ExpansionMode::None,
        ExpansionArg::OneHop => crate::profile::ExpansionMode::OneHop,
    };
    let result = match query_store(StoreQueryRequest {
        profile_path,
        store_root: store,
        query,
        top_k,
        expansion,
    }) {
        Ok(result) => result,
        Err(error) => return fail_query(&error),
    };
    let output = StoreQueryOutput {
        status: "ok",
        command: "query",
        top_k: result.result.top_k,
        expansion: result.result.expansion,
        bundle_hash: &result.result.bundle_hash,
        store_id: P3_STORE_ID,
        generation: &result.generation,
        manifest_hash: &result.manifest_hash,
        results: &result.result.results,
        expanded: &result.result.expanded,
    };
    match format {
        OutputFormat::Json => print_json(&output),
        OutputFormat::Text => {
            println!("status: ok");
            println!("command: query");
            println!("top-k: {}", output.top_k);
            println!(
                "expansion: {}",
                match output.expansion {
                    crate::profile::ExpansionMode::None => "none",
                    crate::profile::ExpansionMode::OneHop => "one-hop",
                }
            );
            println!("bundle-hash: {}", output.bundle_hash);
            println!("store-id: {}", output.store_id);
            println!("generation: {}", output.generation);
            println!("manifest-hash: {}", output.manifest_hash);
            for hit in output.results {
                println!(
                    "bm25[{}]: id={} score={} title={}",
                    hit.rank,
                    hit.id,
                    hit.score,
                    serde_json::to_string(&hit.title).expect("title JSON")
                );
            }
            for hit in output.expanded {
                println!(
                    "expanded[{}]: id={} via={} direction={} relation={} title={}",
                    hit.rank,
                    hit.id,
                    hit.via_id,
                    match hit.direction {
                        crate::profile::Direction::Outgoing => "outgoing",
                        crate::profile::Direction::Incoming => "incoming",
                    },
                    hit.relation,
                    serde_json::to_string(&hit.title).expect("title JSON")
                );
            }
        }
    }
    0
}

pub(super) fn fail_usage(message: &'static str) -> i32 {
    fail_read("cli.usage", message, 2)
}

fn fail_transaction(error: &TransactionError) -> i32 {
    let exit = match error {
        TransactionError::TtyRequired
        | TransactionError::ConfirmationFailed
        | TransactionError::IntakeConfirmationFailed
        | TransactionError::InvalidRequest
        | TransactionError::ReasonInvalid
        | TransactionError::RewriteInvalid
        | TransactionError::RewriteRequired
        | TransactionError::RewriteForbidden
        | TransactionError::JudgmentRender
        | TransactionError::Write
        | TransactionError::PolicyArchive
        | TransactionError::UnsupportedPlatform
        | TransactionError::FaultInjected => 2,
        _ => 1,
    };
    fail_read(error.code(), "human transaction failed", exit)
}

fn fail_index(error: &crate::profile::IndexError) -> i32 {
    fail_code(error.code(), "derived index operation failed")
}

fn fail_code(code: &'static str, message: &'static str) -> i32 {
    fail_read(code, message, stable_error_exit(code))
}

fn stable_error_exit(code: &str) -> i32 {
    if (code.starts_with("profile.") && code != "profile.changed_during_transaction")
        || matches!(
            code,
            "cli.usage"
                | "store.read"
                | "store.write"
                | "store.path_unsafe"
                | "store.symlink"
                | "store.cross_filesystem"
                | "proposal.read"
                | "proposal.write"
                | "proposal.path_unsafe"
                | "bundle.read"
                | "index.output_invalid"
                | "index.build_failed"
                | "index.publish_unsupported"
                | "index.publish_failed"
                | "query.invalid"
                | "query.empty"
                | "query.too_many_tokens"
                | "human.tty_required"
                | "human.confirmation_failed"
                | "human.reason_invalid"
                | "human.rewrite_invalid"
                | "human.rewrite_required"
                | "human.rewrite_forbidden"
        )
    {
        2
    } else {
        1
    }
}

fn fail_query(error: &crate::profile::QueryError) -> i32 {
    match error {
        crate::profile::QueryError::Index(error) => fail_index(error),
        crate::profile::QueryError::Invalid
        | crate::profile::QueryError::Empty
        | crate::profile::QueryError::TooManyTokens => {
            fail_read(error.code(), "query input is invalid", 2)
        }
        crate::profile::QueryError::Execute => fail_read(error.code(), "query failed", 1),
    }
}

fn read_regular_input(path: &std::path::Path) -> Result<Vec<u8>, ()> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(());
    }
    std::fs::read(path).map_err(|_| ())
}

fn print_verdict_text(output: &VerdictOutput) {
    println!("status=ok");
    println!("command=verdict");
    println!("proposal_id={}", output.proposal_id);
    println!("verdict_id={}", output.verdict_id);
    println!("event_id={}", output.event_id);
    println!("verb={}", output.verb.as_str());
    println!("applied={}", output.applied);
    print_optional("generation", output.generation.as_deref());
    print_optional("bundle_hash", output.bundle_hash.as_deref());
    print_optional("manifest_hash", output.manifest_hash.as_deref());
    print_optional("target_hash", output.target_hash.as_deref());
    print_optional("judgment_hash", output.judgment_hash.as_deref());
    println!("ledger_head={}", output.ledger_head);
    print_optional(
        "reindex_receipt_hash",
        output.reindex_receipt_hash.as_deref(),
    );
    println!("idempotent={}", output.idempotent);
}

fn print_verdicts_text(output: &VerdictListOutput) {
    println!("status=ok");
    println!("command=verdicts.list");
    println!("count={}", output.verdicts.len());
    for (index, verdict) in output.verdicts.iter().enumerate() {
        println!(
            "verdict[{index}]={}",
            serde_json::to_string(verdict).expect("verdict serializes")
        );
    }
}

pub(super) struct TrustArgs {
    pub profile: PathBuf,
    pub store: PathBuf,
    pub window: usize,
    pub focus: Option<String>,
    pub output: Option<PathBuf>,
    pub format: OutputFormat,
}

/// Validate that the trust report output path does not overlap with protected
/// store directories or critical files (F01).
fn validate_trust_output_destination(output: &Path, store_root: &Path) -> Result<PathBuf, i32> {
    use std::fs;

    let file_name = output
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            fail_code(
                "trust.output_invalid",
                "trust report output path is invalid",
            )
        })?;
    let requested_parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(requested_parent).map_err(|_| {
        fail_code(
            "trust.output_invalid",
            "trust report output parent is invalid",
        )
    })?;

    // Verify parent is a writable directory (rejects symlinks, non-dirs, read-only).
    let parent_meta = fs::symlink_metadata(&parent).map_err(|_| {
        fail_code(
            "trust.output_invalid",
            "trust report output parent metadata is invalid",
        )
    })?;
    if !parent_meta.file_type().is_dir() || parent_meta.permissions().readonly() {
        return Err(fail_code(
            "trust.output_invalid",
            "trust report output parent must be a writable directory",
        ));
    }

    let store_canonical = fs::canonicalize(store_root)
        .map_err(|_| fail_code("trust.output_invalid", "trust report store root is invalid"))?;

    let destination = parent.join(file_name);

    // Reject if the destination is inside the protected store root or is
    // the store root itself.
    if destination == store_canonical || destination.starts_with(&store_canonical) {
        return Err(fail_code(
            "trust.output_invalid",
            "trust report output path must be outside the store root",
        ));
    }

    // Reject if the destination itself is a known protected file name.
    let protected_names = [
        "policy.toml",
        "CURRENT",
        "verdicts.jsonl",
        "manifest.json",
        "index.md",
    ];
    if let Some(name) = destination.file_name().and_then(|n| n.to_str()) {
        if protected_names.contains(&name) {
            return Err(fail_code(
                "trust.output_invalid",
                "trust report cannot overwrite a protected file",
            ));
        }
    }

    Ok(destination)
}

pub(super) fn run_trust(args: TrustArgs) -> i32 {
    use crate::profile::{
        inspect_store, load_p3_profile, read_ledger,
        trust::{compute_trust, render_trust_json, render_trust_text},
    };

    // F10: --focus is not yet implemented. Warn if provided.
    if args.focus.is_some() {
        eprintln!(
            "{{\"severity\":\"warn\",\"code\":\"trust.focus_unsupported\",\"message\":\"--focus is not yet implemented — showing all sources\"}}"
        );
    }

    let profile = match load_p3_profile(&args.profile) {
        Ok(p) => p,
        Err(_) => return fail_code("profile.read", "profile could not be loaded"),
    };

    let store = match inspect_store(&profile, &args.store) {
        Ok(s) => s,
        Err(e) => return fail_code(e.code(), "store could not be inspected"),
    };

    let ledger = match read_ledger(&store.paths.ledger) {
        Ok(l) => l,
        Err(e) => return fail_code(e.code(), "ledger could not be read"),
    };

    if ledger.verdicts == 0 {
        // When --output is provided with zero verdicts, honour the output path
        // with destination validation (F01/F10).
        if let Some(path) = args.output {
            if let Err(exit) = validate_trust_output_destination(&path, &store.paths.root) {
                return exit;
            }
            let json = serde_json::json!({
                "status": "ok",
                "command": "trust",
                "store_id": store.policy.policy.store_id,
                "window_size": args.window,
                "total_verdicts": 0,
                "sources": [],
            });
            let json_str =
                serde_json::to_string_pretty(&json).expect("JSON serialization is infallible");
            if std::fs::write(&path, json_str.as_bytes()).is_err() {
                return fail_code("trust.write", "trust report could not be written");
            }
            return 0;
        }

        let json = serde_json::json!({
            "status": "ok",
            "command": "trust",
            "store_id": store.policy.policy.store_id,
            "window_size": args.window,
            "total_verdicts": 0,
            "sources": [],
        });
        match args.format {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string(&json).unwrap());
            }
            OutputFormat::Text => {
                println!("No verdicts in ledger.");
            }
        }
        return 0;
    }

    let report = compute_trust(&ledger, &store.policy.policy, args.window);

    if let Some(path) = args.output {
        if let Err(exit) = validate_trust_output_destination(&path, &store.paths.root) {
            return exit;
        }
        let json_str = serde_json::to_string_pretty(&render_trust_json(&report))
            .expect("JSON serialization is infallible");
        if std::fs::write(&path, json_str.as_bytes()).is_err() {
            return fail_code("trust.write", "trust report could not be written");
        }
        return 0;
    }

    match args.format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string(&render_trust_json(&report)).unwrap()
            );
        }
        OutputFormat::Text => {
            print!("{}", render_trust_text(&report));
        }
    }
    0
}

pub(super) fn run_audit_sample(store: PathBuf, sample_size: usize, format: OutputFormat) -> i32 {
    // Stub: audit --sample is deferred until trust module ships.
    // Return a non-zero exit so automation cannot interpret "nothing ran" as success (F09).
    let _ = (store, sample_size, format);
    eprintln!(
        "{{\"severity\":\"error\",\"code\":\"audit.sample\",\"message\":\"audit --sample not yet implemented — this command is not operational\"}}"
    );
    1
}
