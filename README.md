# diff-lsp

Middleware Language server parsing diffs

Acts as a server from your editor's point-of-view, and as a client to your "backend" LSPs (such as rust-analyzer or gopls).


Editor -> diff-lsp -> [rust-analyzer, gopls, pylsp, etc]

Allows your editor to use the following lsp capabilities in diffs
- Hover
- Jump Definition
- Find References
- Jump to type definition
- Document symbols (imenu, breadcrumb / headerline)
- Diagnostics: errors/warnings from your backend LSPs shown on the diff lines (see below).

![Finding references in code review](docs/source/images/diff-lsp-references.png)

Finding the references for a code-review by calling out to rust-analyzer.

`diff-lsp` mostly follows the standard language-server protocol, so you can configure your clients to use this LSP server.  However, diff-lsp typically works on ephemeral buffers (such as git-status or code reviews), and LSP is a file-based protocol.  This means that for usage in these buffers, some client modifications are required.


See [Documentation](https://diff-lsp.readthedocs.io/en/latest/) for full docs

## Quickstart


### Emacs

See [diff-lsp.el](https://www.github.com/C-Hipple/diff-lsp.el) for configuring & running with emacs.

diff-lsp.el can manage diff-lsp for the following modes:
* magit status 
* [code-review-server emacs client](https://www.github.com/C-Hipple/code-review-server)
* [code-review](https://www.github.com/C-Hipple/code-review)

## Diagnostics

diff-lsp collects the `textDocument/publishDiagnostics` notifications sent by the backend LSPs for the files in your diff and re-publishes them on the matching lines of the diff buffer, so lsp-mode/flycheck (or any LSP client) shows them right in magit-status or your code review.

By default only diagnostics that start on an **added (`+`) line** are shown: the new problems introduced by the change. Configure this with `initializationOptions`:

```jsonc
{"diagnostics": "added"}    // default: only added lines
{"diagnostics": "visible"}  // added and unmodified context lines
{"diagnostics": "off"}      // never publish diagnostics to diff buffers
```

In Emacs with lsp-mode, for example: `(lsp-register-client (make-lsp-client ... :initialization-options '(:diagnostics "visible")))`.

Each message is prefixed with the source file and line (`config.go:22: ...`) since a diff buffer covers many files. Ranges are shifted past the diff marker and multi-line ranges are cut off at the end of the diff line.

Diagnostics are delivered as the backends publish them (e.g. right after rust-analyzer finishes `cargo check`), and re-sent when a diff buffer is opened or refreshed.

## Diff locations (staying in the review)

For **Find References**, **Jump Definition** and **Jump to type definition**, backend LSPs return locations in real source files.  When a returned location falls on a line that is visible in the diff (an added or unmodified line of a hunk for that file), diff-lsp also returns the matching location *inside the diff buffer* (the diff buffer's own URI, the diff line, and the column shifted past the `+`/`-`/space marker, using the same marker width as the request mapping — 2 columns for code-review-server buffers).  This lets "find references" during a code review jump around the review instead of opening files.

Configure it with `initializationOptions` from your editor:

```json
{ "diffLocations": "both" }
```

| value    | behaviour |
|----------|-----------|
| `both` (default) | Diff locations first, then all source locations (deduplicated). |
| `prefer` | Each source location visible in the diff is replaced by its diff location; others are kept. |
| `off`    | Backend results are returned unchanged. |

Locations outside the project root (std lib, dependencies) and lines that are not shown in the diff (including removed lines) are returned as plain source locations.

## Backend startup

Backend LSPs are started for the languages found in the initialization file. If a later diff contains a new language, its backend is started on demand when the buffer is opened; no restart needed.

If a backend binary is missing (e.g. `pylsp` isn't installed) or fails to initialize, diff-lsp keeps running without it and shows a warning such as `pylsp not found; Python features disabled`. It doesn't retry that backend for the rest of the session.

Hover, definition, type definition and references return an empty result (not an error) on lines that don't map to source, like headers, comments and blank lines.

## Initialization file

diff-lsp reads its context (`Root:`, optional `Worktree:`, and the diff) from an initialization file. It looks for one in this order:

1. A path on the command line: `diff-lsp --init <path>`, `--init=<path>`, or a bare `diff-lsp <path>` (`--init` wins if both are given)
2. The `DIFF_LSP_INIT_FILE` environment variable
3. The newest `diff_lsp_*` file in the system temp dir (`$TMPDIR`, as used by Emacs on macOS) or `/tmp`

## Worktree Integration

`diff-lsp` supports an optional `Worktree:` parameter in its initialization tempfile. If provided, `diff-lsp` will attempt to start backend LSP clients in the specified subfolder. This is useful for monorepos or when working in a specific part of a large project. If the worktree subfolder does not exist, it gracefully falls back to the project root.

## Document Symbols (imenu / breadcrumb)

`textDocument/documentSymbol` on a diff buffer returns one top-level symbol (kind `File`) per file in the diff, spanning that file's lines in the buffer.  Underneath it are the symbols the backend LSP reports for the real source file, filtered to the ones that touch a visible (added or unmodified) hunk line and translated into diff-buffer positions.

- A symbol that starts or ends outside the hunk (e.g. a function whose signature isn't in the diff) is clamped to the visible lines; if its name isn't visible, its selection range points at the start of the visible part.
- Symbols with no visible lines are dropped; nesting (impl > fn, struct > field, ...) is preserved.
- Files without a running backend (or whose backend fails) still get their top-level entry, so imenu can jump between files.
- Both hierarchical (`DocumentSymbol[]`) and flat (`SymbolInformation[]`) backend responses are supported; flat responses are nested by range containment.

In Emacs this powers `imenu` and lsp-mode's headerline breadcrumb (`lsp-headerline-breadcrumb-mode`) in magit-status and code-review buffers.
