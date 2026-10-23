#!/bin/sh
# check-attribution.sh - does any commit in this repository credit a tool?
#
# THE DEFECT: a commit message that names the tool that wrote it.
# docs/conventions/git.md states the rule, states that it overrides whatever a
# session's own default asks for, and says in as many words that a tool
# enforcing it mechanically beats a rule anyone has to remember. Nothing
# enforced it.
#
# IT IS A RULE THAT GETS BROKEN BY DEFAULT, not by carelessness. Several
# agent tools instruct the model to append a co-author trailer, so the failure
# arrives from a setting rather than from a decision, and it arrives on every
# commit of a session rather than on one. Upstream, one session added a trailer
# to all eighteen of its commits before anything disagreed, and a second added
# one more months later. Both were found after the push.
#
# THIS CHECK CANNOT PREVENT THE THING IT NAMES, AND THE HOOK IS WHY.
# It reads `git log`, which makes it the only check here whose subject is the
# history rather than the tracked tree. Every session's procedure is "run the
# gate, then commit", so at the moment this runs the commit being made does not
# exist yet. It reports afterwards. `--message FILE` applies the same rules
# to a message that is not a commit yet, which is what .githooks/commit-msg
# calls, and that is the half that refuses rather than records.
#
# THE EMOJI RULE UPSTREAM CARRIES IS NOT PORTED HERE. Azathothas/ToolKit
# refuses any emoji in a commit message. The template left that rule out,
# because one of its own commits quoted a marker character. check-markers.sh
# refuses an emoji in the tree, but it does not read commit messages, so a
# commit message is the one place where no machine holds the no-emoji rule.
#
# Usage:
#   sh scripts/common/check-attribution.sh
#   sh scripts/common/check-attribution.sh --json
#   sh scripts/common/check-attribution.sh --message .git/COMMIT_EDITMSG
#
# Exit codes: 0 clean, 1 something credits a tool, 2 could not run.
#
# Read the exit code from this process, unpiped.

set -u

JSON=0
MSGFILE=""

