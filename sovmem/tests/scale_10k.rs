use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

/// Path to the sovmem binary relative to the crate root.
fn sovmem_binary() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target");
    path.push("debug");
    path.push("sovmem");
    path
}

fn create_10k_corpus(dir: &std::path::Path) {
    let bundle = dir.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    for i in 0..10_000 {
        let content = format!(
            "---\nid: doc-{i:05}\ntitle: \"Test {i}\"\ntype: concept\nlayer: concept\ntags: []\nprovenance: synthesis\ncreated: 2026-08-17\nupdated: 2026-08-17\n---\n\nBody text.\n"
        );
        std::fs::write(bundle.join(format!("doc-{i:05}.md")), &content).unwrap();
    }
}

#[test]
#[ignore]
fn rebuild_under_60_seconds() {
    let sovmem = sovmem_binary();
    assert!(
        sovmem.exists(),
        "sovmem binary not found at {}",
        sovmem.display()
    );

    let dir = std::env::temp_dir().join(format!("sovmem-scale-10k-{}", std::process::id()));
    let bundle = dir.join("bundle");
    let index = dir.join("index");

    // Phase 1: generate 10k valid versioned Markdown files
    let gen_start = Instant::now();
    create_10k_corpus(&dir);
    let gen_time = gen_start.elapsed();
    eprintln!("Generation: {gen_time:?}");

    let doc_count = std::fs::read_dir(&bundle).unwrap().count();
    assert_eq!(doc_count, 10_000, "must have 10k documents");

    // Phase 2: verify the bundle (public verify boundary)
    let verify_start = Instant::now();
    let verify_output = Command::new(&sovmem)
        .args([
            "verify",
            "--profile",
            "profiles/audy-ssa0-v0/profile.yaml",
            "--bundle",
            &bundle.to_string_lossy(),
            "--level",
            "3",
        ])
        .output()
        .expect("sovmem verify must execute");
    let verify_time = verify_start.elapsed();
    eprintln!("Verify --level 3: {verify_time:?}");

    assert!(
        verify_output.status.success(),
        "sovmem verify --level 3 failed:\n{}",
        String::from_utf8_lossy(&verify_output.stderr)
    );

    // Phase 3: build the Tantivy index (public index boundary)
    let index_start = Instant::now();
    let index_output = Command::new(&sovmem)
        .args([
            "index",
            "--profile",
            "profiles/audy-ssa0-v0/profile.yaml",
            "--bundle",
            &bundle.to_string_lossy(),
            "--index",
            &index.to_string_lossy(),
        ])
        .output()
        .expect("sovmem index must execute");
    let index_time = index_start.elapsed();
    eprintln!("Index: {index_time:?}");

    assert!(
        index_output.status.success(),
        "sovmem index failed:\n{}",
        String::from_utf8_lossy(&index_output.stderr)
    );

    // Phase 4: verify index document count
    let index_stdout = String::from_utf8_lossy(&index_output.stdout);
    assert!(
        index_stdout.contains("\"documents\": 10000")
            || index_stdout.contains("documents: 10000")
            || index_stdout.contains("\"documents\":10000"),
        "index must report 10000 documents, got: {index_stdout}"
    );

    // Phase 5: total elapsed must be under 60 seconds on named hardware
    let total = gen_time + verify_time + index_time;
    eprintln!("Total (gen + verify + index): {total:?}");
    assert!(
        total.as_secs() < 60,
        "total time {total:?} exceeds 60-second budget on {}/{} ({})",
        std::env::consts::ARCH,
        std::env::consts::FAMILY,
        std::env::consts::OS,
    );

    // Cleanup
    std::fs::remove_dir_all(&dir).unwrap();
}
