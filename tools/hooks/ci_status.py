"""SessionStart hook: say whether CI passes on `master`, so a session starts knowing.

From T-0175 to T-0177 `master` failed CI on every platform, and the next session began work
without knowing: each push cancels the run before it, so a failure can sit behind cancelled
runs, and nothing showed it. This prints the newest run that finished, unless it was
cancelled, and whether one is running. A red `master` is fixed before anything else
(CLAUDE.md).

It never fails the session: without `gh`, a network or a login, it says so and stops.
"""
import json
import shutil
import subprocess
import sys


def main():
    gh = shutil.which("gh")
    if gh is None:
        print("CI status unknown: `gh` is not installed.")
        return 0
    try:
        out = subprocess.run(
            [gh, "run", "list", "--branch", "master", "--workflow", "CI", "--limit", "10",
             "--json", "status,conclusion,displayTitle,headSha,url"],
            capture_output=True, text=True, timeout=15, check=False,
        )
        runs = json.loads(out.stdout) if out.returncode == 0 else None
    except (OSError, subprocess.TimeoutExpired, ValueError):
        runs = None
    if not runs:
        print("CI status unknown: `gh run list` gave nothing (offline, or not logged in).")
        return 0

    running = [r for r in runs if r.get("status") != "completed"]
    done = [r for r in runs if r.get("status") == "completed" and r.get("conclusion") != "cancelled"]
    if running:
        r = running[0]
        print(f"CI on master: running for {r['headSha'][:7]} \"{r['displayTitle']}\".")
    if not done:
        return 0
    last = done[0]
    sha, title = last["headSha"][:7], last["displayTitle"]
    if last.get("conclusion") == "success":
        print(f"CI on master: passed at {sha} \"{title}\".")
    else:
        print(
            f"CI on master FAILED ({last.get('conclusion')}) at {sha} \"{title}\": {last['url']}\n"
            "Tell the user, and fix master before starting other work (CLAUDE.md)."
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
