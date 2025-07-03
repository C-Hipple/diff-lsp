use serde::{Deserialize, Serialize};
use std::fs::read_to_string;
use strum::IntoEnumIterator;

use log::info;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use expanduser::expanduser;
use itertools::Itertools;
use serde_json::Value;
use tower_lsp::jsonrpc::{Error as LspError, ErrorCode, Result as LspResult};
use tower_lsp::lsp_types::notification::Notification;
use tower_lsp::lsp_types::request::{GotoTypeDefinitionParams, GotoTypeDefinitionResponse};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use anyhow::{anyhow, Result};

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::client;
use crate::utils::get_unique_elements;
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

pub fn create_backends_map(
    active_langs: Vec<SupportedFileType>,
    dir: &str,
) -> HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>> {
    let mut backends: HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>> =
        HashMap::new();

    for supported_lang in SupportedFileType::iter() {
        if active_langs.contains(&supported_lang) {
            let command = get_lsp_for_file_type(supported_lang).unwrap();
            info!("Starting client for server: {:?}", command);
            backends.insert(
                supported_lang,
                Arc::new(Mutex::new(client::ClientForBackendServer::new(
                    command, dir,
                ))),
            );
        }
    }
    backends
}

pub fn read_initialization_params_from_tempfile(
    file_path: &PathBuf,
) -> Result<(String, Vec<SupportedFileType>)> {
    if let Ok(input) = read_to_string(file_path) {
        let mut cwd = String::new();
        let mut file_types: Vec<SupportedFileType> = vec![];
        let root_regex = Regex::new(r"^Root:\s(.*)").unwrap();
        let file_regex = Regex::new(r"^(modified|new file|deleted)\s+(.*)").unwrap();
        for line in input.lines() {
            if let Some(caps) = root_regex.captures(line) {
                cwd = caps.get(1).unwrap().as_str().to_string();
                // break;
            }
            if let Some(caps) = file_regex.captures(line) {
                println!("caps: {:?}", caps.len());
                let filename = caps.get(2).unwrap().as_str().to_string();
                if let Some(file_type) = SupportedFileType::from_filename(filename) {
                    file_types.push(file_type);
                }
            }
        }
        Ok((cwd, get_unique_elements(&file_types)))
    } else {
        return Err(anyhow!("Unable to read input tempfile"));
    }
}

#[derive(Debug)]
pub struct DiffLsp {
    pub client: Client,
    pub backends: HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>>,
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
                let diff_path = expanduser("~/.diff-lsp-tempfile").unwrap();

                let contents = fs::read_to_string(diff_path.clone()).unwrap();
                let diff = ParsedDiff::parse(&contents);
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

    async fn get_diff(&self, uri: &Url) -> Option<ParsedDiff> {
        let map = self.diff_map.lock().await;
        let res = map.get(&uri).cloned();
        // info!("Searched for the diff via {:?}, got {:?}",
        //     uri.as_str(),
        //     res
        // );
        res
    }

    async fn get_source_map(&self, text_params: TextDocumentPositionParams) -> Option<SourceMap> {
        let line = text_params.position.line.try_into().unwrap();
        return self
            .line_to_source_map(text_params.text_document.uri.clone(), line)
            .await;
    }

