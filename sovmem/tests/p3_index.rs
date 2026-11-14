use serde_json::Value;
use sovmem::profile::index::{
    allocate_store_build_paths_with, build_generation, build_schema,
    build_verified_index_for_platform, canonical_manifest_bytes, ensure_publish_supported,
    finish_store_publication_after_pointer_with, open_generation, open_selected_generation,
    parse_current, publish_current_pointer, publish_current_pointer_with,
    publish_store_current_pointer, read_current_pointer, read_store_current_pointer,
    rebuild_store_index, rebuild_store_index_with_test_hook, register_tokenizer,
    retrieval_config_hash, store_index_input_matches, CurrentPointer, IndexDocument,
    IndexInputBinding, IndexInputKind, IndexManifest, StoreCurrentPointer, StoreIndexRequest,
    VerifiedCorpus, INDEX_MANIFEST_SCHEMA_V1,
};
use sovmem::profile::p3_test_support::ProvisionedP3Fixture;
use sovmem::profile::{VerdictVerb, GRAPH_SCHEMA, P3_STORE_ID};
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

struct GenerationFixture {
    root: PathBuf,
    corpus: VerifiedCorpus,
}

fn canonical_json(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).expect("serialize canonical JSON");
    bytes.push(b'\n');
    bytes
}

impl GenerationFixture {
    fn new(name: &str) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must follow the Unix epoch")
            .as_nanos();
        let sequence = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-generation-{name}-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create generation fixture root");

        let documents = vec![
            IndexDocument {
                source_path: "01-alpha.md".into(),
                id: "ssa-fixture/alpha".into(),
                title: "Alpha title".into(),
                body: "PRIVATE_ALPHA_BODY".into(),
                layer: "analysis".into(),
                core_type: "concept".into(),
                tags: vec!["alpha".into(), "shared".into()],
                provenance: "synthesis".into(),
            },
            IndexDocument {
                source_path: "02-beta.md".into(),
                id: "ssa-fixture/beta".into(),
                title: "Beta title".into(),
                body: "PRIVATE_BETA_BODY".into(),
                layer: "synthesis".into(),
                core_type: "resource".into(),
                tags: vec!["beta".into(), "shared".into()],
                provenance: "tier1".into(),
            },
        ];
        let documents_hash = sovmem::profile::index::canonical_document_hash(&documents);
        let bundle_hash = "a".repeat(64);
        let graph = sovmem::profile::GraphProjection {
            schema_version: GRAPH_SCHEMA.into(),
            bundle_hash: bundle_hash.clone(),
            nodes: vec![
                sovmem::profile::GraphNode {
                    id: documents[0].id.clone(),
                },
                sovmem::profile::GraphNode {
                    id: documents[1].id.clone(),
                },
            ],
            edges: vec![sovmem::profile::GraphEdge {
                source: documents[0].id.clone(),
                relation: "references".into(),
                target: documents[1].id.clone(),
            }],
            external_typed_links: 1,
        };
        let corpus = VerifiedCorpus {
            profile: "generation-fixture".into(),
            context_namespace: "ssa-fixture".into(),
            source_commit: "b".repeat(40),
            input: IndexInputBinding {
                kind: IndexInputKind::FrozenCorpus,
                store_id: None,
                generation_id: None,
                manifest_hash: "c".repeat(64),
            },
            bundle_hash,
            core: 1,
            research: 1,
            graph,
            documents,
            documents_hash,
        };

