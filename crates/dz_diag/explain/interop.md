# C and C++ interop, DZ07xx

A C or C++ library is vendored as source and described by a recipe in `dazzle.toml`; its
headers are read by libclang into a module of their own (spec §15). These mistakes need a
library, so no single-file program shows them.

## DZ0701

The library's own C or C++ did not compile. It is neither Dazzle's fault nor the Dazzle
code's: the recipe usually lacks a define or an include directory the library needs.

No example: it takes a vendored C or C++ library.

## DZ0702

A header listed in a recipe's `import` could not be read: not found through its include
directories, or with errors of its own that clang reports.

No example: it takes a vendored C or C++ library.

## DZ0703

`import` in a recipe reads headers with libclang, loaded when it is needed; on a machine
without it, nothing can be imported. Install LLVM, or set `LIBCLANG_PATH`.

No example: it depends on the machine, not the program.

## DZ0704

A header that only declares a type — `struct Window;` — gives no size, so a value of it
cannot be made, copied or stored; only a pointer to one can be held (spec §15.2).

No example: it takes a vendored C header.

## DZ0705

A C `union`'s members share one piece of memory, so a literal setting two of them would
keep only the last; set one (spec §15.2).

No example: it takes a vendored C header.

## DZ0706

A bit-field is a few bits inside a larger member, and C++ has no pointer or reference to one.
Copy it into a local, change that, and assign it back (spec §15.2).

No example: it takes a vendored C header.

## DZ0707

C++ cannot copy or assign a raw array, so an imported struct's array member is indexed,
iterated or pointed at rather than taken whole (spec §15.2).

No example: it takes a vendored C header.
