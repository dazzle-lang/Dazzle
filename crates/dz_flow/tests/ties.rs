//! The effect model consumes real checked signatures, not hand-built type tables.
//! Valid and invalid effect comparisons are paired below; enforcement is deliberately
//! not enabled until std's clauses and the fixes are ready (T-0107 steps 2d/2e).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use dz_check::ties::Scope;
use dz_check::{CheckResult, ItemSig};
use dz_diag::Diagnostics;
use dz_flow::ties::{self, Bindings, Mismatch, ParameterTie, Place, ReturnTie, Step};
use dz_hir::LocalId;
use dz_span::SourceMap;

struct Fixture {
    checked: CheckResult,
    res: dz_resolve::Resolution,
}

impl Fixture {
    fn new() -> Self {
        Self::with_source("")
    }

    fn with_source(extra: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "dz_flow_ties_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("dazzle.toml"),
            "[module]\nname = \"ties\"\nsources = [\"src/**/*.dz\"]\n\n\
             [dependencies]\nstd = { open = true }\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/main.dz"), format!("{SOURCE}\n{extra}\n")).unwrap();
        let mut map = SourceMap::new();
        let mut diags = Diagnostics::new();
        let ws = dz_project::load(&dir, &mut map, &mut diags).unwrap();
        let mut files = HashMap::new();
        for module in ws.modules() {
            let mut parsed = Vec::new();
            for path in &module.sources {
                let file = map.add_bytes(path, &module.read_source(path).unwrap()).unwrap();
                let text = map.get(file).text();
                let lexed = dz_lex::lex(file, text);
                let p = dz_parse::parse(file, text, &lexed.tokens);
                diags.extend(lexed.diagnostics);
                diags.extend(p.diagnostics);
                parsed.push(dz_resolve::ParsedFile { file, ast: p.ast });
            }
            files.insert(module.id, parsed);
        }
        let res = dz_resolve::resolve(&ws, &files);
        diags.extend(res.diagnostics.clone());
        let checked = dz_check::check(&ws, &res, &files, &map);
        diags.extend(checked.diagnostics.clone());
        let rendered = dz_diag::render_to_string(&map, &diags);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!diags.has_errors(), "{rendered}");
        Self { checked, res }
    }

    fn scope<'a>(&'a self, name: &'a str) -> Scope<'a> {
        let (index, sym) =
            self.res.symbols.iter().enumerate().find(|(_, s)| s.name == name).unwrap();
        let Some(ItemSig::Fn(sig)) = self.checked.table.get(dz_resolve::SymbolId(index as u32))
        else {
            panic!("{name} should be a function")
        };
        Scope {
            table: &self.checked.table,
            res: &self.res,
            module: sym.module,
            name,
            sig,
            self_ty: None,
            privileged: None,
        }
    }

    fn effects(&self, name: &str, bindings: &Bindings) -> ties::CallEffects {
        ties::call_effects(&self.scope(name), bindings).unwrap_or_else(|e| panic!("{}", e.message))
    }

    fn analysis(&self, name: &str) -> ties::BodyAnalysis {
        let index = self.res.symbols.iter().position(|s| s.name == name).unwrap();
        let owner = dz_hir::BodyOwner::Fn(dz_resolve::SymbolId(index as u32));
        let body = self.checked.hir.find(&owner).unwrap();
        ties::analyse_body(body, &self.checked.table, &self.res)
    }
}

const SOURCE: &str = r#"
pub struct Text {
    pub data: ^mut u8 in @chars = null
    pub fn examine(&self, n: i32) {}
}
pub struct Bag<T> {
    pub data: ^mut T in @items = null
    pub values: [2]T
}
impl<T> Index<usize> for Bag<T> {
    type Output = T
    fn index(&self, i: usize) -> &T in self@items { return unsafe { &data[i] } }
}
impl<T> IndexMut<usize> for Bag<T> {
    type Output = T
    fn index_mut(&mut self, i: usize) -> &mut T in self@items { return unsafe { &mut data[i] } }
}
pub struct World {
    pub bag: Bag<Text> in @bag
    pub fn first(&self) -> ^Text in self@bag@items { return bag.data }
    pub fn elided(&self) -> ^Text { return bag.data }
    fn private_paths(&mut self) kills self@bag@items, self.bag@items {}
}
fn grow(xs: &mut Bag<Text>) kills xs@items {}
fn clear(xs: &mut Bag<Text>) kills xs@items, xs[*] {}
fn nested(w: &mut World) kills w@bag@items, w.bag[*]@chars {}
fn view(w: &World) -> ^Text in w@bag@items { return w.bag.data }
fn broad_view(w: &World) -> ^Text in w@bag { return w.bag.data }
fn tied(e: &mut Text in w.bag[*], w: &mut World) kills w.bag@items {}
fn identity(x: &Text) -> &Text { return x }
fn ambiguous(x: &Text, y: &Text) -> &Text { return x }
fn literal() -> str { return "static" }
fn read(w: &World) -> i32 { return 1 }
fn foreign(a: &mut World, b: ^mut World, c: &World, d: ^World, v: World) {}
fn arrays(xs: &mut [2]Text) kills xs[*]@chars {}
fn slices(xs: &mut []mut Text) kills xs[*]@chars {}
fn iterable(xs: &mut HashMap<i32, i32>) kills xs[*] {}
fn observe(p: ^Text) {}
fn observe_byte(p: ^u8) {}
"#;

fn place(local: u32, steps: &[Step]) -> Place {
    Place { local: LocalId(local), steps: steps.to_vec() }
}

fn field(name: &str) -> Step {
    Step::Field(name.into())
}

fn args(params: Vec<Option<Place>>) -> Bindings {
    Bindings { receiver: None, params }
}

