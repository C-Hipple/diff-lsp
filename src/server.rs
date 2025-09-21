use serde::{Deserialize, Serialize};
use std::fs::read_to_string;
use strum::IntoEnumIterator;

use log::info;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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
use crate::utils::{fetch_origin_nonblocking, get_unique_elements};

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
) -> HashMap<SupportedFileType, &mut client::ClientForBackendServer> {
    let mut backends: HashMap<SupportedFileType, &mut client::ClientForBackendServer> =
        HashMap::new();

    info!("creating backend map for langs: {:?}", active_langs);
    for supported_lang in SupportedFileType::iter() {
        if active_langs.contains(&supported_lang) {
            let (command, args) = get_lsp_for_file_type(supported_lang);
            info!("Starting client for server: {:?}", command);
            backends.insert(
                supported_lang,
                &mut client::ClientForBackendServer::new(command, args, dir),
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
pub struct DiffLsp<'a> {
    pub client: Client,
    pub backends: Mutex<HashMap<SupportedFileType, &'a mut client::ClientForBackendServer>>,
    pub diff_map: Mutex<HashMap<Url, ParsedDiff>>,
    pub root: String, // The project root, without a trailing slash.  ~/diff-lsp for example
}

impl<'a> DiffLsp<'a> {
    pub fn new(
        client: Client,
        backends: HashMap<SupportedFileType, &'a mut client::ClientForBackendServer>,
        root: String,
    ) -> Self {
        let server = DiffLsp {
            client,
            backends: Mutex::new(backends),
            diff_map: Mutex::new((|| {
                // TODO Actually set diff during textDocument/didOpen
                let map: HashMap<Url, ParsedDiff> = HashMap::new();
                // let diff_path = expanduser("~/.diff-lsp-tempfile").unwrap();

                // let contents = fs::read_to_string(diff_path.clone()).unwrap();
                // let diff = ParsedDiff::parse(&contents);
                // let str_diff_path = diff_path.to_str().unwrap();
                // map.insert(Url::from_file_path(str_diff_path).unwrap(), diff.unwrap());
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

    async fn get_backend(&self, source_map: &SourceMap) -> Option<&mut client::ClientForBackendServer> {
        info!("Backends available: {:?}", self.backends);
        if let Some(backend) = self.backends.lock().await.get(&source_map.file_type) {
            return Some(*backend)

        }
        None
    }

    async fn get_diff(&self, uri: &Url) -> Option<ParsedDiff> {
        let map = self.diff_map.lock().await;
        map.get(&uri).cloned()
    }

    async fn get_source_map(&self, text_params: TextDocumentPositionParams) -> Option<SourceMap> {
        let line = text_params.position.line.try_into().unwrap();
        return self
            .line_to_source_map(text_params.text_document.uri.clone(), line)
            .await;
    }

    async fn line_to_source_map(&self, uri: Url, line_num: u16) -> Option<SourceMap> {
        if let Some(diff) = self.get_diff(&uri).await {
            // info!("Found the diff at URI: {:?}", uri.clone());
            info!("Used diff line count {:?}", diff.lines_map.len());
            info!("Used diff line parsed at {:?}", diff.parsed_at);
            info!("Used source with total lines: {:?}", diff.total_lines);
            return diff.map_diff_line_to_src(line_num);
        }
        info!("Failed to find diff at URI: {:?}", uri.clone());
        None
    }

    async fn refresh_file(&self, uri: &Url) -> Option<ParsedDiff> {
        let real_path = uri.path();
        info!("Calling refresh_file for the real path: {:?}", real_path);

        let contents = fs::read_to_string(real_path).unwrap();
        if let Some(diff) = ParsedDiff::parse(&contents) {
            info!("Inserting diff! 2");
            let mut diff_map = self.diff_map.lock().await;
            // self.backends.insert(k, v);

            if let Some(diff_before) = diff_map.get(&uri) {
                info!("Diff before len: {:?}", diff_before.lines_map.len());
            }
            info!("Diff new len: {:?}", diff.lines_map.len());

            diff_map.insert(uri.clone(), diff.clone()); // Use the *same* lock to insert
            Some(diff)
        } else {
            None
        }
    }
}

#[tower_lsp::async_trait]
impl<'a> LanguageServer for DiffLsp<'a> {
    async fn initialize(&self, _: InitializeParams) -> LspResult<InitializeResult> {
        self.client
            .log_message(MessageType::WARNING, "Cruising")
            .await;
        info!("Starting initialize");
        // let locked = self.backends.lock().await;
        for backend in self.backends.lock().await.values_mut() {
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
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["custom.notification".to_string()],
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
                // document_symbol_provider: Some(OneOf::Left(true)),
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
        info!("Doing command with params: {:?}", params);
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
        } else if params.command == "refresh" {
            let keys = {
                // unlocks when the reference goes out of scope
                let diff_map = self.diff_map.lock().await;
                diff_map.keys().cloned().collect::<Vec<_>>()
            };
            for key in keys {
                info!("Starting refresh of {:?}", key);
                self.refresh_file(&key).await;
                info!("Finished refresh of {:?}", key);
            }
            fetch_origin_nonblocking(&self.root);
            Ok(None)
        } else if params.command == "fetch" {
            let mut child = fetch_origin_nonblocking(&self.root);
            let _ = child.wait().await;
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
        info!(
            "Doing hover: {:?}-{:?}",
            params.text_document_position_params.position.line,
            params.text_document_position_params.position.character
        );
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

        info!(
            "source map: {:?} - {:?}",
            source_map.source_line, source_map.source_line_text
        );
        // let backend_mutex_res = self.get_backend(&source_map);
        let backend = match self.get_backend(&source_map) {
            Some(bm) => bm,
            None => {
                info!("No found backend mutex!");
                return Err(LspError::new(ErrorCode::ServerError(1)));
            }
        };
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

        // info!("Hover mapped params: {:?}", mapped_params);
        let hov_res = backend.hover(mapped_params);
        info!("Hover res: {:?}", hov_res);
        match hov_res {
            Ok(res) => Ok(res),
            Err(_) => Err(LspError::new(ErrorCode::ServerError(1))), // Translating LspError type
        }
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        // info!("Opened document: {:?}", params);  // uncomment to show that we get diff-test as our did open, but we send the real file to the backend
        if let Some(diff) = self.refresh_file(&params.text_document.uri).await {
            let filtered_files: Vec<String> = diff.filenames.clone().into_iter().unique().collect();
            for filename in filtered_files {
                let filetype = SupportedFileType::from_filename(filename.clone());

                if let None = filetype {
                    continue;
                }

                if let Some(backend) = self.backends.lock().await.get(&filetype.unwrap()) {
                    // let mut backend = backend_mutex.lock().await;
                    let mut these_params = params.clone();
                    // Here we need to break the LSP contract and use the originator's didOpen URI to read the contents of the file.

                    // 2025-07-07 22:03:47 - INFO - Calling Did open to "rust-analyzer" for file "file:///home/chris/diff-lsp/src/server.rs";

                    these_params.text_document.uri =
                        uri_from_relative_filename(self.root.clone(), &filename);

                    // lol back to a regular filename
                    let full_path = these_params
                        .text_document
                        .uri
                        .as_str()
                        .replace("file://", "");
                    info!("Opening filename: {:?}", full_path);
                    let text = fs::read_to_string(full_path).unwrap();
                    these_params.text_document.text = text;
                    info!(
                        "Calling Did open to {:?} for file {:?};",
                        backend.lsp_command,
                        these_params.text_document.uri.as_str(),
                        // these_params.text_document.text
                    );
                    backend.did_open(&these_params);
                } else {
                }
            }
        }
        info!("Finished did_open");
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        info!("Calling did_change {:?}", params)
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        info!("Calling did_close {:?}", params);
        self.refresh_file(&params.text_document.uri).await;
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
