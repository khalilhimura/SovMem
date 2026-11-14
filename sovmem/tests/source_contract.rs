use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
mod support;

use support::p2::FrozenP2RawSnapshot;
use support::p3::P3CliFixture;

fn repository_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative)
}

fn rust_sources(root: &Path) -> Vec<(PathBuf, String)> {
    fn collect(root: &Path, paths: &mut Vec<PathBuf>) {
        let mut entries = fs::read_dir(root)
            .expect("read source directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read source entry");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("read source metadata");
            if metadata.is_dir() {
                collect(&path, paths);
            } else if metadata.is_file()
                && path.extension().is_some_and(|extension| extension == "rs")
            {
                paths.push(path);
            }
        }
    }

    let mut paths = Vec::new();
    collect(root, &mut paths);
    paths
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path).expect("read Rust source");
            (path, strip_comments_and_literals(&source))
        })
        .collect()
}

fn strip_comments_and_literals(source: &str) -> String {
    let characters = source.chars().collect::<Vec<_>>();
    let mut code = String::with_capacity(source.len());
    let mut index = 0;

    while index < characters.len() {
        let raw_start = match (characters[index], characters.get(index + 1).copied()) {
            ('r', _) => Some(index),
            ('b', Some('r')) => Some(index + 1),
            _ => None,
        };
        if let Some(raw_start) = raw_start {
            let mut quote = raw_start + 1;
            while characters.get(quote) == Some(&'#') {
                quote += 1;
            }
            if characters.get(quote) == Some(&'"') {
                let hashes = quote - raw_start - 1;
                code.push(' ');
                index = quote + 1;
                while index < characters.len() {
                    if characters[index] == '"'
                        && (0..hashes)
                            .all(|offset| characters.get(index + offset + 1) == Some(&'#'))
                    {
                        index += hashes + 1;
                        break;
                    }
                    if characters[index] == '\n' {
                        code.push('\n');
                    }
                    index += 1;
                }
                continue;
            }
        }
        match (characters[index], characters.get(index + 1).copied()) {
            ('/', Some('/')) => {
                code.push(' ');
                index += 2;
                while index < characters.len() && characters[index] != '\n' {
                    index += 1;
                }
            }
            ('/', Some('*')) => {
                code.push(' ');
                index += 2;
                let mut depth = 1;
                while index < characters.len() && depth > 0 {
                    match (characters[index], characters.get(index + 1).copied()) {
                        ('/', Some('*')) => {
                            depth += 1;
                            index += 2;
                        }
                        ('*', Some('/')) => {
                            depth -= 1;
                            index += 2;
                        }
                        ('\n', _) => {
                            code.push('\n');
                            index += 1;
                        }
                        _ => index += 1,
                    }
                }
            }
            ('"', _) | ('b', Some('"')) => {
                code.push(' ');
                index += usize::from(characters[index] == 'b');
                index += 1;
                while index < characters.len() {
                    match characters[index] {
                        '\\' => index += 2,
                        '"' => {
                            index += 1;
                            break;
                        }
                        '\n' => {
                            code.push('\n');
                            index += 1;
                        }
                        _ => index += 1,
                    }
                }
            }
            ('\'', Some('\\')) if characters.get(index + 3) == Some(&'\'') => {
                code.push(' ');
                index += 4;
            }
            ('\'', _) if characters.get(index + 2) == Some(&'\'') => {
                code.push(' ');
                index += 3;
            }
            _ => {
                code.push(characters[index]);
                index += 1;
            }
        }
    }

    code
}

fn function_call_count(source: &str, name: &str) -> usize {
    std::iter::once(name.to_owned())
        .chain(imported_aliases(source, name))
        .map(|reference| function_call_count_for_reference(source, &reference))
        .sum()
}

fn function_call_count_for_reference(source: &str, reference: &str) -> usize {
    source
        .match_indices(reference)
        .filter(|(offset, _)| {
            let prefix = &source[..*offset];
            let suffix = &source[*offset + reference.len()..];
            let preceding_is_identifier = prefix
                .chars()
                .next_back()
                .is_some_and(is_identifier_character);
            !preceding_is_identifier
                && suffix.trim_start().starts_with('(')
                && !prefix.trim_end().ends_with("fn")
        })
        .count()
}

fn imported_aliases(source: &str, symbol: &str) -> Vec<String> {
    source
        .match_indices(symbol)
        .filter_map(|(offset, _)| {
            let statement_start = source[..offset].rfind(';').map_or(0, |index| index + 1);
            let statement_end = source[offset..]
                .find(';')
                .map_or(source.len(), |index| offset + index);
            let statement = source[statement_start..statement_end].trim();
            let (_, import) = use_statement_parts(statement)?;
            let import_offset = statement.len() - import.len();
            let symbol_offset = offset - statement_start;
            if symbol_offset < import_offset {
                return None;
            }

            let suffix = &source[offset + symbol.len()..statement_end];
            let alias = suffix.trim_start().strip_prefix("as ")?;
            let alias = alias.trim_start();
            let alias_length = alias
                .chars()
                .take_while(|character| is_identifier_character(*character))
                .map(char::len_utf8)
                .sum();
            (alias_length > 0).then(|| alias[..alias_length].to_owned())
        })
        .collect()
}

fn use_statement_parts(statement: &str) -> Option<(bool, &str)> {
    let statement = statement.trim();
    if let Some(import) = statement.strip_prefix("use ") {
        return Some((false, import));
    }
    if let Some(import) = statement.strip_prefix("pub use ") {
        return Some((true, import));
    }
    let visibility = statement.strip_prefix("pub(")?;
    let closing = visibility.find(')')?;
    let import = visibility[closing + 1..]
        .trim_start()
        .strip_prefix("use ")?;
    Some((true, import))
}

#[derive(Debug)]
struct ParsedUseStatement {
    start: usize,
    end: usize,
    is_reexport: bool,
    is_public: bool,
    import: String,
    statement: String,
}

fn parsed_use_statements(source: &str) -> Vec<ParsedUseStatement> {
    let source = strip_comments_and_literals(source);
    let mut statements = Vec::new();
    let mut search_start = 0;

    while let Some(relative_offset) = source[search_start..].find("use") {
        let use_offset = search_start + relative_offset;
        let prefix = &source[..use_offset];
        let suffix = &source[use_offset + "use".len()..];
        if prefix
            .chars()
            .next_back()
            .is_some_and(is_identifier_character)
            || suffix.chars().next().is_some_and(is_identifier_character)
        {
            search_start = use_offset + "use".len();
            continue;
        }
        let Some(relative_end) = suffix.find(';') else {
            break;
        };
        let end = use_offset + "use".len() + relative_end;
        let (start, is_reexport, is_public) = prefix
            .rfind("pub")
            .filter(|offset| {
                !prefix[..*offset]
                    .chars()
                    .next_back()
                    .is_some_and(is_identifier_character)
            })
            .and_then(|offset| {
                let visibility = source[offset..use_offset].replace(char::is_whitespace, "");
                (visibility == "pub"
                    || (visibility.starts_with("pub(") && visibility.ends_with(')')))
                .then_some((offset, visibility == "pub"))
            })
            .map_or((use_offset, false, false), |(start, is_public)| {
                (start, true, is_public)
            });
        statements.push(ParsedUseStatement {
            start,
            end,
            is_reexport,
            is_public,
            import: source[use_offset + "use".len()..end].trim().to_owned(),
            statement: source[start..end].trim().to_owned(),
        });
        search_start = end + 1;
    }

    statements
}

