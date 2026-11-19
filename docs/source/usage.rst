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

When a result of References, Definition or Type Definition lands on a line shown in the diff, the
matching diff-buffer location is returned too. See :doc:`configuration` (Diff Locations).

Document Symbols
----------------

A ``textDocument/documentSymbol`` request on a diff buffer returns one top-level symbol of kind ``File`` per file in the diff, spanning that file's lines in the buffer. Its children are the symbols the backend language server reports for the real source file, translated into diff-buffer positions:

- Only symbols that intersect a visible (added or unmodified) hunk line are kept. Removed lines don't exist in the current source file, so they never anchor a symbol.
- Symbols extending beyond the hunk are clamped to its visible lines. If a symbol's name is outside the hunk, its selection range points at the start of the visible part.
- Nesting is preserved. Flat ``SymbolInformation`` responses are nested by range containment.
- Character offsets are shifted by the diff marker: one column for magit and code-review buffers, two (``"+ "``) for code-review-server (``Type: my-code-review``) buffers.
- If a backend is missing or fails, the file's top-level entry is still returned.

With Emacs ``lsp-mode`` this works with ``imenu`` and ``lsp-headerline-breadcrumb-mode``.
