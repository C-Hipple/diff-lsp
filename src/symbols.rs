//! `textDocument/documentSymbol` support for diff buffers.
//!
//! Everything in this module is pure: it takes a [`ParsedDiff`] plus the
//! symbol responses the backend servers returned for the real source files,
//! and produces the nested [`DocumentSymbol`] tree for the diff buffer.  The
//! server handler only has to fetch the backend responses.
//!
//! # Line-number conventions
//!
//! These mirror the forward mapping used by the other handlers (hover,
//! definition, ...):
//!
//! * `ParsedDiff.lines_map` keys ([`InputLineNumber`]) are used *directly* as
//!   the diff buffer's LSP `position.line` (see `DiffLsp::get_source_map`).
//! * [`SourceLineNumber`] is 1-based (it comes from the `+start` of a hunk
//!   header) while LSP ranges coming from backends are 0-based, so the
//!   handlers send `source_line - 1` to the backends.
//!
//! The reverse mapping is therefore: backend line `b` -> source line `b + 1`
//! -> the input line whose diff line has that source line number and is
//! visible (added or unmodified).  Removed lines carry the source line number
//! of the following line, so they are never targets of the reverse mapping.

use std::collections::{BTreeMap, HashMap};

use tower_lsp::lsp_types::{
    DocumentSymbol, DocumentSymbolResponse, Position, Range, SymbolInformation, SymbolKind,
};

use crate::parsers::utils::{LineType, ParsedDiff};

/// A visible (added / unmodified) line of a file inside the diff buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleLine {
    /// Line in the diff buffer (LSP coordinates, see module docs).
    pub input_line: u32,
    /// Length of the whole diff buffer line (prefix included) in UTF-16 code units.
    pub len: u32,
}

/// Reverse index for one file of the diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIndex {
    /// Relative path, as it appears in the diff.
    pub filename: String,
    /// First diff buffer line belonging to this file.
    pub first_input_line: u32,
    /// Last diff buffer line belonging to this file.
    pub last_input_line: u32,
    /// UTF-16 length of the last diff buffer line of this file.
    pub last_line_len: u32,
    /// 0-based source line (backend coordinates) -> diff buffer line.
    pub visible: BTreeMap<u32, VisibleLine>,
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

/// Builds one [`FileIndex`] per file present in `lines_map`, ordered by their
/// position in the diff buffer.
///
/// If the same source line shows up more than once for a file (e.g. a file
/// listed both under staged and unstaged changes in magit), the first
/// occurrence in the buffer wins.
pub fn build_file_indexes(diff: &ParsedDiff) -> Vec<FileIndex> {
    let mut by_file: HashMap<&str, FileIndex> = HashMap::new();

    let mut keys: Vec<_> = diff.lines_map.keys().copied().collect();
    keys.sort();

    for key in keys {
        let (filename, diff_line) = &diff.lines_map[&key];
        let input_line = key.0;
        let len = utf16_len(&diff_line.line);
        let entry = by_file
            .entry(filename.as_str())
            .or_insert_with(|| FileIndex {
                filename: filename.clone(),
                first_input_line: input_line,
                last_input_line: input_line,
                last_line_len: len,
                visible: BTreeMap::new(),
            });
        // keys are iterated in ascending order
        entry.last_input_line = input_line;
        entry.last_line_len = len;

        let source_line = diff_line.source_line_number.0;
        if matches!(diff_line.line_type, LineType::Added | LineType::Unmodified) && source_line > 0
        {
            entry
                .visible
                .entry(source_line - 1)
                .or_insert(VisibleLine { input_line, len });
        }
    }

    let mut files: Vec<FileIndex> = by_file.into_values().collect();
    files.sort_by_key(|f| f.first_input_line);
    files
}

