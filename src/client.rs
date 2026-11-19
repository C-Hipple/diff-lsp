//! Communication with a backend language server (rust-analyzer, gopls, ...).
//!
//! Each backend gets:
//! * one writer (behind an async mutex) for its stdin, so messages are never interleaved,
//! * one dedicated reader task owning a single persistent buffered reader of its stdout,
//!   which dispatches responses to waiting callers by request id, answers
//!   server->client requests, and logs (or forwards) notifications,
//! * one task draining stderr into the log so a chatty backend can't fill the pipe.
//!
//! Requests are async, time out after [`REQUEST_TIMEOUT`], and can run concurrently.

use anyhow::{anyhow, Result};
use expanduser::expanduser;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::fs::canonicalize;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tower_lsp::lsp_types::request::{GotoTypeDefinitionParams, GotoTypeDefinitionResponse};
use tower_lsp::lsp_types::*;

const HEADER_CONTENT_LENGTH: &str = "content-length";
const HEADER_CONTENT_TYPE: &str = "content-type";

/// How long to wait for a backend to answer an ordinary request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// `initialize` can be slow for large projects / heavyweight servers.
pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);

type BoxedWriter = Box<dyn AsyncWrite + Send + Unpin>;
type PendingMap = HashMap<i64, oneshot::Sender<Value>>;

/// State shared between callers and the reader task.
struct Shared {
    name: String,
    writer: Mutex<BoxedWriter>,
    pending: StdMutex<PendingMap>,
    next_id: AtomicI64,
    notification_tx: StdMutex<Option<mpsc::UnboundedSender<Value>>>,
}

impl Shared {
    async fn write_value(&self, value: &Value) -> Result<()> {
        let body = serde_json::to_string(value)?;
        let msg = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut writer = self.writer.lock().await;
        writer.write_all(msg.as_bytes()).await?;
        writer.flush().await?;
        Ok(())
    }

    fn take_pending(&self, id: i64) -> Option<oneshot::Sender<Value>> {
        self.pending.lock().unwrap().remove(&id)
    }

    /// Handle one message read from the backend.
    async fn dispatch(&self, msg: Value) {
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_string);
        let id = msg.get("id").cloned().filter(|v| !v.is_null());

        match (method, id) {
            // Response to one of our requests.
            (None, Some(id)) => {
                let sender = id.as_i64().and_then(|n| self.take_pending(n));
                match sender {
                    Some(tx) => {
                        let _ = tx.send(msg);
                    }
                    None => debug!(
                        "[{}] response for unknown/expired request id {}",
                        self.name, id
                    ),
                }
            }
            // Server -> client request; must be answered or some servers stall.
            (Some(method), Some(id)) => {
                let reply = reply_to_server_request(&method, msg.get("params"), id);
                debug!("[{}] answering server request {}", self.name, method);
                if let Err(e) = self.write_value(&reply).await {
                    warn!("[{}] failed to answer {}: {}", self.name, method, e);
                }
            }
            // Notification.
            (Some(method), None) => {
                debug!("[{}] notification {}: {}", self.name, method, msg);
                let tx = self.notification_tx.lock().unwrap().clone();
                if let Some(tx) = tx {
                    let _ = tx.send(msg);
                }
            }
            (None, None) => debug!("[{}] ignoring malformed message: {}", self.name, msg),
        }
    }

    /// Fail every outstanding request (dropping the senders wakes the waiters).
    fn fail_all_pending(&self) {
        self.pending.lock().unwrap().clear();
    }
}

/// Build the response to a request the backend sent to us.
pub fn reply_to_server_request(method: &str, params: Option<&Value>, id: Value) -> Value {
    match method {
        "workspace/configuration" => {
            let n = params
                .and_then(|p| p.get("items"))
                .and_then(Value::as_array)
                .map(|a| a.len())
                .unwrap_or(0);
            json!({"jsonrpc": "2.0", "id": id, "result": vec![Value::Null; n]})
        }
        // Everything else (workDoneProgress/create, register/unregisterCapability,
        // showMessageRequest, the */refresh requests, ...) gets a null result:
        // the server only needs an answer so it isn't left waiting, and an
        // error would make some servers log or retry.
        _ => json!({"jsonrpc": "2.0", "id": id, "result": null}),
    }
}

