//! Tests for mapping backend diagnostics onto diff buffers (src/diagnostics.rs).
//! All pure: hand-built `Diagnostic`s, fixture diffs, no live backend.

use std::collections::HashMap;
use std::fs;

use diff_lsp::diagnostics::*;
use diff_lsp::parsers::utils::{
    DiffHeader, DiffLine, InputLineNumber, LineType, Parsable, ParsedDiff, SourceLineNumber,
};
use serde_json::json;
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticSeverity, NumberOrString, Position, PublishDiagnosticsParams, Range, Url,
};

const ROOT: &str = "/repo";

fn parse_fixture(name: &str) -> ParsedDiff {
    let raw = fs::read_to_string(format!("tests/data/{name}")).unwrap();
    ParsedDiff::parse(&raw).unwrap()
}

fn file_uri(rel: &str) -> Url {
    Url::from_file_path(format!("{ROOT}/{rel}")).unwrap()
}

/// A backend diagnostic on 0-based `line`, characters `start..end`.
fn diag(line: u32, start: u32, end: u32, msg: &str) -> Diagnostic {
    Diagnostic {
        range: Range::new(Position::new(line, start), Position::new(line, end)),
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String("E0001".to_string())),
        source: Some("backend".to_string()),
        message: msg.to_string(),
        ..Default::default()
    }
}

fn store_with(rel: &str, diags: Vec<Diagnostic>) -> DiagnosticsStore {
    let mut store = DiagnosticsStore::new();
    store.update(PublishDiagnosticsParams::new(file_uri(rel), diags, None));
    store
}

// ---------------------------------------------------------------------------------------
// Mode / config
// ---------------------------------------------------------------------------------------

#[test]
fn mode_defaults_to_added() {
    assert_eq!(DiagnosticsMode::default(), DiagnosticsMode::Added);
    assert_eq!(
        DiagnosticsMode::from_initialization_options(None),
        DiagnosticsMode::Added
    );
    assert_eq!(
        DiagnosticsMode::from_initialization_options(Some(&json!({}))),
        DiagnosticsMode::Added
    );
    assert_eq!(
        DiagnosticsMode::from_initialization_options(Some(&json!({"diagnostics": "bogus"}))),
        DiagnosticsMode::Added
    );
    assert_eq!(
        DiagnosticsMode::from_initialization_options(Some(&json!("not an object"))),
        DiagnosticsMode::Added
    );
}

#[test]
fn mode_from_initialization_options() {
    let m = |v| DiagnosticsMode::from_initialization_options(Some(&v));
    assert_eq!(m(json!({"diagnostics": "added"})), DiagnosticsMode::Added);
    assert_eq!(
        m(json!({"diagnostics": "visible"})),
        DiagnosticsMode::Visible
    );
    assert_eq!(
        m(json!({"diagnostics": "VISIBLE"})),
        DiagnosticsMode::Visible
    );
    assert_eq!(m(json!({"diagnostics": "off"})), DiagnosticsMode::Off);
    assert_eq!(m(json!({"diagnostics": false})), DiagnosticsMode::Off);
}

#[test]
fn mode_accepts_line_types() {
    use DiagnosticsMode as M;
    use LineType as L;
    assert!(M::Added.accepts(L::Added));
    assert!(!M::Added.accepts(L::Unmodified));
    assert!(!M::Added.accepts(L::Removed));
    assert!(M::Visible.accepts(L::Added));
    assert!(M::Visible.accepts(L::Unmodified));
    assert!(!M::Visible.accepts(L::Removed));
    assert!(!M::Off.accepts(L::Added));
    assert!(!M::Off.accepts(L::Unmodified));
}

// ---------------------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------------------

#[test]
fn normalize_uri_key_collapses_slashes() {
    let a = Url::parse("file:///repo//src/a.rs").unwrap();
    let b = Url::parse("file:///repo/src/a.rs").unwrap();
    let c = Url::parse("file:///repo/./src/../src/a.rs").unwrap();
    assert_eq!(normalize_uri_key(&a), normalize_uri_key(&b));
    assert_eq!(normalize_uri_key(&c), normalize_uri_key(&b));
}

