//! S0.1 contract tests for Markdown and plain-text import.
//!
//! Every test lives in `md_text_contract` so that `cargo test -p plumb-import md_text` selects
//! them. Golden IDs and hashes were calculated independently (Python hashlib and literal
//! canonical JSON), not by the importer under test.

mod md_text_contract {
    use std::collections::BTreeSet;

    use plumb_artifacts::ArtifactKind;
    use plumb_core::{to_canonical_json, Hash, HashKind, Id, Timestamp};
    use plumb_import::*;
    use plumb_psg::{EvidenceLocator, Graph, Node, NodePayload};
    use serde_json::{json, Value};

    const T1: &str = "2026-10-01T08:00:00.000000000Z";
    const T2: &str = "2031-01-01T00:00:00.000000000Z";

    const GOLDEN_DOC: &str = "# Leave\n\n- Employees request leave.\n\n| Type | Days |\n|---|---:|\n| Annual | 26 |\n\nManagers approve\nrequests.\n";
    const GOLDEN_RAW_HASH: &str =
        "sha256:f1aa61f3ed78fe89321ab1fffbef83fa0b2743c90a204a905316ef7279ef9f12";
    const GOLDEN_SOURCE_ID: &str = "src:f1aa61f3ed78fe89";
    const GOLDEN_EXTRACTED_JSON: &str = r##"{"text":"# Leave\n\n- Employees request leave.\n\n| Type | Days |\n|---|---:|\n| Annual | 26 |\n\nManagers approve\nrequests.\n","version":1}"##;
    const GOLDEN_EXTRACTED_HASH: &str =
        "sha256:6e35dc882f7f279a202d9698e5416284f18aa92cac7e42102d129397767e5c79";
    const GOLDEN_LINKS_JSON: &str = r#"{"extracted_ref":"sha256:6e35dc882f7f279a202d9698e5416284f18aa92cac7e42102d129397767e5c79","original_ref":"sha256:f1aa61f3ed78fe89321ab1fffbef83fa0b2743c90a204a905316ef7279ef9f12"}"#;
    /// (kind, text, start, end, TextRange JSON, fragment ID, content hash)
    const GOLDEN_FRAGMENTS: [(&str, &str, u64, u64, &str, &str, &str); 4] = [
        (
            "heading",
            "# Leave",
            0,
            7,
            r#"{"data":{"end":7,"start":0},"kind":"TextRange"}"#,
            "evd:d6465eee2141d67e",
            "sha256:a9224bb9e5666fceaa3262a5cc9de71341d82ecbe435bb1590b4b76e03608539",
        ),
        (
            "list_item",
            "- Employees request leave.",
            9,
            35,
            r#"{"data":{"end":35,"start":9},"kind":"TextRange"}"#,
            "evd:69449f7c6768f1d0",
            "sha256:ab64c2b5fb0c3f14564fc74e68d57b31124cd5ab337fe35b232a344100e3247c",
        ),
        (
            "table_row",
            "| Annual | 26 |",
            64,
            79,
            r#"{"data":{"end":79,"start":64},"kind":"TextRange"}"#,
            "evd:7987f66793e6203e",
            "sha256:1e19cb965209bd111810e100d02e11e01f7c74987bebdf096cf1630c17bcc7f2",
        ),
        (
            "paragraph",
            "Managers approve\nrequests.",
            81,
            107,
            r#"{"data":{"end":107,"start":81},"kind":"TextRange"}"#,
            "evd:e4058e40b41a2213",
            "sha256:7988d8a2628480e10397e55ac10634de385b17ae08f5b06f524dbb175161c9e3",
        ),
    ];

    const HR_REQUIREMENTS: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.md");

    // ------------------------------------------------------------------ helpers

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn audit_at(at: &str) -> ImportAudit {
        ImportAudit {
            created_by: id("actor:importer"),
            created_at: at.parse::<Timestamp>().unwrap(),
        }
    }

    fn audit() -> ImportAudit {
        audit_at(T1)
    }

    fn md(text: &str) -> ImportedSource {
        import_markdown("requirements.md", text.as_bytes(), &audit()).unwrap()
    }

    fn txt(text: &str) -> ImportedSource {
        import_plain_text("notes.txt", text.as_bytes(), &audit()).unwrap()
    }

