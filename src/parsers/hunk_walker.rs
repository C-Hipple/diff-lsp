//! Shared hunk-walking logic for the diff-buffer parsers.
//!
//! Both magit-status buffers and code-review buffers have the same overall
//! shape: a block of `Key: value` headers, followed by file sections, each made
//! of `@@ ... @@` hunks.  The formats differ only in whether review comment
//! blocks can be interleaved with diff lines, so each parser supplies a
//! [`DiffFlavor`] describing that and calls [`walk_hunks`].

use std::collections::HashMap;
use std::str::FromStr;

use log::{trace, warn};

use crate::parsers::utils::*;

/// Describes the format-specific parts of a diff buffer.
pub(crate) struct DiffFlavor {
    /// Whether review comment blocks (old `Reviewed by`/`Comment by` ...
    /// `-------`, and new `┌─ REVIEW COMMENT` ... `└`) are skipped.
    pub review_comments: bool,
}

/// The output of walking a diff buffer.
#[derive(Default)]
pub(crate) struct WalkedDiff {
    pub headers: HashMap<DiffHeader, String>,
    pub filenames: Vec<String>,
    pub lines_map: HashMap<InputLineNumber, (String, DiffLine)>,
    pub total_lines: usize,
}

/// The relative filename named by a file-section header (`modified   x`,
/// `new file   x`, `deleted   x`, `diff --git a/x b/x`), or `None` if `line`
/// isn't one.  A header with no filename (`modified`) yields `""`.
pub(crate) fn file_header_filename(line: &str) -> Option<&str> {
    if !is_file_header(line) {
        return None;
    }
    let Some(last) = line.split_whitespace().last() else {
        return Some("");
    };
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

fn is_review_comment_start(line: &str) -> bool {
    // Old format (code-review) or new box-drawing format (code-review-server),
    // which starts with "    ┌─ REVIEW COMMENT ─────"
    line.starts_with("Reviewed by")
        || line.starts_with("Comment by")
        || line.trim_start().starts_with("┌─ REVIEW COMMENT")
}

fn is_review_comment_end(line: &str) -> bool {
    // Old format ends with a dashed line, new format with "    └─────────"
    line.starts_with("-------") || line.trim_start().starts_with("└")
}

pub(crate) fn walk_hunks(source: &str, flavor: &DiffFlavor) -> WalkedDiff {
    let mut diff = WalkedDiff::default();

    let mut found_headers = false;
    let mut current_filename = "";
    let mut building_hunk = false;
    let mut hunk_start: u32 = 0; // first source line of the current hunk
    let mut at_source_line: u32 = 0; // offset into the current hunk
    let mut in_review = false;

    for (i, line) in source.lines().enumerate() {
        if !found_headers {
            if let Some(caps) = HEADER_LINE_RE.captures(line) {
                trace!("{}", line);
                if let Ok(header) = DiffHeader::from_str(&caps[1]) {
                    diff.headers.insert(header, caps[2].to_string());
                }
                continue;
            }
            found_headers = true;
        }

        // Editor lines are 1-indexed, enumerate is 0-indexed.
        let Ok(line_num) = u32::try_from(i + 1) else {
            warn!("Diff has more lines than fit in a u32, stopping parse");
            break;
        };

        if let Some(filename) = file_header_filename(line) {
            current_filename = filename;
            trace!("Current filename when parsing: {:?}", current_filename);
            diff.filenames.push(current_filename.to_string());
            building_hunk = false;
            continue;
        }

        if line.starts_with("@@") {
            trace!("({:?}) Parsing Header `{}`", line_num, line);
            match parse_header(line) {
                Some((_, _, new_start, _)) => {
                    building_hunk = true;
                    hunk_start = new_start;
                    at_source_line = 0;
                }
                None => {
                    // Don't map the following lines against a stale hunk.
                    warn!("({:?}) Malformed hunk header `{}`", line_num, line);
                    building_hunk = false;
                }
            }
            continue;
        }

        if line.starts_with("Recent commits") {
            break;
        }

        if flavor.review_comments {
            if building_hunk && is_review_comment_start(line) {
                trace!("({:?}) Review Start: {}", line_num, line);
                in_review = true;
                continue;
            }

            if in_review {
                if is_review_comment_end(line) {
                    trace!("({:?}) Review End: {}", line_num, line);
                    in_review = false;
                } else {
                    // Skip all lines within comments (both formats)
                    trace!("({:?}) Review Line: {}", line_num, line);
                }
                continue;
            }
        }

        if building_hunk {
            let line_type = LineType::from_line(line);
            let source_line_number = SourceLineNumber(hunk_start.saturating_add(at_source_line));
            trace!(
                "({:?}) Adding line @ {:?} `{}`",
                line_num,
                source_line_number.0,
                line
            );
            diff.lines_map.insert(
                InputLineNumber::new(line_num),
                (
                    current_filename.to_string(),
                    DiffLine {
                        line_type,
                        line: line.to_string(),
                        source_line_number,
                    },
                ),
            );

            if matches!(line_type, LineType::Added | LineType::Unmodified) {
                at_source_line = at_source_line.saturating_add(1);
            }
        }
    }
    diff.total_lines = source.lines().count();
    diff
}
