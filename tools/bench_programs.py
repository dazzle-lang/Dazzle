#!/usr/bin/env python3
"""Whole-program benchmarks: each program in tests/bench_programs, Dazzle against C++.

Each program is written twice, idiomatically: `src/main.dz` and `main.cpp`. For every C++
compiler asked for, this builds both halves with that compiler at the same optimisation
level, runs them alternately, and reports run time, peak memory and build time. Both
halves print a `checksum` line, and the run fails if any two disagree: two programs
computing different things measure nothing.

    python tools/bench_programs.py                       # every program, clang++ and g++
    python tools/bench_programs.py --cxx g++ --runs 20 astar
    python tools/bench_programs.py --quick               # small inputs: does it all agree?
    python tools/bench_programs.py --markdown out.md --json out.json

The numbers are only as good as the machine is quiet. The load average is printed before
anything runs, and a run on a busy machine says so in its report.
"""

import argparse
import json
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SUITE = ROOT / "tests" / "bench_programs"


# The flags `dz build` gives the generated C++ (crates/dz/src/build.rs), per mode. The C++
# half gets the same optimisation, the same standard and the same -fno-exceptions
# -fno-rtti. It does not get -fwrapv: wrapping integer arithmetic is a Dazzle rule
# (spec §5.1) whose cost belongs to Dazzle's side of the comparison.
MODES = {
    "release": ["-O2"],
    "shipping": ["-O3", "-DNDEBUG"],
}
CXX_FLAGS = ["-std=c++17", "-fno-exceptions", "-fno-rtti", "-Wall", "-Wextra"]


def exe(name):
    return name + ".exe" if os.name == "nt" else name


def first_line(cmd):
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, check=False)
    except OSError:
        return None
    text = (out.stdout or out.stderr).strip()
    return text.splitlines()[0] if text else None


def cpu_model():
    if sys.platform.startswith("linux"):
        try:
            for line in Path("/proc/cpuinfo").read_text().splitlines():
                if line.startswith("model name"):
                    return line.split(":", 1)[1].strip()
        except OSError:
            pass
    if sys.platform == "darwin":
        name = first_line(["sysctl", "-n", "machdep.cpu.brand_string"])
        if name:
            return name
    return platform.processor() or "unknown"


def machine():
    commit = first_line(["git", "-C", str(ROOT), "rev-parse", "--short", "HEAD"])
    # Only what can change the numbers: the compiler, the standard library and the programs.
    # Untracked files count, since a new program or crate is one.
    dirty = subprocess.run(
        ["git", "-C", str(ROOT), "status", "--porcelain", "--", "crates", "tests/bench_programs",
         "Cargo.toml", "Cargo.lock"],
        capture_output=True,
        text=True,
        check=False,
    ).stdout.strip()
    return {
        "cpu": cpu_model(),
        "cores": os.cpu_count(),
        "os": platform.platform(),
        "commit": (commit or "unknown") + (" plus uncommitted changes to the compiler or the programs" if dirty else ""),
        "load_before": load(),
    }


def load():
    try:
        return round(os.getloadavg()[0], 2)
    except (AttributeError, OSError):
        return None


def timed(cmd, **kw):
    start = time.perf_counter()
    out = subprocess.run(cmd, capture_output=True, text=True, check=False, **kw)
    return time.perf_counter() - start, out


def build_measure(cxx, work):
    """The helper that times a run and reads its peak memory (measure.cpp), or None where
    there is no fork and wait4 to build it on."""
    if os.name == "nt":
        return None
    binary = work / "measure"
    out = subprocess.run(
        [cxx, "-std=c++17", "-O2", str(SUITE / "measure.cpp"), "-o", str(binary)],
        capture_output=True,
        text=True,
        check=False,
    )
    if out.returncode != 0:
        sys.exit(f"{cxx} failed on measure.cpp:\n{out.stderr}")
    return binary


def run_measured(cmd, measure):
    """Run `cmd`; return (wall seconds, peak resident KiB or None, stdout)."""
    if measure is None:
        wall, done = timed(cmd)
        rss = None
    else:
        _, done = timed([str(measure), *cmd])
        m = re.search(r"^measure: (\S+) (\d+)$", done.stderr, re.M)
        if not m:
            sys.exit(f"measure printed nothing for {cmd[0]}:\n{done.stderr}")
        wall, rss = float(m.group(1)), int(m.group(2))
    if done.returncode != 0:
        sys.exit(f"{cmd[0]} exited with {done.returncode}:\n{done.stderr}")
    return wall, rss, done.stdout


def checksum_of(text, what):
    m = re.search(r"^checksum (\S+)$", text, re.M)
    if not m:
        sys.exit(f"{what} printed no `checksum` line:\n{text}")
    return m.group(1)