fn is_identifier_character(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

#[test]
fn lexical_call_scanner_counts_comment_separated_calls_and_ignores_raw_string_false_positives() {
    assert_eq!(
        function_call_count(
            &strip_comments_and_literals("stage_generation /* comment */ (request);"),
            "stage_generation",
        ),
        1
    );
    assert_eq!(
        function_call_count(
            &strip_comments_and_literals(
                "let literal = r###\"a raw quote \" stage_generation( remains literal\"###;",
            ),
            "stage_generation",
        ),
        0
    );
}

#[test]
fn lexical_call_scanner_resolves_writer_aliases_and_reexports() {
    let source = strip_comments_and_literals(
        "use crate::profile::{commit_human_verdict as verdict, stage_generation as stage};\n\
         pub use crate::profile::recover_store_human as recover;\n\
         fn unapproved_writer_route() { verdict(); stage(); recover(); }",
    );

    assert_eq!(function_call_count(&source, "commit_human_verdict"), 1);
    assert_eq!(function_call_count(&source, "stage_generation"), 1);
    assert_eq!(function_call_count(&source, "recover_store_human"), 1);
}

#[test]
fn lexical_writer_scanner_rejects_visibility_qualified_aliases_and_reexports() {
    let source = strip_comments_and_literals(
        "pub(crate) use crate::profile::commit_human_verdict as verdict;\n\
         pub(super) use crate::profile::stage_generation as stage;\n\
         pub(in crate::profile) use crate::profile::recover_store_human;\n\
         fn unapproved_writer_route() { verdict(); stage(); recover_store_human(); }",
    );

    assert_eq!(function_call_count(&source, "commit_human_verdict"), 1);
    assert_eq!(function_call_count(&source, "stage_generation"), 1);
    assert_eq!(function_call_count(&source, "recover_store_human"), 1);
    assert_eq!(
        writer_aliases_or_public_reexports(&source),
        vec![
            "pub(crate) use crate::profile::commit_human_verdict as verdict".to_owned(),
            "pub(in crate::profile) use crate::profile::recover_store_human".to_owned(),
            "pub(super) use crate::profile::stage_generation as stage".to_owned(),
        ]
    );
}

#[test]
fn lexical_authority_scanner_matches_forbidden_components_inside_identifiers() {
    let source = strip_comments_and_literals(
        "let mcp_server = (); let git_helper = (); let trust_gate = (); let auto_accept_write = (); \
         let McpServer = (); let GitHelper = (); let TrustGate = (); let AutoAcceptWrite = ();",
    );
    for forbidden in ["mcp", "git", "trust", "auto_accept"] {
        assert!(
            contains_identifier(&source, forbidden),
            "compound identifier must retain forbidden component {forbidden}"
        );
    }
    assert!(contains_forbidden_authority_component(&source, "git"));
    assert!(contains_forbidden_authority_component(
        "let command = Command::new(\"git\");",
        "git"
    ));
    assert!(
        !contains_forbidden_authority_component("let git_date = ();", "git"),
        "P2 provenance metadata is not a Git authority route"
    );
    assert!(
        !contains_forbidden_authority_component(
            "// Command::new(\"git\")\nlet prose = \"Command::new(\\\"git\\\")\";",
            "git"
        ),
        "comments and ordinary literals are not Git authority routes"
    );

    let false_positives = strip_comments_and_literals(
        "// mcp_server\nlet literal = \"git_helper trust_gate auto_accept_write\";",
    );
    for forbidden in ["mcp", "git", "trust", "auto_accept"] {
        assert!(
            !contains_identifier(&false_positives, forbidden),
            "comments and literals must not create a forbidden component {forbidden}"
        );
    }
}

#[test]
fn production_authority_scan_observes_git_literals_before_stripping() {
    let violations = production_authority_violations(
        "// Command::new(\"git\")\nlet literal = \"Command::new(\\\"git\\\")\";\nCommand::new(\"git\");",
    );

    assert_eq!(violations, vec!["git".to_owned()]);
}

#[test]
fn production_authority_scan_rejects_every_direct_process_construction() {
    for executable in ["\"/usr/bin/git\"", "\"git.exe\"", "executable"] {
        let source = format!("fn prohibited_route() {{ let _ = Command::new({executable}); }}");
        assert_eq!(
            production_authority_violations(&source),
            vec!["git".to_owned()],
            "direct process construction must be prohibited regardless of executable spelling"
        );
    }

    assert!(production_authority_violations(
        "// Command::new(\"git\")\nlet prose = \"Command::new(executable)\";"
    )
    .is_empty());
}

#[test]
fn production_authority_scan_rejects_process_imports_aliases_and_qualified_routes() {
    for source in [
        "use std::process::Command as Process; fn prohibited_route() { Process::new(\"git\"); }",
        "fn prohibited_route() { std::process::Command::new(\"git\"); }",
        "use std::process; fn prohibited_route() { process::Command::new(\"git\"); }",
        "use std::{process::Command as Process}; fn prohibited_route() { Process::new(\"git\"); }",
        "use std::process::{Command as Process}; fn prohibited_route() { Process::new(\"git\"); }",
    ] {
        assert_eq!(
            production_authority_violations(source),
            vec!["git".to_owned()],
            "any std::process import or path must be prohibited in P3 production"
        );
    }

    assert!(production_authority_violations(
        "// use std::process::Command as Process; Process::new(\"git\");\nlet prose = \"std::process\";"
    )
    .is_empty());
}

#[test]
fn lexical_command_scanner_balances_only_executable_braces() {
    let source = r##"
        fn run_propose() {
            // } comment must not close the function
            let string = "} literal_writer";
            let byte_string = b"} literal_writer";
            let raw = r#"} literal_writer"#;
            let byte_raw = br#"} literal_writer"#;
            let character = '}';
            commit_human_verdict();
        }
    "##;

    let body = function_body(source, "run_propose");
    assert!(body.contains("commit_human_verdict"));
    assert!(!body.contains("comment must not close"));
    assert!(!body.contains("literal_writer"));
}

fn source_for<'a>(sources: &'a [(PathBuf, String)], suffix: &str) -> &'a str {
    sources
        .iter()
        .find(|(path, _)| path.to_string_lossy().ends_with(suffix))
        .map(|(_, source)| source.as_str())
        .expect("required source file is scanned")
}

fn p3_production_paths() -> Vec<PathBuf> {
    [
        "sovmem/src/lib.rs",
        "sovmem/src/cli.rs",
        "sovmem/src/cli/p3.rs",
        "sovmem/src/profile/canon.rs",
        "sovmem/src/profile/clock.rs",
        "sovmem/src/profile/config.rs",
        "sovmem/src/profile/frontmatter.rs",
        "sovmem/src/profile/graph.rs",
        "sovmem/src/profile/index.rs",
        "sovmem/src/profile/judgment.rs",
        "sovmem/src/profile/ledger.rs",
        "sovmem/src/profile/links.rs",
        "sovmem/src/profile/mapping.rs",
        "sovmem/src/profile/mod.rs",
        "sovmem/src/profile/policy.rs",
        "sovmem/src/profile/proposal.rs",
        "sovmem/src/profile/query.rs",
        "sovmem/src/profile/stats.rs",
        "sovmem/src/profile/store.rs",
        "sovmem/src/profile/thresholds.rs",
        "sovmem/src/profile/transaction.rs",
        "sovmem/src/profile/validate.rs",
        "sovmem/src/profile/verify.rs",
    ]
    .into_iter()
    .map(repository_path)
    .collect()
}

fn p3_shared_utility_boundary() -> Vec<(PathBuf, Vec<String>)> {
    vec![
        (
            repository_path("sovmem/src/profile/export.rs"),
            vec![
                "escape_markdown_label".to_owned(),
                "hash_bundle".to_owned(),
                "percent_encode_path".to_owned(),
                "render_index".to_owned(),
            ],
        ),
        (
            repository_path("sovmem/src/profile/source.rs"),
            vec!["ExcludedSource".to_owned(), "SourceDocument".to_owned()],
        ),
        (
            repository_path("sovmem/src/profile/frontmatter.rs"),
            vec![
                "contains_secret".to_owned(),
                "parse_source_document".to_owned(),
                "render_document".to_owned(),
                "split_frontmatter".to_owned(),
            ],
        ),
    ]
}

fn approved_shared_utility_roots(path: &Path) -> Vec<&'static str> {
    let relative = path
        .strip_prefix(repository_path("sovmem/src/profile"))
        .expect("shared utility remains under the profile root");
    match relative.to_string_lossy().as_ref() {
        "export.rs" => vec!["hash_bundle", "render_index"],
        "source.rs" => Vec::new(),
        "frontmatter.rs" => vec!["parse_source_document", "render_document"],
        _ => panic!(
            "shared utility has an explicit P3 root set: {}",
            path.display()
        ),
    }
}

fn named_item_body(source: &str, name: &str) -> String {
    let source = strip_comments_and_literals(source);
    let mut candidates = Vec::new();
    for prefix in ["fn ", "struct ", "enum "] {
        let marker = format!("{prefix}{name}");
        candidates.extend(
            source
                .match_indices(&marker)
                .map(|(offset, _)| (offset, marker.len())),
        );
    }
    let start = candidates
        .into_iter()
        .find(|(offset, marker_length)| {
            source[*offset + marker_length..]
                .chars()
                .next()
                .is_none_or(|character| !is_identifier_character(character))
        })
        .map(|(offset, _)| offset)
        .unwrap_or_else(|| panic!("required approved shared utility {name}"));
    let body_start = source[start..]
        .find('{')
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("approved shared utility {name} has a body"));
    let mut depth = 0usize;
    for (offset, character) in source[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return source[body_start..=body_start + offset].to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("approved shared utility {name} body is balanced")
}