#[test]
fn effects_substitute_arguments_and_keep_elements_separate_from_storage() {
    let fixture = Fixture::new();
    let bag = place(7, &[field("world"), field("bag")]);
    let bindings = args(vec![Some(bag.clone())]);
    let grow = fixture.effects("grow", &bindings);
    let storage = bag.followed(&[field("data"), Step::Storage]);
    let element_text = bag.followed(&[Step::Elements, field("data"), Step::Storage]);
    assert_eq!(grow.kills.len(), 1);
    assert_eq!(grow.kills[0].place, storage);
    assert!(grow.kills[0].clause.is_some());
    assert!(!grow.kills[0].place.covers(&element_text));
    assert!(!grow.incomplete);

    let clear = fixture.effects("clear", &bindings);
    assert_eq!(clear.kills.len(), 2);
    assert!(clear.kills[1].place.covers(&element_text));
    assert!(!clear.kills[1].place.covers(&storage));
    assert!(bag.covers(&element_text));
    assert!(bag.covers(&storage));

    let world = place(9, &[field("scene")]);
    let nested = fixture.effects("nested", &args(vec![Some(world.clone())]));
    assert_eq!(
        nested.kills[0].place,
        world.followed(&[field("bag"), field("data"), Step::Storage])
    );
    assert_eq!(
        nested.kills[1].place,
        world.followed(&[field("bag"), Step::Elements, field("data"), Step::Storage])
    );
}

#[test]
fn explicit_and_elided_return_ties_preserve_what_can_end_them() {
    let fixture = Fixture::new();
    let world = Place::local(LocalId(4));
    let bindings = args(vec![Some(world.clone())]);
    let storage = world.followed(&[field("bag"), field("data"), Step::Storage]);
    let text = world.followed(&[field("bag"), Step::Elements, field("data"), Step::Storage]);
    let view = fixture.effects("view", &bindings);
    assert_eq!(view.result, ReturnTie::Explicit(storage.clone()));
    assert!(view.result.ended_by(&storage));
    assert!(view.result.ended_by(&world));
    assert!(!view.result.ended_by(&text));
    assert!(!view.result.ended_by(&Place::local(LocalId(5))));

    let broad = fixture.effects("broad_view", &bindings);
    assert!(broad.result.ended_by(&storage));
    assert!(broad.result.ended_by(&text));
    let identity = fixture.effects("identity", &bindings);
    assert_eq!(identity.result, ReturnTie::Elided(world.clone()));
    assert!(identity.result.ended_by(&storage));
    let read = fixture.effects("read", &bindings);
    assert_eq!(read.result, ReturnTie::None);
    assert!(read.kills.is_empty());
    assert!(!read.incomplete);
    let ambiguous = fixture.effects("ambiguous", &args(vec![Some(world.clone()), Some(world)]));
    assert_eq!(ambiguous.result, ReturnTie::Unknown);
    assert!(!ambiguous.incomplete);
    assert_eq!(fixture.effects("literal", &Bindings::default()).result, ReturnTie::Unknown);
}

#[test]
fn parameter_contracts_do_not_replace_independent_argument_origins() {
    let fixture = Fixture::new();
    let boss = Place::local(LocalId(1));
    let world = Place::local(LocalId(2));
    let bindings = args(vec![Some(boss.clone()), Some(world.clone())]);
    let effects = fixture.effects("tied", &bindings);
    assert_eq!(
        effects.parameter_ties,
        vec![
            ParameterTie::Known(world.followed(&[field("bag"), Step::Elements])),
            ParameterTie::Unspecified
        ]
    );
    assert_eq!(
        effects.kills[0].place,
        world.followed(&[field("bag"), field("data"), Step::Storage])
    );
    assert_eq!(bindings.params[0], Some(boss));
    assert!(!effects.incomplete);
}

#[test]
fn arrays_slices_and_iterable_only_types_use_the_same_element_step() {
    let fixture = Fixture::new();
    let xs = Place::local(LocalId(3));
    let bindings = args(vec![Some(xs.clone())]);
    for name in ["arrays", "slices"] {
        let effects = fixture.effects(name, &bindings);
        assert_eq!(
            effects.kills[0].place,
            xs.followed(&[Step::Elements, field("data"), Step::Storage])
        );
        assert!(!effects.incomplete);
    }
    let effects = fixture.effects("iterable", &bindings);
    assert_eq!(effects.kills[0].place, xs.followed(&[Step::Elements]));
    assert!(!effects.incomplete);
}

#[test]
fn receiver_paths_and_public_tie_aliases_normalize_like_fields() {
    let fixture = Fixture::new();
    let (index, sym) =
        fixture.res.symbols.iter().enumerate().find(|(_, s)| s.name == "World").unwrap();
    let owner = dz_resolve::SymbolId(index as u32);
    let methods = fixture.checked.table.methods_sig(owner).unwrap();
    let receiver = place(8, &[field("state")]);
    let bindings = Bindings { receiver: Some(receiver.clone()), params: vec![] };
    let mut scope = Scope {
        table: &fixture.checked.table,
        res: &fixture.res,
        module: sym.module,
        name: "private_paths",
        sig: &methods.methods["private_paths"],
        self_ty: Some(dz_hir::ty::Ty::plain(owner)),
        privileged: Some(owner),
    };
    let effects = ties::call_effects(&scope, &bindings).unwrap_or_else(|e| panic!("{}", e.message));
    assert_eq!(effects.kills[0].place, effects.kills[1].place);
    assert_eq!(
        ties::minimal(&effects.kills.iter().map(|k| k.place.clone()).collect::<Vec<_>>()).len(),
        1
    );
    scope.name = "first";
    scope.sig = &methods.methods["first"];
    let effects = ties::call_effects(&scope, &bindings).unwrap_or_else(|e| panic!("{}", e.message));
    assert_eq!(
        effects.result,
        ReturnTie::Explicit(receiver.followed(&[field("bag"), field("data"), Step::Storage]))
    );
    scope.name = "elided";
    scope.sig = &methods.methods["elided"];
    let effects = ties::call_effects(&scope, &bindings).unwrap_or_else(|e| panic!("{}", e.message));
    assert_eq!(effects.result, ReturnTie::Elided(receiver));
}

