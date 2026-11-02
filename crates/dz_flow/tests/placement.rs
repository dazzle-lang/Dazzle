//! Where each allocation goes (spec §14.6, T-0117 stage 1): the proposer's rule, the
//! region check that is its safety net, and the two together over every program that checks.
//!
//! The cases are the prototype's (`tests/run/regions` at the tag `regions-prototype`), each a
//! function of [`SOURCE`] whose sites are looked up by the function's name.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use dz_driver::Analysis;
use dz_flow::placement::{self, Decision, Placements, Proposer, Site};
use dz_hir::BodyOwner;

const SOURCE: &str = r#"
struct Store {
    pub saved: List<i32>
    pub names: List<String>
}

// Placed: only borrowed, by its own methods and a `for`.
fn sum_squares(n: i32) -> i32 {
    let mut xs = List<i32>()
    for i in 0..n {
        xs.push(i * i)
    }
    let mut total = 0
    for &x in xs {
        total += x
    }
    return total
}

// Placed: a region per pass of the loop. What is pushed goes there too: the list keeps it.
fn rounds() -> i32 {
    let mut longest = 0
    for round in 0..50 {
        let mut words = List<String>()
        for i in 0..round {
            words.push(String::from("w"))
            if i > longest {
                longest = i
            }
        }
    }
    return longest
}

fn fill(xs: &mut List<i32>, n: i32) kills xs@items {
    for i in 0..n {
        xs.push(i)
    }
}

// Placed: lent to a callee that grows it.
fn lend() -> i32 {
    let mut grown = List<i32>()
    fill(&mut grown, 10)
    return grown.len() as i32
}

// Placed: printed, and copied, into a map and into another local; a copy is a new value.
fn printed() -> i32 {
    let s = String::from("a")
    println!("{}", s)
    let mut counts = HashMap<String, i32>()
    let word = String::from("w")
    counts.insert(word, 1)
    let copy = word
    return counts.len() as i32 + copy.len() as i32
}

// Placed: in the region of an `if`, and of a nested block.
fn nested(flag: bool) -> usize {
    let mut n: usize = 0
    if flag {
        let path = Path("a/b")
        n = path.as_str().len
    }
    {
        let set = HashSet<i32>()
        n += set.len()
    }
    return n
}

// Returned: built where the caller keeps it (stage 2), and with everything proposed
// in its own region, refused as returned.
fn evens() -> List<i32> {
    let mut xs = List<i32>()
    xs.push(2)
    return xs
}

// Heap: moved into a field of a parameter.
fn keep(store: &mut Store) kills store.saved {
    let xs = List<i32>()
    store.saved = move(xs)
}

// Heap: pushed into a container.
fn tag(names: &mut List<String>) kills names@items {
    let s = String::from("a")
    names.push(move(s))
}

fn consume(xs: List<i32>) -> usize {
    return xs.len()
}

// Placed: passed by value to a parameter that stays in the call.
fn give() -> usize {
    let xs = List<i32>()
    return consume(move(xs))
}

fn hoard(xs: List<i32>, store: &mut Store) kills store.saved {
    store.saved = move(xs)
}

// Heap: passed by value to a parameter that is kept.
fn handed(store: &mut Store) kills store.saved {
    let xs = List<i32>()
    hoard(move(xs), store)
}

// Heap: moved to an outer local, which is placed itself.
fn outer() -> usize {
    let mut kept = List<i32>()
    {
        let inner = List<i32>()
        kept = move(inner)
    }
    return kept.len()
}

// Heap: reassigned in a loop (the loop rule).
fn looped() -> usize {
    let mut s = String()
    for _ in 0..3 {
        s = String::from("x")
    }
    return s.len()
}

// Heap: the program's own allocator-taking function, whose use of it is out of sight.
fn squares_in(n: i32, allocator: ^mut Allocator = Allocators::Global) -> List<i32> {
    let a = unsafe { allocator or panic!("no allocator") }
    let scratch = a.alloc(4, 4)
    a.free(scratch, 4, 4)
    let mut xs = List<i32>()
    xs.push(n)
    return move(xs)
}

fn own_fn() -> usize {
    let d = squares_in(3)
    return d.len()
}

// Heap: an `Rc`, which a `Weak` can outlive.
fn counted() -> i32 {
    let _r = Rc<i32>(1)
    return 1
}

fn main() {
    println!("{} {} {} {} {}", sum_squares(3), rounds(), lend(), printed(), nested(true))
    let mut store = Store{ saved: List<i32>(), names: List<String>() }
    keep(&mut store)
    handed(&mut store)
    tag(&mut store.names)
    println!("{} {} {} {} {} {}", evens().len(), give(), outer(), looped(), own_fn(), counted())
}
"#;

fn scratch() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "dz_flow_placement_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// The whole pipeline over `source`, which must check clean.
fn analyse(source: &str) -> Analysis {
    let dir = scratch();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"placement\"\nsources = [\"src/**/*.dz\"]\n\n\
         [dependencies]\nstd = { open = true }\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.dz"), source).unwrap();
    let a = dz_driver::analyse(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!a.diags.has_errors(), "{}", dz_diag::render_to_string(&a.map, &a.diags));
    a
}

fn place(a: &Analysis, proposer: Proposer) -> Placements {
    placement::place_with(
        a.hir.as_ref().unwrap(),
        a.table.as_ref().unwrap(),
        a.resolution.as_ref().unwrap(),
        proposer,
    )
}

/// The sites of the function `name`, in source order.
fn sites<'p>(a: &Analysis, p: &'p Placements, name: &str) -> Vec<&'p Site> {
    let res = a.resolution.as_ref().unwrap();
    let (_, body) = p
        .iter()
        .find(|(owner, _)| {
            matches!(owner, BodyOwner::Fn(s) if res.symbol(*s).name == name
                && res.symbol(*s).module != res.prelude)
        })
        .unwrap_or_else(|| panic!("`{name}` has no sites"));
    body.sites.iter().collect()
}

/// `name`'s sites as `callee: ` and the label the editor shows: `region: scope`, `result:
/// …` or `heap: why`.
fn decisions(a: &Analysis, p: &Placements, name: &str) -> Vec<String> {
    sites(a, p, name).into_iter().map(|s| format!("{}: {}", s.callee, s.decision.label())).collect()
}

