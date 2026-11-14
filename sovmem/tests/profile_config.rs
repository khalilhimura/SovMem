use sovmem::profile::{
    glob_matches, load_canary, load_p3_profile, load_profile, CanaryConfig, CoreType,
    ProfileConfig, P3_PROFILE_VERSION,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn profile_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/profile.yaml")
}

fn p3_profile_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-p3-v2/profile.yaml")
}

fn temporary_yaml(contents: &str) -> TemporaryYaml {
    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "sovmem-profile-config-{}-{sequence}.yaml",
        std::process::id()
    ));
    std::fs::write(&path, contents).expect("temporary YAML is written");
    TemporaryYaml(path)
}

struct TemporaryYaml(PathBuf);

impl TemporaryYaml {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryYaml {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn temporary_p3_profile(contents: &str) -> TemporaryP3Profile {
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "sovmem-p3-profile-config-{}-{sequence}",
        std::process::id()
    ));
    let directory = root.join(P3_PROFILE_VERSION);
    std::fs::create_dir_all(&directory).expect("temporary P3 profile directory is created");
    let path = directory.join("profile.yaml");
    std::fs::write(&path, contents).expect("temporary P3 profile is written");
    TemporaryP3Profile { root, path }
}

struct TemporaryP3Profile {
    root: PathBuf,
    path: PathBuf,
}

impl Drop for TemporaryP3Profile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn loads_the_audy_reference_profile() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    assert_eq!(profile.nous_schema, "nous-ssa-context-v0");
    assert_eq!(profile.profile, "nous-sovmem-v0");
    assert_eq!(profile.ssa_id, "ssa-0");
    assert_eq!(profile.ssa_profile, "audy-ssa0-v0");
    assert_eq!(profile.familiar_id, "audy");
    assert_eq!(profile.context_namespace, "ssa-0");
    assert_eq!(profile.context_kinds.len(), 23);
    assert!(profile
        .context_kinds
        .values()
        .all(|core_type| *core_type != CoreType::Judgment));
}

#[test]
fn confines_the_human_judgment_extension_to_the_versioned_p3_loader() {
    let p2_error = load_profile(&p3_profile_path()).expect_err("P2 loader rejects judgment");
    assert_eq!(p2_error.code(), "profile.kind_mapping");

    let p3 = load_p3_profile(&p3_profile_path()).expect("P3 profile loads");
    assert_eq!(p3.version, P3_PROFILE_VERSION);
    assert_eq!(p3.config.context_kinds.len(), 24);
    assert_eq!(
        p3.config.context_kinds.get("Human Judgment"),
        Some(&CoreType::Judgment)
    );
}

#[test]
fn p3_loader_rejects_an_unknown_top_level_profile_field() {
    let source = std::fs::read_to_string(p3_profile_path()).expect("P3 profile is readable");
    let file = temporary_p3_profile(&(source + "\nprivate_unknown_p3_field: ignored-before-fix\n"));

    let error = load_p3_profile(&file.path).expect_err("unknown P3 profile field must fail closed");

    assert_eq!(error.code(), "profile.parse");
}

#[test]
fn rejects_any_variant_of_the_reviewed_p3_judgment_extension() {
    let source = std::fs::read_to_string(p3_profile_path()).expect("P3 profile is readable");
    let cases = [
        (
            "extra judgment kind",
            source.replacen(
                "  Human Judgment: judgment\n",
                "  Human Judgment: judgment\n  Other Judgment: judgment\n",
                1,
            ),
        ),
        (
            "renamed judgment kind",
            source.replacen("Human Judgment: judgment", "Judgment: judgment", 1),
        ),
        (
            "broader judgment mapping",
            source.replacen("judgments/*.md", "judgments/**/*.md", 1),
        ),
        (
            "noncanonical judgment mapping",
            source.replacen(
                "context_kind: \"Human Judgment\", layer: concept, authority: canonical",
                "context_kind: \"Human Judgment\", layer: concept, authority: reviewed",
                1,
            ),
        ),
        (
            "broader mapping before judgment mapping",
            source.replacen(
                "mapping_rules:\n",
                "mapping_rules:\n  - { pattern: \"**\", core_type: concept, context_kind: Doctrine, layer: synthesis, authority: canonical }\n",
                1,
            ),
        ),
        (
            "different judgment sensitivity",
            source.replacen(
                "- { pattern: \"judgments/*.md\", sensitivity: internal }",
                "- { pattern: \"judgments/*.md\", sensitivity: private }",
                1,
            ),
        ),
        (
            "broader sensitivity before judgment sensitivity",
            source.replacen(
                "sensitivity_rules:\n",
                "sensitivity_rules:\n  - { pattern: \"**\", sensitivity: private }\n",
                1,
            ),
        ),
    ];

    for (name, contents) in cases {
        let file = temporary_p3_profile(&contents);
        let error = load_p3_profile(&file.path).expect_err(name);
        assert_eq!(error.code(), "profile.kind_mapping", "{name}");
    }
}

