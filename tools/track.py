"""Tasks (docs/tasks/T-NNNN-*.md) and decisions (docs/decisions/D-NNNN-*.md), queried cheaply.

Each file starts with a `key: value` header between `---` lines; this script reads only
those headers, so listing every task costs one line per task rather than every file's text.

    python tools/track.py tasks [--milestone M12] [--status open|done|all] [--area A] [--kind K]
    python tools/track.py milestones
    python tools/track.py decisions [--status decided|open|all]
    python tools/track.py show T-0050
    python tools/track.py new task "Title" --milestone M12 --area stdlib [--kind feature]
    python tools/track.py new decision "Question"
    python tools/track.py check

Milestones are the rows of the roadmap, spec §19 (`docs/spec/19-roadmap.md`), plus `v2`
(deferred past the first release, as 18.11 defers concurrency) and `none` (not scheduled).
"""
import argparse
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOCS = os.path.join(ROOT, "docs")
KINDS = {"T": "tasks", "D": "decisions"}
TASK_KEYS = ["id", "title", "milestone", "status", "kind", "area"]
TASK_STATUS = {"open", "done", "dropped"}
TASK_KIND = {"feature", "deferral", "design", "bug"}
DECISION_KEYS = ["id", "title", "status"]
DECISION_STATUS = {"decided", "open"}


def header(path):
    with open(path, encoding="utf-8") as f:
        lines = f.read().split("\n")
    if not lines or lines[0].strip() != "---":
        return None
    out = {}
    for s in lines[1:]:
        if s.strip() == "---":
            return out
        key, _, value = s.partition(":")
        out[key.strip()] = value.strip()
    return None


def items(prefix):
    folder = os.path.join(DOCS, KINDS[prefix])
    out = []
    for name in sorted(os.listdir(folder)):
        if re.match(prefix + r"-\d{4}-.*\.md$", name):
            h = header(os.path.join(folder, name)) or {}
            h["_file"] = os.path.join(folder, name)
            out.append(h)
    return out


def milestones():
    """{id: (name, done)} from the roadmap table."""
    out = {}
    with open(os.path.join(DOCS, "spec", "19-roadmap.md"), encoding="utf-8") as f:
        for s in f:
            m = re.match(r"\| (M\d+[a-z]?)( ✅)? \| ([^|]*) \|", s)
            if m:
                out[m.group(1)] = (m.group(3).strip(), bool(m.group(2)))
    out["v2"] = ("after the first release", False)
    out["none"] = ("not scheduled", False)
    return out


def cmd_tasks(a):
    rows = [t for t in items("T")
            if (a.status == "all" or t.get("status") == a.status)
            and (not a.milestone or t.get("milestone") == a.milestone)
            and (not a.area or t.get("area") == a.area)
            and (not a.kind or t.get("kind") == a.kind)]
    for t in rows:
        print(f"{t['id']}  {t.get('milestone', '?'):5} {t.get('kind', '?'):8} "
              f"{t.get('area', '?'):12} {t.get('title', '?')}")
    print(f"({len(rows)} tasks)")


def cmd_milestones(a):
    known = milestones()
    count = {}
    for t in items("T"):
        c = count.setdefault(t.get("milestone"), {})
        c[t.get("status")] = c.get(t.get("status"), 0) + 1
    for m, (name, done) in known.items():
        c = count.get(m, {})
        if c or not done:
            mark = "done" if done else "    "
            print(f"{m:5} {mark} open {c.get('open', 0):3}  done {c.get('done', 0):3}  {name}")


def cmd_decisions(a):
    rows = [d for d in items("D") if a.status == "all" or d.get("status") == a.status]
    for d in rows:
        print(f"{d['id']}  {d.get('status', '?'):7} {d.get('title', '?')}")
    print(f"({len(rows)} decisions)")


def find(ident):
    prefix = ident[:1].upper()
    for it in items(prefix):
        if it.get("id") == ident.upper():
            return it["_file"]
    sys.exit(f"no {ident}")


def cmd_show(a):
    with open(find(a.id), encoding="utf-8") as f:
        sys.stdout.write(f.read())


def slug(text):
    text = re.sub(r"[^a-z0-9 ]+", " ", text.lower())
    ws = [w for w in text.split() if w not in ("the", "a", "an", "and", "of", "to", "is", "s")]
    return "-".join(ws[:6]) or "x"


