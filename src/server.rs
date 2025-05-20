use serde::{Deserialize, Serialize};

use log::info;
use std::collections::HashMap;
use std::fs;

use expanduser::expanduser;
use itertools::Itertools;
use serde_json::Value;
use tower_lsp::jsonrpc::{Error, ErrorCode, Result};
use tower_lsp::lsp_types::notification::Notification;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::client;
use crate::SupportedFileType;
use crate::*;

#[derive(Debug, Deserialize, Serialize)]
struct CustomNotificationParams {
    title: String,
    message: String,
}

impl CustomNotificationParams {
    fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        CustomNotificationParams {
            title: title.into(),
            message: message.into(),
        }
    }
}

enum CustomNotification {}

impl Notification for CustomNotification {
    type Params = CustomNotificationParams;

    const METHOD: &'static str = "custom/notification";
}

pub fn get_backends_map(
    dir: &str,
) -> HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>> {
    //let rust_analyzer = client::ClientForBackendServer::new("rust-analyzer".to_string());
    // info!("started rust-analyzer.");
    let gopls = client::ClientForBackendServer::new("gopls".to_string(), dir);
    // // MAYBE global pylsp :/ ?
    // let pylsp = client::ClientForBackendServer::new("pylsp".to_string());

    let mut backends: HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>> =
        HashMap::new();

    //backends.insert(SupportedFileType::Rust, Arc::new(Mutex::new(rust_analyzer)));
    backends.insert(SupportedFileType::Go, Arc::new(Mutex::new(gopls)));
    // backends.insert(SupportedFileType::Python, Arc::new(Mutex::new(pylsp)));
    backends
}

#[derive(Debug)]
pub struct DiffLsp {
    pub client: Client,
    pub backends: HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>>,
    //pub diff:   // Implements the mapping functions too?
    // pub diff: Option<ParsedDiff>,
    pub diff_map: Mutex<HashMap<Url, ParsedDiff>>,
    pub root: String, // The project root, without a trailing slash.  ~/diff-lsp for example
}

impl DiffLsp {
    pub fn new(
        client: Client,
        backends: HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>>,
        root: String,
    ) -> Self {
        let server = DiffLsp {
            client,
            backends,
            diff_map: Mutex::new((|| {
                // TODO Actually set diff during textDocument/didOpen
                let mut map: HashMap<Url, ParsedDiff> = HashMap::new();
                // let diff_path = expanduser("~/lsp-example/test6.diff-test").unwrap();
                // let diff_path = expanduser("~/lsp-example/code-review.diff-test").unwrap();
                let diff_path = expanduser("~/gtdbot/diff-lsp-status.diff-test").unwrap();

                let contents = fs::read_to_string(diff_path.clone()).unwrap();
                let diff = MagitDiff::parse(&contents); // TODO determine type from contents
                let str_diff_path = diff_path.to_str().unwrap();
                map.insert(Url::from_file_path(str_diff_path).unwrap(), diff.unwrap());
                map
            })()),
            // Lazydiff_map,
            root,
        };

        // server.diff_map.insert(
        //     Url::from_file_path(str_diff_path).unwrap(),
        //     diff.unwrap(),
        // );

        info!("Starting server: {:?}", server);
        server
    }

    fn get_backend(
        &self,
        source_map: &SourceMap,
    ) -> Option<&Arc<Mutex<client::ClientForBackendServer>>> {
        self.backends.get(&source_map.file_type)
    }

    async fn get_diff(&self, uri: Url) -> Option<ParsedDiff> {
        let map = self.diff_map.lock().await;
        let res = map.get(&uri).cloned();
        // info!("Searched for the diff via {:?}, got {:?}",
        //     uri.as_str(),
        //     res
        // );
        res
    }

    async fn get_source_map(&self, text_params: TextDocumentPositionParams) -> Option<SourceMap> {
        let line: u16 = text_params.position.line.try_into().unwrap();
        return self
            .line_to_source_map(text_params.text_document.uri, line)
            .await;
    }

