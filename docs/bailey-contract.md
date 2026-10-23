# The bailey contract errand-box implements

This is what errand expects of the program it runs as `bailey`, read from
errand's source. errand-box's `src/shim.rs` and `src/session.rs` implement
it; [design.md](design.md) says how. A subagent wrote this page from the
pinned sources, and each citation was opened at its lines; the build session
checked a sample of them again.

What a replacement `bailey` binary must do so that errand runs unchanged.

Pins: errand@4a8011cd3668, bailey@98d9b455adc6. Every citation below was
checked by opening the file at those lines. Short forms: `E:` is
errand@4a8011cd3668, `B:` is bailey@98d9b455adc6. A statement marked
INFERRED is a conclusion from the cited code, not something the code
says in one place.

## 0. Summary

1. errand runs the bare name `bailey` from PATH. Six call shapes exist:
   `doctor`, `trust <policy>`, `untrust <policy>`, two short check runs of
   `true` (the shape probe and the confined check), and the long session
   run. (sections 1 to 4)
2. `doctor` must exit 0 and print a line starting `landlock: ABI`, or
   errand refuses to start. A line `user namespaces: ... no` also refuses.
   A line `cgroup delegation: ... no` is a gap, and the default config
   refuses to start on any gap. (section 3.1)
3. The session run carries ACP (JSON-RPC 2.0, one record per LF) between
   errand and `kage rpc` inside the sandbox, on the launcher's own stdin
   and stdout. The launcher may write only to stderr. (section 7)
4. Stop is SIGTERM to the launcher's process group, then SIGKILL after
   `sandbox.gracePeriodMs` (default 10000 ms), then `untrust`. errand has
   no orphan cleanup for this backend, so nothing may outlive the
   launcher. (section 8)
5. Inside, the project is at `/workspace`, the session state at `/state`,
   and TCP to `169.254.169.1:<port>` must reach the broker on host
   `127.0.0.1:<port>`. (sections 6 and 9)

## 1. Finding the binary, choosing the backend

- The backend comes from config `sandbox.backend`, `"bailey"` or
  `"podman"` (E:src/config/validate.rs:1134-1146, 1590-1596). The default
  is bailey (E:src/config/schema.rs:600-601).
- errand builds exactly the configured backend and never falls back to the
  other (E:src/daemon.rs:67-101). There are only two backends
  (E:src/sandbox/mod.rs:244-250).
- The program name is the bare string `bailey`, for every call
  (E:src/sandbox/bailey.rs:49, 602). There is no setting for its path.
- Management calls (`doctor`, `trust`, `untrust`, the two check runs)
  inherit the daemon's whole environment, so `bailey` is found on the
  daemon's PATH (E:src/sandbox/bailey.rs:47-65). INFERRED: Rust's
  `Command` searches the parent PATH when the child environment is not
  changed.
- The session run clears the environment and sets the session
  environment (E:src/sandbox/spawn.rs:85-97). That environment carries the
  daemon's PATH (E:src/sandbox/bailey.rs:44, 83-87). INFERRED: Rust's
  `Command` then searches that PATH, which is the daemon's PATH unless a
  launch variable is named PATH.
- To swap in a replacement: put an executable named `bailey` ahead of the
  real one on the daemon's PATH. Experiment 50 does this with a logging
  wrapper (research experiments/50-errand-probe.sh:24-29, 41-42).

### 1.1 Why errand-box answers as bailey, not as podman

errand has two backends, and either could be made to drive Incus. READ, at
errand v0.3.1 (`src/sandbox/podman.rs`):

- The podman backend runs every session as `podman run --userns=keep-id
  --read-only --cap-drop=ALL --security-opt no-new-privileges
  --network=pasta:...` with errand's own image (lines 52-124). Those flags
  are the guarantees errand then reports. A real podman with them gives a
  hardened application container: no systemd, no Docker inside, no package
  installs. That is not the session errand-box exists to give.
- A program named `podman` that drives Incus would have to accept those
  flags and not apply them, because a system container with systemd and
  Docker needs capabilities and a writable root. errand would then report
  guarantees that do not hold. It would also have to answer `podman info
  --format {{.Host.Security.Rootless}}` with `true` (lines 205-214) when the
  daemon behind it runs as root, and `ps --filter label=...` and `rm
  --force` for errand's own cleanup.
- The bailey interface is a policy file and six call shapes (section 0).
  errand-box can honour each of them as written, and its `doctor` output says
  what it does instead of Landlock (section 3.1), so errand's report stays
  true.

