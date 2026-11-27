//! The aliasing matrix: every way two arguments can be, or not be, one place, passed to
//! every kind of code that reads one while writing the other (spec §6.7, §22.3, §22.4).
//!
//! Each hole found in M19 was found by asking "what if I pass the same list twice?": a
//! function's two parameters reaching one list (T-0268), a parameter inside an `Rc`'s value
//! beside another `Rc`. Each rule had a test, but no test tried every pair against every rule,
//! so this one does. A **pair** is two arguments at a call, `&mut xs` and `&xs`, and whether
//! they may be one place. A **use** is what receives them: a function that kills through the
//! first, one that only writes it, a `parallel` block or a `parallel for` writing through the
//! first while reading the second, inside a function or written in place. Each use says what
//! a pair that may alias must produce, and every pair that cannot must be accepted.
//!
//! A combination the compiler gets wrong today is listed in `KNOWN`, with its task: the test
//! holds it to today's behaviour, so the day it is fixed the test says to take it off.

mod parallel;

/// The program every case is written into: places of each shape, all `List<i32>`.
const SETUP: &str = "\
struct Pair {
    pub left: List<i32>
    pub right: List<i32>
}

fn blen(b: &List<i32>) -> i32 {
    return b.len() as i32
}

fn rlen(b: &Rc<List<i32>>) -> i32 {
    if let &l = b {
        return l.len() as i32
    }
    return 0
}

fn bump(a: &mut List<i32>) {
    if a.len() > 0 {
        a[0] += 1
    }
}

fn grow(a: &mut List<i32>) kills a@items {
    a.push(1)
}

fn count(b: &List<i32>, out: &mut i32) {
    out = blen(b)
}

fn rcount(b: &Rc<List<i32>>, out: &mut i32) {
    out = rlen(b)
}
";

/// What a use's second argument is: a list, or an `Rc` of one.
#[derive(Clone, Copy, PartialEq, Debug)]
enum B {
    List,
    Rc,
}

impl B {
    fn ty(self) -> &'static str {
        match self {
            B::List => "&List<i32>",
            B::Rc => "&Rc<List<i32>>",
        }
    }
    fn len(self) -> &'static str {
        match self {
            B::List => "blen",
            B::Rc => "rlen",
        }
    }
    /// Opens a block in which `l` is the list `b` reaches.
    fn open(self) -> &'static str {
        match self {
            B::List => "if true {\n        let l = b",
            B::Rc => "if let &l = b {",
        }
    }
    fn count(self) -> &'static str {
        match self {
            B::List => "count",
            B::Rc => "rcount",
        }
    }
}

/// `(name, first argument, second argument, its kind, whether they may be one place, the
/// statement the call is written inside, if any)`.
type Pair = (&'static str, &'static str, &'static str, B, bool, Option<&'static str>);

const PAIRS: &[Pair] = &[
    ("same_local", "xs", "xs", B::List, true, None),
    ("two_locals", "xs", "ys", B::List, false, None),
    ("same_field", "p.left", "p.left", B::List, true, None),
    ("two_fields", "p.left", "p.right", B::List, false, None),
    ("same_element", "ls[0]", "ls[0]", B::List, true, None),
    ("two_elements", "ls[0]", "ls[1]", B::List, false, None),
    ("elements_by_index", "ls[i]", "ls[j]", B::List, true, None),
    // `v` is the value inside `rc`; `rc2` is another `Rc` to it, which nothing at compile time
    // can tell from one to another value (spec §14.5).
    ("rc_value_and_its_rc", "v", "rc", B::Rc, true, Some("if let &mut v = rc")),
    ("rc_value_and_another_rc", "v", "rc2", B::Rc, true, Some("if let &mut v = rc")),
    ("local_and_an_rc", "xs", "rc2", B::Rc, false, None),
];

/// What a pair that may alias must produce in a use.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Expect {
    /// Refused, with any of these codes.
    Refused(&'static [&'static str]),
    /// Accepted: one thread reads after another writes, which D-0094 allows.
    Accepted,
}

/// `(name, the function it declares, taking `a` and `b`, or `None` when the use is written in
/// place; the statements that use the two arguments; what a pair that may alias produces)`.
/// `A` and `B` in the statements are the two arguments, `F` the function's name.
struct Use {
    name: &'static str,
    function: Option<&'static str>,
    call: &'static str,
    alias: Expect,
}

