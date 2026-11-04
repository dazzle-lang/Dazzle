---
id: T-0166
title: A static's field defaults run code before main, which can read a static C++ has not built
milestone: none
status: done
kind: bug
area: ownership
---

Found by the audit of the generated C++ ([T-0133](T-0133-audit-generated-c-for-undefined-behaviour.md)),
2026-11-04. A hole in [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md)'s
promise. **Done the same day as option B, the user's choice** (r116).

**Where the code is**

| Piece | Where |
|---|---|
| DZ0553, a static's initialiser built from constants | `is_static_initialiser`, called from the `ItemKind::Static` arm of `check_item` in `crates/dz_check/src/check.rs` |
| Which statics take code to build | `static_runs_code`, `runs_code`, `default_runs_code`, `value_init_runs_code` in `crates/dz_cpp/src/lib.rs` |
| How a static is emitted (`StaticCell`, `BuiltStatic`) | `static_decl` in `crates/dz_cpp/src/lib.rs`; `StaticCell`, `BuiltStatic`, `drop_static` in `crates/dz_cpp/src/runtime.rs` |
| Building them from `main` | each module's `dz_build_statics__` (`drop_statics_fn`), the root's `build_all_statics`, called by the generated `main` and `harness_entry` |
| Tests | `tests/run/static_order`, `tests/run/static_cycle` |
| The rule | spec §7.7 |

**Done** (2026-11-04). A static whose value runs code, through a field default that is not
made of constants, is a `BuiltStatic`: storage C++ sets up with no code, and a function that
returns the value. The generated `main` (and the test and bench harness) first calls the
root's `dz_build_all_statics__`, which builds every module's in dependency order, `std`
first, each module's in order of declaration. Reading one goes through `.get()`, which
builds it if its turn has not come, and panics naming it if it is being built. A static of
constants alone is emitted as before, so std's allocators and every plain static cost
nothing more to read. The program below now prints what it should on clang++ and g++,
clean under the sanitizers; `tests/run/static_order` writes to `SECOND` from `FIRST`'s
default and reads it back, and `tests/run/static_cycle` panics.

DZ0553 still refuses an initialiser that is not made of constants
(`static S: List<i32> = List<i32>()`), though building statics from `main` would now allow
it; whether to lift that is a separate choice.

**What is wrong.** §7.7 says a static is built from constants, because "C++ builds statics in
different files in no promised order, so one static cannot be allowed to run code that reads
another". DZ0553 checks the initialiser as written, but not the defaults of the fields a
struct literal leaves out. A default may call a constructor, and a constructor may read a
static. This safe program is accepted:

```dz
struct Bag {
    pub items: List<i32> = List<i32>()
}

struct Probe {
    pub n: usize

    Probe() {
        n = SECOND.items.len()
    }
}

struct Holder {
    pub p: Probe = Probe()
}

static FIRST: Holder = Holder{}
static SECOND: Bag = Bag{}

fn main() {
    println!("FIRST saw {} item(s); SECOND holds {}", FIRST.p.n, SECOND.items.len())
}
```

C++ builds `FIRST` before `main`, and so runs `Probe()`, which reads `SECOND` before C++ has
built it: an object read before its lifetime begins, which C++ leaves undefined. In one file
the order is declaration order, so this reads zeroed storage and prints 0; across modules
the order is not promised at all. With a write in place of the read (here inside `unsafe`,
since a constructor cannot declare a `kills` clause yet,
[T-0122](T-0122-constructor-cannot-declare-kills-clause.md)), the program panics before
`main` with "a List with no allocator cannot grow".

**Why it matters.** It is code running before `main` against memory nothing has built, from a
program with no `unsafe`. std's own statics are safe: every field default of `Heap`,
`TempArena`, `ScopePool`, `RandomState` and `Tracker` is a literal.

**The choice** (B was chosen).

- **A. DZ0553 looks through the defaults.** A struct literal in a static is checked with the
  defaults of the fields it leaves out, as if written, so `Holder{}` is refused here. This is
  what §7.7 already says. It also refuses `pub items: List<i32> = List<i32>()` inside a
  static, which the site's safety page uses (`tour/safety.mdx`, `static BOARD: Board =
  Board{}`), so a static that holds a container would need a constant way to spell an empty
  one, or to be filled in from `main`.
- **B. Statics are built from `main`, in a fixed order**, as T-0163 made their drops: every
  static starts zeroed, and the generated `main` builds them before the program's body, a
  module's statics after those of the modules it uses, and in declaration order within one.
  A default may then run any code, and a static read before it is built is still possible
  within one module, where a later static is read by an earlier one. That case would need
  a check of its own, or reading it to see the default.

A keeps the rule as written, and B makes the rule's reason go away.