So errand-box implements bailey, and errand's config keeps `sandbox.backend
= "bailey"`. [incus-backend.md](incus-backend.md) specifies the backend
errand would need to drive Incus itself.

## 2. How each call is run

| call | stdin | stdout | stderr | cwd | environment | timeout |
| --- | --- | --- | --- | --- | --- | --- |
| management (`run_bailey`) | null | captured whole | captured whole | none (daemon's), or the probe dir for the shape probe | daemon's, inherited | none |
| session (`spawn_agent`) | pipe from errand | pipe to errand | pipe to errand | the project's host path | cleared, then the session env | none; stop is by signal |

- Management calls: E:src/sandbox/bailey.rs:47-65 (stdin null, stdout
  and stderr piped, `output().await`); exit code is `code()` or -1 on a
  signal (line 60). A spawn error becomes code -1 with the error text as
  stderr (E:src/sandbox/bailey.rs:321-330, 770-780).
- INFERRED: there is no timeout, so a management call that hangs hangs
  errand's startup or the session launch.
- Session call: E:src/sandbox/spawn.rs:79-101 (`env_clear`,
  `process_group(0)`, three pipes), cwd from E:src/sandbox/bailey.rs:600-603.

## 3. At daemon start: the probe

Runs before the chat service is touched (E:src/serve.rs:795-822,
E:src/daemon.rs:802-818). Order (E:src/sandbox/bailey.rs:387-432):

1. `bailey doctor`
2. the shape probe (three calls)
3. a PATH lookup of `kage` on the daemon's PATH (E:src/sandbox/bailey.rs:419-432,
   503-508; E:src/sandbox/runtime.rs:40-42, 83-103). Not a bailey call,
   but a replacement cannot avoid it: errand refuses to start without a
   host `kage`.

A refusal ends the daemon with `Exit::Refused`; a gap under
`requireFullEnforcement` ends it with `Exit::EnforcementGap`
(E:src/serve.rs:813-821).

### 3.1 `bailey doctor`

- argv: `bailey doctor` (E:src/sandbox/bailey.rs:388).
- Exit code must be 0. Otherwise: unavailable, reason "bailey is not
  installed, or `bailey doctor` failed" (E:src/sandbox/bailey.rs:389-394).
- errand parses stdout and stderr joined with a newline
  (E:src/sandbox/bailey.rs:396). It splits on `\n` and trims each line
  (E:src/sandbox/bailey.rs:133-136). For each key it takes the first line
  that starts with the prefix.

| line errand looks for | rule | effect | citation |
| --- | --- | --- | --- |
| `landlock:` | the line must exist and start with `landlock: ABI`; the number is not read | otherwise unavailable: "the kernel does not provide Landlock, which this backend requires" | E:src/sandbox/bailey.rs:138-146 |
| `user namespaces:` | if present and it ends with `no` | unavailable: "the kernel does not allow user namespaces, which this backend requires"; absent is accepted | E:src/sandbox/bailey.rs:148-155 |
| `cgroup delegation:` | if present and it ends with `no` | a gap: per-session memory, cpu and process limits not applied; absent is no gap | E:src/sandbox/bailey.rs:157-167 |

- No other line is read.
- A gap with `sandbox.requireFullEnforcement = true` stops the daemon
  (E:src/daemon.rs:812-816). The default is true
  (E:src/config/schema.rs:602-603). So with default config a replacement
  that prints `cgroup delegation: no` makes errand refuse to start.
  Experiment 50 sets it false (research experiments/50-errand-probe.sh:37).
- Real bailey prints these indented two spaces under `kernel:`, and
  always exits 0 (B:crates/bailey/src/cli.rs:463-549). Its forms:
  `landlock: ABI <n>` or `landlock: unavailable` (468-487);
  `user namespaces: yes|no` (489, 551-553);
  `cgroup delegation: no`, `cgroup delegation: memory, pids, cpu`, or
  `cgroup delegation: <list> (<missing> not delegated)` (497-518).
  INFERRED: the third form ends with `)`, so partial delegation is not a
  gap for errand.
- INFERRED: a backend that does not use Landlock (a VM, Incus, gVisor)
  must still print `landlock: ABI <n>` or errand will not start. The gVisor
  shim prints the host kernel's ABI for this reason (research
  poc/bailey-gvisor/bailey:108-112).

### 3.2 The shape probe

Purpose: refuse at startup a bailey too old for the policy errand writes
(E:src/sandbox/bailey.rs:405-417, 669-673).

- errand writes `<stateDir>/probe/shape.toml` and
  `<stateDir>/probe/resolv.conf` (E:src/sandbox/bailey.rs:675-713).
  `<stateDir>` is config `stateDir` (E:src/daemon.rs:88-91).
- The probe policy is the normal policy (section 6) built for a fake
  launch: session id `probe`, project and state both the probe directory,
  empty env, provider `probe`, no agent runtime, no extras
  (E:src/sandbox/bailey.rs:681-707). So the same host directory is placed
  at both `/workspace` and `/state`. With no port list, `[network]` uses
  port 443 (E:src/sandbox/policy.rs:310-314).
- Calls, in order (E:src/sandbox/bailey.rs:715-723):
  1. `bailey trust <stateDir>/probe/shape.toml`, result ignored.
  2. `bailey run --config <stateDir>/probe/shape.toml --quiet -- true`,
     cwd `<stateDir>/probe`. No `--isolate`, no `--profile`, no
     `--egress-proxy`.
  3. `bailey untrust <stateDir>/probe/shape.toml`, result ignored.
- Pass condition: exit code 0 of call 2 (E:src/sandbox/bailey.rs:723).
- Fail text: "the installed bailey does not accept the policy this backend
  writes, which needs resources.file_max and relocatable grants; update
  bailey" (E:src/sandbox/bailey.rs:411-414).
- Real bailey refuses unknown keys in a config (B:crates/bailey/src/config.rs:193-195
  and every `Raw*` struct to 352). A replacement must accept every key in
  section 6.
- Real bailey refuses to run as real root (B:crates/bailey/src/cli.rs:996-1018,
  called at 1038). INFERRED: errand run as root therefore fails this probe
  with real bailey. A replacement may differ.

## 4. At session start: the launch

Sequence in E:src/sandbox/bailey.rs:510-627:

1. Look up `kage` again; refuse if absent (515-521).
2. Create the state directory (523-525). It is `<stateDir>/<sessionId>`,
   with `home/` made earlier (E:src/session/manager.rs:567-576).
3. If `sandbox.diskTmp`: empty and recreate `<stateDir>/<id>/tmp`
   (526-530; E:src/sandbox/backend.rs:130-148).
4. Write kage's config to `<stateDir>/<id>/kage/kage/config.toml` and copy
   plugins to `.../kage/kage/plugins/` (531-537;
   E:src/sandbox/agent_config.rs:443-470).
5. Swap provider keys for nonces (538-545; section 9).
6. Write `<stateDir>/resolv.conf` (546, 651-667).
7. Write the policy to `<stateDir>/<id>/policy.toml`, through
   `write_beneath` so a planted link is not followed (547-571).
8. `bailey trust <policy>` (573-579).
9. The confined check (580; section 4.2).
10. Build the session environment (585-593; section 5).
11. Start `bailey run ...` (594-603; section 4.3).

Before this, the session writes its git config, its wrapper directory and
its memory file into the state directory (E:src/session/session.rs:1063-1067).

### 4.1 `bailey trust <policy>`

- argv: `bailey trust <stateDir>/<id>/policy.toml`, management call
  (E:src/sandbox/bailey.rs:573).
- Exit code must be 0. Otherwise the launch fails with "could not trust
  the generated policy at <policy>: <stderr trimmed>"
  (E:src/sandbox/bailey.rs:574-579). stdout is ignored.
- Real bailey prints any hooks, then `trusted <path>`, and exits 0
  (B:crates/bailey/src/cli.rs:1440-1471). It refuses (exit 1) a file or a
  parent directory that another user can write
  (B:crates/bailey/src/trust.rs:281-319, B:crates/bailey/src/cli.rs:386-397).
- Real bailey records trust in `$XDG_DATA_HOME/bailey/trusted.toml`, else
  `$HOME/.local/share/bailey/trusted.toml`
  (B:crates/bailey/src/trust.rs:21, 340-363). The trust call runs with the
  daemon's environment, but the session run sees `XDG_DATA_HOME=/state/kage`
  (section 5). INFERRED: the session run cannot see the trust record. This
  does not matter to real bailey, because an explicit `--config` needs no
  record (B:crates/bailey/src/config.rs:403-406, 466-473). A replacement
  must not make the session run depend on the record either. The gVisor
  shim keys its store on the passwd home for this reason (research
  poc/bailey-gvisor/bailey:64-77).

### 4.2 The confined check and `not applying`

- argv, management call, no cwd (E:src/sandbox/bailey.rs:741-753):
  `bailey run --isolate --config <policy> --profile <profile> --quiet -- true`
- `<profile>` is `untrusted` when `sandbox.network` is `none`, else
  `ai-agent` (E:src/sandbox/bailey.rs:736-740; E:src/sandbox/policy.rs:16-20).
- If stderr contains the substring `not applying`, the launch fails
  ("bailey declined to apply the generated policy ...")
  (E:src/sandbox/bailey.rs:35-36, 754-760). This is checked before the
  exit code.
- Else the exit code must be 0 ("the confined check command failed")
  (E:src/sandbox/bailey.rs:761-766).
- stdout is ignored.
- Real bailey prints `bailey: not applying `<path>`: <reason>` on stderr
  only for a rejected discovered `bailey.toml` or a rejected global config
  (B:crates/bailey/src/cli.rs:1489-1503, B:crates/bailey/src/config.rs:36,
  407-464). The explicit `--config` is never rejected
  (B:crates/bailey/src/config.rs:466-473).
- Contract: never print `not applying` on stderr unless the policy will
  not be applied. INFERRED: a replacement that cannot apply the policy
  should exit non-zero; either path makes errand refuse the launch.

### 4.3 The session run

argv built by E:src/sandbox/bailey.rs:253-292:

```
bailey run --isolate
           [--egress-proxy 169.254.169.1:<port> | --proxy-net]
           --config <stateDir>/<id>/policy.toml
           --profile <ai-agent|untrusted>
           --
           kage rpc --yolo [-m <provider>/<model>] [--system <text>]
