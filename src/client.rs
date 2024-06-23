use anyhow::{anyhow, Result};
use std::io::{BufRead, BufReader, Write, Read};
use std::path::PathBuf;

use std::{
    process::{Child, Command, Stdio},
    //thread::{spawn},
    //path::{PathBuf}, io::Read,
};
use tower_lsp::lsp_types::*;

use serde::Serialize;
use serde_json::{json, Value};

use tower_lsp::jsonrpc::*;

const HEADER_CONTENT_LENGTH: &str = "content-length";
const HEADER_CONTENT_TYPE: &str = "content-type";

#[derive(Debug)]
pub struct ClientForBackendServer {
    pub lsp_command: String,
    process: Child,
    #[allow(dead_code)]
    path: Option<PathBuf>,
    request_id: i32,

}

fn start_server(command: String) -> Result<Child> {
    let mut process = Command::new(&command);
    //process.current_dir();
    let child = process
        // TODO actually set teh current dir; will be easy once we start the servers when our server gets a didOpen
        .current_dir("/Users/chrishipple/diff-lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    match child {
        Ok(c) => Ok(c),
        Err(_) => Err(Error::new(ErrorCode::ServerError(1)).into()),
    }
}

impl ClientForBackendServer {
    pub fn new(command: String) -> Self {
        ClientForBackendServer {
            lsp_command: command.clone(),
            process: start_server(command.clone()).unwrap(),
            path: None,
            request_id: 1,
        }
    }

    fn get_request_id(&mut self) -> i32 {
        self.request_id = self.request_id + 1;
        self.request_id.clone()
    }

    #[allow(deprecated)] // root_path is deprecated but without it, code doesn't compile? :(
    pub fn initialize(&mut self) -> Result<InitializeResult> {
        let params = InitializeParams {
            process_id: None,
            root_path: None,
            root_uri: None,
            initialization_options: None,
            capabilities: ClientCapabilities {
                workspace: None,
                text_document: {
                    Some(TextDocumentClientCapabilities {
                        hover: Some(HoverClientCapabilities::default()),
                        references: Some(ReferenceClientCapabilities{dynamic_registration: Some(false)}),
                        ..Default::default()
                    })
                },
                window: None,
                general: None,
                experimental: None,
            },
            trace: None,
            workspace_folders: None,
            client_info: None,
            locale: None,
        };
        let method = "initialize".to_string();  // TODO: Is there an enum for this?
        // println!("Sending initialize to backend {}", self.lsp_command);
        let raw_resp = self.request(method, params).unwrap();
        let resp: InitializeResult = serde_json::from_value(raw_resp).unwrap();
        //println!("We got the response: {resp:?}");

        return Ok(resp);
    }

    pub fn initialized(&mut self) {
        // send the initialized notification
        self.notify("initialized".to_string(), InitializedParams{})
    }

    fn request<P: Serialize>(&mut self, method: String, params: P) -> Result<Value> {
        let ser_params = serde_json::to_value(params).unwrap();
        println!("Sending request {} to backend {}", method, self.lsp_command);
        let raw_resp = self.send_value_request(ser_params, method, true).unwrap();
        let as_value: Value = serde_json::from_str(&raw_resp).unwrap();
        Ok(as_value.get("result").unwrap().clone())
    }

    pub fn notify<P: Serialize>(&mut self, method: String, params: P) {
        // Just like a request, but does not expect a response.
        let ser_params = serde_json::to_value(params).unwrap();
        println!("Sending notification {} to backend {}", method, self.lsp_command);
        self.send_value_request(ser_params, method, false).unwrap();
    }