#[test]
fn foreign_effects_only_kill_through_mutable_inputs_and_keep_unknowns_explicit() {
    let fixture = Fixture::new();
    let mut scope = fixture.scope("foreign");
    let mut sig = scope.sig.clone();
    sig.kills_unknown = true;
    scope.sig = &sig;
    let bindings = args((0..5).map(|i| Some(Place::local(LocalId(i)))).collect());
    let effects = ties::call_effects(&scope, &bindings).unwrap_or_else(|e| panic!("{}", e.message));
    assert_eq!(
        effects.kills.iter().map(|k| k.place.clone()).collect::<Vec<_>>(),
        vec![Place::local(LocalId(0)), Place::local(LocalId(1))]
    );
    assert!(effects.kills.iter().all(|k| k.clause.is_none()));
    assert!(!effects.incomplete);
    let unknown = args(vec![None, bindings.params[1].clone()]);
    let effects = ties::call_effects(&scope, &unknown).unwrap_or_else(|e| panic!("{}", e.message));
    assert!(effects.incomplete);
    assert_eq!(effects.kills.len(), 1);
    let unknown = fixture.effects("grow", &Bindings::default());
    assert!(unknown.incomplete);
    assert!(unknown.kills.is_empty());
    let view = fixture.effects("view", &Bindings::default());
    assert_eq!(view.result, ReturnTie::Unknown);
    assert!(!view.incomplete, "an unknown result does not make a kill-free call invalidate memory");
    let tied = fixture.effects("tied", &Bindings::default());
    assert!(tied.incomplete);
    assert_eq!(tied.parameter_ties[0], ParameterTie::Unknown);

    let mut sig = sig.clone();
    sig.takes_self = Some(true);
    scope.sig = &sig;
    let receiver = Place::local(LocalId(12));
    let bindings = Bindings { receiver: Some(receiver.clone()), ..bindings };
    let effects = ties::call_effects(&scope, &bindings).unwrap_or_else(|e| panic!("{}", e.message));
    assert_eq!(effects.kills[0].place, receiver);
    let mut sig = sig.clone();
    sig.takes_self = Some(false);
    scope.sig = &sig;
    let effects = ties::call_effects(&scope, &bindings).unwrap_or_else(|e| panic!("{}", e.message));
    assert_eq!(effects.kills.len(), 2);
}

#[test]
fn malformed_incremental_signatures_return_the_checkers_error() {
    let fixture = Fixture::new();
    let mut scope = fixture.scope("grow");
    let mut sig = scope.sig.clone();
    sig.kills[0].root.name = "missing".into();
    scope.sig = &sig;
    let Err(error) = ties::call_effects(&scope, &Bindings::default()) else {
        panic!("a broken path must not silently become a call with no effects")
    };
    assert_eq!(error.code, dz_diag::codes::TIE_PATH_UNRESOLVED);
}

#[test]
fn exact_clauses_have_missing_unused_and_broad_cases_with_clean_counterparts() {
    let world = Place::local(LocalId(0));
    let items = world.followed(&[field("items")]);
    let other = world.followed(&[field("other")]);
    assert!(ties::compare(std::slice::from_ref(&items), std::slice::from_ref(&items)).is_empty());
    assert!(ties::compare(&[], &[]).is_empty());
    assert_eq!(
        ties::compare(&[], std::slice::from_ref(&items)),
        vec![Mismatch::Missing { actual: 0 }]
    );
    assert_eq!(
        ties::compare(std::slice::from_ref(&items), &[]),
        vec![Mismatch::Unused { declared: 0 }]
    );
    assert_eq!(
        ties::compare(std::slice::from_ref(&world), &[items.clone(), other.clone()]),
        vec![Mismatch::TooBroad { declared: 0, actual: vec![0, 1] }]
    );
    assert_eq!(
        ties::compare(std::slice::from_ref(&items), std::slice::from_ref(&world)),
        vec![Mismatch::Missing { actual: 0 }]
    );
    assert!(ties::compare(&[items.clone(), other.clone()], &[items, other]).is_empty());
}

#[test]
fn minimal_effects_do_not_merge_siblings_or_confuse_different_locals() {
    let world = Place::local(LocalId(0));
    let a = world.followed(&[field("a")]);
    let b = world.followed(&[field("b")]);
    let other = Place::local(LocalId(1));
    assert_eq!(ties::minimal(&[a.clone(), b.clone(), a.clone()]), vec![a.clone(), b]);
    assert_eq!(
        ties::minimal(&[a.clone(), world.clone(), a, other.clone()]),
        vec![world.clone(), other]
    );
    assert!(!world.followed(&[Step::Elements]).covers(&world));
}

trait Followed {
    fn followed(&self, steps: &[Step]) -> Self;
}

fn clean(analysis: &ties::BodyAnalysis) {
    assert!(analysis.invalid_uses.is_empty(), "unexpected invalid use: {analysis:#?}");
    assert!(analysis.invalid_returns.is_empty(), "unexpected returned tie: {analysis:#?}");
    assert!(analysis.incomplete.is_empty(), "analysis is incomplete: {analysis:#?}");
}