    async fn line_to_source_map(&self, uri: Url, line_num: u16) -> Option<SourceMap> {
        if let Some(diff) = self.get_diff(&uri).await {
            info!("Found the diff at URI: {:?}", uri.clone());
            return diff.map_diff_line_to_src(line_num);
        }
        None
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for DiffLsp {
    async fn initialize(&self, _: InitializeParams) -> LspResult<InitializeResult> {
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
            //info!("LspResult for that initialize: {:?}", res);
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
                references_provider: Some(OneOf::Left(true)),
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
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

    async fn shutdown(&self) -> LspResult<()> {
        self.client
            .log_message(MessageType::INFO, "Shutting Down.  Cya next time!")
            .await;
        Ok(())
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> LspResult<Option<Value>> {
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
            Err(LspError::invalid_request())
        }
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
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
            None => {
                info!("Did not find a source map for this hover!");
                return Err(LspError::new(ErrorCode::ServerError(1)));
            }
        };

        info!("source map: {:?}", source_map);
        let backend_mutex_res = self.get_backend(&source_map);
        let backend_mutex = match backend_mutex_res {
            Some(bm) => bm,
            None => return Err(LspError::new(ErrorCode::ServerError(1))),
        };
        let mut backend = backend_mutex.lock().await;
        // TODO do all this mapping in an async func since there's a lot of cloning and whatnot and then futures::join! it with the backend_mutex
        let mut mapped_params = params.clone();
        let uri = uri_from_relative_filename(self.root.clone(), &source_map.file_name);

        mapped_params
            .text_document_position_params
            .text_document
            .uri = uri;
        mapped_params.text_document_position_params.position.line = source_map.source_line.0.into();

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
            Err(_) => Err(LspError::new(ErrorCode::ServerError(1))), // Translating LspError type
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
        let diff = ParsedDiff::parse(&contents).unwrap();
        let filtered_files: Vec<String> = diff.filenames.clone().into_iter().unique().collect();

        self.diff_map
            .lock()
            .await
            .insert(params.text_document.uri.clone(), diff);

        // not sure how to type hint the Vec<String> doing this in the loop constructor
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

                info!("Opening filename: {:?}", filename.clone());
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

    async fn references(&self, _params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let mut mapped_params = _params.clone();
        let source_map = self
            .get_source_map(_params.text_document_position)
            .await
            .ok_or(LspError::new(ErrorCode::ServerError(1)))?;

        let backend_mutex_res = self.get_backend(&source_map);
        let backend_mutex = match backend_mutex_res {
            Some(bm) => bm,
            None => return Err(LspError::new(ErrorCode::ServerError(1))),
        };

        let mut backend = backend_mutex.lock().await;

        let uri = uri_from_relative_filename(self.root.clone(), &source_map.file_name);
        mapped_params.text_document_position.text_document.uri = uri;
        mapped_params.text_document_position.position.line = source_map.source_line.0.into();

        if source_map.source_line_type != LineType::Unmodified {
            // this is a problem for 1 letter variables since emacs won't send the hover request
            // for whitespace, even if it would get mapped to the correct position
            // Account for the + or - at the start of the line
            mapped_params.text_document_position.position.character -= 1;
        }

        let references_result = backend.references(&mapped_params);
        match references_result {
            Ok(res) => Ok(res),
            Err(_) => Err(LspError::new(ErrorCode::ServerError(1))), // Translating LspError type
        }
    }

    async fn goto_definition(
        &self,
        _params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        //info!("goto_definition not yet implemented.");

        let source_map = self
            .get_source_map(_params.text_document_position_params.clone())
            .await
            .ok_or(LspError::new(ErrorCode::ServerError(1)))?;

        let mut mapped_params = _params.clone();
        let backend_mutex_res = self.get_backend(&source_map);
        let backend_mutex = match backend_mutex_res {
            Some(bm) => bm,
            None => return Err(LspError::new(ErrorCode::ServerError(1))),
        };

        let mut backend = backend_mutex.lock().await;

        let uri = uri_from_relative_filename(self.root.clone(), &source_map.file_name);

        mapped_params
            .text_document_position_params
            .text_document
            .uri = uri;
        mapped_params.text_document_position_params.position.line = source_map.source_line.0.into();

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
            Err(_) => Err(LspError::new(ErrorCode::ServerError(1))), // Translating LspError type
        }
    }

    async fn goto_type_definition(
        &self,
        params: GotoTypeDefinitionParams,
    ) -> LspResult<Option<GotoTypeDefinitionResponse>> {
        let source_map = self
            .get_source_map(params.text_document_position_params.clone())
            .await
            .ok_or(LspError::new(ErrorCode::ServerError(1)))?;

        let mut mapped_params = params.clone();
        let backend_mutex_res = self.get_backend(&source_map);
        let backend_mutex = match backend_mutex_res {
            Some(bm) => bm,
            None => return Err(LspError::new(ErrorCode::ServerError(1))),
        };

        let mut backend = backend_mutex.lock().await;

        let uri = uri_from_relative_filename(self.root.clone(), &source_map.file_name);

        mapped_params
            .text_document_position_params
            .text_document
            .uri = uri;
        mapped_params.text_document_position_params.position.line = source_map.source_line.0.into();

        if source_map.source_line_type != LineType::Unmodified {
            // this is a problem for 1 letter variables since emacs won't send the hover request
            // for whitespace, even if it would get mapped to the correct position
            // Account for the + or - at the start of the line
            mapped_params
                .text_document_position_params
                .position
                .character -= 1;
        }
        let goto_type_def_res = backend.goto_type_definition(&mapped_params);
        match goto_type_def_res {
            Ok(res) => Ok(res),
            Err(_) => Err(LspError::new(ErrorCode::ServerError(1))), // Translating LspError type
        }
    }
}
