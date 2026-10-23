# AGENTS.md

errand-box is one Rust program that sets up a Linux host so that each errand
session runs in its own Incus system container. It installs and configures
Incus, a guest image, kage and errand, keeps the daemons up on hosts with or
without an init, and implements the `bailey` interface errand calls, so that
errand runs unchanged. The audience is an operator who runs errand on a host
they control, and the agents who work on this repository.

This file is a router. It restates no rule that has a home in another file.
Each row links to the file that holds the rule, and that file is the
authority. To read a row is not to read the rule.

---

## Start here, every session

1. Read [`TODO/PROGRESS.md`](TODO/PROGRESS.md). It is the record: the
   measured baseline, what the last session did, and the work order. No
   other file has a work order.
2. Run the probe. A different machine or a moved tool changes what this
   session can prove.

   ```sh
   sh scripts/doctor/doctor.sh
   ```

3. Find the row for your task below, and read what it names, in full.

---

## The routing table

| the task | read, in this order |
| --- | --- |
| Any session, first | [`TODO/PROGRESS.md`](TODO/PROGRESS.md), [`TODO/RULES.md`](TODO/RULES.md), [`docs/methodology/sessions.md`](docs/methodology/sessions.md) |
| Work on an open entry | the entry in `TODO/`, [`docs/methodology/gate.md`](docs/methodology/gate.md), [`docs/conventions/code.md`](docs/conventions/code.md), [`docs/conventions/forbidden-patterns.md`](docs/conventions/forbidden-patterns.md) |
| Write a new entry | [`docs/methodology/authoring.md`](docs/methodology/authoring.md), [`docs/methodology/work-todo.md`](docs/methodology/work-todo.md). Do not implement it in the same step. |
| Change the shim (`src/shim.rs`, `src/session.rs`, `src/trust.rs`) | [`docs/bailey-contract.md`](docs/bailey-contract.md), then [`docs/design.md`](docs/design.md) |
| A native Incus backend in errand | [`docs/incus-backend.md`](docs/incus-backend.md), entry T-019 |
| Change setup, `up` or the supervisor | [`docs/operations.md`](docs/operations.md), [`docs/edge-cases.md`](docs/edge-cases.md) |
| Change the guest image (`image/`) | [`docs/design.md`](docs/design.md) section "The guest image", then [`docs/edge-cases.md`](docs/edge-cases.md). A changed file there rebuilds every host's image (E-31). |
| A host problem: something fails on one machine | [`docs/edge-cases.md`](docs/edge-cases.md). Each new condition gets a row, a fallback and a test. |
| Move a pinned version | `sh scripts/pins.sh` (its header), then [`docs/methodology/vendoring.md`](docs/methodology/vendoring.md) |
| Run the tests, or add one | [`docs/tests.md`](docs/tests.md) |
| Without root, or another engine | [`docs/research/rootless.md`](docs/research/rootless.md), [`docs/research/engines.md`](docs/research/engines.md), [`research/README.md`](research/README.md) |
| The agent's home inside a guest, or sandhome | [`docs/research/sandhome.md`](docs/research/sandhome.md) |
| Find something out, or measure | [`docs/methodology/research.md`](docs/methodology/research.md) |
| Study another repository | [`docs/methodology/research.md`](docs/methodology/research.md) section 3 |
| Third-party code in the tree | [`docs/methodology/vendoring.md`](docs/methodology/vendoring.md). Patch it here; upstreaming is not a topic. |
| Anything remote: a push, an issue, a release | [`docs/conventions/git.md`](docs/conventions/git.md), [`docs/security/remote-ops.md`](docs/security/remote-ops.md) |
| A credential or a token | [`docs/security/secrets.md`](docs/security/secrets.md), [`SECURITY.md`](SECURITY.md) |
| Write anything: a page, a comment, a commit message | [`docs/conventions/prose.md`](docs/conventions/prose.md), then [`docs/conventions/docs.md`](docs/conventions/docs.md) |
| A shell script, a heredoc, quoting, an exit code | [`docs/conventions/shell.md`](docs/conventions/shell.md) |
| Wait for something | [`docs/conventions/shell.md`](docs/conventions/shell.md) section 10. Do not end the turn to wait. |
| Something superseded | [`docs/methodology/history.md`](docs/methodology/history.md) |
| A review | [`docs/methodology/reviews.md`](docs/methodology/reviews.md) |
| A machine that is not the operator's | [`docs/hosted-sessions.md`](docs/hosted-sessions.md), [`docs/containers.md`](docs/containers.md) |
| How this project was started | [`docs/methodology/initialize.md`](docs/methodology/initialize.md), [`docs/methodology/template-sync.md`](docs/methodology/template-sync.md) |
| Close a session | [`docs/methodology/sessions.md`](docs/methodology/sessions.md), [`docs/methodology/reviews.md`](docs/methodology/reviews.md) |

