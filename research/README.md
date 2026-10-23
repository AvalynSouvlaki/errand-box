# Research files

The scripts and the raw output behind
[docs/research/engines.md](../docs/research/engines.md) and
[docs/research/rootless.md](../docs/research/rootless.md). The documents give
the numbers and the conditions; this directory keeps what produced them, so a
later session can run them again.

The files are kept as they were run, on one host: Ubuntu 24.04 in a hosted
container, kernel 6.18, 4 CPUs, 16 GiB, cgroup v1, no init, no `/dev/kvm`.
They name the paths of that host (`/srv/rl`, the pool `bt`, the image
`bench`). Read the header of each script for its inputs. CI does not lint
this directory.

## Experiments

| file | what it does | used by |
| --- | --- | --- |
| [experiments/workload.sh](experiments/workload.sh) | One workload (spawn, files, git, cpu, compile) in a fresh directory; prints its own milliseconds. The same file runs in every engine. | x92, uml |
| [experiments/x91-incus-features.sh](experiments/x91-incus-features.sh) | Root Incus: systemd as PID 1, Docker, podman, FUSE, start time, golden copy time, limits. Its limits section failed on this host (`[: Illegal number`); the limits were measured by hand, see engines.md section 3. | engines.md section 1 |
| [experiments/idmap-probe.c](experiments/idmap-probe.c) | Makes an idmapped mount through a user namespace whose maps are written, on btrfs, ext4 and tmpfs. Shows that the kernel supports what Incus 6.0.0 probes wrongly (E-20). | engines.md section 2 |
| [experiments/x92-rootless-cost.sh](experiments/x92-rootless-cost.sh) | runc, root Incus and rootless Incus, interleaved, 5 rounds. | rootless.md section 1 |
| [experiments/rootless/start.sh](experiments/rootless/start.sh) | As the unprivileged user: joins the delegated cgroups and starts the user namespace. | rootless.md section 1 |
| [experiments/rootless/rincus-ns.sh](experiments/rootless/rincus-ns.sh) | Inside that namespace: binds the private state and `/etc/subuid`, then runs incusd. | rootless.md section 1 |
| [experiments/rootless/x93.sh](experiments/rootless/x93.sh) | As the unprivileged user: launch, golden copies, starts, limits and quota. | rootless.md section 1 |
| [experiments/uml/run.sh](experiments/uml/run.sh) | Builds the UML and x86_64 kernels; runs the workloads in UML (ptrace, seccomp) and in QEMU TCG. | rootless.md section 2 |
| [experiments/uml/wl-init.sh](experiments/uml/wl-init.sh) | PID 1 of those guests: runs every workload three times, then exits. | rootless.md section 2 |

The engine comparison against gVisor and runc (`81`) is the script
`research/errand-sandbox/experiments/81-engine-cost-interleaved.sh` of the
agent-box research branch, run unchanged.

## Results

| file | from |
| --- | --- |
| [results/81-repro.txt](results/81-repro.txt), [results/81-repro-raw.txt](results/81-repro-raw.txt) | the `81` comparison, medians and every run |
| [results/91-incus-features.txt](results/91-incus-features.txt) | x91 |
| [results/92-rootless-cost.txt](results/92-rootless-cost.txt), [results/92-rootless-raw.txt](results/92-rootless-raw.txt) | x92, medians and every run |
| [results/uml-tcg.txt](results/uml-tcg.txt) | uml/run.sh: the `RESULT` lines of the ptrace, seccomp and tcg runs |
