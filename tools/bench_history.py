#!/usr/bin/env python3
"""The whole-program benchmarks' numbers over time (T-0075).

`tools/bench_programs.py` measures a run; this keeps it. Every run recorded is a few lines
of `tests/bench_programs/history.jsonl`, one per program and C++ compiler, and
`tests/bench_programs/HISTORY.md` is written from it: each ratio, run after run.

What is compared is the **ratio** of Dazzle's half to the C++ half measured in the same run,
never an absolute time. Absolute times move by 10-20% from one sitting to the next on the
same machine, and more between machines; the two halves of one run share the machine's
mood, so their ratio moves much less. And only runs on the same CPU with the same C++
compiler are compared, since a different compiler is a different C++ half.

    python tools/bench_history.py render          # HISTORY.md from history.jsonl
    python tools/bench_history.py compare run.json  # a run (bench_programs.py --json) against the last
    python tools/bench_history.py record run.json   # add that run to the history
    python tools/bench_history.py backfill        # history.jsonl from RESULTS.md's git history

`bench_programs.py` compares every full run with the history as it finishes, and with
`--record` adds the run to it; `--check` makes a ratio worse past the threshold an error.
"""

import argparse
import json
import re
import subprocess
import sys
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SUITE = ROOT / "tests" / "bench_programs"
HISTORY = SUITE / "history.jsonl"
RENDERED = SUITE / "HISTORY.md"

# How much worse a ratio may get, against the last run recorded on the same CPU and
# compiler, before the comparison says so. Loose on purpose: the ratio of a quiet run moves
# by a few percent, and the comparison is there to catch a change of kind (a lost inlining,
# a copy where there was a move) rather than of degree. Build time is noisier than run time,
# since three builds are fewer samples than ten runs.
RUN_THRESHOLD = 0.10
BUILD_THRESHOLD = 0.20

# Each record's numbers, as (key, what it is). Times are medians; peak memory the largest.
FIELDS = [
    ("dazzle_ms", "Dazzle's run time, ms"),
    ("cpp_ms", "the C++ half's run time, ms"),
    ("dazzle_mib", "Dazzle's peak memory, MiB"),
    ("cpp_mib", "the C++ half's peak memory, MiB"),
    ("build_s", "Dazzle's build from clean in the mode run, s"),
    ("cpp_build_s", "the C++ half's build from clean, s"),
    ("rebuild_s", "Dazzle's build after a one-line change, s"),
    ("cpp_rebuild_s", "the C++ half's build after a one-line change, s"),
    ("frontend_ms", "Dazzle's own phases in the build from clean, ms"),
]


def read(path=HISTORY):
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def write(records, path=HISTORY):
    lines = [json.dumps(r, ensure_ascii=False, separators=(", ", ": ")) for r in records]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def append(new, path=HISTORY):
    write(read(path) + new, path)


def ratio(r, a, b):
    """`r[a] / r[b]`, or None when either is missing."""
    if r.get(a) is None or not r.get(b):
        return None
    return r[a] / r[b]


def run_ratio(r):
    return ratio(r, "dazzle_ms", "cpp_ms")


def build_ratio(r):
    return ratio(r, "build_s", "cpp_build_s")


def records_of(run, when=None):
    """The records of one `bench_programs.py --json` run: one per program and compiler."""
    info = run["machine"]
    when = when or date.today().isoformat()
    out = []
    for res in run["results"]:
        d, c, b, rb = res["dazzle"], res["cpp"], res["build"], res["rebuild"]
        out.append(
            {
                "date": when,
                "commit": info["commit"],
                "cpu": info["cpu"],
                "cxx": res["cxx"],
                "cxx_version": info["compilers"].get(res["cxx"]),
                "mode": run["mode"],
                "program": res["program"],
                "dazzle_ms": round(d["median"] * 1000, 1),
                "cpp_ms": round(c["median"] * 1000, 1),
                "dazzle_mib": round(d["peak_kib"] / 1024, 1) if d.get("peak_kib") else None,
                "cpp_mib": round(c["peak_kib"] / 1024, 1) if c.get("peak_kib") else None,
                "build_s": round(b["dazzle"], 3),
                "cpp_build_s": round(b["cpp"], 3),
                "rebuild_s": round(rb["dazzle"], 3),
                "cpp_rebuild_s": round(rb["cpp"], 3),
                "frontend_ms": round(b["dazzle_frontend"] * 1000, 1),
                "load": info.get("load_before"),
            }
        )
    return out


