use crate::parsers::{code_review::CodeReviewDiff, magit::MagitDiff};
use crate::SupportedFileType;
use chrono::{DateTime, Utc};
use log::info;
use regex::Regex;
use std::collections::HashMap;
use tower_lsp::lsp_types::Position;

use strum_macros::EnumString;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineType {
    Added,
    Removed,
    Unmodified,
}

impl LineType {
    pub fn from_line(line: &str) -> Self {
        match line.chars().next() {
            // Could technically be bugger if it's a diff and the first char
            // is 1 of these and it's unmodified
            Some('+') => LineType::Added,
            Some('-') => LineType::Removed,
            _ => {
                // support delta wash
                // 70 ⋮    │-     let _ = initialize_logger().unwrap();
                // parses as unmodified
                if line.contains("│-") {
                    LineType::Removed
                } else if line.contains("│+") {
                    LineType::Added
                } else {
                    LineType::Unmodified
                }
            }
        }
    }
}

/// A single line in a diff.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub struct DiffLine {
    pub line_type: LineType,
    pub line: String,
    pub source_line_number: SourceLineNumber,
}

pub fn parse_header(header: &str) -> Option<(u16, u16, u16, u16)> {
    // Complex regex to support when the code is added at the start of a file, and we don't have all 4 values
    let re = Regex::new(r"@@ -(\d+)(,(\d+))? \+(\d+)(,(\d+))? @@").unwrap();
    if let Some(caps) = re.captures(header) {
        let old_start = caps[1].parse::<u16>().unwrap();
        let old_lines = caps
            .get(3)
            .map_or("1", |m| m.as_str())
            .parse::<u16>()
            .unwrap();
        let new_start = caps[4].parse::<u16>().unwrap();
        let new_lines = caps
            .get(6)
            .map_or("1", |m| m.as_str())
            .parse::<u16>()
            .unwrap();
        return Some((old_start, old_lines, new_start, new_lines));
    }
    None
}

/// Reprepresents the data of a line in a diff.
#[derive(Debug)]
pub struct SourceMap {
    pub file_name: String,
    pub source_line: SourceLineNumber,
    pub file_type: SupportedFileType,
    pub source_line_type: LineType,
    pub source_line_text: String,
    /// Columns of the diff line taken by its +/-/space marker (see
    /// ParsedDiff::marker_width).
    pub marker_width: u32,
}

impl SourceMap {
    /// The source-file position a position on this diff line points at. The
    /// line comes 0-indexed, as backends count, and the column loses the
    /// marker: clients send the column they see in the diff buffer.
    pub fn source_position(&self, diff_position: Position) -> Position {
        Position {
            line: u32::from(self.source_line.0).saturating_sub(1),
            character: diff_position.character.saturating_sub(self.marker_width),
        }
    }
}

/// How many columns at the start of each diff line are the +/-/space marker.
/// git and magit put the source right after it ("+foo"). code-review-server
/// renders a marker and a space ("+ foo"), blank lines included ("+ "), so
/// every line of its hunks has a space second and is at least two long,
/// while a git diff's blank lines are the bare marker. Empty lines are the
/// separators some buffers put between hunks, not diff lines, and say nothing.
pub fn infer_marker_width<'a>(lines: impl Iterator<Item = &'a str>) -> u32 {
    let mut saw_line = false;
    for line in lines.filter(|l| !l.is_empty()) {
        if line.as_bytes().get(1) != Some(&b' ') {
            return 1;
        }
        saw_line = true;
    }
    if saw_line {
        2
    } else {
        1
    }
}

/// The various information headers at the top of diffs which say what the diff
/// was from, how it was generated.
#[derive(EnumString, Hash, PartialEq, std::cmp::Eq, Debug, Clone)]
pub enum DiffHeader {
    Project,
    Root,
    Buffer,
    Type,
    Head,
    Merge,
    Push,
    Draft,
    State,
}

pub trait Parsable {
    fn parse(source: &str) -> Option<ParsedDiff>;

    // for when I remove the ParsedDiff type
    // fn map_diff_line_to_src(&self, line_num: u16) -> Option<SourceMap>;
}

/// InputLineNumber refers to a line number on the tempfile input that was initial parsed
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputLineNumber(pub u16);

impl InputLineNumber {
    pub fn new(value: u16) -> Self {
        InputLineNumber(value)
    }
}

/// SourceLineNumber refers to a line number on the source file that the diff is referring to.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceLineNumber(pub u16);

impl SourceLineNumber {
    pub fn new(value: u16) -> Self {
        SourceLineNumber(value)
    }
}

#[allow(dead_code)]
#[derive(Default, Debug, Clone)]
pub struct ParsedDiff {
    pub headers: HashMap<DiffHeader, String>,
    pub filenames: Vec<String>, // relative path, i.e. /src/client.rs
    // maps the line of the actual source file (after teh diff was applied to FileName, DiffLine tuple)
    pub lines_map: HashMap<InputLineNumber, (String, DiffLine)>,
    pub parsed_at: DateTime<Utc>, // used for debugging my server
    pub total_lines: usize,       // temp deubgger
    /// Columns before the source text on each diff line; see
    /// infer_marker_width.
    pub marker_width: u32,
}

impl ParsedDiff {
    pub fn map_diff_line_to_src(&self, line_num: u16) -> Option<SourceMap> {
        if let Some((filename, diff_line)) = self.lines_map.get(&InputLineNumber::new(line_num)) {
            if let Some(file_type) = SupportedFileType::from_filename(filename.to_string()) {
                return Some(SourceMap {
                    file_name: filename.clone(),
                    source_line: diff_line.source_line_number,
                    file_type,
                    source_line_type: diff_line.line_type,
                    source_line_text: diff_line.line.clone(),
                    marker_width: self.marker_width,
                });
            }
        }
        None
    }
}

impl Parsable for ParsedDiff {
    fn parse(source: &str) -> Option<ParsedDiff> {
        if source.contains("Type: code-review") || source.contains("Type: my-code-review") {
            // Supports both old (code-review) and new (my-code-review / code-review-server) formats
            CodeReviewDiff::parse(source)
        } else if source.contains("Type: magit-status") {
            MagitDiff::parse(source)
        } else {
            info!("Warning! Unable to determine buffer type to parse!");
            None
        }
    }
}

pub fn is_file_header(line: &str) -> bool {
    // Handle variable whitespace - new code-review-server format uses more spaces
    return line.starts_with("modified ")
        || line.starts_with("new file ")
        || line.starts_with("deleted ")
        || line.starts_with("diff --git ");
}
