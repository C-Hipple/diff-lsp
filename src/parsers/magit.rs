use std::collections::HashMap;

use chrono::Utc;
use log::info;
use std::time::Instant;

use crate::parsers::hunk_walker::{walk_hunks, DiffFlavor};
use crate::parsers::utils::*;

#[allow(dead_code)]
#[derive(Default, Debug, Clone)]
pub struct MagitDiff {
    pub headers: HashMap<DiffHeader, String>,
    pub filenames: Vec<String>, // relative path, i.e. /src/client.rs
    pub lines_map: HashMap<InputLineNumber, (String, DiffLine)>,
}

/// magit-status buffers have no review comments.
const MAGIT_FLAVOR: DiffFlavor = DiffFlavor {
    review_comments: false,
};

impl Parsable for MagitDiff {
    fn parse(source: &str) -> Option<ParsedDiff> {
        let start = Instant::now();
        let magit_diff = MagitDiff::self_parse(source);
        info!(
            "Parsed MagitDiff: {} files, {} mapped lines in {:?}",
            magit_diff.filenames.len(),
            magit_diff.lines_map.len(),
            start.elapsed()
        );
        Some(ParsedDiff {
            headers: magit_diff.headers,
            filenames: magit_diff.filenames,
            lines_map: magit_diff.lines_map,
            parsed_at: Utc::now(),
            total_lines: 0,
            // magit shows the source right after the marker, and its
            // hunks are often indented throughout, which would read as
            // code-review-server's "+ " layout.
            marker_width: 1,
        })
    }
}

impl MagitDiff {
    fn self_parse(source: &str) -> Self {
        let walked = walk_hunks(source, &MAGIT_FLAVOR);
        MagitDiff {
            headers: walked.headers,
            filenames: walked.filenames,
            lines_map: walked.lines_map,
        }
    }
}
