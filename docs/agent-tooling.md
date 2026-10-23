# agent-tooling.md

**Read this before you install anything, write your own, or decide a job
cannot be done here.** It is a catalogue of tools that already exist, with
where each one lives.

**It carries names, links and one line each, and nothing else.** No flags, no
options, no exit codes, no worked invocations. Every one of those is upstream's
to change, and a page that copies them is a page that becomes wrong without
anybody editing it. Read the tool's own documentation at the link for how to
call it.

---

## The three reflexes this page exists to stop

| the reflex | what it costs |
| --- | --- |
| **installing something** | a system change nobody asked for, on somebody else's machine, that outlives the session |
| **writing your own** | a second implementation of a solved problem, with its own defects, that nobody else will ever fix |
| **refusing, because a tool "is not available"** | the most expensive of the three. [`methodology/sessions.md`](methodology/sessions.md) is the rule: a missing tool closes one route, not the question. |

**A tool being absent is a measurement, not a verdict.** Run the probe, say
what is missing, then find another route. Three routes considered and rejected
is a finding; one route tried is a stop.

---

## What this repository ships

Everything here runs with **no network**, which is why it is here rather than
upstream. A gate that has to fetch a check is a gate that is red when somebody
else's host is down, and a check fetched at gate time is code nobody reviewed
judging the tree.

| tool | what it does |
| --- | --- |
| [`../scripts/doctor/doctor.sh`](../scripts/doctor/doctor.sh) | the environment probe. What host, what shell, what tools, what the repository is. A probe, not a gate. |
| [`../scripts/common/check-gate.sh`](../scripts/common/check-gate.sh) | runs every check below and prints one verdict |
| [`../scripts/common/check-docs.sh`](../scripts/common/check-docs.sh) | links resolve, fenced blocks parse, orphan pages |
| [`../scripts/common/check-markers.sh`](../scripts/common/check-markers.sh) | ASCII text, no emoji, no marker character, no dash respelt in hyphens, and no unclosed fence |
| [`../scripts/common/check-one-home.sh`](../scripts/common/check-one-home.sh) | one fact, one home: no long sentence in two documents |
| [`../scripts/common/check-placeholders.sh`](../scripts/common/check-placeholders.sh) | did a template placeholder survive into a real file |
| [`../scripts/common/check-control-bytes.sh`](../scripts/common/check-control-bytes.sh) | a literal control byte in a tracked text file |
| [`../scripts/common/check-attribution.sh`](../scripts/common/check-attribution.sh) | does any commit credit a tool. [`conventions/git.md`](conventions/git.md) is the rule and the hook that refuses one before it exists. |
| [`../scripts/common/check-changelog.sh`](../scripts/common/check-changelog.sh) | the four changelog rules a machine can hold |
| [`../scripts/common/check-no-secrets.sh`](../scripts/common/check-no-secrets.sh) | does anything in the tree carry something that must not be published |

The template also ships `check-twins`, `check-remote-items` and `mine-repo`.
This project left them out.
[`../scripts/common/README.md`](../scripts/common/README.md) says why.

---

## What lives upstream

**Most of these were here and were removed.** A tool kept in two
repositories acquires two sets of defects, and one of the two never gets fixed.
Four left together, and two of the four were carrying a defect their upstream
had already fixed on the day they went.

**Fetch by a pinned commit or a release tag, never a branch.** A moving
reference runs code nobody reviewed. [`containers.md`](containers.md) has the
worked shape of a pinned wrapper and what it cost to get right.

**Where a row links a skill, read the skill and not this row.** Upstream
writes one self-contained page per product and generates it against the
executable's own manual, so it is current in a way no summary here can be. What
this repository wrote instead was a product shape that upstream later deleted,
and the correction arrived from upstream's consumer register rather than from
anybody here. That class of sentence cannot be caught locally.

| tool | upstream | what it does |
| --- | --- | --- |
| `wsl-toolkit` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | surveys the host, owns one WSL distro with a container engine in it, runs a command in a container or a set of them, and removes what it made. [`containers.md`](containers.md) is the procedure; the [skill](https://github.com/Azathothas/ToolKit/blob/main/skills/wsl-toolkit/SKILL.md) is how to drive it. |
| `wsl-toolkit-agents` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | runs a coding agent inside a base and reads back what model and effort it really started on. A [skill](https://github.com/Azathothas/ToolKit/blob/main/skills/wsl-toolkit-agents/SKILL.md), not a second binary. |
| `git-sync` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | commit and push with [`conventions/git.md`](conventions/git.md)'s rules enforced rather than remembered |
| `fill-license` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | writes a `LICENSE` from a canonical text with the holder filled in, and refuses the ones whose notice is not yours to alter. |
| `text-tool` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | writes or patches a file without the shell touching the payload, and refuses a substitution whose match count you did not state. [`conventions/shell.md`](conventions/shell.md) section 1 is why that matters; the [skill](https://github.com/Azathothas/ToolKit/blob/main/skills/text-tool/SKILL.md) is how to drive it. |

---

## The general-purpose ones, which are somebody else's entirely

**Presence is not capability.** The probe reports what resolves on `PATH`,
and a name that resolves can still be the wrong program: measured on one
Windows 11 machine, `sort` resolved to PowerShell's own `Sort-Object` alias and
`python3` resolved to a Microsoft Store stub that exits 49 without running
anything. Probe by RUNNING the tool, not by finding it.

| job | reach for | why not the obvious thing |
| --- | --- | --- |
| talk to a code host's API | [`gh`](https://cli.github.com/) | reads only, and never against somebody else's repository. [`security/remote-ops.md`](security/remote-ops.md). |
| fetch a URL | `curl`, or the host's own client | in Windows PowerShell 5.1 `curl` is an ALIAS for a cmdlet that takes different arguments. [`conventions/shell.md`](conventions/shell.md). |
| read or reshape JSON | [`jq`](https://jqlang.github.io/jq/) | never a regular expression over JSON. A bracket inside a string value is how the template's own page joiner lost an entire comment corpus. |
| read or reshape YAML | [`yq`](https://github.com/mikefarah/yq) | the same reason |
| lint POSIX shell | [`shellcheck`](https://www.shellcheck.net/) | it finds the quoting and exit-code traps [`conventions/shell.md`](conventions/shell.md) documents, before they ship |
| time a command honestly | [`hyperfine`](https://github.com/sharkdp/hyperfine) | a single `time` run is not a measurement. [`methodology/research.md`](methodology/research.md) section 4 says what one owes. |
| count lines of code | [`scc`](https://github.com/boyter/scc) or [`tokei`](https://github.com/XAMPPRocky/tokei) | counters disagree about blank and comment lines, so name which one produced a number |
| search a tree | [`rg`](https://github.com/BurntSushi/ripgrep) | it locates; it does not confirm. Open the file. |
| run something on Linux from Windows | `wsl-toolkit`, above | never install a distro by hand and leave it registered. [`containers.md`](containers.md). |

---

## Adding a row

1. **The tool has to already exist and be reachable.** This is a catalogue, not
   a wish list.
2. **One line, and no behaviour.** If the row needs a flag to be useful, the
   flag belongs in upstream's documentation and the row belongs in
   [`containers.md`](containers.md) or nowhere.
3. **Say where it lives**, as a link a reader can open.
4. **A row for a tool nothing in this repository uses is a row somebody
   maintains for nothing.** Delete it instead.
