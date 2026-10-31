#!/usr/bin/env python3
"""The regions gate (T-0117): each whole-program benchmark on the heap, against the same
program with scope regions on.

Stage 1 of T-0117 keeps scope regions only if they earn it. This measures the gate's rows
that need a stopwatch:

* `astar`, `words` and `expr` from tests/bench_programs, each built twice from the same
  source, the heap version of the program: every `Allocators::Temp` taken out, so the
  allocator is left to its default where the program had a choice, and is
  `Allocators::Global` where it had to name one (T-0104's definition of "heap"). The two
  builds differ only in `DZ_REGIONS=1`, which turns placement on in codegen. Both must
  print the same checksum.
* a loop that reassigns a `String`, a `List` and a formatted string a million times: with
  regions on, its peak memory must be what it is for ten thousand passes (the loop rule).

    python tools/bench_regions.py                     # every program, clang++ and g++
    python tools/bench_regions.py --cxx g++ --runs 20 astar
    python tools/bench_regions.py --quick             # small inputs: does it all agree?
    python tools/bench_regions.py --markdown out.md --json out.json

Peak memory is the peak resident set on Linux and macOS (tests/bench_programs/measure.cpp)
and the peak working set on Windows (GetProcessMemoryInfo), which is what T-0104 and T-0105
measured.
"""

import argparse
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
# Importing the sibling script must not leave a `__pycache__` in the checkout.
sys.dont_write_bytecode = True
import bench_programs as bp  # noqa: E402  (the shared helpers: machine(), timing, measure.cpp)

PROGRAMS = ["astar", "words", "expr"]

# The gate's thresholds, from T-0117.
PEAK_SLACK = 0.10  # peak memory within 10% of the heap's
FLAT_SLACK_KIB = 1024  # a million passes may peak at most 1 MiB above ten thousand


def heap_source(program, text):
    """`text`, the program's main.dz, with every `Allocators::Temp` taken out: an argument
    is dropped where the parameter has a default, and becomes `Allocators::Global` where it
    has none; `Allocators::Temp.reset()` goes. Fails loudly if anything is left, so a
    change to a benchmark cannot quietly leave it half on the arena."""
    out = re.sub(r"^[ \t]*Allocators::Temp\.reset\(\)[ \t]*\r?\n", "", text, flags=re.M)
    if program == "expr":
        # `Parser(…, allocator)` has no default: the heap is named.
        out = out.replace("Allocators::Temp)", "Allocators::Global)")
    out = re.sub(r",\s*Allocators::Temp\)", ")", out)
    code = "\n".join(line.split("//", 1)[0] for line in out.splitlines())
    if "Allocators::Temp" in code:
        sys.exit(f"{program}: an `Allocators::Temp` is left in the heap version")
    return out


LOOP = """\
// The loop rule (T-0117): an allocation that can run more than once while its region is
// alive goes to the heap, unless what it makes dies within the same pass. `s`, `xs` and
// `label` are reassigned on every pass, so what is assigned to them is never placed in the
// region of `main`; `t` is a `let` inside the loop, in a region of each pass.
fn main() {
    let args = Process::args()
    let n = if args.len() > 1 {
        args[1].parse_i64() or 1000000
    } else {
        1000000
    }
    let mut s = String()
    let mut xs = List<i32>()
    let mut label = String("start")
    let mut total: i64 = 0
    for i in 0..n {
        s = format!("item {} of {}", i, n)
        xs = List<i32>(8)
        xs.push(i as i32)
        label = String::from(s.as_str())
        let t = String::from(label.as_str())
        total += (s.len() + t.len() + xs.len()) as i64
    }
    println!("checksum {}", total)
}
"""

LOOP_MANIFEST = """\
[module]
name    = "loop"
sources = ["src/**/*.dz"]

[dependencies]
std = { open = true }

[build]
output = "executable"
"""


def peak_windows(proc):
    """The peak working set of the finished `proc`, in KiB."""
    import ctypes
    from ctypes import wintypes

    class Counters(ctypes.Structure):
        _fields_ = [
            ("cb", wintypes.DWORD),
            ("PageFaultCount", wintypes.DWORD),
            ("PeakWorkingSetSize", ctypes.c_size_t),
            ("WorkingSetSize", ctypes.c_size_t),
            ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
            ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
            ("PagefileUsage", ctypes.c_size_t),
            ("PeakPagefileUsage", ctypes.c_size_t),
        ]

    counters = Counters()
    counters.cb = ctypes.sizeof(Counters)
    get = ctypes.windll.kernel32.K32GetProcessMemoryInfo
    get.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
    get.restype = wintypes.BOOL
    # Popen keeps the process handle open until it is collected, and a process that has
    # exited still answers for its counters while a handle to it is open.
    if not get(int(proc._handle), ctypes.byref(counters), counters.cb):
        sys.exit("GetProcessMemoryInfo failed")
    return counters.PeakWorkingSetSize // 1024


