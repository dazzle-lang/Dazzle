---
id: D-0062
title: Shared ownership, and handles that may be empty
status: decided
---

**`Rc<T>`, `Weak<T>` and `Box<T>` in `std`, each of which may be `null`**, like C++'s `shared_ptr`/`weak_ptr`/`unique_ptr` rather than Rust's never-empty `Rc`: `null` is the default and the moved-from state, so no rule for "a type without a default" is needed, and a field of one is `= null`. **`null`, not `none`**: `null` is for handles — pointers included — and `none` for a value that is absent; the two stay apart so `x = null` reads as releasing something. A handle's value is reached only through `==`/`!=` `null`, `or` and `if let` — never a bare `.` — so an empty handle is always checked. **`or` and `if let` on an `Rc` or `Weak` hold a strong `Rc` of their own** until the `if let` block, or the block enclosing the `or`, ends, so the value cannot be freed while referred to — the cost being one plain count up and down. `if let` takes a `Weak` directly, with no `upgrade()`. Forced unloading is one owner plus `Weak`s, since a value others own cannot be deleted from under them; `Rc<T?>` is a shared slot, not a nullable `Rc`. The compiler knows the prelude trait `Nullable` (`Target`, `Guard`, `is_null`, `guard`, `ptr`) rather than the three names. A `Box` guards with nothing and can dangle as a `^T` can (§1.1). Deferred: `Arc`, `Rc<Derived>` as `Rc<Base>`, handles to trait objects (§14.5)
