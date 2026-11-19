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

When a result of References, Definition or Type Definition lands on a line shown in the diff, the
matching diff-buffer location is returned too. See :doc:`configuration` (Diff Locations).
