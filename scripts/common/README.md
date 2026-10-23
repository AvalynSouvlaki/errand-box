# scripts/common

The checks this project runs as part (a) of its gate. The environment probe
lives beside them in [`../doctor/`](../doctor/).

**One command runs all of it**, and it is the one to reach for rather than
typing a list from memory:

```bash
sh scripts/common/check-gate.sh
```

**What is here is what a project must be able to run with NO NETWORK.** That
is the whole selection rule. A gate that has to fetch a check is red whenever
somebody else's host is, and a check fetched at gate time is code nobody
reviewed judging the tree it is judging.

Every script here is POSIX sh. Each one runs as `sh scripts/common/NAME.sh`,
so each one is mode 644 on purpose. On Windows, run them under Git Bash or WSL.

---

## What this project left out of the template

The template ships more in this directory. This project took the checks below
and left four things out:

| left out | why |
| --- | --- |
| every `.ps1` twin | this project has no PowerShell. The checks run under POSIX sh: natively on Linux, and under Git Bash or WSL on Windows. |
| `check-twins.sh` | it runs each check and its PowerShell twin and compares the answers. With no twins it has nothing to compare. It was also the slowest part of the template's gate: 80% of the run time, by the template's own measurement. |
| `check-remote-items.sh` | it needs the network and an authenticated `gh`, and it makes several API calls for each pinned action in each open pull request, so its run time grows with the open items. Without `gh` it exits 2 at once, which the gate counts as a SKIP and `--strict` turns red. |
| `mine-repo.sh` | it fetches a whole repository and its tracker over the network, which is slow by design. It is a helper, not a check. [`../../docs/methodology/research.md`](../../docs/methodology/research.md) section 3.1 says how to fetch without it. |

---

## What the gate costs

**A gate long enough to skip is a gate that gets skipped.** That is the whole
reason this section exists. The template added a `--fast` flag because one
check was slow enough that somebody wanted a way past it. That check is not
here, so the flag is not here either.

Measured on the tree as adopted, before any project file existed. One Linux
x86_64 container with 4 CPUs, dash as `/bin/sh`, mawk 1.3.4 as `awk`, git
2.43.0, on 2026-10-23. Three runs each, wall clock, and the median is shown.
47 files were in scope and none was committed yet. Another process was taking
measurements on the same host at the time:

| check | median wall time | exit |
| --- | --- | --- |
| `check-docs.sh` | 0.18 s | 1, because two project files did not exist yet |
| `check-markers.sh` | 0.20 s | 0 |
| `check-one-home.sh` | 0.08 s | 0 |
| `check-placeholders.sh` | 0.04 s | 0 |
| `check-control-bytes.sh` | 0.06 s | 0 |
| `check-changelog.sh` | 0.01 s | 2, no `CHANGELOG.md` yet |
| `check-attribution.sh` | 0.04 s | 0 |
| `check-no-secrets.sh` | 0.12 s | 0 |
| `check-no-secrets.sh --public` | 0.15 s | 0 |
| `check-gate.sh`, all of the above | 0.80 s | 1 |

**One run on one machine is not a benchmark.** It says what to expect. A tree
ten times this size does not take ten times as long by arithmetic; measure it
there.

**Under Git Bash a process spawn costs more than reading the file does.** In
the template, `check-docs.sh` went from 25.8s to 4.9s and
`check-control-bytes.sh` from 13.4s to 0.6s on one Windows 11 machine, when a
loop that spawned several processes per file became one `awk` pass. Keep a new
check to one pass over the files for the same reason.

### When the gate gets long: one program instead of many scripts

**A project is free to move its checks into one program in its own
language.** `Azathothas/ToolKit` rewrote every rule as one Go program, and its
`check-*.sh` became thin wrappers over one subcommand each. Its own record puts
its gate at about twelve minutes before the port and under a minute after, on
its development host.

The trade is explicit: one implementation, against a build step and a
toolchain in the requirements. A gate that needs a compiler fails on a machine
with no compiler, which is the first machine a fresh clone lands on. **Do not
vendor a copy of somebody else's checker to get there.** A tool kept in two
repositories acquires two sets of defects, and one of the two never gets
fixed. [`../../docs/methodology/vendoring.md`](../../docs/methodology/vendoring.md)
is what to do instead.

**Ask the same question about every EXCLUSION.** An exclusion is written for a
reason that covers part of a script, and then it is read as covering the
script. In the template, an exclusion was correct about one half of a helper
and silently protected the other half, which discarded an entire comment
corpus while printing "ok". A consumer found it, not the template.

---

## The check contract

**Every check here satisfies all five.** A script that does not is not a
check; it is a script somebody has to remember to interpret.

1. **A header comment saying what defect it exists to catch.** Not what it
   does: what goes wrong without it. This is the field that decides whether a
   future session keeps it, deletes it, or writes a second one that overlaps.
