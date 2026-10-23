#!/bin/sh
# check-control-bytes.sh - is there a literal control byte in any text file?
#
# The defect this exists to catch is a file that is invisible to review. A
# literal control byte makes a file unreadable to BOTH review tools at once:
#
#   - `grep` calls it binary and SKIPS it, saying so in a line nobody reads,
#     and "what else calls this?" is how most real holes get found;
#   - `git diff` prints "Binary files differ", so a code review of the file
#     shows NO DIFF AT ALL. `git diff --text` renders it fine, which is the
#     proof that only reviewability was ever at stake.
#
# The runtime value is identical either way. Write the escape, not the byte:
# `\0` is the same character and stays reviewable. Because correctness never
# depends on it, this survives for a long time unnoticed.
#
# IT IS NOT A RULE PEOPLE CAN REMEMBER. In the project this came from, the
# rule was stated in a shared source file and four source files broke it anyway,
# and the post-mortem writing up the lesson reintroduced the byte TWICE while
# writing about it. A rule that a careful person breaks while documenting it is
# a rule that needs a check.
#
# -- THE THREE BLIND SPOTS THIS SCOPE WAS PAID FOR ---------------------------
#
# 1. TRACKED ALONE IS NOT ENOUGH. `git ls-files` cannot see a file that has
#    never been staged, which is exactly when a new file is most likely to
#    acquire a stray byte. A brand-new test file was written with a literal NUL
#    where a trailing space belonged; grep called it binary, an assertion went
#    green for the wrong reason, and the guard reported clean because the file
#    was not tracked yet.
#
# 2. `git ls-files` IS RELATIVE TO THE PROCESS WORKING DIRECTORY, so this
#    guard's scope used to depend on who called it. Run from the repository
#    root it saw 1071 files; run from one package directory, which is where a
#    per-package test script invoked it, it saw 391 and nothing else. Whole
#    trees were outside the scope of the one invocation that ran on every gate,
#    and a literal NUL rode through two handoffs that each reported green.
#    A guard cannot prove itself. It is pinned to the repository root here.
#
# 3. BINARIES ARE OUT OF SCOPE BY CONSTRUCTION, not by an allowlist. The
#    extension list below says what IS text. An "allowlist of binaries that are
#    fine" is the kind of list that quietly absorbs a real finding.
#
# MARKDOWN IS IN SCOPE HERE AND NOWHERE ELSE. check-docs.sh used to carry a
# markdown-only copy of this rule. Two checks enforcing one rule is two places
# for it to be wrong, so the rule lives here and check-docs.sh points at it.
#
# Usage:
#   sh scripts/common/check-control-bytes.sh
#   sh scripts/common/check-control-bytes.sh --json
#
# Exit codes: 0 clean, 1 a byte was found, 2 could not run.
#
# Read the exit code from this process, unpiped.

set -u

JSON=0

