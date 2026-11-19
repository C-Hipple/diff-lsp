//! Mapping backend diagnostics (from rust-analyzer, gopls, ...) onto diff buffers.
//!
//! Backends publish `textDocument/publishDiagnostics` for *source* files.  This module
//! keeps the latest diagnostics per source file ([`DiagnosticsStore`]) and translates them
//! onto the lines of a diff buffer ([`map_diagnostics_for_diff`]) so the editor can show
//! them (e.g. with flycheck / lsp-mode) directly in magit-status or a code review.
//!
//! Everything in here is pure / synchronous so it can be unit tested without a live
//! backend.  The wiring (capturing notifications from backends and publishing to the
//! editor) lives in `client.rs` and `server.rs`.
//!
//! # Line number conventions
//!
//! * Backend diagnostic ranges are 0-based (LSP).
//! * [`SourceLineNumber`] is 1-based (taken from the hunk headers), so a backend diagnostic
//!   on line `n` corresponds to `SourceLineNumber(n + 1)`.  This mirrors the request
//!   handlers in `server.rs`, which subtract 1 from the source line before forwarding.
//! * [`InputLineNumber`] keys of `ParsedDiff::lines_map` are looked up by the server using
//!   the editor's `position.line` *directly* (see `DiffLsp::map_position`).  To keep
//!   diagnostics consistent with hover/definition we use the inverse of that mapping:
//!   `InputLineNumber(k)` is published on LSP line `k`.  See [`input_line_to_lsp_line`].

use std::collections::HashMap;

use serde_json::Value;
use tower_lsp::lsp_types::{Diagnostic, Position, PublishDiagnosticsParams, Range, Url};

use crate::parsers::utils::{DiffLine, InputLineNumber, LineType, ParsedDiff, SourceLineNumber};

/// Which diff lines should receive diagnostics.
///
/// Configured through the editor's `initializationOptions`:
/// `{"diagnostics": "added" | "visible" | "off"}`.  Defaults to [`DiagnosticsMode::Added`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiagnosticsMode {
    /// Only diagnostics starting on added (`+`) lines: "new problems in this change".
    #[default]
    Added,
    /// Diagnostics on any line visible in the new file: added and unmodified context lines.
    Visible,
    /// Never publish diagnostics to diff buffers.
    Off,
}

impl DiagnosticsMode {
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "added" => Some(DiagnosticsMode::Added),
            "visible" | "all" => Some(DiagnosticsMode::Visible),
            "off" | "none" | "false" => Some(DiagnosticsMode::Off),
            _ => None,
        }
    }

    /// Reads the mode from `InitializeParams.initialization_options`.
    /// Unknown / missing values fall back to the default.
    pub fn from_initialization_options(options: Option<&Value>) -> Self {
        match options.and_then(|o| o.get("diagnostics")) {
            Some(Value::String(s)) => Self::from_str_opt(s).unwrap_or_default(),
            Some(Value::Bool(false)) => DiagnosticsMode::Off,
            _ => DiagnosticsMode::default(),
        }
    }

    /// Whether a diagnostic starting on a line of this type should be shown.
    pub fn accepts(&self, line_type: LineType) -> bool {
        match self {
            DiagnosticsMode::Added => line_type == LineType::Added,
            DiagnosticsMode::Visible => {
                matches!(line_type, LineType::Added | LineType::Unmodified)
            }
            DiagnosticsMode::Off => false,
        }
    }
}

/// Normalizes a file URI so that e.g. `file:///repo//src/a.rs` (from joining a root with a
/// trailing slash) and `file:///repo/src/a.rs` (as echoed by a backend) compare equal.
pub fn normalize_uri_key(uri: &Url) -> String {
    if uri.scheme() != "file" {
        return uri.as_str().to_string();
    }
    let mut parts: Vec<&str> = vec![];
    for seg in uri.path().split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("file:///{}", parts.join("/"))
}

/// Latest diagnostics per source file, as published by the backends.
#[derive(Debug, Default, Clone)]
pub struct DiagnosticsStore {
    by_file: HashMap<String, Vec<Diagnostic>>,
}

impl DiagnosticsStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the diagnostics of a file (LSP semantics: each publish is the full set).
    /// Returns true if the stored value changed.
    pub fn update(&mut self, params: PublishDiagnosticsParams) -> bool {
        let key = normalize_uri_key(&params.uri);
        if params.diagnostics.is_empty() {
            return self.by_file.remove(&key).is_some();
        }
        if self.by_file.get(&key) == Some(&params.diagnostics) {
            return false;
        }
        self.by_file.insert(key, params.diagnostics);
        true
    }

    pub fn get(&self, uri: &Url) -> Option<&Vec<Diagnostic>> {
        self.by_file.get(&normalize_uri_key(uri))
    }

    pub fn len(&self) -> usize {
        self.by_file.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_file.is_empty()
    }
}

/// Converts a diff-buffer input line number into the LSP line the editor uses for it.
/// This is the inverse of `DiffLsp::map_position`, which uses `position.line` directly
/// as the `lines_map` key.
pub fn input_line_to_lsp_line(input_line: InputLineNumber) -> u32 {
    input_line.0
}

/// Converts a 0-based backend (LSP) line to the 1-based [`SourceLineNumber`] used by the
/// diff parsers.
pub fn backend_line_to_source_line(line: u32) -> Option<SourceLineNumber> {
    line.checked_add(1).map(SourceLineNumber::new)
}