2. **Exit 0 pass, 1 fail, 2 could not run.** Those are three different facts.
   "The check failed" and "the check could not run" mean opposite things about
   whether you can ship, and a script that returns 1 for both hides the
   difference.
3. **A json switch**, so a gate runner can consume it.
4. **No dependence on the directory it is run from.** Resolve paths from the
   script's own location.
5. **Read only, unless a fix flag is passed.** A check that repairs things by
   default is a check nobody can use to find out whether something is wrong.

**A check that measures an open defect must not fail the build for that
defect alone.** Record the count and judge it only past a stated ceiling. The
other half of that rule is that the exemption comes off when the item closes.
An exemption nobody removes is a check that stopped checking.

---

## An exit code is read from the process that produced it, unpiped

```bash
sh scripts/common/check-no-secrets.sh
```

Not `check | grep`, not `check | tee`. A pipeline reports the **last**
command's status, so a check that failed reads as green.

This has caught the author of this sentence, in the session that wrote it.

---

## What is here

### `check-gate.sh`

**Run the whole local gate with one command.** Part (a) of
[`../../docs/methodology/gate.md`](../../docs/methodology/gate.md) is a list,
and a list run by hand is run in the order somebody recalls it. The session
that wrote this ran its gate five times and typed a different subset each time.

It delegates and holds no rules of its own. **A skipped check is reported as
a skip, never as a pass**, because a runner that dropped one quietly and
printed green would be the forbidden-patterns row about a step that exits 0
having done nothing. `--strict` makes a skip a failure, which is what CI
should pass.

**Zero passes is red whatever the skips say.** It produced exactly the
opposite on its own first run: a broken presence test made every row report
"not present", and it printed a green verdict over nothing at all. That is the
defect its header describes, produced by the script itself, and it is the
argument for the rule.

### `check-docs.sh`

Do the documents still resolve, and are they written the way this repository
writes documents. Relative links, fenced shell blocks that parse,
shell-unsafe placeholders, and orphan pages.

**It does NOT read for writing style, and its header says why.** A banned
vocabulary list was once claimed and never implemented. Arming that list over
the template found one word; the register that actually goes wrong is metaphor
used as a technical term, which no list reaches.
[`../../docs/conventions/prose.md`](../../docs/conventions/prose.md) names the
standard and the review pass reads for it.

**The character rules are NOT here.** ASCII only, no emoji, no marker
character, and no dash respelt in hyphens: those live in `check-markers.sh`,
which reads every tracked text file rather than markdown alone. Two checks
enforcing one rule is two places for it to be wrong, which is the same move
the control-byte rule already made out of this file.

### `check-markers.sh`

ASCII text, no emoji, no marker character, no dash respelt in hyphens, and no
unclosed fence. Four rules, one subject, one home.

**The emoji rule has no exemption.** A specimen inside a code span or a fenced
block may carry another non-ASCII character, so a page can show which one it
means. It may not carry an emoji or a marker character. A page names one of
those by its code point, for example U+26D4. The script header lists the code
point ranges it refuses.

**The dash rule exists because banning a character taught agents to respell
it.** Reported by the template's operator on 2026-09-17: an em dash refused,
and `--` or a comma written instead, with every run green and not one sentence
shorter. The comma half is not checkable and the page says so, because a green
run is not a well-written page.

**The fence rule closes a BYPASS of the other three, and it was found by
attacking the check rather than by reading it.** The specimen exemption skips
every line inside a fenced block, so a markdown file with an odd number of
fence markers switched the rules off from that point to the end of the file.
Planted in the template on 2026-09-18: a file carrying a plain non-ASCII
character after an unterminated fence exited **0**.

**It reads every tracked text file.** The rule it inherited scanned markdown
alone, and on the day it was widened the template's own scripts held **2290**
characters outside its allowed set across 22 files. Every one was in a
script, so the markdown-only version had never seen any of them.

**A leading byte-order mark is exempt**, because Windows PowerShell 5.1 wants
one at the start of a `.ps1` with non-ASCII bytes. A mark anywhere else is
still reported.

### `check-one-home.sh`

Does any sentence of 12 words or more appear in two documents.

**The rule was in `prose.md` from the start and nothing checked it**, so it
drifted the way an unchecked rule always drifts. Its first run over the
template found **42** duplicated sentences of 8 words or more.

**THE FIRST VERSION OF THE INSTRUMENT REPORTED ZERO AND WAS WRONG.** Its file
collector handed git a quoted pathspec through a shell that treats a quote as
an ordinary character, so it matched nothing and reported a clean tree it had
never opened. It now refuses to report success over an empty scope, and that
is the only reason the number above exists.

**The entry-point routers are exempt from each other and from nothing else.**
This project has one router, `AGENTS.md`, so the exemption does nothing until
a second router joins its list. A sentence shared between a router and any
other file is still refused.