fn body<'p>(a: &Analysis, p: &'p Placements, name: &str) -> &'p placement::BodyPlacement {
    let res = a.resolution.as_ref().unwrap();
    p.iter()
        .find(|(owner, _)| {
            matches!(owner, BodyOwner::Fn(s) if res.symbol(*s).name == name
                && res.symbol(*s).module != res.prelude)
        })
        .map(|(_, b)| b)
        .unwrap_or_else(|| panic!("`{name}` has no sites"))
}

/// Each case: the function, and what its sites print, in source order.
const PLACED: &[(&str, &[&str])] = &[
    ("sum_squares", &["List<i32>: region: `sum_squares`"]),
    (
        "rounds",
        &[
            "List<String>: region: each pass of a loop in `rounds`",
            // Pushed on every pass of the inner loop into a list that keeps it (T-0145).
            "String::from: region: each pass of a loop in `rounds`",
        ],
    ),
    ("lend", &["List<i32>: region: `lend`"]),
    (
        "printed",
        &[
            "String::from: region: `printed`",
            "HashMap<String, i32>: region: `printed`",
            "String::from: region: `printed`",
        ],
    ),
    ("nested", &["Path: region: an `if` in `nested`", "HashSet<i32>: region: a block in `nested`"]),
    ("give", &["List<i32>: region: `give`"]),
];

/// The wrong placements: the function, its site's index, and the proposer's reason.
const WRONG: &[(&str, usize, &str)] = &[
    ("keep", 0, "List<i32>: heap: stored in `store.saved`"),
    ("tag", 0, "String::from: heap: passed to `names.push`"),
    ("handed", 0, "List<i32>: heap: passed to `hoard`"),
    ("outer", 1, "List<i32>: heap: stored in `kept`"),
    ("looped", 1, "String::from: heap: assigned to `s` on every pass of a loop"),
];

/// Heap by a fixed rule, whatever is proposed.
const FIXED: &[(&str, &str)] = &[
    ("own_fn", "squares_in: heap: made inside `squares_in`"),
    ("counted", "Rc<i32>: heap: a `Weak` can outlive the scope"),
];

#[test]
fn values_that_stay_in_their_scope_get_its_region() {
    let a = analyse(SOURCE);
    let p = place(&a, Proposer::Rules);
    for (f, expected) in PLACED {
        assert_eq!(decisions(&a, &p, f), *expected, "in `{f}`");
        assert!(body(&a, &p, f).demoted().is_empty(), "the check refused a placement in `{f}`");
    }
    // `outer` keeps its own list in its region; only the inner one, moved out, is heap.
    assert_eq!(decisions(&a, &p, "outer")[0], "List<i32>: region: `outer`");
    // The region's block and where the placement is passed, as codegen reads them: the
    // hidden parameter past the last the callee writes.
    let lend = body(&a, &p, "lend");
    let site = &lend.sites[0];
    let Decision::Region { block, .. } = site.decision else { panic!() };
    assert_eq!(lend.region_for(site.call), Some((2, block)), "`List(capacity, loc)`");
    assert_eq!(lend.region_blocks(), vec![block]);
    let printed = body(&a, &p, "printed");
    assert_eq!(printed.sites[0].arg, 2, "`String::from(s, loc)`");
}

#[test]
fn values_that_leave_their_scope_keep_the_heap_and_say_why() {
    let a = analyse(SOURCE);
    let p = place(&a, Proposer::Rules);
    for (f, index, expected) in WRONG {
        let decided = decisions(&a, &p, f);
        assert_eq!(decided[*index], *expected, "in `{f}`");
        let Decision::Heap { at, .. } = &sites(&a, &p, f)[*index].decision else { panic!() };
        assert!(at.is_some(), "`{f}`: a reason found at a use says where");
    }
    for (f, expected) in FIXED {
        assert_eq!(decisions(&a, &p, f), [*expected], "in `{f}`");
    }
    // The lists `main` builds are a struct literal's fields, placed where the struct is:
    // `store` is only lent, so in `main`'s region (round 3). `evens().len()` reads a
    // temporary, which no block keeps yet.
    assert_eq!(
        decisions(&a, &p, "main"),
        ["List<i32>: region: `main`", "List<String>: region: `main`", "evens: heap: a temporary"]
    );
}

/// A trait's symbol, by name, in the program's own module.
fn trait_sym(a: &Analysis, name: &str) -> dz_resolve::SymbolId {
    let res = a.resolution.as_ref().unwrap();
    let hir = a.hir.as_ref().unwrap();
    hir.bodies()
        .find_map(|(_, b)| match &b.owner {
            BodyOwner::TraitDefaults { owner, .. } | BodyOwner::Method { owner, .. }
                if res.symbol(*owner).name == name =>
            {
                Some(*owner)
            }
            _ => None,
        })
        .or_else(|| {
            a.table.as_ref().unwrap().impls().iter().find_map(|i| {
                (res.symbol(i.trait_ref.trait_sym).name == name).then_some(i.trait_ref.trait_sym)
            })
        })
        .unwrap_or_else(|| panic!("no trait `{name}`"))
}