#[test]
fn store_replaces_and_clears() {
    let uri = file_uri("src/a.rs");
    let mut store = DiagnosticsStore::new();
    assert!(store.is_empty());

    assert!(store.update(PublishDiagnosticsParams::new(
        uri.clone(),
        vec![diag(1, 0, 1, "one")],
        None
    )));
    // Same content again: no change.
    assert!(!store.update(PublishDiagnosticsParams::new(
        uri.clone(),
        vec![diag(1, 0, 1, "one")],
        None
    )));
    // New publish replaces, not appends.
    assert!(store.update(PublishDiagnosticsParams::new(
        uri.clone(),
        vec![diag(2, 0, 1, "two")],
        None
    )));
    assert_eq!(store.get(&uri).unwrap().len(), 1);
    assert_eq!(store.get(&uri).unwrap()[0].message, "two");

    // Lookup tolerates a differently-joined URI.
    let doubled = Url::parse("file:///repo//src/a.rs").unwrap();
    assert!(store.get(&doubled).is_some());

    // Empty publish clears.
    assert!(store.update(PublishDiagnosticsParams::new(uri.clone(), vec![], None)));
    assert!(store.get(&uri).is_none());
    assert!(!store.update(PublishDiagnosticsParams::new(uri, vec![], None)));
}

#[test]
fn parse_publish_diagnostics_notification() {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": "file:///repo/src/a.rs",
            "diagnostics": [{
                "range": {"start": {"line": 3, "character": 4}, "end": {"line": 3, "character": 9}},
                "severity": 1,
                "message": "boom"
            }]
        }
    })
    .to_string();
    let params = parse_publish_diagnostics(&body).unwrap();
    assert_eq!(params.uri.as_str(), "file:///repo/src/a.rs");
    assert_eq!(params.diagnostics.len(), 1);
    assert_eq!(params.diagnostics[0].range.start, Position::new(3, 4));

    let other = json!({"jsonrpc": "2.0", "method": "window/logMessage", "params": {"type": 3, "message": "publishDiagnostics"}}).to_string();
    assert!(parse_publish_diagnostics(&other).is_none());
    let response = json!({"jsonrpc": "2.0", "id": 2, "result": null}).to_string();
    assert!(parse_publish_diagnostics(&response).is_none());
    assert!(parse_publish_diagnostics("not json").is_none());
}

// ---------------------------------------------------------------------------------------
// Line-number conventions, pinned against fixtures
// ---------------------------------------------------------------------------------------

#[test]
fn backend_line_is_zero_based_source_line_is_one_based() {
    assert_eq!(backend_line_to_source_line(0), Some(SourceLineNumber(1)));
    assert_eq!(backend_line_to_source_line(21), Some(SourceLineNumber(22)));
    assert_eq!(backend_line_to_source_line(u32::MAX), None);
    // Lines past u16 are fine now that line numbers are u32.
    assert_eq!(
        backend_line_to_source_line(70_000),
        Some(SourceLineNumber(70_001))
    );
}

#[test]
fn input_line_maps_to_same_lsp_line_as_get_source_map_uses() {
    // DiffLsp::get_source_map uses `position.line` directly as the lines_map key, so
    // publishing must be the inverse: InputLineNumber(k) -> LSP line k.
    assert_eq!(input_line_to_lsp_line(InputLineNumber(111)), 111);
}

#[test]
fn prefix_width_comes_from_parsed_diff_marker_width() {
    // Diagnostics shift by the same ParsedDiff::marker_width the request handlers
    // subtract (SourceMap::source_position): one source of truth.
    assert_eq!(parse_fixture("go_diff.code_review_server").marker_width, 2);
    assert_eq!(parse_fixture("go_diff.magit_status").marker_width, 1);
    assert_eq!(parse_fixture("go_diff.code_review").marker_width, 1);
}

