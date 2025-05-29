use log::{debug, info};
use regex::Regex;
use std::collections::HashMap;
use url::Url;

// use log::info;

use std::str::FromStr;
use strum_macros::EnumString;

pub mod client;
pub mod server;
pub mod utils;

#[derive(Debug, Hash, PartialEq, std::cmp::Eq, Copy, Clone)]
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
        SupportedFileType::from_extension(filename.split_once('.').unwrap().1.to_string())
    }
}

pub fn get_lsp_for_file_type(file_type: SupportedFileType) -> Option<String> {
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
    fn from_line(line: String) -> Self {
        match line.chars().next() {
            // Could technically be bugger if it's a diff and the first char is 1 of these and it's unmodified
            Some('+') => LineType::Added,
            Some('-') => LineType::Removed,
            _ => LineType::Unmodified,
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub struct DiffLine {
    line_type: LineType,
    line: String,
}

#[allow(dead_code)]
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Hunk {
    pub filename: String, // relative path, i.e. /src/client.rs
    pub start_old: u16,
    pub change_length_old: u16,
    pub start_new: u16, // consider s/new/modified
    pub change_length_new: u16,
    pub changes: Vec<DiffLine>,
    pub diff_location: u16, // Where the raw hunk starts (the @@ line) in the plain text diff.
}

impl Hunk {
    pub fn parse(source: &str, filename: String) -> Option<Hunk> {
        debug!("Parsing the hunk from lines for the file {}:", filename);
        eprintln!("Parsing the hunk from lines for the file {}:", filename);
        debug!("{}", source);
        println!("{}", source);
        let mut found_header = false;
        let mut wip = Hunk::default();
        wip.filename = filename;
        for line in source.lines() {
            let re = Regex::new(r"@@ -(\d+),(\d+) \+(\d+),(\d+) @@").unwrap();
            if let Some(caps) = re.captures(line) {
                found_header = true;
                wip.start_old = caps[1].parse::<u16>().unwrap();
                wip.change_length_old = caps[2].parse::<u16>().unwrap();
                wip.start_new = caps[3].parse::<u16>().unwrap();
                wip.change_length_new = caps[4].parse::<u16>().unwrap();
            } else {
                if found_header {
                    wip.changes.push(DiffLine {
                        line_type: LineType::from_line(line.to_string()),
                        line: line.to_string(),
                    })
                }
            }
        }
        // for li in &wip.changes {
        //     println!("Line: {0:?}, {1:?}", li.line_type, li.line);

        // }
        //TODO: More other handling
        //info!("parsed filetype: {} - {}", wip.file_type(), wip.filename);  // we don't have file_type yet
        Some(wip)
    }

    pub fn diff_length(&self) -> u16 {
        self.changes.len() as u16
    }

    pub fn diff_end(&self) -> u16 {
        self.diff_location + self.diff_length()
    }

    pub fn file_type(&self) -> String {
        // better not get any files with "." in them
        info!("Filename: {}", self.filename);
        self.filename.split_once('.').unwrap().1.to_string()
    }
}

#[derive(Debug)]
pub struct SourceMap {
    // Return type when you translate a
    pub file_name: String,
    pub source_line: u16,
    pub file_type: SupportedFileType,
    pub source_line_type: LineType,
    pub source_line_text: String,
}

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
}

#[allow(dead_code)]
#[derive(Default, Debug, Clone)]
pub struct ParsedDiff {
    pub headers: HashMap<DiffHeader, String>,
    pub hunks: Vec<Hunk>,
}

impl ParsedDiff {
    pub fn map_diff_line_to_src(&self, line_num: u16) -> Option<SourceMap> {
        if let Some(hunk) = self.get_hunk_by_diff_line_number(line_num) {
            if let Some(supported_file_type) = SupportedFileType::from_extension(hunk.file_type()) {
                let pos_in_hunk: usize = (line_num - hunk.diff_location).into();
                info!("map: pos_in_hunk: {:?}", pos_in_hunk);
                return Some(SourceMap {
                    file_name: hunk.filename,
                    // source_line: line_num - hunk.diff_location + hunk.start_new - 1, // LSP is 0 index.  Editors are 1 index.  Subtract 1 so they match
                    source_line: line_num - hunk.diff_location + hunk.start_new, // trying without 0 index?

                    file_type: supported_file_type,
                    source_line_type: hunk.changes[pos_in_hunk].line_type,
                    source_line_text: hunk.changes[pos_in_hunk].line.clone(),
                });
            }
        }
        None
    }

    fn get_hunk_by_diff_line_number(&self, line_num: u16) -> Option<Hunk> {
        for hunk in &self.hunks {
            if line_num > hunk.diff_location && line_num <= hunk.diff_end() {
                return Some(hunk.clone()); // is this going to shoot me in the foot?
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
    pub hunks: Vec<Hunk>,
    src: String,
}

impl Parsable for MagitDiff {
    fn parse(source: &str) -> Option<ParsedDiff> {
        if let Some(magit_diff) = MagitDiff::self_parse(source) {
            return Some(ParsedDiff {
                headers: magit_diff.headers,
                hunks: magit_diff.hunks,
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
        let mut hunk_lines: Vec<&str> = vec![];
        let mut hunk_start = 0;

        for (i, line) in source.lines().enumerate() {
            if !found_headers {
                let re = Regex::new(r"(\w+):\s+(.+)").unwrap();
                if let Some(caps) = re.captures(line) {
                    println!("{}", line);
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
                }
                if line.starts_with("@@") && !building_hunk {
                    building_hunk = true;
                    hunk_start = i + 1; // diff_location doesn't include the @@ line
                    println!("({:?})Adding line `{}`", i, line);
                    hunk_lines.push(line);
                    continue;
                }
                if (line.starts_with("@@") && building_hunk) || line.starts_with("Recent commits") {
                    let mut hunk =
                        Hunk::parse(hunk_lines.join("\n").as_str(), current_filename.to_string())
                            .unwrap();
                    hunk.diff_location = hunk_start as u16;
                    diff.hunks.push(hunk);
                    hunk_lines = vec![];
                    hunk_start = i + 1; // diff_location does not include the @@ line
                    if line.starts_with("@@") {
                        println!("B: ({:?})Adding line `{}`", i, line);
                        hunk_lines.push(line);
                        continue;
                    }
                    if line.starts_with("Recent commits") {
                        break;
                    }
                }

                if building_hunk && !line.starts_with("modified ") {
                    hunk_lines.push(line);
                    println!("C: ({:?})Adding line `{}`", i, line);
                    continue;
                }
            }
        }

        if hunk_lines.len() > 0 {
            let mut hunk =
                Hunk::parse(hunk_lines.join("\n").as_str(), current_filename.to_string()).unwrap();
            hunk.diff_location = hunk_start as u16;
            diff.hunks.push(hunk);
        }
        if !diff.headers.is_empty() && diff.hunks.len() > 0 {
            Some(diff)
        } else {
            None
        }
    }
}

#[allow(dead_code)]
#[derive(Default, Debug, Clone)]
pub struct CodeReviewDiff {
    pub headers: HashMap<DiffHeader, String>,
    pub hunks: Vec<Hunk>,
    src: String,
}

impl Parsable for CodeReviewDiff {
    fn parse(source: &str) -> Option<ParsedDiff> {
        if let Some(cr_diff) = CodeReviewDiff::self_parse(source) {
            return Some(ParsedDiff {
                headers: cr_diff.headers,
                hunks: cr_diff.hunks,
            });
        }
        None
    }
}

impl CodeReviewDiff {
    fn self_parse(source: &str) -> Option<Self> {
        println!("Doing code review parse");
        let mut diff = CodeReviewDiff::default();

        let mut found_headers = false;
        let mut current_filename = "";
        let mut building_hunk = false;
        let mut hunk_lines: Vec<&str> = vec![];
        let mut hunk_start = 0;
        let mut in_comment = false;

        for (i, line) in source.lines().enumerate() {
            if !found_headers {
                let re = Regex::new(r"(\w+):\s+(.+)").unwrap();
                if let Some(caps) = re.captures(line) {
                    println!("{}", line);
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
                if line.starts_with("modified") && !building_hunk {
                    current_filename = line.split_whitespace().nth(1).unwrap();
                    eprintln!("Current filename when parsing: {:?}", current_filename);
                    continue;
                }

                if line.starts_with("@@") && !building_hunk {
                    building_hunk = true;
                    hunk_start = i + 1; // diff_location doesn't include the @@ line
                    println!("starting hunk with line `{}`", line);
                    hunk_lines.push(line);
                    continue;
                }

                if line.starts_with("Reviewed by") {
                    in_comment = true;
                    continue;
                }

                if in_comment && line.starts_with("-------") {
                    in_comment = false;
                    continue;
                }

                if in_comment {
                    println!("Comment line: {}", line);
                }

                // TODO: new files, deleted files
                if (line.starts_with("@@") || line.starts_with("modified ")) && building_hunk {
                    if hunk_lines.len() > 0 {
                        let mut hunk = Hunk::parse(
                            hunk_lines.join("\n").as_str(),
                            current_filename.to_string(),
                        )
                        .unwrap();
                        hunk.diff_location = hunk_start as u16;
                        eprintln!("That hunk is at diff location: {:?}", hunk.diff_location);
                        diff.hunks.push(hunk);
                        hunk_lines = vec![];
                        hunk_start = i + 1; // diff_location does not include the @@ line
                    }

                    if line.starts_with("@@") {
                        println!("B: ({:?})Adding line `{}`", i, line);
                        hunk_lines.push(line);
                        continue;
                    }

                    if line.starts_with("modified ") {
                        current_filename = line.split_whitespace().nth(1).unwrap();
                        println!("Updating the filename to be: {}", current_filename);
                    }

                    if line.starts_with("Recent commits") {
                        break;
                    }
                }

                if building_hunk && !line.starts_with("modified ") {
                    hunk_lines.push(line);
                    println!("C: ({:?})Adding line `{}`", i, line);
                    continue;
                }
            }
        }

        if hunk_lines.len() > 0 {
            let mut hunk =
                Hunk::parse(hunk_lines.join("\n").as_str(), current_filename.to_string()).unwrap();
            hunk.diff_location = hunk_start as u16;
            diff.hunks.push(hunk);
        }
        if !diff.headers.is_empty() && diff.hunks.len() > 0 {
            Some(diff)
        } else {
            None
        }
    }
}