def run(cmd, measure):
    """Run `cmd`; return (wall seconds, peak KiB, stdout)."""
    if os.name != "nt":
        wall, rss, text = bp.run_measured(cmd, measure)
        return wall, rss, text
    start = time.perf_counter()
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    out, err = proc.communicate()
    wall = time.perf_counter() - start
    if proc.returncode != 0:
        sys.exit(f"{cmd[0]} exited with {proc.returncode}:\n{err}")
    return wall, peak_windows(proc), out


def build(dz, project, name, cxx, mode, regions, dest):
    """`dz build` the project at `project`, with regions on or off; copy the binary to
    `dest` and return it."""
    shutil.rmtree(project / "build", ignore_errors=True)
    env = dict(os.environ)
    env.pop("DZ_REGIONS", None)
    if regions:
        env["DZ_REGIONS"] = "1"
    cmd = [str(dz), "build", f"--{mode}", "--cxx", cxx, str(project)]
    out = subprocess.run(cmd, capture_output=True, text=True, check=False, env=env)
    if out.returncode != 0:
        sys.exit(f"dz build failed for {name} with {cxx}:\n{out.stdout}{out.stderr}")
    shutil.copy2(project / "build" / "bin" / bp.exe(name), dest)
    return dest


def copy_program(program, work):
    """The heap version of `program`, written to its own folder under `work`."""
    src = bp.SUITE / program
    dst = work / program
    shutil.rmtree(dst, ignore_errors=True)
    (dst / "src").mkdir(parents=True)
    shutil.copy2(src / "dazzle.toml", dst / "dazzle.toml")
    text = (src / "src" / "main.dz").read_text(encoding="utf-8")
    (dst / "src" / "main.dz").write_text(heap_source(program, text), encoding="utf-8")
    return dst


def bench(dz, program, cxx, mode, runs, args, work, measure):
    project = copy_program(program, work)
    binaries = {
        side: build(dz, project, program, cxx, mode, side == "regions",
                    work / bp.exe(f"{program}-{side}-{cxx}"))
        for side in ("heap", "regions")
    }
    # One untimed run of each first: the first run of a fresh binary pays for loading it from
    # disk, which with two builds to compare is a difference neither of them makes.
    for side in ("heap", "regions"):
        run([str(binaries[side]), *args], measure)
    walls = {"heap": [], "regions": []}
    peaks = {"heap": 0, "regions": 0}
    sums = {}
    for r in range(runs):
        # Alternate which build goes first, so neither always runs on a warmer machine.
        order = ["heap", "regions"] if r % 2 == 0 else ["regions", "heap"]
        for side in order:
            wall, peak, text = run([str(binaries[side]), *args], measure)
            walls[side].append(wall)
            peaks[side] = max(peaks[side], peak or 0)
            sums.setdefault(bp.checksum_of(text, f"{program} ({side}, {cxx})"), set()).add(side)
    if len(sums) != 1:
        sys.exit(f"{program} with {cxx}: regions changed the result: {sums}")
    heap, regions = statistics.median(walls["heap"]), statistics.median(walls["regions"])
    return {
        "program": program,
        "cxx": cxx,
        "checksum": next(iter(sums)),
        "heap": {**bp.summarise(walls["heap"]), "peak_kib": peaks["heap"] or None},
        "regions": {**bp.summarise(walls["regions"]), "peak_kib": peaks["regions"] or None},
        "faster": regions < heap,
        "peak_within": bool(peaks["heap"])
        and peaks["regions"] <= peaks["heap"] * (1 + PEAK_SLACK),
    }


def loop_gate(dz, cxx, mode, work, measure, quick):
    """Peak memory of the loop at ten thousand passes and at a million (or a hundred
    thousand with --quick), regions on and off."""
    project = work / "loop"
    shutil.rmtree(project, ignore_errors=True)
    (project / "src").mkdir(parents=True)
    (project / "dazzle.toml").write_text(LOOP_MANIFEST, encoding="utf-8")
    (project / "src" / "main.dz").write_text(LOOP, encoding="utf-8")
    few, many = 10000, (100000 if quick else 1000000)
    out = {"cxx": cxx, "few": few, "many": many}
    for side in ("heap", "regions"):
        binary = build(dz, project, "loop", cxx, mode, side == "regions",
                       work / bp.exe(f"loop-{side}-{cxx}"))
        peaks = {}
        for n in (few, many):
            peak = 0
            for _ in range(3):
                _, kib, _ = run([str(binary), str(n)], measure)
                peak = max(peak, kib or 0)
            peaks[n] = peak
        out[side] = {"few_kib": peaks[few] or None, "many_kib": peaks[many] or None}
    r = out["regions"]
    out["flat"] = bool(r["few_kib"]) and r["many_kib"] - r["few_kib"] <= FLAT_SLACK_KIB
    return out


def verdict(ok):
    return "yes" if ok else "**no**"


