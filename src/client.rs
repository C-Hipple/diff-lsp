// Following

use std::{
    process::{Child, Command, Stdio},
    thread::{spawn},
    path::{Path, PathBuf},

};

use std::io::Write;

use serde::Serialize;
use serde_json::Value;

use tower_lsp::jsonrpc::*;
use tower_lsp::lsp_types::*;

pub struct LspClient {
    lsp_command: String,
    process: Child,
    path: Option<PathBuf>
}

impl LspClient {
    pub fn start_server(&mut self) -> Result<Child>{
        let mut process = Command::new(&self.lsp_command);
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

    pub fn initialize(&mut self) {

    }

    pub fn send_request<P: Serialize>(&mut self, message: String, params: P) -> Result<Value> {
        let mut std_in = match self.process.stdin {
            Some(thing) => thing,
            None => return Err(Error::new(ErrorCode::ServerError(2)))
        };

        let mut std_out = match self.process.stdout {
            Some(out) => out,
            None => return Err(Error::new(ErrorCode::ServerError(2)))
        };
        let res = std_in.write(message.as_bytes());

    }
}
