// Integration-test binaries consume different subsets of this shared fixture module.
#![allow(dead_code)]

use sovmem::profile::{
    build_index, evaluate, export_corpus, load_corpus, parse_source_document, query_index,
    CorpusExportRequest, EvalOutcome, EvalRequest, ExpansionMode, GeneratedDocument,
    GeneratedMetadata, IndexRequest, QueryRequest, SourceDocument, PINNED_SOURCE_COMMIT,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const EXCLUDED_RESEARCH_PATH: &str =
    "04-Research/DSOM/source-pages/table-of-contents__contributing.md";

const TRACKED_P2_RAW_ARTIFACTS: [(&str, &str); 6] = [
    ("profile", "profiles/audy-ssa0-v0/profile.yaml"),
    (
        "corpus_contract",
        "profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml",
    ),
    ("golden", "evals/audy-sovmem-v0/golden.jsonl"),
    // Relevance labels are the `relevant_ids` fields in the frozen golden input.
    ("relevance", "evals/audy-sovmem-v0/golden.jsonl"),
    ("thresholds", "evals/audy-sovmem-v0/thresholds.json"),
    ("baseline", "evals/audy-sovmem-v0/baseline.json"),
];

#[derive(Debug, PartialEq, Eq)]
pub struct FrozenP2RawSnapshot(pub Vec<(&'static str, String)>);

impl FrozenP2RawSnapshot {
    pub fn capture_tracked() -> Self {
        Self(
            TRACKED_P2_RAW_ARTIFACTS
                .into_iter()
                .map(|(name, relative)| {
                    let bytes = fs::read(repository_path(relative))
                        .expect("read tracked frozen P2 artifact");
                    (name, blake3::hash(&bytes).to_hex().to_string())
                })
                .collect(),
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SemanticSnapshot {
    pub manifest_bytes: Vec<u8>,
    pub graph_bytes: Vec<u8>,
    pub counts: SemanticCounts,
    pub bm25_ids: Vec<Vec<String>>,
    pub expansion_witnesses: Vec<Vec<(String, String, String, String)>>,
    pub baseline_bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SemanticCounts {
    pub documents: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub external_typed_links: usize,
}

pub struct P2Fixture {
    area: TemporaryArea,
    pub profile_path: PathBuf,
    pub bundle_root: PathBuf,
    pub golden_path: PathBuf,
    pub thresholds_path: PathBuf,
    _git_environment: GitEnvironmentGuard,
}

impl P2Fixture {
    pub fn new(name: &str) -> Self {
        Self::new_inner(name, false, None, false)
    }

    pub fn new_with_query_cases(name: &str) -> Self {
        Self::new_inner(name, true, None, false)
    }

    pub fn new_retrievable(name: &str, hit_queries: usize) -> Self {
        assert!(hit_queries <= 25, "frozen evaluation has 25 queries");
        Self::new_inner(name, false, Some(hit_queries), false)
    }

    pub fn new_frozen_eval(name: &str) -> Self {
        Self::new_retrievable(name, 25)
    }

    pub fn new_with_resolvable_wikilink(name: &str) -> Self {
        Self::new_inner(name, false, None, true)
    }

    pub fn git_environment_guard_id(&self) -> u64 {
        self._git_environment.id
    }

    fn new_inner(
        name: &str,
        query_cases: bool,
        hit_queries: Option<usize>,
        resolvable_wikilink: bool,
    ) -> Self {
        let area = TemporaryArea::new(name);
        let profile_path = repository_path("profiles/audy-ssa0-v0/profile.yaml");
        let corpus_path = repository_path("profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml");
        let frozen_golden = repository_path("evals/audy-sovmem-v0/golden.jsonl");
        let retrieval_queries = hit_queries
            .map(|count| retrievable_query_text(&frozen_golden, count))
            .unwrap_or_default();
        let vault = area.root.join("vault");
        initialize_git_repo(&vault);
        let selected_paths = load_corpus(&corpus_path)
            .expect("load fixed corpus")
            .selected_paths();
        let internal_target = canonical_fixture_id(&selected_paths[19]);
        for (position, path) in selected_paths.into_iter().enumerate() {
            let id = canonical_fixture_id(&path);
            write_source(
                &vault,
                &path,
                position,
                &internal_target,
                query_cases,
                retrieval_queries.get(&id).map(String::as_str),
                resolvable_wikilink && position == 0,
            );
        }
        let actual_commit = commit_all(&vault);
        mark_research_candidate_dirty(&vault);
        let git_environment = install_git_shim(&actual_commit, PINNED_SOURCE_COMMIT);
        let bundle_root = area.root.join("bundle");
        export_corpus(CorpusExportRequest {
            vault_root: vault,
            output_root: bundle_root.clone(),
            profile_path: profile_path.clone(),
            corpus_path,
        })
        .expect("export synthetic Level 3 bundle");
        let golden_path = area.root.join("golden.jsonl");
        let thresholds_path = area.root.join("thresholds.json");
        std::fs::copy(frozen_golden, &golden_path).expect("copy frozen golden");
        std::fs::copy(
            repository_path("evals/audy-sovmem-v0/thresholds.json"),
            &thresholds_path,
        )
        .expect("copy frozen thresholds");
        Self {
            area,
            profile_path,
            bundle_root,
            golden_path,
            thresholds_path,
            _git_environment: git_environment,
        }
    }

    pub fn index_root(&self, name: &str) -> PathBuf {
        self.area.root.join(name)
    }

    pub fn build_index(&self, index_root: &Path) {
        build_index(IndexRequest {
            profile_path: self.profile_path.clone(),
            bundle_root: self.bundle_root.clone(),
            index_root: index_root.to_owned(),
        })
        .expect("build P2 fixture index");
    }

    pub fn evaluate(&self, index_root: &Path, baseline_path: PathBuf) -> EvalOutcome {
        evaluate(EvalRequest {
            profile_path: self.profile_path.clone(),
            bundle_root: self.bundle_root.clone(),
            index_root: index_root.to_owned(),
            golden_path: self.golden_path.clone(),
            thresholds_path: self.thresholds_path.clone(),
            baseline_path,
        })
        .expect("evaluate P2 fixture index")
    }

    pub fn semantic_snapshot(&self, index_root: &Path) -> SemanticSnapshot {
        let generation = selected_generation(index_root);
        let manifest_bytes =
            fs::read(generation.join("index-manifest.json")).expect("read selected index manifest");
        let graph_bytes =
            fs::read(generation.join("graph.json")).expect("read selected graph projection");
        let manifest: serde_json::Value =
            serde_json::from_slice(&manifest_bytes).expect("parse selected index manifest");
        let counts = SemanticCounts {
            documents: manifest["documents"].as_u64().expect("manifest documents") as usize,
            graph_nodes: manifest["graph_nodes"]
                .as_u64()
                .expect("manifest graph nodes") as usize,
            graph_edges: manifest["graph_edges"]
                .as_u64()
                .expect("manifest graph edges") as usize,
            external_typed_links: manifest["external_typed_links"]
                .as_u64()
                .expect("manifest external typed links") as usize,
        };

        let queries = fs::read_to_string(&self.golden_path)
            .expect("read frozen golden queries")
            .lines()
            .map(|line| {
                let query: serde_json::Value =
                    serde_json::from_str(line).expect("parse frozen golden query");
                query["query"]
                    .as_str()
                    .expect("frozen golden query text")
                    .to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            queries.len(),
            25,
            "frozen evaluation must contain 25 queries"
        );

        let mut bm25_ids = Vec::with_capacity(queries.len());
        let mut expansion_witnesses = Vec::with_capacity(queries.len());
        for query in queries {
            let result = query_index(QueryRequest {
                profile_path: self.profile_path.clone(),
                bundle_root: self.bundle_root.clone(),
                index_root: index_root.to_owned(),
                query,
                top_k: 5,
                expansion: ExpansionMode::OneHop,
            })
            .expect("run frozen semantic query");
            bm25_ids.push(result.results.into_iter().map(|hit| hit.id).collect());
            expansion_witnesses.push(
                result
                    .expanded
                    .into_iter()
                    .map(|hit| {
                        let direction = serde_json::to_value(hit.direction)
                            .expect("serialize expansion direction")
                            .as_str()
                            .expect("expansion direction string")
                            .to_owned();
                        (hit.id, hit.via_id, direction, hit.relation)
                    })
                    .collect(),
            );
        }

        let baseline_path = self.area.root.join("semantic-baseline.json");
        self.evaluate(index_root, baseline_path.clone());
        let baseline_bytes = fs::read(baseline_path).expect("read semantic baseline");
        SemanticSnapshot {
            manifest_bytes,
            graph_bytes,
            counts,
            bm25_ids,
            expansion_witnesses,
            baseline_bytes,
        }
    }

    pub fn remove_corpus_report(&self) {
        std::fs::remove_file(self.bundle_root.join("corpus-report.json"))
            .expect("remove fixture corpus report");
    }

    pub fn bundle_hash(&self) -> String {
        let bytes =
            fs::read(self.bundle_root.join("manifest.json")).expect("read fixture bundle manifest");
        let manifest: serde_json::Value =
            serde_json::from_slice(&bytes).expect("parse fixture bundle manifest");
        manifest["bundle_hash"]
            .as_str()
            .expect("fixture bundle hash")
            .to_owned()
    }

    pub fn generated_documents(&self) -> Vec<GeneratedDocument> {
        let bundle = self.bundle_root.join("bundle");
        let mut paths = Vec::new();
        collect_generated_paths(&bundle, &bundle, &mut paths);
        paths.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        paths
            .into_iter()
            .map(|source_path| {
                let bytes =
                    fs::read(bundle.join(&source_path)).expect("read generated fixture file");
                let source_hash = blake3::hash(&bytes).to_hex().to_string();
                let parsed = parse_source_document(SourceDocument {
                    source_path,
                    bytes,
                    source_hash: source_hash.clone(),
                    created: String::new(),
                    updated: String::new(),
                })
                .expect("parse generated fixture file");
                let metadata: GeneratedMetadata = serde_yaml::from_value(
                    serde_yaml::to_value(&parsed.metadata)
                        .expect("encode generated fixture metadata"),
                )
                .expect("decode generated fixture metadata");
                GeneratedDocument {
                    metadata,
                    body: parsed.body,
                    source_hash,
                }
            })
            .collect()
    }
}

fn selected_generation(index_root: &Path) -> PathBuf {
    let current = fs::read_to_string(index_root.join("CURRENT")).expect("read current index slot");
    match current.as_str() {
        "slot-a\n" => index_root.join("slot-a"),
        "slot-b\n" => index_root.join("slot-b"),
        _ => panic!("invalid current index slot"),
    }
}

struct TemporaryArea {
    root: PathBuf,
}

impl TemporaryArea {
    fn new(name: &str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let safe_name: String = name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                    character
                } else {
                    '_'
                }
            })
            .collect();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must follow the Unix epoch")
            .as_nanos();
        let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-p2-{safe_name}-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create temporary P2 fixture area");
        Self { root }
    }
}

impl Drop for TemporaryArea {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn repository_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative)
}

fn initialize_git_repo(vault: &Path) {
    fs::create_dir_all(vault).expect("create synthetic P2 vault");
    git(vault, &["init", "--quiet"]);
    git(vault, &["config", "user.name", "SovMem Test"]);
    git(vault, &["config", "user.email", "sovmem@example.invalid"]);
}

fn write_source(
    vault: &Path,
    source_path: &str,
    position: usize,
    internal_target: &str,
    query_cases: bool,
    retrieval_query: Option<&str>,
    resolvable_wikilink: bool,
) {
    let destination = vault.join(source_path);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).expect("create synthetic source parent directories");
    }
    let mut links =
        format!("  - rel: cites\n    target: https://example.com/sovmem/evidence/{position:03}\n");
    if position < 19 {
        links.push_str(&format!(
            "  - rel: references\n    target: vault://{internal_target}#fixture\n  - rel: cites\n    target: https://example.com/sovmem/additional/{position:03}\n"
        ));
    }
    let explicit_title = if query_cases && position == 0 {
        "title: 'fieldboostneedle \"quoted\" \\ path'\n"
    } else {
        ""
    };
    let query_tag = if query_cases && position == 1 {
        "  - fieldboostneedle\n"
    } else {
        ""
    };
    let query_body = match (query_cases, position) {
        (true, 2) => "Fieldboostneedle body result.\n",
        (true, 3 | 4) => "Equalrankneedle tie result.\n",
        _ => "",
    };
    let retrieval_body = retrieval_query
        .map(|query| format!("Frozen retrieval evidence: {query}\n"))
        .unwrap_or_default();
    let wikilink_body = resolvable_wikilink
        .then(|| format!("Resolvable P2 link: [[{internal_target}]].\n"))
        .unwrap_or_default();
    let body = format!(
        "---\n{explicit_title}description: Deterministic synthetic Level 3 evidence {position:03}.\nprovenance: synthesis\ntags:\n  - p2-fixture\n  - synthetic\n{query_tag}links:\n{links}---\n# Synthetic Evidence {position:03}\n\nThis offline fixture document records deterministic retrieval evidence number {position:03}.\n{query_body}{retrieval_body}{wikilink_body}"
    );
    fs::write(destination, body).expect("write synthetic P2 source");
}

fn retrievable_query_text(golden_path: &Path, hit_queries: usize) -> BTreeMap<String, String> {
    fs::read_to_string(golden_path)
        .expect("read frozen golden queries")
        .lines()
        .take(hit_queries)
        .map(|line| {
            let query: serde_json::Value =
                serde_json::from_str(line).expect("parse frozen golden query");
            let relevant = query["relevant_ids"][0]
                .as_str()
                .expect("single frozen relevant ID")
                .to_owned();
            let text = query["query"]
                .as_str()
                .expect("frozen query text")
                .to_owned();
            (relevant, text)
        })
        .collect()
}

fn canonical_fixture_id(source_path: &str) -> String {
    let without_extension = source_path.strip_suffix(".md").unwrap_or(source_path);
    let identity = without_extension
        .split('/')
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/");
    format!("ssa-0/{identity}")
}

fn percent_encode_segment(segment: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

fn collect_generated_paths(root: &Path, directory: &Path, paths: &mut Vec<String>) {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .expect("read generated fixture directory")
        .map(|entry| entry.expect("read generated fixture entry"))
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_generated_paths(root, &path, paths);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("md")
            && path != root.join("index.md")
        {
            paths.push(
                path.strip_prefix(root)
                    .expect("generated fixture path under bundle")
                    .to_str()
                    .expect("generated fixture path is UTF-8")
                    .to_owned(),
            );
        }
    }
}

fn commit_all(vault: &Path) -> String {
    git(vault, &["add", "--all"]);
    let mut command = git_command(vault);
    command
        .args(["commit", "--quiet", "--message", "add synthetic corpus"])
        .env("GIT_AUTHOR_DATE", "2026-01-02T12:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-02T12:00:00Z");
    expect_success(
        command.output().expect("run synthetic corpus commit"),
        "git commit",
    );
    let output = git(vault, &["rev-parse", "HEAD"]);
    String::from_utf8(output.stdout)
        .expect("synthetic commit ID must be UTF-8")
        .trim()
        .to_owned()
}

fn mark_research_candidate_dirty(vault: &Path) {
    let path = vault.join(EXCLUDED_RESEARCH_PATH);
    let mut bytes = fs::read(&path).expect("read deterministic excluded research candidate");
    bytes.extend_from_slice(b"\nDeterministic uncommitted exclusion.\n");
    fs::write(path, bytes).expect("mark deterministic research candidate dirty");
}

pub fn git_command(vault: &Path) -> Command {
    let mut command = Command::new("/usr/bin/git");
    command.args(["-C"]).arg(vault);
    command
}

fn git(vault: &Path, arguments: &[&str]) -> Output {
    let mut command = git_command(vault);
    command.args(arguments);
    let output = command.output().expect("run Git command");
    expect_success(output, &format!("git {}", arguments.join(" ")))
}

fn expect_success(output: Output, operation: &str) -> Output {
    assert!(
        output.status.success(),
        "{operation} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

struct GitEnvironmentState {
    users: usize,
    active_guard_ids: BTreeSet<u64>,
    actual_commit: Option<String>,
    pinned_commit: Option<String>,
    original_path: Option<OsString>,
    original_actual_commit: Option<OsString>,
    original_pinned_commit: Option<OsString>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct GitEnvironmentSnapshot {
    pub users: usize,
    pub active_guard_ids: BTreeSet<u64>,
    pub state_address: usize,
    pub shim_directory: PathBuf,
}

struct GitEnvironmentGuard {
    id: u64,
}

impl Drop for GitEnvironmentGuard {
    fn drop(&mut self) {
        let mut state = git_environment_state().lock().unwrap();
        state.users -= 1;
        assert!(
            state.active_guard_ids.remove(&self.id),
            "P2 fixture Git environment guard must be active when dropped"
        );
        if state.users == 0 {
            restore_environment("PATH", state.original_path.take());
            restore_environment(
                "SOVMEM_TEST_ACTUAL_COMMIT",
                state.original_actual_commit.take(),
            );
            restore_environment(
                "SOVMEM_TEST_PINNED_COMMIT",
                state.original_pinned_commit.take(),
            );
            state.actual_commit = None;
            state.pinned_commit = None;
        }
    }
}

fn git_environment_state() -> &'static Mutex<GitEnvironmentState> {
    static STATE: OnceLock<Mutex<GitEnvironmentState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(GitEnvironmentState {
            users: 0,
            active_guard_ids: BTreeSet::new(),
            actual_commit: None,
            pinned_commit: None,
            original_path: None,
            original_actual_commit: None,
            original_pinned_commit: None,
        })
    })
}

pub fn git_environment_snapshot() -> GitEnvironmentSnapshot {
    let state = git_environment_state().lock().unwrap();
    GitEnvironmentSnapshot {
        users: state.users,
        active_guard_ids: state.active_guard_ids.clone(),
        state_address: git_environment_state() as *const Mutex<GitEnvironmentState> as usize,
        shim_directory: git_shim_path()
            .parent()
            .expect("P2 Git shim has a parent directory")
            .to_path_buf(),
    }
}

fn restore_environment(name: &str, value: Option<OsString>) {
    if let Some(value) = value {
        std::env::set_var(name, value);
    } else {
        std::env::remove_var(name);
    }
}

fn create_git_shim_directory() -> PathBuf {
    static NEXT_SHIM_ID: AtomicU64 = AtomicU64::new(0);

    for retry in 0..32 {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after the Unix epoch")
            .as_nanos();
        let sequence = NEXT_SHIM_ID.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "sovmem-p2-git-shim-{}-{timestamp}-{sequence}-{retry}",
            std::process::id()
        ));
        match fs::create_dir(&directory) {
            Ok(()) => return directory,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create P2 fixture Git shim directory: {error}"),
        }
    }

    panic!("create a unique P2 fixture Git shim directory after retries");
}

