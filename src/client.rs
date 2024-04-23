// Following

use std::{
    process::{Child, Stdio, Command},
    //thread::{spawn},
    path::{PathBuf}, io::Read,
};

use std::io::{Write, BufReader, BufRead};

//use serde::Serialize;
// use serde_json::Value;

use tower_lsp::jsonrpc::*;
// use tower_lsp::lsp_types::*;

pub struct LspClient {
    pub lsp_command: String,
    process: Child,
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

    pub fn initialize(&mut self) {

    }

    //pub fn send_request<P: Serialize>(&mut self, message: String, _params: Option<P>) -> Result<String> {
    pub fn send_request(&mut self, message: String) -> Result<String> {
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
