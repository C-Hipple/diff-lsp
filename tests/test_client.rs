// diff-lsp matches each backend answer to its request by id. Servers write
// other things on the same pipe meanwhile — notifications like tsserver's
// $/typescriptVersion, and requests of their own — and taking one of those as
// the answer shifts every later answer onto the wrong request.
use diff_lsp::client::{read_message, ClientForBackendServer};
use serde_json::{json, Value};
use tokio::io::{duplex, split, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

fn frame(msg: Value) -> String {
    let body = msg.to_string();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
}

/// The backend's end of an in-memory connection to a client.
struct Backend {
    reader: BufReader<ReadHalf<DuplexStream>>,
    writer: WriteHalf<DuplexStream>,
}

impl Backend {
    async fn recv(&mut self) -> Value {
        let body = read_message(&mut self.reader).await.unwrap().unwrap();
        serde_json::from_str(&body).unwrap()
    }

    async fn send(&mut self, msgs: &[Value]) {
        let stream: String = msgs.iter().cloned().map(frame).collect();
        self.writer.write_all(stream.as_bytes()).await.unwrap();
        self.writer.flush().await.unwrap();
    }
}

fn connect() -> (ClientForBackendServer, Backend) {
    let (client_side, backend_side) = duplex(64 * 1024);
    let (cr, cw) = split(client_side);
    let (br, bw) = split(backend_side);
    let client = ClientForBackendServer::from_streams("fake".to_string(), cr, cw);
    (
        client,
        Backend {
            reader: BufReader::new(br),
            writer: bw,
        },
    )
}

#[tokio::test]
async fn test_answers_skip_other_messages() {
    let (client, mut backend) = connect();
    let server = tokio::spawn(async move {
        let first = backend.recv().await;
        backend
            .send(&[
                json!({"jsonrpc": "2.0", "method": "$/typescriptVersion", "params": {"version": "5.9"}}),
                json!({"jsonrpc": "2.0", "method": "window/logMessage", "params": {"type": 3, "message": "hi"}}),
                json!({"jsonrpc": "2.0", "id": 0, "method": "window/workDoneProgress/create", "params": {"token": "t"}}),
                // A hover whose text happens to name a notification is still an answer.
                json!({"jsonrpc": "2.0", "id": first["id"], "result": {"contents": "func showMessage()"}}),
            ])
            .await;
        // The server's own request was answered, so it isn't left waiting.
        assert_eq!(
            backend.recv().await,
            json!({"jsonrpc": "2.0", "id": 0, "result": null})
        );
        // The next answer arrives intact on the same connection.
        let second = backend.recv().await;
        backend
            .send(&[json!({"jsonrpc": "2.0", "id": second["id"], "result": [1]})])
            .await;
        backend
    });

    let first = client
        .request("textDocument/hover", json!({}))
        .await
        .unwrap();
    assert_eq!(first["contents"], "func showMessage()");
    let second = client
        .request("textDocument/references", json!({}))
        .await
        .unwrap();
    assert_eq!(second, json!([1]));
    server.await.unwrap();
}

#[tokio::test]
async fn test_answers_to_other_requests_are_skipped() {
    let (client, mut backend) = connect();
    let server = tokio::spawn(async move {
        let req = backend.recv().await;
        let id = req["id"].as_i64().unwrap();
        backend
            .send(&[
                json!({"jsonrpc": "2.0", "id": id + 100, "result": null}),
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "nope"}}),
            ])
            .await;
        backend
    });
    let err = client.request("m/a", json!({})).await.unwrap_err();
    assert!(err.to_string().contains("-32601"), "{}", err);
    server.await.unwrap();
}

// publishDiagnostics arriving while a request is in flight is handed to the
// notification hook (which the server uses for diagnostics), and the answer is
// still the one returned for the request.
#[tokio::test]
async fn test_notification_hook_receives_diagnostics_before_answer() {
    use diff_lsp::diagnostics::publish_diagnostics_from_notification;
    use tower_lsp::lsp_types::Url;

    let (client, mut backend) = connect();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    client.set_notification_handler(tx);
    let server = tokio::spawn(async move {
        let req = backend.recv().await;
        backend
            .send(&[
                json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {
                    "uri": "file:///proj/src/lib.rs",
                    "diagnostics": [{
                        "range": {"start": {"line": 6, "character": 8}, "end": {"line": 6, "character": 9}},
                        "severity": 2,
                        "message": "unused variable: `x`"
                    }]
                }}),
                json!({"jsonrpc": "2.0", "method": "window/logMessage", "params": {"type": 3, "message": "hi"}}),
                json!({"jsonrpc": "2.0", "id": req["id"], "result": {"contents": "fn foo()"}}),
            ])
            .await;
    });
    let answer = client
        .request("textDocument/hover", json!({}))
        .await
        .unwrap();
    assert_eq!(answer["contents"], "fn foo()");
    server.await.unwrap();

    let captured: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|msg| publish_diagnostics_from_notification(&msg))
        .collect();
    assert_eq!(
        captured.len(),
        1,
        "only the diagnostics notification is captured"
    );
    assert_eq!(
        captured[0].uri,
        Url::parse("file:///proj/src/lib.rs").unwrap()
    );
    assert_eq!(captured[0].diagnostics[0].message, "unused variable: `x`");
    assert_eq!(captured[0].diagnostics[0].range.start.line, 6);
}
