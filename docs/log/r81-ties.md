# r81: ties, `kills` clauses and compiler-placed memory

The language change is spec §6.7 and §14.6, decided as D-0091; the plan, in order, is
T-0107. How it came about: the regions prototype (T-0104, T-0105, on the branch
`t-0104-regions-prototype`) tried to choose allocators without a checker, and each stage's
safety rested on rules a test had found. The user's own "ties" idea supplied the checker, and
D-0091 was decided on 2026-10-27.

## 2c, second part: following references through bodies (2026-10-28)

`dz_flow::ties::analyse_body` now walks a checked body and returns its kills, invalid uses,
wrong returned ties, caller-visible effect summary and clause disagreements. It remains
separate from `dz_flow::check`: this is analysis that can be exercised and reviewed before
std's clauses and the diagnostics/fixes turn enforcement on. No language rule changed.

- **Addresses and owned memory stay separate.** An origin remembers its logical target
  as well as the tie keeping its address valid. `&xs[i]` targets `xs[*]` but lives in
  `xs@items`. A view into an element's owned field follows the former, so it survives a
  reallocation of the array. Index/IndexMut calls, including their lowering to HIR method
  calls, preserve this distinction, as do safe pointer returns into indexed storage.
  Replacing an element ends both its slot's references and its owned memory.
- **Aliases and generations.** Local reference, pointer, slice and string views carry
  origins through bindings, copies and pointer retargeting. A kill marks the existing
  views; a fresh view starts live. Moving a reference ends its binding, not the referent.
  Scalar writes that move nothing keep mutable aliases legal. An input's `in` annotation
  tells the body which owner's kills can end that reference without claiming that writing
  the reference replaces the owner whole.
- **Effects.** Assignment, move, scope exit, destruction intrinsics and calls record
  kills at spans in the body. Calls read signatures, not bodies, with generic type
  substitution and concrete trait implementations where known. Receiver and argument
  values remain live while later arguments are evaluated. A call also checks independent
  argument/receiver aliases against what its clause kills. Indirect calls conservatively
  kill through mutable arguments and remain incomplete until function types keep clauses.
- **Control flow.** Branches union possible origins and kills, excluding paths that do
  not reach the join. Loops reach a fixed point, including `continue` edges and kills from
  earlier iterations; fresh iteration bindings are refreshed. Native array/slice loops
  retain an iteration view. `break`, `continue` and return apply exited scopes' drops and
  defers before recording the edge. Scope metadata and temporary argument/return views
  follow the same branch edges, so a returning branch cannot poison a surviving one.
  Provenance growth and replay are bounded for editor responsiveness; hitting a bound
  explicitly marks the analysis incomplete.
- **Returns and summaries.** A view cannot escape a local or a by-value parameter that
  is about to die, nor survive a deferred kill before return. Known returned origins must
  fit the declared/elided tie; elision through a helper retains the input's slot-address
  dependency. Only kills through mutable inputs become the caller-visible summary;
  local drops, constructor destinations and copy destinations do not. Complete summaries
  use the first part's missing/unused/broad-clause comparison.

**Still open in 2c:** aggregate view storage and destructuring (tuples, optionals and
structs containing views), union payload naming, general iterator provenance, mapping
private pointer indexing to the declaring container's `[*]`, default-argument effects and
function-type clauses. An unknown/unsupported case is recorded in `incomplete`; such a
body produces no clause-removal or narrowing suggestion. Raw pointer fields/casts retain
§6.7's unchecked boundary. New diagnostic codes, rendering and fixes are not enabled yet.

Tests: `dz_flow/tests/ties.rs` now has 27 tests, including valid/invalid body pairs for
calls, indexing and owned element views, branch joins, fresh references, moves and
replacement, scope exit, argument evaluation, returns, defers, breaks, continues and
native `for` loops. Other cases pin clause summaries, indirect/default/aggregate and
provenance-growth boundaries, and run every fixture/std HIR body without panicking.

## 2c, first part: the call-effect model (2026-10-28)

`dz_flow::ties` now reads what a signature can invalidate at a call. This is the shared
model for the body analysis, not enforcement: `dz_flow::check` still reports the same
diagnostics, and 2c remains open. No language rule changed.

- **One representation for each place.** A path is rooted in a caller's `LocalId`, with
  fields, `[*]`, and the storage behind a pointer field as distinct steps. A public tie
  on a non-pointer normalizes to its private field; one on a pointer normalizes to that
  field's storage. Thus `w@bag@items` and `w.bag@items` compare equal, while
  `xs@items` and `xs[*]@text` remain separate. Element types come from
  `dz_check::ties::elements`, shared with the resolver, including generic `Index` and
  Iterable-only containers.
- **Signatures at arguments.** `call_effects` takes the origins of the actual receiver and
  arguments, translates every kill, returned tie and parameter's `in`, and retains each
  kill's clause span for later diagnostics. `Path::bind` also permits normalization once
  and substitution at multiple calls. It reads no callee body. Parameter contracts do not
  replace independently known argument origins.
- **Returned views.** An explicit result keeps its named path; elision with exactly one
  reference input (including `self`) keeps that input whole. A kill beneath a whole-input
  tie ends the view, while a kill of list storage leaves an element's separately owned
  text alone. Ambiguous inputs and origins not known to the caller stay unknown; this
  makes no new decision about elision with multiple inputs.
- **Unknown effects stay unknown.** Imported functions kill through `&mut` and `^mut`
  arguments, and a mutable imported receiver. An argument whose origin is unknown sets
  `incomplete`, rather than silently becoming a function that kills nothing. Unknown
  return or parameter ties are represented separately from unknown kills.
