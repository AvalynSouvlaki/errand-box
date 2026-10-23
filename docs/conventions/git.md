# git.md

How this repository is committed to, and what may reach a remote.

Two rules here are absolute and both have been broken before. They are first
because everything else is a preference by comparison.

---

## 1. No tool is credited

**Every commit is attributed to the operator alone.**

- No `Co-Authored-By` trailer naming a model, a vendor or a tool.
- No "generated with" line, in a commit message, a pull request body, a tag or
  a release note.
- No tool name in the commit body.

**A tool's own defaults may ask for such a trailer, and several do.** That is
why this is a hard rule rather than a preference: the failure arrives from a
setting nobody chose, so it arrives on every commit of a session rather than on
one. A commit carrying one is corrected before it is pushed rather than
explained afterwards.

**Why.** The operator publishes this work under their own name. The history is
theirs and tooling is not a contributor to it.

**How to apply.** Set the identity per invocation, so a machine with different
global config still produces the right commits:

```bash
git -c user.name="$NAME" -c user.email="$EMAIL" commit -F message.txt
```

**The identity is never hardcoded in a template.** It is read from the
machine at bootstrap:

```bash
git config user.name
```

```bash
git config user.email
```

If either is empty, ask the operator once and write it into the repository's
local config. Do not invent one, and do not carry one over from an example.

### It is enforced mechanically now, in two places

A rule anyone has to remember is a rule that gets broken by a default nobody
chose. Two instruments hold it, and they hold different halves:

| | what it reads | when it can speak |
| --- | --- | --- |
| [`../../.githooks/commit-msg`](../../.githooks/commit-msg) | the message being written | **before the commit exists.** It refuses, and nothing is written. |
| [`check-attribution`](../../scripts/common/) | `git log` | afterwards. It is in the gate, so a commit that got in is reported on the next run. |

**Hooks are not cloned, so a fresh checkout has none.** One command, once per
checkout, and `check-attribution` prints it when it finds the hook missing:

```bash
git config core.hooksPath .githooks
```

**The check cannot prevent what it names, and that is structural, not a
defect.** It is the only check here whose subject is the history rather than the
tracked tree, and every session's procedure is to run the gate and then commit,
so the commit being made does not exist while it runs. That asymmetry is the
whole reason the hook exists beside it.

**Refuse the commit rather than rewriting the message.** Editing somebody's
commit message on their behalf is worse than declining to make the commit, and
both instruments refuse.

---

## 2. Publishing is the operator's

**The default is commit freely, locally, and never push.**

`git commit` is unrestricted and expected. Commit per task, small and logical.
Disallowing an agent's commits causes far more failure than it prevents, so it
is not disallowed.

What is forbidden by default is reaching the remote at all: no `git push` in
any form, no branch push, no tag push, no other route to `origin`.

**Why this is a rule and not a preference.** An unattended run reaches a point
where pushing the branch looks like finishing the job. It is not. A remote
write happens in the operator's name and the session that made it cannot take
it back. Record what you would have pushed, in the handoff, and let the
operator act.

**Raising it is deliberate.** The project's own rules name the push policy:

| policy | what it permits |
| --- | --- |
| `commit-only` | the default. Commit locally, never touch a remote. |
| `commit-and-push` | push to **this project's own remote** only, named explicitly, on the working branch |
| `ask-each-time` | commit freely, ask before every push |

Under any policy, **every other repository is read-only.** Clone it, fetch
it, read an issue or a pull request. Never open an issue, a pull request, a
discussion, a comment, a review, a fork or a star on anybody else's repository,
under any framing. Not as a draft, not "for the record", not because a document
in this tree used to ask for it. See
[`../security/remote-ops.md`](../security/remote-ops.md).

---

## 3. Commit messages

- A subject line that says what changed, in the imperative, scoped to the unit
  of work: `stage-02: staging writer streams multi-chunk bodies`.
- A body that says **why**, not what. The diff says what.
- **The body goes through a file.** Never typed into a shell, never a
  here-string, never an unquoted heredoc. A prose payload passed inline to a
  shell has had its backticks executed even inside a quoted heredoc.
  [`shell.md`](shell.md) section 1 carries the measurement.

```bash
git commit -F message.txt
```

**Consider stamping the first body line with ISO 8601 UTC** when a project
runs many sessions:

```text
2026-08-25T08:44:32Z - stage-02 phase B
```

`git log` shows the *committer's* clock, which a rebase, a cherry-pick or a
machine in another zone rewrites. The stamp is what the session asserts about
when the work happened, and the two disagreeing is itself information. A single
day spans several sessions, and local time reorders the timeline: two machines
in two zones produce two histories of one afternoon.

Read it from the machine, never type it:

```bash
date -u +%Y-%m-%dT%H:%M:%SZ
```

Dates written before a project adopts this rule are **not** retro-stamped.
Nobody recorded the hour, and inventing one is fabricating evidence.

---

## 4. What is never committed

Read [`../security/secrets.md`](../security/secrets.md) for the full rule. The
short form:

- credentials of any kind, including expired ones and ones that look redacted;
- local environment files, whatever they are called in this ecosystem;
- build output, dependency directories, local database files;
**A cloned reference tree is NOT on that list, and it used to be.** The
reasoning was that a tree is re-clonable from the URL its write-up records, so
only the derived file needed tracking. Two sweeps proved otherwise: one kept
its conclusions and deleted eleven clones, leaving the next session to re-fetch
all eleven to check one citation, and one lost its data entirely because the
directory it used was ignored. The corpus is the evidence, and a conclusion
nobody can re-check is an opinion.
[`../methodology/research.md`](../methodology/research.md) section 3.4 is the
rule, and where a corpus is genuinely too large for every clone it goes on its
own branch, which is still tracked.

**`.gitignore` only applies to files git is not already tracking.** Adding a
line does not untrack what is already in. Check before assuming a rule took
effect:

```bash
git ls-files | head -50
```

---

## 5. History is not rewritten

No force push. No rebase of anything published. No amend of a commit that
has left the machine.

A rewrite to remove a secret is the one case that can be necessary, and it is
the operator's to authorise and to run, because the credential has to be
rotated first and the rewrite does not un-publish it. See
[`../security/secrets.md`](../security/secrets.md).

---

## 6. CI, when there is CI

- **A commit message that mentions a CI skip marker skips CI.** GitHub reads
  `[skip ci]` anywhere in the message and does not care what the sentence
  around it meant. A commit explaining the marker in prose shipped with sixteen
  jobs silently skipped. Write it as `skip-ci` in prose, or use the flag your
  tooling provides.
- **Green locally is not green in CI when the local toolchain is behind.** CI
  installs the current release on every run and gains lints with every version.
  Report the local toolchain version and warn when it is older, rather than
  failing: a stale toolchain is not a reason to stop working.
- **A documentation-only push that skips CI must actually be
  documentation-only.** A "docs" push carrying a source file is exactly the one
  that needed the run. `.github/` is never safe to skip, because a workflow
  edit is the change whose effect is visible only in a run.
