#[cfg(test)]
mod tests {
    use diff_lsp::{
        uri_from_relative_filename, DiffHeader, DiffLine, Hunk, LineType, MagitDiff, Parsable,
        ParsedDiff,
    };
    use std::fs;

    #[test]
    fn test_parse_hunk() {
        let header: &str = "@@ -60,9 +60,10 @@ impl LspClient {";
        let hunk_lines = "text_document: {
                     Some(TextDocumentClientCapabilities {
                         hover: Some(HoverClientCapabilities::default()),
-                        ..Default::default()
-                    })
-                },
+                        references: Some(ReferenceClientCapabilities{
+                            include_declaration: true
+                        }),
+                        ..Default::default()},
                 window: None,
                 general: None,
                 experimental: None,"
            .split("\n")
            .map(|li| DiffLine {
                line_type: LineType::from_line(li),
                line: li.to_string(),
                pos_in_hunk: 0,
            })
            .collect();

        let parsed_hunk = Hunk::parse(header, hunk_lines, "src/client.rs".to_string(), 0).unwrap();
        assert_eq!(parsed_hunk.start_old, 60);
        assert_eq!(parsed_hunk.start_new, 60);
        assert_eq!(parsed_hunk.change_length_old, 9);
        assert_eq!(parsed_hunk.change_length_new, 10);
        assert_eq!(parsed_hunk.changes.len(), 13);
    }

    #[test]
    fn test_diff_type_selection() {
        let go_status_diff = fs::read_to_string("tests/data/go_diff.magit_status").unwrap();
        let parsed_diff_magit = ParsedDiff::parse(&go_status_diff).unwrap();
        let magit_diff = MagitDiff::parse(&go_status_diff).unwrap();
        assert_eq!(parsed_diff_magit.headers, magit_diff.headers);
        assert_eq!(parsed_diff_magit.hunks, magit_diff.hunks);

        // let go_code_review_diff = fs::read_to_string("tests/data/go_diff.code_review").unwrap();
        // let parsed_diff_code_review = ParsedDiff::parse(&go_code_review_diff).unwrap();
        // let code_review_diff = CodeReviewDiff::parse(&go_code_review_diff).unwrap();
        // assert_eq!(parsed_diff_code_review.headers, code_review_diff.headers);
        // assert_eq!(parsed_diff_code_review.hunks, code_review_diff.hunks);
    }

    #[test]
    fn test_parse_magit_diff() {
        let raw_diff = r#"Project: magit: diff-lsp
Root: /Users/chrishipple/diff-lsp/
Buffer: diff-lsp
Type: magit-status
Head:     main readme typo
Merge:    origin/main readme typo
Push:     origin/main readme typo

Unstaged changes (1)
modified   src/client.rs
@@ -60,9 +60,10 @@ impl LspClient {
                 text_document: {
                     Some(TextDocumentClientCapabilities {
                         hover: Some(HoverClientCapabilities::default()),
-                        ..Default::default()
-                    })
-                },
+                        references: Some(ReferenceClientCapabilities{
+                            include_declaration: true
+                        }),
+                        ..Default::default()},
                 window: None,
                 general: None,
                 experimental: None,
@@ -72,17 +73,17 @@ impl LspClient {
             client_info: None,
             locale: None,
         };
-        let message = "initialize".to_string();
+        let message_type = "initialize".to_string();  // TODO: Is there an enum for this?

-        let raw_resp = self.send_request(message, params).unwrap();
+        let raw_resp = self.send_request(message_type, params).unwrap();
         let resp: InitializeResult = serde_json::from_value(raw_resp).unwrap();
         println!("We got the response: {resp:?}");

         return Ok(resp);
     }

