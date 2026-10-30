//! Editor queries against a real program: `tests/run/generics`, which has generics, traits,
//! `impl` blocks, default methods and bare field access — everything a name can mean.

use std::path::{Path, PathBuf};

use dz_driver::{analyse, analyse_with, Analysis, Mode};
use dz_lex::TokenKind;
use dz_lsp::ide::{self, CompletionKind, DeclKind};
use dz_span::{FileId, Span};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/generics")
}

fn main_file(a: &Analysis) -> FileId {
    a.map.iter().find(|f| f.path().ends_with("main.dz")).expect("main.dz").id()
}

fn generics() -> (Analysis, FileId) {
    let a = analyse(&root());
    assert!(!a.diags.has_errors(), "the fixture must check cleanly");
    let file = main_file(&a);
    (a, file)
}

/// The fixture with `edit` applied to its unsaved buffer, analysed the way the server does.
fn edited(edit: impl Fn(&str) -> String) -> (Analysis, FileId, String) {
    let original = std::fs::read_to_string(root().join("src").join("main.dz")).unwrap();
    let text = edit(&original);
    let main = Path::new("src").join("main.dz");
    let overlay = |p: &Path| p.ends_with(&main).then(|| text.clone());
    let a = analyse_with(&root(), &overlay, Mode::Editor);
    let file = main_file(&a);
    (a, file, text)
}

/// Insert `typed` as a new line just above the line holding `needle`.
fn typed_above(needle: &str, typed: &str) -> impl Fn(&str) -> String {
    let (needle, typed) = (needle.to_owned(), typed.to_owned());
    move |text: &str| {
        let i = text.find(&needle).expect("needle");
        let line = text[..i].rfind('\n').map_or(0, |n| n + 1);
        format!("{}    {typed}\n{}", &text[..line], &text[line..])
    }
}

/// The offset `within` bytes into the first occurrence of `needle`.
fn offset_in(text: &str, needle: &str, within: usize) -> u32 {
    let start = text.find(needle).unwrap_or_else(|| panic!("`{needle}` is not in the text"));
    (start + within) as u32
}

fn at(a: &Analysis, file: FileId, needle: &str, within: usize) -> u32 {
    offset_in(a.map.get(file).text(), needle, within)
}

fn info(a: &Analysis, file: FileId, needle: &str, within: usize) -> ide::Info {
    ide::info_at(a, file, at(a, file, needle, within))
        .unwrap_or_else(|| panic!("nothing known at `{needle}`+{within}"))
}

fn definition_text(a: &Analysis, info: &ide::Info) -> (u32, String) {
    let (file, span) = info.definition.expect("a definition");
    (span.start, a.map.get(file).text()[span.range()].to_owned())
}

fn labels(items: &[ide::Completion]) -> Vec<&str> {
    items.iter().map(|c| c.label.as_str()).collect()
}

// --- hover and definition ------------------------------------------------------------------