        Self { root, corpus }
    }

    fn generation_root(&self) -> PathBuf {
        self.root.join("generation")
    }

    fn corrupt_generation(&self, generation: &Path, mutation: &str) {
        match mutation {
            "manifest" => {
                let path = generation.join("index-manifest.json");
                let mut value: Value =
                    serde_json::from_slice(&fs::read(&path).expect("read manifest for corruption"))
                        .expect("parse manifest for corruption");
                value["unexpected"] = Value::Bool(true);
                fs::write(path, canonical_json(&value)).expect("corrupt manifest");
            }
            "graph" => {
                fs::write(generation.join("graph.json"), b"{}\n").expect("corrupt graph");
            }
            "schema" => {
                let tantivy_root = generation.join("tantivy");
                fs::remove_dir_all(&tantivy_root).expect("remove fixture Tantivy index");
                fs::create_dir(&tantivy_root).expect("recreate fixture Tantivy root");
                let mut builder = tantivy::schema::Schema::builder();
                builder.add_text_field("wrong", tantivy::schema::STRING | tantivy::schema::STORED);
                let index = tantivy::Index::create_in_dir(&tantivy_root, builder.build())
                    .expect("create mismatched schema");
                let mut writer: tantivy::IndexWriter<tantivy::TantivyDocument> = index
                    .writer_with_num_threads(1, 50_000_000)
                    .expect("open mismatched writer");
                writer.commit().expect("commit mismatched schema");
                writer
                    .wait_merging_threads()
                    .expect("finish mismatched schema");
            }
            "document-set" => {
                let index = tantivy::Index::open_in_dir(generation.join("tantivy"))
                    .expect("open fixture Tantivy index");
                register_tokenizer(&index);
                let (_, fields) = build_schema();
                let mut writer = index
                    .writer_with_num_threads(1, 50_000_000)
                    .expect("open fixture writer");
                let mut document = tantivy::TantivyDocument::new();
                document.add_text(fields.id, "ssa-fixture/unexpected");
                document.add_text(fields.title, "Unexpected title");
                document.add_text(fields.body, "Unexpected body");
                document.add_text(fields.layer, "analysis");
                document.add_text(fields.core_type, "concept");
                document.add_text(fields.tags, "unexpected");
                document.add_text(fields.provenance, "synthesis");
                writer
                    .add_document(document)
                    .expect("add unexpected document");
                writer.commit().expect("commit unexpected document");
                writer
                    .wait_merging_threads()
                    .expect("finish unexpected document");
            }
            "tantivy" => {
                fs::remove_file(generation.join("tantivy/meta.json"))
                    .expect("remove Tantivy metadata");
            }
            other => panic!("unknown generation corruption {other}"),
        }
    }
}

impl Drop for GenerationFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn builds_and_reopens_one_complete_generation() {
    let fixture = GenerationFixture::new("complete-generation");
    let generation = fixture.generation_root();
    let built = build_generation(&fixture.corpus, &generation).expect("build generation");
    assert_eq!(built.documents, 2);
    assert_eq!(built.graph_nodes, 2);
    assert!(generation.join("tantivy/meta.json").is_file());
    assert!(generation.join("graph.json").is_file());
    assert!(generation.join("index-manifest.json").is_file());

    let persisted = format!(
        "{}{}",
        fs::read_to_string(generation.join("graph.json")).expect("read graph"),
        fs::read_to_string(generation.join("index-manifest.json")).expect("read manifest")
    );
    assert!(!persisted.contains("PRIVATE_ALPHA_BODY"));
    assert!(!persisted.contains("PRIVATE_BETA_BODY"));
    assert!(!persisted.contains(&fixture.root.to_string_lossy().to_string()));

    let opened = open_generation(&fixture.corpus, &generation).expect("reopen generation");
    assert_eq!(opened.manifest.documents, 2);
    assert_eq!(opened.metadata.len(), 2);
}

#[test]
fn legacy_v1_index_manifest_remains_openable_only_for_frozen_input() {
    let mut fixture = GenerationFixture::new("legacy-v1-index");
    let generation = fixture.generation_root();
    build_generation(&fixture.corpus, &generation).expect("build v2 frozen generation");
    let path = generation.join("index-manifest.json");
    let mut manifest: IndexManifest =
        serde_json::from_slice(&fs::read(&path).expect("read v2 manifest"))
            .expect("parse v2 manifest");
    manifest.schema_version = INDEX_MANIFEST_SCHEMA_V1.to_owned();
    manifest.input_kind = None;
    manifest.store_id = None;
    manifest.generation_id = None;
    manifest.input_manifest_hash = None;
    fs::write(
        &path,
        canonical_manifest_bytes(&manifest).expect("render v1 manifest"),
    )
    .expect("write v1 manifest");
    open_generation(&fixture.corpus, &generation).expect("legacy frozen v1 opens");

    fixture.corpus.input = IndexInputBinding {
        kind: IndexInputKind::P3Canon,
        store_id: Some(P3_STORE_ID.to_owned()),
        generation_id: Some("gen-0000000000000001".to_owned()),
        manifest_hash: "d".repeat(64),
    };
    assert_eq!(
        open_generation(&fixture.corpus, &generation)
            .expect_err("P3 input must never accept legacy v1")
            .code(),
        "index.input_mismatch"
    );
}