#[test]
fn reverse_index_code_review_server_fixture() {
    let diff = parse_fixture("go_diff.code_review_server");
    let index = ReverseIndex::build(&diff);

    // `@@ -33,6 +33,7 @@`: input line 43 `+ ReleaseCommandCheck: str` is source line 36.
    let (input, line) = index.lookup("README.md", SourceLineNumber(36)).unwrap();
    assert_eq!(*input, InputLineNumber(43));
    assert_eq!(line.line_type, LineType::Added);

    // config.go source line 22: the removed lines 90..=100 also carry 22, but only the
    // added line 111 should be indexed.
    let (input, line) = index.lookup("config.go", SourceLineNumber(22)).unwrap();
    assert_eq!(*input, InputLineNumber(111));
    assert_eq!(line.line_type, LineType::Added);
    assert_eq!(line.line, "+ \tWorkflowType        string");

    // config.go line 21 is the unmodified `type RawWorkflow struct {` at input 89.
    let (input, line) = index.lookup("config.go", SourceLineNumber(21)).unwrap();
    assert_eq!(*input, InputLineNumber(89));
    assert_eq!(line.line_type, LineType::Unmodified);

    // The blank separator line after the first README hunk (input 47) is parsed as an
    // unmodified "source line 40" which isn't part of the hunk (+33,7 => 33..=39).
    assert!(index.lookup("README.md", SourceLineNumber(40)).is_none());
    // ...but a real empty context line ("  ", input 46 => source 39) is kept.
    assert_eq!(
        index.lookup("README.md", SourceLineNumber(39)).unwrap().0,
        InputLineNumber(46)
    );
}

#[test]
fn reverse_index_magit_fixture() {
    let diff = parse_fixture("go_diff.magit_status");
    let index = ReverseIndex::build(&diff);

    // `@@ -11,9 +11,10 @@`: input 15 `+var logger *log.Logger` is source line 14.
    let (input, line) = index.lookup("main.go", SourceLineNumber(14)).unwrap();
    assert_eq!(*input, InputLineNumber(15));
    assert_eq!(line.line_type, LineType::Added);

    // Input 14 is a genuinely empty context line in magit (whitespace stripped); keep it.
    let (input, line) = index.lookup("main.go", SourceLineNumber(13)).unwrap();
    assert_eq!(*input, InputLineNumber(14));
    assert_eq!(line.line_type, LineType::Unmodified);

    // main.go source 38: removed `-\t\tlogger = log.New(f, "", 0)` (28) vs added (29).
    assert_eq!(
        index.lookup("main.go", SourceLineNumber(38)).unwrap().0,
        InputLineNumber(29)
    );
}

// ---------------------------------------------------------------------------------------
// Mapping
// ---------------------------------------------------------------------------------------

#[test]
fn maps_added_line_in_code_review_server_diff() {
    let diff = parse_fixture("go_diff.code_review_server");
    // Backend line 21 (0-based) == source line 22 == `\tWorkflowType        string`.
    // `WorkflowType` starts at source char 1 (after the tab), ends at 13.
    let store = store_with("config.go", vec![diag(21, 1, 13, "unused field")]);

    let out = map_diagnostics_for_diff(&diff, ROOT, &store, DiagnosticsMode::Added);
    assert_eq!(out.len(), 1);
    let d = &out[0];
    assert_eq!(d.range.start, Position::new(111, 3)); // 2-char "+ " prefix
    assert_eq!(d.range.end, Position::new(111, 15));
    assert_eq!(d.message, "config.go:22: unused field");
    assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(d.source.as_deref(), Some("backend"));
    assert_eq!(d.code, Some(NumberOrString::String("E0001".to_string())));

    // Check the highlighted text is the identifier in the diff line.
    let text: Vec<char> = "+ \tWorkflowType        string".chars().collect();
    let highlighted: String = text
        [d.range.start.character as usize..d.range.end.character as usize]
        .iter()
        .collect();
    assert_eq!(highlighted, "WorkflowType");
}

