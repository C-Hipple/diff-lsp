//! Reverse mapping: backend results (source-file locations) -> diff-buffer locations.
//!
//! When a backend LSP returns a `Location` that falls on a source line which is
//! visible in the diff buffer (an added or unmodified line of a hunk for that
//! file), we can also offer the equivalent location *inside* the diff buffer, so
//! that e.g. "find references" during a code review stays in the review.
//!
//! Everything in this module is pure (no I/O, no backend) so it is easy to test.
//!
//! # Line number conventions
//!
//! * `ParsedDiff::lines_map` keys (`InputLineNumber`) are the 1-based line
//!   numbers of the parsed diff text.
//! * The server looks up `lines_map` using the editor's `position.line`
//!   *directly* (see `DiffLsp::get_source_map`).  To stay consistent with that
//!   (so that a location we return can be fed straight back into another
//!   request and land on the same diff line), the LSP line we emit for an
//!   `InputLineNumber(n)` is `n`.  This conversion lives in one place:
//!   [`input_line_to_lsp_line`].
//! * `DiffLine::source_line_number` is 1-based (from the hunk header), while
//!   backend `Location`s are 0-based LSP positions.  So a backend line `l`
//!   corresponds to source line `l + 1`.
//! * Columns: each diff line starts with `ParsedDiff::marker_width` columns of
//!   +/-/space marker (1 for git/magit, 2 for code-review-server's `"+ "`).
//!   The forward mapping (`SourceMap::source_position`) subtracts it; the
//!   reverse mapping adds it back, so the two round-trip.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;
use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, LocationLink, Position, Range, Url};

use crate::parsers::utils::{InputLineNumber, LineType, ParsedDiff};

/// How diff-buffer locations are returned alongside backend locations.
///
/// Configured from the editor via `initializationOptions`:
/// `{"diffLocations": "prefer" | "both" | "off"}`.  Defaults to `Both`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffLocationsMode {
    /// Replace a source location by its diff location when one exists.
    Prefer,
    /// Return diff locations first, followed by all source locations (deduplicated).
    #[default]
    Both,
    /// Return backend results unchanged.
    Off,
}

impl DiffLocationsMode {
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "prefer" => Some(DiffLocationsMode::Prefer),
            "both" => Some(DiffLocationsMode::Both),
            "off" | "none" | "false" => Some(DiffLocationsMode::Off),
            _ => None,
        }
    }

    /// Read the mode from the LSP `initializationOptions` value.  Missing or
    /// unrecognised values fall back to the default (`Both`).
    pub fn from_initialization_options(options: Option<&Value>) -> Self {
        options
            .and_then(|o| o.get("diffLocations"))
            .and_then(|v| match v {
                Value::String(s) => Self::from_str_opt(s),
                Value::Bool(false) => Some(DiffLocationsMode::Off),
                Value::Bool(true) => Some(DiffLocationsMode::Both),
                _ => None,
            })
            .unwrap_or_default()
    }
}

/// Convert an `InputLineNumber` to the LSP line the editor uses for it.
/// See the module docs for why this is the identity.
pub fn input_line_to_lsp_line(input: InputLineNumber) -> u32 {
    u32::from(input.0)
}

/// Index of `(relative filename, 1-based source line) -> InputLineNumber` for
/// lines that exist in the new version of the file (added/unmodified).
#[derive(Debug, Clone, Default)]
pub struct ReverseIndex {
    map: HashMap<(String, u32), InputLineNumber>,
    marker_width: u32,
}

