#!/usr/bin/env python3
"""Fuzz the ties check against the sanitizers (T-0134, D-0094).

Writes small programs out of what the ties check reasons about (spec §6.7): views into
lists, strings, hash maps, a `Box`, two `Rc`s to one list, a union's payload and a struct's
fields, taken directly, through a call that returns one, by `if let`, by `switch` and by a
`for` loop; then calls and writes that kill memory, directly, through a helper, through two
arguments, or on something else; then uses of the views. Every program goes through
`dz check`, and

- **an accepted program** is built under AddressSanitizer and UBSan and run. One the
  sanitizers flag is a hole in the checker: safe code that reached freed memory.
- **a program refused only by the ties check** is built anyway, through a debug `dz`'s
  `DZ_FUZZ_ALLOW`, and run. One that runs clean is a refusal that bought nothing in this
  run; how many there are is what a rule costs in freedom (T-0123).

Every value is placed on the heap (`DZ_REGIONS=0`), where AddressSanitizer sees a free; a
scope region keeps freed memory mapped. Every kill grows its container far enough to move
it, so a kill the checker counts is one that happens.

    cargo build -p dz
    python tools/fuzz_ties.py                    # 200 cases, seed 1
    python tools/fuzz_ties.py --cases 1000 --seed 7 --jobs 16
    python tools/fuzz_ties.py --cxx g++

Each case is one function of a batch program, so a batch costs two builds, not one per
case. Findings are written to `target/fuzz_ties/findings/` (`--work` moves it), one program each,
and the run exits 1 if any accepted program was flagged or failed to build. Linux or macOS
with clang++ (or g++) and its sanitizer runtimes; tooling only, so its findings land in
`tests/flow` and `tests/errors` and its history in `docs/log/`.
"""

import argparse
import json
import os
import random
import re
import shutil
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORK = ROOT / "target" / "fuzz_ties"
SANITIZE = "-fsanitize=address,undefined -fno-sanitize-recover=all -fno-omit-frame-pointer -g -O1"

# What the ties check refuses that a run can show to be right or wrong. A refusal for any
# other reason means the generator wrote something it should not have.
TIES = {"DZ0407", "DZ0413", "DZ0414", "DZ0420"}

MANIFEST = """[module]
name    = "fuzz"
sources = ["src/**/*.dz"]

[dependencies]
std = { open = true }

[build]
output = "executable"
"""

# The types and helpers every case may use. The helpers' clauses are what `dz fix` writes.
PRELUDE = """const LONG: const &str = "text long enough to live on the heap rather than in place"
const SHORT: const &str = "tag"

pub struct Node {
    pub name: String
}

pub struct Holder {
    pub node: Box<Node> = null
}

pub struct World {
    pub items: List<i32>
    pub others: List<i32>

    World() {
        items = List<i32>()
        others = List<i32>()
    }
}

union Shape {
    Text { s: String }
    Many { xs: List<i32> }
    Dot { x: i32 }
}

fn grow_ints(xs: &mut List<i32>) kills xs@items {
    for i in 0..200 {
        xs.push(i)
    }
}

fn grow_names(xs: &mut List<String>) kills xs@items {
    for _i in 0..64 {
        xs.push(String::from(LONG))
    }
}

fn grow_text(s: &mut String) kills s@text {
    for _i in 0..64 {
        s.push_str(LONG)
    }
}

fn grow_map(m: &mut HashMap<String, i32>) kills m@items, m[*] {
    for i in 0..200 {
        m.insert(format!("key{}", i), i)
    }
}

fn pick_int(xs: &List<i32>) -> &i32 {
    return &xs[0]
}

fn pick_name(xs: &List<String>) -> &str {
    return xs[0].as_str()
}

fn grow_then_read(xs: &mut List<i32>, v: &i32) kills xs@items {
    grow_ints(xs)
    println!("{}", v)
}

fn grow_names_then_read(xs: &mut List<String>, s: &str) kills xs@items {
    grow_names(xs)
    println!("{}", s)
}

fn grow_both(a: &mut List<i32>, b: &mut List<i32>) kills a@items, b@items {
    grow_ints(a)
    grow_ints(b)
}

fn grow_through(xs: &mut List<i32>) kills xs@items {
    grow_ints(xs)
}

fn grow_lists(xs: &mut List<List<i32>>) kills xs@items {
    for _i in 0..64 {
        xs.push(List<i32>())
    }
}

fn grow_bag(m: &mut HashMap<String, List<i32>>) kills m@items, m[*] {
    for i in 0..200 {
        m.insert(format!("key{}", i), List<i32>())
    }
}
"""