#[test]
fn body_calls_kill_old_views_but_new_views_start_fresh() {
    let fixture = Fixture::with_source(
        r#"
fn old(w: &mut World) kills w.bag@items {
    let p = w.first()
    grow(&mut w.bag)
    observe(p)
}
fn fresh(w: &mut World) kills w.bag@items {
    let p = w.first()
    observe(p)
    grow(&mut w.bag)
    let q = w.first()
    observe(q)
}
fn retarget(w: &mut World) kills w.bag@items {
    let mut p = w.first()
    grow(&mut w.bag)
    p = w.first()
    observe(p)
}
"#,
    );
    let old = fixture.analysis("old");
    assert_eq!(old.invalid_uses.len(), 1, "{old:#?}");
    assert_eq!(old.invalid_uses[0].killed.kind, ties::KillKind::Call);
    assert!(old.incomplete.is_empty(), "{old:#?}");
    assert!(old.mismatches.is_empty(), "{old:#?}");
    clean(&fixture.analysis("fresh"));
    clean(&fixture.analysis("retarget"));
}

#[test]
fn body_indexing_separates_slot_addresses_from_element_allocations() {
    let fixture = Fixture::with_source(
        r#"
fn slot(w: &mut World) kills w.bag@items {
    let element = &w.bag[0]
    grow(&mut w.bag)
    observe(element)
}
fn element_text(w: &mut World) kills w.bag@items {
    let element = &w.bag[0]
    let text = element.data
    grow(&mut w.bag)
    observe_byte(text)
}
fn cleared_text(w: &mut World) kills w.bag@items, w.bag[*] {
    let text = w.bag[0].data
    clear(&mut w.bag)
    observe_byte(text)
}
"#,
    );
    let slot = fixture.analysis("slot");
    assert_eq!(slot.invalid_uses.len(), 1, "{slot:#?}");
    assert!(slot.incomplete.is_empty(), "{slot:#?}");
    clean(&fixture.analysis("element_text"));
    let cleared = fixture.analysis("cleared_text");
    assert_eq!(cleared.invalid_uses.len(), 1, "{cleared:#?}");
    assert!(cleared.incomplete.is_empty(), "{cleared:#?}");
}

#[test]
fn branches_merge_possible_origins_without_reviving_killed_views() {
    let fixture = Fixture::with_source(
        r#"
fn maybe(w: &mut World, flag: bool) kills w.bag@items {
    let p = w.first()
    if flag { grow(&mut w.bag) }
    observe(p)
}
fn early(w: &mut World, flag: bool) kills w.bag@items {
    let p = w.first()
    if flag { grow(&mut w.bag); return }
    observe(p)
}
fn either(a: &mut World, b: &mut World, flag: bool) kills b.bag@items {
    let mut p = a.first()
    if flag { p = b.first() }
    grow(&mut b.bag)
    observe(p)
}
fn branch_fresh(w: &mut World, flag: bool) kills w.bag@items {
    let mut p = w.first()
    if flag { grow(&mut w.bag); p = w.first() }
    observe(p)
}
"#,
    );
    for name in ["maybe", "either"] {
        let analysis = fixture.analysis(name);
        assert_eq!(analysis.invalid_uses.len(), 1, "{name}: {analysis:#?}");
        assert!(analysis.incomplete.is_empty(), "{analysis:#?}");
    }
    clean(&fixture.analysis("early"));
    clean(&fixture.analysis("branch_fresh"));
}

#[test]
fn scope_exit_moves_and_replacement_end_views_but_scalar_writes_allow_aliases() {
    let fixture = Fixture::with_source(
        r#"
fn scoped() {
    let mut p: ^Text = null
    { let x = Text{}; p = &x }
    observe(p)
}
fn moved() {
    let x = Text{}
    let p = &x
    let y = move(x)
    observe(p)
}
fn replaced(w: &mut World) kills w.bag {
    let p = w.first()
    w.bag = Bag<Text>{ values: [Text{}, Text{}] }
    observe(p)
}
fn scalar(x: &mut i32) {
    let a = x
    let b = x
    a += 1
    b += 1
}
fn moving_a_reference(x: &Text) {
    let p = x
    let q = move(p)
    observe(q)
    observe(x)
}
"#,
    );
    for (name, kind) in [
        ("scoped", ties::KillKind::Drop),
        ("moved", ties::KillKind::Move),
        ("replaced", ties::KillKind::Assign),
    ] {
        let analysis = fixture.analysis(name);
        assert_eq!(analysis.invalid_uses.len(), 1, "{name}: {analysis:#?}");
        assert_eq!(analysis.invalid_uses[0].killed.kind, kind);
        assert!(analysis.incomplete.is_empty(), "{analysis:#?}");
    }
    clean(&fixture.analysis("scalar"));
    clean(&fixture.analysis("moving_a_reference"));
}

#[test]
fn body_summaries_compare_only_effects_visible_to_the_caller() {
    let fixture = Fixture::with_source(
        r#"
fn missing(w: &mut World) { grow(&mut w.bag) }
fn unused(w: &mut World) kills w.bag@items { let _ = w.bag.data }
fn wide(w: &mut World) kills w { grow(&mut w.bag) }
fn exact(w: &mut World) kills w.bag@items { grow(&mut w.bag) }
fn own_local() { let mut w = World{ bag: Bag<Text>{ values: [Text{}, Text{}] } }; grow(&mut w.bag) }
"#,
    );
    let missing = fixture.analysis("missing");
    assert_eq!(missing.mismatches, vec![Mismatch::Missing { actual: 0 }], "{missing:#?}");
    let unused = fixture.analysis("unused");
    assert_eq!(unused.mismatches, vec![Mismatch::Unused { declared: 0 }], "{unused:#?}");
    let wide = fixture.analysis("wide");
    assert_eq!(
        wide.mismatches,
        vec![Mismatch::TooBroad { declared: 0, actual: vec![0] }],
        "{wide:#?}"
    );
    let exact = fixture.analysis("exact");
    assert!(exact.mismatches.is_empty(), "{exact:#?}");
    let local = fixture.analysis("own_local");
    assert!(local.actual.is_empty(), "{local:#?}");
    assert!(local.mismatches.is_empty(), "{local:#?}");
    for a in [missing, unused, wide, exact, local] {
        clean(&a);
    }
}

