use std::fs::read_to_string;
use strum::IntoEnumIterator;

use expanduser::expanduser;
use log::{info, warn};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use itertools::Itertools;
use serde_json::Value;
use tower_lsp::jsonrpc::{Error as LspError, ErrorCode, Result as LspResult};
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

pub fn create_backends_map(
    active_langs: Vec<SupportedFileType>,
    dir: &str,
) -> Result<HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>>> {
    let mut backends: HashMap<SupportedFileType, Arc<Mutex<client::ClientForBackendServer>>> =
        HashMap::new();

    for supported_lang in SupportedFileType::iter() {
        if active_langs.contains(&supported_lang) {
            let (command, args) = get_lsp_for_file_type(supported_lang);
            info!("Starting client for server: {:?} in dir {:?}", command, dir);
            backends.insert(
                supported_lang,
                Arc::new(Mutex::new(client::ClientForBackendServer::new(
                    command, args, dir,
                )?)),
            );
        }
    }
    Ok(backends)
}

pub fn read_initialization_params_from_tempfile(
    file_path: &PathBuf,
) -> Result<(String, Option<String>, Vec<SupportedFileType>)> {
    if let Ok(input) = read_to_string(file_path) {
        let mut cwd = String::new();
        let mut worktree: Option<String> = None;
        let mut file_types: Vec<SupportedFileType> = vec![];
        let root_regex = Regex::new(r"^Root:\s(.*)").unwrap();
        let worktree_regex = Regex::new(r"^Worktree:\s(.*)").unwrap();
        let file_regex = Regex::new(r"^(modified|new file|deleted)\s+(.*)").unwrap();
        let diff_git_regex = Regex::new(r"^diff --git\s+(.*)").unwrap();

        for line in input.lines() {
            if let Some(caps) = root_regex.captures(line) {
                cwd = caps.get(1).unwrap().as_str().to_string();
                // break;
            }
            if let Some(caps) = worktree_regex.captures(line) {
                worktree = Some(caps.get(1).unwrap().as_str().to_string());
            }
            if let Some(caps) = file_regex.captures(line) {
                let filename = caps.get(2).unwrap().as_str().to_string();
                if let Some(file_type) = SupportedFileType::from_filename(filename) {
                    file_types.push(file_type);
                }
            } else if let Some(caps) = diff_git_regex.captures(line) {
                // Handle diff --git a/foo.rs b/foo.rs
                // We want the last one, and strip b/
                let files_part = caps.get(1).unwrap().as_str();
                let last_file = files_part.split_whitespace().last().unwrap_or("");
                let filename = last_file
                    .strip_prefix("b/")
                    .or_else(|| last_file.strip_prefix("a/"))
                    .unwrap_or(last_file)
                    .to_string();
                if let Some(file_type) = SupportedFileType::from_filename(filename) {
                    file_types.push(file_type);
                }
            }
        }
        let expanded_cwd = expanduser(cwd)?
            .into_os_string()
            .into_string()
            .map_err(|_| anyhow!("Failed to convert path to string"))?;
        Ok((expanded_cwd, worktree, get_unique_elements(&file_types)))
    } else {
        Err(anyhow!("Unable to read input tempfile"))
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
            diff_map: Mutex::new(HashMap::new()),
            root,
        };
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
        map.get(uri).cloned()
    }

    async fn line_to_source_map(&self, uri: &Url, line_num: u32) -> Option<SourceMap> {
        if let Some(diff) = self.get_diff(uri).await {
            info!("Used diff line count {:?}", diff.lines_map.len());
            info!("Used diff line parsed at {:?}", diff.parsed_at);
            info!("Used source with total lines: {:?}", diff.total_lines);
            return diff.map_diff_line_to_src(line_num);
        }
        info!("Failed to find diff at URI: {:?}", uri);
        None
    }

    /// Maps a position in a diff buffer to the backend that serves the
    /// underlying source file, and the equivalent position in that file.
    ///
    /// Returns `None` if the line isn't part of a hunk, the file type has no
    /// backend, or a URI can't be built for the source file.
    async fn map_position(
        &self,
        params: &TextDocumentPositionParams,
    ) -> Option<(
        Arc<Mutex<client::ClientForBackendServer>>,
        TextDocumentPositionParams,
        SourceMap,
    )> {
        let Some(source_map) = self
            .line_to_source_map(&params.text_document.uri, params.position.line)
            .await
        else {
            info!(
                "Did not find a source map for line {:?}",
                params.position.line
            );
            return None;
        };
        info!(
            "source map: {:?} - {:?}",
            source_map.source_line, source_map.source_line_text
        );

        let backend = self.get_backend(&source_map)?.clone();
        let uri = uri_from_relative_filename(self.root.clone(), &source_map.file_name)?;

        let mut mapped = params.clone();
        mapped.text_document.uri = uri;
        // I'm seeing the right line of my source map when I do actions
        // but without this my hover on the 2nd line of a diff will give
        // the first line, etc.
        // I think there's a + 1 somewhere internally in the LSP servers?
        mapped.position.line = source_map.source_line.0.saturating_sub(1);

        Some((backend, mapped, source_map))
    }

    async fn refresh_file(&self, uri: &Url) -> Option<ParsedDiff> {
        let real_path = uri.path();
        info!("Calling refresh_file for the real path: {:?}", real_path);

        let contents = match fs::read_to_string(real_path) {
            Ok(contents) => contents,
            Err(e) => {
                warn!("Unable to read diff buffer file {:?}: {}", real_path, e);
                return None;
            }
        };
        if let Some(diff) = ParsedDiff::parse(&contents) {
            info!("Inserting diff! 2");
            let mut diff_map = self.diff_map.lock().await;

            if let Some(diff_before) = diff_map.get(uri) {
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
impl LanguageServer for DiffLsp {
    async fn initialize(&self, _: InitializeParams) -> LspResult<InitializeResult> {
        info!("Starting initialize");
        for backend_mutex in self.backends.values() {
            let mut backend = backend_mutex.lock().await;
            info!(
                "Diff LSP doing initialize for backend: {:?}",
                backend.lsp_command
            );
            if let Err(e) = backend.initialize() {
                warn!(
                    "Failed to initialize backend {:?}: {:?}",
                    backend.lsp_command, e
                );
            }
        }

        let res = Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "diff-lsp".to_string(),
                version: Some("0.1.0".to_string()),
            }),
            capabilities: ServerCapabilities {
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        "refresh".to_string(),
                        "fetch".to_string(),
                        "check".to_string(),
                    ],
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
                // document_symbol_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
        });
        info!("Finished initialize! {:?}", res);
        res
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("Starting Initialized");
        for backend_mutex in self.backends.values() {
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
        if params.command == "refresh" {
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
            if let Some(mut child) = fetch_origin_nonblocking(&self.root) {
                let _ = child.wait().await;
            }
            Ok(None)
        } else if params.command == "check" {
            info!("Doing check!");
            for backend_mutex in self.backends.values() {
                let mut backend = backend_mutex.lock().await;
                backend.check_messages();
            }
            Ok(None)
        } else {
            Err(LspError::invalid_request())
        }
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        info!(
            "Doing hover: {:?}-{:?}",
            params.text_document_position_params.position.line,
            params.text_document_position_params.position.character,
        );
        let Some((backend, position, _)) = self
            .map_position(&params.text_document_position_params)
            .await
        else {
            return Err(server_error());
        };
        let mapped_params = HoverParams {
            text_document_position_params: position,
            ..params
        };
        let res = backend.lock().await.hover(mapped_params);
        res.map_err(|_| server_error())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        if let Some(diff) = self.refresh_file(&params.text_document.uri).await {
            let filtered_files: Vec<String> = diff.filenames.clone().into_iter().unique().collect();
            for filename in filtered_files {
                let filetype = SupportedFileType::from_filename(filename.clone());

                let Some(filetype) = filetype else {
                    continue;
                };

                if let Some(backend_mutex) = self.backends.get(&filetype) {
                    // Here we need to break the LSP contract and use the originator's didOpen URI to read the contents of the file.
                    let Some(uri) = uri_from_relative_filename(self.root.clone(), &filename)
                    else {
                        continue;
                    };
                    let mut backend = backend_mutex.lock().await;
                    let mut these_params = params.clone();
                    these_params.text_document.uri = uri;

                    let full_path = these_params
                        .text_document
                        .uri
                        .as_str()
                        .replace("file://", "");
                    info!("Opening filename: {:?}", full_path);
                    let text_result = fs::read_to_string(&full_path);
                    match text_result {
                        Ok(text) => {
                            these_params.text_document.text = text;
                            backend.did_open(&these_params);
                        }
                        Err(_) => {
                            info!("Skipping opening file {:?}", full_path);
                        }
                    }
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

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let Some((backend, position, _)) = self.map_position(&params.text_document_position).await
        else {
            return Err(server_error());
        };
        let mapped_params = ReferenceParams {
            text_document_position: position,
            ..params
        };
        let res = backend.lock().await.references(&mapped_params);
        res.map_err(|_| server_error())
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let Some((backend, position, _)) = self
            .map_position(&params.text_document_position_params)
            .await
        else {
            return Err(server_error());
        };
        let mapped_params = GotoDefinitionParams {
            text_document_position_params: position,
            ..params
        };
        let res = backend.lock().await.goto_definition(&mapped_params);
        res.map_err(|_| server_error())
    }

    async fn goto_type_definition(
        &self,
        params: GotoTypeDefinitionParams,
    ) -> LspResult<Option<GotoTypeDefinitionResponse>> {
        let Some((backend, position, _)) = self
            .map_position(&params.text_document_position_params)
            .await
        else {
            return Err(server_error());
        };
        let mapped_params = GotoTypeDefinitionParams {
            text_document_position_params: position,
            ..params
        };
        let res = backend.lock().await.goto_type_definition(&mapped_params);
        res.map_err(|_| server_error())
    }
}

/// The error currently returned when a request can't be served.
fn server_error() -> LspError {
    LspError::new(ErrorCode::ServerError(1))
}