    fn send_value_request<P: Serialize>(&mut self, val: P, method: String, check_response: bool) -> Result<String> {
        let id = self.get_request_id();
        let std_in = self.process.stdin.as_mut().unwrap();
        // Also make the header
        let full_body;
        if check_response {
            full_body = json!({
                "jsonrpc": "2.0".to_string(),
                "id": id as usize,
                "method": method,
                "params": &val,
            });
        } else {
            full_body = json!({
                "jsonrpc": "2.0".to_string(),
                "method": method,
                "params": &val,
            });
        }
        let full_binding = serde_json::to_string(&full_body).unwrap();
        let msg = format!(
            "Content-Length: {}\r\n\r\n{}",
            full_binding.len(),
            full_binding
        );
        if method.contains("ized") {
            println!("msg: {}", msg);
        }

        let _ = std_in.write_all(msg.as_bytes());
        let _ = std_in.flush();

        if !check_response {
            // // was testing if maybe there was other error output
            // let std_err = self.process.stderr.as_mut().unwrap();
            // let mut stderr_reader = BufReader::new(std_err);
            // let mut body_buffer = vec![0; 200];
            // let _ = stderr_reader.read(&mut body_buffer);
            // println!("Backend stderr: {:?}", String::from_utf8(body_buffer));
            return Ok("".to_string())
        }

        let std_out = self.process.stdout.as_mut().unwrap();
        let mut stdout_reader = BufReader::new(std_out);

        let resp = read_message(&mut stdout_reader);
        match resp {
            Ok(r) => Ok(r),
            Err(e) => {
                let std_err = self.process.stderr.as_mut().unwrap();
                let mut stderr_reader = BufReader::new(std_err);
                let mut body_buffer = vec![0; 200];
                let _ = stderr_reader.read(&mut body_buffer).unwrap();
                println!("Backend stderr: {:?}", String::from_utf8(body_buffer));
                Err(e)
            }
        }
    }

    pub fn did_open(&mut self, params: &DidOpenTextDocumentParams) {
        self.notify("textDocument/didOpen".to_string(), params);
    }

    pub fn hover(&mut self, params: HoverParams) -> Result<Option<Hover>> {
        println!("Doing hover with teh params: {:?}", params);
        let res = self.request("textDocument/hover".to_string(), params).unwrap();
        println!("Got the hover res: {}", res);
        let hover_res: Result<Hover, serde_json::Error> = serde_json::from_value(res);
        match hover_res {
            Ok(parsed_res) => Ok(Some(parsed_res)),
            Err(_) => Ok(None)
        }}
}

pub enum LspHeader {
    ContentType,
    ContentLength(usize),
}

fn parse_header(s: &str) -> Result<LspHeader> {
    let split: Vec<String> = s.splitn(2, ": ").map(|s| s.trim().to_lowercase()).collect();

    if split.len() != 2 {
        return Err(anyhow!("Malformed"));
    };
    //println!("split as: {split:?}");

    //match split[0].as_ref() {
    match <std::string::String as AsRef<str>>::as_ref(&split[0]) {
        HEADER_CONTENT_TYPE => Ok(LspHeader::ContentType),
        HEADER_CONTENT_LENGTH => Ok(LspHeader::ContentLength(split[1].parse::<usize>()?)),
        _ => Err(anyhow!("Unknown parse error occurred")),
    }
}

pub fn read_message<T: BufRead>(reader: &mut T) -> Result<String> {
    let mut buffer = String::new();
    let mut content_length: Option<usize> = None;

    loop {
        buffer.clear();
        let _ = reader.read_line(&mut buffer)?;
        //println!("Buffer: {}", buffer);
        match &buffer {
            s if s.trim().is_empty() => break,
            s => {
                //println!("Found the string: {s:?}");
                match parse_header(s)? {
                    LspHeader::ContentLength(len) => content_length = Some(len),
                    LspHeader::ContentType => (),
                };
            }
        };
    }

    let content_length =
        content_length.ok_or_else(|| anyhow!("Missing content-length header: {}", buffer))?;

    let mut body_buffer = vec![0; content_length];
    reader.read_exact(&mut body_buffer)?;

    let body = String::from_utf8(body_buffer)?;
    // println!("body {}", body);
    Ok(body)
}