    fn metadata(node: &Node) -> FragmentMetadata {
        serde_json::from_value(node.extensions[&FRAGMENT_EXTENSION.parse().unwrap()].clone())
            .unwrap()
    }

    fn links(source: &Node) -> SourceArtifactLinks {
        serde_json::from_value(
            source.extensions[&SOURCE_ARTIFACTS_EXTENSION.parse().unwrap()].clone(),
        )
        .unwrap()
    }

    fn extracted(imported: &ImportedSource) -> ExtractedTextArtifact {
        serde_json::from_slice(&imported.extracted_artifact.bytes).unwrap()
    }

    fn range(node: &Node) -> (u64, u64) {
        match &node.payload {
            NodePayload::EvidenceFragment(f) => match f.locator {
                EvidenceLocator::TextRange { start, end } => (start, end),
                ref other => panic!("not a TextRange: {other:?}"),
            },
            other => panic!("not a fragment: {other:?}"),
        }
    }

    fn fragment_text(node: &Node) -> &str {
        match &node.payload {
            NodePayload::EvidenceFragment(f) => f.extracted_text.as_deref().unwrap(),
            other => panic!("not a fragment: {other:?}"),
        }
    }

    /// (kind, exact text) of every fragment, in order.
    fn kinds_and_texts(imported: &ImportedSource) -> Vec<(FragmentKind, String)> {
        imported
            .fragments
            .iter()
            .map(|f| (metadata(f).kind, fragment_text(f).to_owned()))
            .collect()
    }

    /// Every contract invariant that holds for any import.
    fn check_invariants(imported: &ImportedSource) {
        let text = extracted(imported).text;
        let source_links = links(&imported.source);
        let mut previous_end = 0;
        for (index, node) in imported.fragments.iter().enumerate() {
            let (start, end) = range(node);
            assert!(start < end && end as usize <= text.len());
            assert_eq!(
                &text.as_bytes()[start as usize..end as usize],
                fragment_text(node).as_bytes()
            );
            if index > 0 {
                assert!(start >= previous_end, "overlap at {start}");
            }
            previous_end = end;
            let NodePayload::EvidenceFragment(fragment) = &node.payload else {
                unreachable!()
            };
            assert_eq!(fragment.source_ref, imported.source.id);
            assert_eq!(fragment.content_hash.kind(), HashKind::Generic);
            assert_eq!(
                fragment.content_hash,
                Hash::content_sha256(fragment_text(node).as_bytes())
            );
            assert_eq!(fragment.speaker, None);
            assert_eq!(fragment.source_timestamp, None);
            assert_eq!(
                node.id,
                plumb_psg::evidence_fragment_id(&fragment.source_ref, &fragment.locator).unwrap()
            );
            let meta = metadata(node);
            assert_eq!(meta.extracted_ref, source_links.extracted_ref);
            assert_eq!(
                meta.header_cells.is_some(),
                meta.kind == FragmentKind::TableRow
            );
            assert_eq!(node.extensions.len(), 1);
            assert_eq!(node.revision, 1);
            assert!(node.evidence.is_empty() && node.derivations.is_empty());
            assert!(node.standards.is_empty() && node.tags.is_empty());
        }
        Graph::new(
            id("project:leave-management"),
            id("profile:plumb-software-2026.1"),
            std::iter::once(imported.source.clone())
                .chain(imported.fragments.iter().cloned())
                .collect(),
            vec![],
        )
        .unwrap_or_else(|v| panic!("{v:#?}"));
    }

    // ------------------------------------------------------------------ vocabularies (§§65-66)

