.. title:: Editors

Emacs
-----

The primary client for ``diff-lsp`` is `diff-lsp.el <https://www.github.com/C-Hipple/diff-lsp.el>`_.

It provides integration with:
- Magit status buffers
- `code-review <https://www.github.com/C-Hipple/code-review>`_
- `code-review-server <https://www.github.com/C-Hipple/code-review-server>`_

Other Editors
-------------

Since ``diff-lsp`` follows the Language Server Protocol, it can theoretically be used with any editor that supports LSP, provided a client-side wrapper is written to handle the initialization tempfile and URI mapping.
