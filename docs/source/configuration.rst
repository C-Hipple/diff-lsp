.. title:: Configuration

Initialization
--------------

``diff-lsp`` initializes itself by reading the most recent file in ``/tmp`` that matches the pattern ``diff_lsp_*``. This file contains the context for the current diff session.

Tempfile Format
~~~~~~~~~~~~~~~

The initialization tempfile supports the following fields:

* ``Root: <path>``: (Required) The absolute path to the project root.
* ``Worktree: <subfolder>``: (Optional) A subfolder within the root to use as the working directory for backend LSP clients.
* ``modified <file>``, ``new file <file>``, ``deleted <file>``: Used to detect which languages should be activated based on file extensions.
* ``diff --git ...``: Standard git diff headers are also parsed to detect active languages.

Worktree Integration
--------------------

Worktree integration allows ``diff-lsp`` to start backend LSP clients (like ``rust-analyzer`` or ``gopls``) in a specific subfolder of your project instead of the root.

How it works
~~~~~~~~~~~~

1. When ``diff-lsp`` reads the initialization tempfile, it looks for the ``Worktree:`` field.
2. If ``Worktree: <subfolder>`` is present, ``diff-lsp`` joins this subfolder with the ``Root`` path.
3. If the resulting path exists, ``diff-lsp`` uses this path as the current working directory when spawning backend LSP clients.
4. If the worktree subfolder does not exist, ``diff-lsp`` gracefully falls back to using the ``Root`` path.

This is particularly useful for monorepos or projects where the LSP should be scoped to a specific part of the codebase.
