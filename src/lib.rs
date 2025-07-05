use chrono::{DateTime, Utc};
use log::info;
use regex::Regex;
use std::collections::HashMap;
use url::Url;

use std::str::FromStr;
use strum_macros::{EnumIter, EnumString};

pub mod client;
pub mod server;
pub mod utils;

#[derive(Debug, Hash, PartialEq, std::cmp::Eq, Copy, Clone, EnumIter)]
pub enum SupportedFileType {
    Rust,
    Go,
    Python,
}

impl SupportedFileType {
    pub fn from_extension(extension: String) -> Option<SupportedFileType> {
        match extension.as_str() {
            "rs" => Some(SupportedFileType::Rust),
            "go" => Some(SupportedFileType::Go),
            "py" => Some(SupportedFileType::Python),
            _ => None,
        }
    }

    pub fn from_filename(filename: String) -> Option<SupportedFileType> {
        filename
            .rsplit_once('.')
            .map(|(_name, extension)| extension.to_string())
            .and_then(|extension| SupportedFileType::from_extension(extension))
    }
}

pub fn get_lsp_for_file_type(file_type: SupportedFileType) -> Option<String> {
    // TBH no idea why this is an Option type.  Maybe I should check that these
    // actually exist on the machine running lsp?
    match file_type {
        SupportedFileType::Rust => Some("rust-analyzer".to_string()),
        SupportedFileType::Go => Some("gopls".to_string()),
        SupportedFileType::Python => Some("pylsp".to_string()),
    }
}

pub fn uri_from_relative_filename(project_root: String, rel_filename: &str) -> Url {
    // since teh diff has a relative path like /src/lib.rs and not a full path.
    Url::from_file_path(project_root + "/" + rel_filename).unwrap()
}

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
            _ => LineType::Unmodified,
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
    let re = Regex::new(r"@@ -(\d+),(\d+) \+(\d+),(\d+) @@").unwrap();
    if let Some(caps) = re.captures(header) {
        return Some((
            caps[1].parse::<u16>().unwrap(),
            caps[2].parse::<u16>().unwrap(),
            caps[3].parse::<u16>().unwrap(),
            caps[4].parse::<u16>().unwrap(),
        ));
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
}

