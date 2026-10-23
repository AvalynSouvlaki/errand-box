# TODO

Every entry, one line each, sorted by id. The entry itself is in the file
its row links to, and it closes there with its own acceptance command, run,
and the output recorded.

What to work on next is not here: the work order is in
[`PROGRESS.md`](PROGRESS.md), and only there. This file has the list, the
definitions, the counts and the argument behind the priorities.
[`RULES.md`](RULES.md) is how this repository is worked on.

The counts below come from the rows. Do not type them:

```sh
sh scripts/check-todo.sh --set T-NNN done   # move a status; every count follows
sh scripts/check-todo.sh --fix              # rewrite every count from the rows
sh scripts/check-todo.sh                    # check; the gate runs this
```

---

## Priority

- **P0** breaks correctness, loses data, or takes the process down.
- **P1** a documented capability does not work, or was never shown to work.
- **P2** worth doing; nothing is wrong without it.
- **P3** worth recording so it is not found again.

## Effort

S is under a day. M is a few days. L is a week. XL is two entries.

## Status

`open`, `partial`, `blocked`, `done`. There is no `wontfix` and no
`deferred`: a blocked entry stays open with the blocker named.

---

## The argument behind the priorities

The P0 entries are what errand-box is: one command that sets a host up, a
session that is a whole system, and daemons that come back after a restart.
They are done and shown live on one host.

The open P1 entries are the hosts the operator named and no run has
covered (systemd, musl, WSL2), the first release that `bootstrap.sh`
needs, the scheduled workflows that have not run on GitHub, and a session
with a real chat service. Each is a capability the documents state and no
measurement backs yet; a host type is P1 rather than P0 because the code
paths exist and the failure, if any, is at setup, before errand runs.

P2 and P3 make errand-box cheaper or clearer: rootless setup, a published
image, a pool size that fits the disk, errand's wording, a docs check, and a
native Incus backend in errand, which waits on errand's author.

---

## Counts

**23 items. 8 open, 4 partial, 2 blocked, 9 done.**

| priority | open | partial | blocked | done | total |
| --- | --- | --- | --- | --- | --- |
| P0 | 0 | 0 | 0 | 3 | 3 |
| P1 | 2 | 2 | 1 | 4 | 9 |
| P2 | 4 | 2 | 1 | 2 | 9 |
| P3 | 2 | 0 | 0 | 0 | 2 |
| **All** | 8 | 4 | 2 | 9 | 23 |

---

## Entries

| ID | Priority | Category | Status | Item |
| --- | --- | --- | --- | --- |
| [T-001](setup.md) | P0 | setup | done | One command sets up a host |
| [T-002](sandbox.md) | P0 | sandbox | done | errand runs unchanged, each session in its own system container |
| [T-003](setup.md) | P0 | setup | done | The daemons run, and come back, on a host with no init |
| [T-004](sandbox.md) | P1 | sandbox | done | A session's processes cannot outlive it |
| [T-005](sandbox.md) | P1 | sandbox | done | Docker, podman and FUSE work inside a session |
| [T-006](setup.md) | P1 | setup | done | The config setup writes is one the pinned errand accepts |
| [T-007](upkeep.md) | P1 | upkeep | partial | Scheduled workflows that open pull requests and issues |
| [T-008](setup.md) | P2 | setup | done | Bounded logs, and a daemon that restarts in a loop is reported |
| [T-009](upkeep.md) | P1 | upkeep | open | The first release |
| [T-010](setup.md) | P2 | setup | open | Setup without root at run time |
| [T-011](setup.md) | P1 | setup | done | Live test on a host with systemd |
| [T-012](setup.md) | P1 | setup | partial | Live test on musl and on WSL2 |
| [T-013](setup.md) | P2 | setup | partial | Live test on aarch64 |
| [T-014](sandbox.md) | P2 | sandbox | partial | A distrobuilder image, built once and published |
| [T-015](sandbox.md) | P1 | sandbox | blocked | A live session with a real chat service and model gateway |
| [T-016](sandbox.md) | P3 | sandbox | open | errand's description of this backend is partly wrong |
| [T-017](upkeep.md) | P3 | upkeep | open | A documentation check: every command, key and path a page names exists |
| [T-018](setup.md) | P2 | setup | done | Choose a pool size that fits the host disk |
| [T-019](sandbox.md) | P2 | sandbox | blocked | An incus backend in errand |
| [T-020](sandbox.md) | P1 | sandbox | open | A broker link that does not need proxy devices |
| [T-021](sandbox.md) | P2 | sandbox | open | Operator-chosen session credentials, safely |
| [T-022](sandbox.md) | P2 | sandbox | open | Egress a session can use beyond the HTTP broker |
| [T-023](setup.md) | P2 | setup | open | Observability: metrics over time, and session traces |