It compares SENTENCES, so a fact restated in different words passes here and
fails a review. That is the same split every prose rule has.

### `check-placeholders.sh`

Did a template placeholder survive into a real file. **Only the script itself
is exempt**, because it contains the patterns it looks for. No directory is
exempt, so a directory added later is scanned like any other.

### `check-control-bytes.sh`

Is there a literal control byte in any text file in the tree.

**It covers every text file, not only markdown.** The rule used to live in
`check-docs.sh` and scanned `.md` alone, which left every `.rs`, `.sh` and
`.yml` unchecked for the one defect that makes a file invisible to both review
tools at once: `grep` calls it binary and skips it, and `git diff` prints
"Binary files differ" so a code review shows no diff at all.

The runtime value is identical either way, so only reviewability is ever at
stake. That is exactly why it survives unnoticed.

### `check-attribution.sh`

Does any commit in this repository credit a tool.

**[`../../docs/conventions/git.md`](../../docs/conventions/git.md) stated the
rule from the start, said in as many words that a tool enforcing it beats a
rule anyone has to remember, and nothing enforced it.** It is the rule most
likely to be broken by a setting rather than by a decision: several agent
tools instruct the model to append a co-author trailer, so the failure arrives
on every commit of a session rather than on one.

**It cannot prevent what it names, and the hook is why.** This is the only
check here whose subject is `git log` rather than the tracked tree, and every
session's procedure is to run the gate and then commit, so the commit being
made does not exist while it runs. `--message FILE` applies the same rules to
a message that is not a commit yet, and
[`../../.githooks/commit-msg`](../../.githooks/commit-msg) is what calls it.
That half refuses; this half records.

**The hook state is a NOTE, not a verdict.** Hooks are not cloned, so failing
the gate over a local git setting would make every fresh clone of this
repository red. The check prints the one command instead.

**It also reports whether git will actually RUN the hook.** git invokes a hook
directly rather than through an interpreter, so a `commit-msg` without the
executable bit is skipped in silence on any POSIX host. It is the one file
here where the bit matters. The bit that ships is the one in the INDEX, not
the one on disk, since a Windows checkout does not carry it.

**A shallow clone is reported too, for the same reason.** A default CI checkout
fetches one commit, and this check reads `git log`, so without the full
history it scans one message and reports success about all of them. Both
workflow files in this repository ask for the full history and say why.

**It does not refuse an emoji in a commit message.** `Azathothas/ToolKit`
does. `check-markers.sh` holds the no-emoji rule for the tree, but a commit
message is outside its scope.

### `check-changelog.sh`

Does `CHANGELOG.md` still obey the four rules a machine can hold: newest
first, every heading dated, every entry naming its record, every entry saying
whether it deployed.

It exists because
[`../../docs/conventions/docs.md`](../../docs/conventions/docs.md) stated those
four rules, said in as many words that each was mechanical enough to check,
and nothing checked them.

**No `CHANGELOG.md` is exit 2, not exit 0.** A project without one has
neither broken these rules nor satisfied them, and reporting green over an
absent file is how a check quietly stops applying.

### `check-no-secrets.sh`

Does any file in this tree carry something that must not be published.

**Tracked plus untracked-but-not-ignored, not tracked alone.** A file that
has never been staged is exactly when a new file is likeliest to carry a
credential, and exactly what the next `git add -A` would take.

**It finds the shapes it knows, and a green run is not a clearance.** It
cannot find a password that looks like a word or a page of correct-looking
examples that happens to describe a real system.

`--public` adds the rules that only matter for a repository that will be
public: emails, absolute home paths, long hex identifiers. In a private
project those are legitimate content, which is why they are not the default.
This repository is public, so the gate runs both.

---

## Adding one

1. **Name the defect first.** If you cannot say what goes wrong without this
   script, it is not a check.
2. **Follow the contract**, all five points.
3. **Mutation-prove it.** Plant the defect it exists to catch, run it, and
   read the exit code unpiped. **A guard that has never been seen to refuse is
   a guard nobody knows works.**

   This is not optional advice. A licence filler in the template reported
   success over a licence whose warranty clause it had corrupted, because its
   check only ever asked whether a placeholder *survived*, never whether the
   substitution had reached too far. The mutation test is what found it.

   **And prove the NEGATIVE case too.** A guard that refuses everything is as
   useless as one that refuses nothing, and it looks identical from a passing
   mutation test. A guard that refuses an empty answer over input that has
   records must still accept an empty answer over input that has none.

4. **Wire it into the gate**, if it can fail: add it to the list in
   `check-gate.sh`.
5. **Document it**: here, and in
   [`../../docs/agent-tooling.md`](../../docs/agent-tooling.md).

**A script that lives only in a transcript is re-derived every session.**
When a scratch helper does something a future session will also need, promote
it: write it into `scripts/` with the contract above, document it where agents
are told to look, and wire it into the gate if it is a check rather than a
one-off.
