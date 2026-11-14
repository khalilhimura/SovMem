#[allow(dead_code)]
mod support;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use support::p3::P3CliFixture as P3Fixture;

fn durable_hashes(root: &Path) -> Vec<(String, String)> {
    fn visit(root: &Path, path: &Path, hashes: &mut Vec<(String, String)>) {
        let mut entries = std::fs::read_dir(path)
            .expect("read durable fixture directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read durable fixture entries");
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let file_type = entry.file_type().expect("read durable fixture file type");
            if file_type.is_dir() {
                visit(root, &path, hashes);
            } else if file_type.is_file() {
                hashes.push((
                    path.strip_prefix(root)
                        .expect("durable fixture path is beneath root")
                        .to_string_lossy()
                        .into_owned(),
                    blake3::hash(&std::fs::read(&path).expect("read durable fixture file"))
                        .to_hex()
                        .to_string(),
                ));
            }
        }
    }

    let mut hashes = Vec::new();
    visit(root, root, &mut hashes);
    hashes
}

fn protected_durable_state(root: &Path) -> Vec<(String, String)> {
    fn visit(root: &Path, path: &Path, state: &mut Vec<(String, String)>) {
        let relative = path
            .strip_prefix(root)
            .expect("protected fixture path is beneath root")
            .to_string_lossy()
            .into_owned();
        let metadata = std::fs::symlink_metadata(path).expect("read protected fixture metadata");
        if metadata.file_type().is_symlink() {
            state.push((relative, "symlink".to_owned()));
            return;
        }
        if metadata.is_file() {
            state.push((
                relative,
                blake3::hash(&std::fs::read(path).expect("read protected fixture file"))
                    .to_hex()
                    .to_string(),
            ));
            return;
        }
        state.push((relative, "directory".to_owned()));
        let mut entries = std::fs::read_dir(path)
            .expect("read protected fixture directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect protected fixture directory");
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            visit(root, &entry.path(), state);
        }
    }

    let mut state = Vec::new();
    for protected in [
        "intake",
        "locks",
        "ledger",
        "canon",
        "policies",
        "evidence",
        "derived",
        "transactions",
        "policy.toml",
    ] {
        visit(root, &root.join(protected), &mut state);
    }
    state
}

struct CliPoisoningCase {
    family: &'static str,
    fixture_name: &'static str,
    first_stage: &'static str,
    exit: i32,
    forbidden_diagnostics: &'static [&'static str],
    run: fn(&P3Fixture) -> Output,
}

fn assert_cli_poisoning_case(
    fixture: &P3Fixture,
    before: Vec<(String, String)>,
    output: Output,
    family: &str,
    exit: i32,
    first_stage: &str,
    forbidden_diagnostics: &[&str],
) {
    assert_eq!(output.status.code(), Some(exit), "{family}");
    assert_eq!(diagnostics(&output)[0]["code"], first_stage, "{family}");
    assert!(output.stdout.is_empty(), "{family} emitted failure stdout");
    assert_eq!(
        durable_hashes(&fixture.store_root),
        before,
        "{family} mutated store"
    );
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for sentinel in forbidden_diagnostics {
        assert!(
            !diagnostics.contains(sentinel),
            "{family} leaked {sentinel}"
        );
    }
    fixture.assert_machine_streams_redacted(&output);
}

#[test]
fn real_tty_regression_contracts_are_not_platform_gated() {
    let source = include_str!("p3_cli.rs");
    for gated_test in [
        "#[test]\n#[cfg(target_os = \"macos\")]",
        "#[cfg(target_os = \"macos\")]\n#[test]",
    ] {
        assert!(
            !source.contains(gated_test),
            "real-TTY regression contracts must execute on macOS and Linux"
        );
    }
}

#[test]
fn poisoning_matrix_cli_rows_preserve_exit_class_durable_state_and_redaction() {
    let fixture = P3Fixture::new("cli-poisoning-matrix-no-tty");
    let proposal_id = stage_cli_proposal(&fixture, "04-Research/CLI/README.md");
    let before = durable_hashes(&fixture.store_root);
    let output = fixture
        .command()
        .args([
            "verdict",
            &proposal_id,
            "REJECT",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--reason",
            "MATRIX_PRIVATE_REASON_901",
        ])
        .output()
        .unwrap();
    assert_cli_poisoning_case(
        &fixture,
        before,
        output,
        "missing_tty_verdict_authority_boundary",
        2,
        "human.tty_required",
        &["MATRIX_PRIVATE_REASON_901"],
    );

    let cases = [
        CliPoisoningCase {
            family: "unsafe_platform_prefixed_path",
            fixture_name: "cli-poisoning-matrix-unsafe-path",
            first_stage: "proposal.target_unsafe",
            exit: 2,
            forbidden_diagnostics: &["MATRIX_PRIVATE_TARGET_902"],
            run: |fixture| {
                fixture
                    .command()
                    .args([
                        "propose",
                        "--profile",
                        fixture.profile_arg(),
                        "--store",
                        fixture.store_arg(),
                        "--source",
                        "src:agent:fixture",
                        "--operation",
                        "create",
                        "--intent",
                        "accept",
                        "--target-id",
                        "ssa-0/04-Research/MATRIX_PRIVATE_TARGET_902.md",
                        "--target-path",
                        "C:/MATRIX_PRIVATE_TARGET_902.md",
                        "--candidate",
                        fixture.candidate_arg(),
                    ])
                    .output()
                    .unwrap()
            },
        },
        CliPoisoningCase {
            family: "protected_judgment_namespace",
            fixture_name: "cli-poisoning-matrix-protected-judgment",
            first_stage: "proposal.target_unsafe",
            exit: 2,
            forbidden_diagnostics: &["MATRIX_PRIVATE_JUDGMENT_902"],
            run: |fixture| {
                fixture
                    .command()
                    .args([
                        "propose",
                        "--profile",
                        fixture.profile_arg(),
                        "--store",
                        fixture.store_arg(),
                        "--source",
                        "src:agent:fixture",
                        "--operation",
                        "create",
                        "--intent",
                        "accept",
                        "--target-id",
                        "ssa-0/judgments/MATRIX_PRIVATE_JUDGMENT_902.md",
                        "--target-path",
                        "judgments/MATRIX_PRIVATE_JUDGMENT_902.md",
                        "--candidate",
                        fixture.candidate_arg(),
                    ])
                    .output()
                    .unwrap()
            },
        },
        CliPoisoningCase {
            family: "unsealed_ledger_level4_first_failure",
            fixture_name: "cli-poisoning-matrix-level4",
            first_stage: "ledger.genesis_missing",
            exit: 1,
            forbidden_diagnostics: &[],
            run: |fixture| {
                fixture
                    .command()
                    .args([
                        "verify",
                        "--profile",
                        fixture.profile_arg(),
                        "--store",
                        fixture.store_arg(),
                        "--level",
                        "4",
                    ])
                    .output()
                    .unwrap()
            },
        },
    ];

    for case in cases {
        let fixture = P3Fixture::new(case.fixture_name);
        let before = durable_hashes(&fixture.store_root);
        let output = (case.run)(&fixture);
        assert_cli_poisoning_case(
            &fixture,
            before,
            output,
            case.family,
            case.exit,
            case.first_stage,
            case.forbidden_diagnostics,
        );
    }
}

fn propose(
    fixture: &P3Fixture,
    source: &str,
    target_path: &str,
    candidate: &Path,
    evaluator: Option<&Path>,
    format: &str,
) -> Output {
    let target_id = format!("ssa-0/{target_path}");
    let mut command = fixture.command();
    command.args([
        "propose",
        "--profile",
        fixture.profile_arg(),
        "--store",
        fixture.store_arg(),
        "--source",
        source,
        "--operation",
        "create",
        "--intent",
        "accept",
        "--target-id",
        &target_id,
        "--target-path",
        target_path,
        "--candidate",
        candidate.to_str().expect("UTF-8 candidate path"),
        "--format",
        format,
    ]);
    if let Some(evaluator) = evaluator {
        command.arg("--evaluator").arg(evaluator);
    }
    command.output().expect("run proposal command")
}

fn stage_cli_proposal(fixture: &P3Fixture, target_path: &str) -> String {
    let submitted = propose(
        fixture,
        "src:agent:fixture",
        target_path,
        Path::new(fixture.candidate_arg()),
        None,
        "json",
    );
    assert_eq!(
        submitted.status.code(),
        Some(0),
        "submit proposal before seal: {}",
        String::from_utf8_lossy(&submitted.stderr)
    );
    let submission_id = submission_id(fixture, &submitted);
    let sealed = run_intake_seal_at_real_tty(
        fixture,
        &submission_id,
        &format!("SEAL INTAKE {submission_id}\n"),
        "json",
    );
    assert_eq!(
        sealed.status.code(),
        Some(0),
        "seal proposal through supported human boundary: stdout={} stderr={}",
        String::from_utf8_lossy(&sealed.stdout),
        String::from_utf8_lossy(&sealed.stderr)
    );
    script_json_stdout(&sealed)["proposal_id"]
        .as_str()
        .expect("human seal returns its protected proposal ID")
        .to_owned()
}

