use sovmem::profile::{
    convert_links, load_profile, normalize_document, parse_source_document, GeneratedDocument,
    LinkRelation, SourceDocument, TypedLink,
};
use std::path::{Path, PathBuf};

fn profile_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../profiles/audy-ssa0-v0/profile.yaml")
}

fn document(path: &str, contents: &str) -> GeneratedDocument {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let source = SourceDocument {
        source_path: path.into(),
        bytes: contents.as_bytes().to_vec(),
        source_hash: blake3::hash(contents.as_bytes()).to_hex().to_string(),
        created: "2026-01-02".into(),
        updated: "2026-01-05".into(),
    };
    let parsed = parse_source_document(source).expect("source parses");
    normalize_document(&profile, parsed).expect("source normalizes")
}

fn converted<'a>(documents: &'a [GeneratedDocument], source_path: &str) -> &'a GeneratedDocument {
    documents
        .iter()
        .find(|document| document.metadata.source_path == source_path)
        .expect("converted document exists")
}

#[test]
fn converts_an_exact_relative_aliased_wikilink() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let result = convert_links(
        &profile,
        vec![
            document(
                "04-Research/Notes/README.md",
                "See [[../OKF/OKF-Concept-Map|Map]].\n",
            ),
            document(
                "04-Research/OKF/OKF-Concept-Map.md",
                "# OKF Concept Map\n\nMap body.\n",
            ),
        ],
    )
    .expect("links convert");

    let converted = converted(&result.documents, "04-Research/Notes/README.md");
    assert_eq!(converted.body, "See [Map](../OKF/OKF-Concept-Map.md).\n");
    assert_eq!(
        converted.metadata.links,
        [TypedLink {
            rel: LinkRelation::References,
            target: "ssa-0/04-Research/OKF/OKF-Concept-Map".into(),
        }]
    );
    assert!(result.warnings.is_empty());
}

#[test]
fn exact_path_wins_before_basename_and_preserves_heading_fragments() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let result = convert_links(
        &profile,
        vec![
            document(
                "04-Research/Notes/README.md",
                "See [[../OKF/OKF-Concept-Map#Core Ideas|core ideas]].\n",
            ),
            document(
                "04-Research/OKF/OKF-Concept-Map.md",
                "# OKF Concept Map\n\n## Core Ideas\n\nCore body.\n",
            ),
            document(
                "04-Research/Other/OKF-Concept-Map.md",
                "# Other concept map\n\nOther.\n",
            ),
        ],
    )
    .expect("links convert");

    let converted = converted(&result.documents, "04-Research/Notes/README.md");
    assert_eq!(
        converted.body,
        "See [core ideas](../OKF/OKF-Concept-Map.md#Core%20Ideas).\n"
    );
    assert_eq!(
        converted.metadata.links,
        [TypedLink {
            rel: LinkRelation::References,
            target: "ssa-0/04-Research/OKF/OKF-Concept-Map".into(),
        }]
    );
}

#[test]
fn resolves_a_unique_basename_to_a_relative_markdown_link() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let result = convert_links(
        &profile,
        vec![
            document("01-Soul/Linker.md", "See [[OKF-Concept-Map]].\n"),
            document(
                "04-Research/OKF/OKF-Concept-Map.md",
                "# OKF Concept Map\n\nMap body.\n",
            ),
        ],
    )
    .expect("links convert");

    let converted = converted(&result.documents, "01-Soul/Linker.md");
    assert_eq!(
        converted.body,
        "See [OKF-Concept-Map](../04-Research/OKF/OKF-Concept-Map.md).\n"
    );
}

#[test]
fn records_relative_markdown_links_and_http_links_with_their_default_relations() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let body =
        "See [Map](../OKF/OKF-Concept-Map.md#Summary) and [paper](https://example.com/paper).\n";
    let result = convert_links(
        &profile,
        vec![
            document("04-Research/Notes/README.md", body),
            document(
                "04-Research/OKF/OKF-Concept-Map.md",
                "# OKF Concept Map\n\nMap body.\n",
            ),
        ],
    )
    .expect("links convert");

    let converted = converted(&result.documents, "04-Research/Notes/README.md");
    assert_eq!(converted.body, body);
    assert_eq!(
        converted.metadata.links,
        [
            TypedLink {
                rel: LinkRelation::Cites,
                target: "https://example.com/paper".into(),
            },
            TypedLink {
                rel: LinkRelation::References,
                target: "ssa-0/04-Research/OKF/OKF-Concept-Map".into(),
            },
        ]
    );
}

