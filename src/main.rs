use std::env::current_dir;
use std::fs::{OpenOptions, remove_file};
use std::io::prelude::*;
use std::path::PathBuf;

use expanduser::expanduser;
use log::{info, Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use tower_lsp::{LspService, Server};
use chrono::Local;

use diff_lsp::server::{get_backends_map, DiffLsp};

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
        if self.enabled(record.metadata()) {
            let mut file = match OpenOptions::new()
                .create(true) // Create the file if it doesn't exist
                .append(true)
                .open(logfile_path())
            {
                Ok(file) => file,
                Err(e) => {
                    eprintln!("Couldn't open log file: {}", e);
                    return;
                }
            };

            // Get the current timestamp
            let now = Local::now();
            let formatted_time = now.format("%Y-%m-%d %H:%M:%S");

            if let Err(e) = writeln!(file, "{} - {} - {}", formatted_time, record.level(), record.args()) {
                eprintln!("Couldn't write to log file: {}", e);
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
    let _ = init().unwrap();
    let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());
    let cwd = current_dir().unwrap();
    let backends = get_backends_map(cwd.to_str().unwrap());
    let root: String = expanduser("~/diff-lsp").unwrap().display().to_string();
    let (diff_lsp_service, socket) =
        LspService::new(|client| DiffLsp::new(client, backends, None, root));

    info!("Starting Logger");

    Server::new(stdin, stdout, socket)
        .serve(diff_lsp_service)
        .await;
    println!("Goodbye world.");
}
