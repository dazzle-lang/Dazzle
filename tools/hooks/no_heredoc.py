"""PreToolUse hook for Bash: refuse a command that contains a heredoc.

Heredocs corrupt `\\` in this environment and fail silently (see CLAUDE.md), so the rule is
enforced here rather than trusted to memory. Exit code 2 blocks the call and hands stderr
back to Claude.
"""
import json
import re
import sys

# `<<EOF`, `<<'EOF'`, `<<"EOF"`, `<<-EOF`, but not the here-string `<<<`.
OPENER = re.compile(r"(?<!<)<<-?\s*(['\"]?)([A-Za-z_][A-Za-z0-9_]*)\1")


def has_heredoc(command):
    lines = command.split("\n")
    for i, line in enumerate(lines):
        for m in OPENER.finditer(line):
            word = m.group(2)
            # A heredoc needs its terminator alone on a later line; `a << b` in a C++ snippet
            # inside a grep pattern has none.
            if any(rest.strip() == word for rest in lines[i + 1 :]):
                return True
    return False


def main():
    try:
        command = json.load(sys.stdin).get("tool_input", {}).get("command", "")
    except ValueError:
        return 0
    if has_heredoc(command):
        sys.stderr.write(
            "Blocked: this command uses a heredoc, which this repository forbids (CLAUDE.md): "
            "heredocs silently corrupt `\\\\` here. Write the file with the Write tool, or "
            "Write a script to the scratchpad and run it in a separate Bash call.\n"
        )
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