- **Exact clauses.** `minimal` removes repeated and subsumed effects without merging
  siblings. `compare` distinguishes a missing kill, an unused declaration and a broad
  declaration, keeping indices into the original paths for labels and a replacement
  clause. A too-narrow declaration over a broader actual kill produces only the missing
  kill, rather than also claiming the declared memory never dies. This comparison is
  for complete body summaries, not trait/extern/function-type promises.

Tests: nine `dz_flow/tests/ties.rs` cases use real parsed and checked signatures, with
valid/invalid comparison pairs, public aliases, generic `Index`, arrays, slices,
Iterable-only containers, receiver and parameter substitution, returned ties and elision,
foreign effects, unknown origins, and malformed incremental signatures. The checker and
existing ownership suites pass too.

The body walk follows in the second part above. Diagnostics and fixes become enforced
with 2d/2e's std and corpus updates.

## 2b: resolving tie paths (2026-10-28)

Every tie path in a signature now has to name something, and something every caller can
see. Nothing yet checks a body against its clause; that is 2c.

- **Where the clause lives.** A signature is collected before every type's fields are, so
  `FnSig` keeps `kills` and `ret_tie`, and `ParamSig` its `tie`, as written. The new module
  `dz_check::ties` resolves one into a `Tie` — a root (`self` or a parameter's index) and
  steps (field, `[*]`, tie) — and `ties::check` lists what is wrong with a signature.
  `dz_flow` will call `resolve` for the paths themselves. This kept the change out of the
  dozen places a signature is built: the checker runs it once per function, from
  `Body::function`, and for a trait's required methods, which have no body.
- **`FieldSig::tie`** carries a field's `in @name`, and **`FnSig::kills_unknown`** marks an
  imported C or C++ function, whose header says nothing of what it kills.
- **What a path may name.** A root is a parameter, or `self` in a method. `.field` goes
  through a struct's fields; `@name` through a tie a field declares, and stops there when the
  field is a pointer, since the tie then names that memory whole. `[*]` is settled: the
  elements of an array, a slice, or an `Index<usize>` type, whose `index` gives the element
  type; an `Iterable`-only type such as `HashMap` is named whole at `[*]`.
- **Visibility** (§4): a field in a clause must be reachable from the function and at least
  as visible as it, ranked private < `protected` < `pub(module)` < `pub`. A private free
  function counts as `pub(module)`, since the whole module may call it. A tie is a public
  name, which is how a `pub` method names private storage.
- **Codes**: `DZ0562` a path that names nothing, `DZ0563` a field some caller cannot see,
  `DZ0564` a kill through `&`, `&self` or a by-value parameter, `DZ0565` an `in` on what
  points into nothing (or into itself), `DZ0566` a tie declared twice. Each has its `dz
  explain` entry, wrong and right.
- **Left for later**: a function type's clause is parsed but not resolved, nor part of
  `Ty::Fn`. `List` declares no `@items` until 2d, so `kills xs@items` on a list is `DZ0562`
  today, and the bare `kills xs` stands in for it.

Tests: `typed/` gains every valid form and `typeerrors/` a case for each new code (invariant
8), and two targeted checker tests pin generic types, implementations, `protected` through
inheritance, and `pub(module)` against the function's own visibility.

## 2a: the syntax (2026-10-27)

Parsed and kept, with no meaning yet: nothing past the parser reads a clause.

- **`kills` is a keyword** (`dz_lex`), and `@`, already a token nothing used, starts a tie.
- **The tree** (`dz_ast`): a `TiePath` is a root (a parameter or `self`) and steps — `.field`,
  `[*]`, `@tie`. `FnDecl` gained `ret_tie` (`-> ^T in self@items`) and `kills`; `Param` and
  `Field` gained `tie`; a function type gained optional parameter names and its own `kills`.
  The dump prints them only when present, so no earlier snapshot changed.
- **The parser** (`dz_parse`): a clause sits after the return type, a reference's `in`
  after its type and before a default, a field's `in @name` after its type. A path that
  breaks off is one `DZ0218` and is kept as far as it got: `xs[1]` drops the index and keeps
  `[*]`, and `in items` reads as `in @items`, so the default, the body and everything after
  still parse (invariants 1 and 6).
- **One grammar point settled here:** in a function type, a comma after a bare path begins
  the next parameter (`fn each(f: fn(xs: &mut List<i32>) kills xs@items, n: i32)`), so a
  list of paths there is parenthesised. A declaration's list never needs to be, and a
  parenthesised one is accepted anywhere; the formatter writes the parentheses only where
  they are needed.
- **The formatter** (`dz_fmt`) writes all of it through the dump's own spellings, so a clause
  survives exactly; `tests/fmt/ties.dz` shows the layout it settles on.
- **Hygiene** (`dz_macro`): a tie path's root is renamed with the parameter it names, through
  a new `tie_root` hook, so a clause a macro writes still names its own parameter. A function
  type's clause is not walked: its parameter names are not binders.

- **The editor**: `kills` is coloured as a keyword by the VS Code grammar and offered by
  completion (`dz_lsp`'s `KEYWORDS`), the two places a new keyword was once missed.
- **Unrelated, found on the way:** `dz_resolve`'s workspace snapshots counted std's files
  before the prototype branch added three (`scope_arena`, `scope_pool`, `scope_shared`);
  only those counts changed.

Tests: `tests/parse/ties.dz` (every form), four mistakes in `tests/parse/errors.dz`,
`tests/fmt/ties.dz`, `kills` in `tests/lex/keywords.dz`, and two targeted parser tests (the
comma rule, and `kills` no longer being a name). `dz explain DZ0218` has its wrong and right
programs.
