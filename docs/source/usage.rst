.. title:: Usage

Basic Usage
-----------

``diff-lsp`` is designed to be used as a middleware LSP server. It listens on stdin/stdout and communicates with backend LSP servers.

It is typically invoked by an editor plugin that prepares an initialization tempfile in ``/tmp/diff_lsp_*`` before starting the server.

Features
--------

- **Hover**: View documentation and type information.
- **Definition**: Jump to the source code of a symbol.
- **References**: Find all usages of a symbol.
- **Type Definition**: Jump to the definition of a symbol's type.
- **Document Symbols**: ``imenu`` and breadcrumb / headerline support for diff buffers.
- **Diagnostics**: Errors and warnings from the backend LSPs, shown on the matching diff lines.

When a result of References, Definition or Type Definition lands on a line shown in the diff, the
matching diff-buffer location is returned too. See :doc:`configuration` (Diff Locations).

Document Symbols
----------------

A ``textDocument/documentSymbol`` request on a diff buffer returns one top-level symbol of kind ``File`` per file in the diff, spanning that file's lines in the buffer. Its children are the symbols the backend language server reports for the real source file, translated into diff-buffer positions:

- Only symbols that intersect a visible (added or unmodified) hunk line are kept. Removed lines don't exist in the current source file, so they never anchor a symbol.
- Symbols extending beyond the hunk are clamped to its visible lines. If a symbol's name is outside the hunk, its selection range points at the start of the visible part.
- Nesting is preserved. Flat ``SymbolInformation`` responses are nested by range containment.
- Character offsets are shifted by the diff marker: one column for magit and git diffs, two for code-review-server's ``"+ foo"`` layout (the same ``ParsedDiff::marker_width`` the hover/definition handlers use).
- If a backend is missing or fails, the file's top-level entry is still returned.

With Emacs ``lsp-mode`` this works with ``imenu`` and ``lsp-headerline-breadcrumb-mode``.

Diagnostics
-----------

Backends publish diagnostics for *source* files.  ``diff-lsp`` keeps the latest set per file and maps
them onto every open diff buffer:

1. A backend diagnostic on (0-based) line ``n`` of ``foo.go`` is looked up as source line ``n + 1`` of
   ``foo.go`` in the diff (hunk headers are 1-based).
2. If that line is part of a hunk and passes the configured filter (by default: only added ``+`` lines),
   the diagnostic is published on that diff buffer line.
3. Character offsets are shifted past the diff marker (1 character for magit / git diffs, 2 for
   code-review-server's ``"+ "``).  Multi-line ranges are clamped to the end of the diff line.
4. The message is prefixed with ``<file>:<line>:`` so you can tell which file it came from.

Diagnostics on removed lines are never shown, since those lines don't exist in the new file.

Diagnostics are republished as soon as a backend publishes new ones, and sent for a diff buffer when
it is opened or refreshed.
