---
id: D-0117
title: "spawn(f, value): a thread is started by a function and one moved value"
status: open
---

**Where it stands.** The user asked for the next threading task on 2026-11-24. D-0096 and
§22.3 decide `spawn!(f(a, b))`, which starts a thread and gives back a handle that joins it,
and decide nothing about the function underneath. What is built (§22.11, r150) is that
function, the smallest shape that fits a language with no closures (T-0259). **It is
proposed, and the user has not yet chosen it.**

| | |
|---|---|
| **Starting one** | `spawn(entry, value)` runs `entry(value)` on a new thread and returns a `Thread`. `spawn<A: Send>(entry: fn(A), value: A)`: the value is `Send` and `entry` a plain `fn`, so it is not `#[main_thread]` (below) |
| **What crosses** | One value, moved to the thread, into a heap allocation the thread frees as it starts. To give a thread more, pass a struct or a tuple |
| **Waiting** | `t.join()` returns when the thread has finished, with everything it wrote visible to the caller. A second `join` does nothing |
| **Letting go** | Dropping a `Thread` that was not joined detaches it: the thread runs on, and ends with the program |
| **Results** | None. A thread returns nothing, and shares through an atomic, a `Mutex<T>` or, when it is built, a channel (§22.1) |
| **Failing to start** | Not reported, because the system refusing a thread means the program is out of resources; it is the one failure `std` makes no `Result` for yet |

**Why one value and not a call.** `spawn!(f(a, b))` has to make a function that takes `a` and
`b` and a place to keep them, and a macro in expression position cannot declare either, nor
can a generic count its arguments (there are no variadic type parameters). A struct is how
D-0096 piece 2 already says a body that needs locals is written. `spawn!` is to be the macro
that writes that struct, and wants a decision of its own.

**Why detach and not join on drop.** A handle that joins when dropped makes an early return
wait for a thread it may have nothing to do with, and a thread that never ends would hang the
program. A thread outliving what started it is safe, since it holds only what it was moved
(§22.6); it is the data race that §22.2 refuses, not the lifetime.

**Why it is safe.** The user asked on 2026-11-24 that the safety held before threads be
kept, and that spawning a thread not be `unsafe`. So `spawn` landed with the checks of §22.2
and §22.5 (T-0264): `Send`, which refuses a value holding an `Rc`, a view or a raw pointer;
`#[main_thread]`, held to every body, which keeps a function that touches a plain static or
calls C that needs the main thread from being a plain `fn`; and placement, which builds what
a thread is given on the heap (§22.6). How the compiler knows what runs on another thread was
the user's question: it does not trace it, but checks the one place a thread starts, since a
function may reach only its arguments, statics and C, and the mark carries the last two up
through every call as a `kills` clause does.

**Open.**

- Whether this API is the one wanted, and whether a thread should return a value `join` gives
  back.
- The spelling of `spawn!` (T-0011).
- Naming a thread, its stack size, and `Thread::current`.
