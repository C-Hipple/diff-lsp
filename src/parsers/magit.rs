use std::collections::HashMap;
use std::str::FromStr;

use chrono::Utc;
use log::info;
use regex::Regex;

use crate::parsers::utils::*;

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
                total_lines: 0,
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
