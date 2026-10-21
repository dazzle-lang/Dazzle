---
id: D-0067
title: Random numbers
status: decided
---

**One generator per thread, reached as `Random::…` with no instance**, chosen by the user over a value-type generator: game code wants a number, not a generator to pass around. SplitMix64 (one `u64` of state) rather than PCG or xoshiro, for its size and simplicity; seeded from the clocks unless `Random::seed` is called. `Random::range<T: Number>` covers every number kind through the new `Number` trait, which `Integer` and `Float` both require. Integer ranges are `[lo, hi)`, as floats are. A separate seedable stream — a replay that must not share state with gameplay — can come later as a value type (§14.4)
