//! Every mistake in the feature matrix, in every context and both layouts, must produce
//! exactly its one diagnostic (see `matrix/mod.rs`). Analysis only: the running half is
//! `dz/tests/matrix.rs`.

mod feature_matrix;
mod parallel;

use feature_matrix::{place, write_project, Layout, CONTEXTS, LAYOUTS, MISTAKES};

#[test]
fn every_mistake_is_one_diagnostic_wherever_it_is_written() {
    // Every placement is its own project in its own directory, analysed by a pure function
    // (invariant 3), so they are run on every core rather than one after another.
    let placements: Vec<(Layout, &str, &str, &str, &str, &str)> = LAYOUTS
        .iter()
        .flat_map(|&layout| {
            CONTEXTS.iter().flat_map(move |(context_name, context)| {
                MISTAKES.iter().map(move |(mistake, body, code)| {
                    (layout, *context_name, *context, *mistake, *body, *code)
                })
            })
        })
        .collect();
    let count = placements.len();

    let mut failures = parallel::filter_map(
        &placements,
        |&(layout, context_name, context, mistake, body, code)| {
            let name = format!("{mistake}_in_{context_name}");
            let dir = std::env::temp_dir().join(format!("dz_matrix_{layout:?}_{name}"));
            let code_text = format!("{}\n\nfn main() {{\n}}\n", place(context, &name, body));
            let root = write_project(&dir, layout, &code_text);
            let analysis = dz_driver::analyse(&root);
            let got: Vec<&str> = analysis
                .diags
                .iter()
                .filter_map(|d| d.code)
                // Warnings about the scaffolding: each placement uses a few of the
                // imported names and binds values only to show a rule.
                .filter(|c| {
                    *c != dz_diag::codes::UNUSED_VARIABLE && *c != dz_diag::codes::UNUSED_IMPORT
                })
                .collect();
            let failed = (got != [code]).then(|| {
                let rendered = dz_diag::render_to_string(&analysis.map, &analysis.diags);
                format!("{name} ({layout:?}): expected [{code}], got {got:?}\n{rendered}")
            });
            let _ = std::fs::remove_dir_all(&dir);
            failed
        },
    );
    failures.sort();

    assert!(
        failures.is_empty(),
        "{} of {count} placements failed:\n\n{}",
        failures.len(),
        failures.join("\n")
    );
}
