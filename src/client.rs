// Following

use std::{
    process::{Child, Stdio, Command},
    //thread::{spawn},
    //path::{PathBuf}, io::Read,
};
use std::path::PathBuf;
use tower_lsp::lsp_types::*;
use std::io::{Write, BufReader, BufRead};

use serde::Serialize;
use serde_json::Value;

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
        let binding = serde_json::to_string(&val).unwrap();
        // Also make the header
        let msg = format!("Content-Length: {}\r\n\r\n{}", binding.len(), binding);
        println!("Sending the string: {msg:?}");

        let _ = std_in.write_all(msg.as_bytes());
        let _ = std_in.flush();
        println!("Sent the message!");

        let std_err = self.process.stderr.as_mut().unwrap();
        let stderr_reader = BufReader::new(std_err);
        let stderr_lines = stderr_reader.lines();

        println!("Starting to read: ");
        for line in stderr_lines {
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
