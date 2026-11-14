#[allow(dead_code)]
mod support;

use sovmem::profile::{submit_proposal, ProposalIntent, ProposalOperation, ProposalSubmission};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use support::p3::P3CliFixture as P3Fixture;

#[derive(Clone, Copy)]
enum ScriptFlavor {
    Bsd,
    UtilLinux,
}

fn posix_shell_quote(argument: &str) -> String {
    format!("'{}'", argument.replace('\'', "'\"'\"'"))
}

fn run_intake_seal_at_real_tty(fixture: &P3Fixture, submission_id: &str) -> Output {
    let command = [
        env!("CARGO_BIN_EXE_sovmem").to_owned(),
        "intake".to_owned(),
        "seal".to_owned(),
        submission_id.to_owned(),
        "--profile".to_owned(),
        fixture.profile_arg().to_owned(),
        "--store".to_owned(),
        fixture.store_arg().to_owned(),
        "--format".to_owned(),
        "json".to_owned(),
    ];
    let flavor = if cfg!(target_os = "linux") {
        ScriptFlavor::UtilLinux
    } else {
        ScriptFlavor::Bsd
    };
    let args = match flavor {
        ScriptFlavor::Bsd => ["-q".to_owned(), "/dev/null".to_owned()]
            .into_iter()
            .chain(command.iter().cloned())
            .collect(),
        ScriptFlavor::UtilLinux => vec![
            "-q".to_owned(),
            "-e".to_owned(),
            "-c".to_owned(),
            command
                .iter()
                .map(|argument| posix_shell_quote(argument))
                .collect::<Vec<_>>()
                .join(" "),
            "/dev/null".to_owned(),
        ],
    };
    let mut child = Command::new("/usr/bin/script")
        .args(args)
        .env("SHELL", "/bin/sh")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn intake seal with a real controlling terminal");
    let mut input = child.stdin.take().expect("script stdin");
    let confirmation = format!("SEAL INTAKE {submission_id}\n").into_bytes();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        input
            .write_all(&confirmation)
            .expect("write exact intake confirmation");
        release_receiver
            .recv()
            .expect("hold script input open until the command exits");
    });
    let output = child.wait_with_output().expect("wait for intake seal");
    release_sender.send(()).expect("release script input");
    writer.join().expect("join script input writer");
    output
}

fn script_json_stdout(output: &Output) -> serde_json::Value {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .find_map(|line| serde_json::from_str(line).ok())
        .expect("scripted terminal output contains one JSON object")
}

fn assert_absent(output: &Output, sentinels: &[&str]) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for sentinel in sentinels {
        assert!(!stdout.contains(sentinel), "stdout leaked {sentinel}");
        assert!(!stderr.contains(sentinel), "stderr leaked {sentinel}");
    }
}

