use std::fs::{read_to_string, remove_file, OpenOptions};

use regex::Regex;
use std::io::prelude::*;
use std::path::PathBuf;

use chrono::Local;
use expanduser::expanduser;
use log::{info, Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use tower_lsp::{LspService, Server};

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

pub fn init() -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Info))
}

#[tokio::main]
async fn main() {
    let _ = init().unwrap();
    let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());
    let tempfile_path = expanduser("~/.diff-lsp-tempfile").unwrap();
    let mut cwd = String::new();
    if let Ok(input) = read_to_string(tempfile_path) {
        let re = Regex::new(r"^Root:\s(.*)").unwrap();
        for line in input.lines() {
            if let Some(caps) = re.captures(line) {
                cwd = caps.get(1).unwrap().as_str().to_string();
                break;
            }
        }
    } else {
        panic!("Unable to read coordination temp-file")
    }
    let backends = get_backends_map(&cwd);
    let (diff_lsp_service, socket) =
        LspService::new(|client| DiffLsp::new(client, backends, cwd.to_string()));

    info!("Starting server@{:?}", cwd);

    Server::new(stdin, stdout, socket)
        .serve(diff_lsp_service)
        .await;
    println!("Goodbye world.");
}
