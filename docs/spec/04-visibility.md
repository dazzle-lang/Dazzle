# 4. Visibility

Four levels. Default is private. `[DECIDED]`

| On | `private` (default) | `protected` | `pub(module)` | `pub` |
|---|---|---|---|---|
| Top-level declaration | visible within its module | not allowed | same as private | exported from the module |
| Struct field | visible only inside the struct's own methods | also inside the methods of the structs that inherit it | visible anywhere in the module | visible to importers |
| Method | visible only inside the struct's own methods | also inside the methods of the structs that inherit it | visible anywhere in the module | visible to importers |
| Enum / union variant | always as visible as its type | — | — | — |

**`protected` is a different axis from the other three**, which is why it is spelled on its
own rather than as a `pub(…)` form. `private`, `pub(module)` and `pub` widen by *where the
code is written* — this type, this module, anywhere. `protected` widens by *what the type
is*: a struct that inherits from this one may reach the member wherever it is written, and
a struct that does not may not, however close it sits. It exists because of inheritance
(§7.3) and means nothing without it, so it is refused on a top-level declaration, where
nothing can inherit.

Note the deliberate asymmetry: at top level, `private` and `pub(module)` mean the same
thing, because the module *is* the encapsulation boundary and there is no file-level
privacy. `pub(module)` earns its keep on **struct members**, where it expresses "the rest of
this engine may touch this field, but nobody who imports the engine may."

```dz
pub struct SpriteBatch {
    m_verts:                List<Vertex>     // private: only SpriteBatch's own methods
    pub(module) m_gl_handle: u32             // the rest of the engine may poke this
    pub         count:       u32             // anyone may read

    pub fn draw(&mut self, s: &Sprite) { ... }
    fn flush(&mut self) { ... }          // private helper
}
```

**Why no file-private.** It was considered and rejected: it adds a fourth level, forces
every helper to think about two boundaries instead of one, and its main benefit —
preventing accidental coupling inside a module — is better achieved by keeping modules
small.
