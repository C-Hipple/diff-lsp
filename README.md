# diff-lsp

Middleware Language server parsing diffs

Acts as a server from your editor's point-of-view, and as a client to your "backend" LSPs (such as rust-analyzer or gopls).


Editor -> diff-lsp -> [rust-analyzer, gopls, pylsp, etc]

Allows your editor to use the following lsp capabilities in diffs
- Hover
- Jump Definition
- Find References

![Finding references in code review](docs/source/images/diff-lsp-references.png)

Finding the references for a code-review by calling out to rust-analyzer.

`diff-lsp` follows the standard language-server protocol, so you can configure your clients to use this LSP server.  However, diff-lsp typically works on ephemeral buffers (such as git-status or code reviews), and LSP is a file-based protocol.  This means that for usage in these buffers, some client modifications are required.

See [Documentation](https://diff-lsp.readthedocs.io/en/latest/) for full docs

## Quickstart


### Emacs

See [diff-lsp.el](https://www.github.com/C-Hipple/diff-lsp.el) for configuring & running with emacs.


diff-lsp.el sets up diff-lsp for both magit status and [code-review](https://www.github.com/C-Hipple/code-review) buffers.

## Limitations

Right now I have a limitation where you can't add more clients for backend servers in a single sesion with the LSP, and you have to restart it.  Normally this is not a problem and is trivial to do.  The reason is because of the types with the backends hashmap being not mutable I'd need to put the whole hashmap behind a mutex which gets me into a ton of rust typing headaches.