Read what a row names in full: not a grep, not a skim, not a memory of an
earlier session. When two rows apply, read both.

---

## The absolutes

Each of these was broken once, somewhere.

1. **No tool gets credit in a commit.** No co-author trailer, no
   generated-with line, no tool name in the body. The identity is the
   operator's. [`.githooks/commit-msg`](.githooks/commit-msg) refuses a
   trailer; enable it with `git config core.hooksPath .githooks`.
2. **Commit on `main`. Push to `origin main` only when the operator asks for
   it in the session. Never force-push.**
3. **Every other repository is read-only.** No issue, pull request,
   discussion, comment, review, fork or star anywhere else.
4. **No secret goes into the tree, a log, an argument list, a URL, a commit
   message, an issue or a summary.** This repository is public: also nothing
   that names a private host, account or path.
5. **Read an exit code from the process that made it, not through a pipe.**
6. **The record changes in the same commit as the work.** `sh
   scripts/check-todo.sh` refuses counts that disagree with the rows.
7. **A unit of work is done when the gate passes**, all of it, with the
   commands run and the output read: `sh scripts/ci-local.sh`, and the live
   tests when the change reaches a host ([`docs/tests.md`](docs/tests.md)).
8. **Never signal `incusd` or any daemon by name.** Each guest's monitor is
   also named `incusd` (E-12). `scripts/check-no-kill-by-name.sh` refuses it.
9. **Run one build at a time, with `CARGO_BUILD_JOBS=4`.**

---

## The tree

| path | what is in it |
| --- | --- |
| `src/` | the program: setup, up, doctor, maint, the image build, and the bailey shim |
| `image/` | the guest's provisioning script, launcher and boot step, embedded in the program |
| `tests/` | the shim's integration tests with a fake Incus, the script tests, and `live/` |
| `examples/` | the three input files of `setup`, with every key |
| `pins.toml` | every download's version and SHA-256 |
| `bootstrap.sh` | fetches a release binary, checks its digest, and runs setup |
| `docs/` | the design, the operations, the contract, the edge cases, the research, and the conventions |
| `research/` | the scripts and raw output behind `docs/research/` |
| `scripts/` | the gate, the local CI runner, the record checker, the pin mover, the probe |
| `TODO/` | the record: progress, the index, the rules, and the entries |
| `.github/` | CI, the scheduled maintenance workflows, and the templates |

---

## Use the tool that exists

Before you install something, write your own, or decide that a job cannot be
done here, read [`docs/agent-tooling.md`](docs/agent-tooling.md). A missing
tool closes one route, not the question:
[`docs/methodology/sessions.md`](docs/methodology/sessions.md) asks for three
routes before anything is recorded as not possible.

| you want to | use |
| --- | --- |
| know the host | `sh scripts/doctor/doctor.sh` |
| know whether the tree is green | `sh scripts/ci-local.sh` (every CI step) or `--step NAME` |
| know whether the record agrees with itself | `sh scripts/check-todo.sh` |
| close an entry and move the counts | `sh scripts/check-todo.sh --set T-NNN done`. Do not type a count. |
| add a new upstream version | `sh scripts/pins.sh bump <tool> <version>` |
| test on a host with Incus | `sh tests/live/live.sh` (root; it starts real guests) |
| know what a host lacks | `errand-box doctor` (read-only) |

---

## What a session owes at its end

[`docs/methodology/sessions.md`](docs/methodology/sessions.md) is the
specification. In short, whether the session went well or not:

- the record updated, in the same commit as the work;
- the gate run, all of it;
- the summary table, in chat;
- the next prompt, in chat, and a resume prompt when anything is unfinished;
- every host object the session made for a test, removed.

---

## When you are unsure

In order: what the operator said in this session, what the linked rule says,
what the probe or the code measured, then ask the operator. Do not choose a
fifth option in silence. Two sources that disagree are a finding; report it.
