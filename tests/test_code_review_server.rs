#[cfg(test)]
mod tests {
    use diff_lsp::parsers::code_review::CodeReviewDiff;
    use diff_lsp::parsers::utils::{DiffHeader, LineType, Parsable, ParsedDiff};
    use std::fs;

    #[test]
    fn test_parse_code_review_server_diff() {
        let raw_diff = fs::read_to_string("tests/data/go_diff.code_review_server").unwrap();
        let diff = ParsedDiff::parse(&raw_diff).unwrap();

        // Verify it correctly identifies as my-code-review type
        assert_eq!(
            diff.headers.get(&DiffHeader::Type),
            Some(&"my-code-review".to_string())
        );

        // Verify project header
        assert_eq!(
            diff.headers.get(&DiffHeader::Project),
            Some(&"* Review C-Hipple/gtdbot #9 *".to_string())
        );

        // Verify buffer name
        assert_eq!(
            diff.headers.get(&DiffHeader::Buffer),
            Some(&"code-review-server".to_string())
        );
    }

    #[test]
    fn test_parse_code_review_server_headers() {
        let raw_diff = fs::read_to_string("tests/data/go_diff.code_review_server").unwrap();
        let diff = ParsedDiff::parse(&raw_diff).unwrap();

        // Verify State is parsed
        assert_eq!(
            diff.headers.get(&DiffHeader::State),
            Some(&"closed".to_string())
        );

        // Verify Root is parsed
        assert_eq!(
            diff.headers.get(&DiffHeader::Root),
            Some(&"/home/chris/code-review-server/".to_string())
        );
    }

    #[test]
    fn test_parse_code_review_server_files() {
        let raw_diff = fs::read_to_string("tests/data/go_diff.code_review_server").unwrap();
        let diff = ParsedDiff::parse(&raw_diff).unwrap();

        // Check that all 9 modified files are detected
        assert!(diff.filenames.contains(&"README.md".to_string()));
        assert!(diff.filenames.contains(&"config.go".to_string()));
        assert!(diff
            .filenames
            .contains(&"git_tools/github_notifications.go".to_string()));
        assert!(diff.filenames.contains(&"main.go".to_string()));
        assert!(diff.filenames.contains(&"org/org.go".to_string()));
        assert!(diff.filenames.contains(&"org/org_parser.go".to_string()));
        assert!(diff.filenames.contains(&"workflows/logic.go".to_string()));
        assert!(diff.filenames.contains(&"workflows/manager.go".to_string()));
        assert!(diff
            .filenames
            .contains(&"workflows/workflows.go".to_string()));

        // Should have exactly 9 files
        assert_eq!(diff.filenames.len(), 9);
    }

    #[test]
    fn test_parse_code_review_server_with_comments() {
        let raw_diff = fs::read_to_string("tests/data/go_diff.code_review_server").unwrap();
        let diff = ParsedDiff::parse(&raw_diff).unwrap();

        // Test line mapping in README.md (no comments in this file)
        // Line 40 in the diff should map to line 34 in the source
        let mapped = diff.map_diff_line_to_src(40);
        if let Some(map) = mapped {
            assert_eq!(map.file_name, "README.md".to_string());
            assert_eq!(map.source_line_type, LineType::Unmodified);
        }
    }

    #[test]
    fn test_source_map_around_review_comments() {
        let raw_diff = fs::read_to_string("tests/data/go_diff.code_review_server").unwrap();
        let diff = ParsedDiff::parse(&raw_diff).unwrap();

        // Test around the first review comment in config.go
        // Line 88 has the comment box start "┌─ REVIEW COMMENT ─────────────────"
        // This should not map to source
        let mapped = diff.map_diff_line_to_src(81);
        assert!(
            mapped.is_none(),
            "Comment box lines should not map to source"
        );

        // Line before the comment (line 87) should be the actual code line
        let mapped_before = diff.map_diff_line_to_src(88);
        if let Some(map) = mapped_before {
            assert_eq!(map.file_name, "config.go".to_string());
        }
    }

    #[test]
    fn test_line_mapping_config_go() {
        let raw_diff = fs::read_to_string("tests/data/go_diff.code_review_server").unwrap();
        let diff = ParsedDiff::parse(&raw_diff).unwrap();

        // Test a specific line in config.go that's not near a comment
        // Line 78 should be in config.go
        let mapped = diff.map_diff_line_to_src(78);
        if let Some(map) = mapped {
            assert_eq!(map.file_name, "config.go".to_string());
        }
    }