#[test]
fn published_line_round_trips_through_hover_mapping() {
    // A diagnostic published on LSP line L must, when the user hovers on L, map back to
    // the same source line via the existing get_source_map/map_diff_line_to_src path,
    // which the handlers then send to the backend as `source_line - 1`.
    let diff = parse_fixture("go_diff.code_review_server");
    let backend_line = 21;
    let store = store_with("config.go", vec![diag(backend_line, 1, 13, "x")]);
    let out = map_diagnostics_for_diff(&diff, ROOT, &store, DiagnosticsMode::Added);

    let lsp_line = out[0].range.start.line;
    let sm = diff.map_diff_line_to_src(lsp_line).unwrap();
    assert_eq!(sm.file_name, "config.go");
    assert_eq!(sm.source_line_type, LineType::Added);
    assert_eq!(sm.source_line.0 - 1, backend_line);

    let diff = parse_fixture("go_diff.magit_status");
    let store = store_with("main.go", vec![diag(46, 1, 7, "y")]);
    let out = map_diagnostics_for_diff(&diff, ROOT, &store, DiagnosticsMode::Added);
    let lsp_line = out[0].range.start.line;
    let sm = diff.map_diff_line_to_src(lsp_line).unwrap();
    assert_eq!(sm.file_name, "main.go");
    assert_eq!(sm.source_line.0 - 1, 46);
}

#[test]
fn maps_added_line_in_magit_diff_with_one_char_prefix() {
    let diff = parse_fixture("go_diff.magit_status");
    // Backend line 13 == source 14 == `var logger *log.Logger`; `logger` is chars 4..10.
    let store = store_with("main.go", vec![diag(13, 4, 10, "unused variable")]);
    let out = map_diagnostics_for_diff(&diff, ROOT, &store, DiagnosticsMode::Added);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].range.start, Position::new(15, 5));
    assert_eq!(out[0].range.end, Position::new(15, 11));
    assert_eq!(&"+var logger *log.Logger"[5..11], "logger");
    assert_eq!(out[0].message, "main.go:14: unused variable");
}

#[test]
fn added_mode_skips_unmodified_lines_visible_mode_includes_them() {
    let diff = parse_fixture("go_diff.code_review_server");
    // Backend line 20 == source 21 == unmodified `type RawWorkflow struct {` (input 89).
    let store = store_with(
        "config.go",
        vec![diag(20, 0, 4, "context"), diag(21, 1, 13, "added")],
    );

    let added = map_diagnostics_for_diff(&diff, ROOT, &store, DiagnosticsMode::Added);
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].range.start.line, 111);

    let visible = map_diagnostics_for_diff(&diff, ROOT, &store, DiagnosticsMode::Visible);
    assert_eq!(visible.len(), 2);
    // Sorted by position.
    assert_eq!(visible[0].range.start, Position::new(89, 2));
    assert_eq!(visible[0].message, "config.go:21: context");
    assert_eq!(visible[1].range.start.line, 111);

    let off = map_diagnostics_for_diff(&diff, ROOT, &store, DiagnosticsMode::Off);
    assert!(off.is_empty());
}

#[test]
fn diagnostics_outside_the_diff_are_dropped() {
    let diff = parse_fixture("go_diff.magit_status");
    let mut store = DiagnosticsStore::new();
    // main.go line 1 is not in any hunk.
    store.update(PublishDiagnosticsParams::new(
        file_uri("main.go"),
        vec![diag(0, 0, 1, "not shown")],
        None,
    ));
    // A file that isn't part of the diff at all.
    store.update(PublishDiagnosticsParams::new(
        file_uri("other.go"),
        vec![diag(13, 0, 1, "not shown either")],
        None,
    ));
    let out = map_diagnostics_for_diff(&diff, ROOT, &store, DiagnosticsMode::Visible);
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn diagnostics_on_removed_lines_never_show() {
    let diff = parse_fixture("go_diff.magit_status");
    // main.go: source 18 is `-\tvar logger` (removed, input 19/20) and ` func init() {`
    // is 17. The removed lines' "18" is really the next new line (`defer func()`, input 21).
    let store = store_with("main.go", vec![diag(17, 0, 1, "on defer")]);
    let out = map_diagnostics_for_diff(&diff, ROOT, &store, DiagnosticsMode::Visible);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].range.start.line, 21);
    let out = map_diagnostics_for_diff(&diff, ROOT, &store, DiagnosticsMode::Added);
    assert!(out.is_empty());
}

