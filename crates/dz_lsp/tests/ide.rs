//! Editor queries against a real program: `tests/run/generics`, which has generics, traits,
//! `impl` blocks, default methods and bare field access — everything a name can mean.

use std::path::{Path, PathBuf};

use dz_driver::{analyse, analyse_with, Analysis, Mode};
use dz_lex::TokenKind;
use dz_lsp::ide::{self, CompletionKind, DeclKind};
use dz_span::FileId;

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
    let body = src.find("struct Buffer<T>").unwrap() as u32..src.find("impl<T: Copy>").unwrap() as u32;
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
    let (a, f, text) = edited(typed_above("println(\"done\")", "let broken = ("));
    assert!(!a.parsed_cleanly);
    assert!(
        a.diags.iter().all(|d| d.code.is_some_and(|c| c.starts_with("DZ01") || c.starts_with("DZ02"))),
        "only syntax errors are shown while there is one"
    );
    let hover = ide::info_at(&a, f, offset_in(&text, "largest(3, 9)", 2), ).unwrap().hover.unwrap();
    assert!(hover.contains("fn largest"), "{hover}");
    let copied = ide::info_at(&a, f, offset_in(&text, "copied.len()", 1)).unwrap().hover.unwrap();
    assert!(copied.contains("Buffer<i32>"), "{copied}");
}

// --- references and rename -----------------------------------------------------------------

/// How many times `name` is written as a name (not in a string or comment) in the fixture.
fn written(a: &Analysis, f: FileId, name: &str) -> usize {
    let text = a.map.get(f).text();
    dz_lex::lex(f, text).tokens.iter().filter(|t| t.kind == TokenKind::Ident && t.text(text) == name).count()
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
    let offset = at(&a, f, "println(\"done\")", 0) as usize;
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

#[test]
fn completion_after_a_dot_offers_fields_not_keywords() {
    let got = complete_after("println(\"pair", "p.");
    assert!(got.iter().any(|l| l == "first") && got.iter().any(|l| l == "second"), "{got:?}");
    assert!(!got.iter().any(|l| l == "let"), "no keywords after a dot");
}

#[test]
fn completion_after_a_dot_offers_methods_but_not_private_fields() {
    let got = complete_after("println(\"ints: len", "ints.");
    for want in ["push", "len", "at"] {
        assert!(got.iter().any(|l| l == want), "{want} missing from {got:?}");
    }
    assert!(!got.iter().any(|l| l == "m_data"), "private fields stay private outside the struct");
}

#[test]
fn completion_types_a_call_as_the_receiver() {
    let got = complete_after("println(\"done", "largest(Score(12), Score(4)).");
    for want in ["value", "cmp", "eq"] {
        assert!(got.iter().any(|l| l == want), "{want} missing from {got:?}");
    }
    let got = complete_after("println(\"done", "Countdown(3).");
    for want in ["valid", "get", "advance"] {
        assert!(got.iter().any(|l| l == want), "{want} missing from {got:?}");
    }
}

#[test]
fn completion_by_name_works_from_an_older_analysis() {
    // The analysis has not seen the edit: the receiver is found by name instead.
    let (a, f) = generics();
    let text = a.map.get(f).text();
    let i = text.find("println(\"pair").unwrap();
    let edit = format!("{}p.\n    {}", &text[..i], &text[i..]);
    let got = ide::complete(None, Some((&a, f)), &edit, i + 2);
    assert!(labels(&got).contains(&"first"), "{:?}", labels(&got));
}

#[test]
fn completion_after_type_colons_offers_type_queries() {
    let got = complete_after("println(\"done", "i32::");
    assert!(got.iter().any(|l| l == "is_trivially_copyable"));
}

#[test]
fn completion_is_quiet_inside_comments_and_strings() {
    let (a, f) = generics();
    assert!(ide::complete(Some((&a, f)), None, "// let", 6).is_empty());
    assert!(ide::complete(Some((&a, f)), None, "println(\"le", 11).is_empty());
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

#[test]
fn inlay_hints_name_the_fields_of_a_positional_literal() {
    let (a, f, text) = edited(typed_above("ints.push(10)", "let hinted = Vec2{ 1.5, 2.5 }"));
    let hints = ide::inlay_hints(&a, f, dz_span::Span::new(0, text.len()));
    let field_at = |needle: &str, within: usize| {
        let at = offset_in(&text, needle, within);
        hints.iter().find(|h| h.offset == at && h.kind == ide::HintKind::Field).map(|h| h.label.clone())
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

#[test]
fn formatting_refuses_broken_source_and_is_idempotent() {
    let path = Path::new("t.dz");
    assert!(ide::format(&ide::parse(path, "fn main( {")).is_none());
    let (a, f) = generics();
    let once = ide::format(&ide::parse(path, a.map.get(f).text())).unwrap();
    let twice = ide::format(&ide::parse(path, &once)).unwrap();
    assert_eq!(once, twice);
}