/// A standard-library name leads into the file declaring it: the server shows that file from
/// a copy on disk, but the query itself must find the declaration (spec §14.4).
#[test]
fn definition_of_a_std_name_is_its_declaration_in_std() {
    let a = analyse(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/containers"));
    assert!(!a.diags.has_errors(), "the fixture must check cleanly");
    let f = main_file(&a);
    for (needle, within, declared, file_name) in [
        ("String>()", 0, "String", "<std>/text/string.dz"),
        ("List<i32>()", 0, "List", "<std>/collections/list.dz"),
        ("(0, Allocators::Temp)", 4, "Allocators", "<std>/memory/allocators.dz"),
        ("Temp)", 0, "Temp", "<std>/memory/allocators.dz"),
    ] {
        let i = info(&a, f, needle, within);
        let (file, _) = i.definition.unwrap_or_else(|| panic!("no definition for `{needle}`"));
        let want = ide::embedded_file(&a, file_name).expect("std was parsed");
        assert_eq!(file, want, "`{needle}` did not lead to {file_name}");
        assert_eq!(definition_text(&a, &i).1, declared, "at `{needle}`");
    }
    // And from inside the standard library, once a file of it is open — across its files.
    let string = ide::embedded_file(&a, "<std>/text/string.dz").expect("std was parsed");
    let i = info(&a, string, "m_bytes: List<u8>", 9);
    assert_eq!(definition_text(&a, &i).1, "List");
}

/// The math leads into its own files too (spec §14.4): the type, a method on it, and a method
/// on a number, which comes from `Float`.
#[test]
fn definition_of_a_std_math_name_is_in_its_file() {
    let a = analyse(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/math"));
    assert!(!a.diags.has_errors(), "the fixture must check cleanly");
    let f = main_file(&a);
    for (needle, within, declared, file_name) in [
        ("Vec3{ x: 1, y: 0, z: 0 }", 0, "Vec3", "<std>/math/vec3.dz"),
        ("a.length()", 2, "length", "<std>/math/vec2.dz"),
        ("x.sqrt()", 2, "sqrt", "<std>/math/float.dz"),
        ("Mat4::translation", 6, "translation", "<std>/math/mat4.dz"),
    ] {
        let i = info(&a, f, needle, within);
        let (file, _) = i.definition.unwrap_or_else(|| panic!("no definition for `{needle}`"));
        let want = ide::embedded_file(&a, file_name).expect("std was parsed");
        assert_eq!(a.map.get(file).path(), a.map.get(want).path(), "at `{needle}`");
        assert_eq!(definition_text(&a, &i).1, declared, "at `{needle}`");
    }
}

#[test]
fn hover_on_a_call_shows_the_declared_signature() {
    let (a, f) = generics();
    let hover = info(&a, f, "largest(3, 9)", 2).hover.unwrap();
    assert!(hover.contains("fn largest<T: Ord>(a: T, b: T) -> T"), "{hover}");
}

#[test]
fn hover_on_a_local_shows_its_inferred_type() {
    let (a, f) = generics();
    let hover = info(&a, f, "copied.len()", 1).hover.unwrap();
    assert!(hover.contains("let copied: Buffer<i32>"), "{hover}");
}

#[test]
fn hover_on_a_declaration_shows_its_header() {
    let (a, f) = generics();
    let hover = info(&a, f, "struct Pair<T>", 8).hover.unwrap();
    assert!(hover.contains("struct Pair<T>"), "{hover}");
}

#[test]
fn hover_on_a_trait_in_an_impl_header_is_the_trait() {
    let (a, f) = generics();
    let hover = info(&a, f, "impl Ord for Score", 5).hover.unwrap();
    assert!(hover.contains("trait Ord"), "{hover}");
}

#[test]
fn definition_of_a_method_call_is_the_method_in_its_struct() {
    let (a, f) = generics();
    let (start, text) = definition_text(&a, &info(&a, f, "ints.push(10)", 6));
    assert_eq!(text, "push");
    let src = a.map.get(f).text();
    let body =
        src.find("struct Buffer<T>").unwrap() as u32..src.find("impl<T: Copy>").unwrap() as u32;
    assert!(body.contains(&start), "`push` should be the one declared in `struct Buffer`");
}

#[test]
fn definition_of_an_inherited_default_method_is_the_trait_default() {
    let (a, f) = generics();
    let (start, text) = definition_text(&a, &info(&a, f, "c.shout()", 3));
    assert_eq!(text, "shout");
    let src = a.map.get(f).text();
    assert!(start > src.find("trait Describe {").unwrap() as u32);
}

#[test]
fn definition_of_a_field_is_its_name() {
    let (a, f) = generics();
    let (start, text) = definition_text(&a, &info(&a, f, "p.first, p.second", 3));
    assert_eq!(text, "first");
    assert_eq!(start, at(&a, f, "first:  T", 0));
}

#[test]
fn definition_of_a_bare_field_inside_a_method_is_the_field() {
    let (a, f) = generics();
    let (start, text) = definition_text(&a, &info(&a, f, "m_len == m_cap", 1));
    assert_eq!(text, "m_len");
    assert_eq!(start, at(&a, f, "m_len:  usize", 0));
}

#[test]
fn definition_of_a_type_in_a_signature_is_the_type() {
    let (a, f) = generics();
    let (start, text) = definition_text(&a, &info(&a, f, "rhs: &Score) -> bool", 7));
    assert_eq!(text, "Score");
    assert_eq!(start, at(&a, f, "struct Score", 7));
}

#[test]
fn a_type_argument_is_not_mistaken_for_the_call_around_it() {
    let (a, f) = generics();
    let (_, text) = definition_text(&a, &info(&a, f, "Buffer<Tracked>(3)", 9));
    assert_eq!(text, "Tracked");
}

#[test]
fn a_local_goes_to_its_binding() {
    let (a, f) = generics();
    let (start, _) = definition_text(&a, &info(&a, f, "seen += n", 0));
    assert_eq!(start, at(&a, f, "seen = 0", 0));
}

#[test]
fn queries_still_answer_beside_a_syntax_error() {
    let (a, f, text) = edited(typed_above("println!(\"done\")", "let broken = ("));
    assert!(!a.parsed_cleanly);
    assert!(
        a.diags
            .iter()
            .all(|d| d.code.is_some_and(|c| c.starts_with("DZ01") || c.starts_with("DZ02"))),
        "only syntax errors are shown while there is one"
    );
    let hover = ide::info_at(&a, f, offset_in(&text, "largest(3, 9)", 2)).unwrap().hover.unwrap();
    assert!(hover.contains("fn largest"), "{hover}");
    let copied = ide::info_at(&a, f, offset_in(&text, "copied.len()", 1)).unwrap().hover.unwrap();
    assert!(copied.contains("Buffer<i32>"), "{copied}");
}

// --- references and rename -----------------------------------------------------------------

/// How many times `name` is written as a name (not in a string or comment) in the fixture.
fn written(a: &Analysis, f: FileId, name: &str) -> usize {
    let text = a.map.get(f).text();
    dz_lex::lex(f, text)
        .tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Ident && t.text(text) == name)
        .count()
}

#[test]
fn references_find_every_use_of_each_kind_of_name() {
    let (a, f) = generics();
    // A field used bare, through `other.`, in `impl` bodies; a generic function; a struct
    // with its constructor; a local; a trait method implemented twice and called three ways.
    for name in ["m_len", "largest", "Countdown", "seen", "label", "Score"] {
        let first = at(&a, f, name, 0);
        let refs = ide::references(&a, f, first, true);
        assert_eq!(refs.len(), written(&a, f, name), "references to `{name}`: {refs:?}");
    }
}

#[test]
fn references_can_leave_out_the_declaration() {
    let (a, f) = generics();
    let all = ide::references(&a, f, at(&a, f, "seen += n", 0), true);
    let uses = ide::references(&a, f, at(&a, f, "seen += n", 0), false);
    assert_eq!(uses.len() + 1, all.len());
}

#[test]
fn rename_changes_every_reference() {
    let (a, f) = generics();
    let spans = ide::rename(&a, f, at(&a, f, "seen += n", 0), "total_seen").unwrap();
    assert_eq!(spans.len(), 3);
}

#[test]
fn rename_refuses_keywords_bad_names_and_the_prelude() {
    let (a, f) = generics();
    let seen = at(&a, f, "seen += n", 0);
    assert!(ide::rename(&a, f, seen, "fn").is_err());
    assert!(ide::rename(&a, f, seen, "1st").is_err());
    assert!(ide::rename(&a, f, seen, "two words").is_err());
    let ordering = at(&a, f, "-> Ordering", 3);
    assert!(ide::prepare_rename(&a, f, ordering).unwrap_err().contains("prelude"));
    let self_param = at(&a, f, "return self.name()", 7);
    assert!(ide::prepare_rename(&a, f, self_param).is_err());
}

// --- completion ----------------------------------------------------------------------------

#[test]
fn completion_offers_locals_items_keywords_and_builtins() {
    let (a, f) = generics();
    let text = a.map.get(f).text();
    let offset = at(&a, f, "println!(\"done\")", 0) as usize;
    let items = ide::complete(Some((&a, f)), None, text, offset);
    let got = labels(&items);
    for want in ["ints", "seen", "largest", "Buffer", "Ordering", "println", "let", "i32"] {
        assert!(got.contains(&want), "{want} missing");
    }
    let seen = items.iter().find(|c| c.label == "seen").unwrap();
    assert_eq!(seen.kind, CompletionKind::Variable);
    assert_eq!(seen.detail.as_deref(), Some("i32"));
}

/// Complete right after `typed`, in the fixture with `typed` inserted above `needle` and
/// analysed as it now stands — what the server does when a `.` is typed.
fn complete_after(needle: &str, typed: &str) -> Vec<String> {
    let (a, f, text) = edited(typed_above(needle, typed));
    // The end of the inserted line: the newline just before `needle`'s line. (Searching for
    // `typed` itself could find it in a comment first.)
    let offset = text[..text.find(needle).unwrap()].rfind('\n').unwrap();
    ide::complete(Some((&a, f)), None, &text, offset).into_iter().map(|c| c.label).collect()
}

/// After `#`, only what `#` invokes: the intrinsics, and the `decl` and `stmt` macros in
/// scope. The formatting macros are `println!`, so a bare `println` writes that instead
/// (spec §12.5, §12.7, §12.9).
#[test]
fn completion_after_a_hash_offers_intrinsics_and_println_writes_its_bang() {
    let got = complete_after("println!(\"pair", "#");
    for want in ["location", "file"] {
        assert!(got.iter().any(|l| l == want), "{want} missing from {got:?}");
    }
    for unwanted in ["println", "format", "let", "ints"] {
        assert!(!got.iter().any(|l| l == unwanted), "{unwanted} offered after `#`: {got:?}");
    }

    let (a, f, text) = edited(typed_above("println!(\"pair", "prin"));
    let offset = text[..text.find("println!(\"pair").unwrap()].rfind('\n').unwrap();
    let items = ide::complete(Some((&a, f)), None, &text, offset);
    let println = items.iter().find(|c| c.label == "println").expect("println offered");
    assert_eq!(println.insert.as_deref(), Some("println!"));
}

#[test]
fn completion_after_a_dot_offers_fields_not_keywords() {
    let got = complete_after("println!(\"pair", "p.");
    assert!(got.iter().any(|l| l == "first") && got.iter().any(|l| l == "second"), "{got:?}");
    assert!(!got.iter().any(|l| l == "let"), "no keywords after a dot");
}

#[test]
fn completion_after_a_dot_offers_methods_but_not_private_fields() {
    let got = complete_after("println!(\"ints: len", "ints.");
    for want in ["push", "len", "at"] {
        assert!(got.iter().any(|l| l == want), "{want} missing from {got:?}");
    }
    assert!(!got.iter().any(|l| l == "m_data"), "private fields stay private outside the struct");
}

#[test]
fn completion_types_a_call_as_the_receiver() {
    let got = complete_after("println!(\"done", "largest(Score(12), Score(4)).");
    for want in ["value", "cmp", "eq"] {
        assert!(got.iter().any(|l| l == want), "{want} missing from {got:?}");
    }
    let got = complete_after("println!(\"done", "Countdown(3).");
    for want in ["valid", "get", "advance"] {
        assert!(got.iter().any(|l| l == want), "{want} missing from {got:?}");
    }
}

#[test]
fn completion_by_name_works_from_an_older_analysis() {
    // The analysis has not seen the edit: the receiver is found by name instead.
    let (a, f) = generics();
    let text = a.map.get(f).text();
    let i = text.find("println!(\"pair").unwrap();
    let edit = format!("{}p.\n    {}", &text[..i], &text[i..]);
    let got = ide::complete(None, Some((&a, f)), &edit, i + 2);
    assert!(labels(&got).contains(&"first"), "{:?}", labels(&got));
}

#[test]
fn completion_after_type_colons_offers_type_queries() {
    let got = complete_after("println!(\"done", "i32::");
    assert!(got.iter().any(|l| l == "is_trivially_copyable"));
}

#[test]
fn completion_is_quiet_inside_comments_and_strings() {
    let (a, f) = generics();
    assert!(ide::complete(Some((&a, f)), None, "// let", 6).is_empty());
    assert!(ide::complete(Some((&a, f)), None, "println!(\"le", 12).is_empty());
}

#[test]
fn every_completed_keyword_is_a_keyword() {
    for k in ide::KEYWORDS {
        assert!(dz_lex::keyword(k).is_some(), "`{k}` is not a keyword");
    }
}

// --- signature help, inlay hints, symbols, formatting --------------------------------------

#[test]
fn signature_help_tracks_the_argument_under_the_cursor() {
    let (a, f) = generics();
    let sig = ide::signature_help(&a, f, at(&a, f, "largest(3, 9)", 11)).unwrap();
    assert_eq!(sig.label, "fn largest<T: Ord>(a: T, b: T) -> T");
    let params: Vec<&str> = sig.params.iter().map(|&(s, e)| &sig.label[s..e]).collect();
    assert_eq!(params, ["a: T", "b: T"]);
    assert_eq!(sig.active, 1);
}

/// Every argument past the fixed ones is the variadic parameter's, however many there are
/// (spec §7.2).
#[test]
fn signature_help_stays_on_a_variadic_parameter() {
    let a = analyse(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/variadic"));
    assert!(!a.diags.has_errors(), "the fixture must check cleanly");
    let f = main_file(&a);
    let sig = ide::signature_help(&a, f, at(&a, f, "tell(\"mixed\", c, n, Cat", 17)).unwrap();
    let params: Vec<&str> = sig.params.iter().map(|&(s, e)| &sig.label[s..e]).collect();
    assert_eq!(params, ["prefix: str", "things: ..&Describe"]);
    assert_eq!(sig.active, 1, "the third argument is still `things`");
}

#[test]
fn signature_help_follows_an_argument_passed_by_name() {
    let a = analyse(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/formatting"));
    assert!(!a.diags.has_errors(), "the fixture must check cleanly");
    let f = main_file(&a);
    let call = "total(\"d\", 4, offset = 1, scale = 3)";
    let active = |col: usize| ide::signature_help(&a, f, at(&a, f, call, col)).unwrap().active;
    // label, xs, scale, offset, loc
    assert_eq!(active(12), 1, "`4` is a variadic argument");
    assert_eq!(active(16), 3, "`offset = 1` is `offset`, though it comes first");
    assert_eq!(active(28), 2, "`scale = 3` is `scale`");
}

#[test]
fn signature_help_leaves_out_the_receiver() {
    let (a, f) = generics();
    let sig = ide::signature_help(&a, f, at(&a, f, "ints.push(10)", 10)).unwrap();
    let params: Vec<&str> = sig.params.iter().map(|&(s, e)| &sig.label[s..e]).collect();
    assert_eq!(params, ["v: T"]);
    assert_eq!(sig.active, 0);
}

#[test]
fn inlay_hints_show_inferred_types_only() {
    let (a, f) = generics();
    let text = a.map.get(f).text();
    let hints = ide::inlay_hints(&a, f, dz_span::Span::new(0, text.len()));
    let hint_at = |needle: &str, name: &str| {
        let end = offset_in(text, needle, 0) + name.len() as u32;
        hints.iter().find(|h| h.offset == end).map(|h| h.label.clone())
    };
    assert_eq!(hint_at("copied = ints", "copied").as_deref(), Some(": Buffer<i32>"));
    assert_eq!(hint_at("seen = 0", "seen").as_deref(), Some(": i32"));
    assert_eq!(hint_at("grid: [SLOTS]i32", "grid"), None, "a written type gets no hint");
}

/// A hint must show a name you could actually type at that position (spec §3.3).
///
/// An imported header's type is only ever reachable as `mini::MiniVec2` in the file that
/// wrote `import cpp "mini.h" as mini` — a bare `MiniVec2` names nothing there, so
/// offering it in a hint offers something that would not compile.
#[test]
fn inlay_hints_qualify_an_imported_type_the_way_the_file_must_write_it() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/cinterop");
    let a = analyse(&dir);
    if a.diags.has_errors() {
        // `import cpp` needs libclang; without it there is nothing to assert about.
        eprintln!("skipping: the C interop fixture did not analyse (libclang missing?)");
        return;
    }
    let file = main_file(&a);
    let text = a.map.get(file).text();
    let hints = ide::inlay_hints(&a, file, dz_span::Span::new(0, text.len()));

    let end = offset_in(text, "a = mini::MiniVec2{", 0) + 1;
    let label = hints.iter().find(|h| h.offset == end).map(|h| h.label.clone());
    assert_eq!(label.as_deref(), Some(": mini::MiniVec2"), "{hints:?}");
}

/// What a call may end is part of its signature (spec §6.7), so every place that shows one
/// shows the `kills` clause: hover, signature help, and a method's completion detail.
#[test]
fn a_call_shows_what_it_kills() {
    let a = analyse(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/containers"));
    assert!(!a.diags.has_errors(), "the fixture must check cleanly");
    let f = main_file(&a);
    let push = "names.push(String(\"ada\"))";

    let hover = info(&a, f, push, 6).hover.unwrap();
    assert!(hover.contains("fn push(&mut self, v: T") && hover.contains(") kills self@items"));

    let sig = ide::signature_help(&a, f, at(&a, f, push, 11)).unwrap();
    assert!(sig.label.ends_with("kills self@items"), "{}", sig.label);

    let text = a.map.get(f).text();
    let offset = at(&a, f, push, 6) as usize;
    let items = ide::complete(Some((&a, f)), None, text, offset);
    let detail = |name: &str| {
        items.iter().find(|c| c.label == name).and_then(|c| c.detail.clone()).unwrap_or_default()
    };
    assert_eq!(detail("push"), "fn push(v: T, loc: SourceLocation) kills self@items");
    assert_eq!(detail("clear"), "fn clear() kills self@items, self[*]");
    assert_eq!(detail("len"), "fn len() -> usize", "a function that kills nothing says nothing");
}

/// A function that writes no clause, over a body that kills, is shown the clause it needs
/// where it goes — the one its `DZ0415` fix inserts.
#[test]
fn inlay_hints_show_the_clause_a_body_needs() {
    let (a, f, text) = edited(|t: &str| {
        format!(
            "{t}\nfn grow(xs: &mut Buffer<i32>, ys: &mut List<i32>) {{\n    ys.push(1)\n    \
             ys.push(2)\n}}\n\nfn fine(ys: &mut List<i32>) kills ys@items {{\n    ys.push(1)\n}}\n"
        )
    });
    let hints = ide::inlay_hints(&a, f, dz_span::Span::new(0, text.len()));
    let clauses: Vec<(u32, &str)> = hints
        .iter()
        .filter(|h| h.kind == ide::HintKind::Clause)
        .map(|h| (h.offset, h.label.as_str()))
        .collect();
    let brace = offset_in(&text, "List<i32>) {", 11);
    assert_eq!(clauses, [(brace, "kills ys@items")], "one hint, before `grow`'s body: {hints:?}");
}

#[test]
fn inlay_hints_name_the_fields_of_a_positional_literal() {
    let (a, f, text) = edited(typed_above("ints.push(10)", "let hinted = Vec2{ 1.5, 2.5 }"));
    let hints = ide::inlay_hints(&a, f, dz_span::Span::new(0, text.len()));
    let field_at = |needle: &str, within: usize| {
        let at = offset_in(&text, needle, within);
        hints
            .iter()
            .find(|h| h.offset == at && h.kind == ide::HintKind::Field)
            .map(|h| h.label.clone())
    };
    assert_eq!(field_at("Vec2{ 1.5, 2.5 }", 6).as_deref(), Some("x:"), "{hints:?}");
    assert_eq!(field_at("Vec2{ 1.5, 2.5 }", 11).as_deref(), Some("y:"), "{hints:?}");
    // `Vec2{ x: 1.0, y: 2.0 }` already names them.
    assert_eq!(field_at("Vec2{ x: 1.0, y: 2.0 }", 9), None, "a named field gets no hint");
}

#[test]
fn semantic_tokens_classify_names_by_what_they_are() {
    use dz_lsp::ide::TokenClass as C;
    let (a, f) = generics();
    let text = a.map.get(f).text();
    let tokens = ide::semantic_tokens(&a, f);
    let class_at = |needle: &str, within: usize| {
        let off = offset_in(text, needle, within);
        tokens
            .iter()
            .find(|t| t.span.start <= off && off < t.span.end)
            .map(|t| (t.class, t.declaration, t.readonly))
    };
    assert_eq!(class_at("a: T, b: T) -> T", 0), Some((C::Parameter, true, false)));
    assert_eq!(class_at("return move(a)", 12), Some((C::Parameter, false, false)));
    assert_eq!(class_at("let mut seen", 8), Some((C::Variable, true, false)));
    assert_eq!(class_at("seen += n", 0), Some((C::Variable, false, false)));
    assert_eq!(class_at("m_len == m_cap", 0), Some((C::Property, false, false)));
    assert_eq!(class_at("ints.push(10)", 5), Some((C::Method, false, false)));
    assert_eq!(class_at("return .Less", 8), Some((C::EnumMember, false, false)));
    assert_eq!(class_at("Tracked(1)", 0), Some((C::Struct, false, false)));
    assert_eq!(class_at("SLOTS, HALF, grid", 0), Some((C::Variable, false, true)));
    assert_eq!(class_at("fn largest", 3), Some((C::Function, true, false)));
    assert_eq!(class_at("impl Ord for", 5), Some((C::Interface, false, false)));
    // What the compiler does not resolve — a builtin type — is left to the grammar.
    assert_eq!(class_at("-> i32 {", 3), None);
}

#[test]
fn members_of_builtin_types_still_hover_and_colour() {
    // `Error` is provided by the language (spec §9.2): `e.message` has no declaration.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/failure");
    let a = analyse(&root);
    assert!(!a.diags.has_errors(), "the fixture must check cleanly");
    let f = main_file(&a);
    let hover = info(&a, f, "e.message)", 3).hover.unwrap();
    assert!(hover.contains("message: str"), "{hover}");
    let off = at(&a, f, "e.message)", 3);
    let tokens = ide::semantic_tokens(&a, f);
    let message = tokens.iter().find(|t| t.span.start <= off && off < t.span.end);
    assert_eq!(message.map(|t| t.class), Some(ide::TokenClass::Property));
}

#[test]
fn the_outline_nests_members_under_their_type() {
    let (a, f) = generics();
    let ast = &a.files.values().flatten().find(|p| p.file == f).unwrap().ast;
    let decls = ide::decls(a.map.get(f).text(), ast);
    let buffer = decls.iter().find(|d| d.name == "Buffer").unwrap();
    assert_eq!(buffer.kind, DeclKind::Struct);
    let members: Vec<&str> = buffer.children.iter().map(|c| c.name.as_str()).collect();
    for m in ["m_data", "Buffer", "push", "len", "at"] {
        assert!(members.contains(&m), "{m} missing from {members:?}");
    }
    assert!(decls.iter().any(|d| d.kind == DeclKind::Impl && d.name == "impl Ord for Score"));
}

#[test]
fn workspace_symbols_match_fuzzily_and_name_their_container() {
    let (a, _) = generics();
    let found = ide::workspace_symbols(&a, "cntdwn");
    assert!(found.iter().any(|m| m.name == "Countdown"));
    let push = ide::workspace_symbols(&a, "push");
    assert!(push.iter().any(|m| m.name == "push" && m.container.as_deref() == Some("Buffer")));
    assert!(ide::workspace_symbols(&a, "Ordering").is_empty(), "the prelude is not the project's");
}

// --- imported C and C++ names (spec §15.2) -------------------------------------------------

fn cppinterop() -> (Analysis, FileId) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/cppinterop");
    let a = analyse(&root);
    let file = main_file(&a);
    (a, file)
}

#[test]
fn hover_on_an_imported_function_shows_its_signature_and_the_header_s_docs() {
    // The M7a deferral: an imported name is not written in Dazzle anywhere, so hover has no
    // source to quote. It is rebuilt from the types the checker gave it instead, and the
    // header's own comment is rendered through Doxygen (spec §15.2).
    let (a, f) = cppinterop();
    if a.diags.has_errors() {
        eprintln!("skipping: the fixture needs libclang");
        return;
    }
    let hover = info(&a, f, "area_of(shp", 2).hover.unwrap();
    // The signature, in Dazzle's types rather than C's.
    assert!(hover.contains("fn area_of(kind: Kind, size: i32) -> i32"), "{hover}");
    // Where it really comes from, since the file it is declared in is a C++ header.
    assert!(hover.contains("::shapes::area_of"), "{hover}");
    // The comment, with its Doxygen commands laid out.
    assert!(hover.contains("The area of a shape of the given kind."), "{hover}");
    assert!(hover.contains("**Parameters**"), "{hover}");
    assert!(hover.contains("- `kind` — which shape to measure"), "{hover}");
    assert!(hover.contains("**Returns** the area, in whole units"), "{hover}");
    assert!(hover.contains("**Note:** This is not a real geometry library."), "{hover}");
    // And none of the markup survives as markup.
    assert!(!hover.contains("@param"), "{hover}");
}

#[test]
fn hover_on_an_imported_type_shows_its_fields() {
    let (a, f) = cppinterop();
    if a.diags.has_errors() {
        return;
    }
    let hover = info(&a, f, "Point{ x: 1", 0).hover.unwrap();
    assert!(hover.contains("struct Point"), "{hover}");
    assert!(hover.contains("x: i32"), "{hover}");
}

#[test]
fn a_module_alias_is_coloured_as_a_namespace_not_a_variable() {
    // `rl::SetTargetFPS(60)` — the head of a qualified path is a module, which has no
    // symbol of its own, so without this it reads as a variable (spec §15.2).
    let (a, f) = cppinterop();
    if a.diags.has_errors() {
        return;
    }
    let text = a.map.get(f).text().to_owned();
    let tokens = ide::semantic_tokens(&a, f);
    let head = offset_in(&text, "shp::shapes::Point{ x: 1", 0);
    let token = tokens
        .iter()
        .find(|t| t.span.start == head)
        .unwrap_or_else(|| panic!("no token at the head of the path"));
    assert_eq!(token.class, ide::TokenClass::Namespace);
    // And the C++ namespace after it is one too.
    let ns = offset_in(&text, "shapes::Point{ x: 1", 0);
    let token = tokens.iter().find(|t| t.span.start == ns).expect("no token on the namespace");
    assert_eq!(token.class, ide::TokenClass::Namespace);
}

/// Complete at the end of `typed`, appended as a new line at the end of `main` in the
/// cppinterop fixture.
fn complete_in_cppinterop(typed: &str) -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/cppinterop");
    let original = std::fs::read_to_string(root.join("src").join("main.dz")).unwrap();
    // Just before the final `}` of `main`.
    let close = original.rfind('}').unwrap();
    let text = format!("{}    {typed}\n{}", &original[..close], &original[close..]);
    let offset = original[..close].len() + 4 + typed.len();
    let main = Path::new("src").join("main.dz");
    let overlay = |p: &Path| p.ends_with(&main).then(|| text.clone());
    let a = analyse_with(&root, &overlay, Mode::Editor);
    let file = main_file(&a);
    ide::complete(Some((&a, file)), None, &text, offset).into_iter().map(|c| c.label).collect()
}

#[test]
fn completion_after_a_path_walks_every_segment_written_before_it() {
    // `rl::MouseButton::` — the type is in the module the path names, not in this file's
    // own scope, so completing on the last segment alone found nothing (spec §15.2).
    let variants = complete_in_cppinterop("let k = shp::shapes::Kind::");
    if variants.is_empty() {
        eprintln!("skipping: the fixture needs libclang");
        return;
    }
    assert!(variants.iter().any(|l| l == "Round"), "{variants:?}");
    assert!(variants.iter().any(|l| l == "Square"), "{variants:?}");

    // A C++ namespace lists what it holds, each with its own kind.
    let inside = complete_in_cppinterop("let x = shp::shapes::");
    for want in ["Point", "Kind", "area_of", "Counter", "Buffer", "geom"] {
        assert!(inside.iter().any(|l| l == want), "`{want}` missing from {inside:?}");
    }
    // And a nested one goes a segment deeper.
    let nested = complete_in_cppinterop("let y = shp::shapes::geom::");
    assert!(nested.iter().any(|l| l == "manhattan"), "{nested:?}");

    // The module itself still offers what it exports at the top level.
    let top = complete_in_cppinterop("let z = shp::");
    assert!(top.iter().any(|l| l == "shapes"), "{top:?}");
    assert!(top.iter().any(|l| l == "IntPair"), "{top:?}");
}

#[test]
fn signature_help_on_an_imported_call_names_the_parameter_being_typed() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/cppinterop");
    let a = analyse(&root);
    if a.diags.has_errors() {
        eprintln!("skipping: the fixture needs libclang");
        return;
    }
    let f = main_file(&a);
    let text = a.map.get(f).text();
    // Inside the parentheses of `area_of(shp::shapes::Kind::Round, 5)`.
    let open = text.find("area_of(shp").unwrap() + "area_of(".len();
    let sig = ide::signature_help(&a, f, open as u32).expect("signature help on an imported call");
    assert!(sig.label.contains("fn area_of(kind: Kind, size: i32) -> i32"), "{}", sig.label);
    assert_eq!(sig.params.len(), 2, "{:?}", sig.params);
    // The first parameter is the one being typed, and it is named with its type.
    assert_eq!(sig.active, 0);
    assert_eq!(&sig.label[sig.params[0].0..sig.params[0].1], "kind: Kind");
    assert_eq!(&sig.label[sig.params[1].0..sig.params[1].1], "size: i32");
    // The header's documentation comes with it, rendered.
    assert!(sig.docs.contains("- `kind` — which shape to measure"), "{}", sig.docs);

    // After the comma it is the second.
    let comma = text[open..].find(", 5").unwrap() + open + 2;
    let sig = ide::signature_help(&a, f, comma as u32).expect("signature help after the comma");
    assert_eq!(sig.active, 1);
}

