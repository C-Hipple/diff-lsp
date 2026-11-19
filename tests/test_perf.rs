//! Rough timing harness for the diff parsers. Ignored by default; run with:
//!   cargo test --release --test test_perf -- --ignored --nocapture
#[cfg(test)]
mod tests {
    use diff_lsp::parsers::utils::{Parsable, ParsedDiff};
    use std::fs::read_to_string;
    use std::time::Instant;

    const ITERS: u32 = 2000;

    fn time_parse(path: &str) {
        let raw = read_to_string(path).unwrap();
        // warm up
        assert!(ParsedDiff::parse(&raw).is_some());
        let start = Instant::now();
        for _ in 0..ITERS {
            let diff = ParsedDiff::parse(&raw).unwrap();
            std::hint::black_box(diff);
        }
        let elapsed = start.elapsed();
        println!(
            "{path}: {} lines, {ITERS} parses in {:?} ({:?}/parse)",
            raw.lines().count(),
            elapsed,
            elapsed / ITERS
        );
    }

    #[test]
    #[ignore]
    fn bench_parse_fixtures() {
        time_parse("tests/data/full_go_diff.code_review");
        time_parse("tests/data/go_diff.code_review_server");
        time_parse("tests/data/big_rust_diff.magit_status");
    }
}