#[test]
fn preserves_explicit_typed_links_and_deduplicates_all_metadata_links() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let source = document(
        "01-Soul/Linker.md",
        r#"---
links:
  - { rel: supersedes, target: ssa-0/01-Soul/Old }
  - { rel: depends_on, target: ssa-0/03-Protocols/Review }
  - { rel: cites, target: https://example.com/paper }
---
This prose depends on [the paper](https://example.com/paper) and supersedes nothing.
"#,
    );
    let result = convert_links(&profile, vec![source]).expect("links convert");

    let converted = converted(&result.documents, "01-Soul/Linker.md");
    assert_eq!(
        converted.metadata.links,
        [
            TypedLink {
                rel: LinkRelation::Cites,
                target: "https://example.com/paper".into(),
            },
            TypedLink {
                rel: LinkRelation::Supersedes,
                target: "ssa-0/01-Soul/Old".into(),
            },
            TypedLink {
                rel: LinkRelation::DependsOn,
                target: "ssa-0/03-Protocols/Review".into(),
            },
        ]
    );
}

#[test]
fn leaves_broken_and_ambiguous_wikilinks_unchanged_with_stable_warnings() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let body = "Broken [[Missing]] and ambiguous [[README]].\n";
    let result = convert_links(
        &profile,
        vec![
            document("01-Soul/Linker.md", body),
            document("04-Research/A/README.md", "# A\n\nA.\n"),
            document("04-Research/B/README.md", "# B\n\nB.\n"),
        ],
    )
    .expect("unresolved links only warn");

    let converted = converted(&result.documents, "01-Soul/Linker.md");
    assert_eq!(converted.body, body);
    assert!(converted.metadata.links.is_empty());
    assert_eq!(result.warnings.len(), 2);
    assert_eq!(result.warnings[0].code, "links.ambiguous");
    assert_eq!(result.warnings[0].source_path, "01-Soul/Linker.md");
    assert_eq!(result.warnings[0].target, "README");
    assert_eq!(result.warnings[1].code, "links.broken");
    assert_eq!(result.warnings[1].target, "Missing");
}

#[test]
fn rejects_cross_profile_ids_and_vault_uris_from_metadata_or_body() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let cases = [
        r#"---
links: [{ rel: references, target: ssa-1/04-Research/Foreign }]
---
Explicit ID.
"#,
        r#"---
links: [{ rel: references, target: "vault://ssa-1/04-Research/Foreign" }]
---
Explicit URI.
"#,
        "Body [[ssa-1/04-Research/Foreign]].\n",
        "Body [foreign](vault://ssa-1/04-Research/Foreign).\n",
    ];

    for body in cases {
        let error = convert_links(&profile, vec![document("01-Soul/Linker.md", body)])
            .expect_err("cross-profile targets must fail");
        assert_eq!(error.code(), "links.cross_profile", "{body}");
    }
}

#[test]
fn treats_uri_schemes_case_insensitively_for_citations_and_isolation() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let external = convert_links(
        &profile,
        vec![document(
            "01-Soul/Linker.md",
            "Read [paper](HtTpS://example.com/paper).\n",
        )],
    )
    .expect("mixed-case HTTPS remains external");
    assert_eq!(
        external.documents[0].metadata.links,
        [TypedLink {
            rel: LinkRelation::Cites,
            target: "HtTpS://example.com/paper".into(),
        }]
    );

    let error = convert_links(
        &profile,
        vec![document(
            "01-Soul/Linker.md",
            "Foreign [note](VAULT://ssa-1/04-Research/Foreign).\n",
        )],
    )
    .expect_err("mixed-case vault URI must not bypass profile isolation");
    assert_eq!(error.code(), "links.cross_profile");
}