#[test]
fn loops_find_kills_from_previous_iterations_and_ignore_dead_code() {
    let fixture = Fixture::with_source(
        r#"
fn looped(w: &mut World, flag: bool) kills w.bag@items {
    let p = w.first()
    while flag { observe(p); grow(&mut w.bag) }
}
fn loop_fresh(w: &mut World, flag: bool) kills w.bag@items {
    while flag { let p = w.first(); observe(p); grow(&mut w.bag) }
}
fn no_backedge(w: &mut World, flag: bool) kills w.bag@items {
    let p = w.first()
    while flag { observe(p); grow(&mut w.bag); break }
}
fn after_break(w: &mut World, flag: bool) kills w.bag@items {
    let p = w.first()
    while flag { grow(&mut w.bag); break }
    observe(p)
}
fn unreachable_kill(w: &mut World) {
    return
    grow(&mut w.bag)
}
"#,
    );
    for name in ["looped", "after_break"] {
        let analysis = fixture.analysis(name);
        assert_eq!(analysis.invalid_uses.len(), 1, "{name}: {analysis:#?}");
        assert!(analysis.incomplete.is_empty(), "{analysis:#?}");
    }
    clean(&fixture.analysis("loop_fresh"));
    clean(&fixture.analysis("no_backedge"));
    let dead = fixture.analysis("unreachable_kill");
    clean(&dead);
    assert!(dead.actual.is_empty(), "{dead:#?}");
}

#[test]
fn unresolved_indirect_effects_cannot_suggest_removing_a_clause() {
    let fixture = Fixture::with_source(
        r#"
fn opaque(f: fn(&mut World), w: &mut World) kills w {
    let p = w.first()
    f(w)
    observe(p)
}
"#,
    );
    let analysis = fixture.analysis("opaque");
    assert!(!analysis.incomplete.is_empty(), "{analysis:#?}");
    assert!(analysis.mismatches.is_empty(), "{analysis:#?}");
    assert_eq!(analysis.invalid_uses.len(), 1, "{analysis:#?}");
}

#[test]
fn indirect_calls_with_nothing_mutable_kill_nothing_whatever_their_clause() {
    // A clause names only `&mut` and `^mut` parameters (DZ0564), so a function type
    // with none of them can kill nothing, carried clause or not.
    let fixture = Fixture::with_source(
        r#"
fn reads(f: fn(&World, i32) -> i32, w: &mut World) {
    let p = w.first()
    f(w, 1)
    observe(p)
}
fn reads_with_clause(f: fn(&World) -> i32, w: &mut World) kills w { f(w) }
"#,
    );
    clean(&fixture.analysis("reads"));
    let a = fixture.analysis("reads_with_clause");
    assert!(a.incomplete.is_empty(), "{a:#?}");
    assert!(matches!(a.mismatches[..], [Mismatch::Unused { declared: 0 }]), "{a:#?}");
}

#[test]
fn initialisers_and_defaults_have_no_effect_on_the_callers_places() {
    let fixture = Fixture::with_source(
        r#"
pub struct Settings { pub n: i32 = 3 }
const LIMIT: i32 = 4
fn defaulted_view(w: &mut World, p: ^Text = null, n: i32 = 0) kills w.bag@items {}
fn passes_default(w: &mut World) kills w.bag@items {
    let p = w.first()
    defaulted_view(w)
    observe(p)
}
"#,
    );
    // An initialiser is complete: it has no summary to doubt.
    let mut initialisers = 0;
    for (_, body) in fixture.checked.hir.bodies() {
        if matches!(
            body.owner,
            dz_hir::BodyOwner::Const(_) | dz_hir::BodyOwner::FieldDefault { .. }
        ) {
            let a = ties::analyse_body(body, &fixture.checked.table, &fixture.res);
            assert!(a.incomplete.is_empty(), "{a:#?}");
            initialisers += 1;
        }
    }
    assert!(initialisers >= 2, "LIMIT and Settings::n at least");
    let a = fixture.analysis("passes_default");
    assert_eq!(a.invalid_uses.len(), 1, "{a:#?}");
    assert!(a.incomplete.is_empty(), "{a:#?}");
    assert!(a.mismatches.is_empty(), "{a:#?}");
}

#[test]
fn calls_preserve_temporary_argument_and_receiver_provenance() {
    let fixture = Fixture::with_source(
        r#"
fn consume_arg(p: ^Text, n: i32) {}
fn grow_number(w: &mut World) -> i32 kills w.bag@items { grow(&mut w.bag); return 0 }
fn argument(w: &mut World) kills w.bag@items { consume_arg(w.first(), grow_number(w)) }
fn receiver(w: &mut World) kills w.bag@items {
    let p = w.first() or return
    p.examine(grow_number(w))
}
fn normal_branch(w: &mut World, flag: bool) kills w.bag@items {
    consume_arg(w.first(), if flag { grow(&mut w.bag); return } else { 0 })
}
"#,
    );
    for name in ["argument", "receiver"] {
        let a = fixture.analysis(name);
        assert_eq!(a.invalid_uses.len(), 1, "{name}: {a:#?}");
        assert!(a.incomplete.is_empty(), "{a:#?}");
    }
    clean(&fixture.analysis("normal_branch"));
}

