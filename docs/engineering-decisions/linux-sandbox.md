# Linux worker sandbox (experiment X2, tests R01-R21 subset)

## Target experience
Whatever a worker is made of (a model-driven agent, a parser, a script it wrote itself), it can read only what it was handed, can talk to nothing but the broker, and cannot use up the machine. The person is told if the machine cannot give that guarantee.

## References
Bubblewrap 0.11.1 (the sandbox Codex also uses); Codex's launch sequence and MXC's bubblewrap backend (surveyed in earlier experiments E1-E17); `nearai-red-sandbox` is still to be read in full (not done). Earlier attack experiments on this VM are in `private-ai/specs/experiments/`.

## Approach (`kk-sandbox`)
`bwrap` with `--unshare-all --unshare-user --disable-userns`, `--die-with-parent`, `--new-session`, `--clearenv`, `--cap-drop ALL`, a private `/proc`, `/dev` and `/tmp`, and an allowlist filesystem: read-only `/usr` plus the binds the kernel names. No host paths are visible. Limits come from a systemd user scope (`TasksMax`, `MemoryMax`, `MemorySwapMax=0`) plus `prlimit` for CPU, file size, open files and core size; stdout and stderr are capped; a wall-clock watchdog stops the scope. Before running any worker the launcher runs a probe worker under the same profile and refuses to start if the probe can reach a host listener over IPv4 or IPv6, resolve DNS, read a planted secret, create a nested user namespace, or see more than a handful of processes. The broker channel will be a unix socket in a per-worker directory bound into the sandbox (not an inherited descriptor, because the crate forbids `unsafe`).

## Results (VERIFIED_BY_RUN in the Lima VM `kak`: Ubuntu 26.04, kernel 7.0, bubblewrap 0.11.1; `scripts/test-linux`)
- R01-R06: control run reaches the host listener (5 connections) and sends UDP; sandboxed run: TCP v4, v6, v4-mapped, raw sockets, DNS, curl, bash `/dev/tcp` and proxy-environment attempts all blocked, and the host listener sees no connection.
- R09, R11, R12: direct path, home-directory secret, `/etc/shadow`, `/proc/1/root`, a symlink to the secret, a hard link, `..` traversal all fail; `/home` is empty; only descriptors 0-3 exist in the worker.
- R13, R15: a host process cannot be killed or seen; nested user namespace, network namespace, `sudo`, `mount`, `chroot` fail; effective capabilities are zero.
- R18: a fork bomb stops at the process cap, a memory hog is stopped at the cgroup limit, a CPU spin is stopped, a sleeping worker is killed at the wall limit, a disk filler is stopped by the file-size limit, an output flood is capped.
- R21: the launcher's own probes pass on the real profile and report failures when the profile is weakened (shared network; nested user namespaces allowed).
- The parent's environment variables (including a fake API key) never reach the worker.
- Mutation checks, applied and verified to have rebuilt: removing `--unshare-all` fails 3 tests; always sharing the network fails 2; binding the whole host root read-only fails the filesystem test. Removing `--cap-drop ALL` is not detected, because an unprivileged sandbox already has no effective capabilities (an equivalent mutant; the flag stays as defence in depth).

## Findings
- `--unshare-all` alone does not unshare the user namespace; `--disable-userns` fails with "requires --unshare-user" until `--unshare-user` is given explicitly.
- `RLIMIT_NPROC` counts every process of the user, so it cannot cap a single sandbox and starved the launcher when tests ran in parallel. Per-sandbox process and memory caps need cgroups (systemd user scopes). A host without a systemd user session is reported as unsupported rather than silently run without caps.
- The VM mounts the repository read-only and its view of changed files can lag by a few seconds; a mutation test run too soon after an edit silently tested the old code. Mutation runs now wait and confirm a rebuild.
- One run of the exhaustion test failed once in about 21 runs, the first run after a rebuild, before diagnostics were added; it did not recur in 12 isolated and 8 full runs. Cause unknown: OPEN. Diagnostics now print exit status and output on failure.

## Not covered
macOS (this crate returns `Unsupported`; the macOS strict-mode decision X3 is open); seccomp filtering; the covert channel through the broker socket (R17); GPU, display, clipboard and D-Bus on desktop Linux (R14, R19); the full red-sandbox read; Windows.
