use std::collections::HashMap;
use std::str::FromStr;

use chrono::Utc;
// use log::println;
use regex::Regex;

use crate::parsers::utils::*;

/// CodeReviewDiffs are the output of the code-review emacs package
/// https//www.github.com/C-Hipple/code-review
#[allow(dead_code)]
#[derive(Default, Debug, Clone)]
pub struct CodeReviewDiff {
    pub headers: HashMap<DiffHeader, String>,
    // pub hunks: Vec<Hunk>,
    pub filenames: Vec<String>, // relative path, i.e. /src/client.rs
    lines_map: HashMap<InputLineNumber, (String, DiffLine)>,
    total_lines: usize,
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
                total_lines: cr_diff.total_lines,
            });
        }
        None
    }
}

impl CodeReviewDiff {
    fn self_parse(source: &str) -> Option<Self> {
        let mut diff = CodeReviewDiff::default();

        let mut found_headers = false;
        let current_filename = "";
        let mut building_hunk = false;
        let mut start_new: u16 = 0; // TODO new variable name
        let mut at_source_line: u16 = 0;
        let mut in_review = false;
        let mut line_num;

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
                line_num = i + 1;
                if is_file_header(line) {
                    current_filename = line.split_whitespace().nth(1).unwrap();
                    info!("Current filename when parsing: {:?}", current_filename);
                    diff.filenames.push(current_filename.to_string());
                }
                if line.starts_with("@@") && !building_hunk {
                    building_hunk = true;
                    println!("({:?}) Parsing Header `{}`", line_num, line);
                    start_new = parse_header(line).unwrap().2;
                    at_source_line = 0;
                    continue;
                }
                if (line.starts_with("@@") && building_hunk) || line.starts_with("Recent commits") {
                    if line.starts_with("@@") {
                        println!("B: ({:?}) Setting Header: `{}`", line_num, line);
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
                    println!("D: ({:?}) Review Start : {}", line_num, line);
                    in_review = true;
                    continue;
                }
                if in_review && line.starts_with("-------") {
                    println!("D: ({:?}) Review End: {}", line_num, line);
                    in_review = false;
                    continue;
                }

                if in_review {
                    println!("D: ({:?}) Review Line: {}", line_num, line);
                    continue;
                }

                if building_hunk && !is_file_header(line) {
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

                    println!(
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
        diff.total_lines = source.lines().count();
        Some(diff)
    }
}
