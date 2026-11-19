use diff_lsp::parsers::utils::{parse_header, LineType, Parsable, ParsedDiff, SourceLineNumber};

#[test]
fn test_parse_header_beyond_u16() {
    assert_eq!(
        parse_header("@@ -70000,3 +70000,4 @@ fn foo()"),
        Some((70000, 3, 70000, 4))
    );
}

#[test]
fn test_parse_header_malformed() {
    assert_eq!(parse_header("@@ garbage @@"), None);
    // Doesn't fit in a u32
    assert_eq!(parse_header("@@ -1,1 +99999999999,1 @@"), None);
}

#[test]
fn test_code_review_hunk_beyond_u16() {
    let raw = "Type: my-code-review\n\
               Root: /tmp/foo\n\
               \n\
               modified   src/big.rs\n\
               @@ -70000,3 +70000,4 @@ fn foo()\n \
               context\n\
               +added\n \
               context2\n";
    let diff = ParsedDiff::parse(raw).unwrap();
    let m = diff.map_diff_line_to_src(6).unwrap();
    assert_eq!(m.source_line, SourceLineNumber(70000));
    let m = diff.map_diff_line_to_src(7).unwrap();
    assert_eq!(m.source_line, SourceLineNumber(70001));
    assert_eq!(m.source_line_type, LineType::Added);
    let m = diff.map_diff_line_to_src(8).unwrap();
    assert_eq!(m.source_line, SourceLineNumber(70002));
}

#[test]
fn test_buffer_line_beyond_u16() {
    // A review buffer longer than 65,535 lines.
    let mut raw = String::from("Type: my-code-review\nRoot: /tmp/foo\n\nmodified   src/big.rs\n@@ -1,1 +1,70000 @@\n");
    for i in 0..70_000 {
        raw.push_str(&format!("+line {}\n", i));
    }
    let diff = ParsedDiff::parse(&raw).unwrap();
    // Input line 70_005 is the 70_000th added line.
    let m = diff.map_diff_line_to_src(70_005).unwrap();
    assert_eq!(m.source_line, SourceLineNumber(70_000));
}

#[test]
fn test_magit_hunk_beyond_u16() {
    let raw = "Type: magit-status\n\
               Head: main foo\n\
               \n\
               modified   src/big.rs\n\
               @@ -70000,3 +70000,4 @@ fn foo()\n \
               context\n\
               +added\n";
    let diff = ParsedDiff::parse(raw).unwrap();
    let m = diff.map_diff_line_to_src(7).unwrap();
    assert_eq!(m.source_line, SourceLineNumber(70001));
}

#[test]
fn test_malformed_hunk_header_does_not_panic() {
    for kind in ["my-code-review", "magit-status"] {
        let raw = format!(
            "Type: {}\n\nmodified   src/a.rs\n@@ not a header @@\n+added\n@@ -1,1 +5,1 @@\n+ok\n",
            kind
        );
        let diff = ParsedDiff::parse(&raw).unwrap();
        // Lines after a malformed header aren't mapped against a stale hunk.
        assert!(diff.map_diff_line_to_src(5).is_none(), "{}", kind);
        let m = diff.map_diff_line_to_src(7).unwrap();
        assert_eq!(m.source_line, SourceLineNumber(5), "{}", kind);
    }
}

#[test]
fn test_bare_file_header_does_not_panic() {
    let raw = "Type: my-code-review\n\nmodified\ndiff --git \n@@ -1,1 +1,1 @@\n+x\n";
    assert!(ParsedDiff::parse(raw).is_some());
}