/// Stage 2 (T-0117): a result function builds what it returns where its caller keeps it,
/// and each caller says where that is — its region, its own result placement, the heap.
/// What takes a result placement is decided from signatures alone, so every call agrees.
#[test]
fn a_returned_value_is_built_where_the_caller_keeps_it() {
    let a = analyse(
        r#"
struct Keeper {
    pub saved: List<i32>

    // A method returns into its caller's placement too.
    pub fn fresh(&self) -> List<i32> {
        return List<i32>()
    }
}

trait Maker {
    fn make(&self) -> List<i32>
}

impl Maker for Keeper {
    // Decided by the trait: every implementation and the vtable slot take one alike.
    fn make(&self) -> List<i32> {
        return List<i32>()
    }
}

fn evens() -> List<i32> {
    let mut xs = List<i32>()
    xs.push(2)
    return move(xs)
}

// Passes its own caller's placement on.
fn again() -> List<i32> {
    return evens()
}

// Made on every pass of a loop the caller's placement outlives: the check refuses it.
fn found(n: i32) -> List<i32> {
    for i in 0..n {
        let xs = List<i32>()
        if i == 3 {
            return xs
        }
    }
    return List<i32>()
}

// An optional result: what `or` unwraps is what was built, placed as the `or` is.
fn maybe(n: i32) -> List<i32>? {
    if n > 0 {
        return List<i32>()
    }
    return none
}

fn forward(n: i32) -> List<i32>? {
    let xs = maybe(n) or return none
    return xs
}

fn direct(n: i32) -> List<i32>? {
    return maybe(n) or return none
}

// Used as a function value: its type passes a placement as a call by name does.
fn valued() -> List<i32> {
    return List<i32>()
}

// A bare `T` hands back a value that exists already: no placement.
fn last<T>(xs: &mut List<T>) -> T? kills xs@items, xs[*] {
    return xs.pop()
}

fn main() {
    let kept = evens()
    let mut k = Keeper{ saved: List<i32>() }
    k.saved = again()
    let made = k.fresh()
    let f = valued
    let from_value = f()
    let other = k.make()
    let mut nums = List<i32>()
    let popped = last(&mut nums)
    let unwrapped = maybe(1) or List<i32>()
    let n = unwrapped.len() + (forward(1) or List<i32>()).len() + (direct(1) or List<i32>()).len()
    println!("{} {} {} {} {} {} {}", kept.len(), made.len(), from_value.len(), other.len(),
        found(4).len(), popped or 0, n)
}
"#,
    );
    let p = place(&a, Proposer::Rules);
    let res = a.resolution.as_ref().unwrap();
    let takes = |name: &str| {
        p.iter().any(|(owner, _)| match owner {
            BodyOwner::Fn(s) => res.symbol(*s).name == name && p.takes_result(owner),
            _ => false,
        }) || {
            let hir = a.hir.as_ref().unwrap();
            hir.bodies().any(|(_, b)| {
                matches!(&b.owner, BodyOwner::Fn(s) if res.symbol(*s).name == name)
                    && p.takes_result(&b.owner)
            })
        }
    };
    for f in ["evens", "again", "found", "valued"] {
        assert!(takes(f), "`{f}` takes a result placement");
    }
    for f in ["last", "main"] {
        assert!(!takes(f), "`{f}` takes none");
    }

    assert_eq!(decisions(&a, &p, "evens"), ["List<i32>: result: where `evens`'s caller keeps it"]);
    assert_eq!(decisions(&a, &p, "again"), ["evens: result: where `again`'s caller keeps it"]);
    assert_eq!(
        decisions(&a, &p, "found"),
        [
            "List<i32>: heap: made on every pass of a loop",
            "List<i32>: result: where `found`'s caller keeps it"
        ]
    );
    assert!(body(&a, &p, "found").demoted().contains(&sites(&a, &p, "found")[0].call));
    assert_eq!(
        decisions(&a, &p, "valued"),
        ["List<i32>: result: where `valued`'s caller keeps it"]
    );
    assert_eq!(decisions(&a, &p, "maybe"), ["List<i32>: result: where `maybe`'s caller keeps it"]);
    assert_eq!(decisions(&a, &p, "forward"), ["maybe: result: where `forward`'s caller keeps it"]);
    assert_eq!(decisions(&a, &p, "direct"), ["maybe: result: where `direct`'s caller keeps it"]);
    let main = decisions(&a, &p, "main");
    assert!(main.contains(&"maybe: region: `main`".into()), "through `or`: {main:?}");

    // The method's body, and the trait implementation's.
    let method = |pick: fn(&BodyOwner) -> bool| {
        p.iter().find(|(o, _)| pick(o)).map(|(_, b)| b.sites[0].decision.label()).unwrap()
    };
    assert_eq!(
        method(|o| matches!(o, BodyOwner::Method { name, .. } if name == "fresh")),
        "result: where `fresh`'s caller keeps it"
    );
    assert_eq!(
        method(|o| matches!(o, BodyOwner::ImplMethod { name, .. } if name == "make")),
        "result: where `make`'s caller keeps it"
    );
    let trait_methods = |name: &str| {
        p.takes_result(&BodyOwner::TraitDefaults {
            owner: trait_sym(&a, "Maker"),
            name: name.into(),
        })
    };
    assert!(trait_methods("make"), "the trait's own slot");

    // Each caller says where: kept and borrowed, its region; stored, the heap.
    let main = decisions(&a, &p, "main");
    assert_eq!(main[0], "evens: region: `main`");
    assert!(main.contains(&"again: heap: assigned to `k.saved`; only a `let` gets a region".into()));
    assert!(main.contains(&"k.fresh: region: `main`".into()), "{main:?}");
    // Through a trait method and a function value, as by name.
    assert!(main.contains(&"k.make: region: `main`".into()), "{main:?}");
    assert!(main.contains(&"a function value: region: `main`".into()), "{main:?}");
    // The arguments: one past each callee's last written parameter.
    let evens_call = sites(&a, &p, "main")[0];
    assert_eq!(evens_call.arg, 0);
    let fresh_call = sites(&a, &p, "main").into_iter().find(|s| s.callee == "k.fresh").unwrap();
    assert_eq!(fresh_call.arg, 0, "`self` is not counted");
}

