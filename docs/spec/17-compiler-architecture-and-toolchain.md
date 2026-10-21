# 17. Compiler architecture and toolchain

## 17.1 Implementation

The compiler is written in **Rust** `[DECIDED]`, as a Cargo workspace. The existing Odin
lexer is the reference for §2 and is ported, not extended.

```
crates/
  dz_span        byte spans, source maps, file ids
  dz_diag        diagnostic model + rendering (codespan-style, with notes and fix-its)
  dz_lex         tokens (§2)
  dz_ast         untyped syntax tree, one node per source construct
  dz_parse       recursive-descent + Pratt expressions, error-recovering
  dz_macro       macro grammar, matching, hygienic expansion (§12)
  dz_resolve     modules, imports, visibility, name binding (§3, §4)
  dz_hir         resolved, desugared tree
  dz_check       types, traits, generics, definition-site checking (§5, §8)
  dz_flow        moves, drops, must-use, use-before-init (§6)
  dz_mono        monomorphization + compile-time folding (§8.4)
  dz_cpp         C++ emission, mangling, #line mapping (§16)
  dz_cppimport   libclang header importer (§15)
  dz_project     dazzle.toml parsing, dependency graph, build orchestration
  dz_driver      the pipeline as a library, shared by the CLI and the LSP (§17.2)
  dz_lsp         language server (§17.4)
  dz_fmt         formatter
  dz             the CLI
```

## 17.2 Pipeline

```
 source .dz
     │  dz_lex        tokens
     │  dz_parse      AST (recovers from errors; always produces a tree)
     │  dz_macro      AST with macros expanded
     │  dz_resolve    every name bound to a definition; visibility enforced
     │  dz_check      every expression typed; traits solved; generics checked once
     │  dz_flow       ownership, moves, drops inserted, must-use verified
     │  dz_mono       generics instantiated; comptime branches folded
     │  dz_cpp        C++ text + #line map
     ▼
 build/cpp/*.cpp ──► clang++ / MSVC ──► binary
```

Two properties are load-bearing:

- **The parser always produces a tree.** It never bails on the first error, because the LSP
  runs the same parser on half-typed code every keystroke.
- **Every phase is a pure function of its input**, so results can be cached per module and
  the LSP can re-run only what changed. Incremental caching starts as a simple per-module
  content-hash cache; if that stops being enough, `salsa` is the upgrade path.

## 17.3 CLI

```
dz new <name>          scaffold a project
dz build               generate C++ and compile
dz run                 build and execute
dz check               type-check only, no codegen (what the editor calls)
dz test                run #[test] functions
dz fmt                 format in place
dz expand <file>       print post-macro-expansion source (§12.6)
dz doc                 generate docs from /// comments
dz lsp                 language server on stdio
dz cpp                 emit C++ only, no compilation
```

## 17.4 Language server

Sharing every crate above — the server is not a reimplementation, it is the compiler with a
different front end. It is built on `lsp-server`, a synchronous loop, rather than
`tower-lsp`: the compiler's types are single-threaded (`Ty` holds `Rc`s), so an analysis
cannot live in the `Send + Sync` state an async server needs. Target feature set, in
priority order:

1. Diagnostics on save, then on keystroke.
2. Hover: type, doc comment, and for a fat trait pointer or `own` field, a note saying so.
3. Go to definition, find references, rename (rename is safe here because there is no
   overloading — one name, one symbol).
4. Completion: members after `.`, module members after `::`, struct-literal field names,
   `case` arms for a union, **and inside generic bodies**, which is the payoff for
   definition-site checking (§8.1). A bare name also offers the exports of dependencies
   that are not opened, and choosing one writes it with its key: typing `DrawLi` inserts
   `rl::DrawLine` (§3.2). In `dazzle.toml`, a header in a recipe's `import` and a
   dependency's `path` are links to what they name.
5. Completion inside macro invocations, using the macro's declared grammar (§12).
6. Code actions: fill missing `switch` arms, add a missing trait method, insert `move()` on
   a use-after-move error, add a missing `impl Drop`.
7. Inlay hints: inferred types on `let`, and — specific to this language — a marker where
   the compiler inserts an implicit `drop`.
8. Semantic tokens, document symbols, workspace symbols, formatting.

## 17.5 Testing strategy

The snapshot approach already in `tests/runner.odin` is the right one and carries over.

| Layer | Method |
|---|---|
| Lexer | `tests/lex/*.dz` + `.snap` token dumps (the existing corpus, ported) |
| Parser | `tests/parse/*.dz` + `.snap` AST dumps, including deliberately broken input |
| Diagnostics | `tests/errors/*.dz` with the expected rendered error as the snapshot — every error code in the spec gets one |
| Type checking | `tests/check/*.dz`, pass and fail cases |
| Ownership | `tests/flow/*.dz` — use-after-move, double-drop, partial move, must-use |
| Macros | `tests/expand/*.dz` + expanded-source snapshots |
| Codegen | `tests/cpp/*.dz` + generated-C++ snapshots |
| End to end | `tests/run/*.dz` compiled with a real C++ compiler, executed, stdout compared |
| Interop | a small vendored C and C++ header, imported and called |
| Fuzz | `cargo-fuzz` on lexer and parser: no panics, no hangs, ever |

`insta` provides `cargo insta review` for accepting snapshot changes, which replaces the
`.snap.new` file dance in the current runner.

Every `[DECIDED]` rule in this document should end up with at least one test named after
its section.

## 17.6 Hot reload — constraints to honour now

Hot reload is **not in v0.1**, but it is important enough that the design must not
accidentally foreclose it. Nothing here needs building yet; everything here is a rule that
later work must not break.

The target: `dz run --hot` builds each module as a shared library, watches the source, and
on change rebuilds and swaps that module without restarting the game or losing world state.
This is one command, not a mechanism you re-implement per project.

**What already points the right way.** Several decisions taken for other reasons happen to
be exactly what reload needs, and must stay that way:

| Decision | Why reload needs it |
|---|---|
| One module = one translation unit (§16.1) | gives a natural DLL boundary with no extra build config |
| No global constructors in generated code (§16.4) | a reloaded module has no hidden init order to re-run |
| Explicit allocators (§14) | memory allocated by the old DLL is freed by an allocator owned by the host, so a swap never orphans an allocation |
| The static type table (§13) | field names, types, and offsets are exactly what a state-migration pass needs |
| `#[export]` (§15.5) | already the declared, stable interface between host and module |

**What must be added later, and what it constrains now.**

1. **State migration.** On reload, live objects were laid out by the *old* build. The type
   table makes this tractable: walk old vs new `TypeInfo`, copy fields matched by name and
   type, default-initialise fields that are new, drop fields that disappeared. This requires
   the table to be complete for reloadable types, so `type_info = "all"` becomes implied by
   `--hot`.
2. **Vtable and function-pointer staleness.** Any dynamic-dispatch pointer held across a
   reload points into the unloaded DLL. **Constraint on §7.6:** a vtable must be
   addressable by a stable, deterministic id (a hash of the type and trait names) rather
   than only by its raw address, so it can be re-resolved after a swap. That is cheap to
   guarantee now and impossible to retrofit cheaply.
3. **Persistent state.** `#[persist]` on a module-level binding marks state that survives a
   reload; everything else is reconstructed. Not designed yet.
4. **Reload safety.** A swap may only happen at a point where no reloaded code is on the
   stack — in practice, the top of the frame loop.

`[PROPOSED]`
