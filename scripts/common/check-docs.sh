#!/bin/sh
# check-docs.sh - do the documents still resolve, and are they written the way
# this repository writes documents?
#
# The defect this exists to catch is a document that was true when it was
# written. Four shapes of it, and every one is invisible to every other check:
#
#   - a link or a path that stopped resolving when something was renamed;
#   - a fenced shell block that does not parse, which is a block nobody can
#     copy and paste;
#   - an angle-bracket placeholder inside a shell block: a human reads it as
#     "fill this in" and bash reads it as a redirect, so the reader gets a
#     cryptic syntax error instead of an obvious instruction;
#   - a PowerShell invocation in a block without -NoProfile, which runs the
#     profile scripts of whoever owns the machine before the command, and one
#     that prints a line puts that line in front of the output.
#
# CONTROL BYTES ARE NOT CHECKED HERE. That rule scanned markdown only while
# every .ts, .py, .rs and .sh in the tree went unchecked, so it moved to
# check-control-bytes.sh, which reads every text file. Run both.
#
# THE CHARACTER HALF OF THE PROSE RULE IS NOT HERE. ASCII only, no emoji, no
# marker character, and no dash respelt in hyphens: those belong to
# check-markers.sh, which reads every tracked text file rather than markdown
# alone. Run both. What stays here is what is specific to a document: links,
# fenced blocks, placeholders and orphan pages.
#
# WRITING STYLE IS NOT CHECKED HERE AND MUST NOT BE. This header claimed a
# banned-vocabulary rule for as long as no version of this check implemented
# one, and arming it settled the question rather than closing it: a word list
# catches `blazing`, which nobody writes, and cannot catch `load-bearing`,
# which the template wrote twelve times. The register that actually goes wrong
# in an agent's prose is metaphor used as jargon, and no list can hold it.
# docs/conventions/prose.md names the standard and the review pass reads for
# it.
#
# WHAT IT DOES NOT CHECK IS WHETHER A CLAIM IS TRUE. That is a reading, and
# it belongs to the review pass. A guard that tried to verify prose would
# either pass vacuously or refuse legitimate writing, and both are worse than
# an honest scope.
#
# EVERY PER-LINE TEST IS DONE IN awk, NOT IN A SHELL LOOP. The first version
# ran a pipeline per line of every file, which is tens of thousands of process
# spawns, and it did not finish in two minutes on Windows. It then ran two
# awk passes, a dirname and a subshell PER FILE, plus a tr and a grep PER
# BLOCK: about a thousand spawns over this tree, and a spawn under Git Bash
# costs more than reading the file does. It is now one awk pass over every
# file.
#
# Measured on one Windows 11 machine (10.0.26200) under Git Bash 5.3.15 on
# 2026-09-10, the two versions run back to back: 25.8s over 62 documents
# before, 4.9s over 65 after. The second tree is the LARGER one, so the
# comparison understates the change rather than flattering it. The `sh -n`
# calls, one per fenced block, are most of what is left.
#
# THOSE `sh -n` CALLS ARE NOT BATCHED, and the saving was declined on
# purpose. Concatenating every block of a file into one parse would let an
# unterminated quote in one block be closed by the next, so a broken block
# would report clean. A false pass is not worth the seconds.
#
# Usage:
#   sh scripts/common/check-docs.sh
#   sh scripts/common/check-docs.sh --json
#   sh scripts/common/check-docs.sh --path docs
#
# Exit codes: 0 clean, 1 something is wrong, 2 could not run.
#
# Read the exit code from this process, unpiped.

set -u

JSON=0
SCOPE=""