/// Stage 2, round 3 (T-0117): a value's owned parts live where the value lives. A struct
/// literal's fields, a tuple's parts and a `Box`'s contents are placed by what becomes of
/// the value holding them, followed into the local a `let` moves it to; and the check
/// follows a region into a `Box` as into a struct, so every escape of the holder is still
/// refused, by the check alone too.
#[test]
fn owned_parts_live_where_their_value_lives() {
    let source = r#"
struct Pair {
    pub left: List<i32>
    pub right: List<i32>
}

struct Holder {
    pub kept: Pair = Pair{ left: List<i32>(), right: List<i32>() }
}

// Only lent: both fields in the block's region.
fn lent() -> usize {
    let p = Pair{ left: List<i32>(), right: List<i32>() }
    return p.left.len() + p.right.len()
}

// Moved on to another local, which is only lent.
fn handed_on() -> usize {
    let first = List<i32>()
    let p = Pair{ left: move(first), right: List<i32>() }
    return p.left.len()
}

// Returned: both fields where the caller keeps the pair.
fn built() -> Pair {
    return Pair{ left: List<i32>(), right: List<i32>() }
}

// A box and its contents, placed together.
fn boxed() -> usize {
    let b = Box(List<i32>())
    let held = &b
    return if held.is_null() { 0 } else { 1 }
}

// Stored where the block's region cannot reach: the heap, by the proposer and the check.
fn stored(h: &mut Holder) kills h.kept {
    let p = Pair{ left: List<i32>(), right: List<i32>() }
    h.kept = move(p)
}

// A box given away: its contents with it.
fn given(xs: &mut List<Box<List<i32>>>) kills xs@items {
    let b = Box(List<i32>())
    xs.push(move(b))
}

fn main() {
    let mut h = Holder{}
    stored(&mut h)
    let mut boxes = List<Box<List<i32>>>()
    given(&mut boxes)
    let p = built()
    println!("{} {} {} {} {}", lent(), handed_on(), boxed(), p.left.len(), boxes.len())
}
"#;
    let a = analyse(source);
    let p = place(&a, Proposer::Rules);
    assert_eq!(
        decisions(&a, &p, "lent"),
        ["List<i32>: region: `lent`", "List<i32>: region: `lent`"]
    );
    assert_eq!(
        decisions(&a, &p, "handed_on"),
        ["List<i32>: region: `handed_on`", "List<i32>: region: `handed_on`"]
    );
    assert_eq!(
        decisions(&a, &p, "built"),
        [
            "List<i32>: result: where `built`'s caller keeps it",
            "List<i32>: result: where `built`'s caller keeps it"
        ]
    );
    assert_eq!(
        decisions(&a, &p, "boxed"),
        ["Box<List<i32>>: region: `boxed`", "List<i32>: region: `boxed`"]
    );
    for d in decisions(&a, &p, "stored") {
        assert!(d.ends_with("heap: stored in `h.kept`"), "{d}");
    }
    for d in decisions(&a, &p, "given") {
        assert!(d.ends_with("heap: passed to `xs.push`"), "{d}");
    }

    // The check alone, with every site proposed for its block.
    let p = place(&a, Proposer::Everything);
    for d in decisions(&a, &p, "stored") {
        assert!(d.ends_with("heap: stored in `h.kept`"), "the check: {d}");
    }
    for d in decisions(&a, &p, "given") {
        assert!(d.ends_with("heap: passed to `xs.push`"), "the check: {d}");
    }
    assert_eq!(
        decisions(&a, &p, "boxed"),
        ["Box<List<i32>>: region: `boxed`", "List<i32>: region: `boxed`"]
    );
}

/// Stage 2, round 3 (T-0117): a constructor builds its fields where the value it makes is
/// placed, so it takes a placement as a result function does and each call passes one.
/// A field rebuilt on every pass of a loop, and a value passed away, are refused. A
/// constructor can neither move a field out of `self` nor change what its parameters
/// reach, so those escapes cannot be written.
#[test]
fn a_constructor_builds_where_its_value_is_placed() {
    let a = analyse(
        r#"
struct Bag {
    pub items: List<i32>
    pub spare: List<i32>

    Bag(n: i32) {
        items = List<i32>()
        for i in 0..n {
            items.push(i)
        }
        spare = List<i32>()
    }
}

struct Looping {
    pub items: List<i32>

    // Rebuilt on every pass: the placement would grow with each.
    Looping(n: i32) {
        items = List<i32>()
        for _ in 0..n {
            items = List<i32>()
        }
    }
}

// Keeps what it is given, so a value passed to it leaves the caller.
fn consume(xs: List<i32>) -> usize {
    let mut kept = List<List<i32>>()
    kept.push(move(xs))
    return kept.len()
}

struct Giving {
    pub count: usize
    pub items: List<i32>

    // Passed away by value: whoever takes it may keep it anywhere.
    Giving() {
        count = consume(List<i32>())
        items = List<i32>()
    }
}

fn kept() -> usize {
    let b = Bag(3)
    return b.items.len()
}

fn made() -> Bag {
    return Bag(2)
}

fn main() {
    let l = Looping(3)
    let b = made()
    let g = Giving()
    println!("{} {} {} {}", kept(), l.items.len(), b.items.len(), g.count + g.items.len())
}
"#,
    );
    let p = place(&a, Proposer::Rules);
    let ctor = |owner: &str| {
        let res = a.resolution.as_ref().unwrap();
        p.iter()
            .find(|(o, _)| {
                matches!(o, BodyOwner::Ctor { owner: s, .. } if res.symbol(*s).name == owner)
            })
            .map(|(o, b)| {
                assert!(p.takes_result(o), "`{owner}`'s constructor takes a placement");
                b.sites.iter().map(|s| format!("{}: {}", s.callee, s.decision.label())).collect()
            })
            .unwrap_or_else(Vec::<String>::new)
    };
    assert_eq!(
        ctor("Bag"),
        [
            "List<i32>: result: where the `Bag` constructor's caller keeps it",
            "List<i32>: result: where the `Bag` constructor's caller keeps it"
        ]
    );
    // The field rebuilt in a loop is refused by the check, and the value passed away by
    // the proposer; what is built into `self` once and stays there is placed.
    let looping = ctor("Looping");
    assert!(looping[0].contains("result:"), "{looping:?}");
    assert!(looping[1].contains("heap: made on every pass of a loop"), "{looping:?}");
    let giving = ctor("Giving");
    assert!(giving[0].contains("heap: passed to `consume`"), "{giving:?}");
    assert!(giving[1].contains("result:"), "{giving:?}");

    // Each call passes where the value goes.
    assert_eq!(decisions(&a, &p, "kept"), ["Bag: region: `kept`"]);
    assert_eq!(decisions(&a, &p, "made"), ["Bag: result: where `made`'s caller keeps it"]);
}