# The phases `dz build --timings` reports that are Dazzle's own work, as opposed to importing
# C and C++ headers with libclang or running the C++ compiler.
DAZZLE_PHASES = [
    "manifests",
    "lex and parse",
    "#if and macros",
    "resolve names",
    "type check",
    "ownership",
    "instantiate generics",
    "emit C++",
]


def parse_timings(stderr):
    """The phases `--timings` printed, as {name: seconds}, the total left out."""
    phases = {}
    lines = stderr.splitlines()
    if "timings:" not in lines:
        return phases
    for line in lines[lines.index("timings:") + 1 :]:
        m = re.match(r"^\s+(.+?)\s+([\d.]+) ms$", line)
        if not m:
            break
        if m.group(1) != "total":
            phases[m.group(1)] = float(m.group(2)) / 1000
    return phases


def build_dazzle(dz, program, cxx, mode, work, builds):
    """Build the Dazzle half from clean `builds` times. Return the binary, the median wall
    time, and the median time of each phase `--timings` reported."""
    project = SUITE / program
    walls, phases = [], []
    for _ in range(builds):
        shutil.rmtree(project / "build", ignore_errors=True)
        cmd = [str(dz), "build", f"--{mode}", "--timings", "--cxx", cxx, str(project)]
        wall, out = timed(cmd)
        if out.returncode != 0:
            sys.exit(f"dz build failed for {program} with {cxx}:\n{out.stdout}{out.stderr}")
        walls.append(wall)
        phases.append(parse_timings(out.stderr))
    binary = work / exe(f"{program}-dazzle-{cxx}")
    shutil.copy2(project / "build" / "bin" / exe(program), binary)
    median = {n: statistics.median(p.get(n, 0.0) for p in phases) for n in phases[0]}
    return binary, statistics.median(walls), median


def build_cpp(program, cxx, mode, work, builds):
    """Build the C++ half `builds` times; return the binary and the median wall time."""
    binary = work / exe(f"{program}-cpp-{cxx}")
    cmd = [cxx, *CXX_FLAGS, *MODES[mode], str(SUITE / program / "main.cpp"), "-o", str(binary)]
    walls = []
    for _ in range(builds):
        seconds, out = timed(cmd)
        if out.returncode != 0:
            sys.exit(f"{cxx} failed on {program}/main.cpp:\n{out.stderr}")
        walls.append(seconds)
    return binary, statistics.median(walls)


def summarise(walls):
    return {"median": statistics.median(walls), "min": min(walls), "runs": len(walls)}


def bench(dz, program, cxx, mode, runs, args, work, measure, builds):
    dz_bin, dz_build, phases = build_dazzle(dz, program, cxx, mode, work, builds)
    cpp_bin, cpp_build = build_cpp(program, cxx, mode, work, builds)
    dz_front = sum(t for n, t in phases.items() if n in DAZZLE_PHASES)

    sides = {"dazzle": [str(dz_bin), *args], "cpp": [str(cpp_bin), *args]}
    walls = {"dazzle": [], "cpp": []}
    rss = {"dazzle": 0, "cpp": 0}
    sums = {}
    for r in range(runs):
        # Alternate which half goes first, so neither always runs on a warmer machine.
        order = ["dazzle", "cpp"] if r % 2 == 0 else ["cpp", "dazzle"]
        for side in order:
            wall, peak, text = run_measured(sides[side], measure)
            walls[side].append(wall)
            rss[side] = max(rss[side], peak or 0)
            sums.setdefault(checksum_of(text, f"{program} ({side}, {cxx})"), set()).add(side)
    if len(sums) != 1:
        sys.exit(f"{program} with {cxx}: the two halves disagree: {sums}")

    return {
        "program": program,
        "cxx": cxx,
        "checksum": next(iter(sums)),
        "dazzle": {**summarise(walls["dazzle"]), "peak_kib": rss["dazzle"] or None},
        "cpp": {**summarise(walls["cpp"]), "peak_kib": rss["cpp"] or None},
        # `dazzle_frontend` is the sum of Dazzle's own phases; `phases` has every one.
        "build": {
            "builds": builds,
            "dazzle": dz_build,
            "dazzle_frontend": dz_front,
            "phases": phases,
            "cpp": cpp_build,
        },
    }


def ms(seconds):
    return f"{seconds * 1000:.0f} ms"


def mib(kib):
    return f"{kib / 1024:.1f} MiB" if kib else "—"


