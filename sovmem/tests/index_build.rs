#[allow(dead_code)]
mod support;

use sovmem::profile::{
    build_index, derive_graph, index_schema_descriptor, prepare_verified_corpus, IndexError,
    IndexRequest,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use support::p2::{git_command, P2Fixture};

fn request(fixture: &P2Fixture, index_root: PathBuf) -> IndexRequest {
    IndexRequest {
        profile_path: fixture.profile_path.clone(),
        bundle_root: fixture.bundle_root.clone(),
        index_root,
    }
}

fn semantic_tree_hash(root: &std::path::Path) -> String {
    fn collect(root: &std::path::Path, directory: &std::path::Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("read generation directory") {
            let entry = entry.expect("read generation entry");
            let path = entry.path();
            if entry
                .file_type()
                .expect("read generation file type")
                .is_dir()
            {
                collect(root, &path, files);
            } else {
                files.push(
                    path.strip_prefix(root)
                        .expect("relative generation path")
                        .into(),
                );
            }
        }
    }

    let mut files = Vec::new();
    collect(root, root, &mut files);
    files.sort_by(|left, right| {
        left.as_os_str()
            .as_encoded_bytes()
            .cmp(right.as_os_str().as_encoded_bytes())
    });
    let mut hasher = blake3::Hasher::new();
    for relative in files {
        let relative_bytes = relative.as_os_str().as_encoded_bytes();
        let bytes = fs::read(root.join(&relative)).expect("read generation file");
        hasher.update(&(relative_bytes.len() as u64).to_be_bytes());
        hasher.update(relative_bytes);
        hasher.update(&(bytes.len() as u64).to_be_bytes());
        hasher.update(&bytes);
    }
    hasher.finalize().to_hex().to_string()
}

fn assert_no_building_entry(root: &std::path::Path) {
    assert!(fs::read_dir(root)
        .expect("read index root")
        .all(|entry| !entry
            .expect("read index entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".building-")));
}

fn selected_generation(root: &std::path::Path) -> PathBuf {
    let current = fs::read_to_string(root.join("CURRENT")).expect("read CURRENT");
    root.join(current.trim())
}

fn first_bundle_document(fixture: &P2Fixture) -> PathBuf {
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.bundle_root.join("manifest.json")).expect("read bundle manifest"),
    )
    .expect("parse bundle manifest");
    fixture.bundle_root.join(
        manifest["documents"][0]["output_path"]
            .as_str()
            .expect("first bundle output path"),
    )
}

fn write_bundle_manifest(fixture: &P2Fixture, manifest: &serde_json::Value) {
    let mut bytes = serde_json::to_vec_pretty(manifest).expect("serialize bundle manifest");
    bytes.push(b'\n');
    fs::write(fixture.bundle_root.join("manifest.json"), bytes).expect("write bundle manifest");
}

fn diagnostic_codes(error: &IndexError) -> Vec<String> {
    let IndexError::BundleUnverified(diagnostics) = error else {
        panic!("expected Level 3 bundle rejection, got {error:?}");
    };
    diagnostics
        .iter()
        .map(|diagnostic| {
            serde_json::to_value(diagnostic).expect("serialize diagnostic")["code"]
                .as_str()
                .expect("diagnostic code")
                .to_owned()
        })
        .collect()
}

#[test]
fn fixture_bootstrap_git_bypasses_ambient_path_shims() {
    let command = git_command(std::path::Path::new("/synthetic/vault"));

    assert_eq!(command.get_program(), std::ffi::OsStr::new("/usr/bin/git"));
}

#[test]
fn exposes_the_exact_p2_schema_and_tokenizer_contract() {
    let descriptor = index_schema_descriptor();
    assert_eq!(descriptor.schema_id, "sovmem-tantivy-schema-v1");
    assert_eq!(descriptor.tokenizer, "sovmem-simple-v1");
    assert_eq!(descriptor.ranking, "sovmem-bm25-v1");
    assert_eq!(
        descriptor.fields,
        ["id", "title", "body", "layer", "type", "tags", "provenance"]
    );
    assert_eq!(descriptor.token_limit_bytes, 80);
    assert_eq!(descriptor.title_boost, "3.0");
    assert_eq!(descriptor.tags_boost, "2.0");
    assert_eq!(descriptor.body_boost, "1.0");
    assert_eq!(descriptor.tantivy_version, "0.24.2");
    assert_eq!(descriptor.petgraph_version, "0.8.3");
}

#[test]
fn prepares_exactly_sixty_two_canonical_documents_from_level_three() {
    let fixture = P2Fixture::new("verified-intake");
    let corpus = prepare_verified_corpus(&fixture.profile_path, &fixture.bundle_root)
        .expect("Level 3 corpus prepares");
    assert_eq!(corpus.documents, 62);
    assert_eq!(corpus.core, 8);
    assert_eq!(corpus.research, 54);
    assert_eq!(
        corpus.source_commit,
        "43824cfd9caed7b260aa442128fd1130ddb6e2ff"
    );
    assert_eq!(corpus.document_ids.len(), 62);
    assert_eq!(
        corpus.document_ids.iter().collect::<BTreeSet<_>>().len(),
        62
    );
    assert!(corpus.source_paths.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(corpus.documents_hash.len(), 64);
}

#[test]
fn rejects_non_level_three_input_before_creating_an_index_root() {
    let fixture = P2Fixture::new("missing-corpus-report");
    let index_root = fixture.index_root("index");
    fixture.remove_corpus_report();
    let error = prepare_verified_corpus(&fixture.profile_path, &fixture.bundle_root)
        .expect_err("missing Level 3 evidence must fail");
    assert_eq!(error.code(), "index.bundle_unverified");
    assert!(!index_root.exists());
}

#[test]
fn build_rejects_non_level_three_input_before_creating_an_index_root() {
    let fixture = P2Fixture::new("build-missing-corpus-report");
    let index_root = fixture.index_root("index");
    fixture.remove_corpus_report();
    let error = build_index(request(&fixture, index_root.clone()))
        .expect_err("missing Level 3 evidence must fail");
    assert_eq!(error.code(), "index.bundle_unverified");
    assert!(!index_root.exists());
}

#[test]
fn every_level_three_corruption_rejects_rebuild_and_preserves_current_generation() {
    type Mutation = fn(&P2Fixture);
    let cases: &[(&str, Mutation, &str)] = &[
        (
            "missing-report",
            P2Fixture::remove_corpus_report,
            "bundle.corpus_report_required",
        ),
        (
            "changed-document",
            |fixture| {
                let path = first_bundle_document(fixture);
                let mut bytes = fs::read(&path).expect("read generated document");
                bytes.extend_from_slice(b"\nprivate-changed-document-sentinel\n");
                fs::write(path, bytes).expect("change generated document bytes");
            },
            "bundle.document_hash_mismatch",
        ),
        (
            "repaired-document-hash",
            |fixture| {
                let path = first_bundle_document(fixture);
                let mut bytes = fs::read(&path).expect("read generated document");
                bytes.extend_from_slice(b"\nprivate-repaired-root-sentinel\n");
                fs::write(&path, &bytes).expect("change generated document bytes");
                let mut manifest: serde_json::Value = serde_json::from_slice(
                    &fs::read(fixture.bundle_root.join("manifest.json"))
                        .expect("read bundle manifest"),
                )
                .expect("parse bundle manifest");
                manifest["documents"][0]["hash"] =
                    serde_json::json!(blake3::hash(&bytes).to_hex().to_string());
                write_bundle_manifest(fixture, &manifest);
            },
            "bundle.root_hash_mismatch",
        ),
        (
            "invalid-provenance",
            |fixture| {
                let path = first_bundle_document(fixture);
                let bytes = fs::read_to_string(&path).expect("read generated document");
                assert!(bytes.contains("provenance: synthesis"));
                fs::write(
                    path,
                    bytes.replacen(
                        "provenance: synthesis",
                        "provenance: private-invalid-provenance-sentinel",
                        1,
                    ),
                )
                .expect("write invalid provenance");
            },
            "document.provenance_invalid",
        ),
    ];

    for (name, mutate, required_diagnostic) in cases {
        let fixture = P2Fixture::new(&format!("index-corruption-{name}"));
        let root = fixture.index_root("index");
        fixture.build_index(&root);
        let current_before = fs::read(root.join("CURRENT")).expect("read prior CURRENT");
        let selected_before = semantic_tree_hash(&selected_generation(&root));

        mutate(&fixture);
        let error = build_index(request(&fixture, root.clone()))
            .expect_err("corrupt Level 3 bundle must reject rebuild");
        assert_eq!(error.code(), "index.bundle_unverified", "case {name}");
        assert!(
            diagnostic_codes(&error)
                .iter()
                .any(|code| code == required_diagnostic),
            "case {name} missing {required_diagnostic}"
        );
        assert_eq!(
            fs::read(root.join("CURRENT")).unwrap(),
            current_before,
            "case {name}"
        );
        assert_eq!(
            semantic_tree_hash(&selected_generation(&root)),
            selected_before,
            "case {name}"
        );
    }
}

#[test]
fn body_only_link_does_not_enter_the_graph_projection() {
    let fixture = P2Fixture::new("body-only-link");
    let mut documents = fixture.generated_documents();
    let target = documents[19].metadata.id.clone();
    let source = documents.last().unwrap().metadata.id.clone();
    documents.last_mut().unwrap().body.push_str(&format!(
        "\n[body-only URI evidence](vault://{target}#body-only-link)\n"
    ));
    let graph = derive_graph(&fixture.bundle_hash(), &documents).expect("derive graph projection");

    assert!(!graph.edges.iter().any(|edge| {
        edge.source == source && edge.target == target && edge.relation == "references"
    }));
    assert_eq!(graph.edges.len(), 19);
}

#[test]
fn publishes_first_build_to_slot_a_and_second_to_slot_b() {
    let fixture = P2Fixture::new("slot-switch");
    let root = fixture.index_root("index");
    let first = build_index(request(&fixture, root.clone())).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("CURRENT")).unwrap(),
        "slot-a\n"
    );
    let second = build_index(request(&fixture, root.clone())).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("CURRENT")).unwrap(),
        "slot-b\n"
    );
    assert_eq!(first.bundle_hash, second.bundle_hash);
    assert_eq!(first.index_config, second.index_config);
    assert_eq!(first.documents, 62);
    assert_eq!(first.graph_nodes, 62);
    assert!(!first.replaced_invalid_current);
    assert!(!second.replaced_invalid_current);
    let summary = serde_json::to_string(&second).expect("serialize index summary");
    assert!(!summary.contains(&root.to_string_lossy().to_string()));
    assert!(!summary.contains("slot-"));
    assert!(!summary.contains(".building-"));
    assert_no_building_entry(&root);
}

