---
id: T-0031
title: User-written attribute macros
milestone: M10
status: open
kind: deferral
area: resolve
where: `dz_resolve`, attribute validation
---

**What is missing.** Attributes (§12.8) are the compiler's: `#[test]`, and the others the
language defines. An **unknown** attribute is an error, `DZ0314`, and stays one by D-0060 —
there is no way to declare one of your own.

```dz
#[derive(Serialize)]        // DZ0314: unknown attribute
struct Transform { … }
```

**Why `DZ0314` is right for now.** An unknown attribute silently ignored is a trap: a typo in
`#[tset]` would make a test never run, which is the kind of failure that costs an afternoon.
Refusing it means the set of attributes is knowable and a misspelling is caught at the
spelling. D-0060 settled that deliberately.

**Why it matters, and the thing that reduces the pressure.** `#[derive(…)]` is the shape
people reach for, and §13.4 already gives a mechanism that does the same job with a different
spelling: a **declaration macro** takes the struct's declaration and generates the impl from
its fields.

```dz
#serializable struct Transform {
    position: Vec3
    scale: f32 = 1
}
```

That is `#[derive(Serialize)]` in all but syntax — and arguably better, since it is a
declared macro with a pattern the compiler checks, rather than a name dispatched to code
that may or may not exist. So the *capability* is present; what is absent is the attribute
spelling for it.

**What it would take, and what to decide first.** The question is not how to implement it but
whether the attribute spelling earns its place:

- If a declaration macro already does the work, a user attribute is a **second way to write
  one thing**, which [T-0070](T-0070-one-word-one-job.md) argues against on exactly these
  grounds — and `#serializable` versus `#[serializable]` differing by brackets is the worst
  kind of near-duplicate.
- If it is wanted anyway, §13.4's per-field macro is the model: an attribute macro is a
  declaration macro whose pattern is the item it precedes, resolved by name like any other
  macro, so `DZ0314` would consult the macro namespace before reporting. Mechanically small.
- The one thing an attribute offers that `#name` does not is **several on one declaration**,
  and composition order — which is a real design question, since two macros rewriting the
  same struct have to agree on what they see.

So: revisit with §13.4's macro as the model, as the original note says, and settle whether
the spelling is worth having at all. It needs a decision file either way, since it changes
what a valid program looks like.