# How a view of each kind is used: read, so a freed one is seen.
USE = {
    "int": ["println!(\"{}\", {v})"],
    "mutint": ["{v} += 1", "println!(\"{}\", {v})"],
    "iter": ["println!(\"{}\", {v}.get())"],
    "str": ["println!(\"{}\", {v})"],
    "node": ["println!(\"{}\", {v}.name)"],
    "shape": ["println!(\"{}\", {v}.s)"],
    "list": ["println!(\"{}\", {v}.len())"],
}


class View:
    """A way to take a view: its kind, the lines that take it, and the lines that close the
    blocks those leave open, `depth` of them. Lines are indented relative to the first."""

    def __init__(self, kind, open_lines, close_lines=(), depth=None):
        self.kind = kind
        self.open = list(open_lines)
        self.close = list(close_lines)
        self.depth = depth if depth is not None else (1 if close_lines else 0)


# Each place memory lives: how a case sets it up, the views into it, what kills it, and
# what kills it through a call that also reads a view (`{v}`).
CONTAINERS = {
    "xs": {
        "setup": ["let mut xs = List<i32>()", "for i in 0..4 {", "    xs.push(i)", "}"],
        "views": [
            View("int", ["let {v} = xs.first() or return"]),
            View("int", ["let {v} = &xs[1]"]),
            View("mutint", ["let {v} = &mut xs[2]"]),
            View("iter", ["let {v} = xs.iter()"]),
            View("int", ["let {v} = pick_int(&xs)"]),
            View("int", ["for &{v} in &xs {"], ["    break", "}"]),
        ],
        "kills": [
            ["grow_ints(&mut xs)"],
            ["grow_through(&mut xs)"],
            ["xs.clear()"],
            ["xs.swap_remove(0)"],
            ["xs.insert(0, 9)"],
            ["xs = List<i32>()"],
            ["for i in 0..100 {", "    xs.push(i)", "}"],
            ["grow_both(&mut xs, &mut xs)"],
        ],
        "reads": {"int": ["grow_then_read(&mut xs, {v})"]},
    },
    "names": {
        "setup": [
            "let mut names = List<String>()",
            "for _i in 0..4 {",
            "    names.push(String::from(LONG))",
            "}",
        ],
        "views": [
            View("str", ["let {v} = names[0].as_str()"]),
            View("str", ["let {v} = &names[1]"]),
            View("str", ["let {v} = names.first() or return"]),
            View("str", ["let {v} = pick_name(&names)"]),
            View("str", ["for &{v} in &names {"], ["    break", "}"]),
        ],
        "kills": [
            ["grow_names(&mut names)"],
            ["names.clear()"],
            ["names[0] = String::from(LONG)"],
            ["names[0].push_str(LONG)"],
            ["names.swap_remove(0)"],
            ["let _gone{n} = names.remove(0)"],
        ],
        "reads": {"str": ["grow_names_then_read(&mut names, {v})"]},
    },
    # Short text is kept in the `String` itself (D-0095), so it moves when the list's array
    # does, and a view of it is left in freed memory where a long one's is not.
    "tags": {
        "setup": [
            "let mut tags = List<String>()",
            "for _i in 0..4 {",
            "    tags.push(String::from(SHORT))",
            "}",
        ],
        "views": [
            View("str", ["let {v} = tags[0].as_str()"]),
            View("str", ["let {v} = &tags[1]"]),
            View("str", ["let {v} = tags.first() or return"]),
            View("str", ["let {v} = pick_name(&tags)"]),
            View("str", ["for &{v} in &tags {"], ["    break", "}"]),
        ],
        "kills": [
            ["grow_names(&mut tags)"],
            ["tags.clear()"],
            ["tags[0] = String::from(SHORT)"],
            ["tags[0].push_str(LONG)"],
            ["tags.swap_remove(0)"],
            ["let _gone{n} = tags.remove(0)"],
        ],
        "reads": {"str": ["grow_names_then_read(&mut tags, {v})"]},
    },
    "text": {
        "setup": ["let mut text = String::from(LONG)"],
        "views": [
            View("str", ["let {v} = text.as_str()"]),
            View("str", ["let ({v}, _) = text.as_str().split_once(\" \") or return"]),
        ],
        "kills": [
            ["grow_text(&mut text)"],
            ["text.push_str(LONG)"],
            ["text = String::from(LONG)"],
            ["text.clear()"],
            ["text.insert_str(0, LONG)"],
        ],
        "reads": {},
    },
    "map": {
        "setup": [
            "let mut map = HashMap<String, i32>()",
            "for i in 0..4 {",
            "    map.insert(format!(\"key{}\", i), i)",
            "}",
        ],
        "views": [
            View("mutint", ["let {v} = map.get_or_insert(String::from(\"key1\"), 0)"]),
            View("int", ["let {v} = map.get(&String::from(\"key0\")) or return"]),
        ],
        "kills": [
            ["grow_map(&mut map)"],
            ["map.clear()"],
            ["map.insert(String::from(\"key9\"), 9)"],
            ["let _gone{n} = map.remove(&String::from(\"key0\"))"],
        ],
        "reads": {},
    },
    "h": {
        "setup": [
            "let mut h = Holder{}",
            "h.node = Box<Node>(Node{ name: String::from(LONG) })",
        ],
        "views": [
            View("node", ["if let &{v} = h.node {"], ["}"]),
            View("node", ["if let &mut {v} = h.node {"], ["}"]),
            View("node", ["let {v} = h.node or return"]),
        ],
        "kills": [
            ["h.node = null"],
            ["h.node = Box<Node>(Node{ name: String::from(LONG) })"],
        ],
        "reads": {},
    },
    "rc": {
        "setup": [
            "let mut ra = Rc<List<String>>(List<String>())",
            "if let &mut first = ra {",
            "    grow_names(&mut first)",
            "}",
            "let mut rb = ra",
        ],
        "views": [
            View("str", ["if let &r_{v} = ra {", "    let {v} = r_{v}.first() or return"], ["}"]),
            View("str", ["if let &r_{v} = rb {", "    let {v} = r_{v}.first() or return"], ["}"]),
        ],
        "kills": [
            ["if let &mut g = rb {", "    grow_names(&mut g)", "}"],
            ["if let &mut g = ra {", "    g.clear()", "}"],
        ],
        "reads": {},
    },
    "u": {
        "setup": ["let mut u = Shape::Text{ s: String::from(LONG) }"],
        "views": [
            View(
                "shape",
                ["switch u {", "    case Text(&{v}) {"],
                ["    }", "    case Many {}", "    case Dot {}", "}"],
                depth=2,
            ),
        ],
        "kills": [
            ["u = Shape::Dot{ x: 1 }"],
            ["u = Shape::Text{ s: String::from(LONG) }"],
            ["u = Shape::Many{ xs: List<i32>() }"],
        ],
        "reads": {},
    },
    "w": {
        "setup": ["let mut w = World()", "grow_ints(&mut w.items)", "grow_ints(&mut w.others)"],
        "views": [
            View("int", ["let {v} = &w.items[0]"]),
            View("int", ["let {v} = w.others.first() or return"]),
            View("int", ["let {v} = pick_int(&w.items)"]),
        ],
        "kills": [
            ["grow_ints(&mut w.items)"],
            ["grow_ints(&mut w.others)"],
            ["w.items.clear()"],
            ["w = World()"],
            ["grow_both(&mut w.items, &mut w.others)"],
        ],
        "reads": {"int": ["grow_then_read(&mut w.items, {v})", "grow_then_read(&mut w.others, {v})"]},
    },
    # Lists inside a list: growing the outer list moves the inner `List`s but not their
    # arrays, and growing an inner one moves only its own array (`nest[*]`).
    "nest": {
        "setup": [
            "let mut nest = List<List<i32>>()",
            "for _i in 0..4 {",
            "    let mut inner = List<i32>()",
            "    grow_ints(&mut inner)",
            "    nest.push(move(inner))",
            "}",
        ],
        "views": [
            View("int", ["let {v} = &nest[0][1]"]),
            View("int", ["let {v} = nest[1].first() or return"]),
            View("list", ["let {v} = &nest[0]"]),
            View("int", ["for &{v} in &nest[0] {"], ["    break", "}"]),
            View("list", ["for &{v} in &nest {"], ["    break", "}"]),
        ],
        "kills": [
            ["grow_lists(&mut nest)"],
            ["grow_ints(&mut nest[0])"],
            ["grow_ints(&mut nest[1])"],
            ["nest[0] = List<i32>()"],
            ["nest.clear()"],
            ["let _gone{n} = nest.swap_remove(0)"],
        ],
        "reads": {"int": ["grow_then_read(&mut nest[0], {v})", "grow_then_read(&mut nest[1], {v})"]},
    },
    # Lists as a map's values: a rehash moves the `List`s, a push into one moves its array.
    "bag": {
        "setup": [
            "let mut bag = HashMap<String, List<i32>>()",
            "for i in 0..4 {",
            "    let mut inner = List<i32>()",
            "    grow_ints(&mut inner)",
            "    bag.insert(format!(\"key{}\", i), move(inner))",
            "}",
        ],
        "views": [
            View("list", ["let {v} = bag.get(&String::from(\"key0\")) or return"]),
            View("int", ["let l_{v} = bag.get(&String::from(\"key1\")) or return",
                         "let {v} = l_{v}.first() or return"]),
            View("list", ["let {v} = bag.get_or_insert(String::from(\"key2\"), List<i32>())"]),
        ],
        "kills": [
            ["grow_bag(&mut bag)"],
            ["bag.insert(String::from(\"key0\"), List<i32>())"],
            ["let _gone{n} = bag.remove(&String::from(\"key1\"))"],
            ["grow_ints(bag.get_or_insert(String::from(\"key1\"), List<i32>()))"],
            ["bag.clear()"],
        ],
        "reads": {},
    },
}