fn diagnostic_code(output: &Output) -> String {
    let line = String::from_utf8_lossy(&output.stderr);
    serde_json::from_str::<serde_json::Value>(line.lines().next().unwrap()).unwrap()["code"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn queue_submitted_list_status_and_stats_redact_private_transport_evidence() {
    let fixture = P3Fixture::new("privacy-task7-queue-projections");
    let private_body = b"PRIVATE_QUEUE_BODY_7301\n";
    let candidate = fixture.input_file("PRIVATE_QUEUE_CANDIDATE_PATH_7302", private_body);
    let candidate_hash = blake3::hash(private_body).to_hex().to_string();
    let evaluator = fixture.input_file(
        "PRIVATE_QUEUE_EVALUATOR_PATH_7303",
        &serde_json::to_vec(&sovmem::profile::EvaluatorBlock {
            protocol: "adversarial-bug-finding-v1".to_owned(),
            source_id: "src:agent:queue-privacy".to_owned(),
            evaluated_at: "2026-07-22T01:00:00.000000000Z".to_owned(),
            candidate_hash,
            recommendation: "review".to_owned(),
            reasons: vec!["PRIVATE_QUEUE_EVALUATOR_REASON_7304".to_owned()],
        })
        .unwrap(),
    );
    let submitted = submit_proposal(ProposalSubmission {
        profile_path: fixture.profile_path.clone(),
        store_root: fixture.store_root.clone(),
        source_id: "src:agent:queue-privacy".to_owned(),
        operation: ProposalOperation::Create,
        intent: ProposalIntent::Accept,
        target_id: "ssa-0/04-Research/PRIVATE_QUEUE_TARGET_7305/README.md".to_owned(),
        target_path: "04-Research/PRIVATE_QUEUE_TARGET_7305/README.md".to_owned(),
        candidate_path: candidate,
        confidence: None,
        evaluator_path: Some(evaluator),
    })
    .expect("submit private queue fixture");

    let list = fixture
        .command()
        .args([
            "proposals",
            "list",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(list.status.code(), Some(0));
    assert_eq!(
        fixture.parse_single_json_stdout(&list)["proposals"][0]["state"],
        "submitted"
    );

    let status = fixture
        .command()
        .args([
            "proposals",
            "status",
            &submitted.submission_id,
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(0));
    assert_eq!(
        fixture.parse_single_json_stdout(&status)["proposal"]["state"],
        "submitted"
    );

    let stats = fixture
        .command()
        .args([
            "stats",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(stats.status.code(), Some(0));
    assert_eq!(
        fixture.parse_single_json_stdout(&stats)["queue"]["submitted"],
        1
    );
    assert!(
        fixture.parse_single_json_stdout(&stats)["queue"]
            .get("oldest_submitted_at")
            .is_none(),
        "queue JSON must not expose operational timestamps"
    );

    let stats_text = fixture
        .command()
        .args([
            "stats",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--format",
            "text",
        ])
        .output()
        .unwrap();
    assert_eq!(stats_text.status.code(), Some(0));
    assert_absent(&stats_text, &["oldest_submitted_at"]);

    for output in [&list, &status, &stats, &stats_text] {
        assert_absent(
            output,
            &[
                "PRIVATE_QUEUE_BODY_7301",
                "PRIVATE_QUEUE_CANDIDATE_PATH_7302",
                "PRIVATE_QUEUE_EVALUATOR_PATH_7303",
                "PRIVATE_QUEUE_EVALUATOR_REASON_7304",
                fixture.profile_arg(),
                fixture.store_arg(),
            ],
        );
    }
}

#[test]
fn intake_seal_without_tty_redacts_submission_and_candidate_material() {
    let fixture = P3Fixture::new("privacy-intake-seal");
    let private_body = b"PRIVATE_INTAKE_CANDIDATE_BODY_611\n";
    let candidate = fixture.input_file("PRIVATE_INTAKE_CANDIDATE_PATH_612", private_body);
    let proposed = fixture
        .command()
        .args([
            "propose",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--source",
            "src:agent:private-intake-source-613",
            "--operation",
            "create",
            "--intent",
            "accept",
            "--target-id",
            "ssa-0/04-Research/CLI/README.md",
            "--target-path",
            "04-Research/CLI/README.md",
            "--candidate",
            candidate.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(proposed.status.code(), Some(0));
    let submission_id = fixture.parse_single_json_stdout(&proposed)["submission_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let output = fixture
        .command()
        .args([
            "intake",
            "seal",
            &submission_id,
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(diagnostic_code(&output), "human.tty_required");
    assert_absent(
        &output,
        &[
            "PRIVATE_INTAKE_CANDIDATE_BODY_611",
            "PRIVATE_INTAKE_CANDIDATE_PATH_612",
            "private-intake-source-613",
            fixture.profile_arg(),
            fixture.store_arg(),
        ],
    );
}

fn intake_attack_privacy_families() -> Vec<&'static str> {
    let fixture = P3Fixture::new("privacy-intake-attack-sentinels");
    let candidate_bytes = b"MATRIX_PRIVATE_CANDIDATE_9201\n";
    let candidate = fixture.input_file(
        "MATRIX_PRIVATE_SAFE_CLONE_ROOT_9209-MATRIX_PRIVATE_PATH_9206",
        candidate_bytes,
    );
    let evaluator = fixture.input_file(
        "MATRIX_PRIVATE_TEMP_ROOT_9208-evaluator",
        &serde_json::to_vec(&sovmem::profile::EvaluatorBlock {
            protocol: "adversarial-bug-finding-v1".to_owned(),
            source_id: "src:human:matrix-private-evaluator".to_owned(),
            evaluated_at: "2026-07-22T04:10:00.000000000Z".to_owned(),
            candidate_hash: blake3::hash(candidate_bytes).to_hex().to_string(),
            recommendation: "MATRIX_PRIVATE_ADVICE_9203".to_owned(),
            reasons: vec!["MATRIX_PRIVATE_REASON_9202".to_owned()],
        })
        .unwrap(),
    );
    let output = fixture
        .command()
        .env("USER", "MATRIX_PRIVATE_OS_IDENTITY_9207")
        .env("LOGNAME", "MATRIX_PRIVATE_OS_IDENTITY_9207")
        .args([
            "propose",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--source",
            "src:agent:MATRIX_PRIVATE_SOURCE_9204",
            "--operation",
            "create",
            "--intent",
            "accept",
            "--target-id",
            "ssa-0/MATRIX_PRIVATE_TARGET_9205",
            "--target-path",
            "../MATRIX_PRIVATE_TARGET_9205.md",
            "--candidate",
            candidate.to_str().unwrap(),
            "--evaluator",
            evaluator.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(diagnostic_code(&output), "proposal.target_unsafe");
    assert_absent(
        &output,
        &[
            "MATRIX_PRIVATE_CANDIDATE_9201",
            "MATRIX_PRIVATE_REASON_9202",
            "MATRIX_PRIVATE_ADVICE_9203",
            "MATRIX_PRIVATE_SOURCE_9204",
            "MATRIX_PRIVATE_TARGET_9205",
            "MATRIX_PRIVATE_PATH_9206",
            "MATRIX_PRIVATE_OS_IDENTITY_9207",
            "MATRIX_PRIVATE_TEMP_ROOT_9208",
            "MATRIX_PRIVATE_SAFE_CLONE_ROOT_9209",
            fixture.profile_arg(),
            fixture.store_arg(),
            candidate.to_str().unwrap(),
            evaluator.to_str().unwrap(),
        ],
    );
    vec![
        "candidate",
        "reason",
        "advice",
        "source",
        "target",
        "path",
        "os_identity",
        "temporary_root",
        "safe_clone_root",
    ]
}

#[test]
fn intake_attack_diagnostics_cover_every_private_sentinel_family() {
    assert_eq!(
        intake_attack_privacy_families(),
        [
            "candidate",
            "reason",
            "advice",
            "source",
            "target",
            "path",
            "os_identity",
            "temporary_root",
            "safe_clone_root",
        ]
    );
}

#[test]
fn verdict_usage_tty_and_recovery_failures_never_echo_private_material() {
    let fixture = P3Fixture::new("privacy-verdict");
    let submitted = submit_proposal(ProposalSubmission {
        profile_path: fixture.profile_path.clone(),
        store_root: fixture.store_root.clone(),
        source_id: "src:agent:private-source-excerpt-601".to_owned(),
        operation: ProposalOperation::Create,
        intent: ProposalIntent::Accept,
        target_id: "ssa-0/04-Research/CLI/README.md".to_owned(),
        target_path: "04-Research/CLI/README.md".to_owned(),
        candidate_path: Path::new(fixture.candidate_arg()).to_owned(),
        confidence: None,
        evaluator_path: None,
    })
    .expect("stage privacy proposal");
    let sealed = run_intake_seal_at_real_tty(&fixture, &submitted.submission_id);
    assert_eq!(
        sealed.status.code(),
        Some(0),
        "seal privacy proposal through supported human boundary: stdout={} stderr={}",
        String::from_utf8_lossy(&sealed.stdout),
        String::from_utf8_lossy(&sealed.stderr)
    );
    let proposal_id = script_json_stdout(&sealed)["proposal_id"]
        .as_str()
        .expect("human seal returns its protected proposal ID")
        .to_owned();
    let reason = "PRIVATE_REASON_602";
    let proposal_root = fixture
        .store_root
        .join("intake/proposals")
        .join(&proposal_id[4..6])
        .join(&proposal_id);
    let private_reason_path = fixture
        .store_root
        .join("PRIVATE_REASON_FILE_605_DOES_NOT_EXIST");
    let reason_read = fixture
        .command()
        .args([
            "verdict",
            &proposal_id,
            "ACCEPT",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--reason-file",
            private_reason_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(reason_read.status.code(), Some(2));
    assert_absent(&reason_read, &["PRIVATE_REASON_FILE_605_DOES_NOT_EXIST"]);
    let private_rewrite_path = fixture
        .store_root
        .join("PRIVATE_REWRITE_FILE_606_DOES_NOT_EXIST");
    let rewrite_read = fixture
        .command()
        .args([
            "verdict",
            &proposal_id,
            "REWRITE",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--reason",
            reason,
            "--rewrite-file",
            private_rewrite_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(rewrite_read.status.code(), Some(2));
    assert_absent(
        &rewrite_read,
        &["PRIVATE_REWRITE_FILE_606_DOES_NOT_EXIST", reason],
    );
    std::fs::write(
        proposal_root.join("candidate.md"),
        b"PRIVATE_SOURCE_EXCERPT_603\nPRIVATE_REWRITE_BODY_604",
    )
    .unwrap();
    let verdict = fixture
        .command()
        .args([
            "verdict",
            &proposal_id,
            "ACCEPT",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--reason",
            reason,
        ])
        .output()
        .unwrap();
    assert_eq!(verdict.status.code(), Some(1));
    assert!(verdict.stdout.is_empty());
    assert_absent(
        &verdict,
        &[
            reason,
            "PRIVATE_REWRITE_BODY_604",
            "PRIVATE_SOURCE_EXCERPT_603",
            "private-source-excerpt-601",
            fixture.profile_arg(),
            fixture.store_arg(),
            fixture.candidate_arg(),
            "No such file or directory",
            "Operation not permitted",
        ],
    );

    let recover = fixture
        .command()
        .args([
            "recover",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(recover.status.code(), Some(2));
    assert_absent(
        &recover,
        &[
            fixture.profile_arg(),
            fixture.store_arg(),
            "No such device or address",
        ],
    );
    for byte in ['a', 'b'] {
        let journal = fixture
            .store_root
            .join("transactions")
            .join(format!("txn-{}", byte.to_string().repeat(64)));
        std::fs::create_dir(&journal).unwrap();
        std::fs::write(
            journal.join("plan.json"),
            b"PRIVATE_RECOVERY_OWNER_607 PRIVATE_RECOVERY_PLAN_608",
        )
        .unwrap();
    }
    let ambiguous = fixture
        .command()
        .args([
            "recover",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(ambiguous.status.code(), Some(1));
    assert_absent(
        &ambiguous,
        &["PRIVATE_RECOVERY_OWNER_607", "PRIVATE_RECOVERY_PLAN_608"],
    );
}

#[test]
fn query_and_mode_failures_never_echo_query_roots_process_or_host_sentinels() {
    let fixture = P3Fixture::new("privacy-query");
    fixture.install_fixture_chain_with_one_applied_generation();
    let rebuilt = fixture
        .command()
        .args([
            "index",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(rebuilt.status.code(), Some(0));
    let receipt = std::fs::read_dir(fixture.store_root.join("evidence/reindex"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&receipt, b"PRIVATE_RECEIPT_706").unwrap();
    let query = "PRIVATE_QUERY_701 PRIVATE_HOSTNAME_702";
    let output = fixture
        .command()
        .args([
            "query",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--query",
            query,
        ])
        .env("USER", "PRIVATE_USERNAME_704")
        .env("HOSTNAME", "PRIVATE_HOSTNAME_705")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_absent(
        &output,
        &[
            "PRIVATE_QUERY_701",
            "PRIVATE_HOSTNAME_702",
            "PRIVATE_INDEX_ROOT_703",
            "PRIVATE_RECEIPT_706",
            "PRIVATE_USERNAME_704",
            "PRIVATE_HOSTNAME_705",
            fixture.profile_arg(),
            fixture.store_arg(),
            &std::process::id().to_string(),
        ],
    );
}

#[test]
fn level4_index_publication_and_race_errors_redact_every_private_sentinel() {
    let level4 = P3Fixture::new("privacy-level4");
    level4.install_fixture_chain_with_one_applied_generation();
    std::fs::write(
        level4.ledger_path(),
        b"PRIVATE_LEVEL4_LEDGER_801\nPRIVATE_OS_ERROR_802",
    )
    .unwrap();
    let verified = level4
        .command()
        .args([
            "verify",
            "--profile",
            level4.profile_arg(),
            "--store",
            level4.store_arg(),
            "--level",
            "4",
        ])
        .output()
        .unwrap();
    assert_eq!(verified.status.code(), Some(1));
    assert_absent(
        &verified,
        &["PRIVATE_LEVEL4_LEDGER_801", "PRIVATE_OS_ERROR_802"],
    );

    let publication = P3Fixture::new("privacy-index-publication");
    publication.install_fixture_chain_with_one_applied_generation();
    let first = publication
        .command()
        .args([
            "index",
            "--profile",
            publication.profile_arg(),
            "--store",
            publication.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(0));
    let current = publication
        .store_root
        .join("derived/indexes/gen-0000000000000001/CURRENT");
    std::fs::remove_file(&current).unwrap();
    std::fs::create_dir(&current).unwrap();
    std::fs::write(current.join("PRIVATE_TEMP_803"), b"PRIVATE_CRASH_804").unwrap();
    let failed = publication
        .command()
        .args([
            "index",
            "--profile",
            publication.profile_arg(),
            "--store",
            publication.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    assert_absent(&failed, &["PRIVATE_TEMP_803", "PRIVATE_CRASH_804"]);

    let concurrent = P3Fixture::new("privacy-index-concurrent-failure");
    concurrent.install_fixture_chain_with_one_applied_generation();
    let receipt = std::fs::read_dir(concurrent.store_root.join("evidence/reindex"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&receipt, b"PRIVATE_CONCURRENT_RECEIPT_805").unwrap();
    let spawn = || {
        let mut command = concurrent.command();
        command.args([
            "index",
            "--profile",
            concurrent.profile_arg(),
            "--store",
            concurrent.store_arg(),
        ]);
        command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };
    let first = spawn();
    let second = spawn();
    for output in [
        first.wait_with_output().unwrap(),
        second.wait_with_output().unwrap(),
    ] {
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(diagnostic_code(&output), "ledger.reindex_receipt_mismatch");
        assert_absent(&output, &["PRIVATE_CONCURRENT_RECEIPT_805"]);
    }
}

#[test]
fn reason_and_advice_are_exposed_only_by_their_explicit_success_flags() {
    let fixture = P3Fixture::new("privacy-show-flags");
    fixture.install_fixture_chain_with_one_applied_generation();
    let hidden = fixture
        .command()
        .args([
            "verdicts",
            "list",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(hidden.status.code(), Some(0));
    assert_absent(
        &hidden,
        &["fixture retained reject", "fixture applied accept"],
    );

    let shown = fixture
        .command()
        .args([
            "verdicts",
            "list",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--show-reasons",
            "--show-advice",
        ])
        .output()
        .unwrap();
    assert_eq!(shown.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&shown.stdout);
    assert!(stdout.contains("fixture retained reject"));
    assert!(stdout.contains("fixture applied accept"));
    assert!(!String::from_utf8_lossy(&shown.stderr).contains("fixture retained reject"));
    assert_absent(
        &shown,
        &[
            fixture.profile_arg(),
            fixture.store_arg(),
            fixture.candidate_arg(),
        ],
    );
}
