mod canon;
mod clock;
mod config;
mod corpus;
mod corpus_report;
mod eval;
mod export;
mod frontmatter;
mod golden;
mod graph;
pub mod index;
#[allow(dead_code)]
pub mod intake;
mod judgment;
mod ledger;
mod links;
mod mapping;
#[path = "../../tests/support/p2.rs"]
pub mod p2_fixture;
#[cfg(any(test, feature = "test-support"))]
pub mod p3_internal;
pub mod p3_test_support;
mod policy;
#[allow(dead_code)]
// Task 4 wires these crate-private staging primitives into policy-gated submission.
pub mod proposal;
mod query;
mod source;
pub mod stats;
pub mod store;
mod thresholds;
pub mod transaction;
pub mod trust;
mod validate;
mod verify;

use serde::{Deserialize, Serialize};

pub use canon::*;
pub use clock::*;
pub use config::*;
pub use corpus::*;
pub use corpus_report::*;
pub use eval::*;
pub use export::*;
pub use frontmatter::*;
pub use golden::*;
pub use graph::*;
pub use index::*;
#[allow(unused_imports)]
pub use intake::*;
pub use judgment::*;
pub use ledger::*;
pub use links::*;
pub use mapping::*;
pub use policy::*;
pub use proposal::*;
pub use query::*;
pub use source::*;
pub use stats::*;
pub use store::*;
pub use thresholds::*;
#[allow(unused_imports)]
pub use trust::*;
pub use validate::*;
pub use verify::*;

// All pub items from transaction are accessible.
pub use transaction::*;

pub fn p3_canonical_document_id(namespace: &str, canonical_path: &str) -> Option<String> {
    if !valid_relative_path(canonical_path) || !canonical_path.ends_with(".md") {
        return None;
    }
    let identity = canonical_source_path_identity(canonical_path).ok()?;
    Some(format!("{namespace}/{identity}.md"))
}

#[cfg(any(test, feature = "test-support"))]
mod p3_identity_tests {
    #[allow(unused_imports)]
    use super::p3_canonical_document_id;

    #[test]
    fn p3_document_identity_requires_a_canonical_markdown_path() {
        assert_eq!(
            p3_canonical_document_id("ssa-alt", "04-Research/Audy Himura.md").as_deref(),
            Some("ssa-alt/04-Research/Audy%20Himura.md")
        );
        for path in [
            "04-Research/Audy-Himura",
            "04-Research/../Audy-Himura.md",
            "04-Research//Audy-Himura.md",
            "/04-Research/Audy-Himura.md",
            "04-Research\\Audy-Himura.md",
        ] {
            assert_eq!(
                p3_canonical_document_id("ssa-alt", path),
                None,
                "path {path:?} must be rejected"
            );
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct BundleManifest {
    pub(crate) okf_version: String,
    pub(crate) profile: String,
    pub(crate) nous_schema: String,
    pub(crate) ssa_id: String,
    pub(crate) ssa_profile: String,
    pub(crate) familiar_id: String,
    pub(crate) context_namespace: String,
    pub(crate) vault_id: String,
    pub(crate) exporter_version: String,
    pub(crate) commit: String,
    pub(crate) mode: String,
    pub(crate) working_tree_allowed: bool,
    pub(crate) inclusion: Vec<String>,
    pub(crate) exclusion: Vec<String>,
    pub(crate) excluded_sources: Vec<ManifestExclusion>,
    pub(crate) sources: Vec<ManifestSource>,
    pub(crate) documents: Vec<ManifestDocument>,
    pub(crate) bundle_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestExclusion {
    pub(crate) source_path: String,
    pub(crate) reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestSource {
    pub(crate) source_path: String,
    pub(crate) hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestDocument {
    pub(crate) source_path: String,
    pub(crate) output_path: String,
    pub(crate) id: String,
    pub(crate) resource: String,
    pub(crate) hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct BundleExportReport {
    pub(crate) counts: ExportCounts,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExportCounts {
    pub(crate) selected: usize,
    pub(crate) exported: usize,
    pub(crate) excluded: usize,
    pub(crate) warnings: usize,
    pub(crate) errors: usize,
}
