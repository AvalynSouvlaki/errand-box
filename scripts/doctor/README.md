# doctor

What host is this, what is installed, and what is this repo. One read-only
pass, before any of it costs a task.

One implementation, in POSIX sh. On Windows it needs a POSIX layer, such as
Git Bash or WSL.

```bash
sh scripts/doctor/doctor.sh
```

## Why it exists

The defect it catches is an agent that assumes its environment. A session that
assumes node is present writes a node script and finds out at the gate. A
session that assumes Linux reaches for `pkill` on a machine that wants
`taskkill`. A session that takes "most tools are available" on trust plans
around a tool that is not there.

It is also the validator. When the operator states an environment, run this
and compare. A stated fact and a measured one that disagree is the finding.

## What it is not

It is a probe, not a gate. A missing tool is data, not a failure, so it exits
0 whenever it ran. It exits 2 only when it could not run at all. Nothing here
belongs in a gate chain.

It is read-only. No installer, no config change, no network call unless the
network flag is passed, and the only file it writes is a temp file it removes.

## Flags

| flag | what it does |
| --- | --- |
| `--json` | emit the schema document instead of the report |
| `--text` | select the human report explicitly. It is the default. |
| `--fast` | presence only, skip every version probe |
| `--net` | also test outbound reachability |
| `--group vcs` | probe one group only |

Groups: `vcs`, `runtime`, `compiler`, `pkg-lang`, `pkg-system`, `container`,
`build`, `quality`, `cli`, `cloud`, `shell`, `agent`.

## Measured runtime

On one Windows 11 machine, 86 tools, 51 of them present:

| run | time |
| --- | --- |
| full | 25 s |
| fast | 5 s |
| one group | 2 s |

The numbers are from that machine on 2026-08-25 and are here so a session
knows what to expect, not as a claim about any other host. On one Linux
x86_64 container with 4 CPUs on 2026-10-23, with 46 of the tools present,
`--fast` and `--group vcs` each took 0.15 s. The full run was not measured
there, because it runs the version flag of every package manager it finds. Windows is the slow
case: a process spawn costs more there than anywhere else, and this spawns one
per tool. Re-measure rather than quote these if the answer matters.

## The schema, agent-doctor/1

```
schema      the string "agent-doctor/1"
generated   ISO 8601 UTC
probe       impl, fast, group
host        os flavor wsl container kernel arch distro distro_version
            shell writable_tmp network
repo        is_git root branch remote dirty commits shallow identity
            hooks_path remote_looks_like_template has_codegraph ecosystems
summary     tools_found tools_missing
tools[]     id group found path version
notes[]     things the probe wants said out loud
```

Field notes that are easy to read wrong:

- `flavor` is the shell environment, not the OS. Under Git Bash on a Windows
  machine it reports `msys`, because that is the environment it runs in.
- `kernel` is a best-effort build identifier and its shape differs by platform.
  Do not parse it.
- `ecosystems` is read from manifest files that are actually present. It is
  evidence, not a guess from a directory name.
- `remote_looks_like_template` is a warning, not a fact about the project. It
  fires when `origin` contains the word template, which is the state a fresh
  clone of the template is in and must leave before any project work.
- `shallow`, `identity` and `hooks_path` are the three facts a session on a
  machine somebody else provisioned gets wrong most often, which is why the
  probe reads them rather than leaving each session to assume.
  [`../../docs/hosted-sessions.md`](../../docs/hosted-sessions.md) says what
  each costs. `identity` is empty when either half of the name and email pair
  is unset, because half an identity produces the same wrong author as none.
- `version` empty with `found` true means the tool answered nothing. That is
  reported in `notes` and it usually means a shim rather than an install.

## Things this cost to learn

Each of these was a real defect in this script, found by running it.

- A greedy regex over a version line reports the wrong half of the version
  and does it confidently. `git version 2.51.0.windows.3` came back as
  `5.0.windows.3`, `v22.11.0` as `7.0`. The fix is to split into tokens and
  take the first that reads as a version. A wrong number is worse than a blank
  one, because a blank one gets checked.
- The name may be joined to the number by a hyphen. `jq-1.8.2` read as no
  version at all until the pattern allowed one.
- Several tools block for as long as you let them. `kubectl version` without
  `--client` contacts a cluster. Every probe is time-limited at six seconds,
  and a timeout is reported as its own fact rather than as an absent tool.
- `probe_version` runs inside `$( )`, which is a subshell, so an assignment
  inside it is discarded. The caller reads the exit code.
- `wsl.exe` writes UTF-16LE, which a redirected stdout reads as empty. It is
  probed for presence only.