fn local_function_call_closure(source: &str, roots: &[&str]) -> BTreeSet<String> {
    let available = function_names(source).into_iter().collect::<BTreeSet<_>>();
    let mut closure = BTreeSet::new();
    let mut pending = roots
        .iter()
        .map(|root| (*root).to_owned())
        .collect::<BTreeSet<_>>();

    while let Some(function) = pending.pop_first() {
        assert!(
            available.contains(&function),
            "approved shared utility root must be locally defined: {function}"
        );
        if !closure.insert(function.clone()) {
            continue;
        }
        let body = named_item_body(source, &function);
        for candidate in &available {
            if function_call_count_for_reference(&body, candidate) > 0
                && !closure.contains(candidate)
            {
                pending.insert(candidate.clone());
            }
        }
    }

    closure
}

const P2_ONLY_SHARED_SYMBOLS: [&str; 11] = [
    "export_canary",
    "export_corpus",
    "ExportRequest",
    "ExportSummary",
    "CorpusExportRequest",
    "CorpusExportSummary",
    "ExportError",
    "open_committed_snapshot",
    "read_only_git",
    "SourceSnapshot",
    "SourceError",
];

fn direct_shared_module_symbols(source: &str, module: &str) -> Vec<String> {
    let marker = format!("{module}::");
    source
        .match_indices(&marker)
        .filter_map(|(offset, _)| {
            let prefix = &source[..offset];
            (!prefix
                .chars()
                .next_back()
                .is_some_and(is_identifier_character))
            .then(|| {
                let suffix = &source[offset + marker.len()..];
                let length = suffix
                    .chars()
                    .take_while(|character| is_identifier_character(*character))
                    .map(char::len_utf8)
                    .sum();
                suffix[..length].to_owned()
            })
        })
        .filter(|symbol| !symbol.is_empty())
        .collect()
}

fn is_legal_profile_glob_reexport(import: &str) -> bool {
    let import = import.replace(char::is_whitespace, "");
    let Some(module) = import.strip_suffix("::*") else {
        return false;
    };
    [
        "canon",
        "clock",
        "config",
        "corpus",
        "corpus_report",
        "eval",
        "export",
        "frontmatter",
        "golden",
        "graph",
        "index",
        "judgment",
        "ledger",
        "links",
        "mapping",
        "policy",
        "proposal",
        "query",
        "source",
        "stats",
        "store",
        "thresholds",
        "validate",
        "verify",
    ]
    .contains(&module)
}

fn profile_mod_route_contract_source(source: &str) -> String {
    let mut source = strip_comments_and_literals(source);
    let statements = parsed_use_statements(&source);
    for statement in statements.into_iter().rev() {
        if statement.is_public && is_legal_profile_glob_reexport(&statement.import) {
            source.replace_range(statement.start..statement.end + 1, "");
        }
    }
    source
}

fn p3_shared_route_violations(path: &Path, source: &str) -> Vec<String> {
    let is_profile_module = path == repository_path("sovmem/src/profile/mod.rs");
    let source = if is_profile_module {
        profile_mod_route_contract_source(source)
    } else {
        strip_comments_and_literals(source)
    };
    let boundary = p3_shared_utility_boundary();
    let approved_symbols = boundary
        .iter()
        .flat_map(|(_, symbols)| symbols)
        .collect::<BTreeSet<_>>();
    let mut violations = Vec::new();
    for (path, _) in &boundary {
        let module = path
            .file_stem()
            .and_then(|name| name.to_str())
            .expect("shared utility has a Rust module name");
        for symbol in direct_shared_module_symbols(&source, module) {
            if P2_ONLY_SHARED_SYMBOLS.contains(&symbol.as_str())
                || !approved_symbols.contains(&symbol)
            {
                violations.push(symbol);
            }
        }
    }
    violations.extend(
        module_root_aliases(&source)
            .into_iter()
            .map(|alias| format!("root module alias: {alias}")),
    );
    violations.extend(profile_reexported_p2_symbols(&source));
    violations.extend(
        direct_shared_module_symbols(&source, "profile")
            .into_iter()
            .filter(|symbol| P2_ONLY_SHARED_SYMBOLS.contains(&symbol.as_str())),
    );
    for alias in shared_surface_module_aliases(&source) {
        violations.extend(
            direct_shared_module_symbols(&source, &alias)
                .into_iter()
                .filter(|symbol| P2_ONLY_SHARED_SYMBOLS.contains(&symbol.as_str())),
        );
    }
    if imports_shared_surface_glob(&source) {
        violations.extend(
            P2_ONLY_SHARED_SYMBOLS
                .into_iter()
                .filter(|symbol| contains_identifier_token(&source, symbol))
                .map(str::to_owned),
        );
    }
    if is_profile_module {
        violations.extend(
            P2_ONLY_SHARED_SYMBOLS
                .into_iter()
                .filter(|symbol| contains_identifier_token(&source, symbol))
                .map(str::to_owned),
        );
    }
    violations.sort();
    violations.dedup();
    violations
}

fn shared_utility_contract_violations(
    path: &Path,
    source: &str,
    symbols: &BTreeSet<String>,
) -> Vec<String> {
    let mut violations = Vec::new();
    for symbol in symbols {
        let body = named_item_body(source, symbol);
        violations.extend(
            production_authority_violations(&body)
                .into_iter()
                .map(|violation| format!("{symbol}: {violation}")),
        );
        violations.extend(
            p3_shared_route_violations(path, &body)
                .into_iter()
                .map(|violation| format!("{symbol}: {violation}")),
        );
    }
    violations.sort();
    violations.dedup();
    violations
}

fn aliases_after_import_component(import: &str, component: &str) -> Vec<String> {
    import
        .match_indices(component)
        .filter_map(|(offset, _)| {
            let prefix = &import[..offset];
            let suffix = &import[offset + component.len()..];
            if prefix
                .chars()
                .next_back()
                .is_some_and(is_identifier_character)
                || suffix.chars().next().is_some_and(is_identifier_character)
            {
                return None;
            }
            let alias = suffix.trim_start().strip_prefix("as ")?.trim_start();
            let length = alias
                .chars()
                .take_while(|character| is_identifier_character(*character))
                .map(char::len_utf8)
                .sum();
            (length > 0).then(|| alias[..length].to_owned())
        })
        .collect()
}