#[test]
fn rejects_an_identity_namespace_mismatch() {
    let mut profile = load_profile(&profile_path()).expect("profile loads");
    profile.context_namespace = "ssa-1".into();
    let error = profile.validate().expect_err("mismatch must fail");
    assert_eq!(error.code(), "profile.namespace_mismatch");
}

#[test]
fn rejects_invalid_profile_configuration() {
    struct Case {
        name: &'static str,
        expected_code: &'static str,
        mutate: fn(&mut ProfileConfig),
    }

    let cases = [
        Case {
            name: "empty identifier",
            expected_code: "profile.identifier",
            mutate: |profile| profile.familiar_id.clear(),
        },
        Case {
            name: "unsupported mapped core type",
            expected_code: "profile.kind_mapping",
            mutate: |profile| {
                profile
                    .context_kinds
                    .insert("Doctrine".into(), CoreType::Judgment);
            },
        },
        Case {
            name: "kind mapped to no core type",
            expected_code: "profile.kind_mapping",
            mutate: |profile| profile.mapping_rules[0].context_kind = "Unknown Kind".into(),
        },
        Case {
            name: "duplicate mapping precedence",
            expected_code: "profile.mapping_precedence",
            mutate: |profile| profile.mapping_rules.push(profile.mapping_rules[0].clone()),
        },
        Case {
            name: "vault identifier contains a slash",
            expected_code: "profile.identifier",
            mutate: |profile| profile.vault_id = "audy/vault".into(),
        },
    ];

    for case in cases {
        let mut profile = load_profile(&profile_path()).expect("reference profile loads");
        (case.mutate)(&mut profile);
        let error = profile.validate().expect_err(case.name);
        assert_eq!(error.code(), case.expected_code, "{}", case.name);
    }
}

#[test]
fn rejects_an_unsupported_serialized_core_type() {
    let source = std::fs::read_to_string(profile_path()).expect("reference profile is readable");
    let invalid = source.replacen("Doctrine: concept", "Doctrine: unsupported", 1);
    let file = temporary_yaml(&invalid);

    let error = load_profile(file.path()).expect_err("unknown enum value must fail");
    assert_eq!(error.code(), "profile.parse");
}

#[test]
fn rejects_an_unsupported_schema() {
    let mut profile = load_profile(&profile_path()).expect("reference profile loads");
    profile.nous_schema = "nous-ssa-context-v1".into();

    let error = profile.validate().expect_err("unknown schema must fail");
    assert_eq!(error.code(), "profile.schema");
}

#[test]
fn glob_matching_is_segment_aware_and_supports_double_star() {
    let cases = [
        ("AGENTS.md", "AGENTS.md", true),
        ("01-Soul/*.md", "01-Soul/Audy Himura.md", true),
        ("01-Soul/*.md", "01-Soul/nested/Doctrine.md", false),
        (
            "04-Research/**/source-pages/*.md",
            "04-Research/source-pages/Source.md",
            true,
        ),
        (
            "04-Research/**/source-pages/*.md",
            "04-Research/OKF/source-pages/Source.md",
            true,
        ),
        (
            "04-Research/**/*Concept-Map*.md",
            "04-Research/OKF/OKF-Concept-Map-v1.md",
            true,
        ),
        (
            "04-Research/**/*Concept-Map*.md",
            "04-Research/OKF/Gap-Analysis.md",
            false,
        ),
        ("loops/**/archive/*.md", "loops/a/b/archive/out.md", true),
    ];

    for (pattern, path, expected) in cases {
        assert_eq!(glob_matches(pattern, path), expected, "{pattern} vs {path}");
    }
}

