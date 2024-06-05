use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types::notification::Notification;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::client;

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

#[derive(Debug)]
pub struct DiffLsp {
    pub client: Client,
    pub backends: HashMap<diff_lsp::SupportedFileType, client::ClientForBackendServer>,
    //pub diff:   // Implements the mapping functions too?
    pub diff: Option<diff_lsp::MagitDiff>,
}

impl DiffLsp {
    pub fn new(client: Client, backends: HashMap<diff_lsp::SupportedFileType, client::ClientForBackendServer>) -> Self {
        DiffLsp { client, backends, diff: None }
    }

    #[allow(dead_code)]
    fn get_backend(&self, line_num: u16) -> Option<&client::ClientForBackendServer> {
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
        let output = Hover {
            contents: HoverContents::Scalar(MarkedString::from_markdown("Hover Text".to_string())),
            range: None,
       };
        let line: u16 = params.text_document_position_params.position.line.try_into().unwrap();
        // self.get_backend(line).unwrap()
        //     .hover(params);
        Ok(Some(output))
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