async fn reader_loop<R: AsyncBufRead + Unpin>(shared: Arc<Shared>, mut reader: R) {
    loop {
        match read_message(&mut reader).await {
            Ok(Some(body)) => match serde_json::from_str::<Value>(&body) {
                Ok(v) => shared.dispatch(v).await,
                Err(e) => warn!(
                    "[{}] invalid JSON from backend: {} ({})",
                    shared.name, e, body
                ),
            },
            Ok(None) => {
                info!("[{}] backend closed stdout", shared.name);
                break;
            }
            Err(e) => {
                warn!("[{}] error reading from backend: {}", shared.name, e);
                break;
            }
        }
    }
    shared.fail_all_pending();
}

pub struct ClientForBackendServer {
    pub lsp_command: String,
    path: Option<PathBuf>,
    process_id: Option<u32>,
    shared: Arc<Shared>,
    // Held so the child is killed when the client is dropped.
    _child: Option<Child>,
}

impl std::fmt::Debug for ClientForBackendServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientForBackendServer")
            .field("lsp_command", &self.lsp_command)
            .field("path", &self.path)
            .field("process_id", &self.process_id)
            .finish()
    }
}

fn start_server(command: &str, args: Option<String>, dir: &str) -> Result<Child> {
    let mut process = Command::new(command);
    if let Some(args_val) = args {
        process.args(args_val.split_whitespace());
    }
    let expanded_dir = expanduser(dir)?;
    process
        .current_dir(canonicalize(expanded_dir)?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn local LSP server {}: {}", command, e))
}

impl ClientForBackendServer {
    /// Spawn the backend process and start its reader/stderr tasks.
    /// Must be called from within a tokio runtime.
    pub fn new(command: String, args: Option<String>, directory: &str) -> Result<Self> {
        let mut child = start_server(&command, args, directory)?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        if let Some(stderr) = child.stderr.take() {
            let name = command.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    debug!("[{} stderr] {}", name, line);
                }
            });
        }
        let mut client = Self::from_streams(command, stdout, stdin);
        client.process_id = child.id();
        client.path = Some(canonicalize(expanduser(directory)?)?);
        client._child = Some(child);
        Ok(client)
    }

    /// Build a client over arbitrary streams (used by `new`, and by tests).
    pub fn from_streams<R, W>(name: String, reader: R, writer: W) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let shared = Arc::new(Shared {
            name: name.clone(),
            writer: Mutex::new(Box::new(writer)),
            pending: StdMutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
            notification_tx: StdMutex::new(None),
        });
        tokio::spawn(reader_loop(shared.clone(), BufReader::new(reader)));
        ClientForBackendServer {
            lsp_command: name,
            path: None,
            process_id: None,
            shared,
            _child: None,
        }
    }

    /// Hook point: every notification from the backend (e.g. publishDiagnostics)
    /// is forwarded to `tx` as raw JSON.
    pub fn set_notification_handler(&self, tx: mpsc::UnboundedSender<Value>) {
        *self.shared.notification_tx.lock().unwrap() = Some(tx);
    }

    /// Number of requests still waiting on a response.
    pub fn pending_requests(&self) -> usize {
        self.shared.pending.lock().unwrap().len()
    }

    #[allow(deprecated)] // root_path is deprecated but some servers still want it
    pub async fn initialize(&self) -> Result<InitializeResult> {
        debug!("initialize path: {:?}", self.path);
        let root_path = self
            .path
            .as_ref()
            .and_then(|p| p.clone().into_os_string().into_string().ok());
        let root_uri = self.path.as_ref().and_then(|p| Url::from_file_path(p).ok());
        let workspace_folders = match (&root_uri, &self.path) {
            (Some(uri), Some(path)) => Some(vec![WorkspaceFolder {
                uri: uri.clone(),
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned()),
            }]),
            _ => None,
        };
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_path,
            root_uri,
            initialization_options: None,
            capabilities: ClientCapabilities {
                workspace: None,
                text_document: Some(TextDocumentClientCapabilities {
                    hover: Some(HoverClientCapabilities::default()),
                    references: None,
                    document_symbol: Some(DocumentSymbolClientCapabilities {
                        hierarchical_document_symbol_support: Some(true),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                window: None,
                general: None,
                experimental: None,
            },
            trace: None,
            workspace_folders,
            client_info: Some(ClientInfo {
                name: "diff-lsp-client".to_string(),
                version: Some("0.0.1".to_string()),
            }),
            locale: None,
        };
        let raw = self
            .request_with_timeout("initialize", params, INITIALIZE_TIMEOUT)
            .await?;
        Ok(serde_json::from_value(raw)?)
    }

    pub async fn initialized(&self) {
        if let Err(e) = self.notify("initialized", InitializedParams {}).await {
            warn!("[{}] failed to send initialized: {}", self.lsp_command, e);
        }
    }

    /// Send a request and wait (up to [`REQUEST_TIMEOUT`]) for its result.
    pub async fn request<P: Serialize>(&self, method: &str, params: P) -> Result<Value> {
        self.request_with_timeout(method, params, REQUEST_TIMEOUT)
            .await
    }

    pub async fn request_with_timeout<P: Serialize>(
        &self,
        method: &str,
        params: P,
        timeout: Duration,
    ) -> Result<Value> {
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.shared.pending.lock().unwrap().insert(id, tx);

        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": serde_json::to_value(params)?,
        });
        if let Err(e) = self.shared.write_value(&msg).await {
            self.shared.take_pending(id);
            return Err(e);
        }

        let resp = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(resp)) => resp,
            Ok(Err(_)) => return Err(anyhow!("[{}] backend exited", self.lsp_command)),
            Err(_) => {
                self.shared.take_pending(id);
                let _ = self.notify("$/cancelRequest", json!({ "id": id })).await;
                return Err(anyhow!(
                    "[{}] request {} timed out after {:?}",
                    self.lsp_command,
                    method,
                    timeout
                ));
            }
        };

        if let Some(err) = resp.get("error") {
            return Err(anyhow!("[{}] {} failed: {}", self.lsp_command, method, err));
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }

    /// Send a notification (no response expected).
    pub async fn notify<P: Serialize>(&self, method: &str, params: P) -> Result<()> {
        debug!(
            "Sending notification {} to backend {}",
            method, self.lsp_command
        );
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": serde_json::to_value(params)?,
        });
        self.shared.write_value(&msg).await
    }

    pub async fn did_open(&self, params: &DidOpenTextDocumentParams) {
        if let Err(e) = self.notify("textDocument/didOpen", params).await {
            warn!("[{}] failed to send didOpen: {}", self.lsp_command, e);
        }
    }

    /// Request + deserialize; a null or unparseable result becomes `Ok(None)`.
    async fn typed_request<P: Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        params: P,
    ) -> Result<Option<T>> {
        let raw = self.request(method, params).await?;
        match serde_json::from_value::<Option<T>>(raw) {
            Ok(v) => Ok(v),
            Err(e) => {
                info!(
                    "[{}] could not parse {} result: {}",
                    self.lsp_command, method, e
                );
                Ok(None)
            }
        }
    }

    pub async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        debug!("Doing hover with the params: {:?}", params);
        self.typed_request("textDocument/hover", params).await
    }

    pub async fn goto_definition(
        &self,
        params: &GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        debug!("Doing goto definition with the params: {:?}", params);
        self.typed_request("textDocument/definition", params).await
    }

    pub async fn goto_type_definition(
        &self,
        params: &GotoTypeDefinitionParams,
    ) -> Result<Option<GotoTypeDefinitionResponse>> {
        debug!("Doing goto type definition with the params: {:?}", params);
        self.typed_request("textDocument/typeDefinition", params)
            .await
    }

    pub async fn references(&self, params: &ReferenceParams) -> Result<Option<Vec<Location>>> {
        debug!("Doing references with the params: {:?}", params);
        self.typed_request("textDocument/references", params).await
    }

    pub async fn document_symbol(
        &self,
        params: &DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        debug!("Doing document symbol with the params: {:?}", params);
        self.typed_request("textDocument/documentSymbol", params)
            .await
    }

    /// Messages are now read continuously by the reader task; this just reports state.
    pub fn check_messages(&self) {
        info!(
            "[{}] {} pending request(s)",
            self.lsp_command,
            self.pending_requests()
        );
    }
}

