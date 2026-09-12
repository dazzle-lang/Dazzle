// Hand-written C++ doing exactly what the Dazzle benchmarks beside it do (spec §19 M15).
//
// This is the baseline the language is measured against. It is compiled by `dz` from the
// same recipe, at the same optimisation level, into the same binary, and timed by the same
// clock — so a difference between a pair is a difference between the two languages and not
// between two ways of running a benchmark.
//
// Written the way a C++ programmer would write it, not the way the generated code happens
// to look: an unchecked `[]`, a plain destructor, a plain virtual call. That is the point —
// the comparison is against idiomatic C++, not against Dazzle's own output.
#ifndef DZ_BASELINE_H
#define DZ_BASELINE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Sum an array with C's unchecked indexing. Dazzle's `xs[i]` is bounds-checked in every
/// build (spec §5.3), and this is what that check is measured against.
int32_t baseline_sum_indexed(const int32_t* xs, int32_t n);

/// Acquire and release a resource through a destructor, `count` times. Dazzle's `drop`
/// lowers to the same C++ destructor (§6.6); this is the hand-written equivalent.
int32_t baseline_raii(int32_t count);

/// Call a virtual method through a base pointer, `count` times. The same shape as the
/// Dazzle `virtual`/`override` pair (§7.3, 18.32).
int32_t baseline_virtual(int32_t count);

#ifdef __cplusplus
}
#endif

#endif  // DZ_BASELINE_H
