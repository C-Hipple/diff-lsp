use serde::{Deserialize, Serialize};
use std::fs::read_to_string;
use strum::IntoEnumIterator;

use expanduser::expanduser;
use log::{debug, info, trace, warn};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use itertools::Itertools;
use serde_json::Value;
use tower_lsp::jsonrpc::{Error as LspError, Result as LspResult};
use tower_lsp::lsp_types::notification::Notification;
use tower_lsp::lsp_types::request::{GotoTypeDefinitionParams, GotoTypeDefinitionResponse};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use anyhow::{anyhow, Result};

use std::sync::{Arc, LazyLock};
use tokio::sync::{Mutex, RwLock};

use crate::client;
use crate::diagnostics::{
    map_diagnostics_for_diff, publish_diagnostics_from_notification, DiagnosticsMode,
    DiagnosticsStore,
};
use crate::reverse_map::{DiffLocationMapper, DiffLocationsMode, ReverseIndex};
use crate::symbols;
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

pub type BackendHandle = Arc<client::ClientForBackendServer>;
pub type BackendsMap = HashMap<SupportedFileType, BackendHandle>;

/// A backend LSP that could not be started (or initialized).  diff-lsp keeps
/// running without it; the user is told via `window/showMessage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendFailure {
    pub file_type: SupportedFileType,
    pub command: String,
    pub reason: String,
}

impl BackendFailure {
    pub fn user_message(&self) -> String {
        format!(
            "{} {}; {:?} features disabled",
            self.command, self.reason, self.file_type
        )
    }
}

/// Spawns one backend process.  Does not send `initialize`.
pub fn spawn_backend(
    file_type: SupportedFileType,
    dir: &str,
    command_override: Option<(String, Option<String>)>,
) -> std::result::Result<client::ClientForBackendServer, BackendFailure> {
    let (command, args) = command_override.unwrap_or_else(|| get_lsp_for_file_type(file_type));
    info!("Starting client for server: {:?} in dir {:?}", command, dir);
    client::ClientForBackendServer::new(command.clone(), args, dir).map_err(|e| {
        warn!("Failed to start {} for {:?}: {:?}", command, file_type, e);
        BackendFailure {
            file_type,
            command,
            reason: "not found".to_string(),
        }
    })
}

/// Spawns a backend for each of `active_langs`.  A backend that fails to spawn
/// (e.g. the binary isn't installed) is skipped and returned in the failure
/// list instead of aborting the whole server.
pub fn create_backends_map(
    active_langs: Vec<SupportedFileType>,
    dir: &str,
) -> (BackendsMap, Vec<BackendFailure>) {
    let mut backends: BackendsMap = HashMap::new();
    let mut failures = vec![];

    for supported_lang in SupportedFileType::iter() {
        if active_langs.contains(&supported_lang) {
            match spawn_backend(supported_lang, dir, None) {
                Ok(backend) => {
                    backends.insert(supported_lang, Arc::new(backend));
                }
                Err(failure) => failures.push(failure),
            }
        }
    }
    (backends, failures)
}

static ROOT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Root:\s+(.*)").unwrap());
static WORKTREE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Worktree:\s+(.*)").unwrap());
static FILE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(modified|new file|deleted)\s+(.*)").unwrap());
static DIFF_GIT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^diff --git\s+(.*)").unwrap());