    #[test]
    fn test_line_types_in_hunks() {
        let raw_diff = fs::read_to_string("tests/data/go_diff.code_review_server").unwrap();
        let diff = ParsedDiff::parse(&raw_diff).unwrap();

        // Find a removed line (should start with -)
        // Line 90 in diff has "- \tWorkflowType string"
        let mapped = diff.map_diff_line_to_src(90);
        if let Some(map) = mapped {
            assert_eq!(map.source_line_type, LineType::Removed);
        }

        // Find an added line (should start with +)
        // Line 111 has "+ \tWorkflowType        string"
        let mapped = diff.map_diff_line_to_src(111);
        if let Some(map) = mapped {
            assert_eq!(map.source_line_type, LineType::Added);
        }
    }

    #[test]
    fn test_multiple_files_line_mapping() {
        let raw_diff = fs::read_to_string("tests/data/go_diff.code_review_server").unwrap();
        let diff = ParsedDiff::parse(&raw_diff).unwrap();

        // Test that we can map lines from different files correctly
        // First file is README.md
        let readme_line = diff.map_diff_line_to_src(40);
        if let Some(map) = readme_line {
            assert_eq!(map.file_name, "README.md".to_string());
        }

        // Later lines should be in config.go (starts around line 75)
        let config_line = diff.map_diff_line_to_src(100);
        if let Some(map) = config_line {
            assert_eq!(map.file_name, "config.go".to_string());
        }

        // Even later should be in git_tools/github_notifications.go (around line 277)
        let git_tools_line = diff.map_diff_line_to_src(284);
        if let Some(map) = git_tools_line {
            assert_eq!(
                map.file_name,
                "git_tools/github_notifications.go".to_string()
            );
        }
    }

    #[test]
    fn test_comment_tree_handling() {
        let raw_diff = fs::read_to_string("tests/data/go_diff.code_review_server").unwrap();
        let diff = ParsedDiff::parse(&raw_diff).unwrap();

        // Test around comment with replies in config.go
        // Lines 101-110 contain a comment with a reply
        for line_num in 101..=110 {
            let mapped = diff.map_diff_line_to_src(line_num);
            // These should be None because they're within the comment box
            assert!(
                mapped.is_none(),
                "Line {} is in a comment box and should not map to source",
                line_num
            );
        }

        // Line after the comment should map to actual code
        let mapped_after = diff.map_diff_line_to_src(111);
        if let Some(map) = mapped_after {
            assert_eq!(map.file_name, "config.go".to_string());
            assert_eq!(map.source_line_type, LineType::Added);
        }
    }

    #[test]
    fn test_file_header_format() {
        let raw_diff = fs::read_to_string("tests/data/go_diff.code_review_server").unwrap();
        let diff = CodeReviewDiff::parse(&raw_diff).unwrap();

        // The file uses "modified     README.md" format (with spaces)
        // Verify this is parsed correctly
        assert!(diff.filenames.contains(&"README.md".to_string()));
        assert!(diff.filenames.contains(&"config.go".to_string()));
        assert!(diff.filenames.contains(&"main.go".to_string()));
    }

    #[test]
    fn test_nested_comment_replies() {
        let raw_diff = fs::read_to_string("tests/data/go_diff.code_review_server").unwrap();
        let diff = ParsedDiff::parse(&raw_diff).unwrap();

        // Test the main.go section which has a comment with a reply
        // around lines 296-305
        for line_num in 296..=305 {
            if let Some(map) = diff.map_diff_line_to_src(line_num) {
                // If it maps, it should be to main.go
                if !map.source_line_text.is_empty() {
                    assert_eq!(map.file_name, "main.go".to_string());
                }
            }
        }
    }

    #[test]
    fn test_workflows_file_mapping() {
        let raw_diff = fs::read_to_string("tests/data/go_diff.code_review_server").unwrap();
        let diff = ParsedDiff::parse(&raw_diff).unwrap();

        // Test lines in workflows/workflows.go
        // Should start around line 551 in the diff
        let mapped = diff.map_diff_line_to_src(560);
        if let Some(map) = mapped {
            assert_eq!(map.file_name, "workflows/workflows.go".to_string());
        }

        // Test further down in the same file
        let mapped = diff.map_diff_line_to_src(620);
        if let Some(map) = mapped {
            assert_eq!(map.file_name, "workflows/workflows.go".to_string());
        }
    }
}

/// code-review-server's comment threads and plugin annotations since
/// C-Hipple/code-review-server#343: one card per thread or per anchored line,
/// a `┌─`/`├─` header line per comment or annotation and `└─` to close it.
///
/// ```text
///     ┌─ bob · 2 days ago
///     │ Should this back off?
///     │ 👍 alice
///     ├─ you · pending
///     │ Let's cap the total wait at a minute.
///     └─
///     ┌─ warning · Security Check
///     │ this line looks wrong
///     └─
/// ```
#[cfg(test)]
mod card_layout {
    use diff_lsp::parsers::utils::{InputLineNumber, LineType, Parsable, ParsedDiff};
    use std::fs;

