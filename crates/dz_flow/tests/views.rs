//! Which loops index a `List` through one view taken before them (T-0215,
//! `dz_flow::views`): one function of [`SOURCE`] per rule, each named for what it shows,
//! and the lists its loops view, outermost loop first.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use dz_driver::Analysis;
use dz_flow::views::loop_views;
use dz_hir::{BodyOwner, StmtId};

const SOURCE: &str = r#"
// Read through a `&List`: viewed.
fn reads(xs: &List<i32>) -> i32 {
    let mut t = 0
    for i in 0..xs.len() {
        t += xs[i]
    }
    return t
}

// Written through a `&mut List`, read through a `&List`: both viewed, the first writable.
fn writes(dst: &mut List<i32>, src: &List<i32>) {
    for i in 0..src.len() {
        dst[i] = src[i] * 3
    }
}

// A `while` reads the length each pass: viewed.
fn counts_while(xs: &List<i32>) -> i32 {
    let mut t = 0
    let mut i: usize = 0
    while i < xs.len() {
        t += xs[i]
        i += 1
    }
    return t
}

// The loop grows the list, which kills `xs@items`: not viewed.
fn grows(xs: &mut List<i32>) kills xs@items {
    for i in 0..xs.len() {
        xs.push(xs[i])
    }
}

// The list is grown through a reference to it: the kill is recorded against `xs`, so not
// viewed.
fn through_reference() -> i32 {
    let mut xs = List<i32>()
    xs.push(1)
    let r = &mut xs
    let mut t = 0
    for i in 0..3 {
        t += xs[0]
        r.push(i)
    }
    return t
}

// An owned list, written in place: viewed.
fn owned(n: usize) -> i32 {
    let mut xs = List<i32>()
    for i in 0..n {
        xs.push(i as i32)
    }
    for i in 0..n {
        xs[i] = xs[i] * 2
    }
    return xs[0]
}

// Nested loops: the outer one takes the view, and the inner one reads through it.
fn nested(grid: &List<i32>, w: usize, h: usize) -> i32 {
    let mut t = 0
    for y in 0..h {
        for x in 0..w {
            t += grid[y * w + x]
        }
    }
    return t
}

// The outer loop grows the list between inner loops: only the inner loop is viewed.
fn grown_between(xs: &mut List<i32>) kills xs@items {
    for round in 0..3 {
        for i in 0..xs.len() {
            xs[i] += 1
        }
        xs.push(round)
    }
}

// A list made inside the outer loop does not exist before it: only the inner loop.
fn made_inside(n: usize) -> i32 {
    let mut t = 0
    for round in 0..3 {
        let mut xs = List<i32>()
        xs.push(round)
        for i in 0..xs.len() {
            t += xs[i]
        }
    }
    return t + n as i32
}

// An element's own text grows, which kills `names[*]@text`: refused, since any kill of the
// list is taken to end the view.
fn element_grows(names: &mut List<String>) kills names[*]@text {
    for i in 0..names.len() {
        names[i].push_str("!")
    }
}

// A reference bound in the body is left out, whatever happens in the loop.
fn bound_reference() -> i32 {
    let mut xs = List<i32>()
    xs.push(1)
    let r = &xs
    let mut t = 0
    for i in 0..r.len() {
        t += r[i]
    }
    return t
}

fn main() {
    let mut xs = List<i32>()
    xs.push(1)
    let mut ys = List<i32>()
    ys.push(2)
    writes(&mut ys, &xs)
    grows(&mut ys)
    grown_between(&mut ys)
    let mut names = List<String>()
    names.push(String::from("a"))
    element_grows(&mut names)
    println!("{} {} {} {} {} {} {}", reads(&xs), counts_while(&xs), through_reference(), owned(3), nested(&xs, 1, 1), made_inside(1), bound_reference())
}
"#;

