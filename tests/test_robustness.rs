//! Inputs that used to panic the server: line numbers past u16, malformed
//! hunk/file headers, unreadable diff buffers, relative project roots.

use std::path::PathBuf;

use diff_lsp::parsers::utils::{parse_header, LineType, Parsable, ParsedDiff, SourceLineNumber};
use diff_lsp::server::{DiffLsp, SUPPORTED_COMMANDS};
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

const CODE_REVIEW_FIXTURE: &str = "tests/data/go_diff.code_review_server";

#[test]
fn test_parse_header_beyond_u16() {
    assert_eq!(
        parse_header("@@ -70000,3 +70000,4 @@ fn foo()"),
        Some((70000, 3, 70000, 4))
    );
}

#[test]
fn test_parse_header_malformed() {
    assert_eq!(parse_header("@@ garbage @@"), None);
    // Doesn't fit in a u32
    assert_eq!(parse_header("@@ -1,1 +99999999999,1 @@"), None);
}

#[test]
fn test_code_review_hunk_beyond_u16() {
    let raw = "Type: my-code-review\n\
               Root: /tmp/foo\n\
               \n\
               modified   src/big.rs\n\
               @@ -70000,3 +70000,4 @@ fn foo()\n \
               context\n\
               +added\n \
               context2\n";
    let diff = ParsedDiff::parse(raw).unwrap();
    let m = diff.map_diff_line_to_src(6).unwrap();
    assert_eq!(m.source_line, SourceLineNumber(70000));
    let m = diff.map_diff_line_to_src(7).unwrap();
    assert_eq!(m.source_line, SourceLineNumber(70001));
    assert_eq!(m.source_line_type, LineType::Added);
    // Backends get the 0-indexed line.
    assert_eq!(m.source_position(Position::new(7, 3)).line, 70000);
    let m = diff.map_diff_line_to_src(8).unwrap();
    assert_eq!(m.source_line, SourceLineNumber(70002));
}

#[test]
fn test_buffer_line_beyond_u16() {
    // A review buffer longer than 65,535 lines.
    let mut raw = String::from(
        "Type: my-code-review\nRoot: /tmp/foo\n\nmodified   src/big.rs\n@@ -1,1 +1,70000 @@\n",
    );
    for i in 0..70_000 {
        raw.push_str(&format!("+line {}\n", i));
    }
    let diff = ParsedDiff::parse(&raw).unwrap();
    // Input line 70_005 is the 70_000th added line.
    let m = diff.map_diff_line_to_src(70_005).unwrap();
    assert_eq!(m.source_line, SourceLineNumber(70_000));
}

#[test]
fn test_magit_hunk_beyond_u16() {
    let raw = "Type: magit-status\n\
               Head: main foo\n\
               \n\
               modified   src/big.rs\n\
               @@ -70000,3 +70000,4 @@ fn foo()\n \
               context\n\
               +added\n";
    let diff = ParsedDiff::parse(raw).unwrap();
    let m = diff.map_diff_line_to_src(7).unwrap();
    assert_eq!(m.source_line, SourceLineNumber(70001));
}

#[test]
fn test_malformed_hunk_header_does_not_panic() {
    for kind in ["my-code-review", "magit-status"] {
        let raw = format!(
            "Type: {}\n\nmodified   src/a.rs\n@@ not a header @@\n+added\n@@ -1,1 +5,1 @@\n+ok\n",
            kind
        );
        let diff = ParsedDiff::parse(&raw).unwrap();
        // Lines after a malformed header aren't mapped against a stale hunk.
        assert!(diff.map_diff_line_to_src(5).is_none(), "{}", kind);
        let m = diff.map_diff_line_to_src(7).unwrap();
        assert_eq!(m.source_line, SourceLineNumber(5), "{}", kind);
    }
}

#[test]
fn test_bare_file_header_does_not_panic() {
    let raw = "Type: my-code-review\n\nmodified\ndiff --git \n@@ -1,1 +1,1 @@\n+x\n";
    assert!(ParsedDiff::parse(raw).is_some());
}

#[test]
fn test_magit_new_file_starts_a_section() {
    let raw = "Type: magit-status\n\
               Head: main foo\n\
               \n\
               modified   src/a.rs\n\
               @@ -1,1 +1,2 @@\n \
               a\n\
               +b\n\
               new file   src/c.rs\n\
               @@ -0,0 +1,1 @@\n\
               +c\n";
    let diff = ParsedDiff::parse(raw).unwrap();
    assert_eq!(diff.filenames, vec!["src/a.rs", "src/c.rs"]);
    // The `new file` line is a header, not a line of src/a.rs.
    assert!(diff.map_diff_line_to_src(8).is_none());
    let m = diff.map_diff_line_to_src(10).unwrap();
    assert_eq!(m.file_name, "src/c.rs");
    assert_eq!(m.source_line, SourceLineNumber(1));
}

fn repo_root() -> String {
    env!("CARGO_MANIFEST_DIR").to_string()
}

fn open_params(uri: Url) -> DidOpenTextDocumentParams {
    DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri,
            language_id: "diff".to_string(),
            version: 1,
            text: String::new(),
        },
    }
}

#[tokio::test]
async fn test_unreadable_diff_buffer_does_not_panic() {
    let (service, _socket) =
        LspService::new(|client| DiffLsp::new(client, Default::default(), repo_root()));
    let server = service.inner();
    let missing = Url::from_file_path("/nonexistent/diff-lsp/diff_lsp_missing").unwrap();
    server.did_open(open_params(missing.clone())).await;
    server
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: missing },
        })
        .await;
    assert!(server.diff_map.lock().await.is_empty());
}

#[tokio::test]
async fn test_relative_root_does_not_panic() {
    // File URIs can't be built from a relative root; files are skipped.
    let (service, _socket) = LspService::new(|client| {
        DiffLsp::new(client, Default::default(), "relative/root".to_string())
            .with_command_override("diff-lsp-test-definitely-not-a-real-lsp".to_string(), None)
    });
    let server = service.inner();
    let uri = Url::from_file_path(PathBuf::from(repo_root()).join(CODE_REVIEW_FIXTURE)).unwrap();
    server.did_open(open_params(uri.clone())).await;
    assert_eq!(server.diff_map.lock().await.len(), 1);

    let res = server
        .document_symbol(DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_execute_command_provider_lists_supported_commands() {
    let (service, _socket) =
        LspService::new(|client| DiffLsp::new(client, Default::default(), repo_root()));
    let res = service
        .inner()
        .initialize(InitializeParams::default())
        .await
        .unwrap();
    let commands = res.capabilities.execute_command_provider.unwrap().commands;
    assert_eq!(commands, SUPPORTED_COMMANDS);
    assert_eq!(commands, vec!["refresh", "fetch", "check"]);
}