```

- `--egress-proxy 169.254.169.1:<port>` when `sandbox.egress.mode` is
  `proxy` and a broker port exists (263-269). It does not depend on
  `sandbox.network`. INFERRED: a session with `network = none` under proxy
  mode still gets `--egress-proxy`, with profile `untrusted`.
- Else `--proxy-net` when `sandbox.hideHostAddress` is true (270-272).
- No `--quiet` on this call, so real bailey writes its run summary to
  stderr (B:crates/bailey/src/backend/enforce.rs:86-100, 799-803).
- The agent command (E:src/sandbox/backend.rs:170-200): `kage rpc --yolo`;
  `-m <provider>/<model>` when a model is set, with any `:level` suffix
  removed (182-188); `--system "<KAGE_ROLE>\n\n<memory>"` when the memory
  file has text (191-198), read from the system prompt path at launch
  (E:src/sandbox/bailey.rs:283-290).
- `kage` is a bare name. Real bailey resolves it on its own PATH, which is
  the daemon's (B:crates/bailey/src/cli.rs:1662-1665, 1720-1734).
- cwd is the project's host path (E:src/sandbox/bailey.rs:582-584, 600).
  errand expects the target to start in the place the project is mounted,
  `/workspace` (comment at 582-584). Real bailey maps a granted invocation
  directory to its relocated path (B:crates/bailey/src/backend/world.rs:121-137).
- The launcher's pid is its process group id (E:src/sandbox/spawn.rs:92-114,
  151-158).
- Exit code: errand reads it only as the session's end status
  (E:src/agent/client.rs:469-484). Real bailey exits with the target's
  code, or 128+signal (B:crates/bailey/src/backend/isolation.rs:284-299,
  B:crates/bailey/src/cli.rs:954-966).

## 5. Environment

### 5.1 Management calls

The daemon's full environment, unchanged (E:src/sandbox/bailey.rs:47-57).
INFERRED: this includes everything the daemon holds, which errand says
includes the chat token (E:src/sandbox/bailey.rs:74-76).

### 5.2 The session run's own environment

Built by `session_environment` (E:src/sandbox/bailey.rs:77-107), then set on
a cleared process (E:src/sandbox/spawn.rs:91-97). Applied in this order,
later wins:

1. From the daemon, if set: `PATH`, `LANG`, `LC_ALL`, `TERM`,
   `BAILEY_CGROUP_ROOT` (E:src/sandbox/bailey.rs:38-44, 83-87).
2. `HOME = <stateDir>/<id>/home`, a host path (E:src/sandbox/bailey.rs:88-91,
   589-593).
3. `XDG_CONFIG_HOME`, `XDG_DATA_HOME`: the daemon's value if absolute,
   else `<daemon HOME>/.config` and `<daemon HOME>/.local/share`, else
   unset (E:src/sandbox/bailey.rs:92-102, 109-124).
4. Every launch variable (E:src/sandbox/bailey.rs:103-105). The launch
   variables are (E:src/session/session.rs:2363-2404):
   - each provider's key variable (E:src/session/session.rs:2369-2376),
     after the broker swap of section 9 (E:src/sandbox/bailey.rs:538-545);
   - with GitHub configured: `GH_TOKEN` as the real token, and
     `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_NAME`,
     `GIT_COMMITTER_EMAIL` (E:src/session/session.rs:2377-2386;
     E:src/session/github.rs:20, 112-119);
   - `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`,
     `XDG_STATE_HOME`, each `/state/kage` (E:src/session/session.rs:2388-2402;
     E:src/sandbox/backend.rs:150-157).

Net effect: the session run sees `XDG_CONFIG_HOME` and `XDG_DATA_HOME` as
`/state/kage`, a path that exists inside the sandbox, not on the host.
Step 3 is always overridden (README.md finding, corrected citation in
citations.md).

Nothing else crosses: no `USER`, `LOGNAME`, `SHELL`, `XDG_RUNTIME_DIR`,
`DBUS_*`, or proxy variable of the daemon.

INFERRED for a replacement: the session run's `HOME` and `XDG_*` are
sandbox-side paths or session paths, not the operator's. Read operator
state (trust, global config) from somewhere the caller's environment
cannot move, such as the passwd entry.

### 5.3 The agent's environment inside

errand states it in the policy's `[env]` (section 6):

- `pass`: the launch variable names, sorted (E:src/sandbox/policy.rs:240-248).
  Their values come from the run's own environment (5.2).
- `set`: `PATH`, `HOME = /state/home`, then each operator variable from
  `sandbox.env` and the broker variables, except names already in `pass`
  (E:src/sandbox/policy.rs:249-260; E:src/sandbox/bailey.rs:358-381, 566).

Real bailey builds the target environment from empty
(B:crates/bailey/src/backend/world.rs:229-327):

1. `PATH=/usr/local/bin:/usr/bin:/bin`, `HOME=<inside home>`,
   `BAILEY_SANDBOX=1`, `BAILEY_SANDBOX_NET`, `TMPDIR=/tmp` with a private
   /tmp, and from the caller if set: `TERM`, `LANG`, `LC_ALL`,
   `LC_CTYPE`, `LC_TIME`, `LC_NUMERIC`, `LC_COLLATE`, `LC_MESSAGES`,
   `USER`, `LOGNAME`, `SHELL`, `TZ` (world.rs:20-37, 236-257);
2. caller variables named in `pass` (316-320);
3. `set` (321-323);
4. `deny` removes (325). errand writes no `deny`.

Contract: the agent must see every `pass` name with the value the run
received, and every `set` value, with `set` over the base. INFERRED:
errand does not need the base set beyond `PATH` and `HOME`, which it sets
itself.

## 6. The policy TOML

### 6.1 Where, and how long

- Session: `<stateDir>/<id>/policy.toml` (E:src/sandbox/policy.rs:22-23,
  113-119; E:src/session/manager.rs:567-570). The path is passed fresh on
  every call.
- Rewritten at every launch, including a revived session; never read
  back as a policy (E:src/sandbox/policy.rs:162-171).
- errand never deletes it. Stop only runs `untrust` on it
  (E:src/sandbox/mod.rs:185-187).
- `errand threads revive` reads it to recover the project path from the
  `/workspace` grant, and checks it against the project root
  (E:src/main.rs:125-157). INFERRED: a replacement must not move or delete
  this file.
- The state directory is granted to the session, so the session can
  rewrite the file (E:src/sandbox/policy.rs:164-167). INFERRED: a
  replacement should read the file once, when `run` starts.
- Probe: `<stateDir>/probe/shape.toml`, rewritten at each daemon start
  (section 3.2).
- Values are rendered with JSON string quoting (E:src/sandbox/policy.rs:99-111).
  No secret is written into the file (E:src/sandbox/policy.rs:1-6).

### 6.2 Schema errand writes

From `policy_contents` (E:src/sandbox/policy.rs:175-325):

| key | value errand writes | citation |
| --- | --- | --- |
| `[filesystem].reset` | `true`: clear the profile's filesystem and device grants | policy.rs:270-277 |
| `[filesystem].read` | the 17 system paths; `{ path = <stateDir>/resolv.conf, at = "/etc/resolv.conf" }`; `{ path = <project>, at = "/workspace" }`; `{ path = <stateDir>/<id>, at = "/state" }`; the agent's runtime dirs; `sandbox.policyExtra.read` | policy.rs:28-61, 193-200 |
| `[filesystem].write` | project at `/workspace`; state at `/state`; `policyExtra.write` | policy.rs:262-268 |
| `[filesystem].execute` | `/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`; `/state/home/bin`; the runtime dirs; `policyExtra.execute`; then project at `/workspace` | policy.rs:63-64, 224-238; backend.rs:27-32 |
| `[[device]]` | six blocks of `path`, `access`: `/dev/null` rw, `/dev/zero` r, `/dev/full` rw, `/dev/urandom` r, `/dev/random` r, `/dev/tty` rw | policy.rs:66-81, 121-131 |
| `[env].pass` | sorted launch variable names | policy.rs:240-248 |
| `[env].set` | inline table: `PATH`, `HOME`, operator and broker variables | policy.rs:249-260, 288-290 |
| `[resources].file_max` | `sandbox.fileMax`, default `"1g"`; meant as an rlimit | policy.rs:292-296; schema.rs:624-625 |
| `[resources].tmp_size` | `sandbox.tmpSize`, default `"512m"` | policy.rs:297; schema.rs:626-628 |
| `[resources].shm_size` | `sandbox.shmSize`, default `"256m"` | policy.rs:298; schema.rs:629-630 |
| `[resources].tmp_dir` | only with `sandbox.diskTmp`: `<stateDir>/<id>/tmp`, bound at `/tmp` | policy.rs:300-308 |
| `[network].egress_allow` | only when `sandbox.network` is not `none`: `{ host = "*", port = <p> }` per port of `sandbox.egressPorts`, default 443 | policy.rs:310-323; schema.rs:606-607 |

`PATH` in `set` is `/state/home/bin`, then the runtime dirs, then
`sandbox.pathExtra`, then `/usr/local/bin:/usr/bin:/bin`. Entries are
not de-duplicated (E:src/sandbox/policy.rs:202-210, 253;
E:src/sandbox/runtime.rs:17).

Semantics a replacement must give these keys (from errand's comments and
bailey's schema):

- A grant is a plain path, or `{ path, at }`: the host path appears at
  `at` (B:crates/bailey/src/config.rs:274-299). One host directory can be
  placed at two `at` paths (the probe does this, section 3.2).
- read, write and execute are separate rights. Write does not imply read;
  read does not imply execute (E:src/sandbox/policy.rs:189-192, 212-216).
- `/state/home/bin` in `execute` names a path inside the `/state` grant,
  not a host path (E:src/sandbox/backend.rs:27-32). INFERRED.
- The runtime dirs stay at their host paths inside
  (E:src/sandbox/policy.rs:197-199). INFERRED: a replacement that runs its
  own guest kage may ignore them, but it still receives them.
- Devices are named because `reset` clears the profile's list
  (E:src/sandbox/policy.rs:66-73).
- `/etc/resolv.conf` is errand's file with public resolvers, not the host's
  (E:src/sandbox/policy.rs:83-97).

Real bailey also accepts keys errand does not write: `filesystem.deny`,
`filesystem.read_only`, `network.egress`, `network.bind_ports`,
`resources.memory`, `pids_max`, `cpu_percent`, `env.reset`, `env.deny`,
`home`, `[[fuse]]`, `self_fuse`, `[[socket]]`, `[[forward]]`, `[hooks]`,
`applies_to` (B:crates/bailey/src/config.rs:193-221, 261-272, 301-352).
INFERRED: errand does not depend on them.

### 6.3 Example

errand's output for: config `stateDir = /srv/errand/state`, session id
`abc123`, project `/srv/errand/projects/demo`, `kage` at
`/usr/local/bin/kage` (not a link), defaults otherwise, one provider
whose key variable is `OPENAI_API_KEY`, no GitHub, broker on port 41234.
Built by hand from E:src/sandbox/policy.rs:175-325 and
E:src/sandbox/bailey.rs:358-381; not captured from a run.

```toml
# Generated per session by errand. Do not edit.
[filesystem]
reset = true
read = ["/usr", "/lib", "/lib64", "/bin", "/sbin", "/proc", "/etc/ld.so.cache", "/etc/ld.so.conf", "/etc/ld.so.conf.d", "/etc/ssl", "/etc/ca-certificates.conf", "/etc/ca-certificates", "/var/lib/ca-certificates", "/etc/pki", "/etc/nsswitch.conf", "/etc/services", "/etc/protocols", { path = "/srv/errand/state/resolv.conf", at = "/etc/resolv.conf" }, { path = "/srv/errand/projects/demo", at = "/workspace" }, { path = "/srv/errand/state/abc123", at = "/state" }, "/usr/local/bin"]
write = [{ path = "/srv/errand/projects/demo", at = "/workspace" }, { path = "/srv/errand/state/abc123", at = "/state" }]
execute = ["/usr", "/lib", "/lib64", "/bin", "/sbin", "/state/home/bin", "/usr/local/bin", { path = "/srv/errand/projects/demo", at = "/workspace" }]
[[device]]
path = "/dev/null"
access = "rw"
[[device]]
path = "/dev/zero"
access = "r"
[[device]]
path = "/dev/full"
access = "rw"
[[device]]
path = "/dev/urandom"
access = "r"
[[device]]
path = "/dev/random"
access = "r"
[[device]]
path = "/dev/tty"
access = "rw"