#[test]
fn failed_rebuild_preserves_the_selected_complete_generation() {
    let fixture = P2Fixture::new("rollback");
    let root = fixture.index_root("index");
    build_index(request(&fixture, root.clone())).unwrap();
    let before = semantic_tree_hash(&root.join("slot-a"));
    fs::write(root.join("slot-b"), b"blocked inactive slot")
        .expect("block inactive slot with a non-directory");
    let error = build_index(request(&fixture, root.clone()))
        .expect_err("blocked inactive slot must fail publication");
    assert_eq!(error.code(), "index.publish_failed");
    assert_eq!(
        fs::read_to_string(root.join("CURRENT")).unwrap(),
        "slot-a\n"
    );
    assert_eq!(semantic_tree_hash(&root.join("slot-a")), before);
    assert_no_building_entry(&root);
}

#[test]
fn pointer_preparation_failure_preserves_the_selected_complete_generation() {
    let fixture = P2Fixture::new("pointer-rollback");
    let root = fixture.index_root("index");
    build_index(request(&fixture, root.clone())).unwrap();
    let before = semantic_tree_hash(&root.join("slot-a"));
    fs::create_dir(root.join("CURRENT.next")).expect("block temporary pointer path");
    let error = build_index(request(&fixture, root.clone()))
        .expect_err("blocked temporary pointer must fail publication");
    assert_eq!(error.code(), "index.publish_failed");
    assert_eq!(
        fs::read_to_string(root.join("CURRENT")).unwrap(),
        "slot-a\n"
    );
    assert_eq!(semantic_tree_hash(&root.join("slot-a")), before);
    assert!(root.join("slot-b/index-manifest.json").is_file());
    assert_no_building_entry(&root);
}

