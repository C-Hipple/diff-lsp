use log::info;
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
    pub fn from_line(line: &str) -> Self {
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
    pub line_type: LineType,
    pub line: String,
    pub pos_in_hunk: u16,
    pub source_line_number: SourceLineNumber,
}

// #[allow(dead_code)]
// #[derive(Default, Clone, Debug, PartialEq)]
// pub struct Hunk {
//     pub filename: String, // relative path, i.e. /src/client.rs
//     pub start_old: u16,
//     pub change_length_old: u16,
//     pub start_new: u16, // consider s/new/modified
//     pub change_length_new: u16,
//     pub changes: Vec<DiffLine>,
//     pub diff_location: u16, // Where the raw hunk starts (the @@ line) in the plain text diff.
// }

// impl Hunk {
//     pub fn parse(
//         header: &str,
//         lines: Vec<DiffLine>,
//         filename: String,
//         diff_location: u16,
//     ) -> Option<Hunk> {
//         // NOTE: the last line of the last hunk is an empty line before the "recent commits" line...
//         // unsure if it's a problem or not
//         let re = Regex::new(r"@@ -(\d+),(\d+) \+(\d+),(\d+) @@").unwrap();
//         if let Some(caps) = parse_header(header){
//             return Some(Hunk {
//                 filename: filename,
//                 diff_location: diff_location,
//                 start_old: caps.0,
//                 change_length_old: caps.1,
//                 start_new: caps.2,
//                 change_length_new: caps.3,
//                 changes: lines,
//             });
//         } else {
//             return None;
//         }
//     }

//     pub fn diff_length(&self) -> u16 {
//         self.changes.len() as u16
//     }

//     pub fn diff_end(&self) -> u16 {
//         self.diff_location + self.diff_length()
//     }

//     pub fn file_type(&self) -> String {
//         // better not get any files with "." in them
//         info!("Filename: {}", self.filename);
//         self.filename.split_once('.').unwrap().1.to_string()
//     }
// }

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

#[derive(Debug)]
pub struct SourceMap {
    // Return type when you translate a
    pub file_name: String,
    pub source_line: SourceLineNumber,
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

    // for when I remove the ParsedDiff type
    // fn map_diff_line_to_src(&self, line_num: u16) -> Option<SourceMap>;
}

// InputLineNumber refers to a line number on the tempfile input that was initial parsed
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputLineNumber(pub u16);

impl InputLineNumber {
    pub fn new(value: u16) -> Self {
        InputLineNumber(value)
    }
}

// SourceLineNumber refers to a line number on the source file that the diff is referring to.
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
}

impl ParsedDiff {
    pub fn map_diff_line_to_src(&self, line_num: u16) -> Option<SourceMap> {
        // TODO: consider using an and_then chain? not sure if applicable
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

    //     if let Some(hunk) = self.get_hunk_by_diff_line_number(line_num) {
    //         if let Some(supported_file_type) = SupportedFileType::from_extension(hunk.file_type()) {
    //             let pos_in_hunk: usize = (line_num - hunk.diff_location).into();
    //             println!("map: pos_in_hunk: {:?}", pos_in_hunk);
    //             println!("map: diff_location: {:?}", hunk.diff_location);
    //             println!("map: start_new: {:?}", hunk.start_new);
    //             return Some(SourceMap {
    //                 file_name: hunk.filename,
    //                 source_line: line_num - hunk.diff_location + hunk.start_new - 1, // LSP is 0 index.  Editors are 1 index.  Subtract 1 so they match
    //                 // source_line: line_num - hunk.diff_location + hunk.start_new, // trying without 0 index?
    //                 file_type: supported_file_type,
    //                 source_line_type: hunk.changes[pos_in_hunk].line_type,
    //                 source_line_text: hunk.changes[pos_in_hunk].line.clone(),
    //             });
    //         }
    //     }
    //     None
    // }

    // fn get_hunk_by_diff_line_number(&self, line_num: u16) -> Option<Hunk> {
    //     for hunk in &self.hunks {
    //         if line_num > hunk.diff_location && line_num <= hunk.diff_end() {
    //             return Some(hunk.clone()); // is this going to shoot me in the foot?
    //         }
    //     }
    //     None
    // }
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
        let mut hunk_start = 0;
        let mut start_new: u16 = 0;

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
                    diff.filenames.push(current_filename.to_string());
                }
                if line.starts_with("@@") && !building_hunk {
                    building_hunk = true;
                    hunk_start = i + 1; // diff_location doesn't include the @@ line
                    println!("({:?}) Parsing Header `{}`", i, line);
                    start_new = parse_header(line).unwrap().2;
                    continue;
                }
                if (line.starts_with("@@") && building_hunk) || line.starts_with("Recent commits") {
                    hunk_start = i + 1; // diff_location does not include the @@ line

                    if line.starts_with("@@") {
                        println!("B: ({:?}) Setting Header: `{}`", i, line);
                        continue;
                    }
                    if line.starts_with("Recent commits") {
                        break;
                    }
                }

