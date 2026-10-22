//! `dz explain`'s examples (T-0062): every code has an explanation, and every example in
//! one is compiled — the wrong program must report exactly its code, and the right one
//! nothing — so the text cannot drift from what the compiler does.

mod parallel;

use dz_diag::codes::UNUSED_VARIABLE;
use dz_diag::explain;

/// Every code a single-file program reports, analysed as `dz check` would.
fn codes_of(tag: &str, source: &str) -> (Vec<String>, String) {
    let dir = std::env::temp_dir().join(format!("dz_explain_{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"explain\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.dz"), source).unwrap();
    let analysis = dz_driver::analyse(&dir);
    let codes = analysis.diags.iter().filter_map(|d| d.code).map(str::to_owned).collect();
    let rendered = dz_diag::render_to_string(&analysis.map, &analysis.diags);
    let _ = std::fs::remove_dir_all(&dir);
    (codes, rendered)
}

#[test]
fn every_code_is_explained() {
    let missing: Vec<&str> = explain::all_codes()
        .into_iter()
        .filter(|(code, _, _)| explain::section(code).is_none())
        .map(|(code, _, _)| code)
        .collect();
    assert!(
        missing.is_empty(),
        "no `## DZNNNN` section in crates/dz_diag/explain/ for {missing:?}"
    );

    let codes: Vec<&str> = explain::all_codes().into_iter().map(|(c, _, _)| c).collect();
    let mut seen = std::collections::HashSet::new();
    for (file, code) in explain::sections() {
        assert!(codes.contains(&code.as_str()), "{file}: `## {code}` is not a diagnostic code");
        assert!(seen.insert(code.clone()), "{file}: `## {code}` is explained twice");
    }
}

#[test]
fn every_example_does_what_it_says() {
    let all = explain::all_codes();
    let failures = parallel::filter_map(&all, |(code, _, _)| {
        let e = explain::explain(code)?;
        if e.no_example.is_some() {
            return None;
        }
        let (Some(wrong), Some(right)) = (&e.wrong, &e.right) else {
            return Some(format!(
                "{code}: needs a wrong and a right ```dz block, or a `No example:` line"
            ));
        };
        let mut out = Vec::new();
        let keep = |mut got: Vec<String>| {
            if *code != UNUSED_VARIABLE {
                got.retain(|c| c != UNUSED_VARIABLE);
            }
            got
        };
        let (got, rendered) = codes_of(&format!("{code}_wrong"), wrong);
        let got = keep(got);
        if got != [code.to_string()] {
            out.push(format!("{code}: the wrong example reports {got:?}\n{rendered}"));
        }
        let (got, rendered) = codes_of(&format!("{code}_right"), right);
        let got = keep(got);
        if !got.is_empty() {
            out.push(format!("{code}: the right example reports {got:?}\n{rendered}"));
        }
        (!out.is_empty()).then(|| out.join("\n"))
    });
    let mut failures = failures;
    failures.sort();
    assert!(failures.is_empty(), "{} codes failed:\n\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn a_code_is_found_however_it_is_written() {
    for arg in ["DZ0401", "dz0401", "0401", "401"] {
        assert_eq!(explain::normalise(arg).as_deref(), Some("DZ0401"), "{arg}");
    }
    assert!(explain::explain("DZ0215").is_none(), "a retired code explains nothing");
    let text = explain::render(&explain::explain("DZ0401").unwrap());
    assert!(text.starts_with("DZ0401: use after move\n"), "{text}");
    assert!(text.contains("§6.4"), "the rule names its spec section:\n{text}");
}