    async fn line_to_source_map(&self, uri: Url, line_num: u16) -> Option<SourceMap> {
        if let Some(diff) = self.get_diff(uri).await {
            return diff.map_diff_line_to_src(line_num);
        }
        None
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for DiffLsp {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        self.client
            .log_message(MessageType::WARNING, "Cruising")
            .await;
        info!("Starting initialize");
        for backend_mutex in self.backends.values().into_iter() {
            let mut backend = backend_mutex.lock().await;
            info!(
                "Diff LSP doing initialize for backend: {:?}",
                backend.lsp_command
            );
            backend.initialize().unwrap();
            //info!("Result for that initialize: {:?}", res);
            //return Ok(res);
        }

        let res = Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "diff-lsp".to_string(),
                version: Some("0.1.0".to_string()),
            }),
            capabilities: ServerCapabilities {
                execute_command_provider: None,
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
            ..Default::default()
        });
        info!("Finished initialize! {:?}", res.clone().unwrap());
        // let init_task = tokio::task::spawn(async {
        //     tokio::time::sleep(tokio::time::Duration::from_secs(1));
        //     self.client.send_notification::<Initialized>(InitializedParams{}).await;
        // }
        // );
        res
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("Starting Initialized");
        for backend_mutex in self.backends.values().into_iter() {
            let mut backend = backend_mutex.lock().await;
            info!(
                "Diff LSP doing initialized for backend: {:?}",
                backend.lsp_command
            );
            backend.initialized();
        }
        info!("Finished all initialized");
    }

    async fn shutdown(&self) -> Result<()> {
        self.client
            .log_message(MessageType::INFO, "Shutting Down.  Cya next time!")
            .await;
        Ok(())
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        if params.command == "custom.notification" {
            self.client
                .send_notification::<CustomNotification>(CustomNotificationParams::new(
                    "Hello", "Message",
                ))
                .await;
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("Command execute with params: {params:?}"),
                )
                .await;
            Ok(None)
        } else {
            Err(Error::invalid_request())
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        //  let output = Hover {
        //      contents: HoverContents::Scalar(MarkedString::from_markdown("Hover Text".to_string())),
        //      range: None,
        // };
        info!("Doing hover: {:?}", params);
        let source_map_res = self
            .get_source_map(params.text_document_position_params.clone())
            .await;
        let source_map = match source_map_res {
            Some(sm) => sm,
            None => return Err(Error::new(ErrorCode::ServerError(1))),
        };

        info!("source map: {:?}", source_map);
        let backend_mutex_res = self.get_backend(&source_map);
        let backend_mutex = match backend_mutex_res {
            Some(bm) => bm,
            None => return Err(Error::new(ErrorCode::ServerError(1))),
        };
        let mut backend = backend_mutex.lock().await;
        // TODO do all this mapping in an async func since there's a lot of cloning and whatnot and then futures::join! it with the backend_mutex
        let mut mapped_params = params.clone();
        let uri = uri_from_relative_filename(self.root.clone(), &source_map.file_name);

        mapped_params
            .text_document_position_params
            .text_document
            .uri = uri;
        mapped_params.text_document_position_params.position.line = source_map.source_line.into();

        if source_map.source_line_type != LineType::Unmodified {
            // this is a problem for 1 letter variables since emacs won't send the hover request
            // for whitespace, even if it would get mapped to the correct position
            // Account for the + or - at the start of the line
            mapped_params
                .text_document_position_params
                .position
                .character -= 1
        }

        // Accounting for this elsewhere
        //mapped_params.text_document_position_params.position.line += 1;  // the lsp client is 0 indexing
        info!("Hover mapped params: {:?}", mapped_params);
        let hov_res = backend.hover(mapped_params);
        match hov_res {
            Ok(res) => Ok(res),
            Err(_) => Err(Error::new(ErrorCode::ServerError(1))), // Translating Error type
        }
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        // info!("Opened document: {:?}", params);  // uncomment to show that we get diff-test as our did open, but we send the real file to the backend
        let real_path = params.text_document.uri.path();
        // let real_path = params.text_document.text.clone();

        // get all the paths from all the diffs

        info!(
            "Calling did_open {:?} for the real path: {:?}",
            params, real_path
        );

        let contents = fs::read_to_string(real_path).unwrap();
        let diff = MagitDiff::parse(&contents).unwrap();

        let mut files = vec![]; // Use the filenames to only send the file(s) of the changed files to their respective LSPs.
        for hunk in &diff.hunks {
            files.push(hunk.filename.clone());
        }

        self.diff_map
            .lock()
            .await
            .insert(params.text_document.uri.clone(), diff);

        // not sure how to type hint the Vec<String> doing this in the loop constructor
        let filtered_files: Vec<String> = files.into_iter().unique().collect();
        // TODO filter by filetype?
        for filename in filtered_files {
            let filetype = SupportedFileType::from_filename(filename.clone());

            if let None = filetype {
                continue;
            }

            if let Some(backend_mutex) = self.backends.get(&filetype.unwrap()) {
                let mut backend = backend_mutex.lock().await;
                let mut these_params = params.clone();
                // Here we need to break the LSP contract and use the originator's didOpen URI to read the contents of the file.

                info!("filename: {:?}", filename.clone());
                let full_path = self.root.clone() + "/" + &filename;
                let text = fs::read_to_string(full_path).unwrap();

                these_params.text_document.uri =
                    uri_from_relative_filename(self.root.clone(), &filename);
                these_params.text_document.text = text;
                info!(
                    "Calling Did open to {:?} for file {:?}; with text: {:?}",
                    backend.lsp_command,
                    these_params.text_document.uri.as_str(),
                    these_params.text_document.text
                );
                backend.did_open(&these_params);
            }
        }
        info!("Finished did_open");
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        info!("Calling did_change {:?}", params)
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        info!("Calling did_close {:?}", params)
    }

    async fn references(&self, _params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        info!("Getting references not yet implemented.");
        Ok(None)
    }

    async fn goto_definition(
        &self,
        _params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        //info!("goto_definition not yet implemented.");

        let source_map_res = self
            .get_source_map(_params.text_document_position_params.clone())
            .await;
        let source_map = match source_map_res {
            Some(sm) => sm,
            None => return Err(Error::new(ErrorCode::ServerError(1))),
        };

        let mut mapped_params = _params.clone();

        let backend_mutex_res = self.get_backend(&source_map);
        let backend_mutex = match backend_mutex_res {
            Some(bm) => bm,
            None => return Err(Error::new(ErrorCode::ServerError(1))),
        };

        let mut backend = backend_mutex.lock().await;

        let uri = uri_from_relative_filename(self.root.clone(), &source_map.file_name);

        mapped_params
            .text_document_position_params
            .text_document
            .uri = uri;
        mapped_params.text_document_position_params.position.line = source_map.source_line.into();

        if source_map.source_line_type != LineType::Unmodified {
            // this is a problem for 1 letter variables since emacs won't send the hover request
            // for whitespace, even if it would get mapped to the correct position
            // Account for the + or - at the start of the line
            mapped_params
                .text_document_position_params
                .position
                .character -= 1;
        }
        let goto_def_res = backend.goto_definition(&mapped_params);
        match goto_def_res {
            Ok(res) => Ok(res),
            Err(_) => Err(Error::new(ErrorCode::ServerError(1))), // Translating Error type
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::test_data::*;
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
    use DiffHeader;
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
