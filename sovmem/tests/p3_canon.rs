use sovmem::profile::p3_internal::p3::P3Fixture;
use sovmem::profile::{build_judgment, load_p3_profile, Reason, Timestamp, VerdictVerb};
use std::path::Path;

#[test]
fn generation_input_identity_uses_the_active_profile_namespace() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut profile = load_p3_profile(&root.join("profiles/audy-ssa0-p3-v2/profile.yaml"))
        .expect("load P3 profile");
    profile.config.context_namespace = "ssa-alt".to_owned();
    let _target_path = "04-Research/Alternate-Namespace.md";
    let target_id = "ssa-alt/04-Research/Alternate-Namespace.md";
    let verdict_id = format!("ver-{}", "b".repeat(64));
    let judgment = build_judgment(
        &profile,
        &verdict_id,
        VerdictVerb::Accept,
        target_id,
        &Reason::new(b"alternate namespace", 4096).expect("reason"),
        &Timestamp::parse("2026-07-14T04:30:00.000000000Z").expect("timestamp"),
    )
    .expect("build judgment")
    .expect("accept produces judgment");

    assert_eq!(judgment.id, format!("ssa-alt/judgments/{verdict_id}.md"));
}

#[test]
fn seed_rebinding_normalizes_resolvable_wikilinks_before_the_level_three_check() {
    let fixture = P3Fixture::new_with_resolvable_wikilink("canon-seed-wikilink");

    let generation = fixture.stage_accept_create();

    assert!(generation
        .generated_documents
        .iter()
        .any(|document| document.metadata.source_path == "00-Index.md"));
}
