use std::fs::{File, OpenOptions};
use std::io::{prelude::*, LineWriter};
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Local;
use expanduser::expanduser;
use log::{info, LevelFilter, Log, Metadata, Record, SetLoggerError};
use tower_lsp::{LspService, Server};

use diff_lsp::server::{create_backends_map, read_initialization_params_from_tempfile, DiffLsp};
use diff_lsp::utils::{fetch_origin_nonblocking, get_most_recent_file};

const LOG_FILE_PATH: &str = "~/.diff-lsp.log";
/// Set to e.g. `debug` or `trace` to raise verbosity (default: `info`).
const LOG_LEVEL_ENV: &str = "DIFF_LSP_LOG";

fn logfile_path() -> PathBuf {
    expanduser(LOG_FILE_PATH).unwrap()
}

/// Logs to a file that is opened once at startup. `LineWriter` flushes after
/// every newline, so each record reaches disk promptly without re-opening the
/// file per call.
struct FileLogger {
    file: Mutex<LineWriter<File>>,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let formatted_time = Local::now().format("%Y-%m-%d %H:%M:%S");
        // Recover from a poisoned lock rather than dropping logs forever.
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
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

    fn flush(&self) {
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        let _ = file.flush();
    }
}

fn level_from_env() -> LevelFilter {
    std::env::var(LOG_LEVEL_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<LevelFilter>().ok())
        .unwrap_or(LevelFilter::Info)
}

pub fn initialize_logger() -> Result<(), SetLoggerError> {
    let file = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(logfile_path())
    {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Couldn't open log file: {}", e);
            return Ok(());
        }
    };
    // Leaked once at startup so the logger lives for 'static.
    let logger: &'static FileLogger = Box::leak(Box::new(FileLogger {
        file: Mutex::new(LineWriter::new(file)),
    }));
    log::set_logger(logger).map(|()| log::set_max_level(level_from_env()))
}

#[tokio::main]
async fn main() {
    if let Err(e) = initialize_logger() {
        eprintln!("Failed to initialize logger: {}", e);
        return;
    }
    let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());

    // An explicit tempfile path may be passed as the first argument so a
    // client can pin the init params it just wrote instead of racing other
    // sessions for the most recently modified /tmp/diff_lsp_* file.
    let tempfile_path = match std::env::args().nth(1) {
        Some(arg) => {
            let path = PathBuf::from(&arg);
            if !path.is_file() {
                eprintln!("Initialization tempfile {:?} does not exist", path);
                return;
            }
            path
        }
        None => match get_most_recent_file("/tmp", "diff_lsp_") {
            Ok(Some(path)) => path,
            Ok(None) => {
                eprintln!("No initialization tempfile found in /tmp/diff_lsp_*");
                return;
            }
            Err(e) => {
                eprintln!("Error searching for tempfile: {}", e);
                return;
            }
        },
    };

    info!("Looking at tempfile: {:?}", tempfile_path);
    let (cwd, worktree, langs) = match read_initialization_params_from_tempfile(&tempfile_path) {
        Ok(res) => res,
        Err(e) => {
            eprintln!(
                "Failed to read initialization params from {:?}: {}",
                tempfile_path, e
            );
            return;
        }
    };

    info!("hurr");
    fetch_origin_nonblocking(&cwd);

    let mut backend_root = cwd.clone();
    if let Some(wt) = worktree {
        // The worktree header is usually an absolute path (the
        // code-review-server writes one), but expand ~ and resolve
        // relative values against the root for other clients.
        let expanded_wt = expanduser(&wt).unwrap_or_else(|_| PathBuf::from(&wt));
        let wt_path = if expanded_wt.is_absolute() {
            expanded_wt
        } else {
            std::path::Path::new(&cwd).join(&expanded_wt)
        };
        if wt_path.is_dir() {
            info!("Using worktree at {:?}", wt_path);
            if let Ok(path_str) = wt_path.into_os_string().into_string() {
                backend_root = path_str;
            }
        } else {
            info!(
                "Worktree {:?} does not exist, falling back to root",
                wt_path
            );
        }
    }

    info!("Starting to create backends");
    let backends = match create_backends_map(langs, &backend_root) {
        Ok(b) => b,
        Err(e) => {
            info!("Errored on starting backends map: {:?}", e);
            eprintln!(
                "Failed to create backends for directory {}: {}",
                backend_root, e
            );
            return;
        }
    };
    info!("Done create backends");
    let (diff_lsp_service, socket) =
        LspService::new(|client| DiffLsp::new(client, backends, backend_root.to_string()));

    info!("Starting server@{:?}", backend_root);

    Server::new(stdin, stdout, socket)
        .serve(diff_lsp_service)
        .await;
    info!("Goodbye world.");
    log::logger().flush();
}
