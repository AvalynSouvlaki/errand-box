# Security

errand-box runs code an AI agent writes, on a host that also holds the
agent's credentials. This page states what protects what, and what does
not.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting on this repository ("Security",
then "Report a vulnerability"). Do not open a public issue for it.

## The trust boundary

| side | what runs there | trusted? |
| --- | --- | --- |
| host, root | incusd, incus-user, lxcfs, s6 or systemd, `errand-box setup`, `up`, `maint` | yes |
| host, the errand account | errand, the `bailey` shim, the watchdog | yes, but holds credentials (below) |
| guest | everything in a session: kage, the agent's commands, Docker and podman inside, the guest's root | **no** |

A guest is an unprivileged system container. Its boundary is the shared
host kernel, user namespaces with a uid range of its own, seccomp, and the
cgroup limits Incus sets. It is not a VM. A kernel exploit in a guest is a
host compromise.

## Who holds what

| credential | where it lives | can a guest read it? |
| --- | --- | --- |
| chat bot token | errand's config (`~errand/.config/errand/config.json`, mode 0600) | no |
| model provider key | errand's config; errand's broker adds it to requests | no: under `egress.mode = proxy` the guest gets a per-session nonce, and a provider the broker cannot route gets no variable at all ([docs/research/review.md](docs/research/review.md)) |
| GitHub token | errand's config | **yes**: errand puts it in the session's environment as `GH_TOKEN`. Use a token of an account that owns nothing of value. |
| the operator's `creds.env` | wherever the operator keeps it; setup only reads it | no |

errand-box never prints a credential. `setup` names which keys it read,
never their values. The shim passes the session's environment into the guest
through the Incus API, not on a host command line.

## What the operator supplies, and with which permissions

Give each credential before the step that needs it. Put it in `creds.env`
(mode 0600) on the host, or in the repository's secrets for a workflow.
Never in a chat, an issue, or a file in a repository.

| for | credential | permissions it needs |
| --- | --- | --- |
| errand: chat | `ERRAND_CHAT_TOKEN`, a Discord bot token, and the channel id and user ids in `errand-box.env` | the bot's privileged intent Message Content, set in the Discord developer portal (errand asks for Guilds, Guild Messages and Message Content); in the channel: view, send messages, create public threads, send messages in threads, manage threads (errand archives them), attach files, add reactions, read message history |
| errand: the model | `ERRAND_PROVIDER_CREDENTIAL`, and `ERRAND_PROVIDER_BASE_URL` in `errand-box.env` | a key for that endpoint only. With a base URL the key stays in errand's broker |
| sessions: GitHub (optional) | `ERRAND_GITHUB_TOKEN` | a fine-grained token of an account that owns nothing of value, limited to the repositories the agent may change. Every session can read it |
| the scheduled workflows | the workflow token, given by GitHub | as each workflow declares. For `freshness.yml` to open pull requests, set "Allow GitHub Actions to create and approve pull requests" in the repository's Actions settings |
| CI on those pull requests (optional) | a repository secret `FRESHNESS_TOKEN` | a fine-grained token for this repository only: contents and pull requests, read and write. A pull request opened with the workflow token starts no workflow; one opened with this token does |
| a release | none beyond the workflow token | `release.yml` asks for contents: write |

## What each compromise reaches

| compromised | reaches |
| --- | --- |
| a guest's root | its own instance; the two host directories errand granted it (the project and the session's state); errand's broker, with errand's allowlist. Not the host network, other guests, or the host's other files. |
| a guest that escapes its container through a setup mistake | an unprivileged host uid of its own range; files of the errand account through the agent's mapped uid 1000. |
| a guest that escapes through a kernel exploit | the host. |
| the errand account | errand's credentials, and today root: its restricted project forbids privileged containers, raw LXC settings, and disk devices outside its home, but allows proxy devices for the broker link, and a proxy device can reach any host socket, the Incus admin socket included (E-61). T-020 replaces the link so the project can forbid them. A guest cannot add devices, so this is reached only through the errand account itself. |
| incusd, incus-user, lxcfs | root. |

## The rules each change keeps

1. **The host never acts on a file a guest can write.** The session's state
   directory is writable from the guest. So the shim checks the policy
   errand wrote there against the digest it recorded at `trust`, and every
   Incus client call names errand-box's own client directory instead of the
   `HOME` errand points into that state directory.
2. **No signal by name.** Daemons stop through their supervisor, by pid.
3. **No unverified download runs.** Every fetched archive is checked against
   `pins.toml` before use.
4. **A session gets no network device.** Its only way out is errand's broker.

## Degraded on some hosts

`errand-box doctor` and `setup` report these, each as DEGRADED:
- no AppArmor in the kernel: Incus loses its AppArmor profile; seccomp and
  per-guest uid ranges remain (edge case E-17);
- no usable lxcfs: guests see the host's memory and CPU count (E-08);
- a `dir` storage pool: the disk quota is not enforced (E-05);
- Incus 6.0.0: new instances rewrite their rootfs, which is slow but not less
  safe (E-20).
