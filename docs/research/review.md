# Review of the research record

The research branch of agent-box ended with a review cut short. Its
`next-session.md` section 0 lists five open items. This page closes them.

| item | result |
| --- | --- |
| 1. Check every source-line citation against the pins | Done below: 19 checked, 11 correct, 5 with wrong lines (corrected), 3 that the code does not support (one claim, cited three times). |
| 2. Write down the bailey CLI contract | [../bailey-contract.md](../bailey-contract.md), from errand's source at its pin. |
| 3. Corrections to the draft prompt | Not needed as text: the prompt was carried out in this session. Its four corrections are reflected in [engines.md](engines.md) (sections 1 and 2) and in the edge case table. |
| 4. Edge cases missing from table 1.2 | All in [../edge-cases.md](../edge-cases.md): E-13 (the proxy port moved), E-14 (`apt-get update` exits 0), E-15 (the proxy CA path), and the new ones this session found. |
| 5. Read section 13 for the current answer | Done; [engines.md](engines.md) section 6 confirms it, and corrects its start-up figure for per-guest uid ranges. |

The research also stated, in three files, that errand gives a session the
real provider key when no base URL is known. At errand 4a8011cd3668 that
is not what the bailey backend does: the session gets no key at all. The
details are in "The serve.rs claim" below. This matters for errand-box: its
shim passes the agent exactly the variables errand's policy names, so no
real provider key reaches a guest under `egress.mode = proxy`. A GitHub
token still does (`GH_TOKEN`, errand's own choice), which
[../../SECURITY.md](../../SECURITY.md) records.

## Citation check

Checked on 2026-10-23 against these pins:

| repo | pin | how checked |
| --- | --- | --- |
| QaidVoid/errand | 4a8011cd3668 | HEAD equals pin |
| QaidVoid/bailey | 98d9b455adc6 | HEAD equals pin |
| QaidVoid/kage | cce8b9a6dca4 | HEAD equals pin (no citation points into it) |
| nestybox/sysbox | v0.7.1 = 82881aaaf2ce | cloned with git; tag resolves to this commit |
| sysbox-mgr | 2d45af01bcfd | submodule status at v0.7.1 |
| sysbox-runc | 081856cc5d17 | submodule status at v0.7.1 |
| sysbox-fs | c3d2ebc65102 | submodule status at v0.7.1 |
| sysbox-libs | cacac1939552 | submodule status at v0.7.1 (the research names no commit for it) |

The three submodule commits the research cites (engines-shortlist.md:57-58)
are the ones v0.7.1 checks out.

## Summary

| count | what |
| --- | --- |
| 19 | citations checked (17 of the form `path:N` or `path:N-M`, plus the 2 line claims in next-session.md:40-41) |
| 11 | correct |
| 5 | wrong line range, corrected |
| 3 | claim not supported by the cited code: remove the citation and rewrite the sentence |

All three unsupported citations are the same one, `src/serve.rs:727-736`,
used in three files. See "The serve.rs claim" below.

Method: every file in the tree outside `corpus/` was searched for
`<path>.<ext>:<N>`, `line N`, `lines N`, `#LN` and `:N-M`, and the hits
were read by hand. `corpus/` holds no source citations. Each cited file
was opened at the pin, with line numbers taken from the file itself
(`awk` / `nl -ba` / Read), not from command output with other lines
above it.

## Table

| research file:line | citation as written | verdict | corrected citation | evidence quote |
| --- | --- | --- | --- | --- |
| next-session.md:40 | `NOT_APPLYING` "is at line 36, not 56" (errand `src/sandbox/bailey.rs`) | correct | errand@4a8011cd3668:src/sandbox/bailey.rs:35-36 | line 36: `const NOT_APPLYING: &str = "not applying";` |
| next-session.md:40-41 | "session_environment 97-127" is "probably 77-107" | correct (the proposed fix is right) | errand@4a8011cd3668:src/sandbox/bailey.rs:77-107 | line 77: `pub fn session_environment(`; line 106: `env`; line 107: `}` |
| next-session.md:161 | `src/serve.rs:727-736` for "Without it, errand gives the real key to the session" | claim not supported; remove | errand@4a8011cd3668:src/serve.rs:728-732 (the warning only); errand@4a8011cd3668:src/sandbox/bailey.rs:209-227 (what happens) | serve.rs:730: `"the model store does not say where the provider is, so the credential is given to the session"`; bailey.rs:223: `None => env.remove(name),` |
| README.md:89 | `src/session/session.rs:2378-2382` (GH_TOKEN crosses as the real token) | correct | errand@4a8011cd3668:src/session/session.rs:2377-2386, with src/session/github.rs:20 | session.rs:2382: `env.insert(TOKEN_VARIABLE.to_owned(), github.token.clone());`; github.rs:20: `pub const TOKEN_VARIABLE: &str = "GH_TOKEN";` |
| README.md:91 | `src/serve.rs:727-736` for "With no known base URL, errand gives the session the real provider key and logs only a warning" | claim not supported; remove | as next-session.md:161 | as next-session.md:161 |
| README.md:93 | `src/sandbox/bailey.rs:97-127` (session_environment lets the launch env's XDG_* override the operator's dirs) | wrong line; claim holds | errand@4a8011cd3668:src/sandbox/bailey.rs:77-107 (key lines 97-105) | lines 97-102 set `XDG_CONFIG_HOME` and `XDG_DATA_HOME` from `operator_dir`; lines 103-105 `for (name, value) in launch_env { env.insert(name.clone(), value.clone()); }` run after them. Lines 109-124 are `operator_dir`; 126-127 are the doc comment of `parse_doctor`. |
| README.md:95 | bailey `crates/bailey/src/config.rs:404-406` | correct | bailey@98d9b455adc6:crates/bailey/src/config.rs:403-406 | lines 405-406: "The global config and an explicit `--config` are the user's own, and need no record." |
| README.md:95 | bailey `crates/bailey/src/config.rs:466-473` | correct | bailey@98d9b455adc6:crates/bailey/src/config.rs:466-473 | lines 469-471: `origin: Origin::Explicit,` `rejected: None,` `verified: None,` |
| engines-shortlist.md:68 | sysbox-libs `linuxUtils/linux.go:247-271` (`KernelModSupported` reads only /proc/modules) | correct (the function closes at 272) | sysbox-libs@cacac1939552:linuxUtils/linux.go:247-272; caller sysbox-mgr@2d45af01bcfd:mgr.go:262-264,287-289 | linux.go:253: `filename := "/proc/modules"`; 255-257: `f, err := os.Open(filename)` / `return false, err`; mgr.go:289: `"binfmt_misc kernel module check failed: %v"` |
| engines-shortlist.md:83 | `libcontainer/nsenter/nsexec.c:744-752` (sysbox-runc writes -999 unconditionally, citing issue #381) | correct | sysbox-runc@081856cc5d17:libcontainer/nsenter/nsexec.c:744-752; the failing write is at 352-358 | 750: `sysbox issue #381.`; 752: `update_oom_score_adj("-999", 4);`; 358: `bail("failed to update /proc/self/oom_score_adj");` |
| engines-shortlist.md:101 | `nsenter/event.go:36` (sysbox-fs links sysbox-runc's nsenter) | correct | sysbox-fs@c3d2ebc65102:nsenter/event.go:36, with go.mod:88 | event.go:36: `_ "github.com/nestybox/sysbox-runc/libcontainer/nsenter"`; go.mod:88: `github.com/nestybox/sysbox-runc => ../sysbox-runc` |
| experiments/50-errand-probe.sh:19 | `src/serve.rs:727-736` ("hands the session the real key") | claim not supported; remove | as next-session.md:161 | as next-session.md:161 |
| experiments/lib/policy.sh:2 | `src/sandbox/policy.rs:239-327, policy_contents` | wrong line | errand@4a8011cd3668:src/sandbox/policy.rs:175-325 | 175: `pub fn policy_contents(options: &PolicyOptions) -> String {`; 324: `format!("{}\n", lines.join("\n"))`; 326-327 are a blank line and `#[cfg(test)]` |
| experiments/40-kage-acp.sh:29 | `src/sandbox/runtime.rs:86-108` (the agent's directory is granted, and put on PATH) | wrong line (minor) | errand@4a8011cd3668:src/sandbox/runtime.rs:83-103 | 84: `let launcher = lookup("kage")?;`; 88-89: `add(&mut runtime.path_entries, Some(parent_of(&launcher)));` and the same for `read_paths`. Lines 104-108 are the start of `parent_of`. |
| experiments/40-kage-acp.sh:29 | `policy.rs:180-226` (read and execute grant, and PATH) | wrong line: misses the execute grant | errand@4a8011cd3668:src/sandbox/policy.rs:197-237 | 199: `read.extend(options.runtime.read_paths.iter()...)`; 206: `path.extend(options.runtime.path_entries.iter().cloned());`; 229-230: `execute_set.extend(options.runtime.path_entries...)` and `...read_paths...`. The cited range ends at 226. |
| experiments/40-kage-acp.sh:34 | `src/sandbox/agent_config.rs:310-326` (config for a brokered custom provider) | correct | errand@4a8011cd3668:src/sandbox/agent_config.rs:310-326 | 310: `"\n[providers.custom.{}]"`; 311: `"kind = {}"`; 317: `"api_key_env = {}"`; 322: `"\n[[providers.custom.{}.models]]\n{model}"` |
| experiments/70-engines-setup.sh:4 | `src/sandbox/podman.rs:62-98` (the flags errand's podman backend uses) | correct (wider than needed) | errand@4a8011cd3668:src/sandbox/podman.rs:46-50,62-73 | 62-63: `"--userns=keep-id",` `"--read-only",`; 70-72: `"--cap-drop=ALL",` `"--security-opt",` `"no-new-privileges",`; 73: `&format!("--network={network}"),` with `"none"` at 47 |
| poc/bailey-gvisor/bailey:67 | `src/sandbox/bailey.rs:97-127` (errand sets HOME to the private home and XDG_* to /state/kage) | wrong line; claim holds | errand@4a8011cd3668:src/sandbox/bailey.rs:77-107, with the call at 589-593 | 91: `env.insert("HOME".to_owned(), home.to_owned());`; 592: `&format!("{}/home", launch.state_dir),`; 103-105 apply the launch env last |
| poc/bailey-gvisor/bailey:67 | `session.rs:2392-2402` | correct | errand@4a8011cd3668:src/session/session.rs:2392-2402, with src/sandbox/backend.rs:157 | 2393-2396: `"XDG_DATA_HOME",` `"XDG_CONFIG_HOME",` `"XDG_CACHE_HOME",` `"XDG_STATE_HOME",`; 2400: `crate::sandbox::backend::KAGE_HOME.to_owned(),`; backend.rs:157: `pub const KAGE_HOME: &str = "/state/kage";` |

## The serve.rs claim

Cited at next-session.md:161, README.md:91 and
experiments/50-errand-probe.sh:19. The research says: with no base URL,
errand gives the session the real provider key and only logs a warning.

What the code does at errand@4a8011cd3668:

1. The warning exists: src/serve.rs:728-732, inside `start_broker`
   (src/serve.rs:661-744). Its text says the credential is given to the
   session. The cited range 727-736 starts one line early, on `};`.
2. The bailey backend does not give the key. Under `egress.mode = proxy`,
   `start_broker` always returns a `ProviderBrokering`
   (src/serve.rs:683-686, 739-743). `brokering_credential_names` holds every
   brokered provider with a key (src/serve.rs:442-448). At launch,
   `brokered_env` replaces each such variable with its nonce, or removes
   it when the provider has no route (src/sandbox/bailey.rs:209-227,
   538-541). A test pins this: src/sandbox/bailey/tests.rs:527-557,
   "a provider the broker cannot route loses its variable". So the session
   gets no key at all, real or nonce. The warning text is stale.
3. A missing `baseUrl` does not by itself mean no route. The route's
   upstream is `baseUrl`, else the configured model's base URL in the host
   model store, else a common base URL of the provider's store models
   (src/serve.rs:392-404).
4. Real keys do cross in other cases:
   - `egress.mode = open`: `start_broker` returns `None`
     (src/serve.rs:662-664), so no brokering is applied and
     `launch.env` passes unchanged (src/sandbox/bailey.rs:538-541).
   - a provider with `broker: false`, or an extension provider by
     default: not in the brokered names (src/config/schema.rs:246-262,
     src/serve.rs:442-448).
   - the podman backend: it is given no brokering
     (src/daemon.rs:82-87) and puts every launch env value on its argv as
     `--env KEY=value` (src/sandbox/podman.rs:119-122).

Suggested replacement sentence: "Under the bailey backend with
`egress.mode = proxy`, a brokered provider the broker has no route to
loses its key variable, so the session cannot use it; errand logs a
warning whose text wrongly says the key is given (src/serve.rs:728-732,
src/sandbox/bailey.rs:209-227). Real keys reach the session under
`egress.mode = open`, for `broker: false` and extension providers, and
under the podman backend."

## Found on the way (not counted above)

### File-level references with no line number

| research file:line | reference | result |
| --- | --- | --- |
| next-session.md:140 | errand `src/sandbox/mod.rs`: only two backends | supported: errand@4a8011cd3668:src/sandbox/mod.rs:244-250 (`enum Backend` with `Bailey` and `Podman`), src/daemon.rs:74-101 |
| poc/bailey-gvisor/bailey:6 | errand `src/sandbox/bailey.rs`: the verbs errand calls | supported: doctor at bailey.rs:388, trust at 573 and 715, run at 602 and 716-721 and 741-753, untrust at 722 and src/sandbox/mod.rs:187 |
| runtime-survey.md:23 | virt-pvm `pvm.c` | not checked: that repository is not pinned or cloned |
| incus-gaps-survey.md:11 | `idmap_probe.c` | not checked: the survey says the file is not in this tree |

### Evidence files cited but missing from the tree

| research file:line | reference | result |
| --- | --- | --- |
| poc/bailey-gvisor/bailey:179 | `results/README.md` | not in `results/` |
| experiments/81-engine-cost-interleaved.sh:9 | `results/README.md` | not in `results/` |
| README.md:97 | `results/30-run1-reflection-inside.log` | not in `results/` |
| experiments/lib/fakes.py:60 | `results/30-run1-*` | no such file in `results/` |

### The Sysbox patch

`patches/sysbox-v0.7.1-no-modules-no-sys-resource.diff` has hunks at
`linuxUtils/linux.go @@ -253` and `libcontainer/nsenter/nsexec.c @@ -749`.
`git apply --check` passes for the first in sysbox-libs@cacac1939552 and
for the second in sysbox-runc@081856cc5d17. No tree was changed.

### An uncited claim the pinned code contradicts

poc/bailey-gvisor/bailey:434-435 says "bailey warns and runs without the
policy". At bailey@98d9b455adc6 an explicit `--config` layer is never
rejected (crates/bailey/src/config.rs:466-473). The "not applying" warning
is printed only for a rejected discovered `bailey.toml` layer or a rejected
global config (crates/bailey/src/cli.rs:1489-1503,
crates/bailey/src/config.rs:414-431,452-463). errand's own comment
(src/sandbox/bailey.rs:726-734) describes the same refusal; at these pins
it cannot hit the explicit policy. INFERRED: the comment predates the
explicit-config rule, or guards against a future change.
