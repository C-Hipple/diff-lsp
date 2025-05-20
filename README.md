# diff-lsp

Middleware Language server parsing diffs

Acts as a server from your editor's point-of-view, and as a client to your "backend" LSPs (such as rust-analyzer or gopls).


Editor -> diff-lsp -> [rust-analyzer, gopls, pylsp, etc]

Allows your editor to use the following lsp capabilities in diffs
- Hover
- Jump Definition
- Find References

`diff-lsp` follows the standard language-server protocol, so you can configure your clients to use this LSP server.  However, diff-lsp typically works on ephemeral buffers (such as git-status or code reviews), and LSP is a file-based protocol.  This means that for usage in these buffers, some client modifications are required.

See [Documentation](https://diff-lsp.readthedocs.io/en/latest/) for full docs

## Quickstart


### Emacs

See [diff-lsp.el](https://www.github.com/C-Hipple/diff-lsp.el) for configuring & running with emacs.


diff-lsp.el sets up diff-lsp for both magit status and [code-review](https://www.github.com/C-Hipple/code-review) buffers.