fn git_shim_path() -> &'static PathBuf {
    use std::os::unix::fs::PermissionsExt;

    static SHIM: OnceLock<PathBuf> = OnceLock::new();
    SHIM.get_or_init(|| {
        let directory = create_git_shim_directory();
        let path = directory.join("git");
        let script = "#!/bin/bash\nset -e\npinned=\"${SOVMEM_TEST_PINNED_COMMIT:?}\"\nactual=\"${SOVMEM_TEST_ACTUAL_COMMIT:?}\"\nresolve=0\nargs=()\nfor arg in \"$@\"; do\n  if [[ \"$arg\" == \"$pinned^{commit}\" ]]; then\n    resolve=1\n    args+=(\"$actual^{commit}\")\n  else\n    args+=(\"${arg//$pinned/$actual}\")\n  fi\ndone\nif (( resolve )); then\n  /usr/bin/git \"${args[@]}\" >/dev/null\n  printf '%s\\n' \"$pinned\"\nelse\n  exec /usr/bin/git \"${args[@]}\"\nfi\n";
        fs::write(&path, script).expect("write P2 fixture Git shim");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("make P2 fixture Git shim executable");
        path
    })
}

fn install_git_shim(actual_commit: &str, pinned_commit: &str) -> GitEnvironmentGuard {
    static NEXT_GUARD_ID: AtomicU64 = AtomicU64::new(0);

    let shim = git_shim_path();

    let mut state = git_environment_state().lock().unwrap();
    if state.users == 0 {
        let original_path = std::env::var_os("PATH");
        let mut paths = vec![shim.parent().unwrap().to_path_buf()];
        paths.extend(std::env::split_paths(
            original_path.as_deref().unwrap_or_default(),
        ));
        state.original_path = original_path;
        state.original_actual_commit = std::env::var_os("SOVMEM_TEST_ACTUAL_COMMIT");
        state.original_pinned_commit = std::env::var_os("SOVMEM_TEST_PINNED_COMMIT");
        state.actual_commit = Some(actual_commit.to_owned());
        state.pinned_commit = Some(pinned_commit.to_owned());
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        std::env::set_var("SOVMEM_TEST_ACTUAL_COMMIT", actual_commit);
        std::env::set_var("SOVMEM_TEST_PINNED_COMMIT", pinned_commit);
    } else {
        assert_eq!(state.actual_commit.as_deref(), Some(actual_commit));
        assert_eq!(state.pinned_commit.as_deref(), Some(pinned_commit));
    }
    let id = NEXT_GUARD_ID.fetch_add(1, Ordering::Relaxed);
    assert!(
        state.active_guard_ids.insert(id),
        "P2 fixture Git environment guard IDs must be unique"
    );
    state.users += 1;
    GitEnvironmentGuard { id }
}
