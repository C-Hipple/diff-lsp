use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::{Client, LanguageServer, LspService, Server};
use tower_lsp::lsp_types::*;
use tower_lsp::lsp_types::notification::Notification;

mod client;

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
struct DiffLsp {
    client: Client,
    //my_cliet: client::LspClient,
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
        self.client.log_message(MessageType::INFO, "Initialized!").await;
    }


    async fn shutdown(&self) -> Result<()> {
        self.client.log_message(MessageType::INFO, "Shutting Down.  Cya next time!").await;
        Ok(())
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        if params.command == "custom.notification" {
            self.client
                .send_notification::<CustomNotification>(CustomNotificationParams::new("Hello", "Message"))
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

    async fn hover(&self, _params: HoverParams) -> Result<Option<Hover>> {
        let output = Hover {
            contents: HoverContents::Scalar(MarkedString::from_markdown("Hover Text".to_string())),
            range: None
        };
        Ok(Some(output))
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        println!("Opened document: {:?}", params);

    }

    // async fn references(&self, _params: ReferenceParams) -> Result<Option<Vec<Location>>>{
    //     unimplemented!("Getting references not yet implemented.")
    // }

    // async fn goto_definition(&self, _params: GotoDefinitionParams) -> Result<Option<GotoDefinitionResponse>> {
    //     unimplemented!("goto_definition not yet implemented.")
    // }

}


#[tokio::main]
async fn main() {
    println!("Hello, world!");

    let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());
    let (service, socket) = LspService::new(|client| DiffLsp { client });

    println!("Socket is: {socket:?}");
    let mut client = client::LspClient::new(
        //"rust-analyzer".to_string()
        "gopls".to_string()
        //"top".to_string()
        //"pwd".to_string()
    );

    let init_res = client.initialize().unwrap();
    println!("init res was: {init_res:?}");

    println!("Goodbye world.");
    Server::new(stdin, stdout, socket).serve(service).await;
}
