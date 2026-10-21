# 0. How to read this document

This is a *design* document, not a description of a working compiler. It is meant to be
argued with. Three markers appear throughout:

- **`[DECIDED]`** — settled. Implementation may proceed against it.
- **`[PROPOSED]`** — my recommendation, written as if decided so you can judge it in
  context. Reject freely; nothing depends on it yet.
- **`[OPEN]`** — a genuine fork that needs your call. All of them are collected in §18.

Every code sample here is intended to be a valid Dazzle program (or a deliberately invalid
one, marked as such). They double as the compiler's first test corpus.

## 0.1 The whole language on one page

If you read nothing else, read this. Everything after it is detail.

```dz
// no imports: dazzle.toml names what every file can use — `std`, `engine`, `rl` (§3.2)

pub struct Buffer<T> {                // private by default; pub / pub(module) / (nothing)
    m_allocator: ^Allocator = null    // ^T  nullable pointer
    m_data:      own ^mut T           // own ^mut T  this struct frees it -> Buffer is move-only
    m_len:       u32                  // fields default-initialise; no uninitialised memory

    Buffer(cap: u32, a: ^Allocator = Allocators::Global) { ... }   // constructor

    pub fn length(&self)      -> u32 { return m_len }       // &self     = reads
    pub fn resize(&mut self, n: u32) { ... }                // &mut self = mutates
}

impl<T> Drop for Buffer<T> {                    // destructor -> deterministic, C++ handles it
    fn drop(&mut self) { ... }
}

impl<T: Copy> Copy for Buffer<T> {              // copy constructor -> copyable when T is
    fn copy(&mut self, other: &Self) { ... }
}
```

| Concept | Syntax | One-line rule |
|---|---|---|
| Refer to a value | `&T` `&mut T` `^T` `^mut T` `own ^mut T` | `&`/`&mut` are non-null and **stack-only**; `^` is a C pointer, read-only unless `^mut`; `own ^` means you free it; `^T` → `&T` needs an `or` clause, `&T` → `^T` is free, and reading or writing *through* a pointer is `unsafe` |
| Copy vs move | `let b = a` / `let b = move(a)` | if the type owns anything, copying needs an `impl Copy`; otherwise `move()` |
| Absence | `T?` | postfix, binds to the type name. `^T?` is a pointer to an optional; an optional pointer is just `^T` |
| Failure | `fn f(…) !-> T` | `!` marks the *signature*, never a type. There is no `T!` to store, annotate, or point at |
| Handle anything fallible | `let v = f() or { e -> … }` | one clause for `T!`, `T?`, and `^T` alike |
| Propagate | `f() or return` | only where the enclosing signature can carry the failure |
| Fall back | `f() or Textures::Missing` | "on failure use this instead" — the common case in game code |
| Sum type | `union S { A{…} B{} C }` + `switch` | tagged, exhaustive, no fallthrough; bare `C` carries `struct C` |
| Interface | `trait X` + `impl X for Y` | bound `<T: X>` = static dispatch; `^X` = fat pointer, object stays the same size; an `impl` lives with the trait or with the type, so a module can add a trait to a type it did not write |
| Generics | `fn f<T: Ord>(…)` | checked once at the definition, then monomorphized by Dazzle |
| Module | one `dazzle.toml` = one module | folders mean nothing; `name = "engine::render"` for nesting |
| Metaprogramming | `macro m -> decl { … }`, invoked `#m …` | declarative pattern → expand, so the LSP can complete inside it |
| Reflection | `typeinfo_of<T>()` | one static table; your structs carry zero extra bytes |
| Compiles to | C++17, `-fno-exceptions -fno-rtti` | with `#line` back to `.dz`, so debuggers and profilers show your source |

**The five rules that shape everything else:** private by default · no implicit numeric
conversions · every value is initialised · moved-from values reset to defaults · `&`
references can never be stored in a field (this is why there are no lifetimes).