#[test]
fn root_with_trailing_slash_and_multiple_files() {
    let diff = parse_fixture("go_diff.magit_status");
    let mut store = DiagnosticsStore::new();
    store.update(PublishDiagnosticsParams::new(
        file_uri("server/server.go"),
        vec![diag(16, 2, 18, "server")],
        None,
    ));
    store.update(PublishDiagnosticsParams::new(
        file_uri("main.go"),
        vec![diag(14, 4, 11, "main")],
        None,
    ));
    let out = map_diagnostics_for_diff(&diff, "/repo/", &store, DiagnosticsMode::Added);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].range.start, Position::new(16, 5));
    assert_eq!(out[0].message, "main.go:15: main");
    assert_eq!(out[1].range.start, Position::new(54, 3));
    assert_eq!(out[1].message, "server/server.go:17: server");
}

// ---------------------------------------------------------------------------------------
// map_diagnostic: ranges & clamping
// ---------------------------------------------------------------------------------------

fn added(line: &str, src: u32) -> DiffLine {
    DiffLine {
        line_type: LineType::Added,
        line: line.to_string(),
        source_line_number: SourceLineNumber(src),
    }
}

#[test]
fn multi_line_range_is_clamped_to_visible_line() {
    let dl = added("+fn foo() {", 10);
    let mut d = diag(9, 3, 0, "spans");
    d.range.end = Position::new(14, 1);
    let m = map_diagnostic(&d, "src/a.rs", InputLineNumber(5), &dl, 1);
    assert_eq!(m.range.start, Position::new(5, 4));
    assert_eq!(m.range.end, Position::new(5, 11)); // len("+fn foo() {")
}

#[test]
fn characters_past_end_of_line_are_clamped() {
    let dl = added("+x", 1);
    let m = map_diagnostic(&diag(0, 40, 50, "far"), "a.rs", InputLineNumber(3), &dl, 1);
    assert_eq!(m.range.start, Position::new(3, 2));
    assert_eq!(m.range.end, Position::new(3, 2));
}

#[test]
fn end_never_before_start() {
    let dl = added("+abcdef", 1);
    let mut d = diag(0, 4, 2, "inverted");
    d.range.end.character = 2;
    let m = map_diagnostic(&d, "a.rs", InputLineNumber(1), &dl, 1);
    assert!(m.range.end.character >= m.range.start.character);
}

#[test]
fn line_length_is_measured_in_utf16() {
    // "😀" is 2 UTF-16 code units; LSP positions default to UTF-16.
    let dl = added("+let s = \"😀\";", 1);
    let mut d = diag(0, 0, 0, "multi");
    d.range.end = Position::new(1, 0);
    let m = map_diagnostic(&d, "a.rs", InputLineNumber(1), &dl, 1);
    assert_eq!(m.range.end.character, 14);
}

#[test]
fn hand_built_diff_end_to_end() {
    let mut lines_map = HashMap::new();
    lines_map.insert(
        InputLineNumber(4),
        ("src/lib.rs".to_string(), added("+    let x = 1;", 7)),
    );
    lines_map.insert(
        InputLineNumber(5),
        (
            "src/lib.rs".to_string(),
            DiffLine {
                line_type: LineType::Unmodified,
                line: "     x".to_string(),
                source_line_number: SourceLineNumber(8),
            },
        ),
    );
    let mut headers = HashMap::new();
    headers.insert(DiffHeader::Type, "magit-status".to_string());
    let diff = ParsedDiff {
        headers,
        filenames: vec!["src/lib.rs".to_string(), "src/lib.rs".to_string()],
        lines_map,
        marker_width: 1,
        ..Default::default()
    };
    let mut store = DiagnosticsStore::new();
    store.update(PublishDiagnosticsParams::new(
        Url::parse("file:///proj/src/lib.rs").unwrap(),
        vec![diag(6, 8, 9, "unused x"), diag(7, 4, 5, "ctx")],
        None,
    ));
    let out = map_diagnostics_for_diff(&diff, "/proj", &store, DiagnosticsMode::Added);
    // Duplicate filenames don't duplicate diagnostics.
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].range.start, Position::new(4, 9));
    assert_eq!(out[0].range.end, Position::new(4, 10));
    assert_eq!(out[0].message, "src/lib.rs:7: unused x");

    let out = map_diagnostics_for_diff(&diff, "/proj", &store, DiagnosticsMode::Visible);
    assert_eq!(out.len(), 2);
    assert_eq!(out[1].range.start, Position::new(5, 5));
}
