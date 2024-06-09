use regex::Regex;
use std::collections::HashMap;

use std::str::FromStr;
use strum_macros::EnumString;

#[derive(Debug, Hash, PartialEq, std::cmp::Eq)]
pub enum SupportedFileType {
    Rust,
    Go,
    Python,
}

// TODO this feels clunky

impl SupportedFileType {
    pub fn from_extension(extension: String) -> Option<SupportedFileType> {
        match extension.as_str() {
            "rs" => Some(SupportedFileType::Rust),
            "go" => Some(SupportedFileType::Go),
            "py" => Some(SupportedFileType::Python),
            _    => None
        }
    }

}

pub fn get_lsp_for_file_type(file_type: SupportedFileType) -> Option<String> {
    match file_type {
        SupportedFileType::Rust => Some("rust-analyzer".to_string()),
        SupportedFileType::Go => Some("gopls".to_string()),
        SupportedFileType::Python => Some("pylsp".to_string()),
    }
}


#[allow(dead_code)]
#[derive(Debug, Clone)]
enum LineType {
    Added,
    Removed,
    Unmodified,
}

impl LineType {
    fn from_line(line: String) -> Self {
        match line.chars().next() {
            // Could technically be bugger if it's a diff and the first char is 1 of these and it's unmodified
            Some('+') => LineType::Added,
            Some('-') => LineType::Removed,
            _ => LineType::Unmodified,
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct DiffLine {
    line_type: LineType,
    line: String,
}


#[allow(dead_code)]
#[derive(Default, Clone, Debug)]
pub struct Hunk {
    filename: String, // relative path, i.e. /src/client.rs
    start_old: u16,
    change_length_old: u16,
    start_new: u16, // consider s/new/modified
    change_length_new: u16,
    changes: Vec<DiffLine>,
    diff_location: u16, // Where the raw hunk starts (the @@ line) in the plain text diff.
}

impl Hunk {
    pub fn parse(source: &str) -> Option<Hunk> {
        // println!("Parsing the hunk from lines:");
        // println!("{}", source);
        let mut found_header = false;
        let mut wip = Hunk::default();
        for line in source.lines() {
            let re = Regex::new(r"@@ -(\d+),(\d+) \+(\d+),(\d+) @@").unwrap();
            if let Some(caps) = re.captures(line) {
                found_header = true;
                wip.start_old = caps[1].parse::<u16>().unwrap();
                wip.change_length_old = caps[2].parse::<u16>().unwrap();
                wip.start_new = caps[3].parse::<u16>().unwrap();
                wip.change_length_new = caps[4].parse::<u16>().unwrap();
            } else {
                if found_header {
                    wip.changes.push(DiffLine {
                        line_type: LineType::from_line(line.to_string()),
                        line: line.to_string(),
                    })
                }
            }
        }
        // for li in &wip.changes {
        //     println!("Line: {0:?}, {1:?}", li.line_type, li.line);

        // }
        //TODO: More other handling
        Some(wip)
    }

    pub fn diff_length(&self) -> u16 {
        self.changes.len() as u16
    }

    pub fn diff_end(&self) -> u16 {
        self.diff_location + self.diff_length()
    }

    pub fn file_type(&self) -> String {
        // better not get any files with "." in them
        self.filename.split_once('.').unwrap().1.to_string()
    }
}

pub struct SourceMap {
    // Return type when you translate a
    pub file_name: String,
    pub source_line: u16,
    pub file_type: SupportedFileType,
}


#[derive(EnumString, Hash, PartialEq, std::cmp::Eq, Debug)]
pub enum DiffHeader {
    Project,
    Root,
    Buffer,
    Type,
    Head,
    Merge,
    Push,
}

#[allow(dead_code)]
#[derive(Default, Debug)]
pub struct MagitDiff {
    headers: HashMap<DiffHeader, String>,
    hunks: Vec<Hunk>,
}

#[allow(dead_code)]
impl MagitDiff {
    pub fn parse(source: &str) -> Option<Self> {
        let mut diff = MagitDiff::default();

        let mut found_headers = false;
        let mut current_filename = "";
        let mut building_hunk = false;
        let mut hunk_lines: Vec<&str> = vec![];
        let mut offset = 0;

        for (i, line) in source.lines().enumerate() {
            if !found_headers {
                let re = Regex::new(r"(\w+):\s+(.+)").unwrap();
                if let Some(caps) = re.captures(line) {
                    let header = DiffHeader::from_str(&caps[1]).unwrap();
                    diff.headers.insert(header, caps[2].to_string());
                } else {
                    found_headers = true;
                }
            } else {
                // found headers, moving onto hunks
                // TODO: Handle multiple files
                if line.starts_with("modified") {
                    current_filename = line.split_whitespace().nth(1).unwrap();
                }
                if line.starts_with("@@") && !building_hunk {
                    building_hunk = true;
                    hunk_lines.push(line);
                    continue;
                }
                if (line.starts_with("@@") && building_hunk) || line.starts_with("Recent commits") {
                    if line.starts_with("Recent commits") {
                        // magit puts an empty line between the last diff and "Recent Commits"
                        // and that's our only way to know that we're out of the hunk
                        // since a hunk could have an empty line.
                        offset = 1;
                    }
                    let mut hunk = Hunk::parse(hunk_lines.join("\n").as_str()).unwrap();
                    hunk.filename = current_filename.to_string();
                    hunk.diff_location = (i - hunk.changes.len() - offset) as u16;
                    diff.hunks.push(hunk);
                    hunk_lines = vec![];
                    if line.starts_with("@@") {
                        hunk_lines.push(line);
                    }
                    if line.starts_with("Recent commits") {
                        break;
                    }
                }

                if building_hunk {
                    hunk_lines.push(line);
                    continue;
                }
            }
        }
        Some(diff)
    }

    pub fn map_diff_line_to_src(&self, line_num: u16)  -> Option<SourceMap> {
        if let Some(hunk) = self.get_hunk_by_diff_line_number(line_num) {
            if let Some(supported_file_type) = SupportedFileType::from_extension(hunk.file_type()) {
                return Some(SourceMap{
                    file_name: hunk.filename,
                    source_line: line_num - hunk.diff_location + hunk.start_new,
                    file_type: supported_file_type,
                })
            }
        }
        None
    }

    // fn map_diff_line_to_src_number(&self, line_num: u16) -> Option<u16> {
    //     // Translates a line number on the magit-diff to a line in the source
    //     // the LSP client will always reference the "diff document" but our backend LSP servers need to know the
    //     // line number in the original source file.
    //     for hunk in &self.hunks {
    //         if line_num > hunk.diff_location && line_num <= hunk.diff_end() {
    //             return Some(line_num - hunk.diff_location)
    //         }
    //     }
    //     None
    // }

    fn get_hunk_by_diff_line_number(&self, line_num: u16) -> Option<Hunk> {
        for hunk in &self.hunks {
            if line_num > hunk.diff_location && line_num <= hunk.diff_end() {
                return Some(hunk.clone())  // is this going to shoot me in the foot?
            }
        }
        None
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }

    #[test]
    fn test_parse_hunk() {
        let hunk = "modified   src/client.rs
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
                 experimental: None,";

        let parsed_hunk = Hunk::parse(&hunk).unwrap();
        assert_eq!(parsed_hunk.start_old, 60);
        assert_eq!(parsed_hunk.start_new, 60);
        assert_eq!(parsed_hunk.change_length_old, 9);
        assert_eq!(parsed_hunk.change_length_new, 10);
        assert_eq!(parsed_hunk.changes.len(), 13);
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


}

