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
    pub filenames: Vec<String>, // relative path, i.e. /src/client.rs
    lines_map: HashMap<InputLineNumber, (String, DiffLine)>,
    total_lines: usize,
}

/// Code-review buffers interleave review comments with the diff lines.
const CODE_REVIEW_FLAVOR: DiffFlavor = DiffFlavor {
    review_comments: true,
};

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

impl CodeReviewDiff {
    fn self_parse(source: &str) -> Self {
        let walked = walk_hunks(source, &CODE_REVIEW_FLAVOR);
        CodeReviewDiff {
            headers: walked.headers,
            filenames: walked.filenames,
            lines_map: walked.lines_map,
            total_lines: walked.total_lines,
        }
    }
}