    #[test]
    fn source_kind_has_exactly_three_wire_values() {
        assert_eq!(
            SourceKind::ALL.map(SourceKind::as_str),
            ["markdown", "plain_text", "docx"]
        );
        assert_eq!(
            SourceKind::ALL.map(SourceKind::media_type),
            [
                "text/markdown",
                "text/plain",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            ]
        );
        for kind in SourceKind::ALL {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
            assert_eq!(
                serde_json::from_value::<SourceKind>(json!(kind.as_str())).unwrap(),
                kind
            );
        }
        for bad in [
            "Markdown",
            "MARKDOWN",
            "plaintext",
            "plain-text",
            "text",
            "md",
            "DOCX",
            "",
        ] {
            assert!(bad.parse::<SourceKind>().is_err(), "{bad}");
            assert!(
                serde_json::from_value::<SourceKind>(json!(bad)).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn fragment_kind_has_exactly_four_wire_values() {
        assert_eq!(
            FragmentKind::ALL.map(FragmentKind::as_str),
            ["heading", "list_item", "table_row", "paragraph"]
        );
        for kind in FragmentKind::ALL {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
            assert_eq!(
                serde_json::from_value::<FragmentKind>(json!(kind.as_str())).unwrap(),
                kind
            );
        }
        for bad in [
            "Heading",
            "list-item",
            "sentence",
            "non_requirement",
            "code",
            "blockquote",
            "ordered_list_item",
            "unordered_list_item",
            "",
        ] {
            assert!(bad.parse::<FragmentKind>().is_err(), "{bad}");
        }
    }

    #[test]
    fn fragment_kind_is_defined_once_in_the_workspace() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut definitions = Vec::new();
        let crates = std::fs::read_dir(root.join("crates")).unwrap();
        for krate in crates {
            let src = krate.unwrap().path().join("src");
            let mut stack = vec![src];
            while let Some(dir) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if path.extension().is_some_and(|e| e == "rs") {
                        let text = std::fs::read_to_string(&path).unwrap();
                        if text.contains("FragmentKind, 4 {") || text.contains("enum FragmentKind")
                        {
                            definitions.push(path);
                        }
                    }
                }
            }
        }
        assert_eq!(definitions.len(), 1, "{definitions:?}");
    }

    // ------------------------------------------------------------------ goldens (§§51-53)

    #[test]
    fn golden_ids_and_hashes_match_independent_values() {
        let imported = md(GOLDEN_DOC);
        let NodePayload::SourceArtifact(source) = &imported.source.payload else {
            panic!()
        };
        assert_eq!(source.content_hash.as_str(), GOLDEN_RAW_HASH);
        assert_eq!(imported.source.id.as_str(), GOLDEN_SOURCE_ID);
        assert_eq!(
            String::from_utf8(imported.extracted_artifact.bytes.clone()).unwrap(),
            GOLDEN_EXTRACTED_JSON
        );
        assert_eq!(
            imported.extracted_artifact.hash.as_str(),
            GOLDEN_EXTRACTED_HASH
        );
        assert_eq!(
            serde_json::to_string(
                &imported.source.extensions[&SOURCE_ARTIFACTS_EXTENSION.parse().unwrap()]
            )
            .unwrap(),
            GOLDEN_LINKS_JSON
        );
        assert_eq!(imported.fragments.len(), GOLDEN_FRAGMENTS.len());
        for (node, (kind, text, start, end, locator_json, fragment_id, content_hash)) in
            imported.fragments.iter().zip(GOLDEN_FRAGMENTS)
        {
            assert_eq!(metadata(node).kind.as_str(), kind);
            assert_eq!(fragment_text(node), text);
            assert_eq!(range(node), (start, end));
            let NodePayload::EvidenceFragment(fragment) = &node.payload else {
                panic!()
            };
            assert_eq!(
                String::from_utf8(to_canonical_json(&fragment.locator).unwrap()).unwrap(),
                locator_json
            );
            assert_eq!(node.id.as_str(), fragment_id);
            assert_eq!(fragment.content_hash.as_str(), content_hash);
        }
        check_invariants(&imported);
    }

