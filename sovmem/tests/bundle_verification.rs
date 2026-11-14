#[allow(dead_code)]
mod support;

use sovmem::profile::{
    export_canary, load_profile, verify_bundle, ExportRequest, VerificationLevel,
    VerificationOptions,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use support::GitFixture;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);
const SOURCE_PATH: &str = "04-Research/OKF/README.md";

struct VerificationFixture {
    vault: GitFixture,
    root: PathBuf,
    profile: PathBuf,
    canary: PathBuf,
    bundle: PathBuf,
}

impl VerificationFixture {
    fn new(name: &str) -> Self {
        let vault = GitFixture::new(name);
        vault.write(
            SOURCE_PATH,
            "# Verification fixture\n\nThis fixture provides deterministic verification evidence.\n",
        );
        vault.commit_all("add verification fixture", "2026-01-02T12:00:00Z");

        let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "sovmem-bundle-verification-{name}-{}-{sequence}",
            std::process::id(),
        ));
        fs::create_dir(&root).expect("create verification area");
        let canary = root.join("canary.yaml");
        fs::write(
            &canary,
            format!(
                "mode: committed\nsource_commit: {}\ninclude:\n  - {SOURCE_PATH}\nexclude: []\n",
                vault.head(),
            ),
        )
        .expect("write canary");

        Self {
            vault,
            profile: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../profiles/audy-ssa0-v0/profile.yaml"),
            bundle: root.join("bundle"),
            canary,
            root,
        }
    }
}

impl Drop for VerificationFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn extracted_verifier_preserves_the_valid_canary_contract() {
    let fixture = VerificationFixture::new("module-compatibility");
    export_canary(ExportRequest {
        vault_root: fixture.vault.root.clone(),
        output_root: fixture.bundle.clone(),
        profile_path: fixture.profile.clone(),
        canary_path: fixture.canary.clone(),
    })
    .expect("fixture exports");
    let profile = load_profile(&fixture.profile).expect("profile loads");
    let root = fs::canonicalize(&fixture.bundle).expect("bundle canonicalizes");

    let result = verify_bundle(&profile, &root, VerificationOptions::compatibility(false));

    assert_eq!(result.document_count(), 1);
    assert!(!result.has_errors(), "{:?}", result.diagnostics());
    assert_eq!(result.bundle_hash().len(), 64);
}

#[test]
fn explicit_levels_are_cumulative_at_the_document_hash_boundary() {
    let fixture = VerificationFixture::new("module-level-boundary");
    export_canary(ExportRequest {
        vault_root: fixture.vault.root.clone(),
        output_root: fixture.bundle.clone(),
        profile_path: fixture.profile.clone(),
        canary_path: fixture.canary.clone(),
    })
    .expect("fixture exports");
    let document = fixture.bundle.join(format!("bundle/{SOURCE_PATH}"));
    let mut bytes = fs::read(&document).expect("read generated document");
    bytes.extend_from_slice(b"\n");
    fs::write(&document, bytes).expect("modify document bytes");
    let profile = load_profile(&fixture.profile).expect("profile loads");
    let root = fs::canonicalize(&fixture.bundle).expect("bundle canonicalizes");

    let level_one = verify_bundle(
        &profile,
        &root,
        VerificationOptions::level(VerificationLevel::One),
    );
    let level_two = verify_bundle(
        &profile,
        &root,
        VerificationOptions::level(VerificationLevel::Two),
    );

    assert!(!level_one.has_errors(), "{:?}", level_one.diagnostics());
    assert!(level_two.has_errors());
}

#[test]
fn level_three_requires_corpus_while_compatibility_keeps_it_optional() {
    let fixture = VerificationFixture::new("module-level-three");
    export_canary(ExportRequest {
        vault_root: fixture.vault.root.clone(),
        output_root: fixture.bundle.clone(),
        profile_path: fixture.profile.clone(),
        canary_path: fixture.canary.clone(),
    })
    .expect("fixture exports");
    let profile = load_profile(&fixture.profile).expect("profile loads");
    let root = fs::canonicalize(&fixture.bundle).expect("bundle canonicalizes");

    let compatibility = verify_bundle(&profile, &root, VerificationOptions::compatibility(false));
    let level_three = verify_bundle(
        &profile,
        &root,
        VerificationOptions::level(VerificationLevel::Three),
    );

    assert!(!compatibility.has_errors());
    assert!(level_three.has_errors());
    assert!(level_three
        .diagnostics()
        .iter()
        .any(|diagnostic| format!("{diagnostic:?}").contains("bundle.corpus_report_required")));
}