[env]
pass = ["OPENAI_API_KEY", "XDG_CACHE_HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME"]
set = { PATH = "/state/home/bin:/usr/local/bin:/usr/local/bin:/usr/bin:/bin", HOME = "/state/home", HTTPS_PROXY = "http://169.254.169.1:41234", HTTP_PROXY = "http://169.254.169.1:41234", NODE_USE_ENV_PROXY = "1", NO_PROXY = "169.254.169.1", http_proxy = "http://169.254.169.1:41234", https_proxy = "http://169.254.169.1:41234", no_proxy = "169.254.169.1" }

[resources]
file_max = "1g"
tmp_size = "512m"
shm_size = "256m"

[network]
egress_allow = [{ host = "*", port = 443 }]
```

The `OPENAI_API_KEY` value in the run's environment is the broker's nonce,
not the key (section 9).

### 6.4 What the agent finds inside

| inside path | host side | written by | citation |
| --- | --- | --- | --- |
| `/workspace` | the project | errand and agent | E:src/sandbox/backend.rs:13-19 |
| `/state` | `<stateDir>/<id>` | errand and agent | E:src/sandbox/backend.rs:21-22 |
| `/state/home` | `<stateDir>/<id>/home` | agent's HOME | E:src/sandbox/backend.rs:24-25 |
| `/state/home/bin` | wrappers, first on PATH | errand | E:src/sandbox/backend.rs:27-32 |
| `/state/kage/kage/config.toml` | kage's provider config | errand | E:src/sandbox/backend.rs:150-161; E:src/sandbox/agent_config.rs:450-468 |

errand reads and writes files under the project while a session runs,
translating `/workspace/...` to the host path (E:src/sandbox/mod.rs:115-127).
INFERRED: `/workspace` and `/state` must be live two-way views of the host
directories, not copies.

## 7. ACP over stdio

- The agent, `kage rpc --yolo ...`, speaks ACP. The launcher only carries
  the bytes (E:src/sandbox/backend.rs:170-181; E:src/sandbox/mod.rs:129-135).
  Real bailey gives the target its own stdin, stdout and stderr: it sets
  no `Stdio` on the target command (B:crates/bailey/src/backend/isolation.rs:265-282).
- The launcher must write nothing to stdout itself. Real bailey writes its
  messages to stderr (B:crates/bailey/src/backend/enforce.rs:86-100;
  B:crates/bailey/src/cli.rs:392-394) and uses stdout only for `--json`
  (B:crates/bailey/src/backend/enforce.rs:801), which errand never passes.
- Framing: JSON-RPC 2.0, one record per line, LF is the only delimiter,
  a CR before LF is dropped, split on bytes, at most 8 MiB per record
  (E:src/agent/framing.rs:1-12, 31-35, 52-78). errand ends each frame it
  writes with `\n` (E:src/agent/client.rs:924, 965).
- Start (E:src/agent/client.rs:510-563; E:src/session/session.rs:1102-1148):
  1. errand sends `initialize` with `protocolVersion: 1`,
     `clientCapabilities: {}` (E:src/agent/protocol.rs:133-145). An
     `error` in the answer fails the start.
  2. errand sends `session/new` with `cwd: "/workspace"` and
     `mcpServers: []` (E:src/agent/protocol.rs:147-155;
     E:src/session/session.rs:1112). If `<stateDir>/<id>/kage-session`
     exists, it sends `session/resume` with that id first, and falls back
     to `session/new` on an error (E:src/agent/protocol.rs:157-165;
     E:src/agent/client.rs:524-543; E:src/session/session.rs:2406-2413).
  3. The `session/new` answer must carry `result.sessionId`, else "the
     agent opened no session" (E:src/agent/client.rs:548-556).
  4. Each request waits up to `timeouts.startupMs`, default 60000 ms
     (E:src/session/session.rs:1126-1135; E:src/config/schema.rs:683-684).
- stderr: read in chunks of up to 16 KiB; each chunk is logged as an
  `agent stderr` warning and kept as the last words in a failure report
  (E:src/agent/client.rs:996-1024; E:src/session/session.rs:1137-1147).
- End: errand reads stdout to EOF, waits for stderr EOF, then reads the
  launcher's exit code (E:src/agent/client.rs:469-484). INFERRED: when the
  agent exits, the launcher must close both pipes and exit.

## 8. Stopping a session

- Called once when the session ends, for any reason
  (E:src/session/session.rs:2568-2611, call at 2599-2601).
- `BaileyStop::stop` (E:src/sandbox/mod.rs:154-189):
  1. A second call returns at once (155-160).
  2. SIGTERM to the process group `-pid`; if that fails, to the pid
     (162; E:src/sandbox/spawn.rs:160-171).
  3. Wait for the launcher process to exit, up to `grace_ms` (163-167).
     `grace_ms` is `sandbox.gracePeriodMs` (E:src/sandbox/bailey.rs:623),
     default 10000 ms (E:src/config/schema.rs:637-638).
  4. On timeout: SIGKILL to the group, and a warning "confined process did
     not stop and was killed" (170-183).
  5. `bailey untrust <policy>`, management call, no cwd, result ignored
     (185-187).
- After the first signal, errand refuses further writes to stdin
  (E:src/sandbox/spawn.rs:35-44, 166-167).
- errand waits for the launcher's own exit, not for the group
  (E:src/sandbox/mod.rs:163-166; E:src/sandbox/spawn.rs:130-135).
- No cleanup follows. For bailey, `list_orphans` returns nothing and
  `remove_orphans` removes nothing, because "killing the launcher removes
  every process the agent started" (E:src/sandbox/bailey.rs:629-649).
- Real bailey: `untrust` of a file with no record exits 1 with "`<path>`
  was not trusted" (B:crates/bailey/src/cli.rs:1473-1482); errand ignores it.
- Real bailey: the target runs in a new PID namespace whose PID 1 is a
  bailey process that waits for it (B:crates/bailey/src/backend/isolation.rs:193-245,
  265-299). A search of B:crates/bailey/src found no `setsid`, `setpgid`,
  `sigaction` or `SIGTERM`. INFERRED: every bailey process and the target
  stay in errand's process group; SIGTERM reaches kage directly; the
  namespace's PID 1 has no handler, so the kernel does not deliver SIGTERM
  from outside to it; SIGKILL to the group kills it and so the namespace.

Contract for a replacement (INFERRED from the points above):

- SIGTERM to the launcher's group must end the session within the grace
  period.
- The launcher must not exit before the sandbox is gone: errand treats its
  exit as the end, runs `untrust`, and never looks again.
- SIGKILL of the launcher must also end the sandbox. Processes a daemon
  starts outside the group (for example, an Incus instance) need their own
  link to the launcher's life, such as a watchdog on a pipe or pidfd.

## 9. The broker and 169.254.169.1

- Only when `sandbox.egress.mode` is `proxy` (E:src/serve.rs:661-664). It
  is the default (E:src/config/schema.rs:608-609).
- The broker listens on host `127.0.0.1`, on a port the kernel picks
  (E:src/serve.rs:694-696; E:src/sandbox/broker/server.rs:53-69). One
  broker serves every session of the daemon (E:src/serve.rs:787-793,
  806-812). INFERRED: the port changes at each daemon start.
- The sandbox reaches it as `169.254.169.1:<port>`. The address is a
  constant, link-local, deliberately not the metadata address
  (E:src/sandbox/bailey.rs:172-176; E:src/serve.rs:694-695).
- How the sandbox is told:
  - argv `--egress-proxy 169.254.169.1:<port>` (E:src/sandbox/bailey.rs:248-251,
    263-269);
  - `[env].set`: `HTTPS_PROXY`, `https_proxy`, `HTTP_PROXY`, `http_proxy`
    = `http://169.254.169.1:<port>`; `NO_PROXY`, `no_proxy` =
    `169.254.169.1`; `NODE_USE_ENV_PROXY = "1"`
    (E:src/sandbox/bailey.rs:243-246, 351-381);
  - kage's config: each brokered provider's `base_url` is
    `http://169.254.169.1:<port>/provider/<name>`
    (E:src/sandbox/bailey.rs:178-192, 332-349, 531-537;
    E:src/sandbox/agent_config.rs:253-258, 300-317);
  - each brokered provider's key variable holds a nonce; a provider with
    no route loses its variable (E:src/sandbox/bailey.rs:194-227, 538-541).