-    pub fn send_request<P: Serialize>(&mut self, message: String, params: P) -> Result<Value> {
-        if message == "initialize".to_string() {
+    pub fn send_request<P: Serialize>(&mut self, message_type: String, params: P) -> Result<Value> {
+        if message_type == "initialize".to_string() {
             let _ser_params = serde_json::to_value(params).unwrap();
             let raw_resp = self.send_value_request(_ser_params).unwrap();
             let as_value: Value = serde_json::from_str(&raw_resp).unwrap();

Recent commits
97f1e20 origin/main readme typo
f3b9f94 send message and serialize response (init message atleast)
803d9f2 send message with full body, work on parse resposne
6edde96 MVP--We can send the stdin message to server; working on format & reading response
f3cad47 MVP of starting the server and reading the stdout
d083654 more readme
577afab Create rust.yml
9ce2121 working on adding the client
8ffb4ce Added hover support with static suggestion
4d7867a following tutorial on tower-lsp

"#;
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
        let first_hunk = &parsed_diff.hunks[0];
        assert_eq!(first_hunk.filename, "src/client.rs".to_string());
        assert_eq!(parsed_diff.hunks.len(), 2);

        let mut hunk_iter = parsed_diff.hunks.into_iter();
        let top_hunk = hunk_iter.nth(0).unwrap();
        let second_hunk = hunk_iter.nth(0).unwrap();

        assert_eq!(top_hunk.filename, "src/client.rs".to_string());
        assert_eq!(second_hunk.filename, "src/client.rs".to_string());

        assert_eq!(top_hunk.start_old, 60);
        assert_eq!(top_hunk.start_new, 60);
        assert_eq!(top_hunk.change_length_old, 9);
        assert_eq!(top_hunk.change_length_new, 10);
        assert_eq!(top_hunk.changes.len(), 13);
        assert_eq!(top_hunk.diff_location, 11);

        assert_eq!(second_hunk.start_old, 72);
        assert_eq!(second_hunk.start_new, 73);
        assert_eq!(second_hunk.change_length_old, 17);
        assert_eq!(second_hunk.change_length_new, 17);
        assert_eq!(second_hunk.changes.len(), 21);
        assert_eq!(second_hunk.diff_location, 25);
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

        let map = diff.map_diff_line_to_src(10);
        assert!(map.is_none(), "Before hunk starts");

        let map = diff.map_diff_line_to_src(13).unwrap(); // the empty space
        assert_eq!(map.source_line_type, LineType::Unmodified);
        assert_eq!(map.source_line, 12);

        let map = diff.map_diff_line_to_src(14).unwrap(); // +var logger
        assert_eq!(map.source_line, 13);
        assert_eq!(map.source_line_type, LineType::Added);
        assert_eq!(map.file_name, String::from("main.go"));
    }

    #[test]
    fn test_parse_simple_code_review_buffer() {
        let go_code_review_diff = fs::read_to_string("tests/data/go_diff.code_review").unwrap();
        let diff = ParsedDiff::parse(&go_code_review_diff).unwrap();
        assert_eq!(
            diff.headers.get(&DiffHeader::Project),
            Some(&"*Code Review*".to_string())
        );
        assert_eq!(diff.hunks.len(), 2);

        let first_hunk = &diff.hunks[0];
        let second_hunk = &diff.hunks[1];
        println!("{:?}", first_hunk.filename);
        println!("{:?}", second_hunk.filename);

        assert_eq!(first_hunk.filename, "components/hover.go".to_string());
        assert_eq!(second_hunk.filename, "main.go".to_string());
    }

    #[test]
    fn test_parse_complex_code_review_buffer() {
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
        assert_eq!(diff.hunks.len(), 7);

        let mapped = diff.map_diff_line_to_src(63).unwrap();
        assert_eq!(mapped.file_name, "config.go".to_string());
        assert_eq!(mapped.source_line_type, LineType::Unmodified);
        assert_eq!(mapped.source_line, 50);

        println!("mapped 63: {:?}", mapped);

        let mapped = diff.map_diff_line_to_src(64).unwrap();
        println!("mapped: 64 {:?}", mapped);
        assert_eq!(mapped.file_name, "config.go".to_string());
        assert_eq!(mapped.source_line_type, LineType::Removed);
        assert_eq!(mapped.source_line, 51);

        let mapped = diff.map_diff_line_to_src(65).unwrap();
        assert_eq!(mapped.file_name, "config.go".to_string());
        assert_eq!(mapped.source_line_type, LineType::Added);
        assert_eq!(mapped.source_line, 52);

        let mapped = diff.map_diff_line_to_src(66).unwrap();
        assert_eq!(mapped.file_name, "config.go".to_string());
        assert_eq!(mapped.source_line_type, LineType::Unmodified);
        assert_eq!(mapped.source_line, 53);

        let mapped = diff.map_diff_line_to_src(239).unwrap();
        println!("mapped: 239 {:?}", mapped);
        assert_eq!(mapped.source_line_type, LineType::Added);
        assert_eq!(mapped.file_name, "workflows/review_workflow.go".to_string());

        // Before the comment
        let mapped = diff.map_diff_line_to_src(240).unwrap();
        println!("mapped: 240 {:?}", mapped);

        // Around the comment
        let mapped = diff.map_diff_line_to_src(241);
        println!("mapped: 241 {:?}", mapped);

        assert!(mapped.is_none());
        // assert_eq!(mapped.file_name, "workflows/review_workflow.go".to_string());
        // assert_eq!(mapped.source_line_type, LineType::Added);
        // assert_eq!(mapped.source_line,  53);
    }
}