def sub(line, name, n=0):
    """A template line with `{v}` naming the view and `{n}` a number fresh in the case."""
    return line.replace("{v}", name).replace("{n}", str(n))


def gen_case(rng, k):
    """One case: `fn case_k()`, as lines.

    A sequence of steps: each view is opened (the first at the start, a second after some of
    the kills), kills come in a random order, and each view is used once after it is
    opened, by a statement of its own or by a call that also kills. Blocks the views open
    close at the end, innermost first."""
    views = []
    for i in range(rng.choice([1, 1, 2])):
        c = rng.choice(list(CONTAINERS))
        views.append((f"v{i}", c, rng.choice(CONTAINERS[c]["views"])))
    used = {c for _, c, _ in views}

    # The kills, then the second view's opening among them.
    steps = []
    for n in range(rng.choice([0, 1, 1, 2, 2, 3])):
        c = rng.choice(views)[1] if rng.random() < 0.55 else rng.choice(list(CONTAINERS))
        steps.append(("kill", c, [sub(s, "", n) for s in rng.choice(CONTAINERS[c]["kills"])]))
        used.add(c)
    if len(views) == 2:
        steps.insert(rng.randint(0, len(steps)), ("open", "v1", None))

    # Each view is used once, after its opening: by itself, or now and then by a call that
    # kills a container and reads the view, which then takes a kill's place or adds one.
    for name, _, view in views:
        first = 0 if name == "v0" else next(i for i, s in enumerate(steps) if s[1] == name) + 1
        reads = [(c, f) for c in CONTAINERS for f in CONTAINERS[c]["reads"].get(view.kind, [])]
        if reads and rng.random() < 0.3:
            c, form = rng.choice(reads)
            steps.insert(rng.randint(first, len(steps)), ("read", c, [sub(form, name)]))
            used.add(c)
        else:
            body = [sub(u, name) for u in USE[view.kind]]
            steps.insert(rng.randint(first, len(steps)), ("use", name, body))

    lines = [f"fn case_{k}() {{"]
    for c in CONTAINERS:
        if c in used:
            lines += ["    " + s for s in CONTAINERS[c]["setup"]]
    depth = 1
    closers = []

    def open_view(name, view):
        nonlocal depth
        lines.extend("    " * depth + sub(s, name) for s in view.open)
        if view.close:
            closers.append((depth, view.close))
            depth += view.depth

    open_view(*[(n, v) for n, _, v in views if n == "v0"][0])
    for kind, what, body in steps:
        if kind == "open":
            open_view(what, views[1][2])
        else:
            lines.extend("    " * depth + s for s in body)
    for d, close in reversed(closers):
        lines.extend("    " * d + s for s in close)
    lines.append("}")
    return lines


