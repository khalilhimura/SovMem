use clap::{Parser, Subcommand};
use serde::Serialize;
use sovmem_profile::{
    export_canary, export_corpus, load_profile, summarize_verified_bundle, validate_golden_file,
    verify_bundle, BundleStats, CorpusExportRequest, ExportError, ExportRequest, GoldenError,
    VerificationDiagnostic, VerificationLevel, VerificationOptions,
};
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

#[cfg(feature = "mcp")]
use crate::mcp;
use crate::profile as sovmem_profile;

mod p2;
mod p3;

#[derive(Parser)]
#[command(name = "sovmem", version, about = "Sovereign memory profile tools")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[allow(private_interfaces)]
#[derive(Subcommand)]
pub enum Command {
    Propose {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long, value_enum)]
        operation: ProposalOperationArg,
        #[arg(long, value_enum)]
        intent: ProposalIntentArg,
        #[arg(long)]
        target_id: String,
        #[arg(long)]
        target_path: String,
        #[arg(long)]
        candidate: PathBuf,
        #[arg(long)]
        confidence: Option<String>,
        #[arg(long)]
        evaluator: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    Intake {
        #[command(subcommand)]
        command: IntakeCommand,
    },
    Proposals {
        #[command(subcommand)]
        command: ProposalCommand,
    },
    Verdict {
        proposal_id: String,
        verb: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        store: PathBuf,
        #[arg(
            long,
            required_unless_present = "reason_file",
            conflicts_with = "reason_file"
        )]
        reason: Option<String>,
        #[arg(long, required_unless_present = "reason", conflicts_with = "reason")]
        reason_file: Option<PathBuf>,
        #[arg(long)]
        rewrite_file: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    Verdicts {
        #[command(subcommand)]
        command: VerdictCommand,
    },
    Recover {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        store: PathBuf,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    Index {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long, required_unless_present = "store", conflicts_with = "store")]
        bundle: Option<PathBuf>,
        #[arg(long, required_unless_present = "bundle", conflicts_with = "bundle")]
        store: Option<PathBuf>,
        #[arg(long)]
        index: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    Query {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        #[arg(long, required_unless_present = "store", conflicts_with = "store")]
        bundle: Option<PathBuf>,
        #[arg(long, required_unless_present = "bundle", conflicts_with = "bundle")]
        store: Option<PathBuf>,
        #[arg(long)]
        index: Option<PathBuf>,
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 5)]
        top_k: usize,
        #[arg(long, value_enum, default_value = "none")]
        expand: ExpansionArg,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    Eval {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        index: PathBuf,
        #[arg(long)]
        golden: PathBuf,
        #[arg(long)]
        thresholds: PathBuf,
        #[arg(long)]
        baseline: PathBuf,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    Export {
        #[arg(long)]
        vault: PathBuf,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        canary: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Corpus {
        #[arg(long)]
        vault: PathBuf,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Verify {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long, required_unless_present = "store", conflicts_with = "store")]
        bundle: Option<PathBuf>,
        #[arg(long, required_unless_present = "bundle", conflicts_with = "bundle")]
        store: Option<PathBuf>,
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4))]
        level: Option<u8>,
        #[arg(long)]
        require_corpus: bool,
    },
    Stats {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long, required_unless_present = "store", conflicts_with = "store")]
        bundle: Option<PathBuf>,
        #[arg(long, required_unless_present = "bundle", conflicts_with = "bundle")]
        store: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    VerifyGolden {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        golden: PathBuf,
    },
    #[cfg(feature = "mcp")]
    Serve {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        store: PathBuf,
    },
    Trust {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        store: PathBuf,
        #[arg(long, default_value_t = 50)]
        window: usize,
        #[arg(long)]
        focus: Option<String>,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    Audit {
        #[command(subcommand)]
        command: AuditCommand,
    },
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum OutputFormat {
    Json,
    Text,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum ExpansionArg {
    None,
    OneHop,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum ProposalOperationArg {
    Create,
    Replace,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum ProposalIntentArg {
    Accept,
    Rewrite,
}

#[derive(Subcommand)]
pub(crate) enum ProposalCommand {
    List {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        store: PathBuf,
        #[arg(long, value_enum)]
        state: Option<ProposalStateArg>,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
    Status {
        proposal_id: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        show_advice: bool,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
}

#[derive(Subcommand)]
pub(crate) enum IntakeCommand {
    Seal {
        submission_id: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        store: PathBuf,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
}

#[derive(Subcommand)]
pub(crate) enum VerdictCommand {
    List {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        verb: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        show_reasons: bool,
        #[arg(long)]
        show_advice: bool,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
}

#[derive(Subcommand)]
pub(crate) enum AuditCommand {
    Sample {
        #[arg(long)]
        store: PathBuf,
        #[arg(long, default_value_t = 5)]
        sample: usize,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum ProposalStateArg {
    Submitted,
    Staged,
    Ruled,
    Applied,
    Superseded,
    Retained,
    RejectedAtIntake,
}

#[derive(Serialize)]
struct ExportOutput {
    status: &'static str,
    command: &'static str,
    exported: usize,
    excluded: usize,
    warnings: usize,
    errors: usize,
    bundle_hash: String,
}

#[derive(Serialize)]
struct CorpusOutput {
    status: &'static str,
    command: &'static str,
    exported: usize,
    excluded: usize,
    core: usize,
    research: usize,
    warnings: usize,
    errors: usize,
    bundle_hash: String,
}

#[derive(Serialize)]
struct VerifyOutput {
    status: &'static str,
    command: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<u8>,
    documents: usize,
    warnings: usize,
    errors: usize,
    bundle_hash: String,
}

#[derive(Serialize)]
struct VerifyGoldenOutput {
    status: &'static str,
    command: &'static str,
    queries: usize,
    relevant_ids: usize,
    coverage: Vec<String>,
    errors: usize,
}

#[derive(Serialize)]
struct StatsOutput {
    status: &'static str,
    command: &'static str,
    #[serde(flatten)]
    stats: BundleStats,
}

pub fn run() -> i32 {
    let command = if selected_p2_subcommand() {
        match Cli::try_parse() {
            Ok(cli) => cli.command,
            Err(error) => return handle_p2_parse_error(error),
        }
    } else {
        Cli::parse().command
    };

    match command {
        Command::Propose {
            profile,
            store,
            source,
            operation,
            intent,
            target_id,
            target_path,
            candidate,
            confidence,
            evaluator,
            format,
        } => p3::run_propose(p3::ProposeArgs {
            profile,
            store,
            source,
            operation,
            intent,
            target_id,
            target_path,
            candidate,
            confidence,
            evaluator,
            format,
        }),
        Command::Intake { command } => match command {
            IntakeCommand::Seal {
                submission_id,
                profile,
                store,
                format,
            } => p3::run_intake_seal(p3::IntakeSealArgs {
                submission_id,
                profile,
                store,
                format,
            }),
        },
        Command::Proposals { command } => match command {
            ProposalCommand::List {
                profile,
                store,
                state,
                format,
            } => p3::run_proposals_list(profile, store, state, format),
            ProposalCommand::Status {
                proposal_id,
                profile,
                store,
                show_advice,
                format,
            } => p3::run_proposal_status(profile, store, proposal_id, show_advice, format),
        },
        Command::Verdict {
            proposal_id,
            verb,
            profile,
            store,
            reason,
            reason_file,
            rewrite_file,
            format,
        } => p3::run_verdict(p3::VerdictArgs {
            proposal_id,
            verb,
            profile,
            store,
            reason,
            reason_file,
            rewrite_file,
            format,
        }),
        Command::Verdicts { command } => match command {
            VerdictCommand::List {
                profile,
                store,
                verb,
                source,
                show_reasons,
                show_advice,
                format,
            } => p3::run_verdicts_list(
                profile,
                store,
                verb,
                source,
                show_reasons,
                show_advice,
                format,
            ),
        },
        Command::Recover {
            profile,
            store,
            format,
        } => p3::run_recover(profile, store, format),
        Command::Index {
            profile,
            bundle,
            store,
            index,
            format,
        } => match (bundle, store, index) {
            (Some(bundle), None, Some(index)) => p2::run_index(profile, bundle, index, format),
            (None, Some(store), None) => p3::run_store_index(profile, store, format),
            _ => p3::fail_usage("index storage mode is invalid"),
        },
        Command::Query {
            profile,
            bundle,
            store,
            index,
            query,
            top_k,
            expand,
            format,
        } => match (bundle, store, index) {
            (Some(bundle), None, Some(index)) => {
                p2::run_query(profile, bundle, index, query, top_k, expand, format)
            }
            (None, Some(store), None) => {
                p3::run_store_query(profile, store, query, top_k, expand, format)
            }
            _ => p3::fail_usage("query storage mode is invalid"),
        },
        Command::Eval {
            profile,
            bundle,
            index,
            golden,
            thresholds,
            baseline,
            format,
        } => p2::run_eval(profile, bundle, index, golden, thresholds, baseline, format),
        Command::Export {
            vault,
            profile,
            canary,
            output,
        } => run_export(vault, profile, canary, output),
        Command::Corpus {
            vault,
            profile,
            corpus,
            output,
        } => run_corpus(vault, profile, corpus, output),
        Command::Verify {
            profile,
            bundle,
            store,
            level,
            require_corpus,
        } => match (bundle, store) {
            (Some(bundle), None) => run_verify(profile, bundle, level, require_corpus),
            (None, Some(store)) => p3::run_verify_store(profile, store, level, require_corpus),
            _ => unreachable!("clap enforces exactly one verify storage input"),
        },
        Command::Stats {
            profile,
            bundle,
            store,
            format,
        } => match (bundle, store) {
            (Some(bundle), None) => run_stats(profile, bundle),
            (None, Some(store)) => p3::run_store_stats(profile, store, format),
            _ => unreachable!("clap enforces exactly one stats storage input"),
        },
        Command::VerifyGolden {
            profile,
            bundle,
            golden,
        } => run_verify_golden(profile, bundle, golden),
        #[cfg(feature = "mcp")]
        Command::Serve { profile, store } => mcp::run_server(profile, store),
        Command::Trust {
            profile,
            store,
            window,
            focus,
            output,
            format,
        } => p3::run_trust(p3::TrustArgs {
            profile,
            store,
            window,
            focus,
            output,
            format,
        }),
        Command::Audit { command } => match command {
            AuditCommand::Sample {
                store,
                sample,
                format,
            } => p3::run_audit_sample(store, sample, format),
        },
    }
}

fn selected_p2_subcommand() -> bool {
    std::env::args_os().nth(1).is_some_and(|argument| {
        argument == OsStr::new("index")
            || argument == OsStr::new("query")
            || argument == OsStr::new("eval")
            || argument == OsStr::new("propose")
            || argument == OsStr::new("proposals")
            || argument == OsStr::new("stats")
            || argument == OsStr::new("verdict")
            || argument == OsStr::new("verdicts")
            || argument == OsStr::new("recover")
            || argument == OsStr::new("verify")
    })
}

fn handle_p2_parse_error(error: clap::Error) -> i32 {
    match error.kind() {
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
            let exit = error.exit_code();
            let _ = error.print();
            exit
        }
        _ => {
            let message = if std::env::args_os().nth(1).is_some_and(|argument| {
                argument == OsStr::new("index")
                    || argument == OsStr::new("query")
                    || argument == OsStr::new("eval")
            }) {
                "P2 command usage is invalid"
            } else {
                "command usage is invalid"
            };
            emit_diagnostics(vec![VerificationDiagnostic::error("cli.usage", message)]);
            2
        }
    }
}

fn run_export(vault: PathBuf, profile: PathBuf, canary: PathBuf, output: PathBuf) -> i32 {
    match export_canary(ExportRequest {
        vault_root: vault,
        output_root: output,
        profile_path: profile,
        canary_path: canary,
    }) {
        Ok(summary) => {
            print_json(&ExportOutput {
                status: "ok",
                command: "export",
                exported: summary.exported,
                excluded: summary.excluded,
                warnings: summary.warnings,
                errors: summary.errors,
                bundle_hash: summary.bundle_hash,
            });
            0
        }
        Err(error) => {
            emit_diagnostics(export_diagnostics(&error));
            2
        }
    }
}

fn run_corpus(vault: PathBuf, profile: PathBuf, corpus: PathBuf, output: PathBuf) -> i32 {
    match export_corpus(CorpusExportRequest {
        vault_root: vault,
        output_root: output,
        profile_path: profile,
        corpus_path: corpus,
    }) {
        Ok(summary) => {
            print_json(&CorpusOutput {
                status: "ok",
                command: "corpus",
                exported: summary.exported,
                excluded: summary.excluded,
                core: summary.core,
                research: summary.research,
                warnings: summary.warnings,
                errors: summary.errors,
                bundle_hash: summary.bundle_hash,
            });
            0
        }
        Err(error) => {
            let exit_code = if matches!(error, ExportError::CorpusGate(_)) {
                1
            } else {
                2
            };
            emit_diagnostics(export_diagnostics(&error));
            exit_code
        }
    }
}

fn run_verify(
    profile_path: PathBuf,
    bundle: PathBuf,
    level: Option<u8>,
    require_corpus: bool,
) -> i32 {
    if level == Some(4) {
        emit_diagnostics(vec![VerificationDiagnostic::error(
            "cli.usage",
            "unsupported bundle verification level",
        )]);
        return 2;
    }
    let explicit_level = match level {
        Some(1) => Some(VerificationLevel::One),
        Some(2) => Some(VerificationLevel::Two),
        Some(3) => Some(VerificationLevel::Three),
        Some(_) => unreachable!("clap constrains levels"),
        None => None,
    };
    if require_corpus
        && matches!(
            explicit_level,
            Some(VerificationLevel::One | VerificationLevel::Two)
        )
    {
        emit_diagnostics(vec![VerificationDiagnostic::error(
            "verify.level_conflict",
            "corpus evidence requires verification level 3",
        )]);
        return 2;
    }

    let profile = match load_profile(&profile_path) {
        Ok(profile) => profile,
        Err(error) => {
            emit_diagnostics(vec![VerificationDiagnostic::error(
                error.code(),
                "profile configuration could not be loaded",
            )]);
            return 2;
        }
    };

    let bundle_root = match fs::canonicalize(&bundle) {
        Ok(path) if path.is_dir() => path,
        _ => {
            emit_diagnostics(vec![VerificationDiagnostic::error(
                "bundle.read",
                "bundle root could not be read",
            )]);
            return 2;
        }
    };
    if require_corpus && !bundle_root.join("corpus-report.json").is_file() {
        emit_diagnostics(vec![VerificationDiagnostic::error(
            "bundle.corpus_report_required",
            "required corpus evidence could not be read",
        )]);
        return 1;
    }

    let options = explicit_level.map_or_else(
        || VerificationOptions::compatibility(require_corpus),
        VerificationOptions::level,
    );
    let verification = verify_bundle(&profile, &bundle_root, options);
    let error_count = verification
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == "error")
        .count();
    let warning_count = verification.warning_count();
    if explicit_level.is_none() || error_count > 0 {
        emit_diagnostics(verification.diagnostics.clone());
    }

    if error_count > 0 {
        return 1;
    }

    print_json(&VerifyOutput {
        status: "ok",
        command: "verify",
        level: explicit_level.map(VerificationLevel::number),
        documents: verification.documents,
        warnings: warning_count,
        errors: 0,
        bundle_hash: verification.bundle_hash,
    });
    0
}

fn run_stats(profile_path: PathBuf, bundle: PathBuf) -> i32 {
    let profile = match load_profile(&profile_path) {
        Ok(profile) => profile,
        Err(error) => {
            emit_diagnostics(vec![VerificationDiagnostic::error(
                error.code(),
                "profile configuration could not be loaded",
            )]);
            return 2;
        }
    };
    let bundle_root = match fs::canonicalize(&bundle) {
        Ok(path) if path.is_dir() => path,
        _ => {
            emit_diagnostics(vec![VerificationDiagnostic::error(
                "bundle.read",
                "bundle root could not be read",
            )]);
            return 2;
        }
    };

    let verification = verify_bundle(
        &profile,
        &bundle_root,
        VerificationOptions::compatibility(false),
    );
    if verification.has_errors() {
        emit_diagnostics(verification.diagnostics.clone());
        return 1;
    }
    let stats = summarize_verified_bundle(&verification)
        .expect("successful verification always produces statistics");
    print_json(&StatsOutput {
        status: "ok",
        command: "stats",
        stats,
    });
    0
}

fn run_verify_golden(profile_path: PathBuf, bundle: PathBuf, golden: PathBuf) -> i32 {
    let profile = match load_profile(&profile_path) {
        Ok(profile) => profile,
        Err(error) => {
            emit_diagnostics(vec![VerificationDiagnostic::error(
                error.code(),
                "profile configuration could not be loaded",
            )]);
            return 2;
        }
    };
    let bundle_root = match fs::canonicalize(&bundle) {
        Ok(path) if path.is_dir() => path,
        _ => {
            emit_diagnostics(vec![VerificationDiagnostic::error(
                "bundle.read",
                "bundle root could not be read",
            )]);
            return 2;
        }
    };
    if !golden.is_file() {
        emit_diagnostics(vec![VerificationDiagnostic::error(
            GoldenError::Read.code(),
            "golden query file could not be read",
        )]);
        return 2;
    }
    if !bundle_root.join("manifest.json").is_file()
        || !bundle_root.join("corpus-report.json").is_file()
    {
        emit_diagnostics(vec![VerificationDiagnostic::error(
            GoldenError::Bundle.code(),
            "golden query evidence bundle could not be read",
        )]);
        return 2;
    }

    match validate_golden_file(&profile, &bundle_root, &golden) {
        Ok(summary) => {
            print_json(&VerifyGoldenOutput {
                status: "ok",
                command: "verify-golden",
                queries: summary.queries,
                relevant_ids: summary.relevant_ids,
                coverage: summary.coverage,
                errors: 0,
            });
            0
        }
        Err(error) => {
            emit_diagnostics(vec![VerificationDiagnostic::error(
                error.code(),
                "golden query validation failed",
            )]);
            1
        }
    }
}

fn export_diagnostics(error: &ExportError) -> Vec<VerificationDiagnostic> {
    let mut diagnostics = vec![VerificationDiagnostic::error(error.code(), "export failed")];
    diagnostics.extend(
        error
            .diagnostics()
            .iter()
            .map(VerificationDiagnostic::from_validation),
    );
    diagnostics
}

fn emit_diagnostics(mut diagnostics: Vec<VerificationDiagnostic>) {
    diagnostics.sort();
    diagnostics.dedup();
    for diagnostic in diagnostics {
        eprintln!(
            "{}",
            serde_json::to_string(&diagnostic)
                .expect("diagnostic JSON serialization is infallible")
        );
    }
}

fn print_json(value: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string(value).expect("summary JSON serialization is infallible")
    );
}