fn scratch() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "dz_flow_views_{}_{}",
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
        "[module]\nname = \"views\"\nsources = [\"src/**/*.dz\"]\n\n\
         [dependencies]\nstd = { open = true }\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.dz"), source).unwrap();
    let a = dz_driver::analyse(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!a.diags.has_errors(), "{}", dz_diag::render_to_string(&a.map, &a.diags));
    a
}

/// The lists each loop of `name` views, outermost first, as `xs` or `mut xs`, one entry per
/// loop that views any.
fn viewed(a: &Analysis, name: &str) -> Vec<String> {
    let res = a.resolution.as_ref().unwrap();
    let hir = a.hir.as_ref().unwrap();
    let table = a.table.as_ref().unwrap();
    let (_, body) = hir
        .bodies()
        .find(|(_, b)| {
            matches!(b.owner, BodyOwner::Fn(s) if res.symbol(s).name == name
                && res.symbol(s).module != res.prelude)
        })
        .unwrap_or_else(|| panic!("no function `{name}`"));
    let views = loop_views(body, table, res);
    let mut loops: Vec<StmtId> = (0..body.stmts.len() as u32).map(StmtId).collect();
    loops.sort_by_key(|s| body.stmt(*s).span.start);
    loops
        .into_iter()
        .filter(|s| !views.at(*s).is_empty())
        .map(|s| {
            let mut names: Vec<String> = views
                .at(s)
                .iter()
                .map(|h| {
                    let n = &body.local(h.local).name;
                    if h.mutable {
                        format!("mut {n}")
                    } else {
                        n.clone()
                    }
                })
                .collect();
            names.sort();
            names.join(", ")
        })
        .collect()
}

#[test]
fn a_loop_that_kills_nothing_views_its_lists() {
    let a = analyse(SOURCE);
    assert_eq!(viewed(&a, "reads"), ["xs"]);
    assert_eq!(viewed(&a, "writes"), ["mut dst, src"]);
    assert_eq!(viewed(&a, "counts_while"), ["xs"]);
    assert_eq!(viewed(&a, "owned"), ["mut xs"]);
}

#[test]
fn a_loop_that_may_kill_the_list_does_not_view_it() {
    let a = analyse(SOURCE);
    assert_eq!(viewed(&a, "grows"), Vec::<String>::new());
    assert_eq!(viewed(&a, "through_reference"), Vec::<String>::new());
    assert_eq!(viewed(&a, "element_grows"), Vec::<String>::new());
    assert_eq!(viewed(&a, "bound_reference"), Vec::<String>::new());
}

#[test]
fn the_outermost_loop_that_can_takes_the_view() {
    let a = analyse(SOURCE);
    // One view, taken by the outer loop; the inner loop reads through it.
    assert_eq!(viewed(&a, "nested"), ["grid"]);
    // The outer loop kills the list, or makes it, so the inner one takes the view.
    assert_eq!(viewed(&a, "grown_between"), ["mut xs"]);
    assert_eq!(viewed(&a, "made_inside"), ["mut xs"]);
}

/// `List`'s own bodies write `m_len` themselves, which changes the length without a kill,
/// so none of them views anything.
#[test]
fn lists_own_bodies_view_nothing() {
    let a = analyse(SOURCE);
    let res = a.resolution.as_ref().unwrap();
    let hir = a.hir.as_ref().unwrap();
    let table = a.table.as_ref().unwrap();
    let list = res.lookup(res.prelude, "List").unwrap();
    let mut seen = 0;
    for (_, body) in hir.bodies() {
        let own = body.params.iter().any(|p| {
            let l = body.local(*p);
            l.name == "self"
                && matches!(l.ty.peeled(), dz_hir::ty::Ty::Named { sym, .. } if *sym == list)
        });
        if own {
            seen += 1;
            assert!(loop_views(body, table, res).is_empty(), "{:?} views a list", body.owner);
        }
    }
    assert!(seen > 10, "found only {seen} of List's bodies");
}
