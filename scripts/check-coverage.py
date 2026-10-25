#!/usr/bin/env python3
"""E0.4 coverage gate: reads a `cargo llvm-cov --summary-only` report (stdin or a file) and fails
when a crate's line coverage is below its plan threshold: 85% for plumb-psg, plumb-patch,
plumb-validation, plumb-expr, plumb-sim and plumb-intake, 70% for every other crate."""
import collections
import re
import sys

HIGH = {"plumb-psg", "plumb-patch", "plumb-validation", "plumb-expr", "plumb-sim", "plumb-intake"}

def main() -> int:
    text = open(sys.argv[1]).read() if len(sys.argv) > 1 else sys.stdin.read()
    lines = collections.defaultdict(lambda: [0, 0])
    for row in text.splitlines():
        parts = row.split()
        match = re.search(r"(plumb-[a-z]+)/src", parts[0]) if parts else None
        if not match or len(parts) < 9:
            continue
        lines[match.group(1)][0] += int(parts[7])
        lines[match.group(1)][1] += int(parts[8])
    if not lines:
        print("no crate coverage rows found")
        return 1
    failed = False
    for crate, (total, missed) in sorted(lines.items()):
        cover = 100.0 * (total - missed) / total
        threshold = 85.0 if crate in HIGH else 70.0
        ok = cover >= threshold
        failed |= not ok
        print(f"{'ok  ' if ok else 'FAIL'} {crate:18} {cover:5.1f}% (threshold {threshold:.0f}%)")
    return 1 if failed else 0

if __name__ == "__main__":
    sys.exit(main())
