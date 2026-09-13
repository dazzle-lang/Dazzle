//! The feature matrix, shared by `dz_driver/tests/matrix.rs` (every mistake, analysed) and
//! `dz/tests/matrix.rs` (every valid construct, compiled and run).
//!
//! Most bugs found in this compiler have not been in a feature but where two meet: a
//! `break` inside a `switch` inside a loop, a constant used by a field default, a `case`
//! binding behind a `&`. So instead of hand-picking combinations, every **construct** is
//! placed in every **context**, and every program is built twice — with its types declared
//! beside it, and in another module. A construct that works in one place and not another
//! then fails with the construct and the place in its name.
//!
//! A construct is a statement list that leaves a known value in `r`, declared as `let mut r
//! = 0` by the context. A mistake is a statement list that must produce exactly one
//! diagnostic, wherever it is put.

// Each test file uses half of this: one the constructs, the other the mistakes.
#![allow(dead_code)]

/// Declarations every case may use. `pub`, so they can live in another module unchanged.
pub const DECLS: &str = "\
pub enum Step {
    Go
    Stop
}

pub enum Ctx {
    Go
    Stop
}

pub union Shape {
    Circle { radius: i32 }
    Point
}

pub union Choice<T> {
    One { value: T }
    Neither
}

pub struct Res {
    pub n: i32
}

impl Drop for Res {
    fn drop(&mut self) {}
}

pub struct Pt {
    pub x: i32
    pub y: i32
}

pub const LIMIT = 10
pub const NAME: str = \"go\"

pub fn none_i32() -> i32? {
    return none
}

pub fn some_i32() -> i32? {
    return 5
}