                if building_hunk && !line.starts_with("modified ") {
                    diff.lines_map.insert(
                        InputLineNumber::new((i + 1).try_into().unwrap()),
                        (
                            current_filename.to_string(),
                            DiffLine {
                                line_type: LineType::from_line(line),
                                line: line.to_string(),
                                pos_in_hunk: (i - hunk_start) as u16,
                                source_line_number: SourceLineNumber(
                                    start_new + ((i - hunk_start) as u16),
                                ),
                            },
                        ),
                    );
                    println!("C: ({:?})Adding line `{}`", i, line);
                    continue;
                }
            }
        }
        Some(diff)
    }
}

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
            });
        }
        None
    }
}

impl CodeReviewDiff {
    fn self_parse(source: &str) -> Option<Self> {
        println!("{:?}", source);
        return None;
    }
    //     println!("Doing code review parse");
    //     let mut diff = CodeReviewDiff::default();

    //     let mut found_headers = false;
    //     let mut current_filename = "";
    //     let mut building_hunk = false;
    //     let mut hunk_start = 0;
    //     let mut hunk_header: &str = "";
    //     let mut pos_in_hunk = 0;
    //     let mut lines_map:  HashMap<u16, (String, DiffLine)>;

    //     let mut in_comment = false;

    //     for (i, line) in source.lines().enumerate() {
    //         if !found_headers {
    //             let re = Regex::new(r"(\w+):\s+(.+)").unwrap();
    //             if let Some(caps) = re.captures(line) {
    //                 println!("{}", line);
    //                 match DiffHeader::from_str(&caps[1]) {
    //                     Ok(header) => {
    //                         diff.headers.insert(header, caps[2].to_string());
    //                     }
    //                     Err(_) => continue,
    //                 }
    //             } else {
    //                 found_headers = true;
    //             }
    //         } else {
    //             // found headers, moving onto hunks
    //             if line.starts_with("modified") && !building_hunk {
    //                 current_filename = line.split_whitespace().nth(1).unwrap();
    //                 eprintln!("Current filename when parsing: {:?}", current_filename);
    //                 continue;
    //             }

    //             if line.starts_with("@@") && !building_hunk {
    //                 building_hunk = true;
    //                 pos_in_hunk = 0;
    //                 hunk_header = line;
    //                 hunk_start = i + 1; // diff_location doesn't include the @@ line
    //                 println!("{}starting hunk with line `{}`", hunk_start, line);
    //                 continue;
    //             }

    //             if line.starts_with("Reviewed by") {
    //                 in_comment = true;
    //                 continue;
    //             }

    //             if in_comment && line.starts_with("-------") {
    //                 in_comment = false;
    //                 continue;
    //             }

    //             if in_comment {
    //                 println!("Comment line: {}", line);
    //                 continue;
    //             }

    //             // TODO: new files, deleted files
    //             if (line.starts_with("@@") || line.starts_with("modified ")) && building_hunk {
    //                 if line.starts_with("@@") {
    //                     hunk_header = line;
    //                 }

    //                 eprintln!("header: {:?}", hunk_header);
    //                 eprintln!("lines: {:?}", hunk_lines);
    //                 if hunk_lines.len() > 0 {
    //                     diff.hunks.push(
    //                         Hunk::parse(
    //                             hunk_header,
    //                             hunk_lines,
    //                             current_filename.to_string(),
    //                             hunk_start as u16,
    //                         )
    //                         .unwrap(),
    //                     )
    //                 }

    //                 // eprintln!("That hunk is at diff location: {:?}", hunk.diff_location);

    //                 hunk_lines = vec![];
    //                 hunk_start = i + 1; // diff_location does not include the @@ line

    //                 if line.starts_with("@@") {
    //                     println!("B: ({:?}) Setting Header: `{}`", i, line);
    //                     hunk_header = line;
    //                     pos_in_hunk = 0;
    //                     continue;
    //                 }

    //                 if line.starts_with("modified ") {
    //                     current_filename = line.split_whitespace().nth(1).unwrap();
    //                     println!("Updating the filename to be: {}", current_filename);
    //                 }

    //                 if line.starts_with("Recent commits") {
    //                     break;
    //                 }
    //             }

    //             if building_hunk && !line.starts_with("modified ") {
    //                 pos_in_hunk += 1;
    //                 lines_map.insert(i, v)
    //                 hunk_lines.push(DiffLine {
    //                     line_type: LineType::from_line(line),
    //                     line: line.to_string(),
    //                     pos_in_hunk: pos_in_hunk as u16, // i is wrong, we need another tracker
    //                 });
    //                 println!("C: ({:?} - {:?})Adding line `{}`", i, pos_in_hunk, line,);
    //                 continue;
    //             }
    //         }
    //     }

    //     if hunk_lines.len() > 0 {
    //         diff.hunks.push(
    //             Hunk::parse(
    //                 hunk_header,
    //                 hunk_lines,
    //                 current_filename.to_string(),
    //                 hunk_start as u16,
    //             )
    //             .unwrap(),
    //         )
    //     }
    //     if !diff.headers.is_empty() && diff.hunks.len() > 0 {
    //         Some(diff)
    //     } else {
    //         None
    //     }
    // }
}
