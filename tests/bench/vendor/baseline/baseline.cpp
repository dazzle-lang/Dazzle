// The C++ baseline. See baseline.h for why this exists.
#include "baseline.h"

namespace {

/// The same optimiser barrier the Dazzle side uses through `blackbox`.
///
/// Without it these loops are not the loops they look like: every one here is a pure
/// function of its argument, so the compiler folds it to a closed form and the benchmark
/// reports a few nanoseconds for work it never did. The Dazzle half is held back by
/// `blackbox`, so the C++ half has to be held back identically or the pair measures the
/// difference between running and not running.
template <class T>
inline T barrier(T v) {
#if defined(__GNUC__) || defined(__clang__)
    asm volatile("" : "+m"(v) : : "memory");
#else
    volatile T sink = v;
    v = sink;
#endif
    return v;
}


/// A resource holder with a destructor — what Dazzle's `impl Drop` lowers to.
struct Holder {
    int32_t* slot;
    int32_t value;

    Holder(int32_t* s, int32_t v) : slot(s), value(v) {}
    ~Holder() { *slot += value; }

    Holder(const Holder&) = delete;
    Holder& operator=(const Holder&) = delete;
};

/// A base with a virtual method, and one override — the shape of `virtual`/`override`.
struct Shape {
    int32_t n;
    explicit Shape(int32_t v) : n(v) {}
    virtual ~Shape() = default;
    virtual int32_t area() const { return n; }
};

struct Square : Shape {
    explicit Square(int32_t v) : Shape(v) {}
    int32_t area() const override { return n * n; }
};

}  // namespace

extern "C" int32_t baseline_sum_indexed(const int32_t* xs, int32_t n) {
    int32_t total = 0;
    for (int32_t i = 0; i < n; ++i) {
        total += xs[i];
    }
    return total;
}

extern "C" int32_t baseline_raii(int32_t count) {
    int32_t total = 0;
    for (int32_t i = 0; i < count; ++i) {
        Holder h(&total, i);
        // Matches the Dazzle loop's `blackbox(h.value)`. Without it this whole loop is
        // `total += i` summed, which the compiler replaces with a closed form.
        (void)barrier(h.value);
    }
    return total;
}

extern "C" int32_t baseline_virtual(int32_t count) {
    Square sq(3);
    Shape* s = &sq;
    int32_t total = 0;
    for (int32_t i = 0; i < count; ++i) {
        total += s->area();
    }
    return total;
}
