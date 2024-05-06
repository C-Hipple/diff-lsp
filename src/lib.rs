use regex::Regex;

#[allow(dead_code)]
#[derive(Debug)]
enum LineType {
    Add,
    Remove,
    Unmodified,
}

impl LineType {
    fn from_line(line: String) -> Self {

        match line.chars().next() {
            Some('+') => LineType::Add,
            Some('-') => LineType::Remove,
            _ => LineType::Unmodified,
        }

    }
}

#[allow(dead_code)]
pub struct DiffLine {
    line_type: LineType,
    line: String,
}

#[allow(dead_code)]
#[derive(Default)]
pub struct Hunk {
    start_old: u16,
    change_length_old: u16,
    start_new: u16, // consider s/new/modified
    change_length_new: u16,
    changes: Vec<DiffLine>,
}

impl Hunk {
    pub fn parse(source: &str) -> Option<Hunk> {
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
}

#[allow(dead_code)]
pub struct MagitDiff {
    headers: Vec<String>,
    hunks: Vec<Hunk>,
}

pub trait Parse<S> {
    fn parse(source: &str) -> S;
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
        assert_eq!(parsed_hunk.changes.len(), 13)
    }
}
