//! Every mistake in the feature matrix, in every context and both layouts, must produce
//! exactly its one diagnostic (see `matrix/mod.rs`). Analysis only: the running half is
//! `dz/tests/matrix.rs`.

mod feature_matrix;

use feature_matrix::{place, write_project, CONTEXTS, LAYOUTS, MISTAKES};

#[test]
fn every_mistake_is_one_diagnostic_wherever_it_is_written() {
    let mut failures = Vec::new();
    let mut count = 0;
    for &layout in LAYOUTS {
        for (context_name, context) in CONTEXTS {
            for (mistake, body, code) in MISTAKES {
                count += 1;
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
                if got != [*code] {
                    let rendered = dz_diag::render_to_string(&analysis.map, &analysis.diags);
                    failures.push(format!(
                        "{name} ({layout:?}): expected [{code}], got {got:?}\n{rendered}"
                    ));
                }
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {count} placements failed:\n\n{}",
        failures.len(),
        failures.join("\n")
    );
}