#[test]
fn parses_balanced_markdown_destinations_and_optional_titles() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let body =
        "Read [paper](https://example.com/a_(b)) and [source](https://example.com \"title\").\n";
    let result =
        convert_links(&profile, vec![document("01-Soul/Linker.md", body)]).expect("links convert");

    let converted = converted(&result.documents, "01-Soul/Linker.md");
    assert_eq!(converted.body, body);
    assert_eq!(
        converted.metadata.links,
        [
            TypedLink {
                rel: LinkRelation::Cites,
                target: "https://example.com".into(),
            },
            TypedLink {
                rel: LinkRelation::Cites,
                target: "https://example.com/a_(b)".into(),
            },
        ]
    );
    assert!(result.warnings.is_empty());
}

#[test]
fn resolves_encoded_paths_and_percent_encodes_generated_destinations() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let body = "Wiki [[../OKF/source-pages/Paper (100%)#Core Ideas|paper]] and [encoded](../OKF/source-pages/Paper%20%28100%25%29.md#Core%20Ideas).\n";
    let result = convert_links(
        &profile,
        vec![
            document("04-Research/Notes/README.md", body),
            document(
                "04-Research/OKF/source-pages/Paper (100%).md",
                "# Paper\n\nSource body.\n",
            ),
        ],
    )
    .expect("encoded and raw paths resolve consistently");

    let converted = converted(&result.documents, "04-Research/Notes/README.md");
    assert_eq!(
        converted.body,
        "Wiki [paper](../OKF/source-pages/Paper%20%28100%25%29.md#Core%20Ideas) and [encoded](../OKF/source-pages/Paper%20%28100%25%29.md#Core%20Ideas).\n"
    );
    assert_eq!(
        converted.metadata.links,
        [TypedLink {
            rel: LinkRelation::References,
            target: "ssa-0/04-Research/OKF/source-pages/Paper%20%28100%25%29".into(),
        }]
    );
    assert!(result.warnings.is_empty());
}

#[test]
fn ignores_code_and_backslash_escaped_link_syntax() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let body = r#"Literal examples follow.

`[[Missing]] [foreign](VAULT://ssa-1/Foreign)`

```markdown
[[Missing]]
[foreign](VAULT://ssa-1/Foreign)
```

