# The build log

What each milestone built and decided, one file each. It is the record, not the plan:
**open work is the backlog, `docs/tasks.md`**, and what changed in the language is spec §21. Read a
file here when you need to know *why* something is the way it is; nothing here is needed to
decide what to do next.

When a milestone or revision lands, add its section to the matching file (newest first
within a file is fine; the files are not ordered among themselves), and set the tasks it closes
to `status: done` in `docs/tasks/`.

| File | Covers |
|---|---|
| `m0-m4.md` | lexer, parser, formatter, modules, type checker, codegen and `dz run` |
| `m5.md` | ownership: `own`, `move`, `copy`, `drop`, flow analysis, `dz_hir` |
| `m6.md` | generics, traits, monomorphization; r12–r17 (`unsafe`, `impl`, `move`, `if let`) |
| `m7.md` | C and C++ interop from vendored source, r18–r29 |
| `m8.md` | `switch` on constants, generic unions, r36–r38, the feature matrix |
| `r30-inheritance.md` | struct inheritance, its plan and r30 |
| `m9.md` | language server, VS Code extension, tolerant analysis, quick fixes |
| `m10-m11.md` | macros (r63), reflection (r51), checked attributes (r52) |
| `m12.md` | the standard library, r39–r68, Breakout, CI fixes along the way |
| `tooling.md` | build modes, `dz test` (M14), `dz bench` (M15), CI, the parallel suite |

**Where things stand (2026-10-21):** M0–M8, M10, M11, M14 and M15 are done; M7 is done
apart from the rows left in the register; most of M9 was pulled forward; M12's criterion
(Breakout) is met and the stdlib is being filled in.