#[test]
fn doxygen_commands_render_as_markdown() {
    // Both spellings, the tags that carry layout, and a passthrough for everything else.
    let rendered = ide::doxygen(
        "\\brief Opens a window.\n\
         @param width  how wide\n\
         @param[out] out where it goes\n\
         @returns whether it worked\n\
         @see CloseWindow\n\
         @warning Call this once.",
    );
    assert!(rendered.starts_with("Opens a window."), "{rendered}");
    assert!(rendered.contains("- `width` — how wide"), "{rendered}");
    // `[out]` is Doxygen's bookkeeping, not part of the name.
    assert!(rendered.contains("- `out` — where it goes"), "{rendered}");
    assert!(rendered.contains("**Returns** whether it worked"), "{rendered}");
    assert!(rendered.contains("**See:** CloseWindow"), "{rendered}");
    assert!(rendered.contains("**Warning:** Call this once."), "{rendered}");

    // A comment with no commands is passed through exactly.
    assert_eq!(ide::doxygen("Just prose.\n\nTwo paragraphs."), "Just prose.\n\nTwo paragraphs.");

    // A continuation line belongs to the parameter above it.
    let wrapped = ide::doxygen("@param n the count\n  of things");
    assert!(wrapped.contains("- `n` — the count of things"), "{wrapped}");
}

