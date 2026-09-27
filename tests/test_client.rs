// diff-lsp asks a backend one request at a time and reads until the answer.
// Servers write other things on the same pipe meanwhile — notifications like
// tsserver's $/typescriptVersion, and requests of their own — and taking one
// of those as the answer shifts every later answer onto the wrong request.
use diff_lsp::client::read_response;
use serde_json::{json, Value};
use std::io::Cursor;

fn frame(msg: Value) -> String {
    let body = msg.to_string();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
}

fn parse(body: &str) -> Value {
    serde_json::from_str(body).unwrap()
}

#[test]
fn test_read_response_skips_other_messages() {
    let stream = [
        frame(json!({"jsonrpc": "2.0", "method": "$/typescriptVersion", "params": {"version": "5.9"}})),
        frame(json!({"jsonrpc": "2.0", "method": "window/logMessage", "params": {"type": 3, "message": "hi"}})),
        frame(json!({"jsonrpc": "2.0", "id": 0, "method": "window/workDoneProgress/create", "params": {"token": "t"}})),
        // A hover whose text happens to name a notification is still an answer.
        frame(json!({"jsonrpc": "2.0", "id": 7, "result": {"contents": "func showMessage()"}})),
        frame(json!({"jsonrpc": "2.0", "id": 8, "result": [1]})),
    ]
    .concat();
    let mut reader = Cursor::new(stream.into_bytes());
    let mut replies = Vec::new();

    let first = parse(&read_response(&mut reader, &mut replies, 7).unwrap());
    assert_eq!(first["id"], 7);
    assert_eq!(first["result"]["contents"], "func showMessage()");

    // The server's own request was answered, so it isn't left waiting.
    let reply = String::from_utf8(replies).unwrap();
    let body = &reply[reply.find("\r\n\r\n").unwrap() + 4..];
    assert_eq!(
        parse(body),
        json!({"jsonrpc": "2.0", "id": 0, "result": null})
    );

    // Reading on from the same reader finds the next answer intact.
    let second = parse(&read_response(&mut reader, &mut Vec::new(), 8).unwrap());
    assert_eq!(second["result"], json!([1]));
}

#[test]
fn test_read_response_skips_answers_to_other_requests() {
    let stream = [
        frame(json!({"jsonrpc": "2.0", "id": 3, "result": null})),
        frame(json!({"jsonrpc": "2.0", "id": 4, "error": {"code": -32601, "message": "nope"}})),
    ]
    .concat();
    let mut reader = Cursor::new(stream.into_bytes());
    let answer = parse(&read_response(&mut reader, &mut Vec::new(), 4).unwrap());
    assert_eq!(answer["error"]["code"], -32601);
}
