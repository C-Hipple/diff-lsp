use std::path::{PathBuf};
use std::fs::{OpenOptions, remove_file};
use std::io::prelude::*;

use log::{Log, Record, Level, Metadata, SetLoggerError, LevelFilter, info};
use tower_lsp::{LspService, Server};
use expanduser::expanduser;

mod server;
mod client;

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
    log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(LevelFilter::Info))
}

#[tokio::main]
async fn main() {
    println!("Hello, world!");

    let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());
    let (service, socket) = LspService::new(|client| server::DiffLsp::new( client));

    println!("Socket is: {socket:?}");
    let mut client = client::LspClient::new(
        "rust-analyzer".to_string()
        //"gopls".to_string()
        //"top".to_string()
        //"pwd".to_string()
    );

    client.initialize().unwrap();
    //println!("init res was: {init_res:?}");
    info!("Starting Logger");

    println!("Goodbye world.");
    Server::new(stdin, stdout, socket).serve(service).await;
}
