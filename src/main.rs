use std::fs::{remove_file, OpenOptions};
use std::io::prelude::*;
use std::path::PathBuf;
use std::env::current_dir;


use diff_lsp::SupportedFileType;
use expanduser::expanduser;
use log::{info, Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use tower_lsp::{LspService, Server};

mod client;
mod server;
mod test_data;

fn logfile_path() -> PathBuf {
    println!("setting logfile path");
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

    let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());
    let cwd = current_dir().unwrap();
    let backends = server::get_backends_map(cwd.to_str().unwrap());
    let root: String = expanduser("~/diff-lsp").unwrap().display().to_string();
    let (diff_lsp_service, socket) = LspService::new(|client| server::DiffLsp::new(client, backends, None, root));

    // Testing to make sure we can properly interface with teh backends
    // let mut rust_analyzer2 = client::ClientForBackendServer::new("rust-analyzer".to_string());
    // rust_analyzer2.initialize().unwrap();
    info!("Starting Logger");

    println!("Goodbye world.");
    Server::new(stdin, stdout, socket).serve(diff_lsp_service).await;
}
