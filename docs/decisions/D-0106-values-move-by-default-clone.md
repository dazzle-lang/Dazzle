---
id: D-0106
title: "Values move by default; `clone` replaces `copy`, and `move` is gone"
status: decided
---

**A by-value use of a value that owns something moves it, with nothing written. Plain data
copies, as Rust's `Copy` types do. Every other copy is written `a.clone()`. `move(x)` is
removed from the language. Chosen by the user on 2026-11-15, in T-0235 ("I don't think we
should keep move() because that would be ambiguous whether the language has inference or
not. I think the language will always know"; "we would no longer have copy, would have a
clone instead. I know we will have to migrate everything but i think would be better for the
language"). Revises §6.1, §6.3, §6.4 and §6.5, and supersedes D-0033. Not yet built:
T-0235.**

What was decided, question by question:

- **Moves by default, not only at a last use.** `let b = a`, passing `a` by value, pushing
  it, or putting it in a literal moves `a` when its type is not plain, whether or not `a`
  is read again; a later read is `DZ0401`, use after move, whose fix writes `.clone()`.
  Chosen over moving only at a local's last use, under which a use that is not the last
  still copies, allocating where nothing says so, and a read added further down silently
  turns a move above it into a copy.
- **`move` is removed**, not kept beside the default, so there is one way a value moves.
- **Plain data copies without being written**: a type with no `drop`, no `own` field and no
  `clone` of its own, whose fields are all plain (`i32`, `f32`, `bool`, `Vec3`, an enum, a
  `&str`, a `^T`). A copy and a move of plain data are the same bytes, so a program cannot
  tell them apart. The user's words were "make it like Rust"; Rust asks for `Copy` to be
  derived, where here, as today, plain data is plain by its shape (§6.3). That reading is
  Claude's; the user may still want an opt-in marker.
- **`clone` replaces `copy`, the special member.** A type that owns something is cloneable
  when it has an `impl Clone`, written as `copy` is today, filling a value at its defaults
  from `other`, so the compiler still builds a clone where it places it (§14.6):

  ```dz
  impl Clone for Buffer {
      fn clone(&mut self, other: &Self) {
          self.push_all(other.as_slice())
      }
  }
  ```

  A clone is asked for at the call site as `a.clone()`, a method, so it chains:
  `names[i].clone()`. Every plain type is `Clone` too; `T: Clone` is the bound a generic
  body names to clone a `T`. The three special members are a constructor, `clone` and
  `drop`.
- **A moved value is dropped where its new owner is**, at the end of the function it was
  passed to or of the scope that holds the new binding; the source, moved-from, drops at the
  end of its scope and does nothing (§6.5).

Proposed with the decision, for the implementation to confirm (T-0235):

- **A field of something the function does not own outright is not moved out by a read**:
  `let n = self.name` is `DZ0402`, as today, and `take(self.name)`, a `std` function,
  returns the value and leaves the field at its defaults. A field of a local is moved out
  of as the local is, and tracked per field as partial moves are today.
- **A value is ended early with `drop(a)`**, a `std` function, where `let _ = move(a)` did.
- **`move` stays a reserved word for a while**, so `move(x)` in old code is an error
  saying it is gone, with a fix that removes it, rather than an unknown name.
- A generic body moves a `T` always, since `T` may not be plain, and writes `v.clone()`
  under `T: Clone`: one rule for every instance, which is what D-0033 wanted.