#[test]
fn invalid_current_values_are_replaced_by_a_clean_full_rebuild() {
    let fixture = P2Fixture::new("invalid-current-values");
    for (name, value) in [
        ("malformed", b"slot-a".as_slice()),
        ("traversing", b"../slot-a\n".as_slice()),
        ("absolute", b"/tmp/slot-a\n".as_slice()),
        ("temporary", b".building-interrupted\n".as_slice()),
    ] {
        let root = fixture.index_root(name);
        fs::create_dir(&root).expect("create invalid-current index root");
        fs::write(root.join("CURRENT"), value).expect("write invalid CURRENT");
        let stale = root.join(".building-interrupted");
        fs::create_dir(&stale).expect("create stale unpublished generation");
        fs::write(stale.join("sentinel"), b"unpublished").expect("write unpublished sentinel");

        let summary = build_index(request(&fixture, root.clone()))
            .expect("valid bundle rebuild replaces invalid CURRENT");
        assert!(summary.replaced_invalid_current, "case {name}");
        assert_eq!(
            fs::read_to_string(root.join("CURRENT")).unwrap(),
            "slot-a\n",
            "case {name}"
        );
        assert!(!stale.exists(), "case {name}");
        assert_no_building_entry(&root);
    }
}

#[cfg(unix)]
#[test]
fn current_directory_is_replaced_by_a_clean_full_rebuild() {
    use std::os::unix::fs::symlink;

    let fixture = P2Fixture::new("current-directory");
    let root = fixture.index_root("index");
    fs::create_dir(&root).expect("create current-directory index root");
    let current = root.join("CURRENT");
    let nested = current.join("nested");
    fs::create_dir_all(&nested).expect("create invalid non-empty CURRENT directory");
    fs::write(nested.join("sentinel"), b"invalid current state")
        .expect("write nested invalid-current sentinel");
    let external_target = fixture.index_root("outside-current-directory-target");
    fs::write(&external_target, b"must remain untouched").expect("write external symlink target");
    symlink(&external_target, current.join("external-link"))
        .expect("create invalid-current symlink leaf");

    let summary = build_index(request(&fixture, root.clone()))
        .expect("valid bundle rebuild replaces invalid CURRENT directory");
    assert!(summary.replaced_invalid_current);
    assert_eq!(fs::read(root.join("CURRENT")).unwrap(), b"slot-a\n");
    assert_eq!(
        fs::read(&external_target).unwrap(),
        b"must remain untouched"
    );
    assert!(fs::symlink_metadata(root.join("CURRENT"))
        .unwrap()
        .file_type()
        .is_file());
    assert!(root.join("slot-a/index-manifest.json").is_file());
    assert_no_building_entry(&root);
}