/// Reverse index: (relative file name, source line) -> diff buffer line.
///
/// Removed lines are skipped: they don't exist in the new file, and their
/// `source_line_number` points at the neighbouring new-file line.
///
/// For code-review-server buffers, completely empty lines are skipped too: every real
/// diff line there carries a two-character marker (`"  "` for an empty context line), and
/// the parser also records the blank separator line after a hunk as an unmodified line
/// with a bogus source line number.
#[derive(Debug, Default, Clone)]
pub struct ReverseIndex {
    map: HashMap<(String, u32), (InputLineNumber, DiffLine)>,
}

impl ReverseIndex {
    pub fn build(diff: &ParsedDiff) -> Self {
        let skip_empty = diff.marker_width > 1;
        let mut map: HashMap<(String, u32), (InputLineNumber, DiffLine)> = HashMap::new();
        for (input_line, (filename, diff_line)) in diff.lines_map.iter() {
            if diff_line.line_type == LineType::Removed {
                continue;
            }
            if skip_empty && diff_line.line.is_empty() {
                continue;
            }
            let key = (filename.clone(), diff_line.source_line_number.0);
            match map.get(&key) {
                // If a line somehow shows up twice (overlapping hunks), use the first one.
                Some((existing, _)) if existing <= input_line => {}
                _ => {
                    map.insert(key, (*input_line, diff_line.clone()));
                }
            }
        }
        ReverseIndex { map }
    }

    pub fn lookup(
        &self,
        filename: &str,
        source_line: SourceLineNumber,
    ) -> Option<&(InputLineNumber, DiffLine)> {
        self.map.get(&(filename.to_string(), source_line.0))
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

/// Translates a single backend diagnostic onto a diff buffer line.
///
/// * the line becomes `input_line` (via [`input_line_to_lsp_line`]);
/// * characters are shifted right by `prefix_width` (the diff marker,
///   [`ParsedDiff::marker_width`]);
/// * multi-line ranges are clamped to the end of the visible line;
/// * the message is prefixed with the file name so it's clear where it came from.
pub fn map_diagnostic(
    diag: &Diagnostic,
    filename: &str,
    input_line: InputLineNumber,
    diff_line: &DiffLine,
    prefix_width: u32,
) -> Diagnostic {
    let line = input_line_to_lsp_line(input_line);
    let line_len = utf16_len(&diff_line.line);

    let start_char = diag
        .range
        .start
        .character
        .saturating_add(prefix_width)
        .min(line_len);
    let end_char = if diag.range.end.line == diag.range.start.line {
        diag.range
            .end
            .character
            .saturating_add(prefix_width)
            .min(line_len)
    } else {
        line_len
    };
    let end_char = end_char.max(start_char);

    let source_line = diag.range.start.line + 1;
    let mut mapped = diag.clone();
    mapped.range = Range::new(
        Position::new(line, start_char),
        Position::new(line, end_char),
    );
    mapped.message = format!("{}:{}: {}", filename, source_line, diag.message);
    mapped
}

/// Maps every stored diagnostic that belongs to a file of `diff` onto the diff buffer.
///
/// `root` is the directory the backends were started in (same value used to build the
/// URIs sent in `didOpen`).  Results are sorted by position for deterministic output.
pub fn map_diagnostics_for_diff(
    diff: &ParsedDiff,
    root: &str,
    store: &DiagnosticsStore,
    mode: DiagnosticsMode,
) -> Vec<Diagnostic> {
    if mode == DiagnosticsMode::Off || store.is_empty() {
        return vec![];
    }
    let index = ReverseIndex::build(diff);
    let prefix_width = diff.marker_width;

    let mut filenames = diff.filenames.clone();
    filenames.sort();
    filenames.dedup();

    let mut out = vec![];
    for filename in filenames {
        // Same construction as `uri_from_relative_filename`, without panicking on a
        // relative root.
        let Ok(uri) = Url::from_file_path(format!("{}/{}", root, filename)) else {
            continue;
        };
        let Some(diags) = store.get(&uri) else {
            continue;
        };
        for diag in diags {
            let Some(source_line) = backend_line_to_source_line(diag.range.start.line) else {
                continue;
            };
            let Some((input_line, diff_line)) = index.lookup(&filename, source_line) else {
                continue;
            };
            if !mode.accepts(diff_line.line_type) {
                continue;
            }
            out.push(map_diagnostic(
                diag,
                &filename,
                *input_line,
                diff_line,
                prefix_width,
            ));
        }
    }
    out.sort_by_key(|d| (d.range.start.line, d.range.start.character));
    out
}

/// Parses a raw JSON-RPC message from a backend and returns the params if it is a
/// `textDocument/publishDiagnostics` notification.
pub fn parse_publish_diagnostics(body: &str) -> Option<PublishDiagnosticsParams> {
    publish_diagnostics_from_notification(&serde_json::from_str(body).ok()?)
}

/// Returns the params if `msg` (a backend notification, as delivered by
/// `ClientForBackendServer::set_notification_handler`) is a
/// `textDocument/publishDiagnostics` notification.
pub fn publish_diagnostics_from_notification(msg: &Value) -> Option<PublishDiagnosticsParams> {
    if msg.get("method")?.as_str()? != "textDocument/publishDiagnostics" {
        return None;
    }
    serde_json::from_value(msg.get("params")?.clone()).ok()
}
