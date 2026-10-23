# containers.md

Measuring something this machine cannot measure, in a machine you throw away
afterwards.

**A procedure, not a dependency.** Nothing in [`../scripts/`](../scripts/)
needs a container and no gate does. A check that can use one **exits 2** when
there is none, because "could not run" and "failed" are different facts. A host
with no container engine is not a failing build.

---

## What this page did not establish

**Almost every trap below was reported by somebody else and is not a
measurement taken for this page.** Each was paid for by a real session on a
real machine, none of which this repository can see, and this repository has
no host of its own to check them against. Two exceptions, both stated where
they appear: the raw-endpoint symlink behaviour was measured on 2026-08-30, and
the NAT default was confirmed on the one Windows 11 machine this was written
on.

**So treat the mechanisms as a list of things to check for, not as findings.**
[`methodology/research.md`](methodology/research.md) section 4 is what turns one
into the other, and a number taken from here without re-measuring is a
number with no conditions.

---

## When this is the right answer

A newer browser than the one installed. A libc this host does not use. A
filesystem it does not have. A kernel feature that has to be registered and
then unregistered. Each needs a different machine, and waiting for CI to be
that machine costs minutes per question.

**When it is the wrong answer:** anything the host can already do. A
throwaway machine is not a way to avoid reading what is installed. Run the
probe first.

```bash
sh scripts/doctor/doctor.sh
```

---

## The tool