    const OLD_LAYOUT: &str = "tests/data/go_diff.code_review_server";
    /// The same buffer as `OLD_LAYOUT`, its threads redrawn as cards, plus
    /// annotation blocks, a file-level comment and the rarer thread shapes.
    const CARD_LAYOUT: &str = "tests/data/go_diff.code_review_server_cards";

    fn parse(path: &str) -> (String, ParsedDiff) {
        let raw = fs::read_to_string(path).unwrap();
        let diff = ParsedDiff::parse(&raw).unwrap();
        (raw, diff)
    }

    /// The diff lines a buffer maps, in buffer order but without their
    /// buffer line numbers: what the blocks drawn between them must not change.
    fn mapped_lines(diff: &ParsedDiff) -> Vec<(String, u32, LineType, String)> {
        let mut lines: Vec<_> = diff.lines_map.iter().collect();
        lines.sort_by_key(|(input_line, _)| **input_line);
        lines
            .into_iter()
            .map(|(_, (file, line))| {
                (
                    file.clone(),
                    line.source_line_number.0,
                    line.line_type,
                    line.line.clone(),
                )
            })
            .collect()
    }

    /// The 1-based buffer line of the first line equal to `text` after `after`.
    fn line_of(raw: &str, text: &str, after: u32) -> u32 {
        raw.lines()
            .enumerate()
            .skip(after as usize)
            .find(|(_, line)| *line == text)
            .map(|(i, _)| i as u32 + 1)
            .unwrap_or_else(|| panic!("no line `{}` after line {}", text, after))
    }

    fn assert_maps(diff: &ParsedDiff, line: u32, file: &str, source_line: u32, kind: LineType) {
        let (mapped_file, diff_line) = diff
            .lines_map
            .get(&InputLineNumber::new(line))
            .unwrap_or_else(|| panic!("line {} isn't mapped", line));
        assert_eq!(mapped_file, file, "file of line {}", line);
        assert_eq!(
            diff_line.source_line_number.0, source_line,
            "source line of line {} `{}`",
            line, diff_line.line
        );
        assert_eq!(diff_line.line_type, kind, "type of line {}", line);
    }

    #[test]
    fn test_cards_map_like_the_old_layout() {
        let (_, old) = parse(OLD_LAYOUT);
        let (_, cards) = parse(CARD_LAYOUT);
        assert_eq!(cards.filenames, old.filenames);
        assert_eq!(mapped_lines(&cards), mapped_lines(&old));
        assert_eq!(cards.marker_width, 2);
    }

    #[test]
    fn test_no_block_line_maps() {
        let (raw, cards) = parse(CARD_LAYOUT);
        let mut in_block = false;
        let mut block_lines = 0;
        for (i, line) in raw.lines().enumerate() {
            let line_num = i as u32 + 1;
            if line.starts_with("    ┌─ ") {
                in_block = true;
            }
            if in_block {
                block_lines += 1;
                assert!(
                    cards.map_diff_line_to_src(line_num).is_none()
                        && !cards
                            .lines_map
                            .contains_key(&InputLineNumber::new(line_num)),
                    "block line {} `{}` should not map",
                    line_num,
                    line
                );
            }
            if line == "    └─" {
                in_block = false;
            }
        }
        assert!(block_lines > 50, "the fixture should have blocks to skip");
    }

    #[test]
    fn test_lines_after_threads_keep_their_source_lines() {
        let (raw, cards) = parse(CARD_LAYOUT);
        // config.go's hunk starts at source line 17: "Workflows", "}", "" are
        // 17-19, then a thread and an annotation block hang off line 19.
        let hunk = line_of(&raw, "@@ -17,18 +17,20 @@ type Config struct {", 0);
        let comment = line_of(
            &raw,
            "+ // This struct implements all possible values a workflow can define, then they're written as-needed.",
            hunk,
        );
        assert_maps(&cards, comment, "config.go", 20, LineType::Added);
        // Past eleven removed lines and a thread with a reply.
        let field = line_of(&raw, "+ \tWorkflowType        string", hunk);
        assert_maps(&cards, field, "config.go", 22, LineType::Added);
        // Past the thread whose body has reactions, blank lines and lines
        // that look like diff lines.
        let name = line_of(&raw, "+ \tName                string", hunk);
        assert_maps(&cards, name, "config.go", 23, LineType::Added);
    }