def setup(r):
    """What a ratio is only comparable within: the CPU, and the C++ compiler and its version,
    since another compiler builds another C++ half."""
    return (r["cpu"], r["cxx"], r.get("cxx_version"))


def same_setup(a, b):
    return (setup(a), a["program"], a.get("mode", "release")) == (setup(b), b["program"], b.get("mode", "release"))


def compare(new, history, run_threshold=RUN_THRESHOLD, build_threshold=BUILD_THRESHOLD):
    """Each new record against the last one recorded on the same setup: the report's lines,
    and how many ratios got worse past their threshold."""
    lines = [
        "| Program | Compiler | Run, Dazzle / C++ | Build from clean, Dazzle / C++ | Against |",
        "|---|---|---:|---:|---|",
    ]
    worse = 0
    for r in new:
        before = [h for h in history if same_setup(h, r)]
        if not before:
            lines.append(f"| {r['program']} | {r['cxx']} | {run_ratio(r):.2f}× | {build_ratio(r):.2f}× | nothing recorded |")
            continue
        last = before[-1]
        cells = []
        for now_, then, threshold in (
            (run_ratio(r), run_ratio(last), run_threshold),
            (build_ratio(r), build_ratio(last), build_threshold),
        ):
            if now_ is None or then is None:
                cells.append("—")
                continue
            change = now_ / then - 1
            mark = ""
            if change > threshold:
                mark = " **worse**"
                worse += 1
            cells.append(f"{then:.2f}× → {now_:.2f}× ({change:+.0%}){mark}")
        lines.append(f"| {r['program']} | {r['cxx']} | {cells[0]} | {cells[1]} | {last['date']}, {last['commit'].split()[0]} |")
    return lines, worse


def render(records):
    """HISTORY.md: each ratio, run after run, one table per CPU and compiler."""
    out = [
        "# The benchmarks over time",
        "",
        "Written by `python tools/bench_history.py render` from [`history.jsonl`](history.jsonl); "
        "do not edit it by hand. Each row is a run of `tools/bench_programs.py` recorded with "
        "`--record` (the rows up to 2026-11-17 are the runs `RESULTS.md` held in git, read back "
        "by `bench_history.py backfill`). Each cell is Dazzle's half against the C++ half of "
        "the same run, below 1.00× where Dazzle is faster: compare a column down the page, not "
        "the absolute times, which move by 10–20% between sittings. "
        "[`README.md`](README.md) says what each program is and how the halves are built; "
        "a program's column is empty before it existed.",
    ]
    last = {}
    for r in records:
        last[setup(r)] = r["date"]
    # The newest setups first, since they are what a run is compared against; within one
    # date, by compiler.
    setups = sorted(sorted(last, key=lambda s: s[1]), key=lambda s: last[s], reverse=True)
    for cpu, cxx, version in setups:
        rows = [r for r in records if setup(r) == (cpu, cxx, version)]
        programs = sorted({r["program"] for r in rows})
        runs = []
        for r in rows:
            key = (r["date"], r["commit"])
            if key not in runs:
                runs.append(key)
        for title, of in (("Run time", run_ratio), ("Build from clean", build_ratio)):
            out += ["", f"## {title}: {version or cxx}, {cpu}", ""]
            out.append("| Date | Commit | " + " | ".join(programs) + " |")
            out.append("|---|---|" + "---:|" * len(programs))
            for when, commit in runs:
                at = {r["program"]: r for r in rows if (r["date"], r["commit"]) == (when, commit)}
                cells = []
                for p in programs:
                    v = of(at[p]) if p in at else None
                    cells.append(f"{v:.2f}×" if v is not None else "")
                short = commit.split()[0] + ("+" if "uncommitted" in commit else "")
                out.append(f"| {when} | {short} | " + " | ".join(cells) + " |")
    out += [
        "",
        "A commit marked `+` was measured with uncommitted changes to the compiler or the "
        "programs, usually the change the run was for. Every record has more than these "
        "ratios: both halves' times and peak memory, the build after a one-line change, "
        "and how long Dazzle's own phases took (`FIELDS` in `tools/bench_history.py`).",
    ]
    return "\n".join(out) + "\n"


# ---- backfill: the runs RESULTS.md held before there was a history ----------------------

