# The VS Code extension — notes for Claude Code

Plain JavaScript (`extension.js`), a TextMate grammar (`syntaxes/`), `language-configuration.json`
and `dazzle.natvis` for the Visual Studio debugger. It starts `dz lsp`; every answer comes from
the compiler, so a feature that needs analysis belongs in `crates/dz_lsp`, not here.

```bash
cd editors/vscode && npm install && npm run deploy    # build dazzle.vsix and install it
```

`deploy` installs with `--force`: the extension's version stays 0.1.0 with the language's, so
a plain `code --install-extension` sees the same version and silently does nothing.

A new keyword needs adding to the grammar here and to completion in `dz_lsp` — `static`,
`virtual`, `override` and `protected` were once missed in both.