#[test]
fn whole_input_returns_preserve_slot_dependencies_and_owned_views_follow_targets() {
    let fixture = Fixture::with_source(
        r#"
fn through_identity(w: &mut World) kills w.bag@items {
    let p = identity(&w.bag[0])
    grow(&mut w.bag)
    observe(p)
}
fn returned_element(w: &mut World) kills w.bag@items {
    let element = w.first() or return
    let text = element.data
    grow(&mut w.bag)
    observe_byte(text)
}
"#,
    );
    let a = fixture.analysis("through_identity");
    assert_eq!(a.invalid_uses.len(), 1, "{a:#?}");
    assert!(a.incomplete.is_empty(), "{a:#?}");
    clean(&fixture.analysis("returned_element"));
}

#[test]
fn defers_and_jumps_apply_scope_exit_effects_on_the_correct_edges() {
    let fixture = Fixture::with_source(
        r#"
fn dropped_at_break(flag: bool) {
    let mut p: ^Text = null
    while flag { let x = Text{}; p = &x; break }
    observe(p)
}
fn dropped_at_continue(flag: bool) {
    let mut p: ^Text = null
    while flag { observe(p); let x = Text{}; p = &x; continue }
}
fn deferred_return(w: &mut World) -> ^Text in w.bag@items kills w.bag@items {
    defer grow(&mut w.bag)
    return w.first()
}
fn two_returns(w: &mut World, flag: bool) -> ^Text in w.bag@items kills w.bag@items {
    let p = w.first()
    defer grow(&mut w.bag)
    if flag { return p }
    return p
}
fn unused_defer_branch(w: &mut World, flag: bool) kills w.bag@items {
    let p = w.first()
    if flag { defer grow(&mut w.bag); return }
    observe(p)
}
"#,
    );
    for name in ["dropped_at_break", "dropped_at_continue", "deferred_return"] {
        let a = fixture.analysis(name);
        assert_eq!(a.invalid_uses.len(), 1, "{name}: {a:#?}");
        assert!(a.incomplete.is_empty(), "{a:#?}");
    }
    let a = fixture.analysis("two_returns");
    assert_eq!(a.invalid_uses.len(), 2, "{a:#?}");
    clean(&fixture.analysis("unused_defer_branch"));
}

#[test]
fn returned_views_cannot_escape_owned_locals_or_value_parameters() {
    let fixture = Fixture::with_source(
        r#"
fn escaping_local() -> ^Text { let x = Text{}; return &x }
fn escaping_parameter(x: Text) -> ^Text { return &x }
fn returned_borrow(x: &Text) -> ^Text { return x }
"#,
    );
    for name in ["escaping_local", "escaping_parameter"] {
        let a = fixture.analysis(name);
        assert_eq!(a.invalid_uses.len(), 1, "{name}: {a:#?}");
    }
    clean(&fixture.analysis("returned_borrow"));
}

#[test]
fn independent_arguments_may_alias_until_a_callee_kills_their_tie() {
    let fixture = Fixture::with_source(
        r#"
fn think(e: &Text, w: &mut World) kills w.bag@items { grow(&mut w.bag); observe(e) }
fn conflict(w: &mut World) kills w.bag@items { think(&w.bag[0], w) }
fn independent(boss: &Text, w: &mut World) kills w.bag@items { think(boss, w) }
fn tied_body(e: &Text in w.bag@items, w: &mut World) kills w.bag@items {
    grow(&mut w.bag)
    observe(e)
}
"#,
    );
    for name in ["conflict", "tied_body"] {
        let a = fixture.analysis(name);
        assert_eq!(a.invalid_uses.len(), 1, "{name}: {a:#?}");
        assert!(a.incomplete.is_empty(), "{a:#?}");
    }
    clean(&fixture.analysis("independent"));
}

#[test]
fn native_for_loops_refresh_elements_but_keep_the_iteration_storage_alive() {
    let fixture = Fixture::with_source(
        r#"
fn for_clean(xs: &mut [2]Text) { for &e in xs { observe(e) } }
fn for_replaced(xs: &mut [2]Text) kills xs {
    for &e in xs { observe(e); xs = [Text{}, Text{}] }
}
fn for_replaced_text(xs: &mut [2]Text) kills xs {
    let mut last: ^u8 = null
    for &e in xs { observe_byte(last); last = e.data; xs = [Text{}, Text{}] }
}
"#,
    );
    clean(&fixture.analysis("for_clean"));
    // Writing the array leaves each slot where it is, so `e` survives; the text the
    // old element owned does not, and the next iteration reads it.
    clean(&fixture.analysis("for_replaced"));
    let a = fixture.analysis("for_replaced_text");
    assert_eq!(a.invalid_uses.len(), 1, "{a:#?}");
    assert!(a.incomplete.is_empty(), "{a:#?}");
}
#[test]
fn returned_ties_are_checked_against_each_possible_source() {
    let fixture = Fixture::with_source(
        r#"
fn wrong_return(a: &Text, b: &Text) -> ^Text in a { return b }
fn right_return(a: &Text, b: &Text) -> ^Text in a { return a }
fn wrapped_identity(w: &World) -> ^Text in w.bag@items { return identity(&w.bag[0]) }
"#,
    );
    let wrong = fixture.analysis("wrong_return");
    assert_eq!(wrong.invalid_returns.len(), 1, "{wrong:#?}");
    assert!(wrong.invalid_uses.is_empty(), "{wrong:#?}");
    assert!(wrong.incomplete.is_empty(), "{wrong:#?}");
    clean(&fixture.analysis("right_return"));
    clean(&fixture.analysis("wrapped_identity"));
}