#[test]
fn retrieval_config_hash_is_stable_lowercase_hex() {
    let first = retrieval_config_hash();
    assert_eq!(first.len(), 64);
    assert!(first
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    assert_eq!(first, retrieval_config_hash());
}

#[test]
fn rejects_manifest_graph_schema_and_document_set_corruption() {
    for (mutation, expected_code) in [
        ("manifest", "index.manifest_invalid"),
        ("graph", "index.graph_invalid"),
        ("schema", "index.schema_mismatch"),
        ("document-set", "index.document_set_invalid"),
        ("tantivy", "index.tantivy_invalid"),
    ] {
        let fixture = GenerationFixture::new(mutation);
        let generation = fixture.generation_root();
        build_generation(&fixture.corpus, &generation).expect("build fixture generation");
        fixture.corrupt_generation(&generation, mutation);
        let error =
            open_generation(&fixture.corpus, &generation).expect_err("corruption must fail");
        assert_eq!(error.code(), expected_code, "mutation {mutation}");
    }
}

#[test]
fn current_pointer_parser_accepts_only_the_two_exact_slot_values() {
    assert_eq!(parse_current(b"slot-a\n").unwrap(), "slot-a");
    assert_eq!(parse_current(b"slot-b\n").unwrap(), "slot-b");
    for invalid in [
        b"".as_slice(),
        b"slot-a".as_slice(),
        b"../slot-a\n".as_slice(),
        b"/slot-a\n".as_slice(),
        b".building-1-2\n".as_slice(),
        b"slot-b\nextra".as_slice(),
    ] {
        assert_eq!(
            parse_current(invalid).unwrap_err().code(),
            "index.current_invalid"
        );
    }
}

#[test]
fn current_open_rejects_absent_unsafe_and_temporary_pointers() {
    let fixture = GenerationFixture::new("unsafe-current-open");
    let index_root = fixture.root.join("index-root");
    fs::create_dir(&index_root).expect("create current-open root");
    for current in [
        None,
        Some(b"slot-a".as_slice()),
        Some(b"../slot-a\n".as_slice()),
        Some(b"/slot-a\n".as_slice()),
        Some(b".building-complete\n".as_slice()),
    ] {
        let _ = fs::remove_file(index_root.join("CURRENT"));
        if let Some(bytes) = current {
            fs::write(index_root.join("CURRENT"), bytes).expect("write unsafe CURRENT");
        }
        let error = open_selected_generation(&fixture.corpus, &index_root)
            .expect_err("unsafe CURRENT must not select any generation");
        assert_eq!(error.code(), "index.current_invalid");
    }
}

#[test]
fn unsupported_platform_predicate_has_a_stable_error() {
    assert_eq!(
        ensure_publish_supported(false).unwrap_err().code(),
        "index.publish_unsupported"
    );
    ensure_publish_supported(true).expect("supported platform predicate");
}

#[test]
fn unsupported_platform_pipeline_stops_before_index_root_or_staging() {
    let fixture = GenerationFixture::new("unsupported-pipeline");
    let bundle_root = fixture.root.join("verified-bundle-root");
    let index_root = fixture.root.join("index-root");
    fs::create_dir(&bundle_root).expect("create verified bundle root stand-in");

    let error =
        build_verified_index_for_platform(&fixture.corpus, &bundle_root, &index_root, false)
            .expect_err("unsupported platform must stop the verified build pipeline");
    assert_eq!(error.code(), "index.publish_unsupported");
    assert!(!index_root.exists());
    assert!(fs::read_dir(&fixture.root)
        .expect("read unsupported pipeline fixture")
        .all(|entry| !entry
            .expect("read unsupported pipeline entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".building-")));
}

#[test]
fn store_rebuild_head_change_is_stale_at_the_prepublication_barrier() {
    assert!(store_index_input_matches(
        b"gen-0000000000000001\n",
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        "gen-0000000000000001",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ));
    assert!(!store_index_input_matches(
        b"gen-0000000000000001\n",
        Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        "gen-0000000000000001",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ));
    assert!(!store_index_input_matches(
        b"gen-0000000000000002\n",
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        "gen-0000000000000001",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ));
}

#[test]
fn production_rebuild_hook_cleans_staging_and_preserves_index_on_head_change() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("store-index-head-change");
    fixture
        .apply(VerdictVerb::Accept, "apply fixture before rebuild")
        .expect("seal one applied generation");
    rebuild_store_index(StoreIndexRequest {
        profile_path: fixture.profile_path.clone(),
        store_root: fixture.store_root.clone(),
    })
    .expect("publish the prior authoritative index");
    let before = fixture.index_hashes();
    let index_root = fixture
        .store_root
        .join("derived/indexes/gen-0000000000000001");
    let selected_before = fs::read(index_root.join("CURRENT")).unwrap();
    let current = fixture.store_root.join("canon/CURRENT");
    let ledger = fixture.store_root.join("ledger/verdicts.jsonl");
    let error = rebuild_store_index_with_test_hook(
        StoreIndexRequest {
            profile_path: fixture.profile_path.clone(),
            store_root: fixture.store_root.clone(),
        },
        || {
            fs::write(&current, b"gen-0000000000000002\n").unwrap();
            let mut bytes = fs::read(&ledger).unwrap();
            bytes.extend_from_slice(b"PRIVATE_CHANGED_HEAD_LEDGER_903");
            fs::write(&ledger, bytes).unwrap();
        },
    )
    .expect_err("changed authoritative head must fail before slot installation");
    assert_eq!(error.code(), "index.input_stale");
    assert!(!format!("{error:?}").contains("PRIVATE_CHANGED_HEAD_LEDGER_903"));
    assert_eq!(fixture.index_hashes(), before);
    assert_eq!(
        fs::read(index_root.join("CURRENT")).unwrap(),
        selected_before
    );
    assert_eq!(
        fs::read_dir(&index_root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("slot-"))
            .count(),
        1,
        "the newly installed stale slot must be removed"
    );
    assert!(fs::read_dir(&index_root).unwrap().all(|entry| {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        !name.starts_with(".slot-building-") && !name.starts_with("CURRENT.next-")
    }));
}

#[test]
fn production_rebuild_crash_after_slot_install_can_retry() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("store-index-crash-retry");
    fixture
        .apply(VerdictVerb::Accept, "apply fixture before rebuild")
        .expect("seal one applied generation");
    rebuild_store_index(StoreIndexRequest {
        profile_path: fixture.profile_path.clone(),
        store_root: fixture.store_root.clone(),
    })
    .expect("publish an initial selected slot");
    let index_root = fixture
        .store_root
        .join("derived/indexes/gen-0000000000000001");
    let selected_before = fs::read(index_root.join("CURRENT")).unwrap();

    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = rebuild_store_index_with_test_hook(
            StoreIndexRequest {
                profile_path: fixture.profile_path.clone(),
                store_root: fixture.store_root.clone(),
            },
            || panic!("injected crash after installed-slot self-check"),
        );
    }));
    assert!(crashed.is_err());
    assert_eq!(
        fs::read(index_root.join("CURRENT")).unwrap(),
        selected_before
    );

    rebuild_store_index(StoreIndexRequest {
        profile_path: fixture.profile_path.clone(),
        store_root: fixture.store_root.clone(),
    })
    .expect("a disposable prepublication crash must not permanently block retry");
}

