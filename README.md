# diff-lsp

Middleware Language server parsing diffs

Acts as a server from your editor's point-of-view, and as a client to your "backend" LSPs (such as rust-analyzer or gopls).


Editor -> diff-lsp -> [rust-analyzer, gopls, pylsp, etc]

Allows your editor to use the following lsp capabilities in diffs
- Hover
- TODO Jump Definition

## Usage

`diff-lsp` follows the standard language-server protocol, so you can configure your clients to use this LSP server and it should *just work* (lol)

### Emacs

See [diff-lsp.el](https://www.github.com/C-Hipple/diff-lsp.el) for configuring & running with emacs.


## TODO

[ ] config file update
[ ] it properly starts up when invoked by a "real" client.