#[test]
fn every_fixture_and_std_body_can_be_analysed_without_panicking() {
    let fixture = Fixture::new();
    let mut count = 0;
    for (_, body) in fixture.checked.hir.bodies() {
        let a = ties::analyse_body(body, &fixture.checked.table, &fixture.res);
        // std is correct code, so any finding here is the analysis's mistake: returning a
        // local by value once read as a view of it, in 62 bodies of std and the examples.
        assert!(a.invalid_uses.is_empty() && a.invalid_returns.is_empty(), "{a:#?}");
        count += 1;
    }
    assert!(count > 100, "this must exercise std as well as the small fixture");
}

#[test]
fn replacing_an_element_ends_its_owned_memory_but_not_its_slot() {
    let fixture = Fixture::with_source(
        r#"
fn replaced_slot(w: &mut World) kills w.bag[*] {
    let element = &w.bag[0]
    let text = element.data
    w.bag[0] = Text{}
    observe(element)
    observe_byte(text)
}
fn slot_survives(w: &mut World) kills w.bag[*] {
    let element = &w.bag[0]
    w.bag[1] = Text{}
    observe(element)
}
"#,
    );
    let a = fixture.analysis("replaced_slot");
    assert_eq!(a.invalid_uses.len(), 1, "{a:#?}");
    assert!(a.incomplete.is_empty(), "{a:#?}");
    assert!(a.mismatches.is_empty(), "{a:#?}");
    clean(&fixture.analysis("slot_survives"));
}

#[test]
fn writing_plain_data_kills_nothing_but_payloads_and_owned_memory_die() {
    let fixture = Fixture::with_source(
        r#"
pub struct Vec2 { pub x: f32; pub y: f32 }
pub union Shape { Circle { r: f32 }, Square { side: f32 } }
pub struct Body {
    pub at: Vec2
    pub shape: Shape
    pub width: f32?
    pub name: Text
}
fn observe_f32(x: &f32) {}
fn plain(b: &mut Body) {
    let x = &b.at.x
    b.at = Vec2{ x: 1, y: 2 }
    observe_f32(x)
}
fn variant(b: &mut Body) kills b.shape {
    switch b.shape {
        case Circle(&c) {
            b.shape = Shape::Square{ side: 1 }
            observe_f32(&c.r)
        }
        case Square(&s) {}
    }
}
fn optional(b: &mut Body) kills b.width {
    if let &w = b.width {
        b.width = none
        observe_f32(w)
    }
}
fn owned(b: &mut Body) kills b.name {
    let name = &b.name
    let text = b.name.data
    b.name = Text{}
    observe(name)
    observe_byte(text)
}
fn coarse(b: &mut Body) kills b { b.shape = Shape::Square{ side: 1 } }
fn outside(b: &mut Body) kills b {
    let x = &b.at.x
    coarse(b)
    observe_f32(x)
}
"#,
    );
    clean(&fixture.analysis("plain"));
    for name in ["variant", "optional", "owned"] {
        let a = fixture.analysis(name);
        assert_eq!(a.invalid_uses.len(), 1, "{name}: {a:#?}");
        assert!(a.incomplete.is_empty(), "{name}: {a:#?}");
        assert!(a.mismatches.is_empty(), "{name}: {a:#?}");
    }
    // A callee given `&mut Body` can change what it owns and its payloads, but cannot
    // move a plain field of it.
    let a = fixture.analysis("outside");
    assert!(a.invalid_uses.is_empty(), "{a:#?}");
    assert!(a.mismatches.is_empty(), "{a:#?}");
    // `coarse` itself only kills the payload, so its own clause is wider than that.
    let a = fixture.analysis("coarse");
    assert!(matches!(a.mismatches[..], [Mismatch::TooBroad { .. }]), "{a:#?}");
}

#[test]
fn std_clauses_catch_the_scenarios_of_spec_6_7() {
    let fixture = Fixture::with_source(
        r#"
fn use_str(s: str) {}
fn use_int(n: &mut i32) {}
fn counting(counts: &mut HashMap<String, i32>, k: String, o: String) kills counts@items {
    let n = counts.get_or_insert(k, 0)
    counts.insert(o, 1)
    use_int(n)
}
fn appended(name: &mut String) kills name@text {
    let s = name.as_str()
    name.push_str("x")
    use_str(s)
}
fn slot(xs: &mut List<String>) kills xs@items {
    let e = &xs[0]
    xs.push(String())
    use_str(e.as_str())
}
fn element_text(xs: &mut List<String>) kills xs@items {
    let s = xs[0].as_str()
    xs.push(String())
    use_str(s)
}
fn cleared_text(xs: &mut List<String>) kills xs@items, xs[*] {
    let s = xs[0].as_str()
    xs.clear()
    use_str(s)
}
fn separate(a: &mut List<i32>, b: &mut List<i32>) kills b@items {
    let p = a.first()
    b.push(1)
    let q = p or return
}
"#,
    );
    for name in ["counting", "appended", "slot", "cleared_text"] {
        let a = fixture.analysis(name);
        assert_eq!(a.invalid_uses.len(), 1, "{name}: {a:#?}");
        assert!(a.mismatches.is_empty(), "{name}: {a:#?}");
    }
    // `push` moves the array, not each string's bytes (§6.7).
    clean(&fixture.analysis("element_text"));
    clean(&fixture.analysis("separate"));
}

