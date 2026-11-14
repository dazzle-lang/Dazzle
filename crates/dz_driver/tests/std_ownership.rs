//! `std` passes every ownership check, in each build it can be part of.
//!
//! An analysis checks ownership for the program's bodies and never for `std`'s, since `std`
//! is the same in every program and nothing a program declares can change what its bodies
//! say (T-0025). Its `#if TRACK_MEMORY` branches are the one thing that differs from build
//! to build, so this checks `std` as each kind of build sees it, through the real pipeline.

use std::path::PathBuf;

use dz_driver::{analyse_for, BuildOptions, Mode};

#[test]
fn std_is_ownership_clean_with_each_branch() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/hello");
    for track_memory in [false, true] {
        let options = BuildOptions { track_memory, ..Default::default() };
        let a = analyse_for(&root, &|_| None, Mode::Build, &options);
        assert!(
            !a.diags.has_errors(),
            "hello should analyse cleanly:\n{}",
            dz_diag::render_to_string(&a.map, &a.diags)
        );
        let (Some(hir), Some(table), Some(res)) = (&a.hir, &a.table, &a.resolution) else {
            panic!("hello should type-check");
        };
        let diags = dz_flow::check_std(hir, table, res, &a.map);
        assert!(
            diags.is_empty(),
            "std fails its own ownership checks with track_memory = {track_memory}:\n{}",
            dz_diag::render_to_string(&a.map, &diags)
        );
    }
}
