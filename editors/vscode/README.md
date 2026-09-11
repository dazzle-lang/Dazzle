# Dazzle for VS Code

Language support for [Dazzle](../../docs/spec.md). The extension only starts the compiler's
own language server, `dz lsp` (spec §17.4), so everything it knows comes from the compiler.

- **Highlighting**: a TextMate grammar, refined by the compiler's semantic tokens. Parameters,
  locals, fields, methods, variants and constants are coloured by what they resolve to, so
  the colour of a name matches what hover says about it.
- **Errors and warnings as you type**, with the compiler's codes (`DZ0412`) and related spans
  (a use-after-move also shows where the move was). While there is a syntax error, only
  syntax errors are shown; type errors come back once it parses.
- **Unused bindings** get a warning and are drawn faded. Name one `_x` to say it's unused
  on purpose, for example a guard kept only for its `drop`.
- **Hover**: the type of a local or expression, or a declaration's signature and `///` docs.
- **Go to definition, find references, highlight**: locals, functions, types, fields,
  methods (including `impl` methods and inherited trait defaults), variants, constructors.
- **Rename** across the project. A struct is renamed together with its constructors, and a
  trait method together with the methods implementing it.
- **Completion**: names in scope, keywords and builtins; after `.`, the members of whatever
  expression comes before it (`a.b().` included); after `::`, module items, variants and
  type queries.
- **Signature help** inside a call, and **inlay hints** showing inferred types on `let`,
  `for` and `case` bindings.
- Outline, **Go to Symbol in Workspace**, and Format Document.
- **Run**: the ▷ button in the editor's title bar (or Ctrl+F5) saves everything and runs
  `dz run` on the file's project, the nearest folder above it holding a `dazzle.toml`. **Check
  Project**, next to it, runs `dz check`. Output appears in the terminal panel.

Everything except rename keeps working next to a syntax error: the compiler still resolves
and types the code around it.

## Setup

Build the compiler, then build and install the extension:

```bash
cargo build -p dz
cd editors/vscode
npm install
npm run deploy
```

`deploy` packages `dazzle.vsix` and installs it with `--force`. The version stays 0.1.0, and
without `--force` VS Code keeps the build it already has. Then run **Developer: Reload
Window**.

The extension looks for `dz` in this order:

1. the `dazzle.server.path` setting (`${workspaceFolder}` is expanded),
2. `target/debug/dz` or `target/release/dz` inside an open workspace folder,
3. `dz` on `PATH`.

When you work on a Dazzle project outside this repository, point `dazzle.server.path` at the
compiler build, for example `C:/path/to/dazzle/target/debug/dz.exe`.

The extension runs a copy of `dz`, so `cargo build` can replace the binary while the editor is
open. When the binary changes, the server restarts on its own (turn this off with
`dazzle.server.restartOnRebuild`). **Dazzle: Restart Language Server** restarts it by hand.
Server output, including any compiler panic, is in the **Dazzle** output channel.

## Limits

These are listed in `docs/PROGRESS.md`:

- Rename waits until the project parses, because code that doesn't parse could hide uses
  of the name.
- Not yet available: code actions (quick fixes), or hints for where the compiler inserts a
  `drop`.
- The whole project is re-checked after each pause in typing. That's fast at today's sizes,
  but it isn't incremental yet.
