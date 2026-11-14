use sovmem::profile::{load_corpus, load_profile, CorpusLane, CorpusTheme};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const PINNED_SOURCE_COMMIT: &str = "43824cfd9caed7b260aa442128fd1130ddb6e2ff";
const REMOVED_V0_CANDIDATE: &str = "04-Research/DSOM/source-pages/dsom-automation-encyclopedia-docs-tools__howto-privacy-guardian.md";
const ADDED_V1_CANDIDATE: &str = "04-Research/DSOM/source-pages/2.-operational-rituals__digital-sovereignty-operational-model-palace.md";

fn corpus_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/retrieval-corpus-v1.yaml")
}

fn v0_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/retrieval-corpus.yaml")
}

fn profile_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/profile.yaml")
}

fn temporary_yaml(contents: &str) -> TemporaryYaml {
    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "sovmem-corpus-config-{}-{sequence}.yaml",
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

#[test]
fn loads_the_approved_exact_corpus_contract() {
    let corpus = load_corpus(&corpus_path()).expect("approved corpus loads");
    assert_eq!(corpus.schema, "audy-retrieval-corpus-v1");
    assert_eq!(corpus.source_commit, PINNED_SOURCE_COMMIT);
    assert_eq!(corpus.minimum_documents, 53);
    assert_eq!(corpus.minimum_research_documents, 45);
    assert_eq!(corpus.core.len(), 8);
    assert_eq!(corpus.research.len(), 55);
    assert_eq!(corpus.selected_paths().len(), 63);
    assert_eq!(
        corpus
            .selected_paths()
            .into_iter()
            .collect::<BTreeSet<_>>()
            .len(),
        63
    );

    let mut themes = BTreeMap::new();
    for entry in &corpus.research {
        *themes.entry(entry.theme.clone()).or_insert(0usize) += 1;
    }
    assert_eq!(themes[&CorpusTheme::TfisSsa], 12);
    assert_eq!(themes[&CorpusTheme::SovereignContext], 15);
    assert_eq!(themes[&CorpusTheme::AgentOperations], 14);
    assert_eq!(themes[&CorpusTheme::GovernanceEvidence], 14);

    assert!(corpus
        .entry("01-Soul/Audy Himura.md")
        .is_some_and(|entry| entry.lane == CorpusLane::Core));
    assert!(corpus.entry(REMOVED_V0_CANDIDATE).is_none());
    let replacement = corpus.entry(ADDED_V1_CANDIDATE).unwrap();
    assert_eq!(replacement.lane, CorpusLane::Research);
    assert_eq!(replacement.theme, Some(&CorpusTheme::GovernanceEvidence));
}

#[test]
fn preserves_v0_as_historical_evidence_rejected_by_the_v1_runtime() {
    let source = std::fs::read_to_string(v0_path()).expect("v0 corpus is readable");
    assert_eq!(
        source.lines().next(),
        Some("schema: audy-retrieval-corpus-v0")
    );
    assert!(source.contains(REMOVED_V0_CANDIDATE));
    assert!(!source.contains(ADDED_V1_CANDIDATE));
    assert_eq!(load_corpus(&v0_path()).unwrap_err().code(), "corpus.schema");
}

#[test]
fn every_selected_path_has_an_existing_profile_mapping() {
    let corpus = load_corpus(&corpus_path()).expect("approved corpus loads");
    let profile = load_profile(&profile_path()).expect("profile loads");
    for path in corpus.selected_paths() {
        assert!(
            profile.mapping_for(&path).is_some(),
            "missing mapping for {path}"
        );
    }
}

struct InvalidCase {
    name: &'static str,
    expected_code: &'static str,
    mutate: fn(&mut serde_yaml::Value),
}

#[test]
fn rejects_invalid_corpus_configuration_with_stable_codes() {
    let cases = [
        InvalidCase {
            name: "unsupported schema",
            expected_code: "corpus.schema",
            mutate: |yaml| yaml["schema"] = serde_yaml::Value::from("future-corpus-v1"),
        },
        InvalidCase {
            name: "non-committed mode",
            expected_code: "corpus.schema",
            mutate: |yaml| yaml["mode"] = serde_yaml::Value::from("working"),
        },
        InvalidCase {
            name: "abbreviated source commit",
            expected_code: "corpus.commit",
            mutate: |yaml| yaml["source_commit"] = serde_yaml::Value::from("43824cfd"),
        },
        InvalidCase {
            name: "uppercase source commit",
            expected_code: "corpus.commit",
            mutate: |yaml| {
                yaml["source_commit"] =
                    serde_yaml::Value::from("43824CFD9caed7b260aa442128fd1130ddb6e2ff")
            },
        },
        InvalidCase {
            name: "different full lowercase source commit",
            expected_code: "corpus.commit",
            mutate: |yaml| {
                yaml["source_commit"] =
                    serde_yaml::Value::from("0000000000000000000000000000000000000000")
            },
        },
        InvalidCase {
            name: "missing required core path",
            expected_code: "corpus.core",
            mutate: |yaml| {
                yaml["core"].as_sequence_mut().expect("core sequence").pop();
            },
        },
        InvalidCase {
            name: "duplicate required core path",
            expected_code: "corpus.core",
            mutate: |yaml| {
                let core = yaml["core"].as_sequence_mut().expect("core sequence");
                core[7] = core[0].clone();
            },
        },
        InvalidCase {
            name: "extra core path",
            expected_code: "corpus.core",
            mutate: |yaml| {
                yaml["core"]
                    .as_sequence_mut()
                    .expect("core sequence")
                    .push(serde_yaml::from_str("{ path: Extra.md, role: doctrine }").unwrap());
            },
        },
        InvalidCase {
            name: "role-mismatched required core path",
            expected_code: "corpus.core",
            mutate: |yaml| {
                yaml["core"][0]["role"] = serde_yaml::Value::from("doctrine");
            },
        },
        InvalidCase {
            name: "fewer than fifty-five research entries",
            expected_code: "corpus.research",
            mutate: |yaml| {
                yaml["research"]
                    .as_sequence_mut()
                    .expect("research sequence")
                    .truncate(54);
            },
        },
        InvalidCase {
            name: "fifty-six otherwise-valid research entries",
            expected_code: "corpus.research",
            mutate: |yaml| {
                yaml["research"]
                    .as_sequence_mut()
                    .expect("research sequence")
                    .push(
                        serde_yaml::from_str(
                            "{ path: 04-Research/Extra-55.md, theme: governance-evidence }",
                        )
                        .unwrap(),
                    );
            },
        },
        InvalidCase {
            name: "more than sixty research entries",
            expected_code: "corpus.research",
            mutate: |yaml| {
                let research = yaml["research"]
                    .as_sequence_mut()
                    .expect("research sequence");
                for index in 0..6 {
                    research.push(
                        serde_yaml::from_str(&format!(
                            "{{ path: 04-Research/Extra-{index}.md, theme: governance-evidence }}"
                        ))
                        .unwrap(),
                    );
                }
            },
        },
        InvalidCase {
            name: "valid-count research path replacement",
            expected_code: "corpus.contract",
            mutate: |yaml| {
                yaml["research"][0]["path"] =
                    serde_yaml::Value::from("04-Research/Reviewed-Replacement.md");
            },
        },
        InvalidCase {
            name: "balanced research theme swap",
            expected_code: "corpus.contract",
            mutate: |yaml| {
                let research = yaml["research"].as_sequence_mut().unwrap();
                let first = research
                    .iter()
                    .position(|entry| entry["theme"] == "tfis-ssa")
                    .unwrap();
                let second = research
                    .iter()
                    .position(|entry| entry["theme"] == "agent-operations")
                    .unwrap();
                let first_theme = research[first]["theme"].clone();
                research[first]["theme"] = research[second]["theme"].clone();
                research[second]["theme"] = first_theme;
            },
        },
        InvalidCase {
            name: "unknown research theme",
            expected_code: "corpus.theme",
            mutate: |yaml| {
                yaml["research"][0]["theme"] = serde_yaml::Value::from("unknown-theme");
            },
        },
        InvalidCase {
            name: "tfis-ssa distribution below ten",
            expected_code: "corpus.theme",
            mutate: |yaml| {
                let research = yaml["research"]
                    .as_sequence_mut()
                    .expect("research sequence");
                for entry in research.iter_mut().take(3) {
                    entry["theme"] = serde_yaml::Value::from("agent-operations");
                }
            },
        },
        InvalidCase {
            name: "sovereign-context distribution below fifteen",
            expected_code: "corpus.theme",
            mutate: |yaml| {
                yaml["research"][12]["theme"] = serde_yaml::Value::from("tfis-ssa");
            },
        },
        InvalidCase {
            name: "agent-operations distribution below ten",
            expected_code: "corpus.theme",
            mutate: |yaml| {
                let research = yaml["research"]
                    .as_sequence_mut()
                    .expect("research sequence");
                for entry in research.iter_mut().skip(27).take(5) {
                    entry["theme"] = serde_yaml::Value::from("governance-evidence");
                }
            },
        },
        InvalidCase {
            name: "governance-evidence distribution below ten",
            expected_code: "corpus.theme",
            mutate: |yaml| {
                let research = yaml["research"]
                    .as_sequence_mut()
                    .expect("research sequence");
                for entry in research.iter_mut().skip(41).take(5) {
                    entry["theme"] = serde_yaml::Value::from("agent-operations");
                }
            },
        },
        InvalidCase {
            name: "wildcard selected path",
            expected_code: "corpus.path",
            mutate: |yaml| {
                yaml["research"][0]["path"] = serde_yaml::Value::from("04-Research/*.md");
            },
        },
        InvalidCase {
            name: "traversing selected path",
            expected_code: "corpus.path",
            mutate: |yaml| {
                yaml["research"][0]["path"] = serde_yaml::Value::from("04-Research/../Private.md");
            },
        },
        InvalidCase {
            name: "duplicate selected path",
            expected_code: "corpus.path",
            mutate: |yaml| {
                let first = yaml["research"][0]["path"].clone();
                yaml["research"][1]["path"] = first;
            },
        },
        InvalidCase {
            name: "research path outside 04-Research",
            expected_code: "corpus.path",
            mutate: |yaml| {
                yaml["research"][0]["path"] = serde_yaml::Value::from("03-Protocols/One.md");
            },
        },
        InvalidCase {
            name: "forbidden lane is not excluded",
            expected_code: "corpus.path",
            mutate: |yaml| {
                yaml["exclude"]
                    .as_sequence_mut()
                    .expect("exclude sequence")
                    .remove(0);
            },
        },
        InvalidCase {
            name: "selected path matched by exclusion",
            expected_code: "corpus.selection",
            mutate: |yaml| {
                let selected = yaml["research"][0]["path"].clone();
                yaml["exclude"]
                    .as_sequence_mut()
                    .expect("exclude sequence")
                    .push(selected);
            },
        },
    ];

    let source = std::fs::read_to_string(corpus_path()).expect("approved corpus is readable");
    let approved: serde_yaml::Value =
        serde_yaml::from_str(&source).expect("approved corpus parses");

    for case in cases {
        let mut yaml = approved.clone();
        (case.mutate)(&mut yaml);
        let serialized = serde_yaml::to_string(&yaml).expect("case serializes");
        let file = temporary_yaml(&serialized);
        let error = load_corpus(file.path()).expect_err(case.name);
        assert_eq!(error.code(), case.expected_code, "{}", case.name);
    }
}
