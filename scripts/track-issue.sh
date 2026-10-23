#!/bin/sh
# track-issue.sh - keep one GitHub issue per problem: open it when the
# problem starts, keep its body current while it lasts, close it when it
# clears. Used by the scheduled workflows.
#
#   sh scripts/track-issue.sh raise <key> <title> <body-file>
#   sh scripts/track-issue.sh clear <key> <note>
#
# raise opens the issue once and after that only rewrites its body, so a
# problem that lasts a month is one notification. clear comments and closes,
# and does nothing when nothing is open. The key is an HTML comment in the
# body; the open issues are listed, never searched, because the search index
# lags and a lagging search opens a second issue.
#
# Needs: gh and jq; GH_TOKEN with issues: write; GITHUB_REPOSITORY.
# Exit codes: 0 done, 1 a call failed, 2 could not run.
set -eu
label=${TRACK_ISSUE_LABEL:-automation}
die() { printf 'track-issue: %s\n' "$1" >&2; exit "${2:-1}"; }
for t in gh jq; do command -v "$t" >/dev/null 2>&1 || die "$t is missing" 2; done
slug=${GITHUB_REPOSITORY:-}
case $slug in */*) ;; *) die "GITHUB_REPOSITORY is not owner/name: ${slug:-unset}" 2 ;; esac
mode=${1:-}
key=${2:-}
case $key in '' | *[!a-z0-9._-]*) die "usage: track-issue.sh raise <key> <title> <body-file> | clear <key> <note>" 2 ;; esac
marker="<!-- track: $key -->"

find_open() {
  gh api --paginate "repos/$slug/issues?state=open&labels=$label&per_page=100" |
    jq -r --arg m "$marker" '.[] | select(.pull_request == null) | select((.body // "") | contains($m)) | .number' |
    awk 'NF && !n { print; n = 1 }'
}

case $mode in
  raise)
    title=${3:-}
    body_file=${4:-}
    if [ -z "$title" ] || [ ! -f "$body_file" ]; then die "raise needs a title and a body file" 2; fi
    body="$marker
$(cat "$body_file")"
    n=$(find_open)
    if [ -n "$n" ]; then
      gh api -X PATCH "repos/$slug/issues/$n" -f title="$title" -f body="$body" >/dev/null
      echo "track-issue: $key is still open as #$n; its body is current"
    else
      gh api "repos/$slug/labels/$label" >/dev/null 2>&1 ||
        gh api -X POST "repos/$slug/labels" -f name="$label" -f color=ededed \
          -f description="Opened and closed by this repository's workflows" >/dev/null
      n=$(gh api -X POST "repos/$slug/issues" -f title="$title" -f body="$body" -f "labels[]=$label" --jq .number)
      echo "track-issue: $key opened as #$n"
    fi
    ;;
  clear)
    note=${3:-}
    [ -n "$note" ] || die "clear needs a note that says what cleared it" 2
    n=$(find_open)
    [ -n "$n" ] || { echo "track-issue: $key has no open issue"; exit 0; }
    gh api -X POST "repos/$slug/issues/$n/comments" -f body="$note" >/dev/null
    gh api -X PATCH "repos/$slug/issues/$n" -f state=closed -f state_reason=completed >/dev/null
    echo "track-issue: $key closed #$n"
    ;;
  *) die "usage: track-issue.sh raise <key> <title> <body-file> | clear <key> <note>" 2 ;;
esac