while [ $# -gt 0 ]; do
  case "$1" in
    --json) JSON=1 ;;
    --message) shift; MSGFILE="${1:-}" ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-attribution: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v git >/dev/null 2>&1 || { printf 'check-attribution: git not found\n' >&2; exit 2; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { printf 'check-attribution: not a git repository\n' >&2; exit 2; }
command -v awk >/dev/null 2>&1 || { printf 'check-attribution: awk not found\n' >&2; exit 2; }
REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT" || { printf 'check-attribution: cannot enter %s\n' "$REPO_ROOT" >&2; exit 2; }

WORK="${TMPDIR:-/tmp}/.checkattr.$$"
mkdir -p "$WORK" || { printf 'check-attribution: cannot write to %s\n' "$WORK" >&2; exit 2; }
trap 'rm -rf "$WORK"' EXIT INT TERM

# THE RECORDS ARE SEPARATED BY CONTROL BYTES, NOT BY NEWLINES. A commit body
# holds newlines and blank lines by design, so any line-oriented split tears one
# in half and the halves are then read as two messages. Upstream separates with
# NUL; a NUL cannot survive a shell variable, so this uses 0x1e between records
# and 0x1f between fields. A record that does not split into three fields is
# reported rather than skipped: a message that happened to contain one of those
# bytes would otherwise silently leave the scope.
RS_BYTE=$(printf '\036')
US_BYTE=$(printf '\037')

if [ -n "$MSGFILE" ]; then
  [ -f "$MSGFILE" ] || { printf 'check-attribution: no such message file: %s\n' "$MSGFILE" >&2; exit 2; }
  # COMMENT LINES ARE STRIPPED, AND ONLY COMMENT LINES. git's own template
  # puts the branch name and the staged file list behind `#`, none of which
  # reaches the stored message. Refusing a message over git's own scaffolding
  # is how a hook becomes something people turn off.
  {
    printf '%s' "$RS_BYTE"
    printf 'pending%s' "$US_BYTE"
    awk 'BEGIN { s = 0 } /^#/ { next } { if (!s && $0 != "") { printf "%s", $0; s = 1 } }' "$MSGFILE"
    printf '%s' "$US_BYTE"
    awk '/^#/ { next } { print }' "$MSGFILE"
  } > "$WORK/records" || { printf 'check-attribution: cannot read the message\n' >&2; exit 2; }
else
  git log --format="%x1e%H%x1f%s%x1f%B" > "$WORK/records" 2>/dev/null || {
    printf 'check-attribution: git log gave no history\n' >&2
    exit 2
  }
fi

# AN ABSENCE IS NOT A ZERO. A run that read no message at all would report a
# clean history because it had nothing to disagree with, which is the shape
# every dead check in this tree has had.
if [ ! -s "$WORK/records" ]; then
  printf 'check-attribution: read no commit at all, so it agreed with nothing\n' >&2
  exit 2
fi

# The tokens that make a line a CREDIT rather than prose: vendor and product
# names, matched case-insensitively on a word boundary.
#
# A LIST IS A THING TO FORGET, and this one is short on purpose. The trailer
# and generated-with shapes catch the STRUCTURE of a credit as well as its
# wording, so a name nobody listed still trips one of them.
NAMES='claude anthropic opus sonnet haiku copilot chatgpt openai gpt-4 gpt-5 gemini codex cursor devin aider'

# The lines a tool appends to a message.
SHAPES='generated with|created with|written by ai|authored by ai'

LC_ALL=C awk -v RS="$RS_BYTE" -v US="$US_BYTE" -v names="$NAMES" -v shapes="$SHAPES" '
  function has_word(hay, needle,   re) {
    re = "(^|[^a-z0-9_])" needle "([^a-z0-9_]|$)"
    return (hay ~ re)
  }
  function trim48(s) {
    sub(/^[ \t]+/, "", s); sub(/[ \t]+$/, "", s)
    return (length(s) > 48) ? substr(s, 1, 45) "..." : s
  }
  BEGIN {
    nn = split(names, N, " ")
    ns = split(shapes, S, "|")
    checked = 0; bad = 0
  }
  {
    if ($0 == "") next
    n = split($0, F, US)
    if (n < 3) {
      printf "  a commit record did not split into three fields, so it was not read\n"
      bad++
      next
    }
    hash = F[1]; subject = F[2]
    body = F[3]
    for (i = 4; i <= n; i++) body = body US F[i]
    if (hash == "") next
    checked++
    short = (length(hash) > 8) ? substr(hash, 1, 8) : hash
    lower = tolower(body)

    # 1. The lines a tool appends.
    for (i = 1; i <= ns; i++)
      if (index(lower, S[i]) > 0) {
        printf "  %s (%s) carries \"%s\"\n", short, trim48(subject), S[i]
        bad++
      }

    # 2. A co-author trailer, and ONLY when it names a tool. A human co-author
    #    is a legitimate thing to record.
    at = index(lower, "co-authored-by:")
    if (at > 0) {
      rest = substr(lower, at + 15)
      nl = index(rest, "\n")
      if (nl > 0) rest = substr(rest, 1, nl - 1)
      for (i = 1; i <= nn; i++)
        if (has_word(rest, N[i])) {
          printf "  %s (%s) has a co-author trailer naming \"%s\"\n", short, trim48(subject), N[i]
          bad++
          break
        }
    }

    # 3. A tool name anywhere in the body, trailer or not.
    for (i = 1; i <= nn; i++)
      if (has_word(lower, N[i])) {
        printf "  %s (%s) names \"%s\" in its message\n", short, trim48(subject), N[i]
        bad++
      }
  }
  END { printf "STAT\t%d\t%d\n", checked, bad }
' "$WORK/records" > "$WORK/out" 2>/dev/null

CHECKED=$(awk -F"\t" '$1 == "STAT" { print $2 }' "$WORK/out")
COUNT=$(awk -F"\t" '$1 == "STAT" { print $3 }' "$WORK/out")
CHECKED=${CHECKED:-0}
COUNT=${COUNT:-0}
REPORT=$(grep -v '^STAT	' "$WORK/out" || true)

if [ "$CHECKED" = "0" ]; then
  printf 'check-attribution: read no commit at all, so it agreed with nothing\n' >&2
  exit 2
fi

# THE HOOK STATE IS A NOTE, NOT A VERDICT. Hooks are not cloned, so a fresh
# checkout has none, and failing the gate over a local git setting would make
# every first run of this repository red. Each checkout installs it once;
# docs/conventions/git.md carries the one command.
HOOKS=$(git config core.hooksPath 2>/dev/null || true)
HOOK_OK=0
[ -n "$HOOKS" ] && [ -f "$HOOKS/commit-msg" ] && HOOK_OK=1

# AND WHETHER GIT WILL ACTUALLY RUN IT. git invokes a hook directly rather
# than through an interpreter, so a commit-msg without the executable bit is
# SKIPPED IN SILENCE on any POSIX host. It is the one file in this tree where
# the bit matters: every `.sh` here is invoked as `sh script.sh`, so those are
# mode 644 on purpose and this one is not. The bit that ships is the one in the
# index, not the one in the working tree, because a Windows checkout does not
# carry it. It committed as 644 once, which is what put this test here.
HOOK_EXEC=0
HOOK_NOEXEC=""
for _h in .githooks/commit-msg; do
  case "$(git ls-files -s -- "$_h" 2>/dev/null)" in
    100755\ *) HOOK_EXEC=1 ;;
    100644\ *) HOOK_EXEC=0; HOOK_NOEXEC="$_h" ;;
  esac