pub enum LspHeader {
    ContentType,
    ContentLength(usize),
}

pub fn parse_header(s: &str) -> Result<LspHeader> {
    let (name, value) = s
        .split_once(':')
        .ok_or_else(|| anyhow!("Malformed header: {:?}", s))?;
    let name = name.trim().to_lowercase();
    let value = value.trim();
    match name.as_str() {
        HEADER_CONTENT_TYPE => Ok(LspHeader::ContentType),
        HEADER_CONTENT_LENGTH => Ok(LspHeader::ContentLength(value.parse::<usize>()?)),
        other => Err(anyhow!("Unknown header: {:?}", other)),
    }
}

/// Read one LSP message body. Returns `Ok(None)` on a clean EOF before any header.
pub async fn read_message<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<String>> {
    let mut line = String::new();
    let mut content_length: Option<usize> = None;
    let mut saw_header = false;

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            if saw_header {
                return Err(anyhow!("EOF in the middle of LSP headers"));
            }
            return Ok(None);
        }
        if line.trim().is_empty() {
            if saw_header {
                break;
            }
            // Tolerate stray blank lines between messages.
            continue;
        }
        saw_header = true;
        match parse_header(&line)? {
            LspHeader::ContentLength(len) => content_length = Some(len),
            LspHeader::ContentType => (),
        }
    }

    let content_length = content_length.ok_or_else(|| anyhow!("Missing content-length header"))?;
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).await?;
    Ok(Some(String::from_utf8(body)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, split, DuplexStream, ReadHalf, WriteHalf};

    fn frame(v: &Value) -> String {
        let s = v.to_string();
        format!("Content-Length: {}\r\n\r\n{}", s.len(), s)
    }

    #[test]
    fn test_parse_header() {
        assert!(matches!(
            parse_header("Content-Length: 42\r\n").unwrap(),
            LspHeader::ContentLength(42)
        ));
        assert!(matches!(
            parse_header("content-length:7").unwrap(),
            LspHeader::ContentLength(7)
        ));
        assert!(matches!(
            parse_header("Content-Type: application/vscode-jsonrpc; charset=utf-8").unwrap(),
            LspHeader::ContentType
        ));
        assert!(parse_header("garbage").is_err());
        assert!(parse_header("Content-Length: abc").is_err());
    }

    #[tokio::test]
    async fn test_read_message_sequence_and_eof() {
        let a = json!({"jsonrpc": "2.0", "id": 1, "result": "héllo"});
        let b = json!({"jsonrpc": "2.0", "method": "x"});
        let input = format!(
            "{}Content-Type: application/vscode-jsonrpc\r\n{}",
            frame(&a),
            frame(&b)
        );
        let mut reader = BufReader::new(input.as_bytes());
        let m1: Value =
            serde_json::from_str(&read_message(&mut reader).await.unwrap().unwrap()).unwrap();
        assert_eq!(m1, a);
        let m2: Value =
            serde_json::from_str(&read_message(&mut reader).await.unwrap().unwrap()).unwrap();
        assert_eq!(m2, b);
        assert!(read_message(&mut reader).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_read_message_truncated() {
        let mut reader = BufReader::new("Content-Length: 10\r\n\r\n{}".as_bytes());
        assert!(read_message(&mut reader).await.is_err());
    }

    #[test]
    fn test_reply_to_server_request() {
        let r = reply_to_server_request(
            "workspace/configuration",
            Some(&json!({"items": [{"section": "a"}, {"section": "b"}]})),
            json!(7),
        );
        assert_eq!(
            r,
            json!({"jsonrpc": "2.0", "id": 7, "result": [null, null]})
        );
        let r = reply_to_server_request("client/registerCapability", None, json!("s"));
        assert_eq!(r, json!({"jsonrpc": "2.0", "id": "s", "result": null}));
        let r = reply_to_server_request("made/up", None, json!(1));
        assert_eq!(r, json!({"jsonrpc": "2.0", "id": 1, "result": null}));
    }

    /// The "backend" side of an in-memory connection.
    struct FakeBackend {
        reader: BufReader<ReadHalf<DuplexStream>>,
        writer: WriteHalf<DuplexStream>,
    }

    impl FakeBackend {
        async fn recv(&mut self) -> Value {
            let body = read_message(&mut self.reader).await.unwrap().unwrap();
            serde_json::from_str(&body).unwrap()
        }
        async fn send(&mut self, v: Value) {
            self.writer.write_all(frame(&v).as_bytes()).await.unwrap();
            self.writer.flush().await.unwrap();
        }
    }

    fn connect() -> (ClientForBackendServer, FakeBackend) {
        let (client_side, backend_side) = duplex(64 * 1024);
        let (cr, cw) = split(client_side);
        let (br, bw) = split(backend_side);
        let client = ClientForBackendServer::from_streams("fake".to_string(), cr, cw);
        (
            client,
            FakeBackend {
                reader: BufReader::new(br),
                writer: bw,
            },
        )
    }

    #[tokio::test]
    async fn test_dispatcher_skips_interleaved_traffic() {
        let (client, mut backend) = connect();
        let (ntx, mut nrx) = mpsc::unbounded_channel();
        client.set_notification_handler(ntx);

        let server = tokio::spawn(async move {
            let req = backend.recv().await;
            assert_eq!(req["method"], "textDocument/hover");
            let id = req["id"].clone();
            // Noise before the real response.
            backend
                .send(json!({"jsonrpc": "2.0", "method": "$/progress",
                             "params": {"token": "t", "value": {"kind": "begin"}}}))
                .await;
            backend
                .send(
                    json!({"jsonrpc": "2.0", "id": 900, "method": "workspace/configuration",
                             "params": {"items": [{}, {}, {}]}}),
                )
                .await;
            backend
                .send(
                    json!({"jsonrpc": "2.0", "id": 901, "method": "window/workDoneProgress/create",
                             "params": {"token": "t"}}),
                )
                .await;
            // The server->client requests must be answered.
            let r1 = backend.recv().await;
            assert_eq!(
                r1,
                json!({"jsonrpc": "2.0", "id": 900, "result": [null, null, null]})
            );
            let r2 = backend.recv().await;
            assert_eq!(r2, json!({"jsonrpc": "2.0", "id": 901, "result": null}));
            backend
                .send(json!({"jsonrpc": "2.0", "id": id,
                             "result": {"contents": {"kind": "markdown", "value": "fn main()"}}}))
                .await;
            backend
        });

        let hover = client
            .hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::parse("file:///tmp/x.rs").unwrap(),
                    },
                    position: Position::new(0, 0),
                },
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        match hover.contents {
            HoverContents::Markup(m) => assert_eq!(m.value, "fn main()"),
            other => panic!("unexpected hover {:?}", other),
        }
        let note = nrx.recv().await.unwrap();
        assert_eq!(note["method"], "$/progress");
        server.await.unwrap();
        assert_eq!(client.pending_requests(), 0);
    }

    #[tokio::test]
    async fn test_concurrent_requests_out_of_order() {
        let (client, mut backend) = connect();
        let server = tokio::spawn(async move {
            let a = backend.recv().await;
            let b = backend.recv().await;
            // Answer in reverse order, echoing the method name.
            for req in [b, a] {
                backend
                    .send(json!({"jsonrpc": "2.0", "id": req["id"], "result": req["method"]}))
                    .await;
            }
            backend
        });
        let (ra, rb) = tokio::join!(
            client.request("m/a", json!({})),
            client.request("m/b", json!({}))
        );
        assert_eq!(ra.unwrap(), json!("m/a"));
        assert_eq!(rb.unwrap(), json!("m/b"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_error_response_and_null_result() {
        let (client, mut backend) = connect();
        let server = tokio::spawn(async move {
            let a = backend.recv().await;
            backend
                .send(json!({"jsonrpc": "2.0", "id": a["id"],
                             "error": {"code": -32603, "message": "boom"}}))
                .await;
            let b = backend.recv().await;
            backend
                .send(json!({"jsonrpc": "2.0", "id": b["id"], "result": null}))
                .await;
            backend
        });
        assert!(client.request("m/a", json!({})).await.is_err());
        let refs = client
            .references(&ReferenceParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::parse("file:///tmp/x.rs").unwrap(),
                    },
                    position: Position::new(0, 0),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            })
            .await
            .unwrap();
        assert!(refs.is_none());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_request_timeout() {
        let (client, mut backend) = connect();
        let res = client
            .request_with_timeout("m/slow", json!({}), Duration::from_millis(50))
            .await;
        assert!(res.unwrap_err().to_string().contains("timed out"));
        assert_eq!(client.pending_requests(), 0);
        // A $/cancelRequest is sent for the abandoned request.
        let req = backend.recv().await;
        let cancel = backend.recv().await;
        assert_eq!(cancel["method"], "$/cancelRequest");
        assert_eq!(cancel["params"]["id"], req["id"]);
    }

    #[tokio::test]
    async fn test_backend_exit_fails_pending() {
        let (client, backend) = connect();
        let req = client.request("m/a", json!({}));
        drop(backend);
        assert!(req.await.is_err());
    }

    #[tokio::test]
    async fn test_spawned_fake_backend_process() {
        // A real child process through `new`: a shell script that ignores its
        // input, spews to stderr, then emits noise followed by the response for id 1.
        let resp = r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}"#;
        let noise =
            r#"{"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":"hi"}}"#;
        let script = format!(
            "for i in $(seq 1 10000); do echo 'stderr noise line' >&2; done; \
             printf 'Content-Length: {}\\r\\n\\r\\n%s' '{}'; \
             printf 'Content-Length: {}\\r\\n\\r\\n%s' '{}'; cat >/dev/null",
            noise.len(),
            noise,
            resp.len(),
            resp
        );
        // `new` splits args on whitespace, so run through a script file.
        let dir = std::env::temp_dir().join(format!("diff-lsp-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fake.sh");
        std::fs::write(&path, script).unwrap();
        let client = ClientForBackendServer::new(
            "sh".to_string(),
            Some(path.display().to_string()),
            dir.to_str().unwrap(),
        )
        .unwrap();
        let res = client.initialize().await.unwrap();
        assert_eq!(res.capabilities, ServerCapabilities::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