\[[Missing]] and \[foreign](VAULT://ssa-1/Foreign).
"#;
    let result = convert_links(&profile, vec![document("01-Soul/Linker.md", body)])
        .expect("literal examples are not links");

    let converted = converted(&result.documents, "01-Soul/Linker.md");
    assert_eq!(converted.body, body);
    assert!(converted.metadata.links.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]
fn multiline_inline_code_respects_matching_backtick_run_lengths() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let body = r#"Literal multiline example follows.

``literal starts
` shorter run does not close [[Missing]] [foreign](VAULT://ssa-1/Foreign)
literal ends``
"#;
    let result = convert_links(&profile, vec![document("01-Soul/Linker.md", body)])
        .expect("multiline code span is not interpreted as links");

    let converted = converted(&result.documents, "01-Soul/Linker.md");
    assert_eq!(converted.body, body);
    assert!(converted.metadata.links.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]
fn parses_angle_destinations_before_parentheses_and_title_quotes() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let body = "Read [paper](<https://example.com/a(b>) and [quoted](<https://example.com/a\"title\"(b>).\n";
    let result = convert_links(&profile, vec![document("01-Soul/Linker.md", body)])
        .expect("angle destinations convert");

    let converted = converted(&result.documents, "01-Soul/Linker.md");
    assert_eq!(converted.body, body);
    assert_eq!(
        converted.metadata.links,
        [
            TypedLink {
                rel: LinkRelation::Cites,
                target: "https://example.com/a\"title\"(b".into(),
            },
            TypedLink {
                rel: LinkRelation::Cites,
                target: "https://example.com/a(b".into(),
            },
        ]
    );
    assert!(result.warnings.is_empty());
}

#[test]
fn compares_decoded_vault_namespaces_for_same_and_foreign_profiles() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let same_profile = convert_links(
        &profile,
        vec![
            document(
                "01-Soul/Linker.md",
                "See [target](vault://ssa%2D0/01-Soul/Target).\n",
            ),
            document("01-Soul/Target.md", "# Target\n\nTarget body.\n"),
        ],
    )
    .expect("encoded same-profile namespace is accepted");
    let converted = converted(&same_profile.documents, "01-Soul/Linker.md");
    assert_eq!(
        converted.metadata.links,
        [TypedLink {
            rel: LinkRelation::References,
            target: "ssa-0/01-Soul/Target".into(),
        }]
    );
    assert!(same_profile.warnings.is_empty());

    let error = convert_links(
        &profile,
        vec![document(
            "01-Soul/Linker.md",
            "See [foreign](vault://ssa%2D1/01-Soul/Target).\n",
        )],
    )
    .expect_err("encoded foreign namespace must fail");
    assert_eq!(error.code(), "links.cross_profile");
}

#[test]
fn angle_destination_extraction_uses_the_escape_aware_closing_delimiter() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let body = "Read [paper](<https://example.com/a\\>b>).\n";
    let result = convert_links(&profile, vec![document("01-Soul/Linker.md", body)])
        .expect("escaped angle content is not a closing delimiter");

    let converted = converted(&result.documents, "01-Soul/Linker.md");
    assert_eq!(converted.body, body);
    assert_eq!(
        converted.metadata.links,
        [TypedLink {
            rel: LinkRelation::Cites,
            target: "https://example.com/a\\>b".into(),
        }]
    );
    assert!(result.warnings.is_empty());
}

#[test]
fn decodes_only_the_vault_namespace_and_preserves_the_canonical_encoded_path() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let same_profile = convert_links(
        &profile,
        vec![
            document(
                "01-Soul/Linker.md",
                "See [Audy](vault://ssa%2D0/01-Soul/Audy%20Himura).\n",
            ),
            document(
                "01-Soul/Audy Himura.md",
                "# Audy Himura\n\nIdentity body.\n",
            ),
        ],
    )
    .expect("encoded same-profile namespace resolves without decoding the path");
    let converted = converted(&same_profile.documents, "01-Soul/Linker.md");
    assert_eq!(
        converted.metadata.links,
        [TypedLink {
            rel: LinkRelation::References,
            target: "ssa-0/01-Soul/Audy%20Himura".into(),
        }]
    );
    assert!(same_profile.warnings.is_empty());

    let error = convert_links(
        &profile,
        vec![document(
            "01-Soul/Linker.md",
            "See [foreign](vault://ssa%2D1/01-Soul/Audy%20Himura).\n",
        )],
    )
    .expect_err("encoded foreign vault namespace must fail");
    assert_eq!(error.code(), "links.cross_profile");
}

#[test]
fn fenced_blocks_reset_unmatched_inline_code_spans() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let body = r#"Prelude with unmatched ` delimiter.
```markdown
fenced literal
```
Read [paper](https://example.com/paper) before another ` delimiter.
"#;
    let result = convert_links(&profile, vec![document("01-Soul/Linker.md", body)])
        .expect("fence boundary resets unmatched inline code");

    let converted = converted(&result.documents, "01-Soul/Linker.md");
    assert_eq!(converted.body, body);
    assert_eq!(
        converted.metadata.links,
        [TypedLink {
            rel: LinkRelation::Cites,
            target: "https://example.com/paper".into(),
        }]
    );
    assert!(result.warnings.is_empty());
}

#[test]
fn encoded_bare_profile_ids_are_checked_before_resolution() {
    let profile = load_profile(&profile_path()).expect("profile loads");
    let same_profile = convert_links(
        &profile,
        vec![
            document(
                "01-Soul/Linker.md",
                "See [target](ssa%2D0/01-Soul/Target).\n",
            ),
            document("01-Soul/Target.md", "# Target\n\nTarget body.\n"),
        ],
    )
    .expect("encoded same-profile bare ID resolves");
    let converted = converted(&same_profile.documents, "01-Soul/Linker.md");
    assert_eq!(
        converted.metadata.links,
        [TypedLink {
            rel: LinkRelation::References,
            target: "ssa-0/01-Soul/Target".into(),
        }]
    );
    assert!(same_profile.warnings.is_empty());

    let error = convert_links(
        &profile,
        vec![
            document(
                "01-Soul/Linker.md",
                "See [foreign](ssa%2D1/01-Soul/Target).\n",
            ),
            document("01-Soul/Target.md", "# Target\n\nTarget body.\n"),
        ],
    )
    .expect_err("encoded foreign bare ID must fail before basename resolution");
    assert_eq!(error.code(), "links.cross_profile");
}
