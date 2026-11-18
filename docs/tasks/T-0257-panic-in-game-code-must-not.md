---
id: T-0257
title: A panic in game code must not take down an editor that runs the game in-process
milestone: M13
status: open
kind: design
area: tooling
where: `crates/dz_cpp/src/runtime.rs` (the runtime's panic); `crates/dz/src/build.rs` (`-fno-exceptions`, ~464); spec §9.8 (panics abort), §16.5 (no exceptions), §17.6 (hot reload)
---

Raised by the user on 2026-11-18: a game engine written in Dazzle loads the game as a DLL,
and a panic aborts (§9.8), so every bug in game code would take the engine with it. Set
aside for later, with hot reload, since both are about keeping the game running while it is
edited.

**The user's preference: Unity's model**, where the game runs and draws inside the editor's
own process, not Godot's separate game process. So process isolation alone does not answer
it; a panic has to be stopped inside the process.

**What was discussed, 2026-11-18:**

1. **A panic handler** in `std`: code the engine registers that runs before the abort
   (flush the log, autosave, report). M17 already plans one for targets with no stdio.
   Needed under any answer; does not by itself keep the editor alive.
2. **Panics that unwind, in development builds**, as Rust's `panic = "unwind"`: a panic
   throws a C++ exception, caught by the engine around each system or frame. Dazzle's `drop`
   is the C++ destructor, so unwinding runs every drop with no new code generation; and safe
   code has no undefined behaviour (§1.1), so after a panic memory is intact and only the
   game's own data may be half-updated, which the engine handles by dropping the frame or
   restoring a snapshot (the type table, §13, already serialises any type). Shipping builds
   keep `-fno-exceptions` and abort.

   Costs to decide against: development builds with exceptions on (size, build time); an
   unwind must be caught before it crosses a C frame (a raylib callback), or the program
   terminates anyway; and a worker's panic, which D-0096 has abort, would then come back out
   of `join!`.
3. **What no in-process answer covers**: a segfault in a C library, a stack overflow, a
   driver fault. Unity has the same limit.