/// Translates a backend (source file) range into diff buffer coordinates.
///
/// The range is clamped to the visible lines of the file: if the start line
/// is not visible, the range starts at column 0 of the first visible line
/// inside the range; if the end line is not visible, it ends at the end of the
/// last visible line inside the range.  Returns `None` when no line of the
/// range is visible.
pub fn translate_range(index: &FileIndex, range: &Range, prefix: u32) -> Option<Range> {
    let (start_line, end_line) = (range.start.line, range.end.line);
    if start_line > end_line {
        return None;
    }
    let mut in_range = index.visible.range(start_line..=end_line);
    let (&first_src, &first) = in_range.next()?;
    let (&last_src, &last) = in_range.next_back().unwrap_or((&first_src, &first));

    let start = if first_src == start_line {
        Position::new(
            first.input_line,
            (range.start.character + prefix).min(first.len),
        )
    } else {
        Position::new(first.input_line, 0)
    };
    let mut end = if last_src == end_line {
        Position::new(
            last.input_line,
            (range.end.character + prefix).min(last.len),
        )
    } else {
        Position::new(last.input_line, last.len)
    };
    if end < start {
        // Only possible with duplicated files in the buffer; keep it well formed.
        end = start;
    }
    Some(Range::new(start, end))
}

fn contains(outer: &Range, inner: &Range) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// Translates a backend symbol tree into diff buffer coordinates, dropping
/// symbols (and their subtrees) that have no visible line.
pub fn translate_symbols(
    index: &FileIndex,
    symbols: &[DocumentSymbol],
    prefix: u32,
) -> Vec<DocumentSymbol> {
    symbols
        .iter()
        .filter_map(|symbol| translate_symbol(index, symbol, prefix))
        .collect()
}

fn translate_symbol(
    index: &FileIndex,
    symbol: &DocumentSymbol,
    prefix: u32,
) -> Option<DocumentSymbol> {
    let range = translate_range(index, &symbol.range, prefix)?;
    // LSP requires selection_range to be contained in range.  If the name is
    // not visible (e.g. a function whose signature is outside the hunk), point
    // at the start of the visible part.
    let selection_range = translate_range(index, &symbol.selection_range, prefix)
        .filter(|sel| contains(&range, sel))
        .unwrap_or(Range::new(range.start, range.start));
    let children = symbol
        .children
        .as_ref()
        .map(|children| translate_symbols(index, children, prefix))
        .filter(|children| !children.is_empty());

    let mut translated = symbol.clone();
    translated.range = range;
    translated.selection_range = selection_range;
    translated.children = children;
    Some(translated)
}

#[allow(deprecated)] // DocumentSymbol::deprecated
fn symbol_information_to_document_symbol(info: SymbolInformation) -> DocumentSymbol {
    DocumentSymbol {
        name: info.name,
        detail: None,
        kind: info.kind,
        tags: info.tags,
        deprecated: info.deprecated,
        range: info.location.range,
        selection_range: info.location.range,
        children: None,
    }
}

/// Converts a flat `SymbolInformation` list into a tree, nesting each symbol
/// under the innermost preceding symbol whose range contains it.
pub fn flat_to_nested(symbols: Vec<SymbolInformation>) -> Vec<DocumentSymbol> {
    let mut symbols: Vec<DocumentSymbol> = symbols
        .into_iter()
        .map(symbol_information_to_document_symbol)
        .collect();
    // Outer symbols first when two start at the same position.
    symbols.sort_by(|a, b| {
        a.range
            .start
            .cmp(&b.range.start)
            .then(b.range.end.cmp(&a.range.end))
    });

    fn attach(stack: &mut [DocumentSymbol], roots: &mut Vec<DocumentSymbol>, done: DocumentSymbol) {
        match stack.last_mut() {
            Some(parent) => parent.children.get_or_insert_with(Vec::new).push(done),
            None => roots.push(done),
        }
    }

    let mut roots = Vec::new();
    let mut stack: Vec<DocumentSymbol> = Vec::new();
    for symbol in symbols {
        while let Some(top) = stack.last() {
            if contains(&top.range, &symbol.range) {
                break;
            }
            let done = stack.pop().unwrap();
            attach(&mut stack, &mut roots, done);
        }
        stack.push(symbol);
    }
    while let Some(done) = stack.pop() {
        attach(&mut stack, &mut roots, done);
    }
    roots
}