#[test]
fn clauses_are_compared_over_the_ties_they_name() {
    let fixture = Fixture::with_source(
        r#"
pub struct Name { pub text: String; pub length: i32 }
pub struct Crowd { pub names: List<String>; pub count: i32 }
// `n.text` owns only its bytes, so naming it is naming `n.text@text@items`.
fn whole(n: &mut Name) kills n.text { n.text.push_str("!") }
fn exact(n: &mut Name) kills n.text@text { n.text.push_str("!") }
// A plain field names no tie, so `n` is no wider than `n.text`.
fn outer(n: &mut Name) kills n { n.text.push_str("!") }
// The names' own text does not die when the list grows.
fn wider(c: &mut Crowd) kills c.names { c.names.push(String()) }
fn plain(n: &mut Name) kills n.length { n.length = 1 }
"#,
    );
    for name in ["whole", "exact", "outer"] {
        let a = fixture.analysis(name);
        assert!(a.mismatches.is_empty() && a.incomplete.is_empty(), "{name}: {a:#?}");
    }
    let a = fixture.analysis("wider");
    assert!(matches!(a.mismatches[..], [Mismatch::TooBroad { declared: 0, .. }]), "{a:#?}");
    let a = fixture.analysis("plain");
    assert!(matches!(a.mismatches[..], [Mismatch::Unused { declared: 0 }]), "{a:#?}");
}

#[test]
fn fields_tied_together_live_and_die_together() {
    let fixture = Fixture::with_source(
        r#"
pub struct Table {
    pub keys: ^mut Text in @items = null
    pub values: ^mut Text in @items = null
    pub count: ^mut i32 in @counts = null
}
fn regrow(t: &mut Table) kills t@items {
    let v = t.values
    t.keys = null
    observe(v)
}
fn by_field(t: &mut Table) kills t@items { t.values = null }
fn apart(t: &mut Table) kills t@counts {
    let v = t.values
    t.count = null
    observe(v)
}
"#,
    );
    let a = fixture.analysis("regrow");
    assert_eq!(a.invalid_uses.len(), 1, "{a:#?}");
    assert!(a.incomplete.is_empty() && a.mismatches.is_empty(), "{a:#?}");
    // Killing through either field is killing the tie, which the clause names.
    let a = fixture.analysis("by_field");
    assert!(a.incomplete.is_empty() && a.mismatches.is_empty(), "{a:#?}");
    clean(&fixture.analysis("apart"));
}

#[test]
fn casts_to_another_pointee_are_unchecked_and_dropping_mut_is_not() {
    let fixture = Fixture::with_source(
        r#"
fn reborrow(w: &mut World) kills w.bag@items {
    let p: ^mut Text = w.bag.data
    let q: ^Text = p
    grow(&mut w.bag)
    observe(q)
}
fn reinterpret(w: &mut World) kills w.bag@items {
    let p = unsafe { w.bag.data as ^u8 }
    grow(&mut w.bag)
    observe_byte(p)
}
"#,
    );
    assert_eq!(fixture.analysis("reborrow").invalid_uses.len(), 1);
    let a = fixture.analysis("reinterpret");
    assert!(a.invalid_uses.is_empty(), "{a:#?}");
}

#[test]
fn branches_in_one_owner_keep_each_elements_logical_path() {
    let fixture = Fixture::with_source(
        r#"
pub struct Pair { pub a: Bag<Text>; pub b: Bag<Text> }
fn same_owner(p: &mut Pair, flag: bool) kills p.a@items {
    let list = if flag { &p.a } else { &p.b }
    let text = list[0].data
    grow(&mut p.a)
    observe_byte(text)
}
"#,
    );
    clean(&fixture.analysis("same_owner"));
}

#[test]
fn incomplete_provenance_and_defaults_cannot_remove_or_narrow_clauses() {
    let fixture = Fixture::with_source(
        r#"
pub struct Link {
    pub ptr: ^mut Link in @next = null
    pub fn next(&self) -> ^mut Link in self@next { return ptr }
}
fn chase(p: &mut Link, flag: bool) kills p {
    let mut current: ^mut Link = p
    while flag { let node = current or break; current = node.next() }
}
fn tuple_boundary(w: &mut World) kills w {
    let pair = (&mut w.bag, 1)
    let (b, n) = pair
    grow(b)
}
pub struct Pile {
    pub bag: Bag<Text> in @pile
    pub fn add(&mut self) kills self@pile@items { grow(&mut bag) }
}
pub struct Cursor { pub p: ^mut Pile }
fn field_pointer(p: &mut Pile) {
    let c = Cursor{ p: p }
    unsafe { c.p.add() }
}
fn tuple_view(w: &mut World) kills w {
    let pair = (w.first(), 1)
    let (p, n) = pair
    observe(p)
}
fn defaulted(w: &mut World, other: ^mut World = null) kills other {}
fn default_boundary(w: &mut World) kills w { defaulted(w) }
fn annotated_replace(e: &mut Text in w, w: &mut World) kills e { e = Text{} }
"#,
    );
    for name in ["chase", "tuple_boundary", "default_boundary"] {
        let a = fixture.analysis(name);
        assert!(!a.incomplete.is_empty(), "{name}: {a:#?}");
        assert!(a.mismatches.is_empty(), "{name}: {a:#?}");
    }
    // A view lost in a tuple leaves its use unchecked, but cannot hide a kill: the
    // body kills nothing, which its clause is held to.
    // A call through a pointer field acts on the pointee, never the local holding it,
    // so its kill is not dropped from the summary as that local's; here it is unknown.
    let a = fixture.analysis("field_pointer");
    assert!(!a.incomplete.is_empty(), "{a:#?}");
    assert!(a.mismatches.is_empty(), "{a:#?}");
    let a = fixture.analysis("tuple_view");
    assert!(a.incomplete.is_empty() && !a.unchecked.is_empty(), "{a:#?}");
    assert!(matches!(a.mismatches[..], [Mismatch::Unused { declared: 0 }]), "{a:#?}");
    let a = fixture.analysis("annotated_replace");
    clean(&a);
    assert!(a.mismatches.is_empty(), "{a:#?}");
}

impl Followed for Place {
    fn followed(&self, steps: &[Step]) -> Self {
        let mut place = self.clone();
        place.steps.extend_from_slice(steps);
        place
    }
}
