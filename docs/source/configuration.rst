.. title:: Configuration

Initialization
--------------

``diff-lsp`` initializes itself from an initialization file that contains the context for the current diff session. It looks for one in this order:

1. An explicit path passed on the command line: ``diff-lsp --init <path>``, ``--init=<path>``, or a bare positional ``diff-lsp <path>``. ``--init`` wins if both are given.
2. The ``DIFF_LSP_INIT_FILE`` environment variable.
3. The most recent file matching ``diff_lsp_*`` in the system temp directory (``$TMPDIR`` if set, which Emacs uses on macOS) or in ``/tmp``. If both have one, the newest wins.

An explicit path is the most reliable option when several editors or sessions run at the same time.

Backend LSPs
------------

``diff-lsp`` starts a backend LSP for each language found in the initialization file. A language that first shows up in a later diff gets its backend started on demand when that buffer is opened.

If a backend binary is missing or fails to initialize, ``diff-lsp`` keeps running without it and sends a ``window/showMessage`` warning such as ``pylsp not found; Python features disabled``. That backend isn't retried for the rest of the session.

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