#[test]
fn exact_current_without_a_complete_generation_is_replaced_safely() {
    let fixture = P2Fixture::new("incomplete-current");
    let root = fixture.index_root("index");
    fs::create_dir(&root).expect("create incomplete-current index root");
    fs::write(root.join("CURRENT"), b"slot-a\n").expect("write exact incomplete CURRENT");

    let summary = build_index(request(&fixture, root.clone()))
        .expect("full rebuild replaces incomplete current generation");
    assert!(summary.replaced_invalid_current);
    assert_eq!(
        fs::read_to_string(root.join("CURRENT")).unwrap(),
        "slot-b\n"
    );
    assert!(root.join("slot-b/index-manifest.json").is_file());
}

#[cfg(unix)]
#[test]
fn symlink_current_is_replaced_without_following_its_target() {
    use std::os::unix::fs::symlink;

    let fixture = P2Fixture::new("symlink-current");
    let root = fixture.index_root("index");
    fs::create_dir(&root).expect("create symlink-current index root");
    let target = fixture.index_root("outside-current-target");
    fs::write(&target, b"slot-b\n").expect("write external pointer target");
    symlink(&target, root.join("CURRENT")).expect("create CURRENT symlink");

    let summary = build_index(request(&fixture, root.clone()))
        .expect("full rebuild replaces unsafe CURRENT symlink");
    assert!(summary.replaced_invalid_current);
    assert_eq!(fs::read(&target).unwrap(), b"slot-b\n");
    assert_eq!(
        fs::read_to_string(root.join("CURRENT")).unwrap(),
        "slot-a\n"
    );
    assert!(!fs::symlink_metadata(root.join("CURRENT"))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn rejects_equal_ancestor_and_descendant_bundle_index_overlap() {
    let fixture = P2Fixture::new("overlap");
    let descendant = fixture.bundle_root.join("derived-index");
    for index_root in [
        fixture.bundle_root.clone(),
        fixture
            .bundle_root
            .parent()
            .expect("bundle parent")
            .to_path_buf(),
        descendant.clone(),
    ] {
        let error =
            build_index(request(&fixture, index_root)).expect_err("bundle/index overlap must fail");
        assert_eq!(error.code(), "index.output_invalid");
    }
    assert!(!descendant.exists());
}

#[cfg(unix)]
#[test]
fn rejects_a_symlink_index_root_alias_without_mutating_the_bundle() {
    use std::os::unix::fs::symlink;

    let fixture = P2Fixture::new("symlink-index-root");
    let bundle_hash = fixture.bundle_hash();
    let alias = fixture.index_root("index-alias");
    symlink(&fixture.bundle_root, &alias).expect("create index root symlink alias");
    let error = build_index(request(&fixture, alias)).expect_err("symlink output must fail");
    assert_eq!(error.code(), "index.output_invalid");
    assert_eq!(fixture.bundle_hash(), bundle_hash);
}
