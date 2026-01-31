use diff_lsp::parsers::code_review::CodeReviewDiff;
use diff_lsp::parsers::utils::{DiffHeader, Parsable};
use std::fs;

#[test]
fn test_parse_raw_git_diff_with_headers() {
    let raw_diff = fs::read_to_string("tests/data/rust_diff.bun_client").unwrap();
    let parsed_diff = CodeReviewDiff::parse(&raw_diff).unwrap();

    // Check Headers
    assert_eq!(
        parsed_diff.headers.get(&DiffHeader::Buffer),
        Some(&"PR #9".to_string())
    );
    assert_eq!(
        parsed_diff.headers.get(&DiffHeader::Type),
        Some(&"code-review".to_string())
    );

    // Check Filenames
    assert!(parsed_diff.filenames.contains(&"src/client.rs".to_string()));
    assert!(parsed_diff.filenames.contains(&"src/lib.rs".to_string()));
    assert!(parsed_diff.filenames.contains(&"src/server.rs".to_string()));

    // Check content was actually parsed (lines map populated)
    assert!(parsed_diff.lines_map.len() > 0);
}