#[test]
fn mappings_and_sensitivity_use_stored_precedence() {
    let profile = load_profile(&profile_path()).expect("profile loads");

    let identity = profile
        .mapping_for("01-Soul/Audy Himura.md")
        .expect("identity mapping exists");
    assert_eq!(identity.context_kind, "Identity");

    let doctrine = profile
        .mapping_for("01-Soul/SSA Archetypes.md")
        .expect("doctrine mapping exists");
    assert_eq!(doctrine.context_kind, "Doctrine");

    assert_eq!(
        format!("{:?}", profile.sensitivity_for("02-Memory/MEMORY.md")),
        "Private"
    );
    assert_eq!(
        format!("{:?}", profile.sensitivity_for("unmapped/note.md")),
        "Internal"
    );
}

#[test]
fn rejects_equal_specificity_overlapping_patterns() {
    let mut profile = load_profile(&profile_path()).expect("reference profile loads");
    let mut overlapping = profile.mapping_rules[0].clone();
    profile.mapping_rules[0].pattern = "docs/*-Map.md".into();
    overlapping.pattern = "docs/Map-*.md".into();
    profile.mapping_rules[1] = overlapping;

    let error = profile
        .validate()
        .expect_err("ambiguous equal-specificity rules must fail");
    assert_eq!(error.code(), "profile.mapping_precedence");
}

#[test]
fn loads_and_validates_committed_canary_selection() {
    let file = temporary_yaml(
        r#"mode: committed
source_commit: 43824cfd9caed7b260aa442128fd1130ddb6e2ff
include:
  - 01-Soul/*.md
exclude:
  - 01-Soul/Private.md
"#,
    );

    let canary = load_canary(file.path()).expect("canary loads");
    assert_canary(&canary);
}

fn assert_canary(canary: &CanaryConfig) {
    assert_eq!(
        canary.source_commit,
        "43824cfd9caed7b260aa442128fd1130ddb6e2ff"
    );
    assert_eq!(canary.include, ["01-Soul/*.md"]);
    assert_eq!(canary.exclude, ["01-Soul/Private.md"]);
}

#[test]
fn rejects_invalid_canary_configuration_with_stable_codes() {
    let cases = [
        (
            "unsupported mode",
            "mode: working\nsource_commit: 43824cfd9caed7b260aa442128fd1130ddb6e2ff\ninclude: [\"*.md\"]\nexclude: []\n",
            "canary.mode",
        ),
        (
            "abbreviated commit",
            "mode: committed\nsource_commit: 43824cfd\ninclude: [\"*.md\"]\nexclude: []\n",
            "canary.commit",
        ),
        (
            "empty selection",
            "mode: committed\nsource_commit: 43824cfd9caed7b260aa442128fd1130ddb6e2ff\ninclude: []\nexclude: []\n",
            "canary.selection",
        ),
        (
            "absolute selection",
            "mode: committed\nsource_commit: 43824cfd9caed7b260aa442128fd1130ddb6e2ff\ninclude: [\"/tmp/*.md\"]\nexclude: []\n",
            "canary.selection",
        ),
        (
            "backslash selection",
            "mode: committed\nsource_commit: 43824cfd9caed7b260aa442128fd1130ddb6e2ff\ninclude: ['04-Research\\*.md']\nexclude: []\n",
            "canary.selection",
        ),
        (
            "dot segment selection",
            "mode: committed\nsource_commit: 43824cfd9caed7b260aa442128fd1130ddb6e2ff\ninclude: [\"04-Research/./*.md\"]\nexclude: []\n",
            "canary.selection",
        ),
        (
            "empty segment selection",
            "mode: committed\nsource_commit: 43824cfd9caed7b260aa442128fd1130ddb6e2ff\ninclude: [\"04-Research//*.md\"]\nexclude: []\n",
            "canary.selection",
        ),
        (
            "control character selection",
            r#"mode: committed
source_commit: 43824cfd9caed7b260aa442128fd1130ddb6e2ff
include: ["04-Research/\0*.md"]
exclude: []
"#,
            "canary.selection",
        ),
    ];

    for (name, yaml, expected_code) in cases {
        let file = temporary_yaml(yaml);
        let error = load_canary(file.path()).expect_err(name);
        assert_eq!(error.code(), expected_code, "{name}");
    }
}
