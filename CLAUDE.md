# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
cargo build                  # Debug build
cargo build --release        # Release build
cargo test                   # Run all tests
cargo test test_name         # Run a specific test
cargo fmt                    # Format code
cargo fmt --check            # Check formatting
make check                   # Build + fmt check + test (CI equivalent)
```

## Architecture

diff-lsp is a **middleware LSP server** written in Rust. It sits between a text editor and backend language servers (rust-analyzer, gopls, pylsp, typescript-language-server), enabling LSP features (hover, goto-definition, find-references, goto-type-definition) inside diff buffers.

**Flow:** Editor → diff-lsp → parse diff → map line numbers → forward to backend LSP → return results

### Key Modules

- **`src/server.rs`** — Main LSP server (`DiffLsp` struct) implementing `tower_lsp::LanguageServer`. Handles LSP requests, maintains diff maps and backend connections, translates diff positions to source file positions.
- **`src/client.rs`** — `ClientForBackendServer` spawns and manages child LSP processes, communicating via JSON-RPC over stdin/stdout.
- **`src/parsers/`** — Diff format parsers behind the `Parsable` trait. Implementations: `MagitDiff` (magit-status buffers), `CodeReviewDiff` (code-review formats). Produces `ParsedDiff` with line-number mappings.
- **`src/lib.rs`** — `SupportedFileType` enum (Rust/Go/Python/TypeScript), file-type-to-LSP-command mapping.
- **`src/main.rs`** — Entry point. Reads init params from `/tmp/diff_lsp_*` tempfile, detects languages, spawns backend LSPs, starts the server.

### Key Types

- `InputLineNumber(u16)` / `SourceLineNumber(u16)` — Newtype wrappers preventing confusion between diff-buffer line numbers and source-file line numbers.
- `ParsedDiff` — Contains `lines_map: HashMap<InputLineNumber, (String, DiffLine)>` mapping diff lines to source locations.
- `SourceMap` — Result of mapping a diff line to its source file, line number, and type.

### Initialization

The editor writes a tempfile at `/tmp/diff_lsp_*` containing `Root:`, optional `Worktree:`, and diff content. diff-lsp reads the most recent such file, detects file types from extensions in the diff, and spawns the appropriate backend LSP servers.

## Tests

Tests live in `tests/` with fixture data in `tests/data/`. Test files: `test_lib.rs` (parsing, line mapping), `test_server.rs` (init params, worktree), `test_code_review_server.rs` (code-review format), `test_raw_git_diff.rs` (git diff format).

## Logging

Logs to `~/.diff-lsp.log` (info level, timestamped).