#[test]
fn store_build_allocation_skips_restart_collisions_without_deleting_them() {
    let fixture = GenerationFixture::new("store-build-restart-collisions");
    let indexes = fixture.root.join("indexes");
    let final_root = indexes.join("gen-0000000000000001");
    fs::create_dir_all(&final_root).unwrap();
    let stale_staging = "11111111111111111111111111111111";
    let stale_slot = "22222222222222222222222222222222";
    let stale_next = "33333333333333333333333333333333";
    let fresh = "44444444444444444444444444444444";
    fs::create_dir(final_root.join(format!(".slot-building-{stale_staging}"))).unwrap();
    fs::create_dir(final_root.join(format!("slot-{stale_slot}"))).unwrap();
    fs::write(
        final_root.join(format!("CURRENT.next-{stale_next}")),
        b"stale",
    )
    .unwrap();
    let mut tokens = [stale_staging, stale_slot, stale_next, fresh].into_iter();

    let (slot, staging) =
        allocate_store_build_paths_with(&final_root, || tokens.next().unwrap().to_owned())
            .expect("restart collisions must be skipped");

    assert_eq!(slot, format!("slot-{fresh}"));
    assert_eq!(staging, final_root.join(format!(".slot-building-{fresh}")));
    assert!(final_root
        .join(format!(".slot-building-{stale_staging}"))
        .exists());
    assert!(final_root.join(format!("slot-{stale_slot}")).exists());
    assert!(final_root
        .join(format!("CURRENT.next-{stale_next}"))
        .exists());
}