- What the broker answers on its one port (E:src/sandbox/broker/server.rs:147-200):
  `CONNECT host:port` tunnels and absolute-form `http://` requests, if the
  host is on the allowlist and the port is in `sandbox.egressPorts`, else
  403; any other request is served as the provider endpoint when a route
  exists, else 400. The default allowlist is `["*"]`
  (E:src/config/schema.rs:610-611).
- What real bailey does with `--egress-proxy ADDR:PORT`:
  - implies `--proxy-net` (B:crates/bailey/src/cli.rs:229-237, 759-760);
  - runs `pasta` with `--map-host-loopback ADDR`, no port forwarding, a
    private address, `--no-map-gw` (B:crates/bailey/src/cli.rs:711-751);
  - installs nft rules in the new namespace: drop by default, drop
    127.0.0.0/8 and ::1, accept TCP to ADDR:PORT and established traffic
    (B:crates/bailey/src/cli.rs:620-644, 1062-1078);
  - refuses the run if pasta, user namespaces or nft are missing, or the
    rule fails (B:crates/bailey/src/cli.rs:771-802, 641-643, 1066-1071).
- Contract for a replacement: inside, TCP to `169.254.169.1:<port>` must
  arrive at host `127.0.0.1:<port>`, and every other destination should be
  dropped. INFERRED from bailey's rules and the broker's purpose
  (E:src/sandbox/broker/mod.rs:1-9).