    #[test]
    fn fragment_metadata_values_are_exact() {
        let imported = md(GOLDEN_DOC);
        let key = FRAGMENT_EXTENSION.parse().unwrap();
        let values: Vec<&Value> = imported
            .fragments
            .iter()
            .map(|f| &f.extensions[&key])
            .collect();
        assert_eq!(
            *values[0],
            json!({"kind": "heading", "extracted_ref": GOLDEN_EXTRACTED_HASH})
        );
        assert_eq!(
            *values[1],
            json!({"kind": "list_item", "extracted_ref": GOLDEN_EXTRACTED_HASH})
        );
        assert_eq!(
            *values[2],
            json!({"kind": "table_row", "extracted_ref": GOLDEN_EXTRACTED_HASH,
                   "header_cells": ["Type", "Days"]})
        );
        assert_eq!(
            *values[3],
            json!({"kind": "paragraph", "extracted_ref": GOLDEN_EXTRACTED_HASH})
        );

        let base = json!({"kind": "heading", "extracted_ref": GOLDEN_EXTRACTED_HASH});
        let rejects = |value: Value| serde_json::from_value::<FragmentMetadata>(value).is_err();
        let with = |field: &str, v: Value| {
            let mut value = base.clone();
            value[field] = v;
            value
        };
        assert!(rejects(with("extra", json!(1))));
        assert!(rejects(with("header_cells", json!(["A"]))));
        assert!(rejects(
            json!({"kind": "table_row", "extracted_ref": GOLDEN_EXTRACTED_HASH})
        ));
        assert!(rejects(with(
            "extracted_ref",
            json!(format!("psg:sha256:{}", "a".repeat(64)))
        )));
        assert!(rejects(with("kind", json!("sentence"))));
        assert!(!rejects(base));
        assert!(serde_json::from_value::<SourceArtifactLinks>(json!({
            "original_ref": GOLDEN_RAW_HASH, "extracted_ref": GOLDEN_EXTRACTED_HASH, "extra": 1
        }))
        .is_err());
        assert!(
            serde_json::from_value::<ExtractedTextArtifact>(json!({"version": 2, "text": ""}))
                .is_err()
        );
        assert!(serde_json::from_value::<ExtractedTextArtifact>(
            json!({"version": 1, "text": "", "x": 1})
        )
        .is_err());
    }

    // ------------------------------------------------------------------ artifacts (§§67-68, 47-49)

    #[test]
    fn artifacts_have_exact_kinds_bytes_and_links() {
        for (imported, raw, media) in [
            (md(GOLDEN_DOC), GOLDEN_DOC, "text/markdown"),
            (txt("A list:\n- one\n"), "A list:\n- one\n", "text/plain"),
        ] {
            let original = &imported.original_artifact;
            assert_eq!(original.kind, ArtifactKind::SourceOriginal);
            assert_eq!(original.media_type, media);
            assert_eq!(original.bytes, raw.as_bytes());
            assert_eq!(original.hash, Hash::content_sha256(raw.as_bytes()));
            let extracted_artifact = &imported.extracted_artifact;
            assert_eq!(extracted_artifact.kind, ArtifactKind::SourceExtracted);
            assert_eq!(extracted_artifact.media_type, "application/json");
            assert_eq!(
                extracted_artifact.hash,
                Hash::content_sha256(&extracted_artifact.bytes)
            );
            assert_eq!(
                extracted_artifact.bytes,
                to_canonical_json(&extracted(&imported)).unwrap()
            );
            assert_eq!(
                extracted(&imported),
                ExtractedTextArtifact {
                    version: 1,
                    text: raw.to_owned()
                }
            );
            assert_ne!(original.hash, extracted_artifact.hash);
            let NodePayload::SourceArtifact(source) = &imported.source.payload else {
                panic!()
            };
            assert_eq!(source.content_hash, original.hash);
            assert_eq!(source.media_type, media);
            let source_links = links(&imported.source);
            assert_eq!(source_links.original_ref, original.hash);
            assert_eq!(source_links.extracted_ref, extracted_artifact.hash);
            assert_eq!(imported.source.extensions.len(), 1);
            assert_eq!(original.created_at, audit().created_at);
            assert_eq!(extracted_artifact.created_at, audit().created_at);
            assert_eq!(imported.source.audit.created_by, id("actor:importer"));
            assert_eq!(imported.source.audit.updated_by, None);
            assert_eq!(imported.source.audit.updated_at, None);
            for field in [
                &source.external_uri,
                &source.external_version,
                &source.producer,
                &source.language,
                &source.classification,
            ] {
                assert_eq!(field, &None);
            }
            assert_eq!(source.created_at_source, None);
            check_invariants(&imported);
        }
        let NodePayload::SourceArtifact(source) = &md(GOLDEN_DOC).source.payload else {
            panic!()
        };
        assert_eq!(
            (source.source_kind.as_str(), source.display_name.as_str()),
            ("markdown", "requirements.md")
        );
        let NodePayload::SourceArtifact(source) = &txt("x").source.payload else {
            panic!()
        };
        assert_eq!(source.source_kind, "plain_text");
    }

