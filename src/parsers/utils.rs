use crate::parsers::{code_review::CodeReviewDiff, magit::MagitDiff};
use crate::SupportedFileType;
use chrono::{DateTime, Utc};
use log::info;
use regex::Regex;
use std::collections::HashMap;

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

/// Parses a unified-diff hunk header like `@@ -10,3 +12,4 @@` into
/// `(old_start, old_lines, new_start, new_lines)`.  Returns `None` for
/// malformed headers or numbers that don't fit in a `u32`.
pub fn parse_header(header: &str) -> Option<(u32, u32, u32, u32)> {
    // Complex regex to support when the code is added at the start of a file, and we don't have all 4 values
    let re = Regex::new(r"@@ -(\d+)(,(\d+))? \+(\d+)(,(\d+))? @@").unwrap();
    let caps = re.captures(header)?;
    let num = |idx: usize| -> Option<u32> {
        caps.get(idx)
            .map_or("1", |m| m.as_str())
            .parse::<u32>()
            .ok()
    };
    Some((num(1)?, num(3)?, num(4)?, num(6)?))
}

/// Reprepresents the data of a line in a diff.
#[derive(Debug)]
pub struct SourceMap {
    pub file_name: String,
    pub source_line: SourceLineNumber,
    pub file_type: SupportedFileType,
    pub source_line_type: LineType,
    pub source_line_text: String,
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
    // fn map_diff_line_to_src(&self, line_num: u32) -> Option<SourceMap>;
}

/// InputLineNumber refers to a line number on the tempfile input that was initial parsed
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputLineNumber(pub u32);

impl InputLineNumber {
    pub fn new(value: u32) -> Self {
        InputLineNumber(value)
    }
}

/// SourceLineNumber refers to a line number on the source file that the diff is referring to.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceLineNumber(pub u32);

impl SourceLineNumber {
    pub fn new(value: u32) -> Self {
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
}

impl ParsedDiff {
    pub fn map_diff_line_to_src(&self, line_num: u32) -> Option<SourceMap> {
        if let Some((filename, diff_line)) = self.lines_map.get(&InputLineNumber::new(line_num)) {
            if let Some(file_type) = SupportedFileType::from_filename(filename.to_string()) {
                return Some(SourceMap {
                    file_name: filename.clone(),
                    source_line: diff_line.source_line_number,
                    file_type,
                    source_line_type: diff_line.line_type,
                    source_line_text: diff_line.line.clone(),
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
    line.starts_with("modified ")
        || line.starts_with("new file ")
        || line.starts_with("deleted ")
        || line.starts_with("diff --git ")
}