while [ $# -gt 0 ]; do
  case "$1" in
    --json) JSON=1 ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-control-bytes: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v git >/dev/null 2>&1 || { printf 'check-control-bytes: git not found\n' >&2; exit 2; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { printf 'check-control-bytes: not a git repository\n' >&2; exit 2; }
command -v awk >/dev/null 2>&1 || { printf 'check-control-bytes: awk not found\n' >&2; exit 2; }
REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT" || { printf 'check-control-bytes: cannot enter %s\n' "$REPO_ROOT" >&2; exit 2; }

# Extensions asserted to be TEXT. Anything else is out of scope by construction.
# check-markers.sh carries the same list. Change both, or the two checks read
# different trees.
TEXT_RE='\.(ts|tsx|js|mjs|cjs|jsx|json|md|sql|css|scss|html|toml|yaml|yml|sh|ps1|py|rs|go|c|h|cpp|hpp|java|rb|php|txt|cfg|ini|conf)$'

# THE FILE LIST GOES THROUGH A FILE, LINE BY LINE, NEVER THROUGH WORD
# SPLITTING. `for f in $FILES` splits on every space, so a path containing one
# became two paths that do not exist, both failed the `[ -f ]` test, both were
# skipped, and the check reported success over a file it had never opened.
# Found with a fixture, not by reasoning: a file named with a space and
# carrying a NUL was not reported by the version that split on words.
WORK="${TMPDIR:-/tmp}/.checkcb.$$"
mkdir -p "$WORK" || { printf 'check-control-bytes: cannot write to %s\n' "$WORK" >&2; exit 2; }
trap 'rm -rf "$WORK"' EXIT INT TERM

{
  git ls-files 2>/dev/null
  git ls-files --others --exclude-standard 2>/dev/null
} | LC_ALL=C sort -u | grep -E "$TEXT_RE" > "$WORK/all" || true

NFILES=0
: > "$WORK/list"
while IFS= read -r f; do
  [ -n "$f" ] || continue
  [ -f "$f" ] || continue          # tracked but deleted; git reports that itself
  NFILES=$((NFILES + 1))
  printf '%s\n' "$f" >> "$WORK/list"
done < "$WORK/all"

if [ "$NFILES" = "0" ]; then
  printf 'check-control-bytes: no text files in scope\n' >&2
  exit 2
fi

# ONE awk PASS OVER EVERY FILE, NOT FIVE PROCESSES PER FILE. The loop this
# replaced ran grep, head, cut, wc and tr per file: about five spawns each, and
# a spawn under Git Bash costs more than reading the file does.
#
# Measured on one Windows 11 machine (10.0.26200) under Git Bash 5.3.15 on
# 2026-09-10, the two versions run back to back: 13.4s over 106 text files
# before, 0.6s over 111 after. The second tree is the LARGER one, so the
# comparison understates the change rather than flattering it.
#
# NUL IS WHY THIS IS NOT SIMPLY grep. A NUL cannot live in a shell variable,
# so it cannot be put in a pattern, and measured here on GNU grep 3.0 a file
# holding one is neither matched nor reported as binary: it is passed over in
# silence. awk finds it, because `sprintf("%c", 0)` builds the byte inside awk
# where no shell touches it.
#
# AND awk IS NOT ASSUMED TO BE ABLE TO. Some awk implementations hold a
# record as a C string and truncate it at the first NUL, which would make this
# check report clean over a file full of them. That is the defect class this
# repository exists to refuse, so the ability is PROVED against a fixture on
# every run, and a `no` selects a byte-at-a-time reader rather than a wrong
# answer. The fixture crosses a pipe, never a variable.
NUL_OK=no
if printf 'a\000b\n' | LC_ALL=C awk 'BEGIN { z = sprintf("%c", 0) } index($0, z) > 0 { f = 1 } END { exit(f ? 0 : 1) }' 2>/dev/null; then
  NUL_OK=yes
fi
if [ "$NUL_OK" = "no" ] && ! command -v od >/dev/null 2>&1; then
  printf 'check-control-bytes: this awk cannot see a NUL byte, and od is not\n' >&2
  printf '  installed, so nothing here can. That is "could not run", not a pass.\n' >&2
  exit 2
fi

# IT REPORTS THE FIRST OFFENDING BYTE, ITS LINE AND ITS VALUE. An earlier
# version reported a class name instead of a byte and took its line number
# out of `grep -n` output, which reads
# `Binary file X matches` whenever the file also held a NUL, so the reported
# line number was a fragment of that sentence. The fixture found that too.
LC_ALL=C awk -v nulok="$NUL_OK" '
  function q(s) { gsub(/\047/, "\047\\\047\047", s); return "\047" s "\047" }
  function scan_lines(path,   n, line, i, ch, v) {
    n = 0
    while ((getline line < path) > 0) {
      n++
      for (i = 1; i <= length(line); i++) {
        ch = substr(line, i, 1)
        if (!(ch in ordv)) continue
        v = ordv[ch]
        if (v == 9 || v == 13) continue
        close(path)
        return n " " v
      }
    }
    close(path)
    return ""
  }
  # ONE PROCESS PER FILE, and it runs only where the reader above cannot.
  function scan_bytes(path,   cmd, n, i, v) {
    cmd = "LC_ALL=C od -An -v -tu1 " q(path)
    n = 1
    while ((cmd | getline) > 0) {
      for (i = 1; i <= NF; i++) {
        v = $i + 0
        if (v == 10) { n++; continue }
        if (v == 9 || v == 13) continue
        if (v < 32) { close(cmd); return n " " v }
      }
    }
    close(cmd)
    return ""
  }
  BEGIN {
    # awk has no ord(). The table is built once, over the range that matters.
    for (i = 0; i < 32; i++) ordv[sprintf("%c", i)] = i
    while ((getline path < ARGV[1]) > 0) {
      if (path == "") continue
      hit = (nulok == "yes") ? scan_lines(path) : scan_bytes(path)
      if (hit == "") continue
      split(hit, p, " ")
      printf "  %s:%d a control byte 0x%02x\n", path, p[1], p[2]
    }
    close(ARGV[1])
  }
' "$WORK/list" > "$WORK/findings" 2>/dev/null

COUNT=0
REPORT=""
while IFS= read -r line; do
  [ -n "$line" ] || continue
  COUNT=$((COUNT + 1))
  REPORT="$REPORT$line
"
done < "$WORK/findings"

if [ "$JSON" = "1" ]; then
  printf '{"schema":"check-control-bytes/1","problems":%s,"files":%s}\n' "$COUNT" "$NFILES"
  [ "$COUNT" -gt 0 ] && exit 1
  exit 0
fi

if [ "$COUNT" -gt 0 ]; then
  printf 'literal control bytes in %s file(s):\n\n%s\n' "$COUNT" "$REPORT"
  printf 'Write the ESCAPE, not the byte. The escape is the same character at\n'
  printf 'runtime, and the byte is what makes the file invisible to grep and\n'
  printf 'unreviewable in git diff. docs/conventions/shell.md section 6.\n'
  exit 1
fi

printf 'no literal control bytes in %s text files (tracked plus untracked-not-ignored)\n' "$NFILES"
exit 0
