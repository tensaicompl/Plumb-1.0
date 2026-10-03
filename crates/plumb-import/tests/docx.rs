//! S0.2 contract tests for the deterministic minimal DOCX importer. Every DOCX package is
//! built in memory with `zip::ZipWriter`, except the read-only HR fixture.
//!
//! Every test lives in `docx_contract` so that `cargo test -p plumb-import docx` selects them.

mod docx_contract {
    use std::io::{Cursor, Write};

    use plumb_artifacts::ArtifactKind;
    use plumb_core::{to_canonical_json, Hash, Id, Timestamp};
    use plumb_import::docx::*;
    use plumb_import::*;
    use plumb_psg::{EvidenceLocator, Graph, Node, NodePayload};
    use serde_json::json;
    use zip::write::SimpleFileOptions;

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const DOCX_MEDIA: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    const HR_DOCX: &[u8] = include_bytes!("../../../fixtures/hr-leave/requirements.docx");

    // ------------------------------------------------------------------ builders

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
        audit_at("2026-10-01T08:00:00.000000000Z")
    }

    /// A package with the given parts.
    fn package(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in parts {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn document(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="{W}"><w:body>{body}<w:sectPr/></w:body></w:document>"#
        )
    }

    fn styles(inner: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:styles xmlns:w="{W}">{inner}</w:styles>"#
        )
    }

    fn docx(body: &str) -> Vec<u8> {
        package(&[("word/document.xml", &document(body))])
    }

    fn docx_with_styles(body: &str, style_defs: &str) -> Vec<u8> {
        package(&[
            ("word/document.xml", &document(body)),
            ("word/styles.xml", &styles(style_defs)),
        ])
    }

    fn p(text: &str) -> String {
        format!(r#"<w:p><w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>"#)
    }

    fn p_props(props: &str, text: &str) -> String {
        format!(
            r#"<w:p><w:pPr>{props}</w:pPr><w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>"#
        )
    }

    fn tc(text: &str, props: &str) -> String {
        format!("<w:tc><w:tcPr>{props}</w:tcPr>{}</w:tc>", p(text))
    }

    fn tr(cells: &[String]) -> String {
        format!("<w:tr>{}</w:tr>", cells.concat())
    }

    fn import(bytes: &[u8]) -> ImportedSource {
        import_docx("requirements.docx", bytes, &audit()).unwrap()
    }

    fn metadata(node: &Node) -> FragmentMetadata {
        serde_json::from_value(node.extensions[&FRAGMENT_EXTENSION.parse().unwrap()].clone())
            .unwrap()
    }

    fn fragment_text(node: &Node) -> &str {
        match &node.payload {
            NodePayload::EvidenceFragment(f) => f.extracted_text.as_deref().unwrap(),
            other => panic!("{other:?}"),
        }
    }

    fn range(node: &Node) -> (u64, u64) {
        match &node.payload {
            NodePayload::EvidenceFragment(f) => match f.locator {
                EvidenceLocator::TextRange { start, end } => (start, end),
                ref other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    fn extracted_text(imported: &ImportedSource) -> String {
        serde_json::from_slice::<ExtractedTextArtifact>(&imported.extracted_artifact.bytes)
            .unwrap()
            .text
    }

    fn kinds(imported: &ImportedSource) -> Vec<(FragmentKind, String)> {
        imported
            .fragments
            .iter()
            .map(|f| (metadata(f).kind, fragment_text(f).to_owned()))
            .collect()
    }

    fn pairs(v: &[(FragmentKind, &str)]) -> Vec<(FragmentKind, String)> {
        v.iter().map(|(k, t)| (*k, (*t).to_owned())).collect()
    }

    use FragmentKind::{Heading, ListItem, Paragraph, TableRow};

    /// Contract invariants of any DOCX import.
    fn check(imported: &ImportedSource) {
        let text = extracted_text(imported);
        for node in &imported.fragments {
            let (start, end) = range(node);
            assert!(start < end && end as usize <= text.len());
            assert_eq!(
                &text.as_bytes()[start as usize..end as usize],
                fragment_text(node).as_bytes()
            );
            let NodePayload::EvidenceFragment(f) = &node.payload else {
                unreachable!()
            };
            assert_eq!(f.source_ref, imported.source.id);
            assert_eq!(
                f.content_hash,
                Hash::content_sha256(fragment_text(node).as_bytes())
            );
            assert_eq!(
                node.id,
                plumb_psg::evidence_fragment_id(&f.source_ref, &f.locator).unwrap()
            );
        }
        assert!(imported
            .fragments
            .windows(2)
            .all(|w| range(&w[0]).1 <= range(&w[1]).0));
        assert_eq!(
            source_parse_metadata(&imported.source).unwrap(),
            imported.parse
        );
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

    // ------------------------------------------------------------------ paragraphs and headings

    #[test]
    fn paragraphs_headings_and_visible_text() {
        let body = [
            p("Plain paragraph"),
            p_props(r#"<w:pStyle w:val="Heading1"/>"#, "Built-in heading"),
            p_props(r#"<w:outlineLvl w:val="5"/>"#, "Outline heading"),
            p_props(r#"<w:outlineLvl w:val="6"/>"#, "Level 7 is body"),
            r#"<w:p><w:r><w:t>Tab</w:t><w:tab/><w:t>and</w:t><w:br/><w:t>break</w:t><w:cr/><w:t>cr</w:t></w:r></w:p>"#.to_owned(),
            r#"<w:p><w:r><w:t xml:space="preserve">  keep  spaces  </w:t></w:r></w:p>"#.to_owned(),
            "<w:p/>".to_owned(),
            r#"<w:p><w:r><w:t>Zażółć gęślą</w:t></w:r></w:p>"#.to_owned(),
            r#"<w:p><w:ins><w:r><w:t>kept </w:t></w:r></w:ins><w:del><w:r><w:delText>gone</w:delText></w:r></w:del><w:moveFrom><w:r><w:t>old</w:t></w:r></w:moveFrom><w:moveTo><w:r><w:t>moved</w:t></w:r></w:moveTo><w:r><w:instrText>PAGE</w:instrText></w:r></w:p>"#.to_owned(),
        ]
        .concat();
        let imported = import(&docx(&body));
        assert_eq!(
            kinds(&imported),
            pairs(&[
                (Paragraph, "Plain paragraph"),
                (Heading, "Built-in heading"),
                (Heading, "Outline heading"),
                (Paragraph, "Level 7 is body"),
                (Paragraph, "Tab\tand\nbreak\ncr"),
                (Paragraph, "  keep  spaces  "),
                (Paragraph, "Zażółć gęślą"),
                (Paragraph, "kept moved"),
            ])
        );
        // Records joined by single LFs, the empty paragraph kept as an empty record, no
        // trailing LF.
        assert_eq!(
            extracted_text(&imported),
            "Plain paragraph\nBuilt-in heading\nOutline heading\nLevel 7 is body\nTab\tand\nbreak\ncr\n  keep  spaces  \n\nZażółć gęślą\nkept moved"
        );
        // Non-ASCII: byte offsets.
        let zazolc = &imported.fragments[6];
        let (start, end) = range(zazolc);
        assert_eq!(end - start, "Zażółć gęślą".len() as u64);
        assert_eq!("Zażółć gęślą".chars().count(), 12);
        assert_eq!(end - start, 19);
        assert_eq!(imported.parse, ParseMetadata::complete());
        check(&imported);
    }

    #[test]
    fn style_inherited_headings_and_lists() {
        let style_defs = r#"
            <w:style w:type="paragraph" w:styleId="Title1"><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style>
            <w:style w:type="paragraph" w:styleId="Chapter"><w:basedOn w:val="Title1"/></w:style>
            <w:style w:type="paragraph" w:styleId="ListBase"><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr></w:style>
            <w:style w:type="paragraph" w:styleId="Requirement"><w:basedOn w:val="ListBase"/></w:style>
            <w:style w:type="paragraph" w:styleId="Heading2"><w:pPr><w:outlineLvl w:val="9"/></w:pPr></w:style>"#;
        let body = [
            p_props(r#"<w:pStyle w:val="Chapter"/>"#, "Inherited heading"),
            p_props(
                r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr>"#,
                "Direct list",
            ),
            p_props(r#"<w:pStyle w:val="Requirement"/>"#, "Inherited list"),
            p_props(
                r#"<w:pStyle w:val="Title1"/><w:numPr><w:numId w:val="2"/></w:numPr>"#,
                "Heading wins",
            ),
            p_props(r#"<w:pStyle w:val="Heading2"/>"#, "Style outline 9 is body"),
            p_props(r#"<w:pStyle w:val="Unknown"/>"#, "Unknown style"),
        ]
        .concat();
        let imported = import(&docx_with_styles(&body, style_defs));
        assert_eq!(
            kinds(&imported),
            pairs(&[
                (Heading, "Inherited heading"),
                (ListItem, "Direct list"),
                (ListItem, "Inherited list"),
                (Heading, "Heading wins"),
                (Paragraph, "Style outline 9 is body"),
                (Paragraph, "Unknown style"),
            ])
        );
        // No list marker is fabricated.
        assert!(!extracted_text(&imported).contains('•'));
        check(&imported);
    }

    #[test]
    fn literal_numbering_text_is_not_a_docx_list() {
        let style_defs = r#"<w:style w:type="paragraph" w:styleId="Body"><w:pPr><w:spacing w:after="0"/></w:pPr></w:style>"#;
        let body = [
            p("1. HR-001 [functional] The system shall allow an employee to create a draft leave request."),
            p("- dash text"),
            p("• bullet text"),
            p_props(r#"<w:pStyle w:val="Body"/>"#, "2) styled but unnumbered"),
            p_props(r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>"#, "Structural list"),
        ]
        .concat();
        let imported = import(&docx_with_styles(&body, style_defs));
        assert_eq!(
            kinds(&imported),
            pairs(&[
                (Paragraph, "1. HR-001 [functional] The system shall allow an employee to create a draft leave request."),
                (Paragraph, "- dash text"),
                (Paragraph, "• bullet text"),
                (Paragraph, "2) styled but unnumbered"),
                (ListItem, "Structural list"),
            ])
        );
        check(&imported);
    }

    #[test]
    fn namespace_prefix_does_not_matter() {
        let standard = docx(
            &[
                p_props(r#"<w:pStyle w:val="Heading1"/>"#, "Title"),
                p("Body"),
            ]
            .concat(),
        );
        let custom = package(&[(
            "word/document.xml",
            &format!(
                r#"<?xml version="1.0"?><x:document xmlns:x="{W}"><x:body><x:p><x:pPr><x:pStyle x:val="Heading1"/></x:pPr><x:r><x:t>Title</x:t></x:r></x:p><x:p><x:r><x:t>Body</x:t></x:r></x:p></x:body></x:document>"#
            ),
        )]);
        let strict = package(&[(
            "word/document.xml",
            r#"<?xml version="1.0"?><w:document xmlns:w="http://purl.oclc.org/ooxml/wordprocessingml/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p><w:p><w:r><w:t>Body</w:t></w:r></w:p></w:body></w:document>"#,
        )]);
        let expected = pairs(&[(Heading, "Title"), (Paragraph, "Body")]);
        for bytes in [&standard, &custom, &strict] {
            let imported = import(bytes);
            assert_eq!(kinds(&imported), expected);
            assert_eq!(extracted_text(&imported), "Title\nBody");
            check(&imported);
        }
        // Same content, different bytes: same fragment texts, different source IDs.
        assert_ne!(import(&standard).source.id, import(&custom).source.id);
    }

    // ------------------------------------------------------------------ tables

    #[test]
    fn tables_rows_headers_and_merges() {
        let table = format!(
            "<w:tbl><w:tblPr/><w:tblGrid/>{}{}{}{}</w:tbl>",
            tr(&[
                tc("Type", ""),
                tc("Span", r#"<w:gridSpan w:val="2"/>"#),
                tc("Owner", "")
            ]),
            tr(&[
                tc("Annual", r#"<w:vMerge w:val="restart"/>"#),
                tc("a", ""),
                tc("b", ""),
                tc("HR", "")
            ]),
            tr(&[
                tc("", "<w:vMerge/>"),
                tc("c", ""),
                tc("d", ""),
                tc("", r#"<w:vMerge w:val="continue"/>"#)
            ]),
            tr(&[tc("", ""), tc("", ""), tc("", ""), tc("", "")]),
        );
        let cell_with_two_paragraphs = format!(
            "<w:tbl>{}{}</w:tbl>",
            tr(&[tc("H", "")]),
            tr(&[format!("<w:tc>{}{}</w:tc>", p("line one"), p("line two"))])
        );
        let imported = import(&docx(
            &[p("Before"), table, cell_with_two_paragraphs].concat(),
        ));
        assert_eq!(
            extracted_text(&imported),
            "Before\nType\tSpan\t\tOwner\nAnnual\ta\tb\tHR\n\tc\td\t\n\t\t\t\nH\nline one\nline two"
        );
        let header = Some(vec![
            "Type".to_owned(),
            "Span".to_owned(),
            String::new(),
            "Owner".to_owned(),
        ]);
        let rows: Vec<(FragmentKind, String, Option<Vec<String>>)> = imported
            .fragments
            .iter()
            .map(|f| {
                (
                    metadata(f).kind,
                    fragment_text(f).to_owned(),
                    metadata(f).header_cells,
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                (Paragraph, "Before".to_owned(), None),
                (TableRow, "Annual\ta\tb\tHR".to_owned(), header.clone()),
                (TableRow, "\tc\td\t".to_owned(), header),
                (
                    TableRow,
                    "line one\nline two".to_owned(),
                    Some(vec!["H".to_owned()])
                ),
            ]
        );
        assert_eq!(imported.parse, ParseMetadata::complete());
        check(&imported);
    }

    #[test]
    fn continuation_cell_with_text_is_kept_and_warned() {
        let table = format!(
            "<w:tbl>{}{}{}</w:tbl>",
            tr(&[tc("A", ""), tc("B", "")]),
            tr(&[tc("x", r#"<w:vMerge w:val="restart"/>"#), tc("y", "")]),
            tr(&[tc("stray", "<w:vMerge/>"), tc("z", "")]),
        );
        let imported = import(&docx(&table));
        assert!(extracted_text(&imported).contains("stray\tz"));
        assert_eq!(
            imported.parse.status,
            ParseStatus::PartialWithExplicitUnparsedRegions
        );
        assert_eq!(
            serde_json::to_value(&imported.parse.warnings).unwrap(),
            json!([{"code": "W_DOCX_UNSUPPORTED_VISIBLE",
                    "locator": {"kind": "XmlPath", "data": {"xpath": "/w:document/w:body/*[1]/w:tr[3]/w:tc[1]"}},
                    "message": DOCX_UNSUPPORTED_VISIBLE_MESSAGE}])
        );
        check(&imported);
    }

    // ------------------------------------------------------------------ unsupported visible content

    #[test]
    fn unsupported_top_level_body_element_is_flattened_and_warned() {
        let body = [
            p("First"),
            r#"<w:sdt><w:sdtPr/><w:sdtContent><w:p><w:r><w:t>Inside a content control</w:t></w:r></w:p></w:sdtContent></w:sdt>"#.to_owned(),
            r#"<w:bookmarkStart w:id="0" w:name="x"/>"#.to_owned(),
            p("Last"),
        ]
        .concat();
        let imported = import(&docx(&body));
        assert_eq!(
            extracted_text(&imported),
            "First\nInside a content control\nLast"
        );
        assert_eq!(
            kinds(&imported),
            pairs(&[
                (Paragraph, "First"),
                (Paragraph, "Inside a content control"),
                (Paragraph, "Last")
            ])
        );
        let parse = &imported.parse;
        assert_eq!(
            parse.status,
            ParseStatus::PartialWithExplicitUnparsedRegions
        );
        assert_eq!(parse.warnings.len(), 1);
        assert_eq!(
            parse.warnings[0].code,
            ImportWarningCode::DocxUnsupportedVisible
        );
        assert_eq!(
            parse.warnings[0].locator,
            EvidenceLocator::XmlPath {
                xpath: "/w:document/w:body/*[2]".to_owned()
            }
        );
        assert_eq!(
            parse.warnings[0].message,
            "Visible DOCX content was flattened because its structure is outside the S0.2 import profile."
        );
        let key = PARSE_EXTENSION.parse().unwrap();
        assert_eq!(
            imported.source.extensions[&key],
            serde_json::to_value(parse).unwrap()
        );
        check(&imported);
    }

    #[test]
    fn unsupported_inline_structure_is_flattened_into_its_paragraph() {
        let body = r#"<w:p><w:r><w:t>See </w:t></w:r><w:hyperlink><w:r><w:t>the policy</w:t></w:r></w:hyperlink></w:p>"#;
        let imported = import(&docx(body));
        assert_eq!(kinds(&imported), pairs(&[(Paragraph, "See the policy")]));
        assert_eq!(
            imported.parse.warnings[0].locator,
            EvidenceLocator::XmlPath {
                xpath: "/w:document/w:body/*[1]".to_owned()
            }
        );
        check(&imported);
    }

    // ------------------------------------------------------------------ fatal errors and limits

    #[test]
    fn fatal_errors_are_typed_with_stable_codes() {
        let cases: Vec<(Vec<u8>, &str)> = vec![
            (b"not a zip".to_vec(), E_DOCX_ARCHIVE),
            (package(&[("word/other.xml", "<x/>")]), E_DOCX_MISSING_PART),
            (package(&[("word/document.xml", "<w:document")]), E_DOCX_XML),
            (docx_with_styles(&p("x"), "<w:style"), E_DOCX_XML),
            (
                docx_with_styles(
                    &p_props(r#"<w:pStyle w:val="A"/>"#, "x"),
                    r#"<w:style w:type="paragraph" w:styleId="A"><w:basedOn w:val="B"/></w:style><w:style w:type="paragraph" w:styleId="B"><w:basedOn w:val="A"/></w:style>"#,
                ),
                E_DOCX_XML,
            ),
            (
                docx(&format!(
                    "<w:tbl>{}</w:tbl>",
                    tr(&[tc("x", r#"<w:gridSpan w:val="0"/>"#)])
                )),
                E_DOCX_XML,
            ),
            (
                docx(&format!(
                    "<w:tbl>{}</w:tbl>",
                    tr(&[tc("x", r#"<w:gridSpan w:val="two"/>"#)])
                )),
                E_DOCX_XML,
            ),
            (
                docx(&format!(
                    "<w:tbl>{}</w:tbl>",
                    tr(&[tc("x", r#"<w:vMerge w:val="middle"/>"#)])
                )),
                E_DOCX_XML,
            ),
        ];
        for (bytes, code) in cases {
            let error = import_docx("x.docx", &bytes, &audit()).unwrap_err();
            assert_eq!(error.code(), Some(code), "{error:?}");
            match code {
                E_DOCX_ARCHIVE => assert!(matches!(error, ImportError::DocxArchive { .. })),
                E_DOCX_MISSING_PART => assert_eq!(
                    error,
                    ImportError::DocxMissingPart {
                        part: "word/document.xml".to_owned()
                    }
                ),
                _ => assert!(matches!(error, ImportError::DocxXml { .. })),
            }
        }
        // A style chain deeper than 32 is malformed too.
        let chain: String = (0..40)
            .map(|i| format!(r#"<w:style w:type="paragraph" w:styleId="S{i}"><w:basedOn w:val="S{}"/></w:style>"#, i + 1))
            .collect();
        let error =
            import_docx("x.docx", &docx_with_styles(&p("x"), &chain), &audit()).unwrap_err();
        assert!(matches!(error, ImportError::DocxXml { .. }));
    }

    #[test]
    fn safety_limits_produce_e_docx_limit() {
        assert_eq!(DOCX_MAX_INPUT_BYTES, 128 * 1024 * 1024);
        assert_eq!(DOCX_MAX_ENTRIES, 4096);
        assert_eq!(DOCX_MAX_TOTAL_UNCOMPRESSED_BYTES, 512 * 1024 * 1024);
        assert_eq!(DOCX_MAX_XML_PART_BYTES, 32 * 1024 * 1024);
        for result in [
            check_input_size(DOCX_MAX_INPUT_BYTES + 1),
            check_entry_count(DOCX_MAX_ENTRIES + 1),
            check_total_uncompressed(DOCX_MAX_TOTAL_UNCOMPRESSED_BYTES + 1),
            check_xml_part_size(DOCX_MAX_XML_PART_BYTES + 1),
        ] {
            let error = result.unwrap_err();
            assert_eq!(error.code(), Some(E_DOCX_LIMIT));
            assert!(matches!(error, ImportError::DocxLimit { .. }));
        }
        assert!(check_input_size(DOCX_MAX_INPUT_BYTES).is_ok());
        assert!(check_entry_count(DOCX_MAX_ENTRIES).is_ok());
        assert!(check_total_uncompressed(DOCX_MAX_TOTAL_UNCOMPRESSED_BYTES).is_ok());
        assert!(check_xml_part_size(DOCX_MAX_XML_PART_BYTES).is_ok());

        // End to end: too many entries, and an XML part larger than the read bound.
        let names: Vec<String> = (0..=DOCX_MAX_ENTRIES)
            .map(|i| format!("pad/{i}.xml"))
            .collect();
        let mut parts: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), "")).collect();
        let doc = document(&p("x"));
        parts.push(("word/document.xml", &doc));
        let error = import_docx("x.docx", &package(&parts), &audit()).unwrap_err();
        assert_eq!(error.code(), Some(E_DOCX_LIMIT));
        let big = format!(
            "{}{}",
            document(&p("x")),
            " ".repeat(DOCX_MAX_XML_PART_BYTES as usize + 1)
        );
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "word/document.xml",
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(big.as_bytes()).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert!((bytes.len() as u64) < DOCX_MAX_XML_PART_BYTES);
        let error = import_docx("x.docx", &bytes, &audit()).unwrap_err();
        assert_eq!(
            error,
            ImportError::DocxLimit {
                limit: "XML part bytes".to_owned(),
                actual: DOCX_MAX_XML_PART_BYTES + 1,
                max: DOCX_MAX_XML_PART_BYTES
            }
        );
    }

    // ------------------------------------------------------------------ artifacts and determinism

    #[test]
    fn artifacts_identity_and_determinism() {
        let bytes = docx(
            &[
                p_props(r#"<w:pStyle w:val="Heading1"/>"#, "Title"),
                p("Body"),
            ]
            .concat(),
        );
        let imported = import(&bytes);
        let original = &imported.original_artifact;
        assert_eq!(original.kind, ArtifactKind::SourceOriginal);
        assert_eq!(original.media_type, DOCX_MEDIA);
        assert_eq!(original.bytes, bytes);
        assert_eq!(original.hash, Hash::content_sha256(&bytes));
        let NodePayload::SourceArtifact(source) = &imported.source.payload else {
            panic!()
        };
        assert_eq!(source.source_kind, "docx");
        assert_eq!(source.media_type, DOCX_MEDIA);
        assert_eq!(source.content_hash, original.hash);
        assert_eq!(
            imported.source.id,
            plumb_psg::source_artifact_id(&original.hash).unwrap()
        );
        let extracted = &imported.extracted_artifact;
        assert_eq!(extracted.kind, ArtifactKind::SourceExtracted);
        assert_eq!(extracted.media_type, "application/json");
        assert_eq!(
            extracted.bytes,
            br#"{"text":"Title\nBody","version":1}"#.to_vec()
        );
        assert_eq!(
            extracted.bytes,
            to_canonical_json(&ExtractedTextArtifact {
                version: 1,
                text: "Title\nBody".to_owned()
            })
            .unwrap()
        );
        assert_eq!(extracted.hash, Hash::content_sha256(&extracted.bytes));

        assert_eq!(import(&bytes), imported);
        let later = import_docx(
            "requirements.docx",
            &bytes,
            &audit_at("2031-01-01T00:00:00.000000000Z"),
        )
        .unwrap();
        assert_eq!(later.source.id, imported.source.id);
        assert_eq!(later.source.payload, imported.source.payload);
        assert_eq!(later.source.extensions, imported.source.extensions);
        assert_eq!(
            later.fragments.iter().map(|f| &f.id).collect::<Vec<_>>(),
            imported.fragments.iter().map(|f| &f.id).collect::<Vec<_>>()
        );
        assert_eq!(
            later.original_artifact.hash,
            imported.original_artifact.hash
        );
        assert_eq!(
            later.extracted_artifact.bytes,
            imported.extracted_artifact.bytes
        );
        assert_eq!(later.parse, imported.parse);
        assert_ne!(
            later.original_artifact.created_at,
            imported.original_artifact.created_at
        );
        check(&imported);
    }

    // ------------------------------------------------------------------ HR fixture (§§49-51)

    #[test]
    fn hr_docx_imports_completely_and_deterministically() {
        let imported = import_docx("requirements.docx", HR_DOCX, &audit()).unwrap();
        let NodePayload::SourceArtifact(source) = &imported.source.payload else {
            panic!()
        };
        assert_eq!(source.source_kind, "docx");
        assert_eq!(source.media_type, DOCX_MEDIA);
        assert_eq!(imported.parse, ParseMetadata::complete());
        assert_eq!(
            import_docx("requirements.docx", HR_DOCX, &audit()).unwrap(),
            imported
        );
        check(&imported);
    }

    #[test]
    fn hr_docx_requirements_are_in_order() {
        let imported = import_docx("requirements.docx", HR_DOCX, &audit()).unwrap();
        let text = extracted_text(&imported);
        let mut previous = 0;
        for n in 1..=32 {
            let requirement = format!("HR-{n:03}");
            assert_eq!(text.matches(&requirement).count(), 1, "{requirement}");
            let position = text.find(&requirement).unwrap();
            assert!(position > previous || n == 1, "{requirement}");
            previous = position;
        }
    }

    #[test]
    fn hr_docx_requirement_fragments_are_paragraphs() {
        // The fixture's ordinals are literal w:t text and it has no effective w:numPr, so
        // its requirement paragraphs are Paragraph, not ListItem (Hotfix 022).
        let imported = import_docx("requirements.docx", HR_DOCX, &audit()).unwrap();
        let mut seen = 0;
        for fragment in &imported.fragments {
            let text = fragment_text(fragment);
            if (1..=32).any(|n| text.contains(&format!("HR-{n:03}"))) {
                assert_eq!(metadata(fragment).kind, Paragraph, "{text}");
                seen += 1;
            }
        }
        assert_eq!(seen, 32);
        assert!(imported
            .fragments
            .iter()
            .all(|f| metadata(f).kind != ListItem));
    }

    #[test]
    fn hr_docx_reference_table_rows() {
        let imported = import_docx("requirements.docx", HR_DOCX, &audit()).unwrap();
        let header = vec![
            "Leave type".to_owned(),
            "Deducts annual balance".to_owned(),
            "Eligible contract".to_owned(),
        ];
        let rows: Vec<&str> = imported
            .fragments
            .iter()
            .filter(|f| metadata(f).header_cells.as_ref() == Some(&header))
            .map(fragment_text)
            .collect();
        assert_eq!(
            rows,
            [
                "Annual\tyes\tEmployee",
                "Unpaid\tno\tEmployee, Contractor",
                "Sick\tno\tEmployee"
            ]
        );
    }

    // ------------------------------------------------------------------ source guard (§53)

    #[test]
    fn docx_source_has_no_disk_store_clock_or_network_capability() {
        let source = include_str!("../src/docx.rs");
        for forbidden in [
            "std::fs",
            "File::open",
            "tempfile",
            "LibreOffice",
            "python",
            "pandoc",
            "ArtifactStore",
            "Sqlite",
            "SystemClock",
            "Clock::now",
            "Utc::now",
            "reqwest",
            "InferenceProvider",
            "LlmProvider",
            "Graph::new",
            "SemanticPatch",
            "Proposal",
            "unsafe",
        ] {
            assert!(!source.contains(forbidden), "docx.rs contains {forbidden}");
        }
    }
}