while [ $# -gt 0 ]; do
  case "$1" in
    --json) JSON=1 ;;
    --path) shift; SCOPE="${1:-}" ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-docs: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v git >/dev/null 2>&1 || { printf 'check-docs: git not found\n' >&2; exit 2; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { printf 'check-docs: not a git repository\n' >&2; exit 2; }
command -v awk >/dev/null 2>&1 || { printf 'check-docs: awk not found\n' >&2; exit 2; }
SELF=check-docs
REPO_ROOT=$(git rev-parse --show-toplevel)

# EVERY git QUERY BELOW RUNS FROM THE REPOSITORY ROOT. `git ls-files` is
# relative to the process working directory, so without this a run from a
# subdirectory silently scopes itself to that subtree and reports clean over
# everything else. The scope of a guard must not depend on who called it.
cd "$REPO_ROOT" || { printf '%s: cannot enter %s\n' "$SELF" "$REPO_ROOT" >&2; exit 2; }

# TRACKED **PLUS UNTRACKED-BUT-NOT-IGNORED**. `git ls-files` alone cannot see
# a file that has never been staged, which is exactly when a new file is most
# likely to carry a defect and exactly what the next `git add -A` will take.
# Ignored files stay out: they are ignored on purpose.
list_files() {
  {
    git ls-files -- "$@" 2>/dev/null
    git ls-files --others --exclude-standard -- "$@" 2>/dev/null
  } | LC_ALL=C sort -u
}

WORK="${TMPDIR:-/tmp}/.checkdocs.$$"
mkdir -p "$WORK" || { printf '%s: cannot write to %s\n' "$SELF" "$WORK" >&2; exit 2; }
trap 'rm -rf "$WORK"' EXIT INT TERM

if [ -n "$SCOPE" ]; then
  list_files "$SCOPE" | grep '\.md$' > "$WORK/list" || true
else
  list_files '*.md' > "$WORK/list" || true
fi
[ -s "$WORK/list" ] || { printf 'check-docs: no markdown files in scope\n' >&2; exit 2; }

NFILES=$(awk 'END { print NR }' "$WORK/list")

PROBLEMS=""
COUNT=0
NLINKS=0
NBLOCKS=0

report() { PROBLEMS="$PROBLEMS  $1
"; COUNT=$((COUNT + 1)); }

# ONE awk PASS, READING EVERY FILE ITSELF. The paths reach it through a list
# file rather than an argument list: a large enough tree overruns the command
# line, and the failure there is a truncated scope reporting success.
#
# It emits four kinds of record, tab separated, and writes each fenced shell
# block to its own file with carriage returns already stripped:
#
#   LINK   file line target   a relative link, code spans removed
#   RAW    file target        every link, code spans KEPT, for the orphan pass
#   BLOCK  file line index p q  a fenced sh/bash block. p=1 when it holds a
#                             shell-unsafe angle-bracket placeholder, q=1 when
#                             it invokes PowerShell without -NoProfile.
#
# LINK AND RAW DIFFER ON PURPOSE, and collapsing them would move an answer.
# Stripping code spans is why `[int](2.65)` inside backticks is not reported as
# a broken link; NOT stripping them for the orphan pass is why a page named
# only inside a code span still counts as linked.
LC_ALL=C awk -v work="$WORK" '
  function strip_spans(s,   out) {
    out = s
    while (match(out, /`[^`]*`/))
      out = substr(out, 1, RSTART - 1) substr(out, RSTART + RLENGTH)
    return out
  }
  function emit_links(kind, file, ln, s,   rest, t) {
    rest = s
    while (match(rest, /\]\([^)\t ]+/)) {
      t = substr(rest, RSTART + 2, RLENGTH - 2)
      if (kind == "LINK") printf "LINK\t%s\t%d\t%s\n", file, ln, t
      else                printf "RAW\t%s\t%s\n", file, t
      rest = substr(rest, RSTART + RLENGTH)
    }
  }
  BEGIN {
    while ((getline path < ARGV[1]) > 0) {
      if (path == "") continue
      fence = 0; inb = 0; nblk = 0; n = 0
      while ((getline line < path) > 0) {
        n++
        # Trailing CR removed once, here, rather than by a tr per block.
        sub(/\r$/, "", line)

        if (line ~ /^[ \t]*```/) {
          if (inb) { inb = 0; fence = 0; continue }
          fence = !fence
          if (fence && line ~ /^[ \t]*```(bash|sh)[ \t]*$/) {
            inb = 1; nblk++
            blkfile[nblk] = work "/blk." NR "." nblk
            blkstart[nblk] = n
            blkplace[nblk] = 0
            blknoprof[nblk] = 0
            cur = blkfile[nblk]
            printf "" > cur
          }
          continue
        }
        if (inb) {
          print line > cur
          if (line ~ /<[a-z][a-z0-9-]*>/) blkplace[nblk] = 1
          # A PowerShell invocation without -NoProfile runs the profile
          # scripts of whoever owns the machine first, and one that prints a
          # line puts that line in front of the command output. Every example
          # in this repository already passed it and nothing checked.
          # docs/conventions/shell.md section 8.
          #
          # THE LINE MUST START WITH THE COMMAND, and the first version did
          # not require that. It matched the name anywhere, so a comment inside
          # a block naming the preferred shell was reported as an invocation
          # missing a flag. Refusing correct writing is what the scope note at
          # the top of this file calls worse than not checking, and a planted
          # fixture is what showed it. The cost is a false negative on a
          # wrapped or prefixed invocation, which is the safer direction.
          #
          # NO APOSTROPHE BELONGS IN THIS awk PROGRAM. It is inside a single
          # quoted argument, so one ends the program: the comment above carried
          # one for a minute and the shell reported a syntax error 40 lines
          # away from it.
          if (line ~ /^[ \t]*(pwsh|powershell)([ \t]|$)/ &&
              line !~ /-NoProfile/) blknoprof[nblk] = 1
          continue
        }
        if (fence) continue

        clean = strip_spans(line)
        emit_links("LINK", path, n, clean)
        emit_links("RAW", path, n, line)
      }
      close(path)
      for (i = 1; i <= nblk; i++) {
        close(blkfile[i])
        printf "BLOCK\t%s\t%d\t%s\t%d\t%d\n", path, blkstart[i], blkfile[i], blkplace[i], blknoprof[i]
      }
      NR++    # keeps block filenames unique across files
    }
    close(ARGV[1])
  }
' "$WORK/list" > "$WORK/find" 2>/dev/null

TAB=$(printf '\t')
while IFS="$TAB" read -r kind a b c d e; do
  case "${kind:-}" in
    LINK)
      case "$c" in http://*|https://*|mailto:*|'') continue ;; esac
      NLINKS=$((NLINKS + 1))
      target=${c%%#*}
      [ -z "$target" ] && continue
      dir=${a%/*}
      [ "$dir" = "$a" ] && dir=.
      [ -e "$dir/$target" ] || report "$a:$b broken link -> $c" ;;
    BLOCK)
      NBLOCKS=$((NBLOCKS + 1))
      [ -f "$c" ] || continue
      sh -n "$c" 2>/dev/null || report "$a:$b shell block does not parse"
      [ "$d" = "1" ] && report "$a:$b shell-unsafe placeholder. bash reads it as a redirect; use UPPER_SNAKE"
      [ "$e" = "1" ] && report "$a:$b a PowerShell invocation without -NoProfile. docs/conventions/shell.md section 8" ;;
  esac
done < "$WORK/find"

# -- a page nothing links to -------------------------------------------------
# AN UNLINKED PAGE IS NOT READ, SO IT IS NOT CORRECTED, and that is the state
# every stale document passes through on the way to being wrong.
#
# This rule was written in docs/conventions/prose.md with nothing enforcing
# it, and within the hour a new prompt file was added that nothing referenced.
# A door sweep found it by hand. A rule that can be checked should be a check.
#
# Roots are exempt: a README is an entry point, and the files at the repository
# root are what a reader or a raw URL arrives at directly.
#
# `a/../b` is collapsed so a link from a subdirectory and one from the root
# name the same file. This used to be a sed with a loop label; awk does it in
# the same pass that normalises the path.
awk -F"$TAB" '
  $1 == "RAW" {
    tgt = $3
    if (tgt ~ /^(https?:|mailto:)/ || tgt == "") next
    sub(/#.*$/, "", tgt)
    if (tgt == "") next
    d = $2
    if (sub(/\/[^\/]*$/, "", d)) full = d "/" tgt; else full = tgt
    while (sub(/[^\/][^\/]*\/\.\.\//, "", full)) { }
    sub(/^\.\//, "", full)
    print full
  }
' "$WORK/find" | LC_ALL=C sort -u > "$WORK/linked"

while IFS= read -r f; do
  case "$f" in
    */README.md|README.md) continue ;;
    */*) ;;
    *) continue ;;   # a root-level file is an entry point
  esac
  grep -qxF "$f" "$WORK/linked" || report "$f is linked from nowhere. An unlinked page is not read, so it is not corrected."
done < "$WORK/list"

# -- the character rule moved, it was NOT dropped -------------------------
# THE CHARACTER RULES AND THE EM-DASH RULE LIVE IN check-markers.sh, over
# EVERY tracked text file rather than over markdown alone. Two checks enforcing
# one rule is two places for it to be wrong, and these two would have been
# wrong differently: this one strips fenced blocks and code spans before it
# looks and a whole-tree scan that did not would refuse the page that names
# the character it bans.
#
# It is the same move the control-byte rule made out of this file, for the
# same reason. In the template, the markdown-only scan left 2290 characters in
# 22 scripts unchecked. Run both: this one for documents, that one for the
# whole tree.

# -- report ------------------------------------------------------------------
if [ "$JSON" = "1" ]; then
  printf '{"schema":"check-docs/1","problems":%s,"files":%s,"links":%s,"shell_blocks":%s}\n' \
    "$COUNT" "$NFILES" "$NLINKS" "$NBLOCKS"
  [ "$COUNT" -gt 0 ] && exit 1
  exit 0
fi

if [ "$COUNT" -gt 0 ]; then
  printf 'documentation check failed, %s problem(s):\n\n%s\n' "$COUNT" "$PROBLEMS"
  exit 1
fi

printf 'docs ok: %s files, %s relative links, %s shell blocks. Links and prose clean.\n' \
  "$NFILES" "$NLINKS" "$NBLOCKS"
exit 0