/// The gate's last row (T-0117): with everything proposed, the region check alone refuses
/// every wrong placement, and still places every right one.
#[test]
fn the_region_check_alone_refuses_every_wrong_placement() {
    let a = analyse(SOURCE);
    let p = place(&a, Proposer::Everything);
    let checked: &[(&str, usize, &str)] = &[
        ("evens", 0, "List<i32>: heap: returned"),
        ("keep", 0, "List<i32>: heap: stored in `store.saved`"),
        ("tag", 0, "String::from: heap: passed to `names.push`"),
        ("handed", 0, "List<i32>: heap: passed to `hoard`"),
        ("outer", 1, "List<i32>: heap: stored in `kept`"),
        // The loop rule, from the check: the value goes into a local the pass outlives.
        ("looped", 1, "String::from: heap: stored in `s`"),
        // Proposed for the inner loop's region, but stored into a list declared outside it
        // (T-0145).
        ("rounds", 1, "String::from: heap: stored in `words`"),
    ];
    for (f, index, expected) in checked {
        assert_eq!(decisions(&a, &p, f)[*index], *expected, "in `{f}`");
        let site = sites(&a, &p, f)[*index];
        assert!(body(&a, &p, f).demoted().contains(&site.call), "`{f}`: refused by the check");
    }
    // `rounds`' pushed string is right where the proposer puts it, the list's region, and
    // wrong where everything is proposed, its own loop's: it is in `checked` above.
    for (f, expected) in PLACED.iter().filter(|(f, _)| *f != "rounds") {
        assert_eq!(decisions(&a, &p, f), *expected, "in `{f}`");
    }
    for (f, expected) in FIXED {
        assert_eq!(decisions(&a, &p, f), [*expected], "in `{f}`");
    }
}

/// More wrong placements, each refused by the check with everything proposed: through a
/// block's value, an `or`, a struct moved out, a `while` condition and an element.
#[test]
fn the_region_check_follows_a_region_through_values() {
    let a = analyse(
        r#"
struct Holder {
    pub items: List<i32>
}
fn block_value() -> List<i32> {
    let xs = {
        let inner = List<i32>()
        move(inner)
    }
    return move(xs)
}
fn wrapped() -> Holder {
    let h = Holder{ items: List<i32>() }
    return move(h)
}
fn into_element(lists: &mut List<List<i32>>) kills lists[*] {
    let xs = List<i32>()
    lists[0] = move(xs)
}
fn condition() -> i32 {
    let mut n = 0
    while List<i32>().len() > 10 {
        n += 1
    }
    return n
}
fn optional() -> List<i32>? {
    let xs: List<i32>? = List<i32>()
    return move(xs)
}
fn kept_inside() -> usize {
    let mut outer = List<i32>()
    let h = Holder{ items: List<i32>() }
    outer = h.items
    return outer.len() + h.items.len()
}
fn main() {
    let mut lists = List<List<i32>>()
    lists.push(List<i32>())
    into_element(&mut lists)
    println!("{} {} {} {} {}", block_value().len(), wrapped().items.len(), condition(), (optional() or List<i32>()).len(), kept_inside())
}
"#,
    );
    let p = place(&a, Proposer::Everything);
    let refused = |f: &str, index: usize, why: &str| {
        let site = sites(&a, &p, f)[index];
        let Decision::Heap { why: found, .. } = &site.decision else {
            panic!("`{f}`: site {index} was placed")
        };
        assert_eq!(found, why, "in `{f}`");
        assert!(body(&a, &p, f).demoted().contains(&site.call), "`{f}`: refused by the check");
    };
    refused("block_value", 0, "outlives a block in `block_value`");
    refused("wrapped", 0, "returned");
    refused("into_element", 0, "stored in `lists[…]`");
    refused("condition", 0, "made on every pass of a loop");
    refused("optional", 0, "returned");
    // A copy carries no region: `h.items` copied into `outer` leaves both in place.
    assert_eq!(
        decisions(&a, &p, "kept_inside"),
        ["List<i32>: region: `kept_inside`", "List<i32>: region: `kept_inside`"]
    );
}

/// `format!`'s text is the macro's value, so it is placed by where that value goes, although
/// C++ builds it inside a lambda; and the check alone refuses each wrong placement of it.
#[test]
fn a_formatted_string_goes_where_the_macro_value_goes() {
    let a = analyse(
        r#"
struct Store {
    pub name: String
}
fn kept(n: i32) -> usize {
    let s = format!("n = {}", n)
    return s.len()
}
fn per_pass(n: i32) -> usize {
    let mut total: usize = 0
    for i in 0..n {
        let s = format!("{}", i)
        total += s.len()
    }
    return total
}
fn made(n: i32) -> String {
    return format!("made {}", n)
}
fn pushed(names: &mut List<String>, n: i32) kills names@items {
    let s = format!("{}", n)
    names.push(move(s))
}
fn named(store: &mut Store, n: i32) kills store.name {
    store.name = format!("{}", n)
}
fn reassigned(n: i32) -> usize {
    let mut s = String()
    for i in 0..n {
        s = format!("{}", i)
    }
    return s.len()
}
fn lent(n: i32) -> usize {
    return format!("{}", n).len()
}
fn main() {
    let mut names = List<String>()
    pushed(&mut names, 1)
    let mut store = Store{ name: String() }
    named(&mut store, 2)
    println!("{} {} {} {} {}", kept(1), per_pass(3), made(4), reassigned(5), lent(6))
}
"#,
    );
    let p = place(&a, Proposer::Rules);
    assert_eq!(decisions(&a, &p, "kept"), ["String: region: `kept`"]);
    assert_eq!(
        decisions(&a, &p, "per_pass"),
        ["String: region: each pass of a loop in `per_pass`"]
    );
    assert_eq!(decisions(&a, &p, "made"), ["String: result: where `made`'s caller keeps it"]);
    assert_eq!(decisions(&a, &p, "pushed"), ["String: heap: passed to `names.push`"]);
    assert_eq!(
        decisions(&a, &p, "named"),
        ["String: heap: assigned to `store.name`; only a `let` gets a region"]
    );
    assert_eq!(
        decisions(&a, &p, "reassigned")[1],
        "String: heap: assigned to `s` on every pass of a loop"
    );
    assert_eq!(decisions(&a, &p, "lent"), ["String: heap: a temporary"]);
    for f in ["kept", "per_pass", "made"] {
        assert!(body(&a, &p, f).demoted().is_empty(), "the check refused a placement in `{f}`");
    }

    let p = place(&a, Proposer::Everything);
    let refused = |f: &str, index: usize, why: &str| {
        let site = sites(&a, &p, f)[index];
        let Decision::Heap { why: found, .. } = &site.decision else {
            panic!("`{f}`: site {index} was placed")
        };
        assert_eq!(found, why, "in `{f}`");
        assert!(body(&a, &p, f).demoted().contains(&site.call), "`{f}`: refused by the check");
    };
    refused("pushed", 0, "passed to `names.push`");
    refused("named", 0, "stored in `store.name`");
    refused("reassigned", 1, "stored in `s`");
    refused("made", 0, "returned");
}

