use sovmem::profile::load_p3_profile;
use sovmem::profile::p3_test_support::ProvisionedP3Fixture;
use sovmem::profile::stats::{stats_snapshot_fingerprint, store_stats_with_observer};

fn scratch(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("sovmem-stats-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir(&path).unwrap();
    path
}

#[test]
fn stats_fail_closed_when_a_verified_proposal_changes_before_render() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("stats-snapshot-mutation");
    let profile = load_p3_profile(&fixture.profile_path).unwrap();
    let candidate = fixture
        .store_root
        .join("intake/proposals")
        .join(&fixture.proposal_id[4..6])
        .join(&fixture.proposal_id)
        .join("candidate.md");
    let mutated_candidate = b"mutated after verification";
    assert_ne!(std::fs::read(&candidate).unwrap(), mutated_candidate);
    let result = store_stats_with_observer(&profile, &fixture.store_root, || {
        std::fs::write(&candidate, mutated_candidate).unwrap();
    });
    assert_eq!(std::fs::read(&candidate).unwrap(), mutated_candidate);
    assert_eq!(
        result.as_ref().err().map(|error| error.code()),
        Some("store.changed_during_read")
    );
}

#[test]
fn snapshot_fingerprint_frames_records_without_structural_collisions() {
    let first = scratch("collision-first");
    std::fs::write(first.join("a"), b"X").unwrap();
    std::fs::write(first.join("b"), b"Y").unwrap();
    let second = scratch("collision-second");
    std::fs::write(second.join("a"), b"X\0b\0fileY").unwrap();
    assert_ne!(
        stats_snapshot_fingerprint(&first).unwrap(),
        stats_snapshot_fingerprint(&second).unwrap()
    );
    std::fs::remove_dir_all(first).unwrap();
    std::fs::remove_dir_all(second).unwrap();
}

#[cfg(unix)]
#[test]
fn snapshot_fingerprint_binds_derived_root_but_excludes_only_descendants() {
    use std::os::unix::fs::symlink;

    let root = scratch("derived-root");
    std::fs::create_dir(root.join("derived")).unwrap();
    let before = stats_snapshot_fingerprint(&root).unwrap();
    std::fs::write(root.join("derived/disposable-index"), b"churn").unwrap();
    assert_eq!(before, stats_snapshot_fingerprint(&root).unwrap());

    std::fs::remove_file(root.join("derived/disposable-index")).unwrap();
    std::fs::remove_dir(root.join("derived")).unwrap();
    let outside = scratch("derived-outside");
    symlink(&outside, root.join("derived")).unwrap();
    assert_eq!(
        stats_snapshot_fingerprint(&root).unwrap_err().code(),
        "store.symlink"
    );
    std::fs::remove_file(root.join("derived")).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}
