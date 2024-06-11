use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

use expanduser::expanduser;
use serde_json::Value;
use tower_lsp::jsonrpc::{Error, ErrorCode, Result};
use tower_lsp::lsp_types::notification::Notification;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};
use tower_lsp::{LspService, Server};

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
    let gopls = client::ClientForBackendServer::new("gopls".to_string());
    // MAYBE global pylsp :/ ?
    let pylsp = client::ClientForBackendServer::new("pylsp".to_string());

    let mut backends: HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>> =
        HashMap::new();

    backends.insert(SupportedFileType::Rust, Arc::new(Mutex::new(rust_analyzer)));
    backends.insert(SupportedFileType::Go, Arc::new(Mutex::new(gopls)));
    backends.insert(SupportedFileType::Python, Arc::new(Mutex::new(pylsp)));
    backends
}

#[derive(Debug)]
pub struct DiffLsp {
    pub client: Client,
    pub backends: HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>>,
    //pub diff:   // Implements the mapping functions too?
    pub diff: Option<diff_lsp::MagitDiff>,
}

impl DiffLsp {
    pub fn new(
        client: Client,
        backends: HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>>,
        diff: Option<diff_lsp::MagitDiff>,
    ) -> Self {
        // Hacky that if diff is None, then we want to read our hardcoded file.
        if let Some(my_diff) = diff {
            return DiffLsp {
                client,
                backends,
                diff: Some(my_diff),
            };
        }

        // TODO Actually set diff during textDocument/didOpen
        let contents = fs::read_to_string(expanduser("~/test7.diff-test").unwrap()).unwrap();
        let diff = diff_lsp::MagitDiff::parse(&contents);

        DiffLsp {
            client,
            backends,
            diff,
        }
    }

    fn get_backend(&self, line_num: u16) -> Option<&Arc<Mutex<client::ClientForBackendServer>>> {
        if let Some(source_map) = self.diff.as_ref().unwrap().map_diff_line_to_src(line_num) {
            let backend = self.backends.get(&source_map.file_type);
            backend
        } else {
            None
        }
    }

    #[allow(dead_code)]
    fn set_diff(&mut self, diff: diff_lsp::MagitDiff) {
        self.diff = Some(diff)
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for DiffLsp {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
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
        let backend_mutex = self.get_backend(line).unwrap();
        let mut backend = backend_mutex.lock().await;
        let hov_res = backend.hover(params);
        println!("hov_res: {:?}", hov_res);
        match hov_res {
            Ok(res) => Ok(res),
            Err(_) => Err(Error::new(ErrorCode::ServerError(1))), // Translating Error type
        }
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        println!("Opened document: {:?}", params);
        let _real_path = params.text_document.uri.path();

        // for (_, value) in self.backends.iter_mut() {
        //     value.did_open(&params);
        // }
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
    use std::borrow::Borrow;

    use super::*;
    use crate::test_data;
    use diff_lsp::MagitDiff;

    #[tokio::test]
    async fn end_to_end_test() {
        let diff = MagitDiff::parse(test_data::RAW_MAGIT_DIFF).unwrap();

        assert_eq!(
            diff.headers.get(&diff_lsp::DiffHeader::Buffer),
            Some(&"diff-lsp".to_string())
        );

        let backends = get_backends_map();
        let (service, socket) =
            LspService::new(|client| DiffLsp::new(client, backends, Some(diff)));

        // TODO make relative and include in project.
        let url = Url::from_file_path("/Users/chrishipple/test7.diff-test").unwrap();
        let hover_request = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: (TextDocumentIdentifier {uri: url}),
                position: Position {
                    line: 13,
                    character: 34,
                },
            },
            work_done_progress_params: WorkDoneProgressParams { work_done_token: None }
        };
        let hover_result = service.inner().hover(hover_request).await.unwrap().unwrap();
    }
}