def program(cases):
    """A whole program: the prelude, the cases, and a `main` that runs the one named."""
    out = [PRELUDE]
    for k, lines in cases:
        out.append("\n".join(lines) + "\n")
    main = ["fn main() {", "    let args = Process::args()", "    if args.len() < 2 {", "        return",
            "    }", "    let which = args[1].as_str()"]
    for k, _ in cases:
        main += [f"    if which == \"{k}\" {{", f"        case_{k}()", "    }"]
    main.append("}")
    out.append("\n".join(main) + "\n")
    return "\n".join(out)


def write_project(path, cases):
    """Write the batch to `path`, and the line each case starts on, in order."""
    if path.exists():
        shutil.rmtree(path)
    (path / "src").mkdir(parents=True)
    (path / "dazzle.toml").write_text(MANIFEST)
    text = program(cases)
    (path / "src" / "main.dz").write_text(text)
    starts = {}
    for n, line in enumerate(text.splitlines(), 1):
        m = re.match(r"fn case_(\d+)\(\)", line)
        if m:
            starts[int(m.group(1))] = n
    return starts


def case_of(line, starts):
    """Which case a line of the batch is in, or None for the prelude and `main`."""
    found = None
    for k, start in sorted(starts.items(), key=lambda kv: kv[1]):
        if start <= line:
            found = k
    return found


