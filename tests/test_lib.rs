// use std::io::{self, Write};

struct SimpleLogger;

// To use a logger in a test, add this line at the start of the test
// let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Debug)); // Adjust level here too

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug // Adjust level as needed
    }

    fn log(&self, record: &log::Record) {
        // println!("logging");
        if self.enabled(record.metadata()) {
            // let mut stdout = io::stdout();
            println!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

#[cfg(test)]
mod tests {
    use super::SimpleLogger;
    use diff_lsp::parsers::code_review::CodeReviewDiff;
    use diff_lsp::parsers::magit::MagitDiff;
    use diff_lsp::parsers::utils::{DiffHeader, LineType, Parsable, ParsedDiff, SourceLineNumber};
    use diff_lsp::{uri_from_relative_filename, SupportedFileType};
    use std::fs;

    #[allow(unused)]
    static LOGGER: SimpleLogger = SimpleLogger;

    #[test]
    fn test_supported_file_type_from_filename() {
        assert_eq!(
            SupportedFileType::from_filename("Makefile".to_string()),
            None
        );
        assert_eq!(
            SupportedFileType::from_filename("hi.py".to_string()),
            Some(SupportedFileType::Python)
        );
        assert_eq!(
            SupportedFileType::from_filename("test.hi.py".to_string()),
            Some(SupportedFileType::Python)
        );
        assert_eq!(
            SupportedFileType::from_filename("test.hi.rs".to_string()),
            Some(SupportedFileType::Rust)
        );
        assert_eq!(
            SupportedFileType::from_filename("main.rs".to_string()),
            Some(SupportedFileType::Rust)
        );
        assert_eq!(
            SupportedFileType::from_filename("main.go".to_string()),
            Some(SupportedFileType::Go)
        );
        assert_eq!(SupportedFileType::from_filename("go".to_string()), None);
    }

    #[test]
    fn test_diff_type_selection() {
        let go_status_diff = fs::read_to_string("tests/data/go_diff.magit_status").unwrap();
        let parsed_diff_magit = ParsedDiff::parse(&go_status_diff).unwrap();
        let magit_diff = MagitDiff::parse(&go_status_diff).unwrap();
        assert_eq!(parsed_diff_magit.headers, magit_diff.headers);

        let go_code_review_diff = fs::read_to_string("tests/data/go_diff.code_review").unwrap();
        let parsed_diff_code_review = ParsedDiff::parse(&go_code_review_diff).unwrap();
        let code_review_diff = CodeReviewDiff::parse(&go_code_review_diff).unwrap();
        assert_eq!(parsed_diff_code_review.headers, code_review_diff.headers);
        // assert_eq!(parsed_diff_code_review.hunks, code_review_diff.hunks);
    }

    #[test]
    fn test_parse_magit_diff() {
        let raw_diff = fs::read_to_string("tests/data/rust_diff.magit_status").unwrap();
        let parsed_diff = MagitDiff::parse(&raw_diff).unwrap();
        assert_eq!(
            parsed_diff.headers.get(&DiffHeader::Buffer),
            Some(&"diff-lsp".to_string())
        );
        assert_eq!(
            parsed_diff.headers.get(&DiffHeader::Type),
            Some(&"magit-status".to_string())
        );
        assert_eq!(
            parsed_diff.headers.get(&DiffHeader::Project),
            Some(&"magit: diff-lsp".to_string())
        );

        assert_eq!(parsed_diff.filenames, vec!["src/client.rs".to_string()])
    }

    #[test]
    fn test_uri_for_rel() {
        let output =
            uri_from_relative_filename("/Users/chrishipple/diff-lsp/".to_string(), "src/main.rs");
        println!("output {:?}", output.as_str());
        assert_eq!(
            "file:///Users/chrishipple/diff-lsp/src/main.rs",
            output.as_str()
        );
    }

    #[test]
    fn test_source_map() {
        let go_status_diff = fs::read_to_string("tests/data/go_diff.magit_status").unwrap();
        let diff = MagitDiff::parse(&go_status_diff).unwrap();

        for (k, v) in diff.lines_map.iter() {
            println!("lines map: {:?} => {:?}", k, v);
        }

        let map = diff.map_diff_line_to_src(10);
        assert!(map.is_none(), "Before hunk starts");

        // the first line of the diff
        let map = diff.map_diff_line_to_src(12).unwrap();
        assert_eq!(map.source_line_type, LineType::Unmodified);
        assert_eq!(map.source_line, SourceLineNumber(11)); // only chance that it's input - 1
        assert!(map.source_line_text.contains("github"));
        println!("{:?}", map);

        let map = diff.map_diff_line_to_src(13).unwrap();
        assert_eq!(map.source_line_type, LineType::Unmodified);
        assert_eq!(map.source_line, SourceLineNumber(12));

        let map = diff.map_diff_line_to_src(14).unwrap();
        assert_eq!(map.source_line, SourceLineNumber(13));
        assert_eq!(map.source_line_type, LineType::Unmodified);
        assert_eq!(map.file_name, String::from("main.go"));
    }

    #[test]
    fn test_source_map_multiple_hunks() {
        let raw_diff = fs::read_to_string("tests/data/rust_diff.magit_status").unwrap();
        let diff = MagitDiff::parse(&raw_diff).unwrap();

        for (k, v) in diff.lines_map.iter() {
            println!("lines map: {:?} => {:?}", k, v);
        }

        let map = diff.map_diff_line_to_src(26).unwrap();
        assert_eq!(map.source_line_type, LineType::Unmodified);
        assert_eq!(map.source_line, SourceLineNumber(73));

        // assert!(false);
    }

    #[test]
    fn test_parse_simple_code_review_buffer() {
        let go_code_review_diff = fs::read_to_string("tests/data/go_diff.code_review").unwrap();
        let diff = ParsedDiff::parse(&go_code_review_diff).unwrap();
        assert_eq!(
            diff.headers.get(&DiffHeader::Project),
            Some(&"*Code Review*".to_string())
        );
    }

    #[test]
    fn test_source_map_big_hunks() {
        let raw_diff = fs::read_to_string("tests/data/big_rust_diff.magit_status").unwrap();
        let diff = MagitDiff::parse(&raw_diff).unwrap();

        for (k, v) in diff.lines_map.iter() {
            println!("lines map: {:?} => {:?}", k, v);
        }

        let map = diff.map_diff_line_to_src(11).unwrap();
        assert_eq!(map.source_line_type, LineType::Unmodified);
        assert_eq!(map.source_line, SourceLineNumber(290));

        let map = diff.map_diff_line_to_src(61).unwrap();
        assert_eq!(map.source_line_type, LineType::Added);
        assert_eq!(map.source_line, SourceLineNumber(350));

        // assert!(false);
    }

    #[test]
    fn test_parse_complex_code_review_buffer() {
        // let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Debug)); // Adjust level here too
        let go_code_review_diff =
            fs::read_to_string("tests/data/full_go_diff.code_review").unwrap();
        let diff = ParsedDiff::parse(&go_code_review_diff).unwrap();
        assert_eq!(
            diff.headers.get(&DiffHeader::Project),
            Some(&"*Code Review*".to_string())
        );

        assert_eq!(
            diff.headers.get(&DiffHeader::State),
            Some(&"MERGED".to_string())
        );

        // 63 is 0 index, so it's 64th line when in editor
        let mapped = diff.map_diff_line_to_src(63).unwrap();
        println!("mapped 63: {:?}", mapped);
        assert_eq!(mapped.file_name, "config.go".to_string());
        assert_eq!(mapped.source_line_type, LineType::Unmodified);
        assert_eq!(mapped.source_line, SourceLineNumber(49));

        let mapped = diff.map_diff_line_to_src(64).unwrap();
        println!("mapped: 64 {:?}", mapped);
        assert_eq!(mapped.file_name, "config.go".to_string());
        assert_eq!(mapped.source_line_type, LineType::Unmodified);
        assert_eq!(mapped.source_line, SourceLineNumber(50));

        let mapped = diff.map_diff_line_to_src(65).unwrap();
        assert_eq!(mapped.file_name, "config.go".to_string());
        assert_eq!(mapped.source_line_type, LineType::Removed);
        assert_eq!(mapped.source_line, SourceLineNumber(51));

        let mapped = diff.map_diff_line_to_src(66).unwrap();
        assert_eq!(mapped.file_name, "config.go".to_string());
        assert_eq!(mapped.source_line_type, LineType::Added);
        assert_eq!(mapped.source_line, SourceLineNumber(51));

        let mapped = diff.map_diff_line_to_src(239).unwrap();
        println!("mapped: 239 {:?}", mapped);
        assert_eq!(mapped.source_line_type, LineType::Added);
        assert_eq!(mapped.file_name, "workflows/review_workflow.go".to_string());

        let mapped = diff.map_diff_line_to_src(240).unwrap();
        println!("mapped: 240 {:?}", mapped);
        assert_eq!(mapped.source_line_type, LineType::Added);
        assert_eq!(mapped.file_name, "workflows/review_workflow.go".to_string());

        // Around the comment
        let mapped = diff.map_diff_line_to_src(241);
        println!("mapped: 241 {:?}", mapped);
        assert!(mapped.is_none());

        let mapped = diff.map_diff_line_to_src(242);
        println!("mapped: 242 {:?}", mapped);
        assert!(mapped.is_none());

        let mapped = diff.map_diff_line_to_src(243);
        println!("mapped: 243 {:?}", mapped);
        assert!(mapped.is_none());

        // after the comment
        let mapped = diff.map_diff_line_to_src(244).unwrap();
        println!("mapped: 244 {:?}", mapped);
        assert_eq!(
            mapped.source_line_text,
            "+\tsection, err := doc.GetSection(w.SectionTitle)"
        );
        assert_eq!(mapped.source_line, SourceLineNumber(75));

        let mapped = diff.map_diff_line_to_src(247).unwrap();
        println!("mapped: 247 {:?}", mapped);
        assert_eq!(mapped.source_line, SourceLineNumber(78));
        assert_eq!(mapped.source_line_text, "+\t\treturn");

        // second comment (is a tree with 2 comments)
        let mapped = diff.map_diff_line_to_src(248);
        println!("mapped: 248 {:?}", mapped);
        assert!(mapped.is_none());

        let mapped = diff.map_diff_line_to_src(249);
        println!("mapped: 249 {:?}", mapped);
        assert!(mapped.is_none());

        let mapped = diff.map_diff_line_to_src(250);
        println!("mapped: 250 {:?}", mapped);
        assert!(mapped.is_none());

        let mapped = diff.map_diff_line_to_src(252);
        println!("mapped: 252 {:?}", mapped);
        assert!(mapped.is_none());

        let mapped = diff.map_diff_line_to_src(254).unwrap();
        println!("mapped: 254 {:?}", mapped);
        assert_eq!(mapped.source_line, SourceLineNumber(79));
        assert_eq!(mapped.source_line_text, "+\t}");
    }
}