fn stage_cli_rewrite_proposal(
    fixture: &P3Fixture,
    source: &str,
    target_id: &str,
    target_path: &str,
    candidate: &Path,
) -> String {
    let submitted = fixture
        .command()
        .args([
            "propose",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--source",
            source,
            "--operation",
            "replace",
            "--intent",
            "rewrite",
            "--target-id",
            target_id,
            "--target-path",
            target_path,
            "--candidate",
            candidate.to_str().expect("UTF-8 candidate path"),
            "--format",
            "json",
        ])
        .output()
        .expect("stage rewrite proposal through CLI");
    assert_eq!(
        submitted.status.code(),
        Some(0),
        "submit rewrite proposal: {}",
        String::from_utf8_lossy(&submitted.stderr)
    );
    let submission_id = submission_id(fixture, &submitted);
    let sealed = run_intake_seal_at_real_tty(
        fixture,
        &submission_id,
        &format!("SEAL INTAKE {submission_id}\n"),
        "json",
    );
    assert_eq!(
        sealed.status.code(),
        Some(0),
        "seal rewrite proposal through supported human boundary: stdout={} stderr={}",
        String::from_utf8_lossy(&sealed.stdout),
        String::from_utf8_lossy(&sealed.stderr)
    );
    script_json_stdout(&sealed)["proposal_id"]
        .as_str()
        .expect("human seal returns its protected rewrite proposal ID")
        .to_owned()
}