def report(info, results, mode, runs):
    flags = " ".join(CXX_FLAGS + MODES[mode])
    lines = [
        "| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) "
        "| Build: Dazzle / C++ |",
        "|---|---|---:|---:|---:|---:|---:|",
    ]
    for r in results:
        d, c, b = r["dazzle"], r["cpp"], r["build"]
        lines.append(
            f"| {r['program']} | {r['cxx']} | {ms(d['median'])} | {ms(c['median'])} "
            f"| {d['median'] / c['median']:.2f}× | {mib(d['peak_kib'])} / {mib(c['peak_kib'])} "
            f"| {b['dazzle']:.2f} s / {b['cpp']:.2f} s |"
        )
    lines += [
        "",
        "Where Dazzle's build time goes, from `dz build --timings`:",
        "",
        "| Program | Compiler | Dazzle's own phases | C and C++ headers | C++ compiler | Other |",
        "|---|---|---:|---:|---:|---:|",
    ]
    for r in results:
        b = r["build"]
        headers = b["phases"].get("C and C++ headers", 0.0)
        compiler = b["phases"].get("C++ compiler", 0.0)
        other = max(0.0, b["dazzle"] - b["dazzle_frontend"] - headers - compiler)
        lines.append(
            f"| {r['program']} | {r['cxx']} | {ms(b['dazzle_frontend'])} | {ms(headers)} "
            f"| {ms(compiler)} | {ms(other)} |"
        )
    builds = results[0]["build"]["builds"] if results else 0
    lines += [
        "",
        f"Run time is the median of {runs} runs of each half, alternating. Build time is the "
        f"median of {builds} builds from clean: for Dazzle, `dz build`, which compiles the "
        "program and the standard library; for C++, one compiler invocation. Dazzle's own "
        "phases are reading the manifests, parsing, macros, resolving names, type checking, "
        "ownership, instantiating generics and emitting the C++. Other is what no phase "
        "accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is "
        "the largest resident set of any run.",
        "",
        f"- CPU: {info['cpu']} ({info['cores']} threads)",
        f"- OS: {info['os']}",
    ]
    for cxx, version in info["compilers"].items():
        lines.append(f"- `{cxx}`: {version}")
    lines += [
        f"- Mode: `--{mode}`. C++ flags: `{flags}`; Dazzle's generated code gets the same "
        "and `-fwrapv`.",
        f"- Dazzle: commit {info['commit']}",
        f"- Load average before / after: {info['load_before']} / {info['load_after']}",
    ]
    return "\n".join(lines) + "\n"


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    p.add_argument("programs", nargs="*", help="which programs (default: all)")
    p.add_argument("--cxx", default=None, help="comma-separated compilers (default: clang++,g++)")
    p.add_argument("--dz", default=None, help="the dz binary (default: target/release/dz)")
    p.add_argument("--mode", choices=sorted(MODES), default="release")
    p.add_argument("--runs", type=int, default=10)
    p.add_argument("--builds", type=int, default=3, help="clean builds of each half (default 3)")
    p.add_argument("--quick", action="store_true", help="small inputs and 2 runs")
    p.add_argument("--markdown", help="also write the report here")
    p.add_argument("--json", help="also write every measurement here")
    a = p.parse_args()

    programs = a.programs or sorted(d.name for d in SUITE.iterdir() if (d / "main.cpp").exists())
    for prog in programs:
        if not (SUITE / prog / "main.cpp").exists():
            sys.exit(f"no program {prog!r} in {SUITE}")
    compilers = a.cxx.split(",") if a.cxx else [c for c in ("clang++", "g++") if shutil.which(c)]
    if not compilers:
        sys.exit("no C++ compiler found; pass --cxx")
    dz = Path(a.dz) if a.dz else ROOT / "target" / "release" / exe("dz")
    if not dz.exists():
        sys.exit(f"{dz} does not exist: build it with `cargo build --release -p dz`, since a "
                 "debug dz makes Dazzle's build time look worse than it is")
    runs = 2 if a.quick and a.runs == 10 else a.runs

    info = machine()
    info["compilers"] = {c: first_line([c, "--version"]) for c in compilers}
    print(f"{info['cpu']}, load average {info['load_before']}", file=sys.stderr)
    # A load of one or two is a desktop doing nothing much; past an eighth of the threads,
    # something else is running.
    if info["load_before"] is not None and info["load_before"] > max(1.0, (info["cores"] or 8) / 8):
        print("warning: the machine is busy; these numbers will be noisy", file=sys.stderr)

    results = []
    with tempfile.TemporaryDirectory(prefix="dz-bench-") as tmp:
        measure = build_measure(compilers[0], Path(tmp))
        for prog in programs:
            # `quick_args.txt` is the small input the test suite runs each program on
            # (crates/dz/tests/run.rs); without --quick, a program's defaults are the full size.
            args = (SUITE / prog / "quick_args.txt").read_text().split() if a.quick else []
            for cxx in compilers:
                print(f"{prog} with {cxx}...", file=sys.stderr)
                builds = 1 if a.quick else a.builds
                results.append(bench(dz, prog, cxx, a.mode, runs, args, Path(tmp), measure, builds))
    info["load_after"] = load()

    text = report(info, results, a.mode, runs)
    print(text)
    if a.markdown:
        Path(a.markdown).write_text(text, encoding="utf-8")
    if a.json:
        Path(a.json).write_text(
            json.dumps({"machine": info, "mode": a.mode, "results": results}, indent=2) + "\n",
            encoding="utf-8",
        )


if __name__ == "__main__":
    main()
