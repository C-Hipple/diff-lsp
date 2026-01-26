#[cfg(test)]
mod tests {

    use diff_lsp::parsers::magit::MagitDiff;
    use diff_lsp::parsers::utils::{DiffHeader, Parsable};
    use diff_lsp::server::create_backends_map;
    use diff_lsp::server::read_initialization_params_from_tempfile;
    use diff_lsp::server::DiffLsp;
    use diff_lsp::SupportedFileType;
    use expanduser::expanduser;
    use log::info;
    use std::fs;
    use std::path::PathBuf;
    use tower_lsp::lsp_types::*;
    use tower_lsp::LanguageServer;
    // use super::*;
    use tower_lsp::LspService;

    fn get_init_params() -> tower_lsp::lsp_types::InitializeParams {
        #[allow(deprecated)] // root_path is deprecated but without it, code doesn't compile? :(
        InitializeParams {
            process_id: None,
            root_path: None,
            root_uri: None,
            initialization_options: None,
            capabilities: ClientCapabilities {
                workspace: None,
                text_document: {
                    Some(TextDocumentClientCapabilities {
                        hover: Some(HoverClientCapabilities::default()),
                        references: Some(ReferenceClientCapabilities {
                            dynamic_registration: Some(false),
                        }),
                        ..Default::default()
                    })
                },
                window: None,
                general: None,
                experimental: None,
            },
            trace: None,
            workspace_folders: None,
            client_info: None,
            locale: None,
        }
    }

    pub fn get_open_params_rust(uri: Url) -> tower_lsp::lsp_types::DidOpenTextDocumentParams {
        DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: (uri),
                // TODO: Figure out how to select based on
                language_id: "rust".to_string(),
                version: 1,
                // TODO: Allow which text as param to helper?
                text: fs::read_to_string("tests/data/rust_diff.magit_status").unwrap(),
            },
        }
    }

    #[test]
    fn test_get_initialization_params() {
        let path: PathBuf = "tests/data/full_go_diff.code_review".into();
        let (cwd, worktree, file_types) = read_initialization_params_from_tempfile(&path).unwrap();
        println!("types: {:?}", file_types);
        assert_eq!("/home/chris/gtdbot/".to_string(), cwd);
        assert!(worktree.is_none());
        assert_eq!(file_types, vec![SupportedFileType::Go])
    }

    #[test]
    fn test_get_initialization_params_with_worktree() {
        let path: PathBuf = "tests/data/worktree_test.init_params".into();
        let (cwd, worktree, file_types) = read_initialization_params_from_tempfile(&path).unwrap();
        
        assert!(cwd.ends_with("/tmp/test_root"));
        assert_eq!(Some("my_worktree".to_string()), worktree);
        assert_eq!(file_types, vec![SupportedFileType::Rust]);
    }

    #[allow(dead_code)]
    pub fn get_open_params_go(uri: Url) -> tower_lsp::lsp_types::DidOpenTextDocumentParams {
        DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: (uri),
                language_id: "go".to_string(),
                version: 1,
                text: fs::read_to_string("tests/data/go_diff.magit_status").unwrap(),
            },
        }
    }

    //#[tokio::test]
    #[allow(dead_code)]
    async fn test_end_to_end_rust_analyzer() {
        // Note this test depends on the environment having rust-analyzer installed and on the path.
        let diff_text = fs::read_to_string("tests/data/rust_diff.magit_status").unwrap();
        let diff = MagitDiff::parse(&diff_text).unwrap();
        let root: String = expanduser("~/diff-lsp").unwrap().display().to_string();

        assert_eq!(
            diff.headers.get(&DiffHeader::Buffer),
            Some(&"diff-lsp".to_string())
        );

        let backends = create_backends_map(vec![SupportedFileType::Rust], &root).expect("failed to create backends");
        let (service, _socket) =
            // TODO: This no longer sets the diff to RAW_MAGIT_DIFF_RUST
            LspService::new(|client| DiffLsp::new(client, backends, root));

        // TODO make relative and include in project.
        let url = Url::from_file_path("/Users/chrishipple/test7.diff-test").unwrap();
        let hover_request = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: (TextDocumentIdentifier { uri: url.clone() }),
                position: Position {
                    line: 17,
                    character: 15,
                },
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
        };

        let _init_res = service.inner().initialize(get_init_params()).await.unwrap();

        info!("_init_res: {:?}", _init_res);

        service.inner().initialized(InitializedParams {}).await;
        service.inner().did_open(get_open_params_rust(url)).await;

        let hover_result = service.inner().hover(hover_request).await.unwrap().unwrap();
        info!("{:?}", hover_result);
    }

    //#[tokio::test]
    #[allow(dead_code)]
    async fn test_end_to_end_gopls() {
        // Note this test depends on the environment having gopls installed and on the path.
        let diff_text = fs::read_to_string("tests/data/go_diff.magit_status").unwrap();
        let diff = MagitDiff::parse(&diff_text).unwrap();
        let root: String = expanduser("~/diff-lsp").unwrap().display().to_string();
        info!("Root is {:?}", root);

        assert_eq!(
            diff.headers.get(&DiffHeader::Buffer),
            Some(&"lsp-example".to_string())
        );
        let backends = create_backends_map(vec![SupportedFileType::Go], &root).expect("failed to create backends");
        let (service, _socket) =
            // TODO: This no longer sets the diff to raw go diff
            LspService::new(|client| DiffLsp::new(client, backends, root));

        // TODO make relative and include in project.
        let url = Url::from_file_path("/Users/chrishipple/lsp-example/main.go").unwrap();
        let hover_request = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: (TextDocumentIdentifier { uri: url.clone() }),
                position: Position {
                    line: 18, // 0 index but emacs is 1 indexed, subtract 1 to match (inside hover func)
                    character: 5,
                },
            },
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
        };

        let _init_res = service.inner().initialize(get_init_params()).await.unwrap();

        info!("_init_res: {:?}", _init_res);

        service.inner().initialized(InitializedParams {}).await;
        service.inner().did_open(get_open_params_go(url)).await;

        let hover_result = service.inner().hover(hover_request).await.unwrap().unwrap();
        info!("{:?}", hover_result);
    }
    // TODO move to lib.rs but having trouble importing test_data there
    #[test]
    fn test_parse_go_magit_diff() {
        let diff_text = fs::read_to_string("tests/data/go_diff.magit_status").unwrap();
        let parsed_diff = MagitDiff::parse(&diff_text).unwrap();
        assert_eq!(
            parsed_diff.headers.get(&DiffHeader::Buffer),
            Some(&"lsp-example".to_string())
        );
        assert_eq!(
            parsed_diff.headers.get(&DiffHeader::Type),
            Some(&"magit-status".to_string())
        );
        assert_eq!(
            parsed_diff.headers.get(&DiffHeader::Project),
            Some(&"magit: lsp-example".to_string())
        );
    }
}