- Under `egress.mode = open`, no broker, no `--egress-proxy`, no proxy
  variables, and real keys in the variables (E:src/serve.rs:662-664;
  E:src/sandbox/bailey.rs:358-362, 538-541).

## 10. Checklist for a replacement

| # | must | citation |
| --- | --- | --- |
| 1 | be an executable named `bailey` on the daemon's PATH | E:src/sandbox/bailey.rs:49, 602 |
| 2 | `doctor`: exit 0; print `landlock: ABI <n>`; not print `user namespaces: ... no`; not print `cgroup delegation: ... no` under default config | E:src/sandbox/bailey.rs:130-170, 388-403; E:src/daemon.rs:812-816 |
| 3 | `trust <file>`: exit 0 | E:src/sandbox/bailey.rs:573-579 |
| 4 | `untrust <file>`: any exit code; must not hang | E:src/sandbox/bailey.rs:722; E:src/sandbox/mod.rs:185-187 |
| 5 | `run --config F --quiet -- true` with cwd set: exit 0 for errand's probe policy | E:src/sandbox/bailey.rs:715-723 |
| 6 | `run --isolate --config F --profile P --quiet -- true`: exit 0, no `not applying` on stderr | E:src/sandbox/bailey.rs:741-767 |
| 7 | accept `--isolate`, `--egress-proxy ADDR:PORT`, `--proxy-net`, `--config F`, `--profile ai-agent` or `untrusted`, `--quiet`, `--`, then a command | E:src/sandbox/bailey.rs:257-292, 718, 742-752 |
| 8 | accept every key of section 6.2, including `{ path, at }` grants | E:src/sandbox/policy.rs:175-325 |
| 9 | session run: start `kage rpc ...` in `/workspace`, with stdin and stdout passed through, own output on stderr only | sections 4.3 and 7 |
| 10 | give the agent the `pass` names from the run's environment and the `set` values | section 5.3 |
| 11 | map `169.254.169.1:<port>` to host `127.0.0.1:<port>`; drop the rest | section 9 |
| 12 | end everything on SIGTERM to the group within the grace period; exit only when the sandbox is gone; survive nothing on SIGKILL | section 8 |
| 13 | not depend on `HOME` or `XDG_*` of the session run for its own state | sections 4.1 and 5.2 |
| 14 | finish every management call: errand sets no timeout | E:src/sandbox/bailey.rs:47-65 |

