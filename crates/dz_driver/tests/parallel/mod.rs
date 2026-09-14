//! Running one test's own work on every core.
//!
//! The cargo harness parallelises *across* test functions, never inside one, so a test
//! that is a loop over thousands of cases pins exactly one core however many the machine
//! has — and this suite's long poles are all that shape: `tolerant.rs` analyses every
//! program in `tests/run/` at forty damaged positions, `dz_driver`'s `matrix.rs` runs every
//! mistake in every context in both layouts, and `dz`'s `run.rs` builds and runs every
//! project under `tests/run/` through a real C++ compiler.
//!
//! Splitting them is sound because every phase is a pure function of its input
//! (invariant 3): no case can see another's work, and the process-wide caches the pipeline
//! does keep — the parsed `std` trees, the `import cpp` header cache — are behind a
//! `OnceLock` and a `Mutex` already.
//!
//! Used from other crates' tests through `#[path]`, as `feature_matrix` is.

// Not every test target that includes this module needs both shapes.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Apply `f` to every item on as many threads as there are cores, keeping what it returns.
///
/// **The results come back in an unspecified order.** A caller that reports them to a human
/// should sort first, so the same failure reads the same way twice.
pub fn filter_map<T, R, F>(items: &[T], f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> Option<R> + Sync,
{
    let limit = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    // A shared cursor rather than a slice per thread: the cases are not equally expensive
    // — a damaged `raylib` costs many times a damaged `hello` — so handing out fixed
    // chunks would leave most threads waiting on one.
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<R>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..limit.min(items.len().max(1)) {
            s.spawn(|| {
                while let Some(item) = items.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if let Some(r) = f(item) {
                        out.lock().unwrap().push(r);
                    }
                }
            });
        }
    });
    out.into_inner().unwrap()
}

/// [`filter_map`] for work that only has to happen, not report: any panic in `f` — which is
/// what a failed `assert!` is — still fails the test, because a scope's threads are joined
/// before it returns and a panicked one propagates.
pub fn for_each<T, F>(items: &[T], f: F)
where
    T: Sync,
    F: Fn(&T) + Sync,
{
    filter_map(items, |item| {
        f(item);
        None::<()>
    });
}
