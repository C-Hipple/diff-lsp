use std::fs::{remove_file, OpenOptions};
use std::io::prelude::*;
use std::path::PathBuf;
use std::collections::HashMap;

use expanduser::expanduser;
use log::{info, Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use tower_lsp::{LspService, Server};

mod client;
mod server;

fn logfile_path() -> PathBuf {
    expanduser("~/.diff-lsp.log").unwrap()
}

struct FileLogger;

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        println!("Logging: {}", record.args());
        if self.enabled(record.metadata()) {
            let mut file = OpenOptions::new()
                .append(true)
                .open(logfile_path())
                .unwrap();

            if let Err(e) = writeln!(file, "{} - {}", record.level(), record.args()) {
                eprintln!("Couldn't Write to file {}", e);
            }
        }
    }

    fn flush(&self) {
        remove_file(logfile_path()).unwrap();
    }
}

static LOGGER: FileLogger = FileLogger;

pub fn init() -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Info))
}

#[tokio::main]
async fn main() {
    println!("Hello, world!");

    let mut rust_analyzer = client::BackendLspClient::new("rust-analyzer".to_string());
    let mut gopls = client::BackendLspClient::new("gopls".to_string());

    let mut backends: HashMap<String, client::BackendLspClient> = HashMap::new();
    backends.insert("rust".to_string(), rust_analyzer);
    backends.insert("go".to_string(), gopls);

    let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());

    // Set up our middleware lsp
    let (service, socket) = LspService::new(|client| server::DiffLsp::new(client, backends));

    // Testing to make sure we can properly interface with teh backends
    let mut rust_analyzer2 = client::BackendLspClient::new("rust-analyzer".to_string());
    rust_analyzer2.initialize().unwrap();
    //println!("init res was: {init_res:?}");
    info!("Starting Logger");

    println!("Goodbye world.");
    Server::new(stdin, stdout, socket).serve(service).await;
}