    #[test]
    fn test_annotation_block_is_skipped() {
        let (raw, cards) = parse(CARD_LAYOUT);
        let hunk = line_of(&raw, "@@ -7,7 +7,7 @@ import (", 0);
        let anchor = line_of(
            &raw,
            "+ \tItemTitle(indent_level int, release_check_command string) string",
            hunk,
        );
        assert_maps(&cards, anchor, "org/org.go", 10, LineType::Added);
        assert_eq!(
            &raw.lines().nth(anchor as usize).unwrap(),
            &"    ┌─ error · Vet"
        );
        let next = line_of(&raw, "  \tSummary() string", anchor);
        assert_eq!(next, anchor + 4);
        assert_maps(&cards, next, "org/org.go", 11, LineType::Unmodified);
    }

    #[test]
    fn test_blocks_above_the_first_hunk() {
        // A file-level comment and an annotation that couldn't be anchored
        // hang above main.go's first hunk header.
        let (raw, cards) = parse(CARD_LAYOUT);
        let header = line_of(&raw, "modified     main.go", 0);
        let hunk = line_of(&raw, "@@ -21,7 +21,7 @@ func main() {", header);
        assert_eq!(
            raw.lines().nth(header as usize + 1).unwrap(),
            "    ┌─ C-Hipple · May 3, 2025 · on the file · outdated"
        );
        assert_maps(&cards, hunk + 1, "main.go", 21, LineType::Unmodified);
        let print_all = line_of(&raw, "  \t\tdoc.PrintAll()", hunk);
        assert_maps(&cards, print_all, "main.go", 25, LineType::Unmodified);
    }

    const RUST_BUFFER_HEAD: &str = "Project: * Review acme/widgets #42 *\n\
Root: /tmp/widgets/\n\
Type: my-code-review\n\
\n\
modified     src/lib.rs\n\
\n\
@@ -1,3 +1,4 @@\n";

    #[test]
    fn test_thread_and_annotations_in_a_hunk() {
        let raw = format!(
            "{}{}",
            RUST_BUFFER_HEAD,
            "  fn retry() {}\n\
+ fn backoff() {}\n\
    ┌─ bob · 2 days ago\n\
    │ Should this back off?\n\
    │ 👍 alice\n\
    ├─ you · pending\n\
    │ Let's cap the total wait at a minute.\n\
    └─\n\
    ┌─ warning · Security Check · line 2\n\
    │ + this content looks like an added line\n\
    ├─ Lint\n\
    │ - and this one like a removed line\n\
    └─\n\
  fn wait() {}\n\
  fn done() {}"
        );
        let diff = ParsedDiff::parse(&raw).unwrap();
        assert_maps(&diff, 9, "src/lib.rs", 2, LineType::Added);
        for line in 10..=20 {
            assert!(diff.map_diff_line_to_src(line).is_none(), "line {}", line);
        }
        assert_maps(&diff, 21, "src/lib.rs", 3, LineType::Unmodified);
        assert_maps(&diff, 22, "src/lib.rs", 4, LineType::Unmodified);
    }

    #[test]
    fn test_old_plugin_annotation_block_is_skipped() {
        // Before #343 annotations were drawn as their own titled box.
        let raw = format!(
            "{}{}",
            RUST_BUFFER_HEAD,
            "  fn retry() {}\n\
+ fn backoff() {}\n\
    ┌─ PLUGIN ANNOTATION ────────────\n\
    │ [warning] Security Check\n\
    │   this line looks wrong\n\
    └──────────────────────────────────\n\
  fn wait() {}"
        );
        let diff = ParsedDiff::parse(&raw).unwrap();
        for line in 10..=13 {
            assert!(diff.map_diff_line_to_src(line).is_none(), "line {}", line);
        }
        assert_maps(&diff, 14, "src/lib.rs", 3, LineType::Unmodified);
    }

    #[test]
    fn test_box_drawing_in_source_still_maps() {
        // A diagram in the source isn't a block: a block's header line has
        // a title after its `┌─ `.
        let raw = format!(
            "{}{}",
            RUST_BUFFER_HEAD,
            "  // ┌──────┐\n\
  ┌──────┐\n\
  │ box  │\n\
  └──────┘\n\
+ fn after() {}"
        );
        let diff = ParsedDiff::parse(&raw).unwrap();
        assert_maps(&diff, 9, "src/lib.rs", 2, LineType::Unmodified);
        assert_maps(&diff, 10, "src/lib.rs", 3, LineType::Unmodified);
        assert_maps(&diff, 11, "src/lib.rs", 4, LineType::Unmodified);
        assert_maps(&diff, 12, "src/lib.rs", 5, LineType::Added);
    }
}