ROW = re.compile(
    r"^\| (?P<program>[a-z_]+) \| (?P<cxx>[^|]+?) \| (?P<d>[\d.]+) ms \| (?P<c>[\d.]+) ms \| [\d.]+× "
    r"\| (?P<peak>[^|]*) \| (?P<build>[^|]*) \|"
)
PEAK = re.compile(r"([\d.]+) MiB / ([\d.]+) MiB")
BUILD = re.compile(r"([\d.]+) s(?: \(([\d.]+) s\))? / ([\d.]+) s")


def parse_results(text):
    """The main table of one version of RESULTS.md, with its machine lines."""
    cpu = re.search(r"^- CPU: (.+?) \(\d+ threads\)", text, re.M)
    commit = re.search(r"^- Dazzle: commit (.+)$", text, re.M)
    versions = dict(re.findall(r"^- `([^`]+)`: (.+)$", text, re.M))
    mode = re.search(r"^- Mode: `--(\w+)`", text, re.M)
    rows = []
    for line in text.splitlines():
        m = ROW.match(line)
        if not m:
            continue
        peak = PEAK.search(m["peak"])
        build = BUILD.search(m["build"])
        rows.append(
            {
                "cpu": cpu[1] if cpu else "unknown",
                "cxx": m["cxx"],
                "cxx_version": versions.get(m["cxx"]),
                "mode": mode[1] if mode else "release",
                "program": m["program"],
                "dazzle_ms": float(m["d"]),
                "cpp_ms": float(m["c"]),
                "dazzle_mib": float(peak[1]) if peak else None,
                "cpp_mib": float(peak[2]) if peak else None,
                "build_s": float(build[1]) if build else None,
                "cpp_build_s": float(build[3]) if build else None,
                "commit": commit[1].strip() if commit else "unknown",
            }
        )
    return rows


def backfill():
    """Every version of RESULTS.md in git, oldest first, as records. A version whose numbers
    are the last one's (only its prose changed) adds nothing."""
    log = subprocess.run(
        ["git", "-C", str(ROOT), "log", "--reverse", "--format=%h %ad", "--date=short", "--",
         "tests/bench_programs/RESULTS.md"],
        capture_output=True, text=True, check=True,
    ).stdout.split("\n")
    records, last = [], None
    for line in filter(None, log):
        sha, when = line.split()
        text = subprocess.run(
            ["git", "-C", str(ROOT), "show", f"{sha}:tests/bench_programs/RESULTS.md"],
            capture_output=True, text=True, check=True,
        ).stdout
        rows = parse_results(text)
        key = json.dumps(rows, sort_keys=True)
        if not rows or key == last:
            continue
        last = key
        for r in rows:
            # The keys in the order a recorded run writes them.
            records.append({"date": when, **r, "source": f"RESULTS.md at {sha}"})
    return records


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("render", help="write HISTORY.md from history.jsonl")
    c = sub.add_parser("compare", help="a run's --json against the history")
    c.add_argument("run")
    c.add_argument("--check", action="store_true", help="exit 1 if a ratio got worse past its threshold")
    r = sub.add_parser("record", help="add a run's --json to the history, after looking at it")
    r.add_argument("run")
    b = sub.add_parser("backfill", help="history.jsonl from RESULTS.md's git history")
    b.add_argument("--force", action="store_true", help="replace a history that has records")
    a = p.parse_args()

    if a.cmd == "render":
        RENDERED.write_text(render(read()), encoding="utf-8")
    elif a.cmd == "compare":
        new = records_of(json.loads(Path(a.run).read_text(encoding="utf-8")))
        lines, worse = compare(new, read())
        print("\n".join(lines))
        if worse and a.check:
            sys.exit(f"{worse} ratio(s) worse than the last run recorded, past the threshold")
    elif a.cmd == "record":
        run = Path(a.run)
        # The day the run was made, not the day it is recorded.
        when = date.fromtimestamp(run.stat().st_mtime).isoformat()
        append(records_of(json.loads(run.read_text(encoding="utf-8")), when))
        RENDERED.write_text(render(read()), encoding="utf-8")
    elif a.cmd == "backfill":
        if read() and not a.force:
            sys.exit(f"{HISTORY} has records already; --force replaces them")
        records = backfill()
        write(records)
        RENDERED.write_text(render(records), encoding="utf-8")
        print(f"{len(records)} records from RESULTS.md's history", file=sys.stderr)


if __name__ == "__main__":
    main()