fn shared_surface_module_aliases(source: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    for statement in source
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
    {
        let Some((_, import)) = use_statement_parts(statement) else {
            continue;
        };
        let is_profile_surface = contains_identifier_token(import, "profile")
            || import.trim_start().starts_with("super::");
        if !is_profile_surface {
            continue;
        }
        for component in ["self", "profile", "export", "source"] {
            aliases.extend(aliases_after_import_component(import, component));
        }
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

fn module_root_aliases(source: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    for statement in source
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
    {
        let Some((_, import)) = use_statement_parts(statement) else {
            continue;
        };
        let import = import.trim();
        for root in ["super", "crate"] {
            if let Some(alias) = alias_after_root_import(import, root) {
                aliases.push(alias);
            }
            if let Some(group) = import
                .strip_prefix(&format!("{root}::{{"))
                .and_then(|group| group.strip_suffix('}'))
            {
                aliases.extend(
                    group
                        .split(',')
                        .filter_map(|item| alias_after_root_import(item.trim(), "self")),
                );
            }
        }
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

fn alias_after_root_import(import: &str, root: &str) -> Option<String> {
    let alias = import
        .strip_prefix(root)?
        .trim_start()
        .strip_prefix("as ")?
        .trim_start();
    let length = alias
        .chars()
        .take_while(|character| is_identifier_character(*character))
        .map(char::len_utf8)
        .sum();
    (length > 0).then(|| alias[..length].to_owned())
}

fn imports_shared_surface_glob(source: &str) -> bool {
    source
        .split(';')
        .map(str::trim)
        .filter_map(use_statement_parts)
        .map(|(_, import)| import)
        .any(|import| {
            let import = import.replace(char::is_whitespace, "");
            import.starts_with("super::*")
                || import.contains("profile::*")
                || (import.contains("profile")
                    && (import.contains("export::*") || import.contains("source::*")))
        })
}

fn profile_reexported_p2_symbols(source: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    for statement in source
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
    {
        let Some((_, import)) = use_statement_parts(statement) else {
            continue;
        };
        if !import.starts_with("crate::profile::") && !import.starts_with("super::") {
            continue;
        }
        for symbol in P2_ONLY_SHARED_SYMBOLS {
            if contains_identifier_token(import, symbol) {
                symbols.push(symbol.to_owned());
            }
        }
    }
    symbols.sort();
    symbols.dedup();
    symbols
}

#[test]
fn p3_production_path_set_defers_shared_utilities_to_the_explicit_boundary() {
    let paths = p3_production_paths();
    for excluded in [
        "sovmem/src/profile/corpus.rs",
        "sovmem/src/profile/p3_internal/p3.rs",
        "sovmem/src/profile/p3_test_support.rs",
    ] {
        assert!(
            !paths.contains(&repository_path(excluded)),
            "P2-only or test-only source must not be treated as P3 production: {excluded}"
        );
    }
    for shared in [
        "sovmem/src/profile/export.rs",
        "sovmem/src/profile/source.rs",
    ] {
        assert!(
            !paths.contains(&repository_path(shared)),
            "shared P2/P3 utility must be checked through its explicit symbol boundary: {shared}"
        );
    }
    assert!(paths.contains(&repository_path("sovmem/src/profile/mapping.rs")));
    assert!(paths.contains(&repository_path("sovmem/src/profile/validate.rs")));
}

#[test]
fn shared_p2_utilities_expose_only_the_p3_reachable_boundary() {
    let boundary = p3_shared_utility_boundary();

    assert_eq!(
        boundary,
        vec![
            (
                repository_path("sovmem/src/profile/export.rs"),
                vec![
                    "escape_markdown_label".to_owned(),
                    "hash_bundle".to_owned(),
                    "percent_encode_path".to_owned(),
                    "render_index".to_owned(),
                ],
            ),
            (
                repository_path("sovmem/src/profile/source.rs"),
                vec!["ExcludedSource".to_owned(), "SourceDocument".to_owned()],
            ),
            (
                repository_path("sovmem/src/profile/frontmatter.rs"),
                vec![
                    "contains_secret".to_owned(),
                    "parse_source_document".to_owned(),
                    "render_document".to_owned(),
                    "split_frontmatter".to_owned(),
                ],
            ),
        ]
    );
}

#[test]
fn complete_live_p3_shared_type_boundary_is_explicit_or_whole_file_scanned() {
    let boundary = p3_shared_utility_boundary();
    let source_types = boundary
        .iter()
        .find(|(path, _)| *path == repository_path("sovmem/src/profile/source.rs"))
        .expect("source is a deferred shared utility")
        .1
        .clone();
    assert_eq!(
        source_types,
        vec!["ExcludedSource".to_owned(), "SourceDocument".to_owned()],
        "every P3-reachable source type must be explicit when source.rs is deferred from whole-file scanning"
    );

    let frontmatter = repository_path("sovmem/src/profile/frontmatter.rs");
    assert!(
        p3_production_paths().contains(&frontmatter),
        "frontmatter is whole-file authority scanned rather than deferred"
    );
    let source = fs::read_to_string(&frontmatter).expect("read whole-file-scanned frontmatter");
    for symbol in [
        "GeneratedDocument",
        "GeneratedMetadata",
        "ParsedSource",
        "SourceKind",
        "Provenance",
        "Status",
        "LinkRelation",
        "TypedLink",
    ] {
        let definition = named_item_body(&source, symbol);
        assert!(
            production_authority_violations(&definition).is_empty(),
            "whole-file-scanned P3 type {symbol} has prohibited authority construct(s)"
        );
    }
}

#[test]
fn shared_p2_utility_bodies_and_p3_call_sites_stay_inside_the_approved_boundary() {
    for (path, symbols) in p3_shared_utility_boundary() {
        let source = fs::read_to_string(&path).expect("read shared utility source");
        let symbols = symbols.into_iter().collect::<BTreeSet<_>>();
        assert!(
            shared_utility_contract_violations(&path, &source, &symbols).is_empty(),
            "approved P3 shared utility has prohibited authority or shared-route construct(s) {:?}: {}",
            shared_utility_contract_violations(&path, &source, &symbols),
            path.display()
        );
    }

    for relative in [
        "sovmem/src/profile/canon.rs",
        "sovmem/src/profile/judgment.rs",
        "sovmem/src/profile/policy.rs",
        "sovmem/src/profile/verify.rs",
    ] {
        let source = strip_comments_and_literals(
            &fs::read_to_string(repository_path(relative)).expect("read P3 shared utility caller"),
        );
        for forbidden in [
            "export_canary",
            "export_corpus",
            "open_committed_snapshot",
            "read_only_git",
            "SourceSnapshot",
            "SourceError",
        ] {
            assert!(
                !contains_identifier_token(&source, forbidden),
                "P3 shared utility caller {relative} may not reach P2-only {forbidden}"
            );
        }
    }
}

#[test]
fn every_p3_production_path_rejects_p2_only_and_unapproved_shared_routes() {
    let representative_path = repository_path("sovmem/src/profile/canon.rs");
    let representative_unapproved = "\
        use crate::profile::export::export_canary;\n\
        fn forbidden_p2_route() { export_canary(); }\n";
    assert_eq!(
        p3_shared_route_violations(&representative_path, representative_unapproved),
        vec!["export_canary".to_owned()],
        "an ordinary P3 path must reject a P2-only shared export route"
    );

    let representative_unapproved_helper =
        "fn forbidden_helper_route() { crate::profile::export::new_local_helper(); }";
    assert_eq!(
        p3_shared_route_violations(&representative_path, representative_unapproved_helper),
        vec!["new_local_helper".to_owned()],
        "an ordinary P3 path must reject direct shared-module routing outside the approved surface"
    );

    for path in p3_production_paths() {
        let source = fs::read_to_string(&path).expect("read P3 production source");
        assert!(
            p3_shared_route_violations(&path, &source).is_empty(),
            "P3 production path may not reach P2-only or unapproved shared routing: {}",
            path.display()
        );
    }
}

#[test]
fn all_p3_path_contract_rejects_direct_profile_reexport_of_p2_only_symbol() {
    let path = repository_path("sovmem/src/profile/canon.rs");
    let source = "\
        use crate::profile::export_canary;\n\
        fn forbidden_profile_route() { export_canary(); }\n";

    assert_eq!(
        p3_shared_route_violations(&path, source),
        vec!["export_canary".to_owned()],
        "ordinary P3 paths may not import a P2-only symbol through crate::profile"
    );
}

#[test]
fn all_p3_path_contract_rejects_aliased_profile_reexport_of_p2_only_symbol() {
    let path = repository_path("sovmem/src/profile/canon.rs");
    let source = "\
        use crate::profile::read_only_git as p2_git;\n\
        fn forbidden_profile_route() { p2_git(); }\n";

    assert_eq!(
        p3_shared_route_violations(&path, source),
        vec!["read_only_git".to_owned()],
        "ordinary P3 paths may not alias-import a P2-only symbol through crate::profile"
    );
}

#[test]
fn all_p3_path_contract_rejects_qualified_profile_call_to_p2_only_symbol() {
    let path = repository_path("sovmem/src/profile/canon.rs");
    let source = "\
        // crate::profile::open_committed_snapshot();\n\
        let prose = \"crate::profile::open_committed_snapshot()\";\n\
        fn forbidden_profile_route() { crate::profile::open_committed_snapshot(); }\n";

    assert_eq!(
        p3_shared_route_violations(&path, source),
        vec!["open_committed_snapshot".to_owned()],
        "ordinary P3 paths may not call a P2-only symbol through crate::profile"
    );
    assert!(
        p3_shared_route_violations(
            &path,
            "// crate::profile::open_committed_snapshot();\nlet prose = \"open_committed_snapshot\";"
        )
        .is_empty(),
        "comments and literals must not create P2-only profile routes"
    );
}

#[test]
fn profile_module_route_contract_scans_ordinary_bodies_but_allows_its_legal_glob_reexports() {
    let path = repository_path("sovmem/src/profile/mod.rs");
    let legal_reexports = "\
        pub use export::*;\n\
        pub use source::*;\n\
        fn approved_profile_surface() {}\n";
    assert!(
        p3_shared_route_violations(&path, legal_reexports).is_empty(),
        "the declared profile public surface remains a legal re-export boundary"
    );

    let bypass = "\
        pub use export::*;\n\
        fn forbidden_profile_body_route() { export_canary(); }\n";
    assert_eq!(
        p3_shared_route_violations(&path, bypass),
        vec!["export_canary".to_owned()],
        "profile/mod.rs must scan ordinary body calls after excluding only legal re-exports"
    );
}

#[test]
fn all_p3_path_contract_rejects_sibling_profile_reexports_of_every_p2_only_symbol() {
    let path = repository_path("sovmem/src/profile/canon.rs");
    for symbol in P2_ONLY_SHARED_SYMBOLS {
        for import in [
            format!("use super::{{{symbol}}};"),
            format!("use super::{{{symbol} as hidden}};"),
        ] {
            let source = format!("{import}\nfn forbidden_sibling_route() {{ hidden(); }}");
            assert_eq!(
                p3_shared_route_violations(&path, &source),
                vec![symbol.to_owned()],
                "ordinary P3 paths may not import {symbol} through a profile sibling re-export"
            );
        }
    }
}

#[test]
fn all_p3_path_contract_rejects_profile_surface_aliases_that_route_to_p2_only_symbols() {
    let path = repository_path("sovmem/src/profile/canon.rs");
    for source in [
        "use crate::{profile as p};\nfn forbidden_route() { p::open_committed_snapshot(); }",
        "use crate::profile::export as e;\nfn forbidden_route() { e::export_canary(); }",
        "use crate::profile::source as s;\nfn forbidden_route() { s::read_only_git(); }",
        "use crate::profile::{export as e, source as s};\nfn forbidden_route() { e::export_corpus(); s::read_only_git(); }",
    ] {
        let violations = p3_shared_route_violations(&path, source);
        assert!(
            !violations.is_empty(),
            "an alias to a shared profile surface may not route to P2-only APIs: {source}"
        );
    }
    assert_eq!(
        p3_shared_route_violations(
            &path,
            "use crate::{profile as p};\nfn forbidden_route() { p::open_committed_snapshot(); }"
        ),
        vec!["open_committed_snapshot".to_owned()]
    );
}

#[test]
fn all_p3_path_contract_rejects_shared_surface_self_aliases_that_route_to_p2_only_symbols() {
    let path = repository_path("sovmem/src/profile/canon.rs");
    for (source, expected) in [
        (
            "use crate::profile::{self as p};\nfn forbidden_route() { p::open_committed_snapshot(); }",
            "open_committed_snapshot",
        ),
        (
            "use super::{self as p};\nfn forbidden_route() { p::read_only_git(); }",
            "read_only_git",
        ),
        (
            "use crate::{profile::{self as p}};\nfn forbidden_route() { p::open_committed_snapshot(); }",
            "open_committed_snapshot",
        ),
    ] {
        assert!(
            p3_shared_route_violations(&path, source).contains(&expected.to_owned()),
            "a self alias of a shared profile surface may not route to a P2-only API: {source}"
        );
    }
}

#[test]
fn all_p3_path_contract_fails_closed_on_module_root_aliases() {
    let path = repository_path("sovmem/src/profile/canon.rs");
    for (source, expected) in [
        (
            "use super as p;\nfn forbidden_route() { p::read_only_git(); }",
            "root module alias: p",
        ),
        (
            "use crate as root;\nfn forbidden_route() { root::profile::open_committed_snapshot(); }",
            "root module alias: root",
        ),
        (
            "use crate::{self as root};\nfn forbidden_route() { root::profile::open_committed_snapshot(); }",
            "root module alias: root",
        ),
        (
            "use super::{self as p};\nfn forbidden_route() { p::read_only_git(); }",
            "root module alias: p",
        ),
    ] {
        assert!(
            p3_shared_route_violations(&path, source).contains(&expected.to_owned()),
            "ordinary P3 paths must fail closed on a module-root alias: {source}"
        );
    }
    assert!(
        p3_shared_route_violations(
            &path,
            "// use super as p; p::read_only_git();\nlet prose = \"use crate as root\";"
        )
        .is_empty(),
        "comments and literals must not create module-root aliases"
    );
}

#[test]
fn all_p3_path_contract_rejects_shared_surface_globs_when_they_expose_p2_only_symbols() {
    let path = repository_path("sovmem/src/profile/canon.rs");
    for source in [
        "use crate::profile::*;\nfn forbidden_route() { export_canary(); }",
        "use crate::profile::export::*;\nfn forbidden_route() { export_corpus(); }",
        "use crate::profile::source::*;\nfn forbidden_route() { read_only_git(); }",
        "use crate::profile::{export::*, source::*};\nfn forbidden_route() { export_canary(); read_only_git(); }",
        "use super::*;\nfn forbidden_route() { open_committed_snapshot(); }",
    ] {
        assert!(
            !p3_shared_route_violations(&path, source).is_empty(),
            "a shared-surface glob may not expose a P2-only API: {source}"
        );
    }
    assert!(
        p3_shared_route_violations(
            &path,
            "// use crate::profile::*; export_canary();\nlet prose = \"use crate::profile::source::*; read_only_git()\";"
        )
        .is_empty(),
        "comments and literals must not create a globbed P2-only route"
    );
}

#[test]
fn frontmatter_renderer_is_an_explicit_p3_shared_utility_root_and_closure_member() {
    let path = repository_path("sovmem/src/profile/frontmatter.rs");
    let boundary = p3_shared_utility_boundary();
    let declared = boundary
        .iter()
        .find(|(candidate, _)| *candidate == path)
        .expect("frontmatter is a declared shared utility")
        .1
        .clone();
    assert!(declared.contains(&"render_document".to_owned()));
    assert_eq!(
        approved_shared_utility_roots(&path),
        vec!["parse_source_document", "render_document"]
    );

    let source = fs::read_to_string(&path).expect("read frontmatter source");
    let closure = local_function_call_closure(&source, &approved_shared_utility_roots(&path));
    assert!(closure.contains("render_document"));
    assert!(closure.is_subset(&declared.into_iter().collect()));
}

#[test]
fn approved_shared_utility_route_contract_rejects_p2_only_helper_routes() {
    let path = repository_path("sovmem/src/profile/export.rs");
    let synthetic = "fn approved_shared_helper() { super::source::read_only_git(); }";
    let symbols = BTreeSet::from(["approved_shared_helper".to_owned()]);
    let body = named_item_body(synthetic, "approved_shared_helper");

    assert!(
        production_authority_violations(&body).is_empty(),
        "the pre-existing shared-boundary authority scan misses this P2-only route"
    );
    assert_eq!(
        shared_utility_contract_violations(&path, synthetic, &symbols),
        vec!["approved_shared_helper: read_only_git".to_owned()],
        "shared utility bodies must also be checked through their deferred-module path"
    );
}

#[test]
fn approved_shared_utility_closure_is_explicit_and_authority_scanned() {
    let synthetic = "\
        fn approved_root() { unlisted_helper(); }\n\
        fn unlisted_helper() {}\n";
    assert_eq!(
        local_function_call_closure(synthetic, &["approved_root"]),
        BTreeSet::from(["approved_root".to_owned(), "unlisted_helper".to_owned()])
    );

    for (path, declared_symbols) in p3_shared_utility_boundary() {
        let source = fs::read_to_string(&path).expect("read shared utility source");
        let roots = approved_shared_utility_roots(&path);
        let closure = local_function_call_closure(&source, &roots);
        let declared = declared_symbols.into_iter().collect::<BTreeSet<_>>();
        assert!(
            closure.is_subset(&declared),
            "every locally reachable helper must be explicit in the shared utility boundary: {}",
            path.display()
        );
        assert!(
            shared_utility_contract_violations(&path, &source, &closure).is_empty(),
            "reachable shared utility has prohibited authority or shared-route construct(s) {:?}: {}",
            shared_utility_contract_violations(&path, &source, &closure),
            path.display()
        );
    }
}

#[test]
fn p3_fixture_use_preserves_the_tracked_p2_raw_blake3_snapshot() {
    let before = FrozenP2RawSnapshot::capture_tracked();

    let fixture = P3CliFixture::new("source-contract-frozen-p2");
    fixture.install_fixture_chain_with_one_applied_generation();

    assert_eq!(before, FrozenP2RawSnapshot::capture_tracked());
}

#[test]
fn p3_production_has_no_external_authority_or_automatic_acceptance_constructs() {
    for path in p3_production_paths() {
        let source = fs::read_to_string(&path).expect("read P3 production source");
        assert!(
            production_authority_violations(&source).is_empty(),
            "P3 production source has prohibited authority construct(s) {:?}: {}",
            production_authority_violations(&source),
            path.display()
        );
    }
}

#[test]
fn canonical_generation_writes_have_only_transaction_and_provisioning_harness_routes() {
    let profile_root = repository_path("sovmem/src/profile");
    let sources = rust_sources(&profile_root);
    let mut routes = Vec::new();
    for (path, source) in &sources {
        let calls = function_call_count(source, "stage_generation");
        if calls == 0 {
            continue;
        }
        routes.push((
            path.strip_prefix(&profile_root)
                .expect("profile source remains under root")
                .to_string_lossy()
                .replace('\\', "/"),
            calls,
        ));
    }

    assert_eq!(
        routes,
        vec![
            ("p3_internal/p3.rs".to_owned(), 5),
            ("transaction.rs".to_owned(), 1),
        ],
        "only the test-only provisioning harness and transaction writer may stage a canonical generation"
    );
}

#[test]
fn proposer_and_read_commands_have_no_writer_route() {
    let cli =
        fs::read_to_string(repository_path("sovmem/src/cli/p3.rs")).expect("read P3 CLI source");
    let writer_markers = [
        "commit_human_verdict",
        "recover_store_human",
        "TransactionRuntime",
        "stage_generation",
        "transaction::",
    ];
    for command in [
        "run_propose",
        "run_proposals_list",
        "run_proposal_status",
        "run_store_stats",
        "run_store_query",
    ] {
        let body = function_body(&cli, command);
        for marker in writer_markers {
            assert!(
                !body.contains(marker),
                "{command} must not route to transaction writer marker {marker}"
            );
        }
    }
}

const TRANSACTION_WRITER_SYMBOLS: [&str; 4] = [
    "commit_human_verdict",
    "recover_store_human",
    "TransactionRuntime",
    "stage_generation",
];

fn transaction_writer_references(source: &str) -> Vec<String> {
    let mut references = TRANSACTION_WRITER_SYMBOLS
        .into_iter()
        .flat_map(|symbol| {
            std::iter::once(symbol.to_owned()).chain(imported_aliases(source, symbol))
        })
        .collect::<Vec<_>>();
    references.sort();
    references.dedup();
    references
}

fn writer_aliases_or_public_reexports(source: &str) -> Vec<String> {
    let mut routes = Vec::new();
    for statement in parsed_use_statements(source) {
        for symbol in TRANSACTION_WRITER_SYMBOLS {
            if !contains_identifier_token(&statement.import, symbol) {
                continue;
            }
            let is_alias = statement.import.match_indices(symbol).any(|(offset, _)| {
                statement.import[offset + symbol.len()..]
                    .trim_start()
                    .starts_with("as ")
            });
            if statement.is_reexport || is_alias {
                routes.push(statement.statement.clone());
            }
        }
    }
    routes.sort();
    routes.dedup();
    routes
}

const FORBIDDEN_NON_WRITER_PUBLIC_REEXPORTS: [&str; 6] = [
    "recover_store",
    "P3CliFixture",
    "P3TestFixture",
    "FaultInjector",
    "FilesystemOps",
    "FaultPoint",
];

fn public_reexport_violations(source: &str) -> Vec<String> {
    let mut violations = parsed_use_statements(source)
        .into_iter()
        .filter(|statement| statement.is_public)
        .flat_map(|statement| {
            TRANSACTION_WRITER_SYMBOLS
                .into_iter()
                .chain(FORBIDDEN_NON_WRITER_PUBLIC_REEXPORTS)
                .filter(move |forbidden| contains_identifier_token(&statement.import, forbidden))
                .map(|forbidden| forbidden.to_owned())
        })
        .collect::<Vec<_>>();
    violations.sort();
    violations.dedup();
    violations
}

fn function_names(source: &str) -> Vec<String> {
    let code = strip_comments_and_literals(source);
    let mut names = code
        .match_indices("fn ")
        .filter_map(|(offset, _)| {
            let prefix = &code[..offset];
            (!prefix
                .chars()
                .next_back()
                .is_some_and(is_identifier_character))
            .then(|| {
                let name = &code[offset + "fn ".len()..];
                let length = name
                    .chars()
                    .take_while(|character| is_identifier_character(*character))
                    .map(char::len_utf8)
                    .sum();
                name[..length].to_owned()
            })
        })
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

#[test]
fn p3_cli_writer_symbols_are_confined_to_human_authority_commands() {
    let cli =
        fs::read_to_string(repository_path("sovmem/src/cli/p3.rs")).expect("read P3 CLI source");
    assert!(
        writer_aliases_or_public_reexports(&strip_comments_and_literals(&cli)).is_empty(),
        "P3 CLI must not alias or publicly re-export a transaction writer symbol"
    );

    let writer_references = transaction_writer_references(&strip_comments_and_literals(&cli));
    for function in function_names(&cli) {
        if matches!(function.as_str(), "run_verdict" | "run_recover") {
            continue;
        }
        let body = function_body(&cli, &function);
        for writer in &writer_references {
            assert!(
                !contains_identifier_token(&body, writer),
                "{function} must not route to transaction writer symbol {writer}"
            );
        }
        assert!(
            !body.contains("transaction::"),
            "{function} must not route to the transaction module"
        );
    }
}

#[test]
fn transaction_writer_and_test_doubles_have_no_public_reexport() {
    for (path, source) in rust_sources(&repository_path("sovmem/src")) {
        assert!(
            public_reexport_violations(&source).is_empty(),
            "public re-export crosses the P3 authority boundary in {}: {:?}",
            path.display(),
            public_reexport_violations(&source),
        );
    }
}

#[test]
fn public_reexport_contract_parses_grouped_multiline_writer_and_test_double_imports() {
    let grouped_writer = "\
        pub use super::{\n\
            commit_human_verdict,\n\
        };\n";
    assert_eq!(
        public_reexport_violations(grouped_writer),
        vec!["commit_human_verdict".to_owned()],
        "a grouped writer re-export must be rejected even outside lib.rs/profile/mod.rs"
    );

    let aliased_recovery_writer = "pub use crate::profile::recover_store_human as recover;\n";
    assert_eq!(
        public_reexport_violations(aliased_recovery_writer),
        vec!["recover_store_human".to_owned()],
        "an aliased recovery-writer re-export must be rejected"
    );

    let grouped_staging_writer = "\
        pub use crate::profile::{\n\
            stage_generation as stage,\n\
        };\n";
    assert_eq!(
        public_reexport_violations(grouped_staging_writer),
        vec!["stage_generation".to_owned()],
        "a grouped and aliased staging-writer re-export must be rejected"
    );

    let grouped_test_double = "\
        pub use crate::profile::{\n\
            p3_test_support::{P3CliFixture as Fixture},\n\
        };\n";
    assert_eq!(
        public_reexport_violations(grouped_test_double),
        vec!["P3CliFixture".to_owned()],
        "a grouped and aliased test-double re-export must be rejected"
    );

    let crate_private_control = "\
        pub(crate) use super::{\n\
            commit_human_verdict,\n\
        };\n";
    assert!(
        public_reexport_violations(crate_private_control).is_empty(),
        "crate-private transaction plumbing remains legal"
    );
}

fn identifier_components(identifier: &str) -> Vec<String> {
    let mut components = Vec::new();
    let mut current = String::new();
    let mut previous_was_lowercase = false;
    for character in identifier.chars() {
        if character == '_' {
            if !current.is_empty() {
                components.push(current.to_ascii_lowercase());
                current.clear();
            }
            previous_was_lowercase = false;
            continue;
        }
        if character.is_ascii_uppercase() && previous_was_lowercase && !current.is_empty() {
            components.push(current.to_ascii_lowercase());
            current.clear();
        }
        previous_was_lowercase = character.is_ascii_lowercase();
        current.push(character);
    }
    if !current.is_empty() {
        components.push(current.to_ascii_lowercase());
    }
    components
}

fn contains_identifier(source: &str, component: &str) -> bool {
    let expected = component.split('_').collect::<Vec<_>>();
    source
        .split(|character: char| !is_identifier_character(character))
        .any(|identifier| {
            let components = identifier_components(identifier);
            components.windows(expected.len()).any(|components| {
                components
                    .iter()
                    .map(String::as_str)
                    .eq(expected.iter().copied())
            })
        })
}

fn contains_forbidden_authority_component(source: &str, component: &str) -> bool {
    if component != "git" {
        return contains_identifier(source, component);
    }

    contains_git_command_invocation(source)
        || source
            .split(|character: char| !is_identifier_character(character))
            .any(|identifier| {
                let components = identifier_components(identifier);
                components.windows(2).any(|components| {
                    components[0] == "git"
                        && matches!(
                            components[1].as_str(),
                            "helper" | "command" | "client" | "invoke" | "process" | "exec"
                        )
                })
            })
}

/// Remove the content of lines gated behind `#[cfg(...)]` attributes so that
/// conditionally compiled constructs (e.g. `#[cfg(feature = "mcp")] pub mod mcp;`)
/// are not flagged as authority violations in production paths.
fn strip_cfg_gates(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut index = 0;
    let chars: Vec<char> = source.chars().collect();

    while index < chars.len() {
        // Look for `#[cfg` or `#[cfg_attr` at the start of a line (after optional whitespace).
        let mut check = index;
        while check < chars.len() && (chars[check] == ' ' || chars[check] == '\t') {
            check += 1;
        }
        if check + 4 < chars.len()
            && chars[check] == '#'
            && chars[check + 1] == '['
            && chars[check + 2] == 'c'
            && chars[check + 3] == 'f'
            && chars[check + 4] == 'g'
        {
            // Found a #[cfg(...)] attribute. Skip to the matching `]`.
            let mut depth = 0;
            let mut start = check;
            while start < chars.len() {
                match chars[start] {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            start += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                start += 1;
            }
            // Skip whitespace and newlines after the attribute
            while start < chars.len()
                && (chars[start] == ' ' || chars[start] == '\t' || chars[start] == '\n')
            {
                if chars[start] == '\n' {
                    result.push('\n');
                }
                start += 1;
            }
            // Skip the next statement (up to the next `;` on this logical line or end of identifier line)
            while start < chars.len() && chars[start] != '\n' && chars[start] != ';' {
                start += 1;
            }
            if start < chars.len() && chars[start] == ';' {
                start += 1;
            }
            // Skip whitespace after the stripped statement
            while start < chars.len() && (chars[start] == ' ' || chars[start] == '\t') {
                start += 1;
            }
            index = start;
            continue;
        }
        result.push(chars[index]);
        index += 1;
    }
    result
}

fn production_authority_violations(source: &str) -> Vec<String> {
    let mut stripped = strip_comments_and_literals(source);
    stripped = strip_cfg_gates(&stripped);
    let mut violations = [
        "server",
        "socket",
        "network",
        "model",
        "embedding",
        "vector",
        "qdrant",
        "auto_accept",
        "autoaccept",
    ]
    .into_iter()
    .filter(|term| contains_forbidden_authority_component(&stripped, term))
    .map(str::to_owned)
    .collect::<Vec<_>>();
    if contains_forbidden_authority_component(&stripped, "git")
        || contains_process_command_construction(&stripped)
        || contains_std_process_route(&stripped)
    {
        violations.push("git".to_owned());
    }
    for keyword in ["async", "unsafe"] {
        if contains_identifier_token(&stripped, keyword) {
            violations.push(keyword.to_owned());
        }
    }
    violations.sort();
    violations.dedup();
    violations
}

fn contains_process_command_construction(source: &str) -> bool {
    source.match_indices("Command").any(|(offset, _)| {
        let prefix = &source[..offset];
        let suffix = &source[offset + "Command".len()..];
        let command_is_identifier = !prefix
            .chars()
            .next_back()
            .is_some_and(is_identifier_character);
        let suffix = suffix.trim_start();
        let Some(suffix) = suffix.strip_prefix("::") else {
            return false;
        };
        let suffix = suffix.trim_start();
        command_is_identifier
            && suffix.strip_prefix("new").is_some_and(|after_new| {
                !after_new
                    .chars()
                    .next()
                    .is_some_and(is_identifier_character)
                    && after_new.trim_start().starts_with('(')
            })
    })
}

fn contains_std_process_route(source: &str) -> bool {
    if source
        .split(';')
        .map(str::trim)
        .filter_map(use_statement_parts)
        .any(|(_, import)| {
            contains_identifier_token(import, "std") && contains_identifier_token(import, "process")
        })
    {
        return true;
    }

    source.match_indices("std").any(|(offset, _)| {
        let prefix = &source[..offset];
        if prefix
            .chars()
            .next_back()
            .is_some_and(is_identifier_character)
        {
            return false;
        }
        let Some(after_separator) = source[offset + "std".len()..]
            .trim_start()
            .strip_prefix("::")
        else {
            return false;
        };
        let after_process = after_separator.trim_start();
        let Some(after_process) = after_process.strip_prefix("process") else {
            return false;
        };
        if after_process
            .chars()
            .next()
            .is_some_and(is_identifier_character)
        {
            return false;
        }
        let after_process = after_process.trim_start();
        if after_process.starts_with(';') {
            let statement_start = source[..offset].rfind(';').map_or(0, |index| index + 1);
            return source[statement_start..offset]
                .trim_start()
                .starts_with("use ");
        }
        let Some(after_separator) = after_process.strip_prefix("::") else {
            return false;
        };
        let after_command = after_separator.trim_start();
        after_command
            .strip_prefix("Command")
            .is_some_and(|after_command| {
                !after_command
                    .chars()
                    .next()
                    .is_some_and(is_identifier_character)
            })
    })
}

fn contains_git_command_invocation(source: &str) -> bool {
    let characters = source.chars().collect::<Vec<_>>();
    let mut code = String::with_capacity(source.len());
    let mut index = 0;

    while index < characters.len() {
        let raw_start = match (characters[index], characters.get(index + 1).copied()) {
            ('r', _) => Some(index),
            ('b', Some('r')) => Some(index + 1),
            _ => None,
        };
        if let Some(raw_start) = raw_start {
            let mut quote = raw_start + 1;
            while characters.get(quote) == Some(&'#') {
                quote += 1;
            }
            if characters.get(quote) == Some(&'"') {
                let hashes = quote - raw_start - 1;
                let literal_start = quote + 1;
                let mut literal_end = literal_start;
                while literal_end < characters.len() {
                    if characters[literal_end] == '"'
                        && (0..hashes)
                            .all(|offset| characters.get(literal_end + offset + 1) == Some(&'#'))
                    {
                        if characters[literal_start..literal_end]
                            .iter()
                            .collect::<String>()
                            == "git"
                            && command_constructor_precedes_literal(&code)
                        {
                            return true;
                        }
                        index = literal_end + hashes + 1;
                        code.push(' ');
                        break;
                    }
                    literal_end += 1;
                }
                if index != raw_start {
                    continue;
                }
            }
        }

        match (characters[index], characters.get(index + 1).copied()) {
            ('/', Some('/')) => {
                index += 2;
                while index < characters.len() && characters[index] != '\n' {
                    index += 1;
                }
            }
            ('/', Some('*')) => {
                index += 2;
                let mut depth = 1;
                while index < characters.len() && depth > 0 {
                    match (characters[index], characters.get(index + 1).copied()) {
                        ('/', Some('*')) => {
                            depth += 1;
                            index += 2;
                        }
                        ('*', Some('/')) => {
                            depth -= 1;
                            index += 2;
                        }
                        ('\n', _) => {
                            code.push('\n');
                            index += 1;
                        }
                        _ => index += 1,
                    }
                }
            }
            ('"', _) | ('b', Some('"')) => {
                index += usize::from(characters[index] == 'b');
                index += 1;
                let literal_start = index;
                let mut literal = String::new();
                while index < characters.len() {
                    match characters[index] {
                        '\\' => {
                            literal.push(characters[index]);
                            if let Some(next) = characters.get(index + 1) {
                                literal.push(*next);
                            }
                            index += 2;
                        }
                        '"' => {
                            if literal == "git" && command_constructor_precedes_literal(&code) {
                                return true;
                            }
                            index += 1;
                            break;
                        }
                        '\n' => {
                            code.push('\n');
                            index += 1;
                        }
                        character => {
                            literal.push(character);
                            index += 1;
                        }
                    }
                }
                let _ = literal_start;
                code.push(' ');
            }
            _ => {
                code.push(characters[index]);
                index += 1;
            }
        }
    }

    false
}

fn command_constructor_precedes_literal(code: &str) -> bool {
    let compact = code
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    compact.ends_with("Command::new(")
}

fn contains_identifier_token(source: &str, token: &str) -> bool {
    source.match_indices(token).any(|(offset, _)| {
        let prefix = &source[..offset];
        let suffix = &source[offset + token.len()..];
        !prefix
            .chars()
            .next_back()
            .is_some_and(is_identifier_character)
            && !suffix.chars().next().is_some_and(is_identifier_character)
    })
}

fn function_body(source: &str, function: &str) -> String {
    let source = strip_comments_and_literals(source);
    let signature = format!("fn {function}");
    let start = source
        .match_indices(&signature)
        .map(|(offset, _)| offset)
        .find(|offset| {
            source[*offset + signature.len()..]
                .trim_start()
                .starts_with('(')
        })
        .unwrap_or_else(|| panic!("required P3 CLI command {function}"));
    let body_start = source[start..]
        .find('{')
        .map(|offset| start + offset)
        .expect("P3 CLI command has a body");
    let mut depth = 0usize;
    for (offset, character) in source[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return source[body_start..=body_start + offset].to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("P3 CLI command body is balanced")
}

#[test]
fn p2_fixture_has_one_test_owner_and_two_approved_profile_consumers() {
    let profile_root = repository_path("sovmem/src/profile");
    let profile_sources = rust_sources(&profile_root);
    let profile_module =
        fs::read_to_string(profile_root.join("mod.rs")).expect("read profile module source");
    let internal_module = fs::read_to_string(profile_root.join("p3_internal/mod.rs"))
        .expect("read internal P3 module source");
    let test_support = fs::read_to_string(profile_root.join("p3_test_support.rs"))
        .expect("read P3 test support source");

    assert_eq!(
        profile_sources
            .iter()
            .map(|(path, _)| fs::read_to_string(path).expect("read profile source"))
            .map(|source| source.matches("tests/support/p2.rs").count())
            .sum::<usize>(),
        1,
        "profile must include the P2 fixture source exactly once"
    );
    assert!(profile_module.contains("#[path = \"../../tests/support/p2.rs\"]\npub mod p2_fixture;"));
    assert!(internal_module.contains("use super::p2_fixture as p2;"));
    assert!(test_support.contains("use super::p2_fixture;"));
}

#[test]
fn generation_staging_is_crate_private_with_one_production_route() {
    let profile_root = repository_path("sovmem/src/profile");
    let sources = rust_sources(&profile_root);
    let canon = source_for(&sources, "src/profile/canon.rs");
    let profile_module = source_for(&sources, "src/profile/mod.rs");

    assert_eq!(canon.matches("fn stage_generation(").count(), 1);
    assert!(canon.contains("pub(crate) fn stage_generation("));
    assert!(!canon.contains("pub fn stage_generation("));
    assert!(profile_module.contains("#[cfg(any(test, feature =  ))]\npub mod p3_internal;"));
    assert!(profile_module.contains("pub mod p3_test_support;"));

    let mut production_routes = Vec::new();
    let mut internal_test_routes = Vec::new();
    for (path, source) in &sources {
        let calls = function_call_count(source, "stage_generation");
        if calls == 0 {
            continue;
        }
        let relative = path
            .strip_prefix(&profile_root)
            .expect("profile source remains under root")
            .to_string_lossy()
            .replace('\\', "/");
        if relative.starts_with("p3_internal/") {
            internal_test_routes.push((relative, calls));
        } else {
            production_routes.push((relative, calls));
        }
    }

    assert_eq!(production_routes, vec![("transaction.rs".to_owned(), 1)]);
    assert_eq!(
        internal_test_routes,
        vec![("p3_internal/p3.rs".to_owned(), 5)]
    );
}

#[test]
fn fixed_clock_submission_route_is_test_only_and_external_p3_tests_cannot_bridge_it() {
    let proposal = fs::read_to_string(repository_path("sovmem/src/profile/proposal.rs"))
        .expect("read proposal source");
    let clock = fs::read_to_string(repository_path("sovmem/src/profile/clock.rs"))
        .expect("read clock source");
    let test_support = fs::read_to_string(repository_path("sovmem/src/profile/p3_test_support.rs"))
        .expect("read P3 test support source");

    assert!(proposal.contains("pub fn submit_proposal(request: ProposalSubmission)"));
    assert!(proposal.contains("submit_proposal_with(&request, &SystemClock"));
    assert!(proposal.contains("pub fn submit_proposal_with_clock("));
    assert!(clock.contains("pub trait Clock"));
    assert!(test_support.contains("pub struct FixedClock"));

    let test_root = repository_path("sovmem/tests");
    for (path, source) in rust_sources(&test_root).into_iter().filter(|(path, _)| {
        matches!(
            path.strip_prefix(&test_root)
                .expect("test source remains under root")
                .to_string_lossy()
                .replace('\\', "/")
                .as_str(),
            "p3_cli.rs" | "p3_privacy.rs" | "support/p3.rs"
        )
    }) {
        for forbidden in ["submit_proposal_with_clock", "FixedClock", "Clock"] {
            assert!(
                !source.contains(forbidden),
                "external P3 clock bridge in {}: {forbidden}",
                path.display()
            );
        }
    }
}

#[test]
fn recursive_source_boundary_rejects_public_test_and_transaction_bridges() {
    let profile_root = repository_path("sovmem/src/profile");
    let profile_sources = rust_sources(&profile_root);
    let profile_module = source_for(&profile_sources, "src/profile/mod.rs");
    assert!(profile_module.contains("pub use transaction::"));
    assert!(!profile_module.contains("pub use p3_internal"));
    assert!(!profile_module.contains("pub use p3_test_support"));

    let public_wrapper_markers = ["stage_generation", "P3CliFixture"];
    for (path, source) in &profile_sources {
        for line in source.lines().map(str::trim_start) {
            if line.starts_with("pub ") {
                assert!(
                    !public_wrapper_markers
                        .iter()
                        .any(|marker| line.contains(marker)),
                    "public source-boundary wrapper in {}: {line}",
                    path.display()
                );
            }
        }
    }

    let test_root = repository_path("sovmem/tests");
    let relevant_test_sources = rust_sources(&test_root)
        .into_iter()
        .filter(|(path, _)| {
            matches!(
                path.strip_prefix(&test_root)
                    .expect("test source remains under root")
                    .to_string_lossy()
                    .replace('\\', "/")
                    .as_str(),
                "p3_cli.rs" | "p3_privacy.rs" | "support/p3.rs"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(relevant_test_sources.len(), 3);

    let forbidden_external_bridges = [
        "stage_generation",
        "commit_human_verdict",
        "recover_store_human",
        "TransactionRuntime",
        "FaultInjector",
        "FaultPoint",
        "#[path",
    ];
    for (path, source) in &relevant_test_sources {
        for forbidden in forbidden_external_bridges {
            assert!(
                !source.contains(forbidden),
                "external P3 test bridge in {}: {forbidden}",
                path.display()
            );
        }
    }

    let external_fixture = source_for(&relevant_test_sources, "tests/support/p3.rs");
    let external_fixture_raw = fs::read_to_string(repository_path("sovmem/tests/support/p3.rs"))
        .expect("read external P3 CLI fixture source");
    let cli = source_for(&relevant_test_sources, "tests/p3_cli.rs");
    let privacy = source_for(&relevant_test_sources, "tests/p3_privacy.rs");
    assert!(external_fixture.contains("struct P3CliFixture"));
    assert!(external_fixture.contains("pub(crate) struct P3CliFixture"));
    assert!(!external_fixture.contains("pub struct P3CliFixture"));
    assert!(external_fixture_raw.contains("CARGO_BIN_EXE_sovmem"));
    assert!(external_fixture.contains("verify_store_level4"));
    assert!(external_fixture_raw.contains("fixture.json"));
    assert!(!external_fixture_raw.contains("write_static_variant"));
    assert!(!external_fixture_raw.contains("serialize generated fixture"));
    assert!(cli.contains("P3CliFixture as P3Fixture"));
    assert!(privacy.contains("P3CliFixture as P3Fixture"));
    assert!(!repository_path("sovmem/tests/p3_policy.rs").exists());
    assert!(!repository_path("sovmem/tests/p3_store.rs").exists());
    assert!(!repository_path("sovmem/tests/p3_ledger.rs").exists());
}

#[test]
fn safe_clone_raw_hash_helper_is_read_only_and_outside_production_dispatch() {
    let helper = fs::read_to_string(repository_path("sovmem/examples/sovmem-p3-raw-hash.rs"))
        .expect("read safe-clone raw hash helper");
    let cli = fs::read_to_string(repository_path("sovmem/src/cli.rs"))
        .expect("read production CLI source");
    let main = fs::read_to_string(repository_path("sovmem/src/main.rs"))
        .expect("read production main source");
    let compact = strip_comments_and_literals(&helper)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();

    assert!(helper.contains("blake3::hash"));
    assert!(helper.contains("symlink_metadata"));
    assert!(helper.contains("metadata.is_file()"));
    assert!(helper.contains("ExitCode::from(2)"));
    assert!(!cli.contains("sovmem-p3-raw-hash"));
    assert!(!main.contains("sovmem-p3-raw-hash"));
    for forbidden in [
        "write",
        "create",
        "rename",
        "remove",
        "OpenOptions",
        "Command::new",
        "std::net",
        "Tcp",
        "Udp",
        "proposal",
        "verdict",
        "canon",
        "ledger",
        "unsafe",
        "async",
        "crate::",
    ] {
        assert!(
            !compact.contains(forbidden),
            "safe-clone raw hash helper must not contain {forbidden}"
        );
    }
}
