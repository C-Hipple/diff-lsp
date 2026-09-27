// A position arrives in diff coordinates: a 1-indexed line of the file the
// client handed diff-lsp, and a column of that diff line with its +/-/space
// marker still in front. It has to reach the backend in source coordinates,
// or the backend answers for whatever sits a column or two to the right.
use diff_lsp::parsers::utils::{Parsable, ParsedDiff};
use std::fs;
use tower_lsp::lsp_types::Position;

fn at(line: u32, character: u32) -> Position {
    Position { line, character }
}

fn parse(path: &str) -> ParsedDiff {
    ParsedDiff::parse(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn test_bun_client_go_diff_maps_to_source_positions() {
    // What the bun client writes: its 5-line header, then the diff as
    // code-review-server renders it, "+ " / "- " / "  " before each line.
    let diff = parse("tests/data/go_diff.bun_client");
    assert_eq!(diff.marker_width, 2);

    // Line 25 is "+ \tx := Add(1, 2)", line 13 of demo.go. `Add` starts at
    // diff column 8, source column 6; backends count lines from 0.
    let add = diff.map_diff_line_to_src(25).unwrap();
    assert_eq!(add.file_name, "demo.go");
    assert_eq!(add.source_position(at(25, 8)), at(12, 6));

    // Line 26 is "+ \treturn Mul(x, 3)": the `x` at diff column 14 is source
    // column 12. Left at 14 it would be the `3`.
    let mul = diff.map_diff_line_to_src(26).unwrap();
    assert_eq!(mul.source_position(at(26, 14)), at(13, 12));
}

#[test]
fn test_marker_width_follows_the_diff_layout() {
    // The emacs client's code-review-server buffer uses the same rendering.
    assert_eq!(
        parse("tests/data/go_diff.code_review_server").marker_width,
        2
    );
    // Plain git diffs ("+new") and the older code-review buffer don't.
    assert_eq!(parse("tests/data/rust_diff.bun_client").marker_width, 1);
    assert_eq!(parse("tests/data/go_diff.code_review").marker_width, 1);
    // magit puts the source right after the marker, even when every line of
    // the hunk is indented.
    assert_eq!(parse("tests/data/rust_diff.magit_status").marker_width, 1);
    assert_eq!(
        parse("tests/data/big_rust_diff.magit_status").marker_width,
        1
    );
}

#[test]
fn test_plain_git_diff_keeps_one_marker_column() {
    let source = "Project: demo\nRoot: /src/demo\nWorktree: \nBuffer: PR #7\nType: code-review\n\
diff --git a/main.ts b/main.ts\n\
--- a/main.ts\n\
+++ b/main.ts\n\
@@ -1,3 +1,4 @@\n\
 import { formatGreeting } from './greet';\n\
 \n\
-console.log('hello');\n\
+const message = formatGreeting({ name: 'world' });\n";
    let diff = ParsedDiff::parse(source).unwrap();
    assert_eq!(diff.marker_width, 1);
    // `formatGreeting` at diff column 17 of line 13 is source column 16.
    let map = diff.map_diff_line_to_src(13).unwrap();
    assert_eq!(map.source_position(at(13, 17)), at(2, 16));
}

#[test]
fn test_source_position_never_underflows() {
    // A clicked marker column, or a removed line of a deleted file (whose
    // hunk starts at +0), still maps to a valid position.
    let source = "Project: demo\nRoot: /src/demo\nWorktree: \nBuffer: PR #7\nType: code-review\n\
diff --git a/gone.go b/gone.go\n\
--- a/gone.go\n\
+++ /dev/null\n\
\n\
@@ -1,2 +0,0 @@\n\
- package gone\n\
- \n";
    let diff = ParsedDiff::parse(source).unwrap();
    let map = diff.map_diff_line_to_src(11).unwrap();
    assert_eq!(map.source_position(at(11, 0)), at(0, 0));
}