#[test]
fn formatting_refuses_broken_source_and_is_idempotent() {
    let path = Path::new("t.dz");
    assert!(ide::format(&ide::parse(path, "fn main( {")).is_none());
    let (a, f) = generics();
    let once = ide::format(&ide::parse(path, a.map.get(f).text())).unwrap();
    let twice = ide::format(&ide::parse(path, &once)).unwrap();
    assert_eq!(once, twice);
}

// --- what a manifest names (spec §3.2, §17.4) ----------------------------------------------

fn opened_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/workspace/opened")
}

/// Complete at the end of `typed`, added as a line at the end of `noise` in the `opened`
/// fixture's `clean.dz`.
fn complete_in_opened(typed: &str) -> ide::CompletionSet {
    let root = opened_root();
    let original = std::fs::read_to_string(root.join("src").join("clean.dz")).unwrap();
    let close = original.rfind('}').unwrap();
    let text = format!("{}    {typed}\n{}", &original[..close], &original[close..]);
    let offset = original[..close].len() + 4 + typed.len();
    let clean = Path::new("src").join("clean.dz");
    let overlay = |p: &Path| p.ends_with(&clean).then(|| text.clone());
    let a = analyse_with(&root, &overlay, Mode::Editor);
    let file = a.map.iter().find(|f| f.path().ends_with("clean.dz")).expect("clean.dz").id();
    ide::completion(Some((&a, file)), None, &text, offset)
}