/// A value passed by value to a parameter that stays in the call — the callee only reads,
/// borrows, copies or drops it, or hands it to another such parameter — keeps its caller's
/// region. A parameter returned, stored or handed to one that is kept does not stay, and
/// the check alone withdraws each such claim.
#[test]
fn a_parameter_that_stays_in_the_call_takes_the_callers_region() {
    let a = analyse(
        r#"
struct Holder {
    pub kept: List<i32>

    Holder() {
        kept = List<i32>()
    }

    pub fn total(&self, xs: List<i32>) -> usize {
        return xs.len() + self.kept.len()
    }

    pub fn take(&mut self, xs: List<i32>) kills self.kept {
        self.kept = move(xs)
    }
}
fn measure(xs: List<i32>) -> usize {
    let n = xs.len()
    let ys = move(xs)
    return n + ys.len()
}
fn countdown(xs: List<i32>, n: i32) -> usize {
    if n == 0 {
        return xs.len()
    }
    return countdown(move(xs), n - 1)
}
fn forward(xs: List<i32>) -> usize {
    return measure(move(xs))
}
fn hand_back(xs: List<i32>) -> List<i32> {
    return move(xs)
}
fn hoard(xs: List<i32>, into: &mut List<List<i32>>) kills into@items {
    into.push(move(xs))
}
fn relay(xs: List<i32>, into: &mut List<List<i32>>) kills into@items {
    hoard(move(xs), into)
}
fn caller(h: &mut Holder) -> usize kills h.kept {
    let a = List<i32>()
    let b = List<i32>()
    let c = List<i32>()
    let d = List<i32>()
    let mut lists = List<List<i32>>()
    relay(move(d), &mut lists)
    let e = List<i32>()
    h.take(move(e))
    return measure(move(a)) + countdown(move(b), 3) + forward(move(c)) + h.total(List<i32>())
}
fn main() {
    let mut h = Holder()
    println!("{} {}", caller(&mut h), hand_back(List<i32>()).len())
}
"#,
    );
    let res = a.resolution.as_ref().unwrap();
    let hir = a.hir.as_ref().unwrap();
    let owner = |name: &str| {
        hir.bodies()
            .map(|(_, b)| b.owner.clone())
            .find(|o| match o {
                BodyOwner::Fn(s) => {
                    res.symbol(*s).name == name && res.symbol(*s).module != res.prelude
                }
                BodyOwner::Method { name: n, .. } => n == name,
                _ => false,
            })
            .unwrap_or_else(|| panic!("no `{name}`"))
    };
    for proposer in [Proposer::Rules, Proposer::Everything] {
        let p = place(&a, proposer);
        let stays = |f: &str| p.staying().param_stays(&owner(f), 0);
        for f in ["measure", "countdown", "forward", "total"] {
            assert!(stays(f), "{proposer:?}: `{f}`'s parameter stays in the call");
        }
        for f in ["hand_back", "hoard", "relay", "take"] {
            assert!(!stays(f), "{proposer:?}: `{f}`'s parameter leaves the call");
        }
    }
    let p = place(&a, Proposer::Rules);
    assert_eq!(
        decisions(&a, &p, "caller"),
        [
            "List<i32>: region: `caller`",
            "List<i32>: region: `caller`",
            "List<i32>: region: `caller`",
            "List<i32>: heap: passed to `relay`",
            "List<List<i32>>: region: `caller`",
            "List<i32>: heap: passed to `h.take`",
            "List<i32>: heap: a temporary",
        ]
    );
    assert!(body(&a, &p, "caller").demoted().is_empty(), "the check refused a placement");
}

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every project directly under `dir`.
fn projects(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> =
        std::fs::read_dir(dir).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    out.retain(|p| p.join("dazzle.toml").is_file());
    out.sort();
    out
}

/// The corpus: std and every program that checks clean. The check must refuse nothing the
/// proposer placed; if it does, one of the two is wrong.
#[test]
fn the_region_check_refuses_nothing_the_proposer_places_in_the_corpus() {
    let root = corpus_root();
    let mut dirs = Vec::new();
    for sub in ["examples", "tests/run", "tests/bench_programs"] {
        dirs.extend(projects(&root.join(sub)));
    }
    let mut checked = 0;
    let mut std_seen = false;
    let mut refused = Vec::new();
    let mut std_sites = (0, 0);
    for dir in &dirs {
        let a = dz_driver::analyse(dir);
        // A vendor submodule missing from a fresh checkout, or a library this machine lacks.
        if a.diags.has_errors() || a.placements.is_none() {
            continue;
        }
        checked += 1;
        let res = a.resolution.as_ref().unwrap();
        let p = a.placements.as_ref().unwrap();
        let (mut sites, mut placed) = (0, 0);
        for (owner, b) in p.iter() {
            let prelude = owner_module(owner, &a) == res.prelude;
            if prelude && std_seen {
                continue;
            }
            for call in b.demoted() {
                let site = b.sites.iter().find(|s| s.call == *call).unwrap();
                let file = a.map.get(b.file);
                let at = file.location(site.span.start as usize);
                refused.push(format!(
                    "{}:{}:{} {} {:?}",
                    file.name(),
                    at.line,
                    at.column,
                    site.callee,
                    site.decision
                ));
            }
            if prelude {
                std_sites.0 += b.sites.len();
                std_sites.1 += b
                    .sites
                    .iter()
                    .filter(|s| matches!(s.decision, Decision::Region { .. }))
                    .count();
            } else {
                sites += b.sites.len();
                placed += b
                    .sites
                    .iter()
                    .filter(|s| matches!(s.decision, Decision::Region { .. }))
                    .count();
            }
        }
        std_seen = true;
        if dir.parent().is_some_and(|d| d.ends_with("bench_programs")) {
            eprintln!(
                "{}: {placed} of {sites} sites placed",
                dir.file_name().unwrap().to_string_lossy()
            );
        }
    }
    assert!(checked > 0, "no program in the corpus checked");
    eprintln!("{checked} of {} programs checked", dirs.len());
    eprintln!("std: {} of {} sites placed", std_sites.1, std_sites.0);
    assert!(
        refused.is_empty(),
        "the check refused what the proposer placed:\n{}",
        refused.join("\n")
    );
}