    #[test]
    fn line_endings_are_normalized_but_the_raw_hash_uses_original_bytes() {
        let raw = "A\r\nB\rC\nD";
        let imported = txt(raw);
        assert_eq!(extracted(&imported).text, "A\nB\nC\nD");
        assert_eq!(
            imported.original_artifact.hash,
            Hash::content_sha256(raw.as_bytes())
        );
        assert_eq!(
            imported.extracted_artifact.bytes,
            br#"{"text":"A\nB\nC\nD","version":1}"#.to_vec()
        );
        assert_eq!(
            kinds_and_texts(&imported),
            [(FragmentKind::Paragraph, "A\nB\nC\nD".to_owned())]
        );
        // Only line endings change: tabs, BOM and trailing spaces are kept.
        let kept = "\u{feff}Tab\there  \r\n";
        assert_eq!(extracted(&txt(kept)).text, "\u{feff}Tab\there  \n");
        check_invariants(&imported);
    }

    #[test]
    fn reimport_is_deterministic_and_time_only_changes_operational_fields() {
        let a = md(GOLDEN_DOC);
        let b = md(GOLDEN_DOC);
        assert_eq!(a, b);
        assert_eq!(
            serde_json::to_string(&a.source).unwrap(),
            serde_json::to_string(&b.source).unwrap()
        );
        let later =
            import_markdown("requirements.md", GOLDEN_DOC.as_bytes(), &audit_at(T2)).unwrap();
        assert_eq!(later.source.id, a.source.id);
        assert_eq!(later.original_artifact.hash, a.original_artifact.hash);
        assert_eq!(later.extracted_artifact.hash, a.extracted_artifact.hash);
        assert_eq!(later.extracted_artifact.bytes, a.extracted_artifact.bytes);
        let ids = |i: &ImportedSource| i.fragments.iter().map(|f| f.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&later), ids(&a));
        assert_eq!(kinds_and_texts(&later), kinds_and_texts(&a));
        assert_eq!(later.source.payload, a.source.payload);
        assert_ne!(
            later.original_artifact.created_at,
            a.original_artifact.created_at
        );
        assert_ne!(later.source.audit, a.source.audit);
        // The display name is recorded but never part of an ID.
        let renamed = import_markdown("other name.md", GOLDEN_DOC.as_bytes(), &audit()).unwrap();
        assert_eq!(renamed.source.id, a.source.id);
        assert_eq!(ids(&renamed), ids(&a));
    }

    #[test]
    fn display_name_must_be_clean_and_is_not_repaired() {
        for bad in ["", " name.md", "name.md ", "na\tme.md", "a\nb"] {
            assert_eq!(
                import_markdown(bad, b"x", &audit()),
                Err(ImportError::InvalidDisplayName(bad.to_owned()))
            );
            assert!(import_plain_text(bad, b"x", &audit()).is_err());
        }
        let NodePayload::SourceArtifact(source) =
            &import_markdown("HR requirements v2.md", b"x", &audit())
                .unwrap()
                .source
                .payload
        else {
            panic!()
        };
        assert_eq!(source.display_name, "HR requirements v2.md");
    }

    // ------------------------------------------------------------------ encoding (§56)

    #[test]
    fn invalid_utf8_fails_with_e_source_encoding() {
        for (bytes, valid_up_to, error_len) in [
            (&b"ok \xc3\x28 bad"[..], 3, Some(1)),
            (&b"truncated \xe2\x82"[..], 10, None),
        ] {
            for result in [
                import_markdown("a.md", bytes, &audit()),
                import_plain_text("a.txt", bytes, &audit()),
            ] {
                let error = result.unwrap_err();
                assert_eq!(
                    error,
                    ImportError::SourceEncoding {
                        valid_up_to,
                        error_len
                    }
                );
                assert_eq!(error.code(), Some(E_SOURCE_ENCODING));
                assert_eq!(E_SOURCE_ENCODING, "E_SOURCE_ENCODING");
            }
        }
        assert_eq!(ImportError::InvalidDisplayName(String::new()).code(), None);
    }

    // ------------------------------------------------------------------ Markdown classification (§54)

    fn texts(imported: &ImportedSource) -> Vec<(&'static str, String)> {
        kinds_and_texts(imported)
            .into_iter()
            .map(|(k, t)| (k.as_str(), t))
            .collect()
    }

    fn pairs(v: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
        v.iter().map(|(k, t)| (*k, (*t).to_owned())).collect()
    }

    #[test]
    fn markdown_headings() {
        let imported = md("# One\n###### Six  \n####### Seven\n#NoSpace\n");
        assert_eq!(
            texts(&imported),
            pairs(&[
                ("heading", "# One"),
                ("heading", "###### Six  "),
                ("paragraph", "####### Seven\n#NoSpace"),
            ])
        );
        check_invariants(&imported);
    }

    #[test]
    fn markdown_lists() {
        let imported =
            md("- dash\n+ plus\n* star\n1. one\n2) two\n  - nested\n\t3. tabbed\n-nospace\n");
        assert_eq!(
            texts(&imported),
            pairs(&[
                ("list_item", "- dash"),
                ("list_item", "+ plus"),
                ("list_item", "* star"),
                ("list_item", "1. one"),
                ("list_item", "2) two"),
                ("list_item", "  - nested"),
                ("list_item", "\t3. tabbed"),
                ("paragraph", "-nospace"),
            ])
        );
        check_invariants(&imported);
    }

    #[test]
    fn markdown_paragraphs() {
        let imported =
            md("First line\nsecond line\n- item\nafter item\n# Head\ntail\n \t \nlast\n");
        assert_eq!(
            texts(&imported),
            pairs(&[
                ("paragraph", "First line\nsecond line"),
                ("list_item", "- item"),
                ("paragraph", "after item"),
                ("heading", "# Head"),
                ("paragraph", "tail"),
                ("paragraph", "last"),
            ])
        );
        check_invariants(&imported);
        // Unicode whitespace is content, not blank.
        let nbsp = md("a\n\u{a0}\nb\n");
        assert_eq!(texts(&nbsp), pairs(&[("paragraph", "a\n\u{a0}\nb")]));
    }

    #[test]
    fn markdown_crlf_lone_cr_and_non_ascii_offsets() {
        let imported = md("# Zażółć\r\n\r\n- żądanie urlopu\rParagraf ąę\n");
        assert_eq!(
            extracted(&imported).text,
            "# Zażółć\n\n- żądanie urlopu\nParagraf ąę\n"
        );
        assert_eq!(
            texts(&imported),
            pairs(&[
                ("heading", "# Zażółć"),
                ("list_item", "- żądanie urlopu"),
                ("paragraph", "Paragraf ąę"),
            ])
        );
        // Byte offsets, not char indices: "# Zażółć" is 8 chars but 12 bytes.
        assert_eq!(range(&imported.fragments[0]), (0, 12));
        // "- żądanie urlopu" is 16 chars and 18 bytes.
        assert_eq!(range(&imported.fragments[1]), (14, 32));
        assert_eq!("# Zażółć".chars().count(), 8);
        check_invariants(&imported);
    }

    #[test]
    fn markdown_tables() {
        let imported = md("| Type | Days | Note |\n|:---|:---:|---:|\n| Annual | 26 |  |\n| Sick | 0 | paid |\n\nType | Days\n--- | ---\nUnpaid | 10\n");
        let rows: Vec<(String, Option<Vec<String>>)> = imported
            .fragments
            .iter()
            .map(|f| (fragment_text(f).to_owned(), metadata(f).header_cells))
            .collect();
        let with_pipes = Some(vec![
            "Type".to_owned(),
            "Days".to_owned(),
            "Note".to_owned(),
        ]);
        let without_pipes = Some(vec!["Type".to_owned(), "Days".to_owned()]);
        assert_eq!(
            rows,
            [
                ("| Annual | 26 |  |".to_owned(), with_pipes.clone()),
                ("| Sick | 0 | paid |".to_owned(), with_pipes),
                ("Unpaid | 10".to_owned(), without_pipes),
            ]
        );
        assert!(imported
            .fragments
            .iter()
            .all(|f| metadata(f).kind == FragmentKind::TableRow));
        check_invariants(&imported);

        assert_eq!(table_cells("| a | | c |"), ["a", "", "c"]);
        assert_eq!(table_cells("a|b"), ["a", "b"]);
        assert_eq!(table_cells("|\tA  |"), ["A"]);
    }

    #[test]
    fn markdown_table_like_block_with_invalid_delimiter_is_a_paragraph() {
        for doc in [
            "| A | B |\n| -- | -- |\n| 1 | 2 |\n",
            "| A | B |\n| x | y |\n| 1 | 2 |\n",
            "| A | B |\n",
        ] {
            let imported = md(doc);
            assert_eq!(
                texts(&imported),
                pairs(&[("paragraph", doc.trim_end_matches('\n'))]),
                "{doc}"
            );
            check_invariants(&imported);
        }
        // A heading line with a pipe but no delimiter line stays a heading.
        assert_eq!(texts(&md("# A | B\n")), pairs(&[("heading", "# A | B")]));
    }

    #[test]
    fn markdown_special_constructs_are_not_parsed() {
        let imported = md("```\ncode\n```\n> quote\n---\n- [ ] task\n");
        assert_eq!(
            texts(&imported),
            pairs(&[
                ("paragraph", "```\ncode\n```\n> quote\n---"),
                ("list_item", "- [ ] task")
            ])
        );
        check_invariants(&imported);
    }

    // ------------------------------------------------------------------ plain text (§55)

    #[test]
    fn plain_text_has_no_heading_or_table_inference() {
        let imported = txt("# Not a heading\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n- item\n1. first\nplain\nwords\n");
        assert_eq!(
            texts(&imported),
            pairs(&[
                ("paragraph", "# Not a heading"),
                ("paragraph", "| A | B |\n|---|---|\n| 1 | 2 |"),
                ("list_item", "- item"),
                ("list_item", "1. first"),
                ("paragraph", "plain\nwords"),
            ])
        );
        assert!(imported.fragments.iter().all(|f| !matches!(
            metadata(f).kind,
            FragmentKind::Heading | FragmentKind::TableRow
        )));
        check_invariants(&imported);
        let crlf = txt("żółw\r\n- a\r");
        assert_eq!(
            texts(&crlf),
            pairs(&[("paragraph", "żółw"), ("list_item", "- a")])
        );
        assert_eq!(range(&crlf.fragments[1]), (8, 11));
        check_invariants(&crlf);
    }

    #[test]
    fn empty_and_blank_sources_have_no_fragments() {
        for doc in ["", "\n", " \t\n\n"] {
            let imported = md(doc);
            assert!(imported.fragments.is_empty());
            check_invariants(&imported);
            assert!(txt(doc).fragments.is_empty());
        }
    }

    // ------------------------------------------------------------------ HR acceptance (§57)

    #[test]
    fn hr_requirements_markdown_imports_deterministically() {
        let imported = import_markdown("requirements.md", HR_REQUIREMENTS, &audit()).unwrap();
        let NodePayload::SourceArtifact(source) = &imported.source.payload else {
            panic!()
        };
        assert_eq!(source.source_kind, "markdown");
        assert_eq!(imported.original_artifact.bytes, HR_REQUIREMENTS);
        check_invariants(&imported);
        let starts: Vec<u64> = imported.fragments.iter().map(|f| range(f).0).collect();
        assert!(starts.windows(2).all(|w| w[0] < w[1]));
        assert!(!imported.fragments.is_empty());
        let kinds: BTreeSet<FragmentKind> = imported
            .fragments
            .iter()
            .map(|f| metadata(f).kind)
            .collect();
        assert!(kinds.contains(&FragmentKind::Heading));
        let again = import_markdown("requirements.md", HR_REQUIREMENTS, &audit()).unwrap();
        assert_eq!(again, imported);
    }

    // ------------------------------------------------------------------ source guards (§§58, 71)

    #[test]
    fn production_source_has_no_persistence_clock_or_network_capability() {
        for (name, source) in [
            ("lib.rs", include_str!("../src/lib.rs")),
            ("source.rs", include_str!("../src/source.rs")),
            ("md.rs", include_str!("../src/md.rs")),
            ("text.rs", include_str!("../src/text.rs")),
        ] {
            for forbidden in [
                "HashMap",
                "SystemClock",
                "Utc::now",
                "Clock::now",
                "ArtifactStore",
                "Sqlite",
                "rusqlite",
                "std::fs",
                "reqwest",
                "InferenceProvider",
                "LlmProvider",
                "Graph::new",
                "SemanticPatch",
                "Proposal",
                "from_utf8_lossy",
            ] {
                assert!(!source.contains(forbidden), "{name} contains {forbidden}");
            }
        }
    }
}