#[test]
fn completion_offers_what_opened_dependencies_export_bare() {
    let set = complete_in_opened("let g = gra");
    let gravity = set.items.iter().find(|c| c.label == "gravity").expect("phys's gravity, bare");
    assert_eq!(gravity.insert, None, "an opened dependency's name is written as it is");
    // `[names]`, a `via` key opened, and the standard library, all bare too.
    let labels = labels(&complete_in_opened("let x = ").items).join(" ");
    for want in ["Color", "Meters", "String", "Body", "phys", "audio"] {
        assert!(labels.split(' ').any(|l| l == want), "`{want}` missing from {labels}");
    }
}

#[test]
fn completion_writes_a_name_of_a_dependency_that_is_not_opened_with_its_key() {
    let set = complete_in_opened("let n = pla");
    let play = set.items.iter().find(|c| c.label == "play_sound").expect("audio's play_sound");
    assert_eq!(play.insert.as_deref(), Some("audio::play_sound"));
    assert_eq!(play.source.as_deref(), Some("audio"));
    // Fuzzy, the way the editor filters: the first letter, then the rest in order.
    assert!(complete_in_opened("let n = psnd").items.iter().any(|c| c.label == "play_sound"));
    assert!(!complete_in_opened("let n = snd").items.iter().any(|c| c.label == "play_sound"));
    // Nothing typed yet: none of them, and the editor is told to ask again.
    let empty = complete_in_opened("let n = ");
    assert!(empty.incomplete);
    assert!(!empty.items.iter().any(|c| c.source.is_some()), "a prefix narrows them first");
}

