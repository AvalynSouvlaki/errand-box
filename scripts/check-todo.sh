#!/bin/sh
# check-todo.sh - does the TODO record agree with itself?
#
# The defect this exists to catch: a count that disagrees with the rows, or
# an entry whose status in TODO/INDEX.md is not its status in its own file.
# docs/methodology/work-todo.md section "The counts have to agree with the
# rows" says what that cost.
#
# Usage:
#   sh scripts/check-todo.sh                   check, change nothing
#   sh scripts/check-todo.sh --fix             rewrite every count from the rows
#   sh scripts/check-todo.sh --set T-NNN STATUS
#                                              move one entry's status in the
#                                              index and in the entry, then --fix
#
# It reads the rows of TODO/INDEX.md (`| [T-NNN](file.md) | P1 | cat | status |`),
# the `# T-NNN: title` headings and `**Status:**` lines of the entry files,
# the counts line and the priority table of the index, and the counts line of
# TODO/PROGRESS.md.
# Exit codes: 0 consistent (or fixed), 1 a disagreement, 2 could not run.
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
dir=$root/TODO
index=$dir/INDEX.md
progress=$dir/PROGRESS.md
if [ ! -r "$index" ] || [ ! -r "$progress" ]; then echo "check-todo: $index or $progress is missing" >&2; exit 2; fi
tmp=$(mktemp -d) || exit 2
trap 'rm -rf "$tmp"' EXIT INT TERM
problems=0
bad() { printf 'check-todo: %s\n' "$1"; problems=1; }

# id file priority category status, one row per line.
rows() {
  awk -F'|' '/^\| \[T-[0-9]+\]\(/ {
    id = $2; sub(/^ *\[/, "", id); sub(/\].*/, "", id)
    file = $2; sub(/^[^(]*\(/, "", file); sub(/[)#].*/, "", file)
    for (i = 3; i <= 6; i++) { gsub(/^ +| +$/, "", $i) }
    print id, file, $3, $4, $5
  }' "$index"
}

if [ "${1:-}" = --set ]; then
  id=${2:-}
  st=${3:-}
  case $st in open | partial | blocked | done) ;; *) echo "check-todo: status must be open, partial, blocked or done" >&2; exit 2 ;; esac
  file=$(rows | awk -v id="$id" '$1 == id { print $2 }')
  [ -n "$file" ] || { echo "check-todo: no row for $id" >&2; exit 2; }
  awk -v id="$id" -v st="$st" -F'|' 'BEGIN { OFS = "|" }
    $0 ~ "^\\| \\[" id "\\]\\(" { $5 = " " st " " } { print }' "$index" > "$tmp/i" && cat "$tmp/i" > "$index"
  awk -v id="$id" -v st="$st" '
    /^# T-[0-9]+: / { cur = ($2 == id ":") }
    cur && /^\*\*Status:\*\* / { print "**Status:** " st; next } { print }' "$dir/$file" > "$tmp/e" && cat "$tmp/e" > "$dir/$file"
  set -- --fix
fi

rows > "$tmp/rows"
[ -s "$tmp/rows" ] || { echo "check-todo: $index has no rows" >&2; exit 2; }

# Every row: a known priority and status, an entry in its file with the same status.
while read -r id file pri cat st; do
  case $pri in P0 | P1 | P2 | P3) ;; *) bad "$id: priority $pri is not P0 to P3" ;; esac
  case $st in open | partial | blocked | done) ;; *) bad "$id: status $st is not open, partial, blocked or done" ;; esac
  f=$dir/$file
  if [ ! -f "$f" ]; then bad "$id: its file $file is missing"; continue; fi
  est=$(awk -v id="$id" '/^# T-[0-9]+: / { cur = ($2 == id ":") } cur && /^\*\*Status:\*\* / { print $2; exit }' "$f")
  [ -n "$est" ] || { bad "$id: no '# $id:' entry with a **Status:** line in $file"; continue; }
  [ "$est" = "$st" ] || bad "$id: the index says $st, $file says $est"
  ecat=$(awk -v id="$id" '/^# T-[0-9]+: / { cur = ($2 == id ":") } cur && /^\*\*Category:\*\* / { print $2; exit }' "$f")
  [ "$ecat" = "$cat" ] || bad "$id: the index says category $cat, $file says ${ecat:-none}"
done < "$tmp/rows"

# Every entry has a row.
for f in "$dir"/*.md; do
  case ${f##*/} in INDEX.md | PROGRESS.md | RULES.md) continue ;; esac
  grep -o '^# T-[0-9]*:' "$f" | tr -d '#: ' | while read -r id; do
    awk -v id="$id" '$1 == id { found = 1 } END { exit !found }' "$tmp/rows" || echo "$id"
  done
done > "$tmp/orphans"
while read -r id; do bad "$id has an entry and no row in INDEX.md"; done < "$tmp/orphans"

# The counts, derived.
awk '
  { n++; s[$5]++; p[$3, $5]++; pt[$3]++ }
  END {
    printf "**%d items. %d open, %d partial, %d blocked, %d done.**\n", n, s["open"], s["partial"], s["blocked"], s["done"]
    split("P0 P1 P2 P3", ps, " ")
    for (i = 1; i <= 4; i++) {
      q = ps[i]
      printf "| %s | %d | %d | %d | %d | %d |\n", q, p[q, "open"], p[q, "partial"], p[q, "blocked"], p[q, "done"], pt[q]
    }
    printf "| **All** | %d | %d | %d | %d | %d |\n", s["open"], s["partial"], s["blocked"], s["done"], n
    printf "**%d entries: %d done, %d partial, %d blocked, %d open.**\n", n, s["done"], s["partial"], s["blocked"], s["open"]
  }' "$tmp/rows" > "$tmp/counts"

sed -n 1p "$tmp/counts" > "$tmp/line"
sed -n 2,6p "$tmp/counts" > "$tmp/table"
sed -n 7p "$tmp/counts" > "$tmp/pline"
awk -v line="$(cat "$tmp/line")" -v table="$(cat "$tmp/table")" '
  /^\*\*[0-9]+ items\. / { print line; next }
  /^\| P[0-3] \|/ { next }
  /^\| \*\*All\*\* \|/ { print table; next }
  { print }' "$index" > "$tmp/index"
awk -v line="$(cat "$tmp/pline")" '/^\*\*[0-9]+ entries: / { print line; next } { print }' "$progress" > "$tmp/progress"
grep -q '^\*\*[0-9]* items\. ' "$index" || bad "INDEX.md has no '**N items. ...**' line"
grep -q '^| \*\*All\*\* |' "$index" || bad "INDEX.md has no priority table"
grep -q '^\*\*[0-9]* entries: ' "$progress" || bad "PROGRESS.md has no '**N entries: ...**' line"

if [ "${1:-}" = --fix ]; then
  cat "$tmp/index" > "$index"
  cat "$tmp/progress" > "$progress"
  echo "check-todo: counts rewritten: $(cat "$tmp/line")"
else
  cmp -s "$tmp/index" "$index" || bad "the counts in INDEX.md disagree with its rows; run: sh scripts/check-todo.sh --fix"
  cmp -s "$tmp/progress" "$progress" || bad "the counts in PROGRESS.md disagree with the index; run: sh scripts/check-todo.sh --fix"
fi
[ "$problems" = 0 ] && echo "check-todo: $(wc -l < "$tmp/rows") entries, consistent"
exit "$problems"