def dz(args, cwd, env_extra=None, timeout=600):
    env = dict(os.environ)
    env.update(env_extra or {})
    return subprocess.run([str(DZ)] + args, cwd=cwd, env=env, capture_output=True, text=True,
                          timeout=timeout)


def check(path, starts):
    """Each case's error codes, from `dz check`."""
    r = dz(["check", "--format=json", "."], path)
    report = json.loads(r.stdout)
    errors = {k: set() for k in starts}
    stray = []
    for d in report["diagnostics"]:
        if d["severity"] != "error":
            continue
        k = case_of(d["line"], starts)
        if k is None:
            stray.append(d)
        else:
            errors[k].add(d["code"])
    if stray:
        sys.exit("fuzz_ties: the prelude or main does not check:\n"
                 + "\n".join(f"  {d['code']} line {d['line']}: {d['message']}" for d in stray))
    return errors


def build(path, cxx, allow=None):
    """Build the batch under the sanitizers; the executable, or the build's output."""
    env = {"DZ_CXX": cxx, "DZ_CXXFLAGS": SANITIZE, "DZ_REGIONS": "0"}
    if allow:
        env["DZ_FUZZ_ALLOW"] = ",".join(sorted(allow))
    r = dz(["build", "."], path, env)
    if r.returncode != 0:
        return None, r.stdout + r.stderr
    exe = path / "build" / "bin" / "fuzz"
    return exe, ""


def run_case(exe, k):
    """Run one case: `report`, `panic`, `crash`, `timeout` or `clean`, and the telling line."""
    env = dict(os.environ, ASAN_OPTIONS="detect_leaks=0", UBSAN_OPTIONS="print_stacktrace=1")
    try:
        r = subprocess.run([str(exe), str(k)], env=env, capture_output=True, text=True, timeout=30)
    except subprocess.TimeoutExpired:
        return "timeout", ""
    err = r.stderr
    for marker in ("ERROR: AddressSanitizer", "runtime error:", "UndefinedBehaviorSanitizer"):
        if marker in err:
            line = next(l for l in err.splitlines() if marker in l)
            return "report", line.strip()
    if r.returncode == 0:
        return "clean", ""
    if "panic:" in err:
        return "panic", next(l for l in err.splitlines() if "panic:" in l).strip()
    return "crash", f"exit {r.returncode}"


def standalone(lines):
    """One case as a program of its own, for a finding."""
    return program([(0, [re.sub(r"fn case_\d+\(\)", "fn case_0()", lines[0])] + lines[1:])])