/// Normalizes either response shape into a symbol tree.
pub fn response_to_nested(response: DocumentSymbolResponse) -> Vec<DocumentSymbol> {
    match response {
        DocumentSymbolResponse::Nested(symbols) => symbols,
        DocumentSymbolResponse::Flat(symbols) => flat_to_nested(symbols),
    }
}

/// Builds the symbol tree for a diff buffer.
///
/// Returns one top-level [`SymbolKind::FILE`] symbol per file in the diff
/// (named by its relative path and spanning the file's lines in the buffer).
/// Its children are the backend symbols for that file (keyed by relative
/// filename in `backend_symbols`) that intersect the visible hunk lines.
/// Files without a backend response still get their top-level entry.
#[allow(deprecated)] // DocumentSymbol::deprecated
pub fn build_diff_symbols(
    diff: &ParsedDiff,
    backend_symbols: &HashMap<String, DocumentSymbolResponse>,
) -> Vec<DocumentSymbol> {
    let prefix = diff.marker_width;
    build_file_indexes(diff)
        .into_iter()
        .map(|index| {
            let children = backend_symbols
                .get(&index.filename)
                .map(|response| {
                    translate_symbols(&index, &response_to_nested(response.clone()), prefix)
                })
                .filter(|children| !children.is_empty());
            let start = Position::new(index.first_input_line, 0);
            DocumentSymbol {
                name: index.filename.clone(),
                detail: None,
                kind: SymbolKind::FILE,
                tags: None,
                deprecated: None,
                range: Range::new(
                    start,
                    Position::new(index.last_input_line, index.last_line_len),
                ),
                selection_range: Range::new(start, start),
                children,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::utils::Parsable;
    use tower_lsp::lsp_types::{Location, Url};

    // Line numbers (1-based, as in an editor) on the right.
    const MAGIT: &str = "Project: magit: test
Root: /tmp/test/
Buffer: test
Type: magit-status
Head:     main x

Unstaged changes (1)
modified   src/foo.rs
@@ -10,4 +10,5 @@ fn foo() {
     let a = 1;
-    let b = 2;
+    let b = 3;
+    let c = 4;
     a + b
 }
@@ -30,2 +31,2 @@ fn bar() {
     x
-    y
+    z
modified   src/bar.rs
@@ -1,1 +1,1 @@
-fn é() {}
+fn ü() {}
";
    // lines_map keys -> (source line, type):
    // src/foo.rs: 10->(10,U) 11->(11,R) 12->(11,A) 13->(12,A) 14->(13,U) 15->(14,U)
    //             17->(31,U) 18->(32,R) 19->(32,A)
    // src/bar.rs: 22->(1,R) 23->(1,A)

    fn diff() -> ParsedDiff {
        ParsedDiff::parse(MAGIT).unwrap()
    }

    fn foo_index() -> FileIndex {
        build_file_indexes(&diff()).remove(0)
    }

    fn r(sl: u32, sc: u32, el: u32, ec: u32) -> Range {
        Range::new(Position::new(sl, sc), Position::new(el, ec))
    }

    #[allow(deprecated)]
    fn sym(
        name: &str,
        range: Range,
        selection: Range,
        children: Vec<DocumentSymbol>,
    ) -> DocumentSymbol {
        DocumentSymbol {
            name: name.to_string(),
            detail: None,
            kind: SymbolKind::FUNCTION,
            tags: None,
            deprecated: None,
            range,
            selection_range: selection,
            children: if children.is_empty() {
                None
            } else {
                Some(children)
            },
        }
    }

    #[allow(deprecated)]
    fn info(name: &str, range: Range) -> SymbolInformation {
        SymbolInformation {
            name: name.to_string(),
            kind: SymbolKind::FUNCTION,
            tags: None,
            deprecated: None,
            location: Location::new(Url::parse("file:///tmp/test/src/foo.rs").unwrap(), range),
            container_name: None,
        }
    }

    #[test]
    fn index_matches_forward_mapping_conventions() {
        let diff = diff();
        let files = build_file_indexes(&diff);
        assert_eq!(files.len(), 2);
        let foo = &files[0];
        assert_eq!(foo.filename, "src/foo.rs");
        assert_eq!(foo.first_input_line, 10);
        assert_eq!(foo.last_input_line, 19);

        // forward: editor line 12 -> key 12 -> source line 11 (1-based) -> backend line 10
        let fwd = diff.map_diff_line_to_src(12).unwrap();
        assert_eq!(fwd.source_line.0, 11);
        // reverse: backend line 10 -> editor line 12 (not the removed line 11)
        assert_eq!(foo.visible[&10].input_line, 12);

        let expected: Vec<(u32, u32)> = vec![
            (9, 10),
            (10, 12),
            (11, 13),
            (12, 14),
            (13, 15),
            (30, 17),
            (31, 19),
        ];
        let actual: Vec<(u32, u32)> = foo
            .visible
            .iter()
            .map(|(s, v)| (*s, v.input_line))
            .collect();
        assert_eq!(actual, expected);

        let bar = &files[1];
        assert_eq!(bar.filename, "src/bar.rs");
        assert_eq!((bar.first_input_line, bar.last_input_line), (22, 23));
        assert_eq!(bar.visible[&0].input_line, 23);
        // "+fn ü() {}" is 10 UTF-16 code units
        assert_eq!(bar.visible[&0].len, 10);
    }

    #[test]
    fn round_trip_every_visible_line() {
        let diff = diff();
        let files = build_file_indexes(&diff);
        for (key, (filename, line)) in &diff.lines_map {
            if line.line_type == LineType::Removed {
                continue;
            }
            let file = files.iter().find(|f| &f.filename == filename).unwrap();
            let backend_line = line.source_line_number.0 - 1;
            assert_eq!(file.visible[&backend_line].input_line, key.0);
        }
    }

    #[test]
    fn magit_diff_uses_one_column_marker() {
        assert_eq!(diff().marker_width, 1);
    }

    #[test]
    fn fully_visible_range_shifts_character_by_prefix() {
        // "    let b = 3;" -> `b` at source char 8 on backend line 10
        let got = translate_range(&foo_index(), &r(10, 8, 10, 9), 1).unwrap();
        assert_eq!(got, r(12, 9, 12, 10));
        let got = translate_range(&foo_index(), &r(10, 8, 10, 9), 2).unwrap();
        assert_eq!(got, r(12, 10, 12, 11));
    }

    #[test]
    fn start_before_hunk_is_clamped_to_first_visible_line() {
        // fn foo spans backend lines 8..=13; line 8 (the signature) is not in the diff
        let got = translate_range(&foo_index(), &r(8, 0, 13, 1), 1).unwrap();
        assert_eq!(got, r(10, 0, 15, 2));
    }

    #[test]
    fn end_after_hunk_is_clamped_to_end_of_last_visible_line() {
        // backend 12..=20; last visible line in range is backend 13 -> input 15 " }" (len 2)
        let got = translate_range(&foo_index(), &r(12, 4, 20, 0), 1).unwrap();
        assert_eq!(got, r(14, 5, 15, 2));
    }

    #[test]
    fn range_spanning_two_hunks_covers_both() {
        let got = translate_range(&foo_index(), &r(9, 0, 31, 5), 1).unwrap();
        assert_eq!(got, r(10, 1, 19, 6));
    }

    #[test]
    fn invisible_ranges_are_dropped() {
        let idx = foo_index();
        assert_eq!(translate_range(&idx, &r(0, 0, 8, 3), 1), None);
        assert_eq!(translate_range(&idx, &r(14, 0, 29, 0), 1), None); // between hunks
        assert_eq!(translate_range(&idx, &r(40, 0, 50, 0), 1), None);
        assert_eq!(translate_range(&idx, &r(5, 0, 2, 0), 1), None); // malformed
    }

    #[test]
    fn character_is_clamped_to_line_length() {
        let got = translate_range(&foo_index(), &r(13, 0, 13, 100), 1).unwrap();
        assert_eq!(got, r(15, 1, 15, 2));
    }

    #[test]
    fn symbols_keep_nesting_and_drop_invisible_children() {
        let input = vec![
            sym(
                "foo",
                r(8, 0, 13, 1),
                r(8, 3, 8, 6),
                vec![
                    sym("b", r(10, 4, 10, 14), r(10, 8, 10, 9), vec![]),
                    sym("hidden", r(2, 0, 3, 0), r(2, 0, 2, 1), vec![]),
                ],
            ),
            sym("gone", r(0, 0, 5, 0), r(0, 3, 0, 7), vec![]),
            sym(
                "bar",
                r(29, 0, 31, 3),
                r(29, 3, 29, 6),
                vec![sym(
                    "hidden_child",
                    r(29, 4, 29, 8),
                    r(29, 4, 29, 8),
                    vec![],
                )],
            ),
        ];
        let out = translate_symbols(&foo_index(), &input, 1);
        assert_eq!(out.len(), 2);

        let foo = &out[0];
        assert_eq!(foo.name, "foo");
        assert_eq!(foo.range, r(10, 0, 15, 2));
        // signature is not visible: selection collapses to start of visible range
        assert_eq!(foo.selection_range, r(10, 0, 10, 0));
        let children = foo.children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "b");
        assert_eq!(children[0].range, r(12, 5, 12, 15));
        assert_eq!(children[0].selection_range, r(12, 9, 12, 10));

        let bar = &out[1];
        assert_eq!(bar.range, r(17, 0, 19, 4));
        assert_eq!(bar.children, None);
    }

    #[test]
    fn flat_symbols_are_nested_by_containment() {
        let flat = vec![
            info("inner", r(10, 4, 10, 14)),
            info("other", r(30, 0, 31, 3)),
            info("outer", r(8, 0, 13, 1)),
            info("inner2", r(12, 0, 12, 5)),
        ];
        let nested = flat_to_nested(flat);
        let names: Vec<_> = nested.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["outer", "other"]);
        let kids: Vec<_> = nested[0]
            .children
            .as_ref()
            .unwrap()
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(kids, vec!["inner", "inner2"]);
        assert_eq!(nested[0].selection_range, nested[0].range);
    }

    #[test]
    fn build_diff_symbols_handles_missing_backends_and_both_shapes() {
        let diff = diff();
        let mut backend = HashMap::new();
        backend.insert(
            "src/foo.rs".to_string(),
            DocumentSymbolResponse::Flat(vec![info("foo", r(8, 0, 13, 1))]),
        );
        let out = build_diff_symbols(&diff, &backend);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "src/foo.rs");
        assert_eq!(out[0].kind, SymbolKind::FILE);
        assert_eq!(out[0].range, r(10, 0, 19, 6));
        assert_eq!(out[0].children.as_ref().unwrap()[0].name, "foo");
        // no backend response for bar.rs (e.g. the backend failed): still listed
        assert_eq!(out[1].name, "src/bar.rs");
        assert_eq!(out[1].range, r(22, 0, 23, 10));
        assert_eq!(out[1].children, None);

        let mut backend = HashMap::new();
        backend.insert(
            "src/bar.rs".to_string(),
            DocumentSymbolResponse::Nested(vec![sym("ü", r(0, 0, 0, 9), r(0, 3, 0, 4), vec![])]),
        );
        let out = build_diff_symbols(&diff, &backend);
        assert_eq!(out[0].children, None);
        let bar_children = out[1].children.as_ref().unwrap();
        assert_eq!(bar_children[0].range, r(23, 1, 23, 10));
        assert_eq!(bar_children[0].selection_range, r(23, 4, 23, 5));
    }
}
