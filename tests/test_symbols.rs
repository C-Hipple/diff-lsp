//! Pins the document symbol reverse mapping against the real fixtures.
use std::collections::HashMap;
use std::fs;

use diff_lsp::parsers::utils::{LineType, Parsable, ParsedDiff};
use diff_lsp::symbols::{build_diff_symbols, build_file_indexes, prefix_width, translate_range};
use tower_lsp::lsp_types::{DocumentSymbol, DocumentSymbolResponse, Position, Range, SymbolKind};

fn load(name: &str) -> ParsedDiff {
    let raw = fs::read_to_string(format!("tests/data/{}", name)).unwrap();
    ParsedDiff::parse(&raw).unwrap()
}

fn r(sl: u32, sc: u32, el: u32, ec: u32) -> Range {
    Range::new(Position::new(sl, sc), Position::new(el, ec))
}

/// Every visible diff line must map back to itself: the inverse of the
/// forward mapping the hover/definition handlers use
/// (editor line K -> lines_map[K].source_line_number S -> backend line S - 1).
#[test]
fn reverse_index_inverts_forward_mapping_for_all_fixtures() {
    for name in [
        "go_diff.code_review_server",
        "go_diff.code_review",
        "full_go_diff.code_review",
        "go_diff.magit_status",
        "rust_diff.magit_status",
        "big_rust_diff.magit_status",
    ] {
        let diff = load(name);
        let files = build_file_indexes(&diff);
        assert!(!files.is_empty(), "{name}");
        for (key, (filename, line)) in &diff.lines_map {
            if line.line_type == LineType::Removed {
                continue;
            }
            let file = files.iter().find(|f| &f.filename == filename).unwrap();
            let backend_line = u32::from(line.source_line_number.0) - 1;
            let fwd = diff.map_diff_line_to_src(key.0);
            if let Some(fwd) = fwd {
                assert_eq!(u32::from(fwd.source_line.0) - 1, backend_line);
            }
            assert_eq!(
                file.visible[&backend_line].input_line,
                u32::from(key.0),
                "{name}: {filename} line {}",
                key.0
            );
        }
    }
}

#[test]
fn code_review_server_config_go_pin() {
    let diff = load("go_diff.code_review_server");
    assert_eq!(prefix_width(&diff), 2);
    let files = build_file_indexes(&diff);
    let names: Vec<_> = files.iter().map(|f| f.filename.as_str()).collect();
    assert_eq!(names[0], "README.md");
    assert_eq!(names[1], "config.go");

    // Line 78 (1-based in the fixture) is "  \tWorkflows []workflows.Workflow",
    // the first line after "@@ -17,18 +17,20 @@" -> source line 17 -> backend line 16.
    let config = &files[1];
    assert_eq!(config.visible[&16].input_line, 78);
    let fwd = diff.map_diff_line_to_src(78).unwrap();
    assert_eq!(fwd.source_line.0, 17);

    // `Workflows` starts at backend char 1 (after the tab); +2 for "  " prefix.
    let got = translate_range(config, &r(16, 1, 16, 10), 2).unwrap();
    assert_eq!(got, r(78, 3, 78, 12));
}

#[test]
fn magit_rust_diff_pin() {
    let diff = load("rust_diff.magit_status");
    assert_eq!(prefix_width(&diff), 1);
    let files = build_file_indexes(&diff);
    let client = files
        .iter()
        .find(|f| f.filename == "src/client.rs")
        .unwrap();
    // "@@ -60,9 +60,10 @@" is fixture line 11, so line 12 is source line 60 (backend 59).
    assert_eq!(client.first_input_line, 12);
    assert_eq!(client.visible[&59].input_line, 12);
}

#[test]
#[allow(deprecated)]
fn build_symbols_for_code_review_server_fixture() {
    let diff = load("go_diff.code_review_server");
    let mut backend = HashMap::new();
    backend.insert(
        "config.go".to_string(),
        DocumentSymbolResponse::Nested(vec![DocumentSymbol {
            name: "Config".to_string(),
            detail: None,
            kind: SymbolKind::STRUCT,
            tags: None,
            deprecated: None,
            // type Config struct { starts above the hunk (line 15, 0-based 14)
            range: r(14, 0, 40, 1),
            selection_range: r(14, 5, 14, 11),
            children: None,
        }]),
    );
    let out = build_diff_symbols(&diff, &backend);
    assert_eq!(out.len(), 9);
    assert!(out.iter().all(|s| s.kind == SymbolKind::FILE));
    let config = out.iter().find(|s| s.name == "config.go").unwrap();
    let children = config.children.as_ref().unwrap();
    assert_eq!(children[0].name, "Config");
    // clamped to start at the first visible line of the hunk
    assert_eq!(children[0].range.start, Position::new(78, 0));
    assert_eq!(children[0].selection_range, r(78, 0, 78, 0));
    assert!(children[0].range.end <= config.range.end);
    // files appear in buffer order and don't overlap
    for pair in out.windows(2) {
        assert!(pair[0].range.end < pair[1].range.start);
    }
}
