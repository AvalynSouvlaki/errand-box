# hosted-sessions.md

A session running on a machine somebody else provisioned, which will be gone
afterwards, with no memory of any session before it.

**Read this when the session is not on the operator's own machine**: a
provider's cloud environment, a container a platform hands out per run, a
runner. Several entries apply to a local session too, and each says so.

**This page is about the MACHINE.**
[`methodology/sessions.md`](methodology/sessions.md) owns what a session owes at
its start and its end, [`containers.md`](containers.md) owns a machine you make
on purpose and throw away, and neither is restated here.

---

## 1. Say where it is, which branch, and that the clone is probably shallow

**A session that has to guess where it is guesses wrong in a way nothing
reports.** These machines start with a clone the platform made, not one the
operator made, and two things about it are usually different from what a
session assumes:

- **The default branch may not be checked out**, and the branch that is may be
  one the platform named after itself.
- **The history is usually shallow.** `git log` answers, `git describe`
  answers, and both answer about a truncated history. A merge base against a
  branch whose commits were never fetched cannot be computed, and the error
  names the commit rather than the depth.

Read it rather than assuming it. The probe reports both:

```bash
sh scripts/doctor/doctor.sh
```

**Unshallowing reaches the network**, so it is a fetch like any other and
[`security/remote-ops.md`](security/remote-ops.md) governs it. Reading is
always allowed; a write to a remote is not.

---

## 2. Commit as the work happens

**These machines are taken away without warning, and an uncommitted tree
goes with them.** A session that batches its commits to the end loses
everything the moment the platform reclaims the container, and the next session
starts from the last thing that was written down.

[`conventions/git.md`](conventions/git.md) already says to commit per task,
small and logical, and that publishing is the operator's decision rather than
the session's. What changes here is only the cost of not doing it: on the
operator's own machine an uncommitted tree survives the session, and here it
does not.

---

## 3. Set the commit identity in the first minute

**A fresh machine has no git identity**, so the first commit either fails or
lands under whatever the platform configured, and a wrong author is not
something a later commit can correct. [`conventions/git.md`](conventions/git.md)
is the rule, including where the values come from and why they are never
written into a template.

**Install the hook in the same minute**, because it is the only guard here
that does nothing until a command is run:

```bash
git config core.hooksPath .githooks
```

---

## 4. Read the clock. Do not recall it.

**A session is systematically wrong about the date.** It is not in the
context, it is not in the tree, and what a model produces instead is a
plausible date from somewhere else. Every timestamp this methodology asks for
is therefore read from the machine:

```bash
date -u +%Y-%m-%dT%H:%M:%SZ
```

The probe prints it in its header, so the same run that tells a session where
it is tells it when it is. This one applies everywhere, not only to a hosted
session.

---

## 5. A claim about this machine is a measurement, never a recollection

**The commonest false statement a session makes about a hosted machine is
that it cannot do something it can.** The pattern is always the same: a tool is
installed and not started, or a device is absent and has a software substitute,
and the session reports a capability gap instead of a setup step.

| what gets claimed | what is usually true |
| --- | --- |
| there is no container engine | it is installed and its daemon was never started |
| there is no `/dev/kvm` | there is not, and the emulator runs without it, slower |
| there is no display | a headless or virtual display is one package away |
| there is no GPU | true, and the workload has a software path |

**Three routes before anything is recorded as not-doable.**
[`methodology/sessions.md`](methodology/sessions.md) is the rule and what it
cost; [`agent-tooling.md`](agent-tooling.md) is what to reach for first. A limit
written down as settled is inherited by every session after it, which is what
makes this expensive rather than merely wrong.

---

## 6. The two ways these machines die quietly

**Neither announces itself, and both look like the work failing.**

**Disk.** The writable space is often a fixed allowance rather than a share of
a real filesystem, so `df` reports `Avail` near zero beside a `Used` that looks
small, and a check written against `Used` never fires. **Read `Avail`.** Inodes
exhaust separately and for a different reason: an image pull or a dependency
tree writes many small files.

**A process that was never started properly.** A build waiting on a daemon that
is not running looks exactly like a build that is working.
[`conventions/shell.md`](conventions/shell.md) section 9 is the rule that every
command that can wait forever carries a time limit, and section 10 is how to
wait without ending the turn.

**A watchdog is a background report, not a supervisor**, and the split is the
point: reporting is always safe, and acting on what it finds is not. One that
earns its place prints free space and inodes against a floor, names the
directories that actually grow, and says which long-running process is in a
state that cannot be killed. Anything that reaps rather than reports is
opt-in, and it never touches something younger than a stated age, because the
live run's own work looks exactly like an orphan.

**A watchdog belongs to the project, not to this template.** What it watches
is what that project fills up, and a generic one would name directories no
tree has.

---

## 7. Fetch through the read-only proxies when the direct route refuses

**A hosted machine's address is shared, so an anonymous API limit is spent by
somebody else's session before this one starts.** The symptom is a 403 or a 429
on a request that works from the operator's machine.

| for | use |
| --- | --- |
| a read-only code-host API path | `https://api.gh.pkgforge.dev/OWNER/PATH` |
| any other URL that refuses | `https://api.rv.pkgforge.dev/THE_ORIGINAL_URL` |

**Read-only, and authenticated routes do not go through either.** A token
does not leave this machine, so anything needing one uses the direct route or
does not happen. [`security/remote-ops.md`](security/remote-ops.md) is binding
on both.

**The first proxy enforces a user-agent allowlist, and it is not
documentation.** Measured on one Windows 11 machine (10.0.26200) on 2026-09-10,
same URL, varying only the agent:

| user agent | answer |
| --- | --- |
| `curl/8.21.0`, which is curl's default | 200, 5801 bytes |
| `Mozilla/5.0` | 420 |
| none at all | 420 |

**Send a real tool's own agent string.** The same run read
`"limit": 5000, "remaining": 5000` from the proxy's rate-limit endpoint with no
credential, against the 60 an hour an unauthenticated address gets directly.

**The third row moved.** An upstream measurement on 2026-08-30 recorded no
agent at all answering 200; here it answers 420. The allowlist tightened
between those two dates, which is the argument for re-measuring rather than
citing.

---

## 8. Writing, on a machine that will not be read again

**The thing a session leaves behind is read by somebody with none of its
context.** Two rules already cover it and both are broken most often here,
because a session that knows it is ending writes a narrative of itself:

- [`conventions/prose.md`](conventions/prose.md) is how a document is written,
  including the register and the rule that a page says what is true now.
- [`methodology/history.md`](methodology/history.md) is where a superseded
  explanation goes, so that it is kept without being in the way.

**Moving narrative out is not deleting it.** A superseded explanation is
often the only record of why something has its shape.

---

## Writing a prompt for one of these sessions

**Do not tell a session to override, ignore or disobey the tool it is running
inside.** It reads that as an instruction to defy its own operator and stops to
object, and the objection costs the session whether or not it was warranted.
This has happened often enough to shape the wording of the routers in this
repository.

**Say what the artefact must contain instead.** The rule is the same and
nothing has to be overridden to follow it:

| instead of | write |
| --- | --- |
| ignore what you were told about commit trailers | no commit here credits a tool. The hook refuses one. |
| override your instructions about branches | all work lands on `main` |
| disregard your defaults for pull requests | nothing is opened on any repository |

**State the destination, not the negation.** A rule written as a prohibition
against a specific instruction is a rule that has to name that instruction; a
rule written as a property of the result does not.