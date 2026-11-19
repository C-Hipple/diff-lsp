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

Initialization Options
----------------------

``diff-lsp`` reads the following keys from the LSP ``initializationOptions`` sent by your editor:

* ``diagnostics``: which diff lines receive backend diagnostics.

  * ``"added"`` (default): only added (``+``) lines, i.e. new problems introduced by the change.
  * ``"visible"``: added and unmodified context lines.
  * ``"off"``: never publish diagnostics to diff buffers.

.. code-block:: json

   {"diagnostics": "visible"}

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

Diff Locations
--------------

For references, goto-definition and goto-type-definition, backend LSPs return locations in real
source files. When such a location falls on a source line that is visible in the diff buffer (an
added or unmodified line of a hunk for that file), ``diff-lsp`` additionally returns the matching
location inside the diff buffer, so navigation during a code review can stay in the review.

The diff location uses the diff buffer's URI, the diff buffer line, and the character shifted past
the diff line's marker: the same marker width used when mapping requests to the backend (1 column
for ``+``/``-``/space; 2 columns for code-review-server buffers, which render
``"+ "``/``"- "``/``"  "``).

Configure it via ``initializationOptions``:

.. code-block:: json

   { "diffLocations": "both" }

* ``both`` (default): diff locations first, followed by all source locations, deduplicated.
  A single (scalar) definition becomes a list ``[diff location, source location]``.
* ``prefer``: source locations that are visible in the diff are replaced by their diff location;
  all other locations are returned as-is.
* ``off``: backend results are returned unchanged.

``LocationLink`` results map ``targetUri``, ``targetSelectionRange`` and ``targetRange`` (if the
end of the target range is not visible in the diff it collapses to the selection start).
Locations outside the project ``Root`` and removed lines are never mapped.

Emacs (lsp-mode) example:

.. code-block:: elisp

   (lsp-register-client
    (make-lsp-client
     ;; ...
     :initialization-options '(:diffLocations "prefer")))
