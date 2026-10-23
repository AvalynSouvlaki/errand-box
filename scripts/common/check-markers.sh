#!/bin/sh
# check-markers.sh - ASCII text, no emoji, and no marker characters.
#
# Four rules, one subject, one home. docs/conventions/prose.md is the rule and
# this is the machine behind it.
#
#   1. THE CHARACTER SET. Every tracked text file is ASCII. In markdown, a
#      specimen inside a code span or a fenced block is exempt, so a page can
#      show a character it refuses.
#   2. NO EMOJI AND NO MARKER CHARACTER, ANYWHERE. Not in prose, not in a
#      script, and not inside a specimen either. A page names such a
#      character by its code point, for example U+26D4.
#   3. THE RESPELT EM DASH. Markdown prose only. Rule 1 already refuses the
#      character, and this refuses the ASCII spellings of it.
#   4. AN UNCLOSED FENCE. Markdown only, and it exists because it is a BYPASS
#      of the other three rather than a typo.
#
# The plain-text labels NOTE:, CAUTION: and WARNING: are the allowed way to
# flag a trap. They are ASCII, so nothing here refuses them.
#
# -- WHY RULE 2 HAS NO EXEMPTION -------------------------------------------
#
# The template this project came from allowed five marker characters and
# capped their density at 30 per 100 non-blank lines. The cap existed because
# an agent kept strictly to the five allowed characters and used them until
# the documents were unreadable. This project allows none, so there is no
# ceiling to raise and no allowlist to grow.
#
# The specimen exemption of rule 1 does not apply to rule 2. A specimen of an
# emoji is still an emoji on the page, and a code point names it exactly.
#
# WHAT RULE 2 COUNTS AS AN EMOJI OR A MARKER CHARACTER. A list of code point
# ranges, matched after the UTF-8 is decoded:
#
#   U+2139           the information source sign
#   U+2300-U+23FF    miscellaneous technical, which holds the clocks and the
#                    media controls
#   U+25A0-U+25FF    geometric shapes, which hold the squares and triangles
#                    that get used as bullets and status glyphs
#   U+2600-U+27BF    miscellaneous symbols and dingbats, which hold the five
#                    template markers, the check marks and the crosses
#   U+2B00-U+2BFF    miscellaneous symbols and arrows, which holds the star
#   U+FE00-U+FE0F    the variation selectors that ask for emoji presentation
#   U+200D, U+20E3   the joiner and the keycap that build an emoji sequence
#   U+1F000-U+1FAFF  every emoji block
#   U+E0000-U+E007F  the tag characters of a flag sequence
#
# The list is an explicit set of ranges and not a principle, because no check
# can decide what a reader will take as a marker. Outside markdown specimens,
# rule 1 refuses every other non-ASCII character anyway.
#
# -- WHY RULE 3 EXISTS, AND WHY IT BELONGS BESIDE RULE 1 ---------------------
#
# BANNING A CHARACTER TAUGHT AGENTS TO SPELL IT DIFFERENTLY. Reported by
# the template's operator on 2026-09-17: agents reaching for an em dash,
# finding the character refused, and writing `--` or a comma instead. The rule
# was satisfied on every run and the sentence was unchanged: keeping to the
# letter of a rule was read as compliance, and nothing measured the thing the
# rule was for.
#
# SO THE SPELLING IS THE CHECKABLE HALF AND NOT THE WHOLE RULE. What the
# rule actually asks for is a sentence that needs no dash at all. A comma
# splice carrying a parenthetical is the same defect and NOTHING HERE CAN SEE
# IT, because a comma is an ordinary character. prose.md says so in the same
# paragraph that states the rule, so a reader does not mistake a green run for
# a well-written page.
#
# ONE SPELLING IS DELIBERATELY NOT REFUSED. A spaced single hyphen reads as
# a dash to some writers, and no incident in the template involved one. A rule
# with no incident behind it is a preference, so it is not armed.
#
# -- WHY THIS READS EVERY TRACKED TEXT FILE ----------------------------------
#
# THE RULE USED TO SCAN MARKDOWN ALONE, which left every script unchecked.
# Measured on the template on 2026-08-28, before this check existed: 2290
# characters outside its allowed set, in 22 files, and every one of them was
# in a script rather than a document.
#
# Two checks enforcing one rule is two places for it to be wrong, and they
# WOULD have been wrong differently: check-docs strips fenced blocks and code
# spans before it looks, and a whole-tree scan that did not would refuse the
# page that names the character it bans. So the rule lives here entire, the
# same way the control-byte rule lives in check-control-bytes.sh.
#
# -- THE ONE EXEMPTION -------------------------------------------------------
#
# A LEADING BYTE-ORDER MARK IS EXEMPT, and only a leading one. Windows
# PowerShell 5.1 wants one at the start of a .ps1 that carries non-ASCII bytes.
# A BOM anywhere else in a file is a real defect and is still reported.
#
# Usage:
#   sh scripts/common/check-markers.sh
#   sh scripts/common/check-markers.sh --json
#
# Exit codes: 0 clean, 1 a character, a dash or a fence was refused,
# 2 could not run.
#
# Read the exit code from this process, unpiped.