pub fn fails() !-> i32 {
    return error(\"no\")
}

pub fn take(r: Res) -> i32 {
    return r.n
}

pub fn keep<T>(x: T) -> T {
    return move(x)
}

pub fn sum(xs: []i32) -> i32 {
    let mut t = 0
    for x in xs {
        t += x
    }
    return t
}

pub fn add_to(x: &mut i32, v: i32) {
    x += v
}

pub fn pair() -> (i32, i32) {
    return (3, 9)
}

pub struct Counter {
    pub n: i32

    pub fn bump(&mut self) {
        n += 1
    }
}
";

/// The names `DECLS` exports, for a selective import from the other module.
pub const EXPORTS: &str = "Step, Ctx, Shape, Choice, Res, Pt, LIMIT, NAME, none_i32, some_i32, \
                           fails, take, keep, sum, add_to, pair, Counter";

/// `(name, body, value left in r)`.
pub const CONSTRUCTS: &[(&str, &str, i32)] = &[
    ("enum_switch", "let s = Step::Stop\nswitch s {\n    case Go { r = 1 }\n    case Stop { r = 2 }\n}", 2),
    (
        "int_switch",
        "let n = 11\nr = switch n {\n    case 1, 2 { 0 }\n    case LIMIT + 1 { 7 }\n    default { 9 }\n}",
        7,
    ),
    (
        "str_switch",
        "let w: str = \"go\"\nswitch w {\n    case \"stop\" { r = 1 }\n    case NAME { r = 3 }\n    default { r = 4 }\n}",
        3,
    ),
    (
        "union_copy_binding",
        "let s = Shape::Circle{ radius: 4 }\nswitch s {\n    case Circle(c) { r = c.radius }\n    case Point { r = 0 - 1 }\n}",
        4,
    ),
    (
        "union_mut_binding",
        "let mut s = Shape::Circle{ radius: 4 }\nswitch s {\n    case Circle(&mut c) { c.radius += 1 }\n    case Point {}\n}\nswitch s {\n    case Circle(&c) { r = c.radius }\n    default {}\n}",
        5,
    ),
    ("or_value", "r = none_i32() or 6", 6),
    ("or_block", "r = fails() or { _e ->\n    7\n}", 7),
    ("if_let", "if let v = some_i32() {\n    r = v + 3\n} else {\n    r = 0\n}", 8),
    ("for_copy", "let xs = [1, 2, 6]\nfor x in xs {\n    r += x\n}", 9),
    (
        "for_mut",
        "let mut xs = [1, 2, 3]\nfor &mut x in xs {\n    x *= 2\n}\nfor x in xs {\n    r += x\n}\nr -= 2",
        10,
    ),
    ("for_indexed", "let xs = [5, 5, 5]\nfor i, &x in xs.indexed() {\n    r += i as i32 + x\n}\nr -= 4", 14),
    ("for_tuple", "let ps = [(1, 2), (3, 4)]\nfor a, &b in ps {\n    r += a * b\n}", 14),
    (
        "generic_union_inferred",
        "let c = Choice::One{ value: 13 }\nswitch c {\n    case One(o) { r = o.value }\n    case Neither {}\n}",
        13,
    ),
    ("block_if", "let flag = true\nr = if flag { 14 } else { 0 }", 14),
    ("move_only_by_ref", "let rs = [Res{ n: 7 }, Res{ n: 8 }]\nfor &x in rs {\n    r += x.n\n}", 15),
    ("move_into_call", "let res = Res{ n: 16 }\nr = take(move(res))", 16),
    (
        "struct_field_through_mut",
        "let mut pts = [Pt{ x: 1, y: 2 }]\nfor &mut p in pts {\n    p.x += 16\n}\nr = pts[0].x",
        17,
    ),
    (
        "break_and_continue",
        "for i in 0..10 {\n    if i == 2 {\n        continue\n    }\n    if i == 5 {\n        break\n    }\n    r += i\n}",
        8,
    ),
    (
        "break_in_switch_in_loop",
        "let steps = [Step::Go, Step::Go, Step::Stop, Step::Go]\nfor &s in steps {\n    switch s {\n        case Go { r += 1 }\n        case Stop { break }\n    }\n}",
        2,
    ),
    (
        "or_break_in_loop",
        "for i in 0..5 {\n    let o = if i < 3 { some_i32() } else { none_i32() }\n    let v = o or break\n    r += v\n}",
        15,
    ),
    ("defer_in_block", "{\n    defer r += 1\n    r = 20\n}", 21),
    (
        "generic_union_of_struct_by_ref",
        "let c = Choice::One{ value: Pt{ x: 3, y: 4 } }\nswitch c {\n    case One(&o) { r = o.value.x * o.value.y }\n    case Neither {}\n}",
        12,
    ),
    (
        "switch_on_copy_in_loop",
        "let shapes = [Shape::Circle{ radius: 2 }, Shape::Point, Shape::Circle{ radius: 3 }]\nfor s in shapes {\n    switch s {\n        case Circle(c) { r += c.radius }\n        case Point { r += 10 }\n    }\n}",
        15,
    ),
    (
        "labelled_break_and_continue",
        "outer: for i in 0..3 {\n    for j in 0..3 {\n        if j == 1 {\n            continue outer\n        }\n        if i == 2 {\n            break outer\n        }\n        r += 1\n    }\n}",
        2,
    ),
    (
        "switch_in_switch",
        "let s = Step::Go\nlet n = 2\nswitch s {\n    case Go {\n        switch n {\n            case 2 { r = 22 }\n            default { r = 0 }\n        }\n    }\n    case Stop {}\n}",
        22,
    ),
    ("loop_in_loop", "let grid = [[1, 2], [3, 4]]\nfor &row in grid {\n    for x in row {\n        r += x\n    }\n}", 10),
    ("array_as_slice_argument", "let xs = [4, 5]\nr = sum(xs)", 9),
    (
        "method_through_mut_element",
        "let mut cs = [Counter{ n: 1 }, Counter{ n: 2 }]\nfor &mut c in cs {\n    c.bump()\n}\nfor &c in cs {\n    r += c.n\n}",
        5,
    ),
    ("optional_compared_to_none", "let o = none_i32()\nif o == none {\n    r = 3\n}", 3),
    ("generic_call_with_move_only", "let a = keep(Res{ n: 4 })\nr = a.n", 4),
    (
        "switch_value_in_loop",
        "for i in 0..4 {\n    r += switch i {\n        case 0 { 1 }\n        case 1, 2 { 10 }\n        default { 100 }\n    }\n}",
        121,
    ),
    ("mut_reference_argument", "add_to(&mut r, 30)", 30),
    (
        "labelled_while_continue",
        "let mut i = 0\nouter: while i < 3 {\n    i += 1\n    for j in 0..3 {\n        if j == 1 {\n            continue outer\n        }\n        r += 1\n    }\n}",
        3,
    ),
    (
        "or_break_outer",
        "outer: for i in 0..3 {\n    for j in 0..2 {\n        let o = if i == 1 { none_i32() } else { some_i32() }\n        let v = o or break outer\n        r += v + j\n    }\n}",
        11,
    ),
    (
        "break_outer_from_switch",
        "let steps = [Step::Go, Step::Stop]\nouter: for k in 0..3 {\n    for &s in steps {\n        switch s {\n            case Go { r += 1 }\n            case Stop {\n                if k == 1 {\n                    break outer\n                }\n            }\n        }\n    }\n}",
        2,
    ),
    (
        "sibling_loops_share_a_label",
        "a: for _i in 0..2 {\n    for j in 0..2 {\n        if j == 1 {\n            continue a\n        }\n        r += 1\n    }\n}\na: for _i in 0..2 {\n    for j in 0..2 {\n        if j == 1 {\n            break a\n        }\n        r += 10\n    }\n}",
        12,
    ),
    ("tuple_destructured_let", "let (a, b) = pair()\nr = a * b", 27),
];

/// `(name, body, the one diagnostic it must produce)`.
pub const MISTAKES: &[(&str, &str, &str)] = &[
    ("write_to_copy_binding", "let xs = [1]\nfor x in xs {\n    x = 2\n}", "DZ0507"),
    ("mut_binding_on_read_only", "let xs = [1]\nfor &mut x in xs {\n    x = 2\n}", "DZ0507"),
    (
        "write_to_case_copy",
        "let s = Shape::Point\nswitch s {\n    case Circle(c) { c.radius = 1 }\n    default {}\n}",
        "DZ0507",
    ),
    (
        "case_mut_on_immutable",
        "let s = Shape::Point\nswitch s {\n    case Circle(&mut c) { c.radius = 1 }\n    default {}\n}",
        "DZ0507",
    ),
    ("non_exhaustive_switch", "let s = Step::Go\nswitch s {\n    case Go {}\n}", "DZ0513"),
    ("duplicate_case", "let n = 1\nswitch n {\n    case 1 {}\n    case 1 {}\n    default {}\n}", "DZ0548"),
    ("function_as_case_label", "let n = 1\nswitch n {\n    case some_i32 {}\n    default {}\n}", "DZ0501"),
    ("run_time_case_label","let n = 1\nlet m = 2\nswitch n {\n    case m {}\n    default {}\n}", "DZ0547"),
    ("use_after_move", "let a = Res{ n: 1 }\nlet b = take(move(a))\nr = a.n + b", "DZ0401"),
    ("copy_move_only_element", "let rs = [Res{ n: 1 }]\nfor x in rs {\n    r = x.n\n}", "DZ0403"),
    ("reference_to_a_range_value", "for &i in 0..2 {\n    r = i\n}", "DZ0549"),
    ("fallible_not_consumed", "fails()", "DZ0510"),
    ("fallible_not_consumed_in_if", "if r == 0 {\n    fails()\n} else {\n    r = 1\n}", "DZ0510"),
    ("float_into_int", "let f: f32 = 1\nr = f", "DZ0501"),
    ("variant_without_arguments", "let c = Choice::Neither", "DZ0534"),
    ("names_on_a_non_tuple", "let xs = [1]\nfor a, b in xs {\n    r = a + b\n}", "DZ0549"),
    ("pass_count_is_usize", "let xs = [1]\nfor i, x in xs.indexed() {\n    r = i + x\n}", "DZ0514"),
    ("condition_not_bool", "if r {\n    r = 1\n}", "DZ0508"),
    ("assign_to_immutable", "let q = 1\nq = 2\nr = q", "DZ0507"),
    ("assign_through_shared_reference", "let q = 1\nlet p = &q\np = 2\nr = q", "DZ0507"),
    ("missing_field", "let p = Pt{ x: 1 }\nr = p.x", "DZ0512"),
    ("unknown_method", "let p = Pt{ x: 1, y: 2 }\np.nope()", "DZ0520"),
    ("wrong_argument_count", "r = take()", "DZ0504"),
    ("mutating_method_on_copy_binding", "let cs = [Counter{ n: 1 }]\nfor c in cs {\n    c.bump()\n}", "DZ0507"),
    ("mut_reference_to_immutable", "let q = 1\nadd_to(&mut q, 1)", "DZ0507"),
    ("break_to_unknown_label", "for i in 0..2 {\n    r += i\n    break nowhere\n}", "DZ0550"),
    ("continue_to_unknown_label", "while r < 2 {\n    r += 1\n    continue nowhere\n}", "DZ0550"),
];

/// `(name, wrapper)`: `BODY` is replaced by the case, indented to fit, and `NAME` by a
/// unique function name. Each defines `fn NAME() -> i32` returning `r`.
pub const CONTEXTS: &[(&str, &str)] = &[
    ("function", "fn NAME() -> i32 {\n    let mut r = 0\n    BODY\n    return r\n}"),
    (
        "method",
        "struct NAME_s {\n    pub base: i32\n\n    pub fn run(&mut self) -> i32 {\n        let mut r = 0\n        BODY\n        return r + base\n    }\n}\n\nfn NAME() -> i32 {\n    let mut s = NAME_s{ base: 0 }\n    return s.run()\n}",
    ),
    (
        "generic_function",
        "fn NAME_g<T: Copy>(_t: T) -> i32 {\n    let mut r = 0\n    BODY\n    return r\n}\n\nfn NAME() -> i32 {\n    return NAME_g(1.5)\n}",
    ),
    ("loop_body", "fn NAME() -> i32 {\n    let mut r = 0\n    for _pass in 0..1 {\n        BODY\n    }\n    return r\n}"),
    (
        "switch_arm",
        "fn NAME() -> i32 {\n    let mut r = 0\n    let ctx = Ctx::Go\n    switch ctx {\n        case Go {\n            BODY\n        }\n        case Stop {}\n    }\n    return r\n}",
    ),
    (
        "or_block",
        "fn NAME() -> i32 {\n    let mut r = 0\n    let _v = none_i32() or {\n        BODY\n        0\n    }\n    return r\n}",
    ),
    (
        "if_let",
        "fn NAME() -> i32 {\n    let mut r = 0\n    if let _x = some_i32() {\n        BODY\n    }\n    return r\n}",
    ),
    ("unsafe_block", "fn NAME() -> i32 {\n    let mut r = 0\n    unsafe {\n        BODY\n    }\n    return r\n}"),
    ("nested_block", "fn NAME() -> i32 {\n    let mut r = 0\n    {\n        {\n            BODY\n        }\n    }\n    return r\n}"),
    (
        "while_body",
        "fn NAME() -> i32 {\n    let mut r = 0\n    let mut once = true\n    while once {\n        once = false\n        BODY\n    }\n    return r\n}",
    ),
    (
        "constructor",
        "struct NAME_s {\n    pub v: i32\n\n    NAME_s() {\n        let mut r = 0\n        BODY\n        v = r\n    }\n}\n\nfn NAME() -> i32 {\n    let s = NAME_s()\n    return s.v\n}",
    ),
    (
        "trait_method",
        "trait NAME_t {\n    fn run(&self) -> i32\n}\n\nstruct NAME_s {\n    pub z: i32\n}\n\nimpl NAME_t for NAME_s {\n    fn run(&self) -> i32 {\n        let mut r = 0\n        BODY\n        return r + z\n    }\n}\n\nfn NAME() -> i32 {\n    let s = NAME_s{ z: 0 }\n    return s.run()\n}",
    ),
    (
        "generic_struct_method",
        "struct NAME_s<T> {\n    pub t: T\n\n    pub fn run(&self) -> i32 {\n        let mut r = 0\n        BODY\n        return r\n    }\n}\n\nfn NAME() -> i32 {\n    let s = NAME_s<f32>{ t: 1.5 }\n    return s.run()\n}",
    ),
];

/// One case placed in one context: the Dazzle source of `fn name() -> i32`.
pub fn place(context: &str, name: &str, body: &str) -> String {
    let (line, _) = context
        .lines()
        .find(|l| l.trim() == "BODY")
        .map(|l| (l, ()))
        .expect("every context has a BODY line");
    let indent = &line[..line.len() - line.trim_start().len()];
    let body: Vec<String> = body
        .lines()
        .map(|l| if l.is_empty() { String::new() } else { format!("{indent}{l}") })
        .collect();
    // The name first, so a case that uses the constant `NAME` keeps it.
    context.replace("NAME", name).replace(line, &body.join("\n"))
}

/// Where the shared declarations go.
#[derive(Clone, Copy, Debug)]
pub enum Layout {
    /// Beside the cases, in the same file.
    SameModule,
    /// In a module of their own that the cases import.
    OtherModule,
}

pub const LAYOUTS: &[Layout] = &[Layout::SameModule, Layout::OtherModule];

/// Write a project whose `src/main.dz` holds `code` after the imports, with the shared
/// declarations placed as `layout` says. Returns the project's root.
pub fn write_project(dir: &std::path::Path, layout: Layout, code: &str) -> std::path::PathBuf {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let main = match layout {
        Layout::SameModule => {
            std::fs::write(
                dir.join("dazzle.toml"),
                "[module]\nname = \"matrix\"\nsources = [\"src/**/*.dz\"]\n\n[build]\noutput = \"executable\"\n",
            )
            .unwrap();
            format!("{DECLS}\n{code}")
        }
        Layout::OtherModule => {
            let lib = dir.join("libs/shared");
            std::fs::create_dir_all(lib.join("src")).unwrap();
            std::fs::write(
                lib.join("dazzle.toml"),
                "[module]\nname = \"shared\"\nsources = [\"src/**/*.dz\"]\n\n[build]\noutput = \"static-lib\"\n",
            )
            .unwrap();
            std::fs::write(lib.join("src/shared.dz"), DECLS).unwrap();
            std::fs::write(
                dir.join("dazzle.toml"),
                "[module]\nname = \"matrix\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nshared = { path = \"libs/shared\" }\n\n[build]\noutput = \"executable\"\n",
            )
            .unwrap();
            format!("import shared::{{{EXPORTS}}}\n\n{code}")
        }
    };
    std::fs::write(dir.join("src/main.dz"), main).unwrap();
    dir.to_path_buf()
}