def report(info, results, loops, mode, runs):
    lines = [
        "| Program | Compiler | Heap | Regions | Regions / heap | Peak (heap / regions) "
        "| Faster | Peak within 10% |",
        "|---|---|---:|---:|---:|---:|---|---|",
    ]
    for r in results:
        h, g = r["heap"], r["regions"]
        lines.append(
            f"| {r['program']} | {r['cxx']} | {bp.ms(h['median'])} | {bp.ms(g['median'])} "
            f"| {g['median'] / h['median']:.3f} | {bp.mib(h['peak_kib'])} / "
            f"{bp.mib(g['peak_kib'])} | {verdict(r['faster'])} | {verdict(r['peak_within'])} |"
        )
    lines += [
        "",
        "| Loop rule | Compiler | Heap: peak at {few} / {many} | Regions: peak at {few} / {many} "
        "| Flat |".format(few=loops[0]["few"] if loops else "", many=loops[0]["many"] if loops else ""),
        "|---|---|---:|---:|---|",
    ]
    for lp in loops:
        h, g = lp["heap"], lp["regions"]
        lines.append(
            f"| reassigned in a loop | {lp['cxx']} | {bp.mib(h['few_kib'])} / "
            f"{bp.mib(h['many_kib'])} | {bp.mib(g['few_kib'])} / {bp.mib(g['many_kib'])} "
            f"| {verdict(lp['flat'])} |"
        )
    lines += [
        "",
        f"Run time is the median of {runs} runs of each build, alternating. Both builds of a "
        "program compile the same source, its heap version (every `Allocators::Temp` taken "
        "out); they differ only in `DZ_REGIONS=1`. Peak memory is the largest of any run. "
        "Flat means a million passes peak at most 1 MiB above ten thousand.",
        "",
        f"- CPU: {info['cpu']} ({info['cores']} threads)",
        f"- OS: {info['os']}",
    ]
    for cxx, version in info["compilers"].items():
        lines.append(f"- `{cxx}`: {version}")
    lines += [
        f"- Mode: `--{mode}`",
        f"- Dazzle: commit {info['commit']}",
        f"- Load average before / after: {info['load_before']} / {info['load_after']}",
    ]
    return "\n".join(lines) + "\n"


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    p.add_argument("programs", nargs="*", help="which programs (default: all, and the loop)")
    p.add_argument("--cxx", default=None, help="comma-separated compilers (default: clang++,g++)")
    p.add_argument("--dz", default=None, help="the dz binary (default: target/release/dz)")
    p.add_argument("--mode", choices=sorted(bp.MODES), default="release")
    p.add_argument("--runs", type=int, default=10)
    p.add_argument("--quick", action="store_true", help="small inputs and 2 runs")
    p.add_argument("--no-loop", action="store_true", help="skip the loop rule's row")
    p.add_argument("--markdown", help="also write the report here")
    p.add_argument("--json", help="also write every measurement here")
    a = p.parse_args()

    programs = a.programs or PROGRAMS
    for prog in programs:
        if prog not in PROGRAMS:
            sys.exit(f"no program {prog!r}: the gate measures {', '.join(PROGRAMS)}")
    compilers = a.cxx.split(",") if a.cxx else [c for c in ("clang++", "g++") if shutil.which(c)]
    if not compilers:
        sys.exit("no C++ compiler found; pass --cxx")
    dz = Path(a.dz) if a.dz else bp.ROOT / "target" / "release" / bp.exe("dz")
    if not dz.exists():
        sys.exit(f"{dz} does not exist: build it with `cargo build --release -p dz`")
    runs = 2 if a.quick and a.runs == 10 else a.runs

    info = bp.machine()
    info["compilers"] = {c: bp.first_line([c, "--version"]) for c in compilers}
    print(f"{info['cpu']}, load average {info['load_before']}", file=sys.stderr)

    results, loops = [], []
    with tempfile.TemporaryDirectory(prefix="dz-regions-") as tmp:
        work = Path(tmp)
        measure = bp.build_measure(compilers[0], work)
        for prog in programs:
            args = (bp.SUITE / prog / "quick_args.txt").read_text().split() if a.quick else []
            for cxx in compilers:
                print(f"{prog} with {cxx}...", file=sys.stderr)
                results.append(bench(dz, prog, cxx, a.mode, runs, args, work, measure))
        if not a.no_loop and not a.programs:
            for cxx in compilers:
                print(f"the loop with {cxx}...", file=sys.stderr)
                loops.append(loop_gate(dz, cxx, a.mode, work, measure, a.quick))
    info["load_after"] = bp.load()

    text = report(info, results, loops, a.mode, runs)
    print(text)
    if a.markdown:
        Path(a.markdown).write_text(text, encoding="utf-8")
    if a.json:
        Path(a.json).write_text(
            json.dumps({"machine": info, "mode": a.mode, "results": results, "loops": loops},
                       indent=2) + "\n",
            encoding="utf-8",
        )


if __name__ == "__main__":
    main()