/// `astar`'s maps and path are locals of the functions that build them (T-0104).
#[test]
fn astar_places_its_queue_maps_and_path() {
    let a = dz_driver::analyse(&corpus_root().join("tests/bench_programs/astar"));
    assert!(!a.diags.has_errors(), "{}", dz_diag::render_to_string(&a.map, &a.diags));
    let p = a.placements.as_ref().unwrap();
    assert_eq!(
        decisions(&a, p, "find_path"),
        [
            // The queue: its constructor builds its list where the queue is (round 3).
            "MinHeap<Node>: region: `find_path`",
            "HashMap<Cell, i32>: region: `find_path`",
            "HashMap<Cell, Cell>: region: `find_path`",
        ]
    );
    assert_eq!(decisions(&a, p, "trace"), ["List<Cell>: region: `trace`"]);
}

/// Which of std's methods store what they are given into `self`, worked out from their
/// bodies, and which keep it on every call (T-0145).
#[test]
fn std_containers_store_what_they_are_given() {
    let a = analyse(
        "fn main() {}
",
    );
    let res = a.resolution.as_ref().unwrap();
    let staying = a.placements.as_ref().unwrap().staying();
    let method = |ty: &str, name: &str| BodyOwner::Method {
        owner: res.lookup(res.prelude, ty).unwrap_or_else(|| panic!("no `{ty}` in std")),
        name: name.to_owned(),
    };
    // (type, method, parameter, stored, kept)
    for (ty, name, index, stored, kept) in [
        ("List", "push", 0, true, true),
        ("List", "insert", 1, true, true),
        ("SlotMap", "insert", 0, true, true),
        // It may drop the key it already had, or the value it replaces.
        ("HashMap", "insert", 0, true, false),
        ("HashMap", "insert", 1, true, false),
        ("HashMap", "get_or_insert", 1, true, false),
        ("HashSet", "insert", 0, true, false),
    ] {
        let owner = method(ty, name);
        assert_eq!(staying.param_stored(&owner, index), stored, "{ty}::{name} stores {index}");
        assert_eq!(staying.param_kept(&owner, index), kept, "{ty}::{name} keeps {index}");
    }
}

/// A value stored into a container is built where the container lives, and the region
/// check follows it back out: popped and kept elsewhere, or lent out with the container as
/// `&mut`, it keeps the heap. On every pass of a loop it goes there only when the container
/// keeps everything stored into it (T-0145).
#[test]
fn a_value_stored_into_a_container_is_built_where_it_lives() {
    let a = analyse(
        r#"
struct Store {
    pub names: List<String>
}

fn filled(n: i32) -> usize {
    let mut names = List<String>()
    for _ in 0..n {
        names.push(String::from("x"))
        names.push(format!("{}", n))
    }
    return names.len()
}

fn popped(store: &mut Store) -> usize kills store.names@items {
    let mut names = List<String>()
    names.push(String::from("kept elsewhere"))
    let last = names.pop() or return 0
    store.names.push(move(last))
    return names.len()
}

fn drained(n: i32) -> usize {
    let mut names = List<String>()
    let mut count: usize = 0
    for _ in 0..n {
        names.push(String::from("pass"))
        let s = names.pop() or return 0
        count += s.len()
    }
    return count
}

fn give(from: &mut List<String>, store: &mut Store) kills from@items, from[*], store.names@items {
    let s = from.pop() or return
    store.names.push(move(s))
}

fn lent(store: &mut Store) -> usize kills store.names@items {
    let mut names = List<String>()
    names.push(String::from("lent"))
    give(&mut names, store)
    return names.len()
}

fn mapped(n: i32) -> usize {
    let mut m = HashMap<i32, String>()
    m.insert(1, String::from("once"))
    for i in 0..n {
        m.insert(i, String::from("may replace"))
    }
    return m.len()
}

fn made() -> List<String> {
    let mut out = List<String>()
    out.push(String::from("made"))
    return out
}

fn tagged(names: &mut List<String>) kills names@items {
    names.push(String::from("the caller's list"))
}

fn main() {
    let mut store = Store{ names: List<String>() }
    tagged(&mut store.names)
    let m = made()
    println!("{} {} {} {} {} {}", filled(3), popped(&mut store), drained(2), lent(&mut store), mapped(2), m.len())
}
"#,
    );
    let p = a.placements.as_ref().unwrap();
    let region = |f: &str| format!("region: `{f}`");
    assert_eq!(
        decisions(&a, p, "filled"),
        [
            format!("List<String>: {}", region("filled")),
            // Built on every pass and stored at once, into a list that keeps it.
            format!("String::from: {}", region("filled")),
            // Built inside the macro, and stored only as its value.
            "String: heap: stored in `names` on every pass of a loop".to_owned(),
        ]
    );
    assert_eq!(
        decisions(&a, p, "popped")[1],
        "String::from: heap: passed to `store.names.push`, once taken out of `names`",
        "taken back out and stored where the region cannot follow"
    );
    assert_eq!(
        decisions(&a, p, "drained")[1],
        "String::from: heap: stored in `names` on every pass of a loop",
        "popped in the same loop: the list does not keep it, and the region would grow"
    );
    assert_eq!(decisions(&a, p, "lent")[1], "String::from: heap: lent as `&mut` with `names`");
    assert_eq!(
        decisions(&a, p, "mapped"),
        [
            format!("HashMap<i32, String>: {}", region("mapped")),
            format!("String::from: {}", region("mapped")),
            // `insert` may drop the value it replaces, so a loop keeps the heap.
            "String::from: heap: stored in `m` on every pass of a loop".to_owned(),
        ]
    );
    assert_eq!(
        decisions(&a, p, "made"),
        [
            "List<String>: result: where `made`'s caller keeps it",
            "String::from: result: where `made`'s caller keeps it",
        ]
    );
    // The list is the caller's, wherever it is.
    assert_eq!(decisions(&a, p, "tagged"), ["String::from: heap: passed to `names.push`"]);
}

