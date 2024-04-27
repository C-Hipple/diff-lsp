// Following

use std::{
    process::{Child, Stdio, Command}, collections::HashMap,
    //thread::{spawn},
    //path::{PathBuf}, io::Read,
};
use std::path::PathBuf;
use tower_lsp::lsp_types::*;
use std::io::{Write, BufReader, BufRead};
use anyhow::{anyhow, Result};

use serde::Serialize;
use serde_json::{Value, json};

use tower_lsp::jsonrpc::*;

pub struct LspClient {
    pub lsp_command: String,
    process: Child,
    #[allow(dead_code)]
    path: Option<PathBuf>
}

fn start_server(command: String) -> Result<Child>{
    let mut process = Command::new(&command);
    let child = process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    match child {
        Ok(c) => Ok(c),
        Err(_) => Err(Error::new(ErrorCode::ServerError(1)))
    }
}

impl LspClient {
    pub fn new(command: String) -> Self {
        LspClient {
            lsp_command: command.clone(),
            process: start_server(command.clone()).unwrap(),
            path: None,
        }
    }

    #[allow(deprecated)] // root_path is deprecated but without it, code doesn't compile? :(
    pub fn initialize(&mut self) -> Result<InitializeResult> {
        let params = InitializeParams{
            process_id: None,
            root_path: None,
            root_uri: None,
            initialization_options: None,
            capabilities: ClientCapabilities {
                workspace: None,
                text_document: {
                    Some(TextDocumentClientCapabilities {
                        hover: Some(HoverClientCapabilities::default()),
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
        let message = "initialize".to_string();

        let resp: InitializeResult = serde_json::from_value(self.send_request(message, params).unwrap()).unwrap();
        println!("We got the response: {resp:?}");

        return Ok(resp)
    }

    pub fn send_request<P: Serialize>(&mut self, message: String, params: P) -> Result<Value> {
        if message == "initialize".to_string() {
            let _ser_params = serde_json::to_value(params).unwrap();
            self.send_value_request(_ser_params)
        } else {
            Err(Error::new(ErrorCode::ServerError(4)))
        }

    }

    pub fn send_value_request<P: Serialize>(&mut self, val: P) -> Result<Value> {
        let std_in = self.process.stdin.as_mut().unwrap();
        // Also make the header
        let full_body = json!({
            "jsonrpc": "2.0".to_string(),
            "id": 1,
            "method": "initialize".to_string(), // TODO: Right method name?
            "params": &val,
        });
        let full_binding = serde_json::to_string(&full_body).unwrap();
        let msg = format!("Content-Length: {}\r\n\r\n{}", full_binding.len(), full_binding);
        println!("Sending the string: {msg:?}");

        let _ = std_in.write_all(msg.as_bytes());
        let _ = std_in.flush();

        println!("Sent the message!");

        let std_out = self.process.stdout.as_mut().unwrap();
        let stdout_reader = BufReader::new(std_out);
        let stdout_lines = stdout_reader.lines();

        println!("Starting to read: from std_out");
        for line in stdout_lines {
            println!("Read: {:?}", line.unwrap());
        }

        Ok(
            serde_json::Value::default()
        )

    }

    //pub fn send_request<P: Serialize>(&mut self, message: String, _params: Option<P>) -> Result<String> {
    pub fn send_raw_request(&mut self, message: String) -> Result<String> {
        let mut std_in = match &self.process.stdin {
            Some(thing) => thing,
            None => return Err(Error::new(ErrorCode::ServerError(2)))
        };

        std_in.write(message.as_bytes()).unwrap();
        println!("Sent the message!");

        let std_out = self.process.stdout.as_mut().unwrap();
        let stdout_reader = BufReader::new(std_out);
        let stdout_lines = stdout_reader.lines();

        println!("Starting to read");

        for line in stdout_lines {
            println!("Read: {:?}", line.unwrap());
        }

        Ok("Read!".to_string())

        // match self.process.stdout {
        //     Some(ref mut out) => {
        //         let mut s = String::new();
        //         out.read_to_string(&mut s);
        //         return Ok(s)
        //     }
        //     None => return Err(Error::new(ErrorCode::ServerError(2)))
        // };
    //Err(_) => Err(Error::new(ErrorCode::ServerError(2)))
    }
}

pub enum LspHeader {
    ContentType,
    ContentLength(usize),
}

fn parse_header(s: &str) -> Result<LspHeader> {
    let split: Vec<String> =
        s.splitn(2, ": ").map(|s| s.trim().to_lowercase()).collect();
    if split.len() != 2 {
        return Err(anyhow!("Malformed"));
    };
    match split[0].as_ref() {
        HEADER_CONTENT_TYPE => Ok(LspHeader::ContentType),
        HEADER_CONTENT_LENGTH => {
            Ok(LspHeader::ContentLength(split[1].parse::<usize>()?))
        }
        _ => Err(anyhow!("Unknown parse error occurred")),
    }
}

pub fn read_message<T: BufRead>(reader: &mut T) -> Result<String> {
    let mut buffer = String::new();
    let mut content_length: Option<usize> = None;

    loop {
        buffer.clear();
        let _ = reader.read_line(&mut buffer)?;
        match &buffer {
            s if s.trim().is_emptry() => break,
            s => {
                match parse_header(s)? {
                    LspHeader::ContentLength(len) => content_length = Some(len),
                    LspHeader::ContentType => (),
                };
            }
        };
    }

    let content_length = content_length.ok_or_else(|| anyhow!("Missing content-length header: {}", buffer))?;

    let mut body_buffer = vec![0: content_length];
    reader.read_exact(&mut body_buffer)?;

    let body = String::from_utf8(body_buffer)?;
    Ok(body)
}