#[test]
fn completion_offers_a_shared_name_only_with_each_key() {
    // `Vec3` is exported by std, geo and phys, all opened: bare it is DZ0310.
    let set = complete_in_opened("let v = Vec");
    let vec3: Vec<Option<&str>> =
        set.items.iter().filter(|c| c.label == "Vec3").map(|c| c.insert.as_deref()).collect();
    assert!(!vec3.contains(&None), "a bare `Vec3` would not compile: {vec3:?}");
    for want in ["std::Vec3", "geo::Vec3", "phys::Vec3"] {
        assert!(vec3.contains(&Some(want)), "`{want}` missing from {vec3:?}");
    }
}

#[test]
fn completion_after_a_key_walks_public_dependencies() {
    let labels: Vec<String> =
        complete_in_opened("let m = phys::").items.into_iter().map(|c| c.label).collect();
    assert!(labels.iter().any(|l| l == "fall"), "{labels:?}");
    assert!(labels.iter().any(|l| l == "units"), "a public dependency is offered: {labels:?}");
    let inner: Vec<String> =
        complete_in_opened("let m = phys::units::").items.into_iter().map(|c| c.label).collect();
    assert!(inner.iter().any(|l| l == "Meters"), "{inner:?}");
}

#[test]
fn a_manifest_links_its_dependencies() {
    let root = opened_root();
    let a = analyse(&root);
    assert!(!a.diags.has_errors(), "the fixture must check cleanly");
    let links = ide::manifest_links(&a, &root.join("dazzle.toml"));
    let text = std::fs::read_to_string(root.join("dazzle.toml")).unwrap();
    let geo = links
        .iter()
        .find(|(_, span, _, _)| &text[span.range()] == "\"libs/geo\"")
        .expect("a link on geo's path");
    assert!(geo.2.ends_with(Path::new("geo").join("dazzle.toml")), "{:?}", geo.2);
    // `std` is carried by the compiler and has no manifest to open.
    assert!(!links.iter().any(|(_, span, _, _)| &text[span.range()] == "std"));
}

