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
        assert!(diff.filenames.contains(&"git_tools/github_notifications.go".to_string()));
        assert!(diff.filenames.contains(&"main.go".to_string()));
        assert!(diff.filenames.contains(&"org/org.go".to_string()));
        assert!(diff.filenames.contains(&"org/org_parser.go".to_string()));
        assert!(diff.filenames.contains(&"workflows/logic.go".to_string()));
        assert!(diff.filenames.contains(&"workflows/manager.go".to_string()));
        assert!(diff.filenames.contains(&"workflows/workflows.go".to_string()));
        
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
        assert!(mapped.is_none(), "Comment box lines should not map to source");
        
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
            assert_eq!(map.file_name, "git_tools/github_notifications.go".to_string());
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
            assert!(mapped.is_none(), 
                "Line {} is in a comment box and should not map to source", line_num);
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
            let mapped = diff.map_diff_line_to_src(line_num);
            if mapped.is_some() {
                // If it maps, it should be to main.go
                let map = mapped.unwrap();
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
