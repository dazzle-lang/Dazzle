---
id: T-0043
title: A `HashMap<String, V>` is looked up with a `&String`
milestone: M12
status: open
kind: deferral
area: stdlib
where: `dz_project/src/std/collections/hash_map.dz`
---

**What goes wrong.** `HashMap<K, V>`'s lookup methods take a `&K`. So for a
`HashMap<String, V>` — the map every game has, from an asset name to the asset — looking up
a key you hold as a `str` means **building a `String` first**, allocating and copying bytes
only to hash them and throw them away:

```dz
let mut textures = HashMap<String, Texture>()
textures.insert(String::from("hero"), tex)

let name: str = line.slice(0, 4) or return      // a str, from wherever
let t = textures.get(&String::from(name))       // allocates, hashes, frees
```

**Why it is this way.** Nothing is wrong with the hashing: `String` hashes as its text
(§14.2), so the `String` built from `name` hashes identically to `name` itself. The problem
is purely that the signature demands the owned type. Rust answers this with `Borrow<Q>` —
`get<Q>(&self, k: &Q) where K: Borrow<Q>` — and Dazzle has no such trait.

**Why a game needs it** (from the readiness review, 2026-09-14). Asset lookup by name is in
the inner loop of loading, and often in the frame loop too (a sprite naming its texture, a
sound cue naming its clip). An allocation per lookup is the kind of cost that turns up as a
hitch during a level transition and is invisible in the source.

**What it would take, and the obstacle.** The clean answer is a `Borrow` trait and generic
lookup methods — but a generic method on a struct is fine, while the trait-bound form wants
associated types or at least careful scoring, and it adds a trait whose whole job is to say
two types hash alike, which is a promise nothing checks. The cheap answer is a second set of
methods on the `HashMap<String, V>` instance specifically — `get_str`, `contains_str`,
`remove_str` — which needs no language feature at all but means a specialised `impl` for one
instantiation of a generic struct, so check that is expressible first. A third answer is to
make `Hash` and `Eq` on `String` and `str` explicitly cross-comparable and give the lookup
methods a `str` overload, which D-0005 rules out while overloading is out.

The cheap answer is probably the right one for M12: three methods on `std`'s own
`HashMap<String, V>`, documented as the way to look up by `str`, with a `tests/run` case
proving no allocation happens (the `--track-memory` tracker can count).