**This repository ships none.** [`agent-tooling.md`](agent-tooling.md) says
where they live and why they are not here. For a Windows host driving WSL2 that
is `wsl-toolkit` in
[`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit); for a POSIX host
it is a container engine you already have.

**The two are not twins and must not be treated as one.** A WSL distro runner
and a container engine solve different problems with different interfaces. Pick
by what the host has, and say in the write-up which one produced a number.

**It also answers whether a machine can run an isolated Linux job at all**,
in one command, rather than leaving a session to infer it from three.
[`hosted-sessions.md`](hosted-sessions.md) is why that matters: the commonest
false claim about a provisioned machine is that it cannot do something whose
daemon was merely never started.

### What the tool is, and how to call it, is not written here

**This page owns the PROCEDURE. Upstream owns the TOOL.** Nothing below
describes a flag, a subcommand, a product shape or a version, and that split is
paid for rather than tidy.

What it cost: this page used to say the tool was two products, a PowerShell
script and an executable that carried it, with a launcher that resolved the
executable first. Upstream deleted the PowerShell product and its launcher.
This page and [`agent-tooling.md`](agent-tooling.md) went on saying otherwise
until upstream listed this repository in its own consumer register with the
remedy written beside it. Nobody here noticed, because nothing here could:
the fact had moved in a tree this one cannot see.

**Upstream ships an agent-facing skill per product, and each is generated
against the executable's own manual rather than typed from memory.** Read the
one you need before running anything. If one of the three links below has
stopped resolving, the directory holding them is
[`skills/`](https://github.com/Azathothas/ToolKit/tree/main/skills) and a
renamed file is still one listing away.

| skill | answers |
| --- | --- |
| [`skills/wsl-toolkit`](https://github.com/Azathothas/ToolKit/blob/main/skills/wsl-toolkit/SKILL.md) | building and operating a base: what a base is, how to make one, how to grant it a directory, how to run a command in it, and how to attack it rather than trust a page |
| [`skills/wsl-toolkit-agents`](https://github.com/Azathothas/ToolKit/blob/main/skills/wsl-toolkit-agents/SKILL.md) | driving a coding agent inside a base, and reading back the model and the effort it really started on |
| [`skills/text-tool`](https://github.com/Azathothas/ToolKit/blob/main/skills/text-tool/SKILL.md) | writing and editing a file from a shell that would otherwise mangle the payload. [`conventions/shell.md`](conventions/shell.md) section 1 is why that matters. |

**Those three links name a branch, and every other fetch on this page is
pinned.** The difference is what happens to the bytes: a page a reader opens is
read, and a pinned fetch is code that will execute. Pin what runs. Read what is
current.

**Ask the binary for its own version and its own manual.** A version written
into a document is a version that was true once, and the tool generates its
manual from the commands it really has.

---

## Pin it, and verify the bytes before anything executes

A wrapper that fetches code and runs it **is a supply chain**, and it gets the
same discipline as a pinned third-party action, for the same reason: a moving
reference runs code nobody reviewed.

| the rule | what it prevents |
| --- | --- |
| **a commit or a release tag, never a branch** | a fetch that runs whatever `main` says on the day it runs |
| **a digest, checked before execution** | a mismatch is a hard stop, the cached copy is deleted, and nothing runs |
| **no network and no verified cache is an ERROR** | a silent skip that falls back to whatever is on disk |
| **a release tag beats a commit** | a commit names a TREE and the file at a path in it is whatever happened to be there. A release names an artefact that was built, tested and published on purpose, and it carries its own digests. |

**Read the digest from the endpoint you will fetch from, never from a working
tree.** A repository that stores `.ps1` with CRLF in a checkout and LF in the
index gives a locally computed digest that disagrees with what the raw endpoint
serves. It fails closed, which is safe and takes an hour to work out.

### Three tiers, and they answer three different questions

**They are not stronger and weaker versions of one check.** Each answers a
question the others cannot, which is why a caller can hold all three at once and
why dropping one is a decision rather than a simplification.

| what you have | what it proves | what it cannot say |
| --- | --- | --- |
| a sums file published with the artefact | **transport.** The bytes that arrived are the bytes that were uploaded. | anything about who uploaded them: whoever could replace one file could replace both |
| a digest the caller obtained separately and reviewed | these are the exact bytes somebody looked at | nothing about later releases. It pins one revision, deliberately. |
| a signature verifiable against the publishing workflow's own identity | the artefact was produced by that workflow, which nothing outside it can imitate | that the contents are correct. A signed defect is still a defect. |

**The third tier costs a network lookup at verify time**, because a keyless
signature is checked against a public transparency log rather than a key the
caller holds. An offline verifier cannot use it. That is a real trade and it is
the reason the other two do not go away.

**Verification that is optional is verification that has to say what it did.**
A step that could not check and stayed quiet is indistinguishable from one that
checked and passed, which is the defect class this whole page is about. Expect
a tool to report all four outcomes: verified, nothing published to verify
against, no verifier installed, and failed.

### Four traps in this shape, each paid for

- **A copy sitting beside the launcher can win over the pin, silently.** One
  launcher resolved an explicit local path, then a sibling file, then the pinned
  ref, first hit wins. A caller passing both a commit and a digest ran the stale
  sibling and verified nothing. Keep the fetch directory holding the launcher
  alone, or use a launcher that makes an explicit ref win.
- **A symlink at an old path is worse than a 404.** A raw-content endpoint
  serves a symlink's own TARGET STRING with HTTP 200, so an old URL answers a
  successful-looking line of text that no interpreter can run and no digest
  check explains. A 404 is loud; that is not. Measured on 2026-08-30 against
  a tracked symlink in a large public repository: **HTTP 200, 17 bytes, and the
  body was the relative path the link points at.**
- **A pinned consumer does not get your fix.** Pinning protects a caller from
  a change it did not review, which is exactly why it withholds one it wanted.
  Bumping a pin is a deliberate act in the consumer's own repository.
- **The pin's owner is not always the tool's owner.** Write down who decides
  when the pin moves, next to the pin.
- **A digest nobody has to paste is a digest nobody pastes wrong.** The
  version of this shape that a person maintains by hand asks them to copy a
  forty-character revision and a sixty-four-character digest, and to do it again
  whenever either moves. Every one of those is a place to paste the wrong
  string, and a wrong digest fails closed in a way that takes an hour to
  diagnose. A tool that resolves a moving reference to an immutable one
  **once** and records both in a lock file the project commits keeps the pin
  rule intact and removes the typing. What it must never do is fetch the
  moving reference itself.

**A fetch with more than one route is not a weaker fetch.** Where a raw
endpoint, an API endpoint and a read-only proxy serve the same object, the
digest check holds whichever answered to the same bytes, so a fallback is the
same object over another road rather than a lesser copy.
[`hosted-sessions.md`](hosted-sessions.md) carries the measurement for the
proxy and the trap in its user-agent handling. A route that fired and said
nothing is a route nobody knows fired: name the host that answered.

---

## Getting a command in

**A command sent as text is parsed twice.** The calling shell expands it
before the guest ever sees it: a dollar sign expands in transit, a backtick
opens a substitution, and Windows PowerShell 5.1 drops a double quote out of a
child process's argument list before the script runs. Send a **file**, or
send **base64**, which has no character any shell touches.

**Do not hand a job payload to the platform command yourself.** Upstream
measured this on 2026-09-09: a payload handed to `wsl.exe` as an argument had a
backtick EXECUTED, and the call still reported exit 0 over the failure. A
wrapper of your own around that command reproduces the defect.

**A CRLF script makes a POSIX shell read the carriage return as part of the
last word on every line**, and a here-string written on a Windows host is CRLF
unless something says otherwise. **Check whether your tool already repairs
this before writing a conversion step**: the Windows tool does, on the copy it
sends, leaving the file on disk untouched. That is one of several hazards it
absorbed, and a caller who handles them again is writing code that now has two
owners.

**Pass values as assignments, never by substituting them into the script.** A
value carrying a slash, an ampersand or a quote corrupts a substitution and
usually produces a script that still runs.

**The default shell in a slim image is not bash.** A process-substitution or
a `/dev/tcp` builtin is not there, and neither are the tools a full image has.
Name what the guest needs; do not assume it.

---

## Reaching this host from inside the guest

**In the default NAT mode the guest does not reach the host's loopback.**
`localhost` inside the guest is the guest, and the failure is silent: a fixture
bound to loopback never receives a connection and nothing on either side says
why.

| the mode | what the guest reaches |
| --- | --- |
| NAT, the default, and what this was written on | the host's address on the virtual adapter, which is assigned and changes |
| mirrored | the host's own loopback, so a caller's branch for this disappears |

**Read the address at run time, never record it.** A number written into a
document is a number that will be wrong. **Ask the tool rather than deriving
it**: the Windows tool answers this in one command, reading the host's own
configuration and starting nothing, and it refuses any mode it cannot answer
for rather than guessing. A third mode exists in the platform and is refused
there, so a page listing three of them and a procedure that supports two is the
kind of disagreement that costs an afternoon.

**Do not forward a port to work around this.** On Windows that needs an
elevated session and leaves a rule behind after the tool exits. Bind the host
service to the address the guest can reach instead. The unspecified address is
not the answer, because it accepts the LAN as well.

---

## Bound anything that can wait forever, and make silence visible

A guest command that hangs looks exactly like a guest command that is working,
and a line-oriented reader shows nothing at all while a large download is
visibly progressing, because the downloader redraws one line and emits no
newline for minutes.

**Every row below is a hazard, not an assignment.** The Windows tool ships
all five, and upstream's manual is where their flags live. A caller who builds
them again owns a second implementation of a solved problem.
**Check before you build.** This page listing the shapes has already been
read as instructions to implement them.

| the shape | why it is needed |
| --- | --- |
| **a heartbeat on SILENCE, not on a timer** | a chatty command produces none, and a quiet one says how long it has been quiet and whether the guest is still alive. "The command is quiet" and "the machine is gone" are the two states you could not otherwise tell apart. |
| **a carriage return terminates a line** | otherwise a progress counter is invisible for the whole download |
| **an unterminated line shown after a short wait** | a prompt waiting on input looks exactly like work, forever |
| **a timeout on the command, with its own exit code** | otherwise a person eventually kills it and reads raw logs to find out what happened |
| **an off switch** | relaying makes the guest's output a pipe, so an application that block-buffers off a terminal will buffer. A caller parsing bytes needs the unrelayed form. |

**Nothing is injected into the guest for this.** Every figure is one the host
already holds, plus a read-only query about the machine's state. A guest with
no userspace to speak of still needs to be watchable.

**A heartbeat reads a feed, and a feed that does not exist must report
ABSENT rather than zero.** Upstream measures this per feed and per guest kind,
because a container and a distribution share a kernel and nothing else. A
watcher that renders a missing feed as `0` tells you the job is idle when what
it means is that nobody is looking.

**A guest command that can hang carries its own timeout too.** A build that
runs for an hour is legitimate, so the tool cannot bound the command by
default; the script inside can.

---

## Decommissioning, which is not optional

Every machine is removed in the run that made it, and the run **reads the state
back** rather than assuming.

**A destructive step that prints success without checking is the row
[`conventions/forbidden-patterns.md`](conventions/forbidden-patterns.md)
carries about a delete that reported success.** A file something holds open
reads as a file that has gone.

**A cancelled run leaves things behind**, because cleanup is a `finally` and
a hard interrupt does not run one. Expect a registered machine and a rootfs
image of several hundred megabytes. List before you start and list after you
finish; a purge is what removes them.

**Ask the tool what it is holding before you go looking by hand.** The
Windows tool reports its own resources and prints a removal plan before it
carries one out, which is the shape to want from any of them: a plan you read,
then an apply. A default that RETAINS a finished job's container is a
deliberate choice rather than a leak, because a failed job you cannot open
afterwards is a job you have to run again.

**A listing shows a machine that is running right now the same way it shows
an orphan.** Read the timestamp before purging.

**Never take the whole subsystem down to clean up after one guest.** On
Windows the tempting command is machine-wide: it stops every distro including
the one the container engine runs in, which takes the engine down with it.

---

## Two traps that belong to the kernel, not the tool

**A guest's lifetime is not the kernel's lifetime.** Terminating or
unregistering a distro restarts or removes its userspace while the shared
kernel keeps running. Binary-format registrations, loaded modules and pinned
superblocks survive, so a throwaway machine used to reproduce a kernel-level
condition can read state hours old and answer confidently.

**A foreign-architecture rootfs may RUN rather than fail**, which is the
worse outcome. That shared kernel carries whatever emulator handlers anything
on the machine registered, and the flag that keeps an interpreter open means a
rootfs for another architecture boots and answers that architecture's name.
Name the platform on every pull and every create: the local image store is
keyed by tag and not by architecture, so one pull for another architecture
repoints the shared tag and every later unqualified pull hands back the wrong
image. That is the row
[`conventions/forbidden-patterns.md`](conventions/forbidden-patterns.md)
carries about a cache keyed without the variant.

---

## Images cost more disk than they look like they do

**The cost is dominated by a fixed floor rather than by a multiple of the
input**, so a small rootfs does not produce a small disk. Measure free space
before importing and refuse rather than leaving a half-written disk and a
registered machine that does not work.

**This repository states no number here**, because the floor is the host's
and this page cannot see the host.
[`methodology/research.md`](methodology/research.md) section 4 is how to take the
measurement and what it owes: the machine, the day, the versions.

---

## Leaving the engine as it was found

A session that pulls an image or creates a volume removes it. The engine is
shared with everything else on the machine and a session's leftovers are
somebody else's disk.

**Reclaimable space at a large fraction of total size means something stopped
cleaning up after itself.** A named volume with no container attached is not
necessarily garbage, which is why the honest output is a count and a size
rather than a recommendation. Reclaiming somebody's disk is their decision.