done

# A SHALLOW CLONE MAKES THIS CHECK LOOK CLEAN OVER A HISTORY IT NEVER READ.
# The default checkout in a CI job fetches ONE commit, so a run there scans one
# message and reports success about the whole history. That is a check that
# reports success having shown nothing, which is the shape this repository
# exists to refuse. It is REPORTED rather than failed: a shallow clone is a
# legitimate thing for a CI job to have, and failing would make every default
# workflow red. The caller decides whether one commit was the scope they meant.
SHALLOW=0
[ -z "$MSGFILE" ] && [ "$(git rev-parse --is-shallow-repository 2>/dev/null)" = "true" ] && SHALLOW=1

if [ "$JSON" = "1" ]; then
  printf '{"schema":"check-attribution/1","problems":%s,"commits":%s,"hook_installed":%s,"hook_executable":%s,"shallow":%s}\n' \
    "$COUNT" "$CHECKED" "$HOOK_OK" \
    "$(if [ "$HOOK_EXEC" = 1 ]; then printf 'true'; else printf 'false'; fi)" \
    "$(if [ "$SHALLOW" = 1 ]; then printf 'true'; else printf 'false'; fi)"
  [ "$COUNT" -gt 0 ] && exit 1
  exit 0
fi

if [ "$COUNT" -gt 0 ]; then
  printf 'a tool is credited in %s place(s):\n\n%s\n\n' "$COUNT" "$REPORT"
  printf 'docs/conventions/git.md: no tool is credited in a commit. The identity\n'
  printf 'is the operator alone, and this overrides whatever default asked for\n'
  printf 'the trailer. Edit the message and commit again.\n'
  exit 1
fi

if [ -n "$MSGFILE" ]; then
  printf 'no tool credited in the pending message\n'
  exit 0
fi

printf 'no tool credited in %s commit(s)\n' "$CHECKED"
if [ -n "$HOOK_NOEXEC" ]; then
  printf '\nWARNING: %s is tracked WITHOUT the executable bit, so git skips it\n' "$HOOK_NOEXEC"
  printf '  in silence on any POSIX host. Fix the mode in the index, not on disk:\n\n'
  printf '    git update-index --chmod=+x %s\n' "$HOOK_NOEXEC"
fi
if [ "$SHALLOW" = "1" ]; then
  printf '\nNOTE: this clone is SHALLOW, so that is every commit that was fetched\n'
  printf '  rather than every commit there is. A default CI checkout fetches\n'
  printf '  one. Fetch the full history where the whole record is the scope.\n'
fi
if [ "$HOOK_OK" = "0" ]; then
  # NAME THE INSTALL COMMAND ONLY WHEN THE HOOK IS THERE TO INSTALL. A command
  # that points git at a directory with no hook in it is an instruction that
  # does nothing.
  WHERE=""
  [ -f ".githooks/commit-msg" ] && WHERE=".githooks"
  printf '\nNOTE: the commit-msg hook is not installed here, so this can only report\n'
  if [ -n "$WHERE" ]; then
    printf '  after the fact. Install it once, per checkout:\n\n'
    printf '    git config core.hooksPath %s\n' "$WHERE"
  else
    printf '  after the fact, and no commit-msg hook is in this tree to install.\n'
    printf '  docs/conventions/git.md says where it belongs.\n'
  fi
fi
exit 0