/// r90's examples of a drop that only frees, worked out per type instance from the drop
/// bodies themselves (T-0118).
#[test]
fn a_drop_only_frees_when_its_body_does_nothing_but_free() {
    let a = analyse("fn main() {}\n");
    let (hir, table, res) =
        (a.hir.as_ref().unwrap(), a.table.as_ref().unwrap(), a.resolution.as_ref().unwrap());
    let d = dz_flow::release::Drops::new(hir, table, res);
    let named = |name: &str, args: Vec<dz_hir::ty::Ty>| dz_hir::ty::Ty::Named {
        sym: res.lookup(res.prelude, name).unwrap_or_else(|| panic!("no `{name}` in std")),
        args,
    };
    let int = dz_hir::ty::Ty::Int(dz_hir::ty::IntTy::I32);
    let string = named("String", vec![]);
    for (ty, frees) in [
        (named("List", vec![int.clone()]), true),
        (named("List", vec![string.clone()]), true),
        (string.clone(), true),
        (named("HashMap", vec![int.clone(), int.clone()]), true),
        (named("HashMap", vec![string.clone(), int.clone()]), true),
        (named("Box", vec![string.clone()]), true),
        // It closes a handle; it changes a count other owners see.
        (named("File", vec![]), false),
        (named("Rc", vec![int.clone()]), false),
        (named("List", vec![named("File", vec![])]), false),
        // It writes what it holds out before it goes.
        (named("Formatter", vec![]), false),
    ] {
        assert_eq!(d.only_frees(&ty), frees, "{}", dz_check::type_name(res, &ty));
    }
}

/// A drop is skipped only when the region's release frees everything the value owns: the
/// value was built whole in the region, owns nothing that came from elsewhere, and nothing
/// replaced it or a part of it since (T-0118).
#[test]
fn a_drop_is_skipped_only_when_its_region_frees_everything() {
    let a = analyse(
        r#"
struct Bag {
    pub items: List<i32>
    pub label: String

    Bag(n: i32) {
        items = List<i32>()
        for i in 0..n {
            items.push(i)
        }
        label = String::from("bag")
    }
}

struct Taken {
    pub items: List<i32>

    // Holds what it is given, which was built wherever its caller built it.
    Taken(items: List<i32>) {
        self.items = move(items)
    }
}

struct Noisy {
    pub items: List<i32>

    Noisy() {
        items = List<i32>()
    }
}

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("dropped {}", items.len())
    }
}

struct Half {
    pub built: List<i32>
    pub given: List<i32> = List<i32>()
}

fn reset(s: &mut String) kills s {
    s = String::from("on the heap")
}

fn pass_on(x: String) -> String {
    return move(x)
}

fn fresh() -> String {
    let mut s = String::from("fresh")
    s.push_str("!")
    return s
}

fn skipped() -> usize {
    let mut xs = List<i32>()
    xs.push(1)
    let s = String::from("abc")
    let mut m = HashMap<i32, i32>()
    m.insert(1, 2)
    let b = Bag(3)
    let f = fresh()
    let mut n: usize = 0
    for _ in 0..3 {
        let t = String::from("each pass")
        n += t.len()
    }
    // Everything put into it was built in its region (T-0145).
    let mut names = List<String>()
    for _ in 0..3 {
        names.push(String::from("each pass, kept"))
    }
    let one = String::from("one")
    names.push(move(one))
    return xs.len() + s.len() + m.len() + b.items.len() + f.len() + n + names.len()
}

fn kept() -> usize {
    let mut names = List<String>()
    names.push(pass_on(String::from("built on the heap")))
    let mut grown = List<String>()
    grown.push(String::from("in the region"))
    grown.resize(4)
    let key = List<i32>()
    let mut keyed = HashMap<List<i32>, i32>()
    keyed.insert(key, 1)
    let mut a = String::from("a")
    a = String::from("b")
    let mut r = String::from("r")
    reset(&mut r)
    let c = pass_on(String::from("c"))
    let t = Taken(List<i32>())
    let noisy = Noisy()
    let h = Half{ built: List<i32>() }
    return names.len() + grown.len() + keyed.len() + a.len() + r.len() + c.len() + t.items.len() +
        noisy.items.len() + h.built.len()
}

fn main() {
    println!("{} {}", skipped(), kept())
}
"#,
    );
    let p = a.placements.as_ref().unwrap();
    let skipped = |name: &str| -> Vec<String> {
        sites(&a, p, name).iter().filter(|s| s.drop_skipped).map(|s| s.callee.clone()).collect()
    };
    assert_eq!(
        skipped("skipped"),
        [
            "List<i32>",
            "String::from",
            "HashMap<i32, i32>",
            "Bag",
            "fresh",
            "String::from",
            "List<String>",
            // `one`, moved into the list: nothing is left for its drop.
            "String::from"
        ],
        "{:?}",
        decisions(&a, p, "skipped")
    );
    // Holds a string built on the heap; grown with elements `resize` builds itself; given a
    // copy of `key` as its key, built on the heap (T-0145);
    // replaced; handed on as `&mut`; built from a value the callee was given; a constructor
    // that keeps its argument; a drop that prints; a field left to its default.
    // Only `key`, which is copied from and keeps its own memory, skips its drop.
    assert_eq!(skipped("kept"), ["List<i32>"], "{:?}", decisions(&a, p, "kept"));
    // Each of them is in the region all the same: it is the drop that is kept, not the heap.
    let regions = sites(&a, p, "kept")
        .iter()
        .filter(|s| matches!(s.decision, Decision::Region { .. }))
        .count();
    assert_eq!(regions, 12, "{:?}", decisions(&a, p, "kept"));
    let labels: Vec<String> = sites(&a, p, "skipped").iter().map(|s| s.label()).collect();
    assert_eq!(labels[0], "region: `skipped`; drop skipped");
}

fn owner_module(owner: &BodyOwner, a: &Analysis) -> dz_project::ModuleId {
    let res = a.resolution.as_ref().unwrap();
    let sym = match owner {
        BodyOwner::Fn(s)
        | BodyOwner::Copy(s)
        | BodyOwner::Drop(s)
        | BodyOwner::Const(s)
        | BodyOwner::Static(s) => *s,
        BodyOwner::Method { owner, .. }
        | BodyOwner::Ctor { owner, .. }
        | BodyOwner::AssocConst { owner, .. }
        | BodyOwner::FieldDefault { owner, .. }
        | BodyOwner::TraitDefaults { owner, .. } => *owner,
        BodyOwner::ImplMethod { imp, .. } => {
            return a.table.as_ref().unwrap().impls()[*imp as usize].module;
        }
    };
    res.symbol(sym).module
}