def main():
    global DZ
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--cases", type=int, default=200)
    ap.add_argument("--batch", type=int, default=50, help="cases per program built")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--cxx", default="clang++")
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    ap.add_argument("--dz", default=str(ROOT / "target" / "debug" / "dz"),
                    help="a debug build of dz, which reads DZ_FUZZ_ALLOW")
    ap.add_argument("--work", default=str(WORK),
                    help="where the batches are built and the findings written")
    args = ap.parse_args()
    DZ = Path(args.dz)
    if not DZ.exists():
        sys.exit(f"fuzz_ties: no {DZ}; run `cargo build -p dz` first")

    rng = random.Random(args.seed)
    work = Path(args.work)
    findings = work / "findings"
    if findings.exists():
        shutil.rmtree(findings)
    findings.mkdir(parents=True)

    tally = {"accepted": {}, "refused": {}, "other": {}}
    other_codes = {}
    failed_builds = 0
    holes = []

    def save(kind, k, lines, note):
        f = findings / f"{kind}-{args.seed}-{k}.dz"
        f.write_text(f"// {kind}: {note}\n" + standalone(lines))
        return f

    for first in range(0, args.cases, args.batch):
        cases = [(k, gen_case(rng, k)) for k in range(first, min(first + args.batch, args.cases))]
        by_k = dict(cases)
        path = work / "batch"
        # The ties check runs only on a program that type-checks (spec §17.2), so a case
        # refused for another reason is taken out and the rest checked again, until no
        # such case is left; only then are the ties check's answers the whole answer.
        live = cases
        while True:
            errors = check(path, write_project(path, live))
            other = {k: e for k, e in errors.items() if e and not e <= TIES}
            for k, e in other.items():
                tally["other"][k] = e
                for c in e - TIES:
                    other_codes[c] = other_codes.get(c, 0) + 1
                    if other_codes[c] <= 3:
                        save("generator", k, by_k[k], ", ".join(sorted(e)))
            if not other:
                break
            live = [(k, lines) for k, lines in live if k not in other]
        accepted = [k for k, e in errors.items() if not e]
        refused = [k for k, e in errors.items() if e]

        for label, ks, allow in (("accepted", accepted, None), ("refused", refused, TIES)):
            if not ks:
                continue
            part = [(k, by_k[k]) for k in ks]
            starts = write_project(path, part)
            exe, out = build(path, args.cxx, allow)
            if exe is None:
                failed_builds += 1
                f = findings / f"build-{args.seed}-{first}-{label}.txt"
                f.write_text(out)
                print(f"build failed ({label}, batch from {first}): {f}")
                if label == "accepted":
                    holes.append(f)
                continue
            with ThreadPoolExecutor(args.jobs) as pool:
                results = list(pool.map(lambda k: (k, run_case(exe, k)), ks))
            for k, (outcome, line) in results:
                tally[label][k] = outcome
                if label == "accepted" and outcome in ("report", "crash", "timeout"):
                    holes.append(save("hole", k, by_k[k], line or outcome))
                if label == "refused" and outcome == "clean" and \
                        sum(1 for o in tally["refused"].values() if o == "clean") <= 20:
                    save("clean-refusal", k, by_k[k], ", ".join(sorted(errors[k])))
        done = min(first + args.batch, args.cases)
        print(f"{done}/{args.cases} cases", flush=True)

    def count(label, outcome):
        return sum(1 for o in tally[label].values() if o == outcome)

    print()
    print(f"seed {args.seed}, {args.cases} cases, {args.cxx}")
    n = len(tally["accepted"])
    print(f"accepted: {n}  clean {count('accepted', 'clean')}, panicked {count('accepted', 'panic')}, "
          f"flagged {count('accepted', 'report')}, crashed {count('accepted', 'crash')}, "
          f"timed out {count('accepted', 'timeout')}")
    n = len(tally["refused"])
    print(f"refused by the ties check: {n}  flagged when run anyway {count('refused', 'report')}, "
          f"ran clean {count('refused', 'clean')}, panicked {count('refused', 'panic')}")
    if count("refused", "clean"):
        print("  (a clean run is not always a needless refusal: the sanitizers see only freed "
              "memory, not a view read after `clear()` or a union's change of variant)")
    if tally["other"]:
        print(f"refused for another reason (the generator's to avoid): {len(tally['other'])}  "
              + ", ".join(f"{c} x{n}" for c, n in sorted(other_codes.items())))
    if failed_builds:
        print(f"builds that failed: {failed_builds}")
    print(f"findings: {findings}")
    if holes:
        print("HOLES:")
        for h in holes:
            print(f"  {h}")
        return 1
    return 0


DZ = None

if __name__ == "__main__":
    sys.exit(main())
