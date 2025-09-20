use std::fs::{remove_file, OpenOptions};

use std::io::prelude::*;
use std::path::PathBuf;
use std::env;

use chrono::Local;
use expanduser::expanduser;
use log::{info, Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use tower_lsp::{LspService, Server};

use diff_lsp::server::{create_backends_map, read_initialization_params_from_tempfile, DiffLsp};
use diff_lsp::utils::fetch_origin_nonblocking;

const LOG_FILE_PATH: &str = "~/.diff-lsp.log";

fn logfile_path() -> PathBuf {
    // println!("setting logfile path");
    expanduser(LOG_FILE_PATH).unwrap()
}

struct FileLogger;

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let mut file = match OpenOptions::new()
                .create(true)
                .append(true)
                .open(logfile_path())
            {
                Ok(file) => file,
                Err(e) => {
                    eprintln!("Couldn't open log file: {}", e);
                    return;
                }
            };

            let now = Local::now();
            let formatted_time = now.format("%Y-%m-%d %H:%M:%S");

            if let Err(e) = writeln!(
                file,
                "{} - {} - {}",
                formatted_time,
                record.level(),
                record.args()
            ) {
                eprintln!("Couldn't write to log file: {}", e);
            }
        }
    }

    fn flush(&self) {
        remove_file(logfile_path()).unwrap();
    }
}

static LOGGER: FileLogger = FileLogger;

pub fn initialize_logger() -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Info))
}

#[tokio::main]
async fn main() {
    let _ = initialize_logger().unwrap();
    info!("starting printing args!");
    for arg in env::args() {
        info!("arg!{:?}", arg);

    }

    info!("done printing args!");
    let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());
    let tempfile_path = expanduser("~/.diff-lsp-tempfile").unwrap();
    let (cwd, langs) = read_initialization_params_from_tempfile(&tempfile_path).unwrap();
    fetch_origin_nonblocking(&cwd);
    let backends = create_backends_map(langs, &cwd);
    let (diff_lsp_service, socket) =
        LspService::new(|client| DiffLsp::new(client, backends, cwd.to_string()));

    info!("Starting server@{:?}", cwd);

    Server::new(stdin, stdout, socket)
        .serve(diff_lsp_service)
        .await;
    println!("Goodbye world.");
}