errand also requires `kage` on the daemon's PATH, whatever the replacement
runs inside (E:src/sandbox/bailey.rs:419-432, 515-521). Its startup report
then says sessions are "confined host processes using the host's own
tools" (E:src/sandbox/bailey.rs:434-456). INFERRED: under a VM or container
replacement that note is false, and errand has no way to say otherwise.

## 11. The podman backend, for comparison

- Probe (E:src/sandbox/podman.rs:200-266): `podman info --format
  {{.Host.Security.Rootless}}` must exit 0 and print `true`; then `podman
  image exists <sandbox.image>` must exit 0. No gaps are reported.
- Launch (E:src/sandbox/podman.rs:268-308, args at 44-134):

  ```
  podman run --interactive --rm --name errand-<id>
    --label errand.system=true --label errand.session=<id>
    --userns=keep-id --read-only
    --volume <project>:/workspace:rw,Z --volume <stateDir>/<id>:/state:rw,Z
    --workdir /workspace --cap-drop=ALL --security-opt no-new-privileges
    --network=<pasta:--map-host-loopback,none,--map-guest-addr,none | none>
    --memory <memory> --cpus <cpus>
    (--tmpfs /tmp:size=<bytes>,mode=1777 | --volume <stateDir>/<id>/tmp:/tmp:rw,Z)
    --shm-size <bytes> --pids-limit <n> --ulimit fsize=<bytes>
    --env HOME=/state/home [--env K=V for sandbox.env] [--env K=V for each launch variable]
    <image> kage rpc --yolo [...]
  ```

- Run by `spawn_agent("podman", args, None, None)`: own process group, and
  an empty environment, since `env_clear` runs and no env is given
  (E:src/sandbox/podman.rs:287; E:src/sandbox/spawn.rs:91-97). INFERRED:
  podman then runs with no `HOME` or `XDG_RUNTIME_DIR`.
- No broker: podman gets no brokering (E:src/daemon.rs:82-87). Credentials
  cross as `--env K=V` on podman's argv (E:src/sandbox/podman.rs:119-122).
  INFERRED: they are readable in the host's process table.
- Stop (E:src/sandbox/mod.rs:192-241): `podman stop --time
  <max(grace_ms/1000, 1)> errand-<id>`; on exit 0, SIGKILL the launcher's
  group; else `podman rm --force errand-<id>`, then SIGKILL, and a warning.
- Orphans: `podman ps --all --filter label=errand.system=true --format
  {{.Names}}`, then `podman rm --force <name>` for each
  (E:src/sandbox/podman.rs:310-351). This backend has the cleanup path the
  bailey backend lacks.
