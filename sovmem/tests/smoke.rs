#[allow(dead_code)]
mod support;

use sovmem::PROFILE_SCHEMA_VERSION;

#[test]
fn exposes_the_approved_profile_schema() {
    assert_eq!(PROFILE_SCHEMA_VERSION, "nous-ssa-context-v0");
}
