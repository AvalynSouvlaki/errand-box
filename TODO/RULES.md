# Rules

How this repository is worked on. [`PROGRESS.md`](PROGRESS.md) is the
record and holds the work order; this file is the part that does not change
from session to session. Every general rule is a link.

| topic | where it lives |
| --- | --- |
| what a session owes at its start and its end | [`docs/methodology/sessions.md`](../docs/methodology/sessions.md) |
| the todo model: entries, the index, the counts | [`docs/methodology/work-todo.md`](../docs/methodology/work-todo.md) |
| what a unit of work passes before it is done | [`docs/methodology/gate.md`](../docs/methodology/gate.md) |
| commit identity, and what may reach a remote | [`docs/conventions/git.md`](../docs/conventions/git.md), [`docs/security/remote-ops.md`](../docs/security/remote-ops.md) |
| the register of every text | [`docs/conventions/prose.md`](../docs/conventions/prose.md) |
| where superseded wording goes | [`docs/methodology/history.md`](../docs/methodology/history.md) |

---

## 1. This project's commands

| | |
| --- | --- |
| every gate step | `sh scripts/ci-local.sh` |
| the live tests, on a host with Incus | `sh tests/live/live.sh`, as root |
| the record agrees with itself | `sh scripts/check-todo.sh` |
| move an entry's status and every count | `sh scripts/check-todo.sh --set T-NNN STATUS` |
| the record | [`PROGRESS.md`](PROGRESS.md) and [`INDEX.md`](INDEX.md) |

Measure the baseline again with these commands before you change anything.
The numbers in `PROGRESS.md` were true once.

---

## 2. Git, for this project

Work on `main` of this repository. Push to `origin main` only when the
operator asks in the session, and never force-push. The commit identity is
the operator's, from the repository's git configuration, with no trailer.

CI is off on this repository. `scripts/ci-local.sh` is the CI; a pull
request says that CI did not run, so that whoever merges it runs it.

---

## 3. The rules that bite here

### The record is part of the change

`PROGRESS.md`, `INDEX.md` and the entry change in the same commit as the
work. `scripts/check-todo.sh` runs in the gate, so counts that disagree with
the rows cannot reach a commit.

### A live test is the proof for anything that touches a host

The unit tests and the fake Incus prove the calls errand-box makes, not what
a host does with them. In the session that built this, four defects passed
every unit test and failed live: the agent lost its `docker` group (E-29),
rootless podman had no ids and no route (E-30), `up` trusted a zombie
supervisor (E-33), and errand refused a config key the generator wrote for
a later errand. Run `tests/live/live.sh` before you close an entry that
changes setup, `up`, the shim or the image.

### A host object made for a test is removed in the same session

An instance, an image, a user, a mount or a file a test made is deleted
before the session ends, and the deletion is checked. Never delete an
object this session did not make.

### Never signal a daemon by name

Every guest's monitor is also called `incusd`. A `pkill` by pattern also
matched the session's own shell once. Signal by pid, through the
supervisor.

### A blocked entry stays open

Nothing closes because somebody else would have to act. A blocked entry
names the blocker and what would unblock it.

---

## 4. Settled decisions

- **Incus system containers, not gVisor or a VM.** 2026-10-23. Measured on a
  second host: Incus costs 0.9 to 1.1 times runc; gVisor 1.2 to 9 times, with
  no FUSE and Docker only in part. A VM needs KVM, which many hosts lack.
  [`docs/research/engines.md`](../docs/research/engines.md).
- **errand stays unchanged; errand-box answers as `bailey`.** 2026-10-23. The
  podman backend of errand runs host processes, not a system;
  [`docs/bailey-contract.md`](../docs/bailey-contract.md) says why the
  bailey interface is the seam.
- **Root at setup.** 2026-10-23. Without root at run time Incus loses the
  disk quota; without root at all only emulation works, at 18 to 53 times
  the cost. [`docs/research/rootless.md`](../docs/research/rootless.md).
  Rootless setup is entry T-010.
- **s6 from the distribution, not s6-overlay.** 2026-10-23. s6-overlay's
  binaries look for `/package/admin/s6`, which a host does not have.