def cmd_new(a):
    prefix = "T" if a.what == "task" else "D"
    used = [int(it["id"][2:]) for it in items(prefix) if re.match(r"[TD]-\d{4}$", it.get("id", ""))]
    ident = f"{prefix}-{max(used, default=0) + 1:04d}"
    if prefix == "T":
        if a.milestone not in milestones():
            sys.exit(f"unknown milestone {a.milestone!r}; see `python tools/track.py milestones`")
        head = [f"id: {ident}", f"title: {a.title}", f"milestone: {a.milestone}", "status: open",
                f"kind: {a.kind}", f"area: {a.area}"]
        body = "What is wrong or missing, why it matters, and where in the code it lives."
    else:
        head = [f"id: {ident}", f"title: {a.title}", "status: open"]
        body = "The question, the options, and — once the user decides — the resolution and the spec sections it changes."
    path = os.path.join(DOCS, KINDS[prefix], f"{ident}-{slug(a.title)}.md")
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write("---\n" + "\n".join(head) + "\n---\n\n" + body + "\n")
    print(os.path.relpath(path, ROOT).replace(os.sep, "/"))


def cmd_check(a):
    problems = []
    known = milestones()
    ids = {}
    for prefix, keys, statuses in (("T", TASK_KEYS, TASK_STATUS), ("D", DECISION_KEYS, DECISION_STATUS)):
        for it in items(prefix):
            name = os.path.basename(it["_file"])
            for k in keys:
                if not it.get(k):
                    problems.append(f"{name}: no `{k}`")
            if it.get("id") and not name.startswith(it["id"] + "-"):
                problems.append(f"{name}: id {it['id']} does not match the file name")
            if it.get("id") in ids:
                problems.append(f"{name}: id {it['id']} also used by {ids[it['id']]}")
            ids[it.get("id")] = name
            if it.get("status") and it["status"] not in statuses:
                problems.append(f"{name}: status `{it['status']}` is not one of {sorted(statuses)}")
            if prefix == "T":
                if it.get("milestone") and it["milestone"] not in known:
                    problems.append(f"{name}: unknown milestone `{it['milestone']}`")
                if it.get("kind") and it["kind"] not in TASK_KIND:
                    problems.append(f"{name}: kind `{it['kind']}` is not one of {sorted(TASK_KIND)}")

    # §N.M cross-references in the spec must name a heading, or a decision for §18.N.
    spec = os.path.join(DOCS, "spec")
    heads, refs = set(), set()
    for name in os.listdir(spec):
        with open(os.path.join(spec, name), encoding="utf-8") as f:
            text = f.read()
        heads.update(re.findall(r"^#+ (\d+\.\d+)", text, re.M))
        refs.update((name, r) for r in re.findall(r"§(\d+\.\d+)", text))
    for name, r in sorted(refs):
        major, minor = r.split(".")
        if r in heads or (major == "18" and f"D-{int(minor):04d}" in ids):
            continue
        problems.append(f"spec/{name}: §{r} names no heading")

    # T-NNNN and D-NNNN mentioned in the docs must exist.
    for folder, _, files in os.walk(DOCS):
        for name in files:
            if name.endswith(".md"):
                with open(os.path.join(folder, name), encoding="utf-8") as f:
                    for r in set(re.findall(r"\b([TD]-\d{4})\b", f.read())):
                        if r not in ids:
                            problems.append(f"{os.path.relpath(os.path.join(folder, name), DOCS)}: {r} does not exist")

    for p in problems:
        print(p)
    print(f"{len(problems)} problems" if problems else "ok")
    return 1 if problems else 0


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    t = sub.add_parser("tasks")
    t.add_argument("--milestone")
    t.add_argument("--status", default="open")
    t.add_argument("--area")
    t.add_argument("--kind")
    sub.add_parser("milestones")
    d = sub.add_parser("decisions")
    d.add_argument("--status", default="all")
    s = sub.add_parser("show")
    s.add_argument("id")
    n = sub.add_parser("new")
    n.add_argument("what", choices=["task", "decision"])
    n.add_argument("title")
    n.add_argument("--milestone", default="none")
    n.add_argument("--area", default="none")
    n.add_argument("--kind", default="feature")
    sub.add_parser("check")
    a = p.parse_args()
    return {"tasks": cmd_tasks, "milestones": cmd_milestones, "decisions": cmd_decisions,
            "show": cmd_show, "new": cmd_new, "check": cmd_check}[a.cmd](a) or 0


if __name__ == "__main__":
    sys.exit(main())