const USES: &[Use] = &[
    // One thread: a kill through `a` ends what `b` points at (DZ0420).
    Use {
        name: "kills",
        function: Some("fn F(a: &mut List<i32>, b: BT) kills a@items {\n    a.push(BLEN(b))\n}"),
        call: "F(&mut A, &B)",
        alias: Expect::Refused(&["DZ0420"]),
    },
    // One thread: a view through `b` kept across the kill would point at freed memory, which
    // only the caller can see; the function itself is right for every `a` apart from `b`.
    Use {
        name: "kills_keeping_a_view",
        function: Some("fn F(a: &mut List<i32>, b: BT) kills a@items {\n    OPEN\n        if l.len() > 0 {\n            let first = &l[0]\n            a.push(1)\n            a[0] = first\n        }\n    }\n}"),
        call: "F(&mut A, &B)",
        alias: Expect::Refused(&["DZ0420"]),
    },
    // One thread: a write that kills nothing, beside a read, is allowed (D-0094).
    Use {
        name: "writes",
        function: Some("fn F(a: &mut List<i32>, b: BT) {\n    if a.len() > 0 {\n        a[0] = BLEN(b)\n    }\n}"),
        call: "F(&mut A, &B)",
        alias: Expect::Accepted,
    },
    // Two threads, in place: one call writes what the other reads (DZ0424).
    Use {
        name: "block_in_place",
        function: None,
        call: "let mut n = 0\nparallel {\n    bump(&mut A)\n    COUNT(&B, &mut n)\n}",
        alias: Expect::Refused(&["DZ0424"]),
    },
    // The same, the calls inside a function given the two: its `lends a` keeps them apart at
    // the call (DZ0434, T-0268).
    Use {
        name: "block_in_function",
        function: Some("fn F(a: &mut List<i32>, b: BT) lends a {\n    let mut n = 0\n    parallel {\n        bump(a)\n        COUNT(b, &mut n)\n    }\n}"),
        call: "F(&mut A, &B)",
        alias: Expect::Refused(&["DZ0434"]),
    },
    // A block that grows the list on another thread: its `kills` keeps it apart already.
    Use {
        name: "growing_block_in_function",
        function: Some("fn F(a: &mut List<i32>, b: BT) kills a@items lends a {\n    let mut n = 0\n    parallel {\n        grow(a)\n        COUNT(b, &mut n)\n    }\n}"),
        call: "F(&mut A, &B)",
        alias: Expect::Refused(&["DZ0420"]),
    },
    // Passes writing their elements while each reads the other list, in place (DZ0433).
    Use {
        name: "loop_in_place",
        function: None,
        call: "parallel for &mut x in A {\n    x += BLEN(&B)\n}",
        alias: Expect::Refused(&["DZ0433", "DZ0424"]),
    },
    // The same inside a function given the two: the passes write what `a` reaches, so the
    // function `lends a` though nothing grows (T-0268).
    Use {
        name: "loop_in_function",
        function: Some("fn F(a: &mut List<i32>, b: BT) lends a {\n    parallel for &mut x in a {\n        x += BLEN(b)\n    }\n}"),
        call: "F(&mut A, &B)",
        alias: Expect::Refused(&["DZ0434"]),
    },
];

/// Combinations the compiler gets wrong today, by `use/pair`, with the task that fixes each.
/// Each is held to what it does now, so fixing one fails the test until it is taken off.
// None today: T-0268, T-0272 and T-0273, which it found, are fixed.
const KNOWN: &[(&str, &str)] = &[];

/// The program for one use of one pair.
fn program(u: &Use, pair: &Pair) -> String {
    let (_, a, b, kind, _, wrap) = *pair;
    let fill = |s: &str| {
        s.replace("BLEN", kind.len())
            .replace("COUNT", kind.count())
            .replace("OPEN", kind.open())
            .replace("BT", kind.ty())
            .replace('F', "use_them")
    };
    let mut call = fill(u.call).replace('A', a).replace('B', b);
    if let Some(open) = wrap {
        let body: String = call.lines().map(|l| format!("    {l}\n")).collect();
        call = format!("{open} {{\n{body}}}");
    }
    let body: String = call.lines().map(|l| format!("    {l}\n")).collect();
    let function = u.function.map(|f| format!("{}\n", fill(f))).unwrap_or_default();
    format!(
        "{SETUP}\n{function}\nfn main() {{\n    let mut xs = List<i32>()\n    xs.push(1)\n    \
         let mut ys = List<i32>()\n    ys.push(2)\n    let mut p = Pair{{ left: List<i32>(), \
         right: List<i32>() }}\n    let mut ls = List<List<i32>>()\n    ls.push(List<i32>())\n    \
         ls.push(List<i32>())\n    let i: usize = 0\n    let j: usize = 1\n    let mut rc = \
         Rc<List<i32>>(List<i32>())\n    let rc2 = rc.clone()\n{body}}}\n"
    )
}

/// The error codes analysing `code` produces, in order.
fn errors(dir: &std::path::Path, code: &str) -> (Vec<&'static str>, String) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"aliasing\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { \
         open = true }\n\n[build]\noutput = \"executable\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.dz"), code).unwrap();
    let analysis = dz_driver::analyse(dir);
    let got = analysis
        .diags
        .iter()
        .filter(|d| d.severity == dz_diag::Severity::Error)
        .filter_map(|d| d.code)
        .collect();
    let rendered = dz_diag::render_to_string(&analysis.map, &analysis.diags);
    let _ = std::fs::remove_dir_all(dir);
    (got, rendered)
}

#[test]
fn every_pair_is_kept_apart_where_a_rule_needs_it() {
    let cases: Vec<(&Use, &Pair)> =
        USES.iter().flat_map(|u| PAIRS.iter().map(move |p| (u, p))).collect();
    let count = cases.len();
    let mut failures = parallel::filter_map(&cases, |&(u, pair)| {
        let name = format!("{}/{}", u.name, pair.0);
        let dir = std::env::temp_dir().join(format!("dz_aliasing_{}_{}", u.name, pair.0));
        let code = program(u, pair);
        let (got, rendered) = errors(&dir, &code);
        let wanted = if pair.4 { u.alias } else { Expect::Accepted };
        let right = match wanted {
            Expect::Accepted => got.is_empty(),
            Expect::Refused(codes) => !got.is_empty() && got.iter().all(|c| codes.contains(c)),
        };
        let known = KNOWN.iter().find(|(k, _)| *k == name).map(|(_, task)| *task);
        match (right, known) {
            (true, None) => None,
            (false, Some(_)) => None,
            (true, Some(task)) => Some(format!(
                "{name}: now right ({wanted:?}); {task} fixed it, so take it off `KNOWN`"
            )),
            (false, None) => {
                Some(format!("{name}: expected {wanted:?}, got {got:?}\n{code}\n{rendered}"))
            }
        }
    });
    failures.sort();
    assert!(
        failures.is_empty(),
        "{} of {count} cases failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