impl ParsedDiff {
    pub fn map_diff_line_to_src(&self, line_num: u16) -> Option<SourceMap> {
        if let Some((filename, diff_line)) = self.lines_map.get(&InputLineNumber::new(line_num)) {
            if let Some(file_type) = SupportedFileType::from_filename(filename.to_string()) {
                return Some(SourceMap {
                    file_name: filename.clone(),
                    source_line: diff_line.source_line_number,
                    file_type: file_type,
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
        if source.contains("Type: code-review") {
            CodeReviewDiff::parse(source)
        } else if source.contains("Type: magit-status") {
            MagitDiff::parse(source)
        } else {
            info!("Warning! Unable to determine buffer type to parse!");
            None
        }
    }
}

#[allow(dead_code)]
#[derive(Default, Debug, Clone)]
pub struct MagitDiff {
    pub headers: HashMap<DiffHeader, String>,
    pub filenames: Vec<String>, // relative path, i.e. /src/client.rs
    pub lines_map: HashMap<InputLineNumber, (String, DiffLine)>,
    src: String,
}

impl Parsable for MagitDiff {
    fn parse(source: &str) -> Option<ParsedDiff> {
        if let Some(magit_diff) = MagitDiff::self_parse(source) {
            return Some(ParsedDiff {
                headers: magit_diff.headers,
                filenames: magit_diff.filenames,
                lines_map: magit_diff.lines_map,
                parsed_at: Utc::now(),
            });
        }
        None
    }
}

#[allow(dead_code)]
impl MagitDiff {
    fn self_parse(source: &str) -> Option<Self> {
        let mut diff = MagitDiff::default();

        let mut found_headers = false;
        let mut current_filename = "";
        let mut building_hunk = false;
        let mut start_new: u16 = 0;
        let mut at_source_line: u16 = 0;

        for (i, line) in source.lines().enumerate() {
            if !found_headers {
                let re = Regex::new(r"(\w+):\s+(.+)").unwrap();
                if let Some(caps) = re.captures(line) {
                    info!("{}", line);
                    match DiffHeader::from_str(&caps[1]) {
                        Ok(header) => {
                            diff.headers.insert(header, caps[2].to_string());
                        }
                        Err(_) => continue,
                    }
                } else {
                    found_headers = true;
                }
            } else {
                // found headers, moving onto hunks
                if line.starts_with("modified") {
                    current_filename = line.split_whitespace().nth(1).unwrap();
                    info!("Current filename when parsing: {:?}", current_filename);
                    diff.filenames.push(current_filename.to_string());
                }
                if line.starts_with("@@") && !building_hunk {
                    building_hunk = true;
                    info!("({:?}) Parsing Header `{}`", i, line);
                    start_new = parse_header(line).unwrap().2;
                    at_source_line = 0;
                    continue;
                }
                if (line.starts_with("@@") && building_hunk) || line.starts_with("Recent commits") {
                    if line.starts_with("@@") {
                        info!("B: ({:?}) Setting Header: `{}`", i, line);
                        start_new = parse_header(line).unwrap().2;
                        at_source_line = 0;
                        continue;
                    }
                    if line.starts_with("Recent commits") {
                        break;
                    }
                }

                if building_hunk && !line.starts_with("modified ") {
                    let line_type = LineType::from_line(line);
                    let diff_line = DiffLine {
                        line_type: line_type,
                        line: line.to_string(),
                        source_line_number: SourceLineNumber(start_new + at_source_line),
                    };

                    // the i + 1 is because i is 0 index, but file lines are 1 index.
                    diff.lines_map.insert(
                        InputLineNumber::new((i + 1).try_into().unwrap()),
                        (current_filename.to_string(), diff_line.clone()),
                    );

                    info!(
                        "C: ({:?})Adding line @ {:?} `{}`",
                        i + 1,
                        diff_line.source_line_number.0,
                        line
                    );

                    if matches!(line_type, LineType::Added | LineType::Unmodified) {
                        at_source_line += 1;
                    }

                    continue;
                }
            }
        }
        Some(diff)
    }
}

/// CodeReviewDiffs are the output of the code-review emacs package
/// https//www.github.com/C-Hipple/code-review
#[allow(dead_code)]
#[derive(Default, Debug, Clone)]
pub struct CodeReviewDiff {
    pub headers: HashMap<DiffHeader, String>,
    // pub hunks: Vec<Hunk>,
    pub filenames: Vec<String>, // relative path, i.e. /src/client.rs
    lines_map: HashMap<InputLineNumber, (String, DiffLine)>,
    src: String,
}

impl Parsable for CodeReviewDiff {
    fn parse(source: &str) -> Option<ParsedDiff> {
        if let Some(cr_diff) = CodeReviewDiff::self_parse(source) {
            return Some(ParsedDiff {
                headers: cr_diff.headers,
                filenames: cr_diff.filenames,
                lines_map: cr_diff.lines_map,
                parsed_at: Utc::now(),
            });
        }
        None
    }
}

impl CodeReviewDiff {
    fn self_parse(source: &str) -> Option<Self> {
        let mut diff = CodeReviewDiff::default();

        let mut found_headers = false;
        let mut current_filename = "";
        let mut building_hunk = false;
        let mut start_new: u16 = 0; // TODO new variable name
        let mut at_source_line: u16 = 0;
        let mut in_review = false;
        let mut line_num;

        for (i, line) in source.lines().enumerate() {
            if !found_headers {
                let re = Regex::new(r"(\w+):\s+(.+)").unwrap();
                if let Some(caps) = re.captures(line) {
                    info!("{}", line);
                    match DiffHeader::from_str(&caps[1]) {
                        Ok(header) => {
                            diff.headers.insert(header, caps[2].to_string());
                        }
                        Err(_) => continue,
                    }
                } else {
                    found_headers = true;
                }
            } else {
                // found headers, moving onto hunks
                line_num = i + 1;
                if line.starts_with("modified") {
                    current_filename = line.split_whitespace().nth(1).unwrap();
                    info!("Current filename when parsing: {:?}", current_filename);
                    diff.filenames.push(current_filename.to_string());
                }
                if line.starts_with("@@") && !building_hunk {
                    building_hunk = true;
                    info!("({:?}) Parsing Header `{}`", line_num, line);
                    start_new = parse_header(line).unwrap().2;
                    at_source_line = 0;
                    continue;
                }
                if (line.starts_with("@@") && building_hunk) || line.starts_with("Recent commits") {
                    if line.starts_with("@@") {
                        info!("B: ({:?}) Setting Header: `{}`", line_num, line);
                        start_new = parse_header(line).unwrap().2;
                        at_source_line = 0;
                        continue;
                    }
                    if line.starts_with("Recent commits") {
                        break;
                    }
                }

                if building_hunk
                    && (line.starts_with("Reviewed by") || line.starts_with("Comment by"))
                {
                    info!("D: ({:?}) Review Start : {}", line_num, line);
                    in_review = true;
                    continue;
                }
                if in_review && line.starts_with("-------") {
                    info!("D: ({:?}) Review End: {}", line_num, line);
                    in_review = false;
                    continue;
                }

                if in_review {
                    info!("D: ({:?}) Review Line: {}", line_num, line);
                    continue;
                }

                if building_hunk && !line.starts_with("modified ") {
                    let line_type = LineType::from_line(line);
                    let diff_line = DiffLine {
                        line_type: line_type,
                        line: line.to_string(),
                        source_line_number: SourceLineNumber(start_new + at_source_line),
                    };

                    // the  line_num is because line_num is 0 index, but file lines are 1 index.
                    diff.lines_map.insert(
                        InputLineNumber::new((line_num).try_into().unwrap()),
                        (current_filename.to_string(), diff_line.clone()),
                    );

                    info!(
                        "C: ({:?}) Adding line @ {:?} `{}`",
                        line_num, diff_line.source_line_number.0, line
                    );

                    if matches!(line_type, LineType::Added | LineType::Unmodified) {
                        at_source_line += 1;
                    }

                    continue;
                }
            }
        }
        Some(diff)
    }
}