/// A dependency key has no declaration, but going to it opens what it names: a project's
/// manifest, through public dependencies too (spec §3.2, §17.4).
#[test]
fn definition_of_a_dependency_key_is_its_manifest() {
    let root = opened_root();
    let a = analyse(&root);
    assert!(!a.diags.has_errors(), "the fixture must check cleanly");
    let f = a.map.iter().find(|s| s.path().ends_with("clean.dz")).expect("clean.dz").id();
    for (needle, within, manifest_dir) in [
        ("phys::fall(t)", 0, "phys"),
        ("phys::units::Meters", 6, "units"),
        ("audio::play", 0, "audio"),
    ] {
        let i = info(&a, f, needle, within);
        let (file, _) = i.definition.unwrap_or_else(|| panic!("no definition at `{needle}`"));
        let path = a.map.get(file).path();
        assert!(
            path.ends_with(Path::new(manifest_dir).join("dazzle.toml")),
            "`{needle}` → {path:?}"
        );
        assert!(i.hover.unwrap().starts_with("**dependency**"), "at `{needle}`");
    }
    // The standard library has no file to open, but still says what the key is.
    let i = info(&a, f, "std::PI", 0);
    assert!(i.definition.is_none());
    assert!(i.hover.unwrap().contains("the standard library"));
}

/// A `cpp` dependency's key goes to its header (spec §15.1, §17.4).
#[test]
fn definition_of_a_cpp_dependency_key_is_its_header() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/cinterop");
    let a = analyse(&root);
    if a.diags.has_errors() {
        eprintln!("skipping: the fixture needs libclang");
        return;
    }
    let f = main_file(&a);
    let i = info(&a, f, "mini::", 0);
    let (file, _) = i.definition.expect("a definition for `mini`");
    assert!(a.map.get(file).path().ends_with("mini.h"), "{:?}", a.map.get(file).path());
    assert!(i.hover.unwrap().contains("`mini.h`"));
}

/// Append a declaration to the fixture's buffer, and give back the analysis and the text.
fn with_appended(extra: &str) -> (Analysis, FileId, String) {
    let extra = extra.to_owned();
    edited(move |text| format!("{text}\n{extra}"))
}

/// Apply a fix's edits to `text`, rightmost first so earlier offsets stay valid.
fn apply(text: &str, fix: &dz_diag::Fix) -> String {
    let mut edits: Vec<_> = fix.edits.iter().collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut out = text.to_owned();
    for e in edits {
        out.replace_range(e.span.start as usize..e.span.end as usize, &e.text);
    }
    out
}

/// A `switch` missing arms offers to write them, and **what it writes type-checks** — the
/// property that matters, since an editor applies a fix without the user reading it.
#[test]
fn a_non_exhaustive_switch_offers_the_missing_arms() {
    let source = "enum Colour {\n    Red\n    Green\n    Blue\n}\n\n\
                  fn pick(c: Colour) {\n    switch c {\n        case Red {}\n    }\n}\n";
    let (a, file, text) = with_appended(source);

    let switch = text.find("switch c").expect("the switch");
    let fixes = ide::code_actions(&a, file, Span::new(switch, switch));
    let fix = match fixes.as_slice() {
        [one] => one,
        other => panic!("expected exactly one fix, got {}", other.len()),
    };
    assert_eq!(fix.title, "add the missing arms");

    let fixed = apply(&text, fix);
    let tail = fixed[fixed.find("enum Colour").unwrap()..].to_owned();
    assert!(fixed.contains("case Green {}") && fixed.contains("case Blue {}"), "{tail}");
    // Indented one step past the `switch`, which is itself indented inside `pick`.
    assert!(fixed.contains("\n        case Green {}\n"), "wrong indent:\n{tail}");

    // The real assertion: re-analysing the fixed buffer is clean.
    let main = Path::new("src").join("main.dz");
    let overlay = |p: &Path| p.ends_with(&main).then(|| fixed.clone());
    let after = analyse_with(&root(), &overlay, Mode::Editor);
    let codes: Vec<&str> = after.diags.iter().filter_map(|d| d.code).collect();
    assert!(
        !codes.contains(&dz_diag::codes::NON_EXHAUSTIVE_SWITCH),
        "the fix left the switch non-exhaustive: {codes:?}"
    );
    assert!(!after.diags.has_errors(), "{}", dz_diag::render_to_string(&after.map, &after.diags));
}

