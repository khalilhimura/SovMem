#![allow(dead_code)]

use super::p2::P2Fixture;
use serde::Deserialize;
use sovmem::profile::{
    load_p3_profile, load_policy, load_profile, parse_source_document, render_document,
    verify_bundle, verify_store_level4, GeneratedDocument, GeneratedMetadata, SourceDocument,
    VerificationLevel, VerificationOptions,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

const FIXTURE_SCHEMA: &str = "sovmem-p3-cli-store-fixture-v2";

pub(crate) struct P3CliFixture {
    p2: P2Fixture,
    pub profile_path: PathBuf,
    policy_template: PathBuf,
    pub store_root: PathBuf,
    candidate_path: PathBuf,
}

impl P3CliFixture {
    pub fn new(name: &str) -> Self {
        let p2 = P2Fixture::new(name);
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
        fixture.candidate_path = fixture.input_file(
            "cli-candidate",
            &created_target_bytes(&fixture, target_path, &fixture_id(target_path)),
        );
        fixture
    }

    pub fn command(&self) -> Command {
        Command::new(env!("CARGO_BIN_EXE_sovmem"))
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

    pub fn input_file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.p2.index_root(&format!("p3-input-{name}.md"));
        fs::write(&path, bytes).expect("write proposal input");
        path
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

    pub fn install_fixture_chain_with_one_applied_generation(&self) {
        self.load_static_store("one-applied");
    }

    pub fn extend_fixture_chain_with_superseding_generation(&self) {
        self.load_static_store("two-applied");
    }

    pub fn verify_level4(
        &self,
    ) -> Result<sovmem::profile::Level4Summary, sovmem::profile::Level4Failure> {
        let profile = load_p3_profile(&self.profile_path).expect("load P3 fixture profile");
        verify_store_level4(&profile, &self.store_root)
    }

    pub fn ledger_path(&self) -> PathBuf {
        self.store_root.join("ledger/verdicts.jsonl")
    }

    fn load_static_store(&self, variant: &str) {
        let root = static_fixture_root(variant);
        let manifest = read_manifest(&root);
        assert_eq!(
            manifest.schema, FIXTURE_SCHEMA,
            "fixture schema for {variant}"
        );
        assert_eq!(
            manifest.profile_hash,
            load_p3_profile(&self.profile_path)
                .expect("load P3 fixture profile")
                .hash,
            "fixture profile hash for {variant}"
        );
        assert_eq!(
            manifest.policy_hash,
            load_policy(&self.policy_template)
                .expect("load P3 fixture policy")
                .hash,
            "fixture policy hash for {variant}"
        );
        assert_eq!(manifest.seed_bundle_hash, self.p2.bundle_hash());

        let source = root.join("store");
        let source_tree =
            reconstruct_git_untracked_empty_directories(&manifest, collect_tree(&source));
        assert_manifest_matches(&manifest, &source_tree, "source");

        fs::remove_dir_all(&self.store_root).expect("clear temporary P3 store before fixture copy");
        fs::create_dir(&self.store_root).expect("create fresh temporary P3 store");
        copy_verified_tree(&source, &self.store_root, &source_tree);
        assert_manifest_matches(&manifest, &collect_tree(&self.store_root), "copied");

        let report = verify_store_level4(
            &load_p3_profile(&self.profile_path).expect("load P3 fixture profile"),
            &self.store_root,
        )
        .expect("verify copied static P3 fixture at Level 4");
        assert_eq!(
            report.events, manifest.events,
            "fixture events for {variant}"
        );
        assert_eq!(
            report.verdicts, manifest.verdicts,
            "fixture verdicts for {variant}"
        );
        assert_eq!(report.applied_generations, manifest.applied_generations);
        assert_eq!(report.current_generation, manifest.current_generation);
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticFixtureManifest {
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

fn static_fixture_root(variant: &str) -> PathBuf {
    assert!(matches!(variant, "one-applied" | "two-applied"));
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/p3-cli-store-v2")
        .join(variant)
}

fn read_manifest(root: &Path) -> StaticFixtureManifest {
    let root_metadata = fs::symlink_metadata(root).expect("read static fixture root metadata");
    assert!(
        root_metadata.is_dir() && !root_metadata.file_type().is_symlink(),
        "static fixture root must be a real directory"
    );
    let manifest_path = root.join("fixture.json");
    let manifest_metadata =
        fs::symlink_metadata(&manifest_path).expect("read static fixture manifest metadata");
    assert!(
        manifest_metadata.is_file() && !manifest_metadata.file_type().is_symlink(),
        "static fixture manifest must be a regular file"
    );
    serde_json::from_slice(&fs::read(manifest_path).expect("read static P3 fixture manifest"))
        .expect("parse static P3 fixture manifest")
}

fn collect_tree(root: &Path) -> FixtureTree {
    let metadata = fs::symlink_metadata(root).expect("read static fixture root metadata");
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
        .expect("read static fixture directory")
        .collect::<Result<Vec<_>, _>>()
        .expect("read static fixture entry");
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).expect("read static fixture entry metadata");
        assert!(
            !metadata.file_type().is_symlink(),
            "static fixture cannot contain a symlink"
        );
        let relative = path
            .strip_prefix(root)
            .expect("static fixture entry remains beneath root")
            .to_str()
            .expect("static fixture path is UTF-8")
            .replace('\\', "/");
        assert_safe_relative_path(&relative);
        if metadata.is_dir() {
            tree.directories.push(relative);
            collect_tree_entries(root, &path, tree);
        } else if metadata.is_file() {
            tree.files.insert(
                relative,
                blake3::hash(&fs::read(&path).expect("read static fixture file"))
                    .to_hex()
                    .to_string(),
            );
        } else {
            panic!("static fixture entry must be a regular file or directory");
        }
    }
}

fn reconstruct_git_untracked_empty_directories(
    manifest: &StaticFixtureManifest,
    mut source_tree: FixtureTree,
) -> FixtureTree {
    for directory in &manifest.directories {
        let prefix = format!("{directory}/");
        let has_descendant_file = manifest.files.keys().any(|path| path.starts_with(&prefix));
        if !has_descendant_file && !source_tree.directories.contains(directory) {
            source_tree.directories.push(directory.clone());
        }
    }
    source_tree.directories.sort();
    source_tree
}

fn assert_safe_relative_path(path: &str) {
    let candidate = Path::new(path);
    assert!(!path.is_empty() && !candidate.is_absolute());
    assert!(!has_platform_prefix(path));
    assert!(!candidate
        .components()
        .any(|component| { matches!(component, Component::Prefix(_) | Component::RootDir) }));
    assert!(!path.contains('\\'));
    assert!(path.split('/').all(|component| {
        !component.is_empty()
            && component != "."
            && component != ".."
            && component.bytes().all(|byte| !byte.is_ascii_control())
    }));
}

fn has_platform_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with("//")
        || path.starts_with("\\\\")
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

fn copy_verified_tree(source: &Path, destination: &Path, tree: &FixtureTree) {
    for directory in &tree.directories {
        assert_safe_relative_path(directory);
        fs::create_dir(destination.join(directory))
            .expect("create copied static fixture directory");
    }
    for (relative, expected_hash) in &tree.files {
        assert_safe_relative_path(relative);
        let source_file = source.join(relative);
        let metadata = fs::symlink_metadata(&source_file).expect("read copied fixture source");
        assert!(metadata.is_file() && !metadata.file_type().is_symlink());
        let bytes = fs::read(&source_file).expect("read copied fixture source file");
        assert_eq!(blake3::hash(&bytes).to_hex().as_str(), expected_hash);
        fs::write(destination.join(relative), bytes).expect("write copied static fixture file");
    }
}

fn assert_manifest_matches(manifest: &StaticFixtureManifest, tree: &FixtureTree, stage: &str) {
    assert_eq!(
        manifest.directories, tree.directories,
        "fixture directories at {stage}"
    );
    assert_eq!(manifest.files, tree.files, "fixture files at {stage}");
    assert_eq!(
        manifest.tree_hash,
        tree_hash(tree),
        "fixture tree hash at {stage}"
    );
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

fn created_target_bytes(fixture: &P3CliFixture, target_path: &str, target_id: &str) -> Vec<u8> {
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
    render_document(&GeneratedDocument {
        metadata,
        body: format!("{}\nP3 fixture create exact bytes.\n", parsed.body),
        source_hash: String::new(),
    })
    .expect("render create candidate")
}

fn fixture_id(path: &str) -> String {
    format!("ssa-0/{path}")
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
        fs::read(policy_template).expect("read P3 policy"),
    )
    .expect("write P3 policy");
    fs::write(store_root.join("ledger/verdicts.jsonl"), b"").expect("write zero ledger");
    fs::write(store_root.join("intake/events.jsonl"), b"").expect("write zero intake log");
    fs::write(store_root.join("canon/CURRENT"), b"seed\n").expect("write seed CURRENT");
    load_p3_profile(profile_path).expect("load P3 fixture profile");
}

fn copy_tree(source: &Path, destination: &Path) {
    let metadata = fs::symlink_metadata(source).expect("read fixture source metadata");
    assert!(
        !metadata.file_type().is_symlink(),
        "fixture source cannot be a symlink"
    );
    if metadata.is_file() {
        fs::write(
            destination,
            fs::read(source).expect("read fixture source file"),
        )
        .expect("write fixture destination file");
        return;
    }
    assert!(metadata.is_dir());
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

#[cfg(test)]
mod tests {
    use super::{
        assert_manifest_matches, assert_safe_relative_path, collect_tree, read_manifest,
        reconstruct_git_untracked_empty_directories, static_fixture_root,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEMPORARY_ROOT: AtomicU64 = AtomicU64::new(0);

    struct TemporaryRoot(PathBuf);

    impl TemporaryRoot {
        fn new(label: &str) -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock must be after the Unix epoch")
                .as_nanos();
            let counter = NEXT_TEMPORARY_ROOT.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "sovmem-p3-fixture-loader-{label}-{}-{timestamp}-{counter}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("create temporary static fixture root");
            Self(root)
        }
    }

    impl Drop for TemporaryRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    #[test]
    #[should_panic(expected = "static fixture root must be a real directory")]
    fn manifest_loader_rejects_a_symlinked_variant_root_before_reading_it() {
        let temporary = TemporaryRoot::new("symlinked-root");
        let root = temporary.0.join("variant");
        std::os::unix::fs::symlink(static_fixture_root("one-applied"), &root)
            .expect("create symlinked static fixture root");

        let _ = read_manifest(&root);
    }

    #[cfg(unix)]
    #[test]
    #[should_panic(expected = "static fixture manifest must be a regular file")]
    fn manifest_loader_rejects_a_symlinked_manifest_before_reading_it() {
        let temporary = TemporaryRoot::new("symlinked-manifest");
        let root = temporary.0.join("variant");
        fs::create_dir(&root).expect("create static fixture variant root");
        std::os::unix::fs::symlink(
            static_fixture_root("one-applied").join("fixture.json"),
            root.join("fixture.json"),
        )
        .expect("create symlinked static fixture manifest");

        let _ = read_manifest(&root);
    }

    #[test]
    fn static_fixture_child_paths_reject_platform_prefixes_and_other_unsafe_names() {
        for path in [
            "C:/fixture",
            "C:\\fixture",
            "C:relative",
            "//server/share",
            "\\\\server\\share",
            "/absolute",
            "directory/../escape",
            "directory/./child",
            "directory/child\\name",
            "directory/child\u{0000}name",
        ] {
            assert!(
                std::panic::catch_unwind(|| assert_safe_relative_path(path)).is_err(),
                "unsafe static fixture child path was accepted: {path:?}"
            );
        }
    }

    #[test]
    fn fixture_loader_reconstructs_only_git_untrackable_empty_directories() {
        let root = static_fixture_root("one-applied");
        let manifest = read_manifest(&root);
        let mut source_tree = collect_tree(&root.join("store"));
        source_tree.directories.retain(|directory| {
            let prefix = format!("{directory}/");
            manifest.files.keys().any(|path| path.starts_with(&prefix))
        });

        let reconstructed = reconstruct_git_untracked_empty_directories(&manifest, source_tree);
        assert_manifest_matches(&manifest, &reconstructed, "reconstructed");
    }
}