set -u

JSON=0

while [ $# -gt 0 ]; do
  case "$1" in
    --json) JSON=1 ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-markers: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v git >/dev/null 2>&1 || { printf 'check-markers: git not found\n' >&2; exit 2; }
command -v awk >/dev/null 2>&1 || { printf 'check-markers: awk not found\n' >&2; exit 2; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { printf 'check-markers: not a git repository\n' >&2; exit 2; }

# PINNED TO THE REPOSITORY ROOT. `git ls-files` is relative to the process
# working directory, so a guard invoked from a subdirectory silently reports on
# a smaller tree and calls it clean. check-control-bytes.sh carries the same
# note and the same fix, and it was paid for there.
REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT" || { printf 'check-markers: cannot enter %s\n' "$REPO_ROOT" >&2; exit 2; }

# Extensions asserted to be TEXT. Anything else is out of scope by
# construction, which is the same reasoning check-control-bytes.sh states: an
# allowlist of "binaries that are fine" is the kind of list that quietly
# absorbs a real finding.
TEXT_RE='\.(ts|tsx|js|mjs|cjs|jsx|json|md|sql|css|scss|html|toml|yaml|yml|sh|ps1|py|rs|go|c|h|cpp|hpp|java|rb|php|txt|cfg|ini|conf)$'

# TRACKED PLUS UNTRACKED-BUT-NOT-IGNORED. A file that has never been staged
# is exactly when a new file is likeliest to carry the defect, and it is what
# the next `git add -A` would take.
FILES=$(
  {
    git ls-files 2>/dev/null
    git ls-files --others --exclude-standard 2>/dev/null
  } | sort -u | grep -E "$TEXT_RE" || true
)
if [ -z "$FILES" ]; then
  printf 'check-markers: no text files in scope\n' >&2
  exit 2
fi

TMP="${TMPDIR:-/tmp}/.checkmarkers.$$"
mkdir -p "$TMP" || { printf 'check-markers: cannot write to %s\n' "$TMP" >&2; exit 2; }
trap 'rm -rf "$TMP"' EXIT INT TERM

# THE BOM IS BUILT BY printf, NOT WRITTEN AS A LITERAL, and the pass is
# matched as BYTES. Two separate reasons, and both have bitten a check before:
#
#   1. A pattern written as a literal escape is not expanded by every tool.
#      POSIX grep reads `\001` inside a bracket expression as a backslash and
#      two digits, and that exact mistake once reported a control byte in all
#      48 clean files of a tree. printf makes real bytes first.
#   2. A byte class under a UTF-8 locale splits a three-byte character into
#      three fragments, so a count comes out three times too high and looks
#      like real output. LC_ALL=C below states that this pass is byte-oriented,
#      and the awk program decodes the UTF-8 itself.
BOM=$(printf '\357\273\277')

PROBLEMS=0
NFILES=0
NEMOJI=0
REPORT=""

report() { REPORT="$REPORT  $1
"; PROBLEMS=$((PROBLEMS + 1)); }

for f in $FILES; do
  [ -f "$f" ] || continue          # tracked but deleted; git reports that itself
  NFILES=$((NFILES + 1))

  case "$f" in
    *.md) IS_MD=1 ;;
    *)    IS_MD=0 ;;
  esac

  # One awk pass per file: find the first emoji on each raw line, strip the
  # exempt regions, and emit every other offending position.
  LC_ALL=C awk -v BOM="$BOM" -v ISMD="$IS_MD" '
    # Decode the UTF-8 sequence that starts at byte i of s. The code point is
    # the return value; its length in bytes goes into the global CPLEN.
    function decode(s, i,   b1) {
      b1 = ORD[substr(s, i, 1)]
      if (b1 >= 240) {
        CPLEN = 4
        return (b1 % 8) * 262144 + (ORD[substr(s, i+1, 1)] % 64) * 4096 \
             + (ORD[substr(s, i+2, 1)] % 64) * 64 + (ORD[substr(s, i+3, 1)] % 64)
      }
      if (b1 >= 224) {
        CPLEN = 3
        return (b1 % 16) * 4096 + (ORD[substr(s, i+1, 1)] % 64) * 64 \
             + (ORD[substr(s, i+2, 1)] % 64)
      }
      if (b1 >= 192) {
        CPLEN = 2
        return (b1 % 32) * 64 + (ORD[substr(s, i+1, 1)] % 64)
      }
      CPLEN = 1
      return b1
    }
    # Rule 2. The ranges are the ones the header lists, and only those.
    function is_emoji(cp) {
      if (cp == 8505) return 1                          # U+2139
      if (cp >= 8960 && cp <= 9215) return 1            # U+2300-U+23FF
      if (cp >= 9632 && cp <= 9727) return 1            # U+25A0-U+25FF
      if (cp >= 9728 && cp <= 10175) return 1           # U+2600-U+27BF
      if (cp >= 11008 && cp <= 11263) return 1          # U+2B00-U+2BFF
      if (cp >= 65024 && cp <= 65039) return 1          # U+FE00-U+FE0F
      if (cp == 8205 || cp == 8419) return 1            # U+200D, U+20E3
      if (cp >= 126976 && cp <= 129791) return 1        # U+1F000-U+1FAFF
      if (cp >= 917504 && cp <= 917631) return 1        # U+E0000-U+E007F
      return 0
    }
    BEGIN {
      for (i = 0; i < 256; i++) ORD[sprintf("%c", i)] = i
      fence = 0
    }
    {
      line = $0

      # A LEADING BOM IS EXEMPT, AND ONLY A LEADING ONE. Stripped from the
      # first line of the file and nowhere else, so a BOM that a merge left in
      # the middle of a file is still a finding.
      if (NR == 1 && index(line, BOM) == 1) line = substr(line, length(BOM) + 1)

      # RULE 2 READS THE RAW LINE, BEFORE ANY SPECIMEN IS CUT OUT, and before
      # the fence logic can skip the line. That is the whole difference from
      # rule 1: a fenced block or a code span does not exempt an emoji.
      emoji = 0
      for (i = 1; i <= length(line); i++) {
        if (ORD[substr(line, i, 1)] < 128) continue
        cp = decode(line, i)
        if (is_emoji(cp)) { printf "EMOJI\t%d\t%04X\n", NR, cp; emoji = 1; break }
        i += CPLEN - 1
      }

      line2 = line

      # THE SPECIMEN EXEMPTION OF RULE 1, markdown only. A fenced block is
      # skipped entire and an inline code span is cut out, so a page can name
      # a character it bans. Outside markdown there is no exemption: a source
      # file has no reader who needs a specimen.
      if (ISMD) {
        if (line ~ /^[ \t]*```/) { fence = !fence; if (fence) fenceline = NR; next }
        if (fence) next
        while (match(line2, /`[^`]*`/))
          line2 = substr(line2, 1, RSTART - 1) substr(line2, RSTART + RLENGTH)

        # AN EM DASH RESPELT IN HYPHENS. Rule 3, and its scope is prose in a
        # markdown file. A quoted line, a table row and a heading are out:
        # quoted wording is kept verbatim by a rule of its own, and neither of
        # the other two is a sentence. Link targets and bare URLs go too,
        # because a hyphen pair inside a path somebody else owns is not
        # writing. An apostrophe anywhere in this comment ENDS THE awk
        # PROGRAM, because the program is single-quoted by the shell around
        # it. That is the trap docs/conventions/shell.md section 1 documents,
        # and it fired in the template while this rule was written.
        if (line !~ /^[ \t]*>/ && line !~ /^[ \t]*\|/ && line !~ /^[ \t]*#/) {
          d = line2
          gsub(/\]\([^)]*\)/, " ", d)
          gsub(/https?:\/\/[^ )]*/, " ", d)
          if (d ~ /[ \t]--+[ \t]/ || d ~ /[A-Za-z0-9]--+[A-Za-z0-9]/)
            printf "DASH\t%d\t-\n", NR
        }
      }

      # Rule 1. Whatever survives must be ASCII. Report the first offender per
      # line; a line with one wrong character usually has several of the same.
      # A line that rule 2 already reported is not reported twice.
      #
      # THE CODEPOINT IS DECODED AND REPORTED, not just the position. U+2014
      # tells a reader exactly which character to search for.
      if (emoji) next
      for (i = 1; i <= length(line2); i++) {
        if (ORD[substr(line2, i, 1)] < 128) continue
        printf "CHAR\t%d\t%04X\n", NR, decode(line2, i)
        break
      }
    }
    END {
      # AN UNCLOSED FENCE IS A BYPASS, NOT A TYPO. The specimen exemption
      # skips every line inside a fenced block, so a markdown file with an odd
      # number of fence markers switches the character rule and the dash rule
      # off from that point to the end of the file, and the check still exits
      # 0. Reproduced in the template on 2026-09-18: a file carrying a plain
      # non-ASCII character after an unterminated fence passed.
      if (ISMD && fence) printf "FENCE\t%d\t-\n", fenceline
    }
  ' "$f" > "$TMP/out" 2>/dev/null || true

  while IFS="$(printf '\t')" read -r kind a b; do
    case "${kind:-}" in
      EMOJI) NEMOJI=$((NEMOJI + 1))
             report "$f:$a U+$b is an emoji or a marker character. Write NOTE:, CAUTION: or WARNING: instead, or name the code point. docs/conventions/prose.md" ;;
      CHAR)  report "$f:$a U+$b is not ASCII. docs/conventions/prose.md" ;;
      DASH)  report "$f:$a an em dash respelt in hyphens. Split the sentence; do not respell the dash. docs/conventions/prose.md" ;;
      FENCE) report "$f: the fenced block opened at line $a is never closed, so every rule stopped applying from there to the end of the file. Close it." ;;
    esac
  done < "$TMP/out"
done

if [ "$JSON" = "1" ]; then
  printf '{"schema":"check-markers/2","problems":%s,"files":%s,"emoji":%s}\n' \
    "$PROBLEMS" "$NFILES" "$NEMOJI"
  [ "$PROBLEMS" -gt 0 ] && exit 1
  exit 0
fi

if [ "$PROBLEMS" -gt 0 ]; then
  printf 'marker check failed, %s problem(s):\n\n%s\n' "$PROBLEMS" "$REPORT"
  printf 'Every text file is ASCII, with no emoji and no marker character.\n'
  printf 'docs/conventions/prose.md is the rule.\n'
  exit 1
fi

printf 'markers ok: %s text files, all ASCII outside markdown specimens, no emoji, no marker character\n' \
  "$NFILES"
exit 0
