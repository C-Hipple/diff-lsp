// use crate::test_data::RAW_MAGIT_DIFF_RUST;
use crate::test_data::*;

#[cfg(test)]
mod tests {

    use tower_lsp::LanguageServer;
    use diff_lsp::server::get_backends_map;
    use diff_lsp::server::DiffLsp;
    use diff_lsp::{DiffHeader, MagitDiff, Parsable};
    use expanduser::expanduser;
    use log::info;
    use tower_lsp::lsp_types::*;
    // use super::*;
    use tower_lsp::LspService;

    //#[tokio::test]
    #[allow(dead_code)]
    async fn test_end_to_end_rust_analyzer() {
        // Note this test depends on the environment having rust-analyzer installed and on the path.
        let diff = MagitDiff::parse(RAW_MAGIT_DIFF_RUST).unwrap();
        let root: String = expanduser("~/diff-lsp").unwrap().display().to_string();

        assert_eq!(
            diff.headers.get(&DiffHeader::Buffer),
            Some(&"diff-lsp".to_string())
        );

        let backends = get_backends_map(&root);
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

        let _init_res = service
            .inner()
            .initialize(test_data::get_init_params())
            .await
            .unwrap();

        info!("_init_res: {:?}", _init_res);

        service.inner().initialized(InitializedParams {}).await;
        service
            .inner()
            .did_open(test_data::get_open_params_rust(url))
            .await;

        let hover_result = service.inner().hover(hover_request).await.unwrap().unwrap();
        info!("{:?}", hover_result);
    }

    //#[tokio::test]
    #[allow(dead_code)]
    async fn test_end_to_end_gopls() {
        // Note this test depends on the environment having gopls installed and on the path.
        let diff = MagitDiff::parse(test_data::RAW_MAGIT_DIFF_GO).unwrap();
        let root: String = expanduser("~/diff-lsp").unwrap().display().to_string();
        info!("Root is {:?}", root);

        assert_eq!(
            diff.headers.get(&DiffHeader::Buffer),
            Some(&"lsp-example".to_string())
        );
        let backends = get_backends_map(&root);
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

        let _init_res = service
            .inner()
            .initialize(test_data::get_init_params())
            .await
            .unwrap();

        info!("_init_res: {:?}", _init_res);

        service.inner().initialized(InitializedParams {}).await;
        service
            .inner()
            .did_open(test_data::get_open_params_go(url))
            .await;

        let hover_result = service.inner().hover(hover_request).await.unwrap().unwrap();
        info!("{:?}", hover_result);
    }
    // TODO move to lib.rs but having trouble importing test_data there
    #[test]
    fn test_parse_go_magit_diff() {
        let parsed_diff = MagitDiff::parse(test_data::RAW_MAGIT_DIFF_GO).unwrap();
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
