use std::fs::{self, File};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use diff_lsp::parsers::utils::{Parsable, ParsedDiff};
use diff_lsp::server::{create_backends_map, spawn_backend, DiffLsp};
use diff_lsp::utils::{
    get_most_recent_file_in_dirs, init_file_source, InitFileSource, INIT_FILE_PREFIX,
};
use diff_lsp::SupportedFileType;
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

const NONEXISTENT_COMMAND: &str = "diff-lsp-test-definitely-not-a-real-lsp";
const CODE_REVIEW_FIXTURE: &str = "tests/data/go_diff.code_review_server";

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "diff-lsp-test-{}-{}-{}",
        name,
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn touch(path: &PathBuf, mtime: SystemTime) {
    let f = File::create(path).unwrap();
    f.set_modified(mtime).unwrap();
}

fn repo_root() -> String {
    env!("CARGO_MANIFEST_DIR").to_string()
}

fn fixture_uri() -> Url {
    Url::from_file_path(PathBuf::from(repo_root()).join(CODE_REVIEW_FIXTURE)).unwrap()
}

fn open_params() -> DidOpenTextDocumentParams {
    DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: fixture_uri(),
            language_id: "diff".to_string(),
            version: 1,
            text: fs::read_to_string(CODE_REVIEW_FIXTURE).unwrap(),
        },
    }
}

fn hover_params(line: u32) -> HoverParams {
    HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: fixture_uri() },
            position: Position { line, character: 0 },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    }
}

// --- init file resolution ---

#[test]
fn test_init_file_from_arg() {
    assert_eq!(
        init_file_source(&args(&["--init", "/x/diff_lsp_a"]), None),
        InitFileSource::Arg("/x/diff_lsp_a".into())
    );
    assert_eq!(
        init_file_source(&args(&["--stdio", "--init=/x/y"]), Some("/env".into())),
        InitFileSource::Arg("/x/y".into())
    );
}

#[test]
fn test_init_file_from_positional_arg() {
    // The bare positional path from before `--init` existed still works.
    assert_eq!(
        init_file_source(&args(&["/x/diff_lsp_a"]), Some("/env".into())),
        InitFileSource::Arg("/x/diff_lsp_a".into())
    );
    assert_eq!(
        init_file_source(&args(&["--stdio", "/x/diff_lsp_a"]), None),
        InitFileSource::Arg("/x/diff_lsp_a".into())
    );
    // --init wins over a positional path, wherever it appears.
    assert_eq!(
        init_file_source(&args(&["/x/pos", "--init", "/x/flag"]), None),
        InitFileSource::Arg("/x/flag".into())
    );
}

#[test]
fn test_init_file_from_env() {
    assert_eq!(
        init_file_source(&args(&["--stdio"]), Some("/env/file".into())),
        InitFileSource::Env("/env/file".into())
    );
    // A dangling --init with no value falls through.
    assert_eq!(
        init_file_source(&args(&["--init"]), Some("/env/file".into())),
        InitFileSource::Env("/env/file".into())
    );
}

#[test]
fn test_init_file_scan_fallback() {
    assert_eq!(init_file_source(&[], None), InitFileSource::Scan);
    assert_eq!(
        init_file_source(&[], Some("  ".into())),
        InitFileSource::Scan
    );
}

#[test]
fn test_most_recent_file_across_dirs() {
    let a = scratch_dir("scan-a");
    let b = scratch_dir("scan-b");
    let now = SystemTime::now();
    let older = a.join(format!("{}older", INIT_FILE_PREFIX));
    let newer = b.join(format!("{}newer", INIT_FILE_PREFIX));
    let newest_but_wrong_prefix = a.join("something_else");
    touch(&older, now - Duration::from_secs(100));
    touch(&newer, now - Duration::from_secs(10));
    touch(&newest_but_wrong_prefix, now);

    let missing = PathBuf::from("/definitely/not/a/dir/for/diff-lsp");
    let found = get_most_recent_file_in_dirs(&[missing, a.clone(), b.clone()], INIT_FILE_PREFIX);
    assert_eq!(found, Some(newer.clone()));

    // Order of directories doesn't matter.
    let found = get_most_recent_file_in_dirs(&[b.clone(), a.clone()], INIT_FILE_PREFIX);
    assert_eq!(found, Some(newer));

    let empty = scratch_dir("scan-empty");
    assert_eq!(
        get_most_recent_file_in_dirs(std::slice::from_ref(&empty), INIT_FILE_PREFIX),
        None
    );

    let _ = fs::remove_dir_all(a);
    let _ = fs::remove_dir_all(b);
    let _ = fs::remove_dir_all(empty);
}

// --- missing backend tolerance ---

#[test]
fn test_spawn_missing_backend_is_an_error_not_a_panic() {
    let failure = spawn_backend(
        SupportedFileType::Python,
        &repo_root(),
        Some((NONEXISTENT_COMMAND.to_string(), None)),
    )
    .unwrap_err();
    assert_eq!(failure.file_type, SupportedFileType::Python);
    assert_eq!(failure.command, NONEXISTENT_COMMAND);
    assert!(failure.user_message().contains("Python features disabled"));
}

#[test]
fn test_create_backends_map_with_no_langs() {
    let (backends, failures) = create_backends_map(vec![], &repo_root());
    assert!(backends.is_empty());
    assert!(failures.is_empty());
}

#[tokio::test]
async fn test_missing_backend_lazy_start_is_tolerated() {
    let (service, _socket) = LspService::new(|client| {
        DiffLsp::new(client, Default::default(), repo_root())
            .with_command_override(NONEXISTENT_COMMAND.to_string(), None)
    });
    let server = service.inner();

    // Go is in the diff; the (fake) backend can't be spawned.
    server.did_open(open_params()).await;
    assert!(server.backend_failed(SupportedFileType::Go).await);
    assert!(server.backends.read().await.is_empty());

    // Re-opening doesn't panic and doesn't retry.
    server.did_open(open_params()).await;
    assert!(server.backends.read().await.is_empty());

    // Hover on a mapped line with no backend → Ok(None), not an error.
    let raw = fs::read_to_string(CODE_REVIEW_FIXTURE).unwrap();
    let diff = ParsedDiff::parse(&raw).unwrap();
    let mapped_line = (0..500u32)
        .find(|l| {
            diff.map_diff_line_to_src(*l)
                .map(|m| m.file_type == SupportedFileType::Go)
                .unwrap_or(false)
        })
        .expect("fixture has a mapped go line");
    let res = server.hover(hover_params(mapped_line)).await;
    assert!(matches!(res, Ok(None)));

    // Hover on a header line (no source map) → Ok(None).
    let res = server.hover(hover_params(0)).await;
    assert!(matches!(res, Ok(None)));
}

#[cfg(unix)]
#[tokio::test]
async fn test_backend_failing_initialize_is_disabled() {
    // `true` spawns fine but exits immediately, so initialize fails.
    let (service, _socket) = LspService::new(|client| {
        DiffLsp::new(client, Default::default(), repo_root())
            .with_command_override("true".to_string(), None)
    });
    let server = service.inner();
    server.did_open(open_params()).await;
    assert!(server.backend_failed(SupportedFileType::Go).await);
    assert!(server.backends.read().await.is_empty());
}