pub fn read_initialization_params_from_tempfile(
    file_path: &PathBuf,
) -> Result<(String, Option<String>, Vec<SupportedFileType>)> {
    if let Ok(input) = read_to_string(file_path) {
        let mut cwd = String::new();
        let mut worktree: Option<String> = None;
        let mut file_types: Vec<SupportedFileType> = vec![];
        // Values may be padded with extra whitespace (the emacs client
        // tab-aligns header values), so match any run of whitespace and trim.

        for line in input.lines() {
            if let Some(caps) = ROOT_RE.captures(line) {
                cwd = caps.get(1).unwrap().as_str().trim().to_string();
                // break;
            }
            if let Some(caps) = WORKTREE_RE.captures(line) {
                let value = caps.get(1).unwrap().as_str().trim();
                // Clients may emit the header with no value (or a serialized
                // JS null) when no worktree exists for the review.
                if !value.is_empty() && value != "undefined" && value != "null" {
                    worktree = Some(value.to_string());
                }
            }
            if let Some(caps) = FILE_RE.captures(line) {
                trace!("caps: {:?}", caps.len());
                let filename = caps.get(2).unwrap().as_str().to_string();
                if let Some(file_type) = SupportedFileType::from_filename(filename) {
                    file_types.push(file_type);
                }
            } else if let Some(caps) = DIFF_GIT_RE.captures(line) {
                // Handle diff --git a/foo.rs b/foo.rs
                // We want the last one, and strip b/
                let files_part = caps.get(1).unwrap().as_str();
                let last_file = files_part.split_whitespace().last().unwrap_or("");
                let filename = if last_file.starts_with("b/") || last_file.starts_with("a/") {
                    last_file[2..].to_string()
                } else {
                    last_file.to_string()
                };
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
        return Err(anyhow!("Unable to read input tempfile"));
    }
}

#[derive(Debug)]
pub struct DiffLsp {
    pub client: Client,
    /// Running, initialized backends.  Clone the `Arc` out under a read lock;
    /// don't hold the map lock across backend I/O.
    pub backends: RwLock<BackendsMap>,
    /// File types whose backend failed to start; not retried on every didOpen.
    failed_backends: Mutex<HashSet<SupportedFileType>>,
    /// Failures from before the client was ready; shown in `initialized`.
    pending_failures: Mutex<Vec<BackendFailure>>,
    /// Serializes lazy backend startup so a language is only spawned once.
    startup_lock: Mutex<()>,
    /// Test hook: spawn this command instead of the real backend for lazy startups.
    command_override: Option<(String, Option<String>)>,
    pub diff_map: Arc<Mutex<HashMap<Url, Arc<ParsedDiff>>>>,
    pub root: String, // The project root, without a trailing slash.  ~/diff-lsp for example
    /// How backend locations are mapped back into the diff buffer (initializationOptions.diffLocations).
    pub diff_locations_mode: std::sync::Mutex<DiffLocationsMode>,
    /// Latest backend diagnostics per source file.
    pub diagnostics: Arc<Mutex<DiagnosticsStore>>,
    /// Which diff lines get diagnostics; set from `initializationOptions.diagnostics`.
    pub diagnostics_mode: Arc<std::sync::RwLock<DiagnosticsMode>>,
    /// Every backend forwards its notifications here (see
    /// `ClientForBackendServer::set_notification_handler`).
    notification_tx: tokio::sync::mpsc::UnboundedSender<Value>,
    /// Taken in `initialized`, when the diagnostics task starts.
    notification_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Value>>>,
}

/// Maps the stored backend diagnostics onto each diff buffer (all of them, or
/// just `only_uri`) and publishes them to the editor.  Empty sets are sent too,
/// so fixed problems disappear.
async fn publish_diff_diagnostics(
    client: &Client,
    diff_map: &Mutex<HashMap<Url, Arc<ParsedDiff>>>,
    store: &Mutex<DiagnosticsStore>,
    mode: DiagnosticsMode,
    root: &str,
    only_uri: Option<&Url>,
) {
    if mode == DiagnosticsMode::Off {
        return;
    }
    let to_publish: Vec<(Url, Vec<Diagnostic>)> = {
        let diff_map = diff_map.lock().await;
        let store = store.lock().await;
        diff_map
            .iter()
            .filter(|(uri, _)| only_uri.is_none_or(|only| only == *uri))
            .map(|(uri, diff)| {
                (
                    uri.clone(),
                    map_diagnostics_for_diff(diff, root, &store, mode),
                )
            })
            .collect()
    };
    for (uri, diags) in to_publish {
        info!("Publishing {} diagnostics to {}", diags.len(), uri);
        client.publish_diagnostics(uri, diags, None).await;
    }
}

impl DiffLsp {
    pub fn new(client: Client, backends: BackendsMap, root: String) -> Self {
        let (notification_tx, notification_rx) = tokio::sync::mpsc::unbounded_channel();
        for backend in backends.values() {
            backend.set_notification_handler(notification_tx.clone());
        }
        let server = DiffLsp {
            client,
            backends: RwLock::new(backends),
            failed_backends: Mutex::new(HashSet::new()),
            pending_failures: Mutex::new(vec![]),
            startup_lock: Mutex::new(()),
            command_override: None,
            diff_map: Arc::new(Mutex::new(HashMap::new())),
            root,
            diff_locations_mode: std::sync::Mutex::new(DiffLocationsMode::default()),
            diagnostics: Arc::new(Mutex::new(DiagnosticsStore::new())),
            diagnostics_mode: Arc::new(std::sync::RwLock::new(DiagnosticsMode::default())),
            notification_tx,
            notification_rx: Mutex::new(Some(notification_rx)),
        };
        info!("Starting server: {:?}", server);
        server
    }

    /// Records backends that failed to start before the server was created
    /// (see [`create_backends_map`]).  They are reported to the user once the
    /// client is initialized and are not retried.
    pub fn with_backend_failures(self, failures: Vec<BackendFailure>) -> Self {
        {
            let mut failed = self.failed_backends.try_lock().unwrap();
            failed.extend(failures.iter().map(|f| f.file_type));
        }
        self.pending_failures.try_lock().unwrap().extend(failures);
        self
    }

    /// Test hook: use `command` (with optional args) for every lazily started backend.
    pub fn with_command_override(mut self, command: String, args: Option<String>) -> Self {
        self.command_override = Some((command, args));
        self
    }

    /// Returns true if the backend for this file type has failed and is disabled.
    pub async fn backend_failed(&self, file_type: SupportedFileType) -> bool {
        self.failed_backends.lock().await.contains(&file_type)
    }

    async fn get_backend(&self, file_type: SupportedFileType) -> Option<BackendHandle> {
        self.backends.read().await.get(&file_type).cloned()
    }

    async fn all_backends(&self) -> Vec<(SupportedFileType, BackendHandle)> {
        self.backends
            .read()
            .await
            .iter()
            .map(|(ft, b)| (*ft, b.clone()))
            .collect()
    }

    async fn report_failure(&self, failure: BackendFailure) {
        warn!("Backend disabled: {}", failure.user_message());
        self.failed_backends.lock().await.insert(failure.file_type);
        self.client
            .show_message(MessageType::WARNING, failure.user_message())
            .await;
    }

    /// Returns the running backend for `file_type`, spawning and initializing
    /// it first if needed.  Failures are remembered so a missing binary is
    /// only attempted (and reported) once per session.
    async fn ensure_backend(&self, file_type: SupportedFileType) -> Option<BackendHandle> {
        if let Some(backend) = self.get_backend(file_type).await {
            return Some(backend);
        }
        if self.backend_failed(file_type).await {
            return None;
        }

        let _guard = self.startup_lock.lock().await;
        // Re-check: another task may have started it while we waited.
        if let Some(backend) = self.get_backend(file_type).await {
            return Some(backend);
        }
        if self.backend_failed(file_type).await {
            return None;
        }

        info!("Lazily starting backend for {:?}", file_type);
        let backend = match spawn_backend(file_type, &self.root, self.command_override.clone()) {
            Ok(b) => b,
            Err(failure) => {
                self.report_failure(failure).await;
                return None;
            }
        };
        backend.set_notification_handler(self.notification_tx.clone());
        if let Err(e) = backend.initialize().await {
            warn!(
                "Backend {} failed to initialize: {:?}",
                backend.lsp_command, e
            );
            self.report_failure(BackendFailure {
                file_type,
                command: backend.lsp_command.clone(),
                reason: "failed to initialize".to_string(),
            })
            .await;
            return None;
        }
        backend.initialized().await;

        let handle = Arc::new(backend);
        self.backends
            .write()
            .await
            .insert(file_type, handle.clone());
        Some(handle)
    }

    async fn get_diff(&self, uri: &Url) -> Option<Arc<ParsedDiff>> {
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
            debug!(
                "Using diff with {} mapped lines, {} total lines, parsed at {:?}",
                diff.lines_map.len(),
                diff.total_lines,
                diff.parsed_at
            );
            return diff.map_diff_line_to_src(line_num);
        }
        info!("Failed to find diff at URI: {:?}", uri);
        None
    }

    fn diff_locations_mode(&self) -> DiffLocationsMode {
        *self.diff_locations_mode.lock().unwrap()
    }

    /// Add diff-buffer locations to backend locations (see `reverse_map`).
    async fn map_locations_to_diff(
        &self,
        diff_uri: &Url,
        locs: Option<Vec<Location>>,
    ) -> Option<Vec<Location>> {
        let (locs, mode) = (locs?, self.diff_locations_mode());
        let Some(diff) = self.get_diff(diff_uri).await else {
            return Some(locs);
        };
        let index = ReverseIndex::from_diff(&diff);
        let mapper = DiffLocationMapper {
            index: &index,
            root: &self.root,
            diff_uri,
        };
        Some(mapper.map_locations(locs, mode))
    }

    /// Add diff-buffer locations to a goto-(type-)definition response.
    async fn map_definition_to_diff(
        &self,
        diff_uri: &Url,
        resp: Option<GotoDefinitionResponse>,
    ) -> Option<GotoDefinitionResponse> {
        let (resp, mode) = (resp?, self.diff_locations_mode());
        let Some(diff) = self.get_diff(diff_uri).await else {
            return Some(resp);
        };
        let index = ReverseIndex::from_diff(&diff);
        let mapper = DiffLocationMapper {
            index: &index,
            root: &self.root,
            diff_uri,
        };
        Some(mapper.map_definition_response(resp, mode))
    }

    fn diagnostics_mode(&self) -> DiagnosticsMode {
        self.diagnostics_mode.read().map(|m| *m).unwrap_or_default()
    }

    /// Publishes mapped diagnostics for one diff buffer (e.g. after it is
    /// opened or re-parsed).
    async fn publish_diagnostics_for(&self, uri: &Url) {
        publish_diff_diagnostics(
            &self.client,
            &self.diff_map,
            &self.diagnostics,
            self.diagnostics_mode(),
            &self.root,
            Some(uri),
        )
        .await;
    }

    /// Starts the task that stores `publishDiagnostics` from the backends and
    /// republishes the diff buffers whenever they change.  Runs once.
    async fn start_diagnostics_task(&self) {
        let Some(mut rx) = self.notification_rx.lock().await.take() else {
            return;
        };
        let client = self.client.clone();
        let diff_map = self.diff_map.clone();
        let store = self.diagnostics.clone();
        let mode = self.diagnostics_mode.clone();
        let root = self.root.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let Some(params) = publish_diagnostics_from_notification(&msg) else {
                    continue;
                };
                debug!(
                    "Backend published {} diagnostics for {}",
                    params.diagnostics.len(),
                    params.uri
                );
                if !store.lock().await.update(params) {
                    continue;
                }
                let mode = mode.read().map(|m| *m).unwrap_or_default();
                publish_diff_diagnostics(&client, &diff_map, &store, mode, &root, None).await;
            }
        });
    }

    async fn refresh_file(&self, uri: &Url) -> Option<Arc<ParsedDiff>> {
        let real_path = uri.path();
        debug!("Calling refresh_file for the real path: {:?}", real_path);

        let contents = fs::read_to_string(real_path).unwrap();
        if let Some(diff) = ParsedDiff::parse(&contents) {
            let diff = Arc::new(diff);
            let mut diff_map = self.diff_map.lock().await;

            if let Some(diff_before) = diff_map.get(uri) {
                debug!("Diff before len: {:?}", diff_before.lines_map.len());
            }
            debug!("Diff new len: {:?}", diff.lines_map.len());

            diff_map.insert(uri.clone(), Arc::clone(&diff)); // Use the *same* lock to insert
            Some(diff)
        } else {
            None
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for DiffLsp {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        let mode =
            DiffLocationsMode::from_initialization_options(params.initialization_options.as_ref());
        info!("diffLocations mode: {:?}", mode);
        *self.diff_locations_mode.lock().unwrap() = mode;
        let diagnostics_mode =
            DiagnosticsMode::from_initialization_options(params.initialization_options.as_ref());
        info!("Diagnostics mode: {:?}", diagnostics_mode);
        if let Ok(mut m) = self.diagnostics_mode.write() {
            *m = diagnostics_mode;
        }
        self.client
            .log_message(MessageType::WARNING, "Cruising")
            .await;
        info!("Starting initialize");
        for (file_type, backend) in self.all_backends().await {
            info!(
                "Diff LSP doing initialize for backend: {:?}",
                backend.lsp_command
            );
            if let Err(e) = backend.initialize().await {
                warn!(
                    "Backend {} failed to initialize: {}",
                    backend.lsp_command, e
                );
                self.backends.write().await.remove(&file_type);
                self.failed_backends.lock().await.insert(file_type);
                // The client isn't ready for showMessage until `initialized`.
                self.pending_failures.lock().await.push(BackendFailure {
                    file_type,
                    command: backend.lsp_command.clone(),
                    reason: "failed to initialize".to_string(),
                });
            }
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
                document_symbol_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
            ..Default::default()
        });
        info!("Finished initialize! {:?}", res.clone().unwrap());
        res
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("Starting Initialized");
        self.start_diagnostics_task().await;
        let pending: Vec<BackendFailure> = self.pending_failures.lock().await.drain(..).collect();
        for failure in pending {
            warn!("Backend disabled: {}", failure.user_message());
            self.client
                .show_message(MessageType::WARNING, failure.user_message())
                .await;
        }
        for (_, backend) in self.all_backends().await {
            info!(
                "Diff LSP doing initialized for backend: {:?}",
                backend.lsp_command
            );
            backend.initialized().await;
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
                self.publish_diagnostics_for(&key).await;
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
            for (_, backend) in self.all_backends().await {
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
        let source_map_res = self
            .get_source_map(params.text_document_position_params.clone())
            .await;

        let source_map = match source_map_res {
            Some(sm) => sm,
            None => {
                // Headers, comments, blank lines etc. have nothing to hover.
                info!("Did not find a source map for this hover!");
                return Ok(None);
            }
        };

        info!(
            "source map: {:?} - {:?}",
            source_map.source_line, source_map.source_line_text
        );
        if source_map.source_line_type == LineType::Removed {
            info!("Hover on a removed line; nothing to query");
            return Ok(None);
        }
        let backend_mutex = match self.get_backend(source_map.file_type).await {
            Some(bm) => bm,
            None => {
                info!("No running backend for {:?}", source_map.file_type);
                return Ok(None);
            }
        };
        let backend = backend_mutex;
        // TODO do all this mapping in an async func since there's a lot of cloning and whatnot and then futures::join! it with the backend_mutex
        let mut mapped_params = params.clone();
        let uri = uri_from_relative_filename(self.root.clone(), &source_map.file_name);

        mapped_params
            .text_document_position_params
            .text_document
            .uri = uri;

        // The source map's lines are 1-indexed; the client's column counts the
        // diff line's +/- marker.
        mapped_params.text_document_position_params.position =
            source_map.source_position(params.text_document_position_params.position);

        // info!("Hover mapped params: {:?}", mapped_params);
        let hov_res = backend.hover(mapped_params).await;
        // info!("Hover res: {:?}", hov_res);
        match hov_res {
            Ok(res) => Ok(res),
            Err(e) => {
                warn!("Backend request failed: {:?}", e);
                Ok(None)
            }
        }
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        if let Some(diff) = self.refresh_file(&params.text_document.uri).await {
            let filtered_files: Vec<String> = diff.filenames.clone().into_iter().unique().collect();
            for filename in filtered_files {
                let filetype = SupportedFileType::from_filename(filename.clone());

                if let None = filetype {
                    continue;
                }

                if let Some(backend) = self.ensure_backend(filetype.unwrap()).await {
                    let mut these_params = params.clone();
                    // Here we need to break the LSP contract and use the originator's didOpen URI to read the contents of the file.
                    these_params.text_document.uri =
                        uri_from_relative_filename(self.root.clone(), &filename);

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
                            backend.did_open(&these_params).await;
                        }
                        Err(_) => {
                            info!("Skipping opening file {:?}", full_path);
                        }
                    }
                }
            }
        }
        self.publish_diagnostics_for(&params.text_document.uri)
            .await;
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
        let mapped_params_uri = _params.text_document_position.text_document.uri.clone();
        let Some(source_map) = self.get_source_map(_params.text_document_position).await else {
            return Ok(None);
        };
        if source_map.source_line_type == LineType::Removed {
            return Ok(None);
        }

        let backend_mutex = match self.get_backend(source_map.file_type).await {
            Some(bm) => bm,
            None => {
                info!("No running backend for {:?}", source_map.file_type);
                return Ok(None);
            }
        };

        let backend = backend_mutex;

        let uri = uri_from_relative_filename(self.root.clone(), &source_map.file_name);
        mapped_params.text_document_position.text_document.uri = uri;
        // Same as for hover
        mapped_params.text_document_position.position =
            source_map.source_position(mapped_params.text_document_position.position);

        let references_result = backend.references(&mapped_params).await;
        let diff_uri = &mapped_params_uri;
        match references_result {
            Ok(res) => Ok(self.map_locations_to_diff(diff_uri, res).await),
            Err(e) => {
                warn!("Backend request failed: {:?}", e);
                Ok(None)
            }
        }
    }

    async fn goto_definition(
        &self,
        _params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let Some(source_map) = self
            .get_source_map(_params.text_document_position_params.clone())
            .await
        else {
            return Ok(None);
        };
        if source_map.source_line_type == LineType::Removed {
            return Ok(None);
        }

        let mut mapped_params = _params.clone();
        let backend_mutex = match self.get_backend(source_map.file_type).await {
            Some(bm) => bm,
            None => {
                info!("No running backend for {:?}", source_map.file_type);
                return Ok(None);
            }
        };

        let backend = backend_mutex;

        let uri = uri_from_relative_filename(self.root.clone(), &source_map.file_name);

        mapped_params
            .text_document_position_params
            .text_document
            .uri = uri;
        // same as for hover
        mapped_params.text_document_position_params.position =
            source_map.source_position(_params.text_document_position_params.position);
        let goto_def_res = backend.goto_definition(&mapped_params).await;
        let diff_uri = &_params.text_document_position_params.text_document.uri;
        match goto_def_res {
            Ok(res) => Ok(self.map_definition_to_diff(diff_uri, res).await),
            Err(e) => {
                warn!("Backend request failed: {:?}", e);
                Ok(None)
            }
        }
    }

    async fn goto_type_definition(
        &self,
        params: GotoTypeDefinitionParams,
    ) -> LspResult<Option<GotoTypeDefinitionResponse>> {
        let Some(source_map) = self
            .get_source_map(params.text_document_position_params.clone())
            .await
        else {
            return Ok(None);
        };
        if source_map.source_line_type == LineType::Removed {
            return Ok(None);
        }

        let mut mapped_params = params.clone();
        let backend_mutex = match self.get_backend(source_map.file_type).await {
            Some(bm) => bm,
            None => {
                info!("No running backend for {:?}", source_map.file_type);
                return Ok(None);
            }
        };

        let backend = backend_mutex;

        let uri = uri_from_relative_filename(self.root.clone(), &source_map.file_name);

        mapped_params
            .text_document_position_params
            .text_document
            .uri = uri;
        // same as for hover
        mapped_params.text_document_position_params.position =
            source_map.source_position(params.text_document_position_params.position);

        let goto_type_def_res = backend.goto_type_definition(&mapped_params).await;
        let diff_uri = &params.text_document_position_params.text_document.uri;
        match goto_type_def_res {
            Ok(res) => Ok(self.map_definition_to_diff(diff_uri, res).await),
            Err(e) => {
                warn!("Backend request failed: {:?}", e);
                Ok(None)
            }
        }
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let diff = match self.get_diff(&params.text_document.uri).await {
            Some(diff) => diff,
            None => return Ok(None),
        };

        // Fetch backend symbols for every file in the diff we have a backend for.
        // A failing backend just leaves that file without children.
        let mut backend_symbols: HashMap<String, DocumentSymbolResponse> = HashMap::new();
        for filename in diff.filenames.iter().unique() {
            let Some(file_type) = SupportedFileType::from_filename(filename.clone()) else {
                continue;
            };
            let Some(backend) = self.get_backend(file_type).await else {
                continue;
            };
            let mapped_params = DocumentSymbolParams {
                text_document: TextDocumentIdentifier {
                    uri: uri_from_relative_filename(self.root.clone(), filename),
                },
                work_done_progress_params: params.work_done_progress_params.clone(),
                partial_result_params: params.partial_result_params.clone(),
            };
            match backend.document_symbol(&mapped_params).await {
                Ok(Some(response)) => {
                    backend_symbols.insert(filename.clone(), response);
                }
                other => info!("No document symbols for {:?}: {:?}", filename, other),
            }
        }

        Ok(Some(DocumentSymbolResponse::Nested(
            symbols::build_diff_symbols(&diff, &backend_symbols),
        )))
    }
}