fn diagnostics(output: &Output) -> Vec<serde_json::Value> {
    String::from_utf8(output.stderr.clone())
        .expect("stderr is UTF-8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("diagnostic JSONL"))
        .collect()
}

fn submission_id(fixture: &P3Fixture, output: &Output) -> String {
    fixture.parse_single_json_stdout(output)["submission_id"]
        .as_str()
        .expect("CLI proposal returns its submission ID")
        .to_owned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScriptFlavor {
    Bsd,
    UtilLinux,
}

#[derive(Debug, PartialEq, Eq)]
struct ScriptInvocation {
    program: &'static str,
    args: Vec<String>,
}

fn script_invocation(flavor: ScriptFlavor, command: &[String]) -> ScriptInvocation {
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
    ScriptInvocation {
        program: "/usr/bin/script",
        args,
    }
}

fn posix_shell_quote(argument: &str) -> String {
    format!("'{}'", argument.replace('\'', "'\"'\"'"))
}

#[test]
fn real_tty_script_argv_preserves_the_public_cli_contract_on_bsd_and_util_linux() {
    let command = [
        "/tmp/sov mem".to_owned(),
        "intake".to_owned(),
        "quote'case".to_owned(),
        "$literal".to_owned(),
        String::new(),
    ];
    assert_eq!(
        script_invocation(ScriptFlavor::Bsd, &command),
        ScriptInvocation {
            program: "/usr/bin/script",
            args: ["-q", "/dev/null"]
                .into_iter()
                .map(str::to_owned)
                .chain(command.iter().cloned())
                .collect(),
        }
    );
    assert_eq!(
        script_invocation(ScriptFlavor::UtilLinux, &command),
        ScriptInvocation {
            program: "/usr/bin/script",
            args: vec![
                "-q".to_owned(),
                "-e".to_owned(),
                "-c".to_owned(),
                "'/tmp/sov mem' 'intake' 'quote'\"'\"'case' '$literal' ''".to_owned(),
                "/dev/null".to_owned(),
            ],
        }
    );
}

fn run_intake_seal_at_real_tty(
    fixture: &P3Fixture,
    submission_id: &str,
    confirmation: &str,
    format: &str,
) -> Output {
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
        format.to_owned(),
    ];
    let flavor = if cfg!(target_os = "linux") {
        ScriptFlavor::UtilLinux
    } else {
        ScriptFlavor::Bsd
    };
    let invocation = script_invocation(flavor, &command);
    let mut child = Command::new(invocation.program)
        .args(invocation.args)
        .env("SHELL", "/bin/sh")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn intake seal with a real controlling terminal");
    let mut input = child.stdin.take().expect("script stdin");
    let confirmation = confirmation.as_bytes().to_vec();
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

fn assert_store_slot_pointer(pointer: &str) {
    let token = pointer
        .strip_suffix('\n')
        .and_then(|value| value.strip_prefix("slot-"))
        .expect("store index pointer uses an immutable slot token");
    assert_eq!(token.len(), 32);
    assert!(token
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
}

#[test]
fn verdict_without_a_real_controlling_terminal_fails_before_mutation() {
    let fixture = P3Fixture::new("cli-no-tty");
    let proposal_id = stage_cli_proposal(&fixture, "04-Research/CLI/README.md");
    let before = durable_hashes(&fixture.store_root);
    let output = fixture
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
            "Reviewed fixture reason.",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(diagnostics(&output)[0]["code"], "human.tty_required");
    assert_eq!(durable_hashes(&fixture.store_root), before);
}

#[test]
fn propose_derives_authoritative_fields_and_never_echoes_input_paths() {
    let fixture = P3Fixture::new("cli-propose");
    let output = fixture
        .command()
        .args([
            "propose",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--source",
            "src:agent:fixture",
            "--operation",
            "create",
            "--intent",
            "accept",
            "--target-id",
            "ssa-0/04-Research/CLI/README.md",
            "--target-path",
            "04-Research/CLI/README.md",
            "--candidate",
            fixture.candidate_arg(),
            "--confidence",
            "0.750000",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = fixture.parse_single_json_stdout(&output);
    assert_eq!(value["command"], "propose");
    assert_eq!(value["state"], "submitted");
    let submission_id = value["submission_id"].as_str().unwrap();
    assert!(submission_id.starts_with("sub-"));
    assert!(value.get("proposal_id").is_none());
    assert!(
        std::fs::read(fixture.store_root.join("intake/events.jsonl"))
            .unwrap()
            .is_empty()
    );
    let inbox = fixture.store_root.join("staging/inbox").join(submission_id);
    let mut names = std::fs::read_dir(inbox)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["candidate.md", "submission.meta.json"]);
    fixture.assert_machine_streams_redacted(&output);
}

#[test]
fn intake_propose_is_transport_only_and_preserves_every_protected_root() {
    let fixture = P3Fixture::new("cli-task9-propose-transport-only");
    let before = protected_durable_state(&fixture.store_root);
    let output = fixture
        .command()
        .args([
            "propose",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--source",
            "src:agent:task9-proposer",
            "--operation",
            "create",
            "--intent",
            "accept",
            "--target-id",
            "ssa-0/04-Research/CLI/README.md",
            "--target-path",
            "04-Research/CLI/README.md",
            "--candidate",
            fixture.candidate_arg(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let value = fixture.parse_single_json_stdout(&output);
    assert_eq!(value["command"], "propose");
    assert_eq!(value["state"], "submitted");
    assert_eq!(
        protected_durable_state(&fixture.store_root),
        before,
        "propose must not mutate intake, locks, ledger, canon, policy, evidence, derived, or transaction authority",
    );

    let submission_id = value["submission_id"].as_str().unwrap();
    let own_request = fixture.store_root.join("staging/inbox").join(submission_id);
    let mut entries = std::fs::read_dir(&own_request)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .into_iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, ["candidate.md", "submission.meta.json"]);
    assert_eq!(
        std::fs::read_dir(fixture.store_root.join("staging/inbox"))
            .unwrap()
            .count(),
        1,
        "propose creates exactly its own untrusted request"
    );
    fixture.assert_machine_streams_redacted(&output);
}

#[test]
fn propose_without_confidence_preserves_null_confidence_through_cli_views() {
    let fixture = P3Fixture::new("cli-propose-absent-confidence");
    let proposed = fixture
        .command()
        .args([
            "propose",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--source",
            "src:agent:fixture",
            "--operation",
            "create",
            "--intent",
            "accept",
            "--target-id",
            "ssa-0/04-Research/CLI/README.md",
            "--target-path",
            "04-Research/CLI/README.md",
            "--candidate",
            fixture.candidate_arg(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(proposed.status.code(), Some(0));
    let submission_id = submission_id(&fixture, &proposed);
    let meta = std::fs::read(
        fixture
            .store_root
            .join("staging/inbox")
            .join(submission_id)
            .join("submission.meta.json"),
    )
    .unwrap();
    let meta: serde_json::Value = serde_json::from_slice(&meta).unwrap();
    assert_eq!(meta["confidence"], serde_json::Value::Null);
}

#[test]
fn intake_seal_without_a_real_tty_fails_before_mutation() {
    let fixture = P3Fixture::new("cli-intake-seal-no-tty");
    let submitted = propose(
        &fixture,
        "src:agent:fixture",
        "04-Research/CLI/README.md",
        Path::new(fixture.candidate_arg()),
        None,
        "json",
    );
    assert_eq!(submitted.status.code(), Some(0));
    let submission_id = submission_id(&fixture, &submitted);
    let before = durable_hashes(&fixture.store_root);
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
    assert!(output.stdout.is_empty());
    assert_eq!(diagnostics(&output)[0]["code"], "human.tty_required");
    assert_eq!(durable_hashes(&fixture.store_root), before);
}

#[test]
fn intake_seal_requires_exact_phrase_and_stages_only_after_human_confirmation() {
    let wrong_fixture = P3Fixture::new("cli-intake-seal-wrong-phrase");
    let submitted = propose(
        &wrong_fixture,
        "src:agent:fixture",
        "04-Research/CLI/README.md",
        Path::new(wrong_fixture.candidate_arg()),
        None,
        "json",
    );
    assert_eq!(submitted.status.code(), Some(0));
    let wrong_submission_id = submission_id(&wrong_fixture, &submitted);

    let wrong = run_intake_seal_at_real_tty(
        &wrong_fixture,
        &wrong_submission_id,
        &format!("SEAL INTAKE {wrong_submission_id} WRONG\n"),
        "json",
    );
    assert_eq!(
        wrong.status.code(),
        Some(1),
        "script wrapper stdout={} stderr={}",
        String::from_utf8_lossy(&wrong.stdout),
        String::from_utf8_lossy(&wrong.stderr)
    );
    let wrong_stream = String::from_utf8_lossy(&wrong.stdout);
    assert!(wrong_stream.contains("intake.incomplete"));
    assert!(
        std::fs::read(wrong_fixture.store_root.join("intake/events.jsonl"))
            .unwrap()
            .is_empty()
    );
    assert!(
        wrong_fixture
            .store_root
            .join("intake/transactions")
            .join(&wrong_submission_id)
            .is_dir(),
        "wrong confirmation retains protected custody for explicit recovery"
    );
    assert_eq!(
        std::fs::read_dir(wrong_fixture.store_root.join("locks"))
            .unwrap()
            .count(),
        1,
        "wrong confirmation retains the intake lease and fails closed"
    );

    let fixture = P3Fixture::new("cli-intake-seal-exact-phrase");
    let submitted = propose(
        &fixture,
        "src:agent:fixture",
        "04-Research/CLI/README.md",
        Path::new(fixture.candidate_arg()),
        None,
        "json",
    );
    assert_eq!(submitted.status.code(), Some(0));
    let submission_id = submission_id(&fixture, &submitted);

    let canon_before = durable_hashes(&fixture.store_root.join("canon"));
    let ledger_before = std::fs::read(fixture.ledger_path()).unwrap();
    let sealed = run_intake_seal_at_real_tty(
        &fixture,
        &submission_id,
        &format!("SEAL INTAKE {submission_id}\n"),
        "json",
    );
    assert_eq!(
        sealed.status.code(),
        Some(0),
        "script wrapper stdout={} stderr={}",
        String::from_utf8_lossy(&sealed.stdout),
        String::from_utf8_lossy(&sealed.stderr)
    );
    let stream = String::from_utf8_lossy(&sealed.stdout);
    assert!(stream.contains("\"state\":\"staged\""), "{stream}");
    assert!(!fixture
        .store_root
        .join("staging/inbox")
        .join(&submission_id)
        .exists());
    assert_eq!(
        durable_hashes(&fixture.store_root.join("canon")),
        canon_before
    );
    assert_eq!(std::fs::read(fixture.ledger_path()).unwrap(), ledger_before);
}

#[test]
fn intake_seal_rejection_is_ledger_and_canon_inert_and_text_is_redacted() {
    let fixture = P3Fixture::new("cli-intake-seal-rejected");
    let private_body = b"PRIVATE_INTAKE_REJECTED_BODY_321\n";
    let candidate = fixture.input_file("intake-rejected-private", private_body);
    let submitted = propose(
        &fixture,
        "src:agent:fixture",
        "04-Research/CLI/README.md",
        &candidate,
        None,
        "json",
    );
    assert_eq!(submitted.status.code(), Some(0));
    let submission_id = submission_id(&fixture, &submitted);
    let canon_before = durable_hashes(&fixture.store_root.join("canon"));
    let ledger_before = std::fs::read(fixture.ledger_path()).unwrap();
    let sealed = run_intake_seal_at_real_tty(
        &fixture,
        &submission_id,
        &format!("SEAL INTAKE {submission_id}\n"),
        "text",
    );
    assert_eq!(
        sealed.status.code(),
        Some(0),
        "script wrapper stdout={} stderr={}",
        String::from_utf8_lossy(&sealed.stdout),
        String::from_utf8_lossy(&sealed.stderr)
    );
    let stream = String::from_utf8_lossy(&sealed.stdout);
    assert!(stream.contains("state=rejected-at-intake"), "{stream}");
    assert!(!stream.contains("PRIVATE_INTAKE_REJECTED_BODY_321"));
    assert_eq!(
        durable_hashes(&fixture.store_root.join("canon")),
        canon_before
    );
    assert_eq!(std::fs::read(fixture.ledger_path()).unwrap(), ledger_before);
}

#[test]
fn intake_seal_cli_has_no_noninteractive_or_batch_escape_hatch() {
    let fixture = P3Fixture::new("cli-intake-seal-grammar");
    let first = format!("sub-{}", "a".repeat(64));
    let second = format!("sub-{}", "b".repeat(64));
    let cases = [
        vec![
            "intake",
            "seal",
            &first,
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--yes",
        ],
        vec![
            "intake",
            "seal",
            &first,
            &second,
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
        ],
        vec![
            "intake",
            "seal",
            &first,
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--unknown",
        ],
    ];
    for args in cases {
        let output = fixture.command().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn proposal_list_incomplete_inbox_is_one_redacted_data_failure() {
    let fixture = P3Fixture::new("cli-task7-incomplete-inbox");
    let incomplete = fixture
        .store_root
        .join("staging/inbox")
        .join(format!("sub-{}", "a".repeat(64)));
    std::fs::create_dir(&incomplete).unwrap();
    std::fs::write(
        incomplete.join("candidate.md"),
        b"PRIVATE_INCOMPLETE_QUEUE_BODY_7206\n",
    )
    .unwrap();
    let output = fixture
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
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let errors = diagnostics(&output);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["code"], "proposal.pair_incomplete");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_INCOMPLETE_QUEUE_BODY_7206"));
    fixture.assert_machine_streams_redacted(&output);
}

#[test]
fn proposal_list_and_status_project_a_complete_inbox_as_submitted() {
    let fixture = P3Fixture::new("cli-task7-submitted-projection");
    let submitted = propose(
        &fixture,
        "src:agent:task7-submitted",
        "04-Research/Task7-Submitted/README.md",
        Path::new(fixture.candidate_arg()),
        None,
        "json",
    );
    assert_eq!(submitted.status.code(), Some(0));
    let submission_id = submission_id(&fixture, &submitted);

    let list = fixture
        .command()
        .args([
            "proposals",
            "list",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--state",
            "submitted",
        ])
        .output()
        .unwrap();
    assert_eq!(list.status.code(), Some(0));
    let records = fixture.parse_single_json_stdout(&list)["proposals"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["submission_id"], submission_id);
    assert_eq!(records[0]["state"], "submitted");

    let status = fixture
        .command()
        .args([
            "proposals",
            "status",
            &submission_id,
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
}

#[test]
fn store_stats_counts_complete_inbox_submissions_without_queue_authority() {
    let fixture = P3Fixture::new("cli-task7-submitted-stats");
    let submitted = propose(
        &fixture,
        "src:agent:task7-stats",
        "04-Research/Task7-Stats/README.md",
        Path::new(fixture.candidate_arg()),
        None,
        "json",
    );
    assert_eq!(submitted.status.code(), Some(0));
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
    let value = fixture.parse_single_json_stdout(&stats);
    assert_eq!(value["queue"]["submitted"], 1);
    assert_eq!(value["queue"]["staged_open"], 0);
    assert_eq!(value["queue"]["rejected_at_intake"], 0);
    assert_eq!(value["queue"]["alarm"], false);
}

#[test]
fn proposal_list_and_status_are_sorted_redacted_for_submitted_evidence() {
    let fixture = P3Fixture::new("cli-list-status");
    let candidate_body_sentinel = "PRIVATE_CANDIDATE_BODY_84";
    let mut candidate_bytes = std::fs::read(fixture.candidate_arg()).unwrap();
    candidate_bytes.extend_from_slice(format!("\n{candidate_body_sentinel}\n").as_bytes());
    let candidate_file = fixture.input_file("private-candidate-body", &candidate_bytes);
    let candidate = candidate_file.as_path();
    let candidate_hash = blake3::hash(&std::fs::read(candidate).unwrap())
        .to_hex()
        .to_string();
    let advice_sentinel = "PRIVATE_EVALUATOR_REASON_42";
    let evaluator = fixture.input_file(
        "PRIVATE_EVALUATOR_INPUT_PATH_73",
        &serde_json::to_vec(&sovmem::profile::EvaluatorBlock {
            protocol: "adversarial-bug-finding-v1".to_owned(),
            source_id: "src:agent:skeptic".to_owned(),
            evaluated_at: "2026-07-14T04:29:00.000000000Z".to_owned(),
            candidate_hash,
            recommendation: "looks-sound".to_owned(),
            reasons: vec![advice_sentinel.to_owned()],
        })
        .unwrap(),
    );
    let submitted = propose(
        &fixture,
        "src:agent:fixture",
        "04-Research/CLI/README.md",
        candidate,
        Some(&evaluator),
        "json",
    );
    assert_eq!(submitted.status.code(), Some(0));
    let submission_id = submission_id(&fixture, &submitted);

    let listed = fixture
        .command()
        .args([
            "proposals",
            "list",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(listed.status.code(), Some(0));
    let value = fixture.parse_single_json_stdout(&listed);
    assert_eq!(value["command"], "proposals.list");
    assert_eq!(value["proposals"][0]["submission_id"], submission_id);
    assert_eq!(value["proposals"][0]["state"], "submitted");
    assert_eq!(value["proposals"][0]["evaluator"]["reason_count"], 1);
    assert!(!String::from_utf8_lossy(&listed.stdout).contains(advice_sentinel));
    let listed_streams = format!(
        "{}{}",
        String::from_utf8_lossy(&listed.stdout),
        String::from_utf8_lossy(&listed.stderr)
    );
    for sentinel in [
        candidate_body_sentinel,
        evaluator.to_str().unwrap(),
        candidate.to_str().unwrap(),
    ] {
        assert!(!listed_streams.contains(sentinel), "leaked {sentinel}");
    }
    fixture.assert_machine_streams_redacted(&listed);

    let status = |show_advice: bool| {
        let mut command = fixture.command();
        command.args([
            "proposals",
            "status",
            &submission_id,
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--format",
            "json",
        ]);
        if show_advice {
            command.arg("--show-advice");
        }
        command.output().unwrap()
    };
    let hidden = status(false);
    assert_eq!(hidden.status.code(), Some(0));
    let hidden_text = String::from_utf8_lossy(&hidden.stdout);
    assert!(!hidden_text.contains("evaluator_reasons"));
    assert!(!hidden_text.contains(advice_sentinel));
    assert!(!hidden_text.contains(candidate_body_sentinel));
    assert!(!hidden_text.contains(evaluator.to_str().unwrap()));
    let hidden_value = fixture.parse_single_json_stdout(&hidden);
    assert_eq!(
        hidden_value["proposal"]["base_kind"],
        serde_json::Value::Null
    );
    assert!(hidden_value["proposal"]["metadata_hash"].is_string());

    let shown = status(true);
    assert_eq!(shown.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&shown.stdout).contains(advice_sentinel));
    fixture.assert_machine_streams_redacted(&shown);
}

#[test]
fn proposal_failures_are_stable_sorted_jsonl_and_never_emit_partial_success() {
    let fixture = P3Fixture::new("cli-negative");
    let malformed = fixture
        .command()
        .args([
            "propose",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--source",
            "src:agent:fixture",
            "--operation",
            "create",
            "--intent",
            "accept",
            "--target-id",
            "ssa-0/04-Research/CLI/README.md",
            "--target-path",
            "04-Research/CLI/README.md",
            "--candidate",
            fixture.candidate_arg(),
            "--confidence",
            "0.1234567",
        ])
        .output()
        .unwrap();
    assert_eq!(malformed.status.code(), Some(2));
    assert!(malformed.stdout.is_empty());
    assert_eq!(
        diagnostics(&malformed)[0]["code"],
        "proposal.confidence_invalid"
    );

    let _first_proposal_id = stage_cli_proposal(&fixture, "04-Research/CLI/README.md");
    let duplicate = propose(
        &fixture,
        "src:agent:fixture",
        "04-Research/CLI/README.md",
        Path::new(fixture.candidate_arg()),
        None,
        "json",
    );
    assert_eq!(duplicate.status.code(), Some(0));
    let duplicate_submission_id = submission_id(&fixture, &duplicate);
    let rejected = run_intake_seal_at_real_tty(
        &fixture,
        &duplicate_submission_id,
        &format!("SEAL INTAKE {duplicate_submission_id}\n"),
        "json",
    );
    assert_eq!(rejected.status.code(), Some(0));
    let rejected = script_json_stdout(&rejected);
    assert_eq!(rejected["submission_id"], duplicate_submission_id);
    assert_eq!(rejected["proposal_id"], serde_json::Value::Null);
    assert_eq!(rejected["state"], "rejected-at-intake");
    let intake = std::fs::read_to_string(fixture.store_root.join("intake/events.jsonl")).unwrap();
    let duplicate_event: serde_json::Value =
        serde_json::from_str(intake.lines().last().unwrap()).unwrap();
    assert_eq!(duplicate_event["outcome"], "REJECTED");
    assert_eq!(
        duplicate_event["rule_codes"],
        serde_json::json!(["proposal.duplicate_content"])
    );

    let unsafe_target = fixture
        .command()
        .args([
            "propose",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--source",
            "src:agent:fixture",
            "--operation",
            "create",
            "--intent",
            "accept",
            "--target-id",
            "ssa-0//private/tmp/PRIVATE_TARGET_PATH_91.md",
            "--target-path",
            "/private/tmp/PRIVATE_TARGET_PATH_91.md",
            "--candidate",
            fixture.candidate_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(unsafe_target.status.code(), Some(2));
    assert!(unsafe_target.stdout.is_empty());
    assert!(diagnostics(&unsafe_target)
        .iter()
        .any(|diagnostic| diagnostic["code"] == "proposal.target_unsafe"));
    assert!(!String::from_utf8_lossy(&unsafe_target.stderr).contains("PRIVATE_TARGET_PATH_91"));
    fixture.assert_machine_streams_redacted(&unsafe_target);
}

#[test]
fn reserved_canonical_targets_are_unsafe_before_intake() {
    let fixture = P3Fixture::new("cli-reserved-targets");
    for target_path in [
        "index.md",
        "judgments/x.md",
        "staging/x.md",
        "ledger/x.md",
        "evidence/x.md",
        "derived/x.md",
        "transactions/x.md",
        "locks/x.md",
        "manifest.json",
        "policy.toml",
        "CURRENT",
    ] {
        let output = fixture
            .command()
            .args([
                "propose",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
                "--source",
                "src:agent:reserved",
                "--operation",
                "create",
                "--intent",
                "accept",
                "--target-id",
                &format!("ssa-0/{target_path}"),
                "--target-path",
                target_path,
                "--candidate",
                fixture.candidate_arg(),
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{target_path}");
        assert!(output.stdout.is_empty(), "{target_path}");
        assert_eq!(diagnostics(&output)[0]["code"], "proposal.target_unsafe");
    }
    assert_eq!(
        std::fs::read_dir(fixture.store_root.join("staging/inbox"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        std::fs::read_dir(fixture.store_root.join("intake/proposals"))
            .unwrap()
            .count(),
        0
    );
    assert!(
        std::fs::read(fixture.store_root.join("intake/events.jsonl"))
            .unwrap()
            .is_empty()
    );
}

fn exact_store_stats_json(value: &serde_json::Value) -> String {
    let queue = format!(
        concat!(
            "{{\"submitted\":{},\"staged_open\":{},\"rejected_at_intake\":{},",
            "\"open\":{},\"alarm_at\":{},\"alarm\":{}}}"
        ),
        value["queue"]["submitted"],
        value["queue"]["staged_open"],
        value["queue"]["rejected_at_intake"],
        value["queue"]["open"],
        value["queue"]["alarm_at"],
        value["queue"]["alarm"],
    );
    let ledger = format!(
        "{{\"events\":{},\"verdicts\":{},\"applied_generations\":{},\"head\":{}}}",
        value["ledger"]["events"],
        value["ledger"]["verdicts"],
        value["ledger"]["applied_generations"],
        value["ledger"]["head"],
    );
    format!(
        concat!(
            "{{\"status\":\"ok\",\"command\":\"stats\",\"sealed\":{},",
            "\"documents\":{},\"by_provenance\":{},\"by_type\":{},\"by_layer\":{},",
            "\"warnings\":{},\"exclusions\":{},\"bundle_hash\":{},",
            "\"queue\":{},\"ledger\":{},\"store_id\":{},\"profile_version\":{},",
            "\"current_generation\":{}}}\n"
        ),
        value["sealed"],
        value["documents"],
        value["by_provenance"],
        value["by_type"],
        value["by_layer"],
        value["warnings"],
        value["exclusions"],
        value["bundle_hash"],
        queue,
        ledger,
        value["store_id"],
        value["profile_version"],
        value["current_generation"],
    )
}

#[test]
fn store_stats_public_output_excludes_queue_timestamps() {
    let fixture = P3Fixture::new("cli-stats-no-queue-timestamps");
    let run = |format: &str| {
        fixture
            .command()
            .args([
                "stats",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
                "--format",
                format,
            ])
            .output()
            .unwrap()
    };

    let json = run("json");
    assert_eq!(json.status.code(), Some(0));
    let json_stdout = String::from_utf8(json.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(json_stdout.trim_end()).unwrap();
    assert!(
        value["queue"].get("oldest_submitted_at").is_none(),
        "queue stats must not expose operational timestamps"
    );
    assert!(!json_stdout.contains("oldest_submitted_at"));

    let text = run("text");
    assert_eq!(text.status.code(), Some(0));
    assert!(!String::from_utf8(text.stdout)
        .unwrap()
        .contains("oldest_submitted_at"));
}

#[test]
fn store_stats_exactly_extend_bundle_stats_for_unsealed_and_current_sealed_generation() {
    let fixture = P3Fixture::new("cli-stats-exact");
    let run = |format: &str| {
        fixture
            .command()
            .args([
                "stats",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
                "--format",
                format,
            ])
            .output()
            .unwrap()
    };

    let unsealed = run("json");
    assert_eq!(unsealed.status.code(), Some(0));
    let unsealed_value = fixture.parse_single_json_stdout(&unsealed);
    assert!(unsealed_value.get("seed_documents").is_none());
    for field in [
        "documents",
        "by_provenance",
        "by_type",
        "by_layer",
        "warnings",
        "exclusions",
    ] {
        assert!(unsealed_value.get(field).is_some(), "missing {field}");
    }
    assert_eq!(
        String::from_utf8(unsealed.stdout).unwrap(),
        exact_store_stats_json(&unsealed_value)
    );
    let unsealed_text = String::from_utf8(run("text").stdout).unwrap();
    assert!(unsealed_text.contains("documents="));
    assert!(unsealed_text.contains("by_provenance.secondary="));
    assert!(!unsealed_text.contains("seed_documents="));

    let seed_documents = unsealed_value["documents"].as_u64().unwrap();
    fixture.install_fixture_chain_with_one_applied_generation();
    let sealed = run("json");
    assert_eq!(sealed.status.code(), Some(0));
    let sealed_value = fixture.parse_single_json_stdout(&sealed);
    assert_eq!(sealed_value["sealed"], true);
    assert_eq!(sealed_value["current_generation"], "gen-0000000000000001");
    assert_eq!(sealed_value["documents"].as_u64(), Some(seed_documents + 2));
    assert_eq!(
        String::from_utf8(sealed.stdout).unwrap(),
        exact_store_stats_json(&sealed_value)
    );
    let sealed_text = String::from_utf8(run("text").stdout).unwrap();
    assert!(sealed_text.contains(&format!("documents={}\n", seed_documents + 2)));
    assert!(sealed_text.ends_with("current_generation=gen-0000000000000001\n"));
}

#[test]
fn no_cli_flag_can_supply_derived_authority_and_stats_requires_one_storage_mode() {
    let fixture = P3Fixture::new("cli-authority-shape");
    for forbidden in [
        "--proposal-id",
        "--submitted-at",
        "--candidate-hash",
        "--policy-hash",
        "--profile-hash",
        "--base-hash",
    ] {
        let output = fixture
            .command()
            .args(["propose", forbidden, "forged"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{forbidden}");
        assert!(output.stdout.is_empty());
        assert_eq!(diagnostics(&output)[0]["code"], "cli.usage");
    }
    let neither = fixture
        .command()
        .args(["stats", "--profile", fixture.profile_arg()])
        .output()
        .unwrap();
    assert_eq!(neither.status.code(), Some(2));
    assert_eq!(diagnostics(&neither)[0]["code"], "cli.usage");
    let both = fixture
        .command()
        .args([
            "stats",
            "--profile",
            fixture.profile_arg(),
            "--bundle",
            fixture.p2_bundle_arg(),
            "--store",
            fixture.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(both.status.code(), Some(2));
    assert_eq!(diagnostics(&both)[0]["code"], "cli.usage");
}

#[test]
fn store_stats_submitted_inbox_has_no_alarm_authority_and_order_is_deterministic() {
    let fixture = P3Fixture::new("cli-queue-alarm");
    for index in 0..25 {
        let target_path = format!("04-Research/CLI-{index:02}/README.md");
        let candidate = fixture.cli_candidate(&format!("queue-{index:02}"), &target_path);
        let source = if index < 10 {
            "src:agent:quota".to_owned()
        } else {
            format!("src:agent:queue-{index:02}")
        };
        let staged = propose(&fixture, &source, &target_path, &candidate, None, "json");
        assert_eq!(
            staged.status.code(),
            Some(0),
            "stage queue fixture proposal: {}",
            String::from_utf8_lossy(&staged.stderr)
        );
    }

    let quota_path = "04-Research/CLI-Quota/README.md";
    let quota_candidate = fixture.cli_candidate("queue-quota", quota_path);
    let extra = propose(
        &fixture,
        "src:agent:quota",
        quota_path,
        &quota_candidate,
        None,
        "json",
    );
    assert_eq!(extra.status.code(), Some(0));

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
    let stats = fixture.parse_single_json_stdout(&stats);
    assert_eq!(stats["sealed"], false);
    assert_eq!(stats["ledger"]["events"], 0);
    assert_eq!(stats["ledger"]["head"], serde_json::Value::Null);
    assert_eq!(stats["current_generation"], "seed");
    assert_eq!(stats["queue"]["submitted"], 26);
    assert_eq!(stats["queue"]["staged_open"], 0);
    assert_eq!(stats["queue"]["open"], 0);
    assert_eq!(stats["queue"]["alarm_at"], 25);
    assert_eq!(stats["queue"]["alarm"], false);
    assert!(stats["queue"].get("oldest_submitted_at").is_none());

    let list = fixture
        .command()
        .args([
            "proposals",
            "list",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--state",
            "submitted",
        ])
        .output()
        .unwrap();
    assert_eq!(list.status.code(), Some(0));
    let proposals = fixture.parse_single_json_stdout(&list)["proposals"]
        .as_array()
        .unwrap()
        .clone();
    let keys = proposals
        .iter()
        .map(|proposal| {
            (
                proposal["submitted_at"].as_str().unwrap().to_owned(),
                proposal["submission_id"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn bundle_stats_bytes_remain_legacy_compatible_and_text_is_stable() {
    let fixture = P3Fixture::new("cli-compat-text");
    let bundle_stats = || {
        fixture
            .command()
            .args([
                "stats",
                "--profile",
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../profiles/audy-ssa0-v0/profile.yaml")
                    .to_str()
                    .unwrap(),
                "--bundle",
                fixture.p2_bundle_arg(),
            ])
            .output()
            .unwrap()
    };
    let first = bundle_stats();
    let second = bundle_stats();
    assert_eq!(first.status.code(), Some(0));
    assert_eq!(first.stdout, second.stdout);
    let value: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(value["command"], "stats");
    assert!(value.get("queue").is_none());
    assert!(value.get("ledger").is_none());
    assert!(value.get("store_id").is_none());

    let proposed = propose(
        &fixture,
        "src:agent:fixture",
        "04-Research/CLI/README.md",
        Path::new(fixture.candidate_arg()),
        None,
        "text",
    );
    assert_eq!(proposed.status.code(), Some(0));
    let text = String::from_utf8(proposed.stdout.clone()).unwrap();
    assert!(text.starts_with("status=ok\ncommand=propose\nsubmission_id=sub-"));
    assert!(text.ends_with('\n'));
    fixture.assert_machine_streams_redacted(&proposed);
    let submission_id = text
        .lines()
        .find_map(|line| line.strip_prefix("submission_id="))
        .unwrap();

    let list_text = fixture
        .command()
        .args([
            "proposals",
            "list",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--format",
            "text",
        ])
        .output()
        .unwrap();
    assert_eq!(list_text.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&list_text.stdout)
        .starts_with("status=ok\ncommand=proposals.list\ncount=1\n"));

    let status_text = fixture
        .command()
        .args([
            "proposals",
            "status",
            submission_id,
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--format",
            "text",
        ])
        .output()
        .unwrap();
    assert_eq!(status_text.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&status_text.stdout)
        .starts_with("status=ok\ncommand=proposals.status\nproposal.submission_id=sub-"));
    assert!(String::from_utf8_lossy(&status_text.stdout).contains("\nproposal.state=submitted\n"));

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
    let stats_text = String::from_utf8_lossy(&stats_text.stdout);
    assert!(stats_text.starts_with("status=ok\ncommand=stats\nsealed=false\n"));
    assert!(stats_text.contains("queue.submitted=1\n"));
    assert!(stats_text.contains("queue.open=0\n"));
    assert!(stats_text.ends_with("current_generation=seed\n"));
}

#[test]
fn rejected_intake_receipts_are_ordered_filterable_and_status_safe() {
    let fixture = P3Fixture::new("cli-rejection-receipts");
    for (source, target) in [
        ("src:agent:reject-b", "01-Core/Reject-B.md"),
        ("src:agent:reject-a", "01-Core/Reject-A.md"),
    ] {
        let candidate = fixture.cli_candidate(source, target);
        let output = fixture
            .command()
            .args([
                "propose",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
                "--source",
                source,
                "--operation",
                "create",
                "--intent",
                "accept",
                "--target-id",
                &format!("ssa-0/{target}"),
                "--target-path",
                target,
                "--candidate",
                candidate.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        let submission_id = submission_id(&fixture, &output);
        let sealed = run_intake_seal_at_real_tty(
            &fixture,
            &submission_id,
            &format!("SEAL INTAKE {submission_id}\n"),
            "json",
        );
        assert_eq!(
            sealed.status.code(),
            Some(0),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&sealed.stdout),
            String::from_utf8_lossy(&sealed.stderr)
        );
        let sealed = script_json_stdout(&sealed);
        assert_eq!(sealed["submission_id"], submission_id);
        assert_eq!(sealed["proposal_id"], serde_json::Value::Null);
        assert_eq!(sealed["state"], "rejected-at-intake");
    }

    let listed = fixture
        .command()
        .args([
            "proposals",
            "list",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--state",
            "rejected-at-intake",
        ])
        .output()
        .unwrap();
    assert_eq!(listed.status.code(), Some(0));
    let records = fixture.parse_single_json_stdout(&listed)["proposals"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(records.len(), 2);
    let keys = records
        .iter()
        .map(|record| {
            (
                record["received_at"].as_str().unwrap().to_owned(),
                record["receipt_id"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    assert!(records.iter().all(|record| record["proposal_id"].is_null()));
    fixture.assert_machine_streams_redacted(&listed);

    let receipt_id = records[0]["receipt_id"].as_str().unwrap();
    let status = fixture
        .command()
        .args([
            "proposals",
            "status",
            receipt_id,
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--show-advice",
        ])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(0));
    let value = fixture.parse_single_json_stdout(&status);
    assert_eq!(value["proposal"]["state"], "rejected-at-intake");
    assert!(value["proposal"]["metadata_hash"].is_null());

    let missing = fixture
        .command()
        .args([
            "proposals",
            "status",
            "prp-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    assert_eq!(diagnostics(&missing)[0]["code"], "proposal.not_found");
}

#[test]
fn terminal_list_and_status_cover_ruled_applied_and_superseded_without_private_evidence() {
    let fixture = P3Fixture::new("cli-terminal-states");
    fixture.install_fixture_chain_with_one_applied_generation();
    fixture.extend_fixture_chain_with_superseding_generation();
    let listed = fixture
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
    assert_eq!(listed.status.code(), Some(0));
    let stdout = String::from_utf8(listed.stdout.clone()).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.ends_with('\n'));
    assert!(stdout.find("\"proposal_id\"").unwrap() < stdout.find("\"receipt_id\"").unwrap());
    let value = fixture.parse_single_json_stdout(&listed);
    let records = value["proposals"].as_array().unwrap();
    for state in ["ruled", "applied", "superseded"] {
        let record = records
            .iter()
            .find(|record| record["state"] == state)
            .unwrap_or_else(|| panic!("missing {state}"));
        let id = record["proposal_id"].as_str().unwrap();
        let status = fixture
            .command()
            .args([
                "proposals",
                "status",
                id,
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
            state
        );
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
        for sentinel in [
            "fixture retained reject",
            "fixture applied accept",
            "fixture superseding accept",
            "Superseding fixture revision",
            "/private/tmp",
            "/var/folders",
        ] {
            assert!(!combined.contains(sentinel), "leaked {sentinel}");
        }
        fixture.assert_machine_streams_redacted(&status);
    }
}

#[test]
fn level4_verdict_list_filters_show_flags_and_exact_replay_are_stable() {
    let fixture = P3Fixture::new("cli-task16-list-replay");
    fixture.install_fixture_chain_with_one_applied_generation();

    let verified = fixture
        .command()
        .args([
            "verify",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--level",
            "4",
        ])
        .output()
        .unwrap();
    assert_eq!(verified.status.code(), Some(0));
    let verify_value = fixture.parse_single_json_stdout(&verified);
    assert_eq!(verify_value["level"], 4);
    assert_eq!(verify_value["events"], 3);
    assert_eq!(verify_value["verdicts"], 2);
    assert_eq!(verify_value["accept"], 1);
    assert_eq!(verify_value["reject"], 1);
    assert_eq!(verify_value["applied_generations"], 1);
    assert_eq!(verify_value["current_generation"], "gen-0000000000000001");

    let list = |extra: &[&str]| {
        let mut command = fixture.command();
        command.args([
            "verdicts",
            "list",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
        ]);
        command.args(extra).output().unwrap()
    };
    let hidden = list(&[]);
    assert_eq!(hidden.status.code(), Some(0));
    let hidden_text = String::from_utf8_lossy(&hidden.stdout);
    assert!(!hidden_text.contains("fixture retained reject"));
    assert!(!hidden_text.contains("fixture applied accept"));
    assert!(!hidden_text.contains("evaluator_reasons"));
    let records = fixture.parse_single_json_stdout(&hidden)["verdicts"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["event_id"], "evt-0000000000000001");
    assert_eq!(records[1]["event_id"], "evt-0000000000000002");
    assert_eq!(records[0]["generation_id"], serde_json::Value::Null);
    assert_eq!(records[0]["manifest_hash"], serde_json::Value::Null);
    assert_eq!(records[0]["applied"], false);
    assert_eq!(records[1]["applied"], true);

    let reject_only = list(&["--verb", "REJECT"]);
    assert_eq!(reject_only.status.code(), Some(0));
    assert_eq!(
        fixture.parse_single_json_stdout(&reject_only)["verdicts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let source_only = list(&["--source", "src:agent:fixture"]);
    assert_eq!(source_only.status.code(), Some(0));
    assert_eq!(
        fixture.parse_single_json_stdout(&source_only)["verdicts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let shown = list(&["--show-reasons", "--show-advice"]);
    assert_eq!(shown.status.code(), Some(0));
    let shown_text = String::from_utf8_lossy(&shown.stdout);
    assert!(shown_text.contains("fixture retained reject"));
    assert!(shown_text.contains("fixture applied accept"));
    assert!(!String::from_utf8_lossy(&shown.stderr).contains("fixture applied accept"));

    let accept = records
        .iter()
        .find(|record| record["verb"] == "ACCEPT")
        .unwrap();
    let before = durable_hashes(&fixture.store_root);
    let replay = fixture
        .command()
        .args([
            "verdict",
            accept["proposal_id"].as_str().unwrap(),
            "ACCEPT",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--reason",
            "fixture applied accept",
        ])
        .output()
        .unwrap();
    assert_eq!(
        replay.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(
        fixture.parse_single_json_stdout(&replay)["idempotent"],
        true
    );
    assert_eq!(durable_hashes(&fixture.store_root), before);
}

#[test]
fn task16_storage_modes_reject_every_forbidden_combination() {
    let fixture = P3Fixture::new("cli-task16-modes");
    let cases = vec![
        (
            vec!["verify", "--profile", fixture.profile_arg(), "--level", "4"],
            "cli.usage",
        ),
        (
            vec![
                "verify",
                "--profile",
                fixture.profile_arg(),
                "--bundle",
                fixture.p2_bundle_arg(),
                "--level",
                "4",
            ],
            "cli.usage",
        ),
        (
            vec![
                "verify",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
                "--level",
                "3",
            ],
            "cli.usage",
        ),
        (
            vec![
                "verify",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
                "--level",
                "4",
                "--require-corpus",
            ],
            "cli.usage",
        ),
        (
            vec![
                "index",
                "--profile",
                fixture.profile_arg(),
                "--bundle",
                fixture.p2_bundle_arg(),
            ],
            "cli.usage",
        ),
        (
            vec![
                "index",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
                "--index",
                "PRIVATE_INDEX_ROOT",
            ],
            "cli.usage",
        ),
        (
            vec![
                "query",
                "--profile",
                fixture.profile_arg(),
                "--bundle",
                fixture.p2_bundle_arg(),
                "--query",
                "x",
            ],
            "cli.usage",
        ),
        (
            vec![
                "query",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
                "--index",
                "PRIVATE_INDEX_ROOT",
                "--query",
                "x",
            ],
            "cli.usage",
        ),
        (
            vec![
                "eval",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
            ],
            "cli.usage",
        ),
    ];
    for (args, code) in cases {
        let output = fixture.command().args(&args).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "args={args:?} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert_eq!(diagnostics(&output)[0]["code"], code, "args={args:?}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_INDEX_ROOT"));
    }
}

#[test]
fn store_index_rebuild_and_query_are_receipt_bound_and_metadata_only() {
    let fixture = P3Fixture::new("cli-task16-index-query");
    fixture.install_fixture_chain_with_one_applied_generation();
    let index_root = fixture
        .store_root
        .join("derived/indexes/gen-0000000000000001");
    if index_root.exists() {
        std::fs::remove_dir_all(&index_root).unwrap();
    }

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
    assert_eq!(
        rebuilt.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&rebuilt.stderr)
    );
    let rebuilt_value = fixture.parse_single_json_stdout(&rebuilt);
    assert_eq!(rebuilt_value["store_id"], "audy-sovmem-writeback-v2");
    assert_eq!(rebuilt_value["generation"], "gen-0000000000000001");
    assert!(rebuilt_value["manifest_hash"].as_str().unwrap().len() == 64);
    assert!(rebuilt_value["receipt_hash"].as_str().unwrap().len() == 64);

    let private_query = "PRIVATE_QUERY_SENTINEL_208";
    let queried = fixture
        .command()
        .args([
            "query",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--query",
            private_query,
        ])
        .output()
        .unwrap();
    assert_eq!(queried.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&queried.stdout);
    assert!(!stdout.contains(private_query));
    let value = fixture.parse_single_json_stdout(&queried);
    assert_eq!(value["generation"], "gen-0000000000000001");
    assert!(value.get("query").is_none());
    assert!(value["results"].is_array());
    fixture.assert_machine_streams_redacted(&queried);
}

#[test]
fn verdict_list_default_projection_is_the_exact_approved_whole_object() {
    let fixture = P3Fixture::new("cli-task16-list-exact-projection");
    fixture.install_fixture_chain_with_one_applied_generation();
    let output = fixture
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
    assert_eq!(output.status.code(), Some(0));
    let value = fixture.parse_single_json_stdout(&output);
    let first = value["verdicts"][0].as_object().unwrap();
    let mut expected_keys = vec![
        "event_id",
        "event_hash",
        "occurred_at",
        "proposal_id",
        "verdict_id",
        "source_id",
        "verb",
        "actor",
        "confidence",
        "evaluator",
        "applied",
        "candidate_hash",
        "rewrite_hash",
        "target_prior_hash",
        "target_new_hash",
        "judgment_hash",
        "generation_id",
        "manifest_hash",
        "bundle_hash",
        "reindex_receipt_hash",
        "reason_hash",
        "reason_bytes",
    ];
    expected_keys.sort_unstable();
    assert_eq!(
        first.keys().map(String::as_str).collect::<Vec<_>>(),
        expected_keys
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let positions = [
        "\"event_id\"",
        "\"event_hash\"",
        "\"occurred_at\"",
        "\"proposal_id\"",
        "\"verdict_id\"",
        "\"source_id\"",
        "\"verb\"",
        "\"actor\"",
        "\"confidence\"",
        "\"evaluator\"",
        "\"applied\"",
        "\"candidate_hash\"",
        "\"rewrite_hash\"",
        "\"target_prior_hash\"",
        "\"target_new_hash\"",
        "\"judgment_hash\"",
        "\"generation_id\"",
        "\"manifest_hash\"",
        "\"bundle_hash\"",
        "\"reindex_receipt_hash\"",
        "\"reason_hash\"",
        "\"reason_bytes\"",
    ]
    .map(|field| stdout.find(field).unwrap());
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{stdout}"
    );
}

#[test]
fn store_query_text_one_hop_renders_bm25_and_expanded_hits() {
    let fixture = P3Fixture::new("cli-task16-store-query-text-expansion");
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
    let index_root = fixture
        .store_root
        .join("derived/indexes/gen-0000000000000001");
    let selected = std::fs::read_to_string(index_root.join("CURRENT")).unwrap();
    let graph: serde_json::Value = serde_json::from_slice(
        &std::fs::read(index_root.join(selected.trim()).join("graph.json")).unwrap(),
    )
    .unwrap();
    let source_id = graph["edges"][0]["source"].as_str().unwrap();
    let target_id = graph["edges"][0]["target"].as_str().unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            fixture
                .store_root
                .join("canon/generations/gen-0000000000000001/manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let source_path = manifest["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|document| document["id"] == source_id)
        .unwrap()["canonical_path"]
        .as_str()
        .unwrap();
    let source = std::fs::read_to_string(
        fixture
            .store_root
            .join("canon/generations/gen-0000000000000001/bundle")
            .join(source_path),
    )
    .unwrap();
    let query = source
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .expect("edge source has a title")
        .to_owned();
    let output = fixture
        .command()
        .args([
            "query",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--query",
            &query,
            "--top-k",
            "1",
            "--expand",
            "one-hop",
            "--format",
            "text",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains(&format!("bm25[1]: id={source_id}")),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("expanded[1]: id={target_id} via={source_id}")),
        "{stdout}"
    );
    assert!(stdout.contains(" via="), "{stdout}");
    assert!(stdout.contains(" direction="), "{stdout}");
    assert!(stdout.contains(" relation="), "{stdout}");
}

#[test]
fn task16_exit_classes_follow_the_stable_underlying_error_codes() {
    let corrupted = P3Fixture::new("cli-task16-exit-readable-corruption");
    corrupted.install_fixture_chain_with_one_applied_generation();
    std::fs::write(corrupted.store_root.join("ledger/events.jsonl"), b"{}\n").unwrap();
    let verdict = corrupted
        .command()
        .args([
            "verdict",
            "proposal-does-not-matter",
            "REJECT",
            "--profile",
            corrupted.profile_arg(),
            "--store",
            corrupted.store_arg(),
            "--reason",
            "private reason must not affect exit class",
        ])
        .output()
        .unwrap();
    assert_eq!(verdict.status.code(), Some(1));

    for command in ["verify", "index", "query"] {
        let fixture = P3Fixture::new(&format!("cli-task16-exit-unreadable-{command}"));
        let missing = fixture.store_root.join("missing-store");
        let mut args = vec![
            command,
            "--profile",
            fixture.profile_arg(),
            "--store",
            missing.to_str().unwrap(),
        ];
        if command == "verify" {
            args.extend(["--level", "4"]);
        } else if command == "query" {
            args.extend(["--query", "private query"]);
        }
        let output = fixture.command().args(args).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "command={command} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn store_index_publication_uses_atomic_slots_and_cleans_receipt_failure_staging() {
    let fixture = P3Fixture::new("cli-task16-index-atomic-slots");
    fixture.install_fixture_chain_with_one_applied_generation();
    let run_index = || {
        fixture
            .command()
            .args([
                "index",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
            ])
            .output()
            .unwrap()
    };
    let first = run_index();
    assert_eq!(first.status.code(), Some(0));
    let root = fixture
        .store_root
        .join("derived/indexes/gen-0000000000000001");
    let pointer = std::fs::read_to_string(root.join("CURRENT")).expect("atomic index pointer");
    assert_store_slot_pointer(&pointer);
    assert!(root.join(pointer.trim()).is_dir());

    let receipt_path = std::fs::read_dir(fixture.store_root.join("evidence/reindex"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let receipt = std::fs::read(&receipt_path).unwrap();
    std::fs::write(&receipt_path, b"PRIVATE_RECEIPT_CORRUPTION").unwrap();
    let failed = run_index();
    assert_eq!(failed.status.code(), Some(1));
    assert!(
        std::fs::read_dir(&root).unwrap().all(|entry| {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            !name.starts_with(".slot-building-") && !name.starts_with("CURRENT.next-")
        }),
        "receipt failure left hidden staging"
    );
    std::fs::write(&receipt_path, receipt).unwrap();
    let query = fixture
        .command()
        .args([
            "query",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--query",
            "deterministic retrieval evidence",
        ])
        .output()
        .unwrap();
    assert_eq!(query.status.code(), Some(0));
}

#[test]
fn concurrent_same_generation_rebuilds_leave_one_receipt_equivalent_selected_slot() {
    let fixture = P3Fixture::new("cli-task16-index-concurrent");
    fixture.install_fixture_chain_with_one_applied_generation();
    let mut first = fixture.command();
    first.args([
        "index",
        "--profile",
        fixture.profile_arg(),
        "--store",
        fixture.store_arg(),
    ]);
    let mut second = fixture.command();
    second.args([
        "index",
        "--profile",
        fixture.profile_arg(),
        "--store",
        fixture.store_arg(),
    ]);
    first.stdout(std::process::Stdio::piped());
    first.stderr(std::process::Stdio::piped());
    second.stdout(std::process::Stdio::piped());
    second.stderr(std::process::Stdio::piped());
    let first_child = first.spawn().unwrap();
    let second_child = second.spawn().unwrap();
    let first = first_child.wait_with_output().unwrap();
    let second = second_child.wait_with_output().unwrap();
    for output in [&first, &second] {
        assert!(matches!(output.status.code(), Some(0) | Some(2)));
        if output.status.code() == Some(2) {
            assert_eq!(diagnostics(output)[0]["code"], "index.publish_failed");
        }
    }
    assert!(first.status.success() || second.status.success());
    let generation_root = fixture
        .store_root
        .join("derived/indexes/gen-0000000000000001");
    let pointer = std::fs::read_to_string(generation_root.join("CURRENT")).unwrap();
    assert_store_slot_pointer(&pointer);
    assert!(generation_root.join(pointer.trim()).is_dir());
    assert!(std::fs::read_dir(&generation_root).unwrap().all(|entry| {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        !name.starts_with(".slot-building-") && !name.starts_with("CURRENT.next-")
    }));
    let query = fixture
        .command()
        .args([
            "query",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--query",
            "deterministic retrieval evidence",
        ])
        .output()
        .unwrap();
    assert_eq!(
        query.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&query.stderr)
    );
}

#[test]
fn task16_verdict_level4_and_recovery_negative_matrix_is_explicit() {
    let fixture = P3Fixture::new("cli-task16-negative-matrix");
    let reason_file = fixture.input_file("reason.txt", b"reason from file");
    let rewrite_file = fixture.input_file("rewrite.md", b"rewrite bytes");
    let base = [
        "verdict",
        "prp-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "REJECT",
        "--profile",
        fixture.profile_arg(),
        "--store",
        fixture.store_arg(),
    ];
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (base.to_vec(), "cli.usage"),
        (
            [base.as_slice(), &["--reason", ""]].concat(),
            "human.reason_invalid",
        ),
        (
            [
                base.as_slice(),
                &["--reason-file", "/definitely/missing/private-reason-file"],
            ]
            .concat(),
            "human.reason_invalid",
        ),
        (
            [
                base.as_slice(),
                &[
                    "--reason",
                    "one",
                    "--reason-file",
                    reason_file.to_str().unwrap(),
                ],
            ]
            .concat(),
            "cli.usage",
        ),
        (
            vec![
                "verdict",
                "prp-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "rewrite",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
                "--reason",
                "reason",
            ],
            "verdict.verb_invalid",
        ),
        (
            vec![
                "verdict",
                "prp-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "UNKNOWN",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
                "--reason",
                "reason",
            ],
            "verdict.verb_invalid",
        ),
        (
            vec![
                "verdict",
                "prp-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "REWRITE",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
                "--reason",
                "reason",
            ],
            "human.rewrite_required",
        ),
        (
            vec![
                "verdict",
                "prp-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "REWRITE",
                "--profile",
                fixture.profile_arg(),
                "--store",
                fixture.store_arg(),
                "--reason",
                "reason",
                "--rewrite-file",
                "/definitely/missing/private-rewrite-file",
            ],
            "human.rewrite_invalid",
        ),
        (
            [
                base.as_slice(),
                &[
                    "--reason",
                    "reason",
                    "--rewrite-file",
                    rewrite_file.to_str().unwrap(),
                ],
            ]
            .concat(),
            "human.rewrite_forbidden",
        ),
        (
            [base.as_slice(), &["--reason", "reason"]].concat(),
            "proposal.not_found",
        ),
    ];
    for (args, code) in cases {
        let output = fixture.command().args(&args).output().unwrap();
        assert_eq!(diagnostics(&output)[0]["code"], code, "args={args:?}");
        assert!(!output.status.success());
    }

    let pregenesis = fixture
        .command()
        .args([
            "verify",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--level",
            "4",
        ])
        .output()
        .unwrap();
    assert_eq!(pregenesis.status.code(), Some(1));
    assert_eq!(
        diagnostics(&pregenesis)[0]["code"],
        "ledger.genesis_missing"
    );

    let synthesis_path = "01-Soul/Audy Himura.md";
    let synthesis_id = "ssa-0/01-Soul/Audy%20Himura.md";
    let synthesis_candidate = String::from_utf8(
        std::fs::read(fixture.store_root.join("seed/bundle").join(synthesis_path)).unwrap(),
    )
    .unwrap()
    .replace("ssa-0/01-Soul/Audy%20Himura", synthesis_id)
    .into_bytes();
    let synthesis_file = fixture.input_file("synthesis-rewrite.md", &synthesis_candidate);
    let proposal_id = stage_cli_rewrite_proposal(
        &fixture,
        "src:agent:intent-conflict",
        synthesis_id,
        synthesis_path,
        &synthesis_file,
    );
    let conflict = fixture
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
            "conflicting intent",
        ])
        .output()
        .unwrap();
    assert_eq!(diagnostics(&conflict)[0]["code"], "verdict.intent_conflict");

    let ambiguous = P3Fixture::new("cli-task16-recovery-ambiguous");
    for byte in ['a', 'b'] {
        let journal = ambiguous
            .store_root
            .join("transactions")
            .join(format!("txn-{}", byte.to_string().repeat(64)));
        std::fs::create_dir(&journal).unwrap();
        std::fs::write(journal.join("plan.json"), b"{}\n").unwrap();
    }
    let recovered = ambiguous
        .command()
        .args([
            "recover",
            "--profile",
            ambiguous.profile_arg(),
            "--store",
            ambiguous.store_arg(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        diagnostics(&recovered)[0]["code"],
        "transaction.recovery_ambiguous"
    );
    assert_eq!(recovered.status.code(), Some(1));
}

#[test]
fn changed_exact_replay_material_is_rejected_before_tty_or_mutation() {
    let fixture = P3Fixture::new("cli-task16-changed-replay");
    fixture.install_fixture_chain_with_one_applied_generation();
    let listed = fixture
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
    let value = fixture.parse_single_json_stdout(&listed);
    let proposal_id = value["verdicts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["verb"] == "ACCEPT")
        .unwrap()["proposal_id"]
        .as_str()
        .unwrap();
    let before = durable_hashes(&fixture.store_root);
    let output = fixture
        .command()
        .args([
            "verdict",
            proposal_id,
            "ACCEPT",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--reason",
            "changed replay reason",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(diagnostics(&output)[0]["code"], "verdict.already_ruled");
    assert_eq!(durable_hashes(&fixture.store_root), before);
}

#[test]
fn two_applied_v2_fixture_level4_contract_has_exact_counts_and_hash_shape() {
    let fixture = P3Fixture::new("cli-task10-two-applied-v2");
    fixture.install_fixture_chain_with_one_applied_generation();
    fixture.extend_fixture_chain_with_superseding_generation();
    let summary = fixture
        .verify_level4()
        .expect("verify v2 two-applied fixture");
    let output = fixture
        .command()
        .args([
            "verify",
            "--profile",
            fixture.profile_arg(),
            "--store",
            fixture.store_arg(),
            "--level",
            "4",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let rendered = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        rendered,
        format!(
            "{{\"status\":\"ok\",\"command\":\"verify\",\"level\":4,\"events\":4,\"verdicts\":3,\"accept\":1,\"reject\":1,\"rewrite\":1,\"tp\":0,\"fp\":0,\"applied_generations\":2,\"current_generation\":\"gen-0000000000000002\",\"bundle_hash\":\"{}\",\"ledger_head\":\"{}\",\"errors\":0}}\n",
            summary.bundle_hash, summary.ledger_head
        )
    );
    assert!(summary
        .bundle_hash
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()));
    assert!(summary
        .ledger_head
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()));
}