#[test]
fn store_pointer_is_synced_before_best_effort_cleanup_failure() {
    let fixture = GenerationFixture::new("store-pointer-cleanup-failure");
    let root = fixture.root.join("store-index");
    let old = "slot-11111111111111111111111111111111";
    let new = "slot-22222222222222222222222222222222";
    fs::create_dir_all(root.join(old)).unwrap();
    fs::create_dir(root.join(new)).unwrap();
    fs::write(root.join("CURRENT"), format!("{old}\n")).unwrap();
    publish_store_current_pointer(&root, new).unwrap();
    let calls = std::cell::RefCell::new(Vec::new());

    finish_store_publication_after_pointer_with(
        &root,
        new,
        Some(old),
        |_| {
            calls.borrow_mut().push("sync");
            Ok(())
        },
        |_| {
            calls.borrow_mut().push("remove");
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        },
    )
    .expect("cleanup failure after durable publication is best effort");

    assert_eq!(&*calls.borrow(), &["sync", "remove", "sync"]);
    assert_eq!(
        read_store_current_pointer(&root),
        StoreCurrentPointer::Exact(new.to_owned())
    );
    assert!(root.join(old).exists());
}

#[test]
fn production_verdict_list_rejects_transient_created_after_level4() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("verdict-list-level4-race");
    fixture
        .apply(VerdictVerb::Reject, "seal rejected fixture")
        .expect("seal fixture");
    let profile = sovmem::profile::load_p3_profile(&fixture.profile_path).unwrap();
    let marker = fixture
        .store_root
        .join(format!("canon/.current-txn-{}", "9".repeat(64)));

    let error = sovmem::profile::proposal::verdict_list_with_test_hook(
        &profile,
        &fixture.store_root,
        false,
        false,
        || fs::write(&marker, b"PRIVATE_LIST_RACE_BODY_902").unwrap(),
    )
    .expect_err("post-Level-4 transient must fail closed");

    assert_eq!(error.code(), "transaction.incomplete");
    let diagnostic = format!("{error:?}");
    assert!(!diagnostic.contains("PRIVATE_LIST_RACE_BODY_902"));
}

#[test]
fn store_slot_crash_and_pointer_failure_keep_the_old_slot_selected() {
    let fixture = GenerationFixture::new("store-slot-crash");
    let root = fixture.root.join("store-index");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("slot-a")).unwrap();
    fs::write(root.join("CURRENT"), b"slot-a\n").unwrap();

    // A crash after installing the inactive slot but before publishing the
    // pointer leaves the prior slot selected.
    fs::create_dir(root.join("slot-b")).unwrap();
    assert_eq!(read_current_pointer(&root), CurrentPointer::Exact("slot-a"));

    assert_eq!(
        publish_current_pointer_with(&root, "slot-b", |_, _| {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        })
        .unwrap_err()
        .code(),
        "index.publish_failed"
    );
    assert_eq!(fs::read(root.join("CURRENT")).unwrap(), b"slot-a\n");
    assert!(!root.join("CURRENT.next").exists());

    // A prepublication obstruction fails without replacing CURRENT.
    fs::create_dir(root.join("CURRENT.next")).unwrap();
    assert_eq!(
        publish_current_pointer(&root, "slot-b").unwrap_err().code(),
        "index.publish_failed"
    );
    assert_eq!(fs::read(root.join("CURRENT")).unwrap(), b"slot-a\n");
    assert_eq!(read_current_pointer(&root), CurrentPointer::Exact("slot-a"));
}
