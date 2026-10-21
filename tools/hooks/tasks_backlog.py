"""PostToolUse hook: regenerate docs/tasks.md after anything touches a task file.

docs/tasks.md is what a new session reads to answer "what is next", so it must never lag the
files in docs/tasks/. Writes, edits and Bash commands that mention a task file or the roadmap
(whose rows name the milestones) trigger it; the backlog is rewritten only when it changed.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import track  # noqa: E402

WATCHED = ("docs/tasks/", "19-roadmap", "track.py new")


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return 0
    data = event.get("tool_input", {})
    # Windows paths arrive with `\`, sometimes mixed with `/`.
    text = " ".join(str(data.get(k, "")) for k in ("file_path", "command")).replace("\\", "/")
    if any(w in text for w in WATCHED) and not track.backlog_is_current():
        track.write_backlog()
    return 0


if __name__ == "__main__":
    sys.exit(main())
