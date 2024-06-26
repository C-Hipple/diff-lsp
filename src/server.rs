use diff_lsp::SourceMap;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use std::fs;

use expanduser::expanduser;
use itertools::Itertools;
use serde_json::Value;
use tower_lsp::jsonrpc::{Error, ErrorCode, Result};
use tower_lsp::lsp_types::notification::Notification;
use tower_lsp::lsp_types::*;
use tower_lsp::LspService;
use tower_lsp::{Client, LanguageServer};

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::client;
use crate::SupportedFileType;

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

pub fn get_backends_map() -> HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>>
{
    let rust_analyzer = client::ClientForBackendServer::new("rust-analyzer".to_string());
    println!("started rust-analyzer.");
    // let gopls = client::ClientForBackendServer::new("gopls".to_string());
    // // MAYBE global pylsp :/ ?
    // let pylsp = client::ClientForBackendServer::new("pylsp".to_string());

    let mut backends: HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>> =
        HashMap::new();

    backends.insert(SupportedFileType::Rust, Arc::new(Mutex::new(rust_analyzer)));
    // backends.insert(SupportedFileType::Go, Arc::new(Mutex::new(gopls)));
    // backends.insert(SupportedFileType::Python, Arc::new(Mutex::new(pylsp)));
    backends
}

#[derive(Debug)]
pub struct DiffLsp {
    pub client: Client,
    pub backends: HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>>,
    //pub diff:   // Implements the mapping functions too?
    pub diff: Option<diff_lsp::MagitDiff>,
    pub root: String, // The project root, without a trailing slash.  ~/diff-lsp for example
}

impl DiffLsp {
    pub fn new(
        client: Client,
        backends: HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>>,
        diff: Option<diff_lsp::MagitDiff>,
        root: String,
    ) -> Self {
        // Hacky that if diff is None, then we want to read our hardcoded file.
        if let Some(my_diff) = diff {
            return DiffLsp {
                client,
                backends,
                diff: Some(my_diff),
                root,
            };
        }

        // TODO Actually set diff during textDocument/didOpen
        let contents = fs::read_to_string(expanduser("~/test7.diff-test").unwrap()).unwrap();
        let diff = diff_lsp::MagitDiff::parse(&contents);

        DiffLsp {
            client,
            backends,
            diff,
            root,
        }
    }

    fn get_backend(
        &self,
        source_map: &SourceMap,
    ) -> Option<&Arc<Mutex<client::ClientForBackendServer>>> {
        self.backends.get(&source_map.file_type)
    }

    fn get_source_map(&self, line_num: u16) -> Option<SourceMap> {
        self.diff.as_ref().unwrap().map_diff_line_to_src(line_num)
    }

    #[allow(dead_code)]
    fn set_diff(&mut self, diff: diff_lsp::MagitDiff) {
        self.diff = Some(diff)
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for DiffLsp {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        for backend_mutex in self.backends.values().into_iter() {
            let mut backend = backend_mutex.lock().await;
            println!(
                "Diff LSP doing initialize for backend: {:?}",
                backend.lsp_command
            );
            let res = backend.initialize().unwrap();
            return Ok(res);
        }

        Ok(InitializeResult {
            server_info: None,
            capabilities: ServerCapabilities {
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["custom.notification".to_string()],
                    work_done_progress_options: Default::default(),
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                ..ServerCapabilities::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        for backend_mutex in self.backends.values().into_iter() {
            let mut backend = backend_mutex.lock().await;
            println!(
                "Diff LSP doing initialized for backend: {:?}",
                backend.lsp_command
            );
            backend.initialized();
        }
        self.client
            .log_message(MessageType::INFO, "Initialized!")
            .await;
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
        let line: u16 = params
            .text_document_position_params
            .position
            .line
            .try_into()
            .unwrap();
        let source_map = self.get_source_map(line).unwrap();
        let backend_mutex = self.get_backend(&source_map).unwrap();
        let mut backend = backend_mutex.lock().await;
        let mut mapped_params = params.clone();
        mapped_params
            .text_document_position_params
            .text_document
            .uri = diff_lsp::uri_from_relative_filename(self.root.clone(), &source_map.file_name);
        mapped_params.text_document_position_params.position.line = source_map.source_line.into();
        let hov_res = backend.hover(mapped_params);
        println!("hov_res: {:?}", hov_res);
        match hov_res {
            Ok(res) => Ok(res),
            Err(_) => Err(Error::new(ErrorCode::ServerError(1))), // Translating Error type
        }
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        // println!("Opened document: {:?}", params);  // uncomment to show that we get diff-test as our did open, but we send the real file to the backend
        let _real_path = params.text_document.uri.path();
        // get all the paths from all the diffs

        let mut files = vec![];
        if let Some(diff) = &self.diff {
            for hunk in &diff.hunks {
                files.push(hunk.filename.clone());
            }
        }

        // not sure how to type hint the Vec<String> doing this in the loop constructor
        let filtered_files: Vec<String> = files.into_iter().unique().collect();
        for filename in filtered_files {
            let filetype = SupportedFileType::from_filename(filename.clone()).unwrap();
            if let Some(backend_mutex) = self.backends.get(&filetype) {
                let mut backend = backend_mutex.lock().await;
                let mut these_params = params.clone();
                // Here we need to break the LSP contract and use the originator's didOpen URI to read the contents of the file.

                let text = fs::read_to_string(filename.clone()).unwrap();

                these_params.text_document.uri =
                    diff_lsp::uri_from_relative_filename(self.root.clone(), &filename);
                these_params.text_document.text = text;
                println!(
                    "Calling Did open to {:?} for file {:?}; with text: {:?}",
                    backend.lsp_command,
                    these_params.text_document.uri.path(),
                    these_params.text_document.text
                );
                backend.did_open(&these_params);
            }
        }
    }

    async fn did_change(&self, _params: DidChangeTextDocumentParams) {
        println!("Calling did_change")
    }

    async fn did_close(&self, _params: DidCloseTextDocumentParams) {
        println!("Calling did_close")
    }

    // async fn references(&self, _params: ReferenceParams) -> Result<Option<Vec<Location>>>{
    //     unimplemented!("Getting references not yet implemented.")
    // }

    // async fn goto_definition(&self, _params: GotoDefinitionParams) -> Result<Option<GotoDefinitionResponse>> {
    //     unimplemented!("goto_definition not yet implemented.")
    // }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::test_data::{self};
    use diff_lsp::MagitDiff;
    #[tokio::test]
    async fn end_to_end_test() {
        // Note this test depends on the environment having rust-analyzer installed and on the path.
        let diff = MagitDiff::parse(test_data::RAW_MAGIT_DIFF).unwrap();
        let root: String = expanduser("~/diff-lsp").unwrap().display().to_string();

        assert_eq!(
            diff.headers.get(&diff_lsp::DiffHeader::Buffer),
            Some(&"diff-lsp".to_string())
        );

        let backends = get_backends_map();
        let (service, _socket) =
            LspService::new(|client| DiffLsp::new(client, backends, Some(diff), root));

        // TODO make relative and include in project.
        let url = Url::from_file_path("/Users/chrishipple/test7.diff-test").unwrap();
        let hover_request = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: (TextDocumentIdentifier { uri: url.clone() }),
                position: Position {
                    line: 19,
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
        println!("_init_res: {:?}", _init_res);

        service.inner().initialized(InitializedParams {}).await;

        service.inner().did_open(test_data::get_open_params(url)).await;

        let _hover_result = service.inner().hover(hover_request).await.unwrap().unwrap();
        assert_eq!(1, 2);

    }
}