impl ReverseIndex {
    pub fn from_diff(diff: &ParsedDiff) -> Self {
        let mut map: HashMap<(String, u32), InputLineNumber> = HashMap::new();
        // Iterate in input order so that if a (file, line) shows up more than
        // once (e.g. a file in both the staged and unstaged sections of a magit
        // buffer) the first occurrence wins, deterministically.
        let mut keys: Vec<&InputLineNumber> = diff.lines_map.keys().collect();
        keys.sort();
        for input in keys {
            let (filename, diff_line) = &diff.lines_map[input];
            if !matches!(diff_line.line_type, LineType::Added | LineType::Unmodified) {
                continue;
            }
            let src = u32::from(diff_line.source_line_number.0);
            map.entry((normalize_filename(filename), src))
                .or_insert(*input);
        }
        ReverseIndex {
            map,
            marker_width: diff.marker_width,
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Look up the diff line showing `source_line` (1-based) of `filename`.
    pub fn lookup(&self, filename: &str, source_line: u32) -> Option<InputLineNumber> {
        self.map
            .get(&(normalize_filename(filename), source_line))
            .copied()
    }

    /// Map a 0-based backend position in `filename` to a diff-buffer position.
    pub fn map_position(&self, filename: &str, pos: Position) -> Option<Position> {
        let input = self.lookup(filename, pos.line + 1)?;
        Some(Position {
            line: input_line_to_lsp_line(input),
            character: pos.character + self.marker_width,
        })
    }

    /// Map a range.  The start must be visible; if the end is not, the result
    /// collapses to an empty range at the mapped start.
    pub fn map_range(&self, filename: &str, range: Range) -> Option<Range> {
        let start = self.map_position(filename, range.start)?;
        let end = self.map_position(filename, range.end).unwrap_or(start);
        Some(Range { start, end })
    }
}

fn normalize_filename(name: &str) -> String {
    name.trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

/// Convert a backend `file://` URI to a path relative to `root`, matching the
/// filenames stored in `ParsedDiff`.  Returns `None` for URIs outside `root`.
pub fn relative_path_from_uri(root: &str, uri: &Url) -> Option<String> {
    if uri.scheme() != "file" {
        return None;
    }
    let path = uri.to_file_path().ok()?;
    let rel = path.strip_prefix(Path::new(root)).ok()?;
    let rel = rel.to_str()?;
    if rel.is_empty() {
        return None;
    }
    Some(rel.to_string())
}

/// Bundles what's needed to map backend locations into one diff buffer.
pub struct DiffLocationMapper<'a> {
    pub index: &'a ReverseIndex,
    pub root: &'a str,
    pub diff_uri: &'a Url,
}

impl DiffLocationMapper<'_> {
    pub fn map_location(&self, loc: &Location) -> Option<Location> {
        let rel = relative_path_from_uri(self.root, &loc.uri)?;
        let range = self.index.map_range(&rel, loc.range)?;
        Some(Location {
            uri: self.diff_uri.clone(),
            range,
        })
    }

    pub fn map_link(&self, link: &LocationLink) -> Option<LocationLink> {
        let rel = relative_path_from_uri(self.root, &link.target_uri)?;
        let target_selection_range = self.index.map_range(&rel, link.target_selection_range)?;
        let target_range = self
            .index
            .map_range(&rel, link.target_range)
            .unwrap_or(target_selection_range);
        Some(LocationLink {
            origin_selection_range: link.origin_selection_range,
            target_uri: self.diff_uri.clone(),
            target_range,
            target_selection_range,
        })
    }

    /// Apply `mode` to a list of locations (used for references).
    pub fn map_locations(&self, locs: Vec<Location>, mode: DiffLocationsMode) -> Vec<Location> {
        combine(locs, mode, |l| self.map_location(l))
    }

    pub fn map_links(
        &self,
        links: Vec<LocationLink>,
        mode: DiffLocationsMode,
    ) -> Vec<LocationLink> {
        combine(links, mode, |l| self.map_link(l))
    }

    /// Apply `mode` to a goto-(type-)definition response.  When nothing maps
    /// into the diff, the response is returned unchanged.
    pub fn map_definition_response(
        &self,
        resp: GotoDefinitionResponse,
        mode: DiffLocationsMode,
    ) -> GotoDefinitionResponse {
        if mode == DiffLocationsMode::Off {
            return resp;
        }
        match resp {
            GotoDefinitionResponse::Scalar(loc) => {
                if self.map_location(&loc).is_none() {
                    return GotoDefinitionResponse::Scalar(loc);
                }
                GotoDefinitionResponse::Array(self.map_locations(vec![loc], mode))
            }
            GotoDefinitionResponse::Array(locs) => {
                GotoDefinitionResponse::Array(self.map_locations(locs, mode))
            }
            GotoDefinitionResponse::Link(links) => {
                GotoDefinitionResponse::Link(self.map_links(links, mode))
            }
        }
    }
}

/// Shared ordering/dedup logic for locations and location links.
fn combine<T, F>(items: Vec<T>, mode: DiffLocationsMode, map: F) -> Vec<T>
where
    T: Clone + PartialEq,
    F: Fn(&T) -> Option<T>,
{
    let mut out: Vec<T> = Vec::with_capacity(items.len() * 2);
    let push = |out: &mut Vec<T>, item: T| {
        if !out.contains(&item) {
            out.push(item);
        }
    };
    match mode {
        DiffLocationsMode::Off => {
            for item in items {
                push(&mut out, item);
            }
        }
        DiffLocationsMode::Prefer => {
            for item in items {
                let mapped = map(&item).unwrap_or(item);
                push(&mut out, mapped);
            }
        }
        DiffLocationsMode::Both => {
            for item in &items {
                if let Some(mapped) = map(item) {
                    push(&mut out, mapped);
                }
            }
            for item in items {
                push(&mut out, item);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::utils::{DiffLine, Parsable, SourceLineNumber};
    use serde_json::json;

    const ROOT: &str = "/home/user/proj";

    fn diff_uri() -> Url {
        Url::parse("file:///tmp/diff_lsp_review").unwrap()
    }

    fn src_uri(rel: &str) -> Url {
        Url::from_file_path(format!("{}/{}", ROOT, rel)).unwrap()
    }

    fn loc(uri: Url, line: u32, start: u32, end: u32) -> Location {
        Location {
            uri,
            range: Range {
                start: Position::new(line, start),
                end: Position::new(line, end),
            },
        }
    }

    fn line(t: LineType, text: &str, src: u16) -> DiffLine {
        DiffLine {
            line_type: t,
            line: text.to_string(),
            source_line_number: SourceLineNumber(src),
        }
    }

    /// Small synthetic diff: two files, two hunks in a.rs, a removed line.
    fn synthetic() -> ParsedDiff {
        let mut d = ParsedDiff::default();
        d.marker_width = 1;
        d.filenames = vec!["src/a.rs".into(), "src/b.rs".into()];
        let entries = vec![
            (5, "src/a.rs", line(LineType::Unmodified, " fn a() {", 10)),
            (6, "src/a.rs", line(LineType::Removed, "-    old();", 11)),
            (7, "src/a.rs", line(LineType::Added, "+    new();", 11)),
            (8, "src/a.rs", line(LineType::Unmodified, " }", 12)),
            // second hunk
            (10, "src/a.rs", line(LineType::Added, "+fn z() {}", 40)),
            // second file, same source line numbers as a.rs
            (13, "src/b.rs", line(LineType::Unmodified, " fn b() {", 10)),
            (14, "src/b.rs", line(LineType::Added, "+    b();", 11)),
        ];
        for (input, f, dl) in entries {
            d.lines_map
                .insert(InputLineNumber(input), (f.to_string(), dl));
        }
        d
    }

    #[test]
    fn test_mode_parsing() {
        assert_eq!(
            DiffLocationsMode::from_initialization_options(None),
            DiffLocationsMode::Both
        );
        let o = json!({"diffLocations": "prefer"});
        assert_eq!(
            DiffLocationsMode::from_initialization_options(Some(&o)),
            DiffLocationsMode::Prefer
        );
        let o = json!({"diffLocations": "OFF"});
        assert_eq!(
            DiffLocationsMode::from_initialization_options(Some(&o)),
            DiffLocationsMode::Off
        );
        let o = json!({"diffLocations": false});
        assert_eq!(
            DiffLocationsMode::from_initialization_options(Some(&o)),
            DiffLocationsMode::Off
        );
        let o = json!({"diffLocations": "bogus"});
        assert_eq!(
            DiffLocationsMode::from_initialization_options(Some(&o)),
            DiffLocationsMode::Both
        );
        let o = json!({"somethingElse": 1});
        assert_eq!(
            DiffLocationsMode::from_initialization_options(Some(&o)),
            DiffLocationsMode::Both
        );
    }

    #[test]
    fn test_reverse_index_synthetic() {
        let idx = ReverseIndex::from_diff(&synthetic());
        // removed line excluded; 6 visible lines
        assert_eq!(idx.len(), 6);
        assert_eq!(idx.lookup("src/a.rs", 10), Some(InputLineNumber(5)));
        // source line 11 is the *added* line, not the removed one
        assert_eq!(idx.lookup("src/a.rs", 11), Some(InputLineNumber(7)));
        assert_eq!(idx.lookup("src/a.rs", 12), Some(InputLineNumber(8)));
        assert_eq!(idx.lookup("src/a.rs", 40), Some(InputLineNumber(10)));
        assert_eq!(idx.lookup("src/a.rs", 13), None);
        // multiple files don't collide
        assert_eq!(idx.lookup("src/b.rs", 10), Some(InputLineNumber(13)));
        assert_eq!(idx.lookup("src/b.rs", 11), Some(InputLineNumber(14)));
        assert_eq!(idx.lookup("src/c.rs", 10), None);
        // leading ./ or / tolerated
        assert_eq!(idx.lookup("./src/a.rs", 10), Some(InputLineNumber(5)));
    }

    #[test]
    fn test_reverse_index_removed_only_line_is_invisible() {
        let mut d = ParsedDiff::default();
        d.lines_map.insert(
            InputLineNumber(3),
            ("x.rs".into(), line(LineType::Removed, "-gone", 7)),
        );
        let idx = ReverseIndex::from_diff(&d);
        assert!(idx.is_empty());
        assert_eq!(idx.lookup("x.rs", 7), None);
    }

    #[test]
    fn test_reverse_index_first_occurrence_wins() {
        let mut d = ParsedDiff::default();
        d.lines_map.insert(
            InputLineNumber(30),
            ("x.rs".into(), line(LineType::Unmodified, " a", 7)),
        );
        d.lines_map.insert(
            InputLineNumber(4),
            ("x.rs".into(), line(LineType::Unmodified, " a", 7)),
        );
        let idx = ReverseIndex::from_diff(&d);
        assert_eq!(idx.lookup("x.rs", 7), Some(InputLineNumber(4)));
    }

    #[test]
    fn test_map_position_conventions() {
        let idx = ReverseIndex::from_diff(&synthetic());
        // backend line 10 (0-based) == source line 11 (1-based) == input line 7
        let p = idx.map_position("src/a.rs", Position::new(10, 4)).unwrap();
        assert_eq!(p, Position::new(7, 5)); // 1-char prefix
        assert!(idx.map_position("src/a.rs", Position::new(0, 0)).is_none());
    }

    #[test]
    fn test_map_range_end_not_visible_collapses() {
        let idx = ReverseIndex::from_diff(&synthetic());
        let r = Range {
            start: Position::new(9, 0),
            end: Position::new(30, 1),
        };
        let m = idx.map_range("src/a.rs", r).unwrap();
        assert_eq!(m.start, Position::new(5, 1));
        assert_eq!(m.end, m.start);

        let r = Range {
            start: Position::new(9, 0),
            end: Position::new(11, 1),
        };
        let m = idx.map_range("src/a.rs", r).unwrap();
        assert_eq!(m.end, Position::new(8, 2));
    }

    #[test]
    fn test_relative_path_from_uri() {
        let u = src_uri("src/a.rs");
        assert_eq!(relative_path_from_uri(ROOT, &u), Some("src/a.rs".into()));
        // root with a trailing slash also works
        assert_eq!(
            relative_path_from_uri("/home/user/proj/", &u),
            Some("src/a.rs".into())
        );
        // outside root (e.g. std lib / cargo registry)
        let other = Url::parse("file:///usr/lib/rust/src/vec.rs").unwrap();
        assert_eq!(relative_path_from_uri(ROOT, &other), None);
        // prefix that isn't a path component boundary
        let sibling = Url::parse("file:///home/user/project2/src/a.rs").unwrap();
        assert_eq!(relative_path_from_uri(ROOT, &sibling), None);
        let http = Url::parse("https://example.com/src/a.rs").unwrap();
        assert_eq!(relative_path_from_uri(ROOT, &http), None);
    }

    fn mapper_fixture() -> (ReverseIndex, Url) {
        (ReverseIndex::from_diff(&synthetic()), diff_uri())
    }

    #[test]
    fn test_map_locations_modes() {
        let (idx, duri) = mapper_fixture();
        let m = DiffLocationMapper {
            index: &idx,
            root: ROOT,
            diff_uri: &duri,
        };
        let visible = loc(src_uri("src/a.rs"), 10, 4, 7); // source 11
        let hidden = loc(src_uri("src/a.rs"), 100, 0, 3);
        let external = loc(Url::parse("file:///usr/lib/x.rs").unwrap(), 10, 0, 1);
        let input = vec![
            hidden.clone(),
            visible.clone(),
            external.clone(),
            visible.clone(),
        ];
        let diff_loc = loc(duri.clone(), 7, 5, 8);

        assert_eq!(
            m.map_locations(input.clone(), DiffLocationsMode::Both),
            vec![
                diff_loc.clone(),
                hidden.clone(),
                visible.clone(),
                external.clone()
            ]
        );
        assert_eq!(
            m.map_locations(input.clone(), DiffLocationsMode::Prefer),
            vec![hidden.clone(), diff_loc.clone(), external.clone()]
        );
        assert_eq!(
            m.map_locations(input.clone(), DiffLocationsMode::Off),
            vec![hidden, visible, external]
        );
    }

    #[test]
    fn test_map_definition_scalar() {
        let (idx, duri) = mapper_fixture();
        let m = DiffLocationMapper {
            index: &idx,
            root: ROOT,
            diff_uri: &duri,
        };
        let visible = loc(src_uri("src/b.rs"), 9, 3, 4); // source 10 -> input 13
        let resp = GotoDefinitionResponse::Scalar(visible.clone());
        match m.map_definition_response(resp.clone(), DiffLocationsMode::Both) {
            GotoDefinitionResponse::Array(v) => {
                assert_eq!(v, vec![loc(duri.clone(), 13, 4, 5), visible.clone()])
            }
            other => panic!("unexpected {:?}", other),
        }
        match m.map_definition_response(resp.clone(), DiffLocationsMode::Prefer) {
            GotoDefinitionResponse::Array(v) => assert_eq!(v, vec![loc(duri.clone(), 13, 4, 5)]),
            other => panic!("unexpected {:?}", other),
        }
        assert_eq!(
            m.map_definition_response(resp.clone(), DiffLocationsMode::Off),
            resp
        );
        // Unmappable scalar stays a scalar
        let hidden = GotoDefinitionResponse::Scalar(loc(src_uri("src/b.rs"), 500, 0, 1));
        assert_eq!(
            m.map_definition_response(hidden.clone(), DiffLocationsMode::Both),
            hidden
        );
    }

    #[test]
    fn test_map_definition_link() {
        let (idx, duri) = mapper_fixture();
        let m = DiffLocationMapper {
            index: &idx,
            root: ROOT,
            diff_uri: &duri,
        };
        let link = LocationLink {
            origin_selection_range: Some(Range::new(Position::new(1, 1), Position::new(1, 3))),
            target_uri: src_uri("src/a.rs"),
            // whole function: source lines 10..=12 (0-based 9..=11)
            target_range: Range::new(Position::new(9, 0), Position::new(11, 1)),
            // name on source line 10
            target_selection_range: Range::new(Position::new(9, 3), Position::new(9, 4)),
        };
        let resp = GotoDefinitionResponse::Link(vec![link.clone()]);
        match m.map_definition_response(resp, DiffLocationsMode::Both) {
            GotoDefinitionResponse::Link(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(v[0].target_uri, duri);
                assert_eq!(v[0].origin_selection_range, link.origin_selection_range);
                assert_eq!(
                    v[0].target_selection_range,
                    Range::new(Position::new(5, 4), Position::new(5, 5))
                );
                assert_eq!(
                    v[0].target_range,
                    Range::new(Position::new(5, 1), Position::new(8, 2))
                );
                assert_eq!(v[1], link);
            }
            other => panic!("unexpected {:?}", other),
        }
    }

    // ---- fixtures: pin conventions against real parser output ----

    fn fixture(name: &str) -> ParsedDiff {
        let raw = std::fs::read_to_string(format!("tests/data/{}", name)).unwrap();
        ParsedDiff::parse(&raw).unwrap()
    }

    /// Every visible line round-trips: diff line -> source -> diff line.
    fn assert_round_trip(diff: &ParsedDiff) {
        let idx = ReverseIndex::from_diff(diff);
        assert!(!idx.is_empty());
        for (input, (filename, dl)) in diff.lines_map.iter() {
            if dl.line_type == LineType::Removed {
                continue;
            }
            let found = idx
                .lookup(filename, u32::from(dl.source_line_number.0))
                .unwrap();
            let (f2, dl2) = &diff.lines_map[&found];
            assert_eq!(f2, filename);
            assert_eq!(dl2.source_line_number, dl.source_line_number);
            assert!(found <= *input);
        }
    }

    #[test]
    fn test_fixture_round_trips() {
        for name in [
            "go_diff.magit_status",
            "rust_diff.magit_status",
            "big_rust_diff.magit_status",
            "go_diff.code_review",
            "full_go_diff.code_review",
            "go_diff.code_review_server",
        ] {
            assert_round_trip(&fixture(name));
        }
    }

    /// A click in the diff, sent to the backend through main's forward
    /// mapping (`SourceMap::source_position`), comes back through
    /// `map_position` to the same diff line and column.
    fn assert_position_round_trip(diff: &ParsedDiff) {
        let idx = ReverseIndex::from_diff(diff);
        let mut checked = 0;
        for (input, (filename, dl)) in diff.lines_map.iter() {
            if dl.line_type == LineType::Removed {
                continue;
            }
            // Skip lines shadowed by an earlier occurrence of the same source
            // line (magit staged + unstaged sections).
            if idx.lookup(filename, u32::from(dl.source_line_number.0)) != Some(*input) {
                continue;
            }
            // Only lines of a supported file type reach a backend.
            let Some(src_map) = diff.map_diff_line_to_src(input.0) else {
                continue;
            };
            let clicked = Position::new(u32::from(input.0), diff.marker_width + 3);
            let backend = src_map.source_position(clicked);
            assert_eq!(backend.character, 3);
            assert_eq!(idx.map_position(filename, backend), Some(clicked));
            checked += 1;
        }
        assert!(checked > 0);
    }

    #[test]
    fn test_fixture_position_round_trips_with_source_position() {
        for (name, width) in [
            ("go_diff.magit_status", 1),
            ("rust_diff.magit_status", 1),
            ("go_diff.code_review", 1),
            ("go_diff.code_review_server", 2),
            ("go_diff.bun_client", 2),
            ("rust_diff.bun_client", 1),
        ] {
            let diff = fixture(name);
            assert_eq!(diff.marker_width, width, "{}", name);
            assert_position_round_trip(&diff);
        }
    }

    #[test]
    fn test_bun_client_definition_round_trip() {
        // tests/test_positions.rs: diff line 26 "+ \treturn Mul(x, 3)", the `x`
        // at diff column 14 is demo.go 0-based (13, 12). A backend location
        // there maps back to the same diff position.
        let diff = fixture("go_diff.bun_client");
        let idx = ReverseIndex::from_diff(&diff);
        let fwd = diff
            .map_diff_line_to_src(26)
            .unwrap()
            .source_position(Position::new(26, 14));
        assert_eq!(fwd, Position::new(13, 12));
        assert_eq!(
            idx.map_position("demo.go", fwd),
            Some(Position::new(26, 14))
        );
    }

    #[test]
    fn test_fixture_magit_go_conventions() {
        // go_diff.magit_status: "@@ -11,9 +11,10 @@" on file line 11, so the
        // first hunk line (file line 12) is source line 11.
        let diff = fixture("go_diff.magit_status");
        assert_eq!(diff.marker_width, 1);
        let idx = ReverseIndex::from_diff(&diff);
        assert_eq!(idx.lookup("main.go", 11), Some(InputLineNumber(12)));
        // "+var logger *log.Logger" is file line 15 -> source line 14
        assert_eq!(idx.lookup("main.go", 14), Some(InputLineNumber(15)));
        // " func init() {" (file 18) is source 17; the removed lines 19,20 that
        // follow are skipped, so source 18 is file line 21.
        assert_eq!(idx.lookup("main.go", 17), Some(InputLineNumber(18)));
        assert_eq!(idx.lookup("main.go", 18), Some(InputLineNumber(21)));
        // Second hunk "@@ -34,7 +35,8 @@" on file line 24.
        assert_eq!(idx.lookup("main.go", 35), Some(InputLineNumber(25)));
        // the removed line at file 28 is skipped: file 29 "+logger = log.New(f, \"new\"..."
        assert_eq!(idx.lookup("main.go", 38), Some(InputLineNumber(29)));

        // Forward map agrees: server uses position.line as the key, and the
        // reverse mapping emits that same LSP line.
        let fwd = diff.map_diff_line_to_src(29).unwrap();
        assert_eq!(fwd.source_line, SourceLineNumber(38));
        // backend 0-based line 37 == source line 38
        let p = idx.map_position("main.go", Position::new(37, 2)).unwrap();
        assert_eq!(p, Position::new(29, 3));
        assert_eq!(
            diff.map_diff_line_to_src(p.line.try_into().unwrap())
                .unwrap()
                .source_line,
            SourceLineNumber(38)
        );
    }

    #[test]
    fn test_fixture_code_review_server_conventions() {
        let diff = fixture("go_diff.code_review_server");
        // code-review-server lines look like "+ \tWorkflowType ..." (2-char prefix)
        assert_eq!(diff.marker_width, 2);
        let idx = ReverseIndex::from_diff(&diff);
        // README.md "@@ -33,6 +33,7 @@" on file line 39; "+ ReleaseCommandCheck"
        // is file line 43 -> source line 36.
        assert_eq!(idx.lookup("README.md", 33), Some(InputLineNumber(40)));
        assert_eq!(idx.lookup("README.md", 36), Some(InputLineNumber(43)));
        let p = idx.map_position("README.md", Position::new(35, 0)).unwrap();
        assert_eq!(p, Position::new(43, 2));

        // Lines inside review comment boxes are never targets.
        let idx_values: Vec<InputLineNumber> = idx.map.values().copied().collect();
        for n in 101..=110u16 {
            assert!(!idx_values.contains(&InputLineNumber(n)));
        }
        // Multiple files are indexed.
        assert!(idx.map.keys().any(|(f, _)| f == "config.go"));
        assert!(idx
            .map
            .keys()
            .any(|(f, _)| f == "git_tools/github_notifications.go"));
    }

    #[test]
    fn test_fixture_end_to_end_location() {
        let diff = fixture("go_diff.code_review_server");
        let idx = ReverseIndex::from_diff(&diff);
        let duri = diff_uri();
        let root = "/home/chris/code-review-server/";
        let m = DiffLocationMapper {
            index: &idx,
            root,
            diff_uri: &duri,
        };
        let backend = Location {
            uri: Url::from_file_path("/home/chris/code-review-server/README.md").unwrap(),
            range: Range::new(Position::new(35, 0), Position::new(35, 19)),
        };
        let out = m.map_locations(vec![backend.clone()], DiffLocationsMode::Both);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].uri, duri);
        assert_eq!(
            out[0].range,
            Range::new(Position::new(43, 2), Position::new(43, 21))
        );
        assert_eq!(out[1], backend);
    }
}
