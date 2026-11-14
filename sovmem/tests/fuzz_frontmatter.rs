use sovmem::profile::SourceDocument;

#[test]
fn fuzz_malformed_frontmatter_does_not_panic() {
    // Cases that can be expressed as static byte slices
    let cases: &[&[u8]] = &[
        b"",
        b"---\n---\nbody",
        b"---\nmalformed: [\n---\n",
        b"---\nkey: value\nno-end",
        b"\0\0\0",
        // YAML anchor reference that might confuse the parser
        b"---\ntitle: &anchor reuse\n---\n*anchor\n",
        // Null byte in body only (not in frontmatter, valid field value)
        b"---\nfield: ok\n---\nbody with \0 null byte\n",
    ];

    for (i, bytes) in cases.iter().enumerate() {
        let doc = SourceDocument {
            source_path: format!("test/fuzz-{i}.md").to_owned(),
            bytes: bytes.to_vec(),
            source_hash: "a".repeat(64),
            created: "2026-08-17".to_owned(),
            updated: "2026-08-17".to_owned(),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _parsed = sovmem::profile::parse_source_document(doc);
        }));
        assert!(result.is_ok(), "fuzz case {i} panicked");
    }

    // Large body (10 KiB of 'A' chars) with valid frontmatter
    {
        let mut bytes = b"---\ntitle: large\n---\n".to_vec();
        bytes.extend(std::iter::repeat(b'A').take(10_000));
        let doc = SourceDocument {
            source_path: "test/fuzz-large-body.md".to_owned(),
            bytes,
            source_hash: "a".repeat(64),
            created: "2026-08-17".to_owned(),
            updated: "2026-08-17".to_owned(),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _parsed = sovmem::profile::parse_source_document(doc);
        }));
        assert!(result.is_ok(), "fuzz case large-body panicked");
    }

    // Excessively long YAML value
    {
        let mut bytes = b"---\nkey: ".to_vec();
        bytes.extend(std::iter::repeat(b'x').take(4096));
        bytes.push(b'\n');
        bytes.extend(b"---\nbody");
        let doc = SourceDocument {
            source_path: "test/fuzz-long-value.md".to_owned(),
            bytes,
            source_hash: "a".repeat(64),
            created: "2026-08-17".to_owned(),
            updated: "2026-08-17".to_owned(),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _parsed = sovmem::profile::parse_source_document(doc);
        }));
        assert!(result.is_ok(), "fuzz case long-value panicked");
    }
}