/// A union variant carrying a payload needs a binding, and the arm written for it uses `_`
/// so the fix does not trade one diagnostic for an unused-variable warning.
#[test]
fn a_fixed_union_switch_binds_its_payload_with_a_wildcard() {
    let source = "union Shot {\n    Hit { at: i32 }\n    Miss {}\n}\n\n\
                  fn score(s: &Shot) {\n    switch s {\n    }\n}\n";
    let (a, file, text) = with_appended(source);

    let switch = text.find("switch s").expect("the switch");
    let fixes = ide::code_actions(&a, file, Span::new(switch, switch));
    let fix = fixes.first().expect("a fix");
    let fixed = apply(&text, fix);
    let tail = &fixed[fixed.find("union Shot").unwrap()..];
    assert!(fixed.contains("case Hit(_) {}"), "payload arm:\n{tail}");
    assert!(fixed.contains("case Miss {}"), "payload-less arm:\n{tail}");

    let main = Path::new("src").join("main.dz");
    let overlay = |p: &Path| p.ends_with(&main).then(|| fixed.clone());
    let after = analyse_with(&root(), &overlay, Mode::Editor);
    assert!(!after.diags.has_errors(), "{}", dz_diag::render_to_string(&after.map, &after.diags));
}

/// An `impl` missing a method offers to write it, with the signature the trait asks for —
/// one fix per missing method, so they can be taken one at a time.
#[test]
fn an_incomplete_impl_offers_each_missing_method() {
    let source = "trait Greet {\n    fn hello(&self)\n    fn loudly(&self, times: i32)\n}\n\n\
                  struct Host {\n    pub n: i32 = 0\n}\n\n\
                  impl Greet for Host {\n}\n";
    let (a, file, text) = with_appended(source);

    let at = text.find("Greet for Host").expect("the trait name");
    let fixes = ide::code_actions(&a, file, Span::new(at, at));
    let titles: Vec<&str> = fixes.iter().map(|f| f.title.as_str()).collect();
    assert_eq!(titles, ["implement `hello`", "implement `loudly`"], "one fix per method");

    // Applying one writes that method with the declared signature, indented inside the
    // `impl`, and leaves the other still missing.
    let fixed = apply(&text, &fixes[1]);
    let tail = &fixed[fixed.find("impl Greet").unwrap()..];
    assert!(tail.contains("\n    fn loudly(&self, times: i32) {\n"), "{tail}");
    assert!(tail.contains("\n        panic!(\"not implemented\")\n"), "{tail}");

    let main = Path::new("src").join("main.dz");
    let overlay = |p: &Path| p.ends_with(&main).then(|| fixed.clone());
    let after = analyse_with(&root(), &overlay, Mode::Editor);
    let codes: Vec<&str> = after.diags.iter().filter_map(|d| d.code).collect();
    // `loudly` is implemented now; `hello` is still reported, and nothing else broke — in
    // particular the stub is not a syntax error and its signature matches the trait's.
    assert_eq!(
        codes.iter().filter(|c| **c == dz_diag::codes::MISSING_TRAIT_MEMBER).count(),
        1,
        "{codes:?}"
    );
    assert!(!codes.contains(&dz_diag::codes::TRAIT_MEMBER_MISMATCH), "{codes:?}");
}

/// A stub for a method that returns something leaves `DZ0405` behind: `panic` is not one of
/// the forms that satisfy definite return (§10.5), so the fix scaffolds the method and the
/// compiler goes on saying it is unfinished. Pinned because it is a deliberate choice —
/// the alternative, returning a default, compiles and is silently wrong.
#[test]
fn a_stub_for_a_returning_method_still_asks_to_be_written() {
    let source = "trait Measure {\n    fn size(&self) -> i32\n}\n\n\
                  struct Crate2 {\n    pub n: i32 = 0\n}\n\n\
                  impl Measure for Crate2 {\n}\n";
    let (a, file, text) = with_appended(source);
    let at = text.find("Measure for Crate2").expect("the trait name");
    let fixes = ide::code_actions(&a, file, Span::new(at, at));
    let fixed = apply(&text, fixes.first().expect("a fix"));
    assert!(fixed.contains("fn size(&self) -> i32 {"), "{fixed}");

    let main = Path::new("src").join("main.dz");
    let overlay = |p: &Path| p.ends_with(&main).then(|| fixed.clone());
    let after = analyse_with(&root(), &overlay, Mode::Editor);
    let codes: Vec<&str> = after.diags.iter().filter_map(|d| d.code).collect();
    assert!(!codes.contains(&dz_diag::codes::MISSING_TRAIT_MEMBER), "{codes:?}");
    assert!(codes.contains(&dz_diag::codes::MISSING_RETURN), "{codes:?}");
}

/// A cursor nowhere near a fixable diagnostic is offered nothing.
#[test]
fn code_actions_are_offered_only_where_the_diagnostic_is() {
    let source = "enum Tone {\n    Warm\n    Cool\n}\n\n\
                  fn tint(t: Tone) {\n    switch t {\n    }\n}\n";
    let (a, file, text) = with_appended(source);
    assert!(ide::code_actions(&a, file, Span::new(0, 0)).is_empty(), "offered at the file start");
    // On the `switch` itself, it is offered.
    let switch = text.find("switch t").expect("the switch");
    assert_eq!(ide::code_actions(&a, file, Span::new(switch, switch)).len(), 1);
}

/// Ctrl+click on a recipe's header opens the file the include search found — and so does
/// the `cpp = "…"` naming the recipe (spec §15.1, §17.4).
#[test]
fn a_manifest_links_a_recipe_header_to_the_header() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/cinterop");
    let a = analyse(&root);
    if a.diags.has_errors() {
        eprintln!("skipping: the fixture needs libclang");
        return;
    }
    let links = ide::manifest_links(&a, &root.join("dazzle.toml"));
    let text = std::fs::read_to_string(root.join("dazzle.toml")).unwrap();
    for written in ["\"mini.h\"", "\"mini\""] {
        let (_, _, target, _) = links
            .iter()
            .find(|(_, span, _, _)| &text[span.range()] == written)
            .unwrap_or_else(|| panic!("no link on {written}"));
        assert!(target.ends_with("mini.h") && target.is_file(), "{written} → {target:?}");
    }
}
