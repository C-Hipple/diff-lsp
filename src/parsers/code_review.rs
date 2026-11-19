use std::collections::HashMap;

use chrono::Utc;
use log::info;
use std::time::Instant;

use crate::parsers::hunk_walker::{walk_hunks, DiffFlavor};
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
        let start = Instant::now();
        let cr_diff = CodeReviewDiff::self_parse(source);
        let marker_width =
            infer_marker_width(cr_diff.lines_map.values().map(|(_, l)| l.line.as_str()));
        info!(
            "Parsed CodeReviewDiff: {} files, {} mapped lines in {:?}",
            cr_diff.filenames.len(),
            cr_diff.lines_map.len(),
            start.elapsed()
        );
        Some(ParsedDiff {
            headers: cr_diff.headers,
            filenames: cr_diff.filenames,
            lines_map: cr_diff.lines_map,
            parsed_at: Utc::now(),
            total_lines: cr_diff.total_lines,
            marker_width,
        })
    }
}

/// Recognizes `modified`, `new file`, `deleted` and `diff --git a/x b/x`
/// file headers (with variable whitespace).
fn code_review_file_header(line: &str) -> Option<&str> {
    if !is_file_header(line) {
        return None;
    }
    let last = line.split_whitespace().last()?;
    if line.starts_with("diff --git") {
        Some(
            last.strip_prefix("b/")
                .or_else(|| last.strip_prefix("a/"))
                .unwrap_or(last),
        )
    } else {
        Some(last)
    }
}

const CODE_REVIEW_FLAVOR: DiffFlavor = DiffFlavor {
    file_header: code_review_file_header,
    review_comments: true,
};

impl CodeReviewDiff {
    fn self_parse(source: &str) -> Self {
        let walked = walk_hunks(source, &CODE_REVIEW_FLAVOR);
        CodeReviewDiff {
            headers: walked.headers,
            filenames: walked.filenames,
            lines_map: walked.lines_map,
            total_lines: walked.total_lines,
            src: String::new(),
        }
    }
}
