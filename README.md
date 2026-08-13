# quickinit

quickinit is the first userspace process and future service supervisor. The
current 0.1 implementation is a host-testable policy core: it parses and
validates a strict init configuration, resolves inherited environment values,
starts services in dependency/order sequence, models spawn or notification
readiness, handles exits, restarts, backoff, timeouts, forwards signals,
retains bounded sequence-numbered logs, and exposes a small control CLI
grammar.

The policy core deliberately has no host process execution and no hidden
fallback to Linux paths. ProcessBackend is the narrow adapter for the target's
spawn, exec, wait, and signal syscalls. The freestanding bootstrap now runs as
PID 1 during the kernel hand-off, proves child reaping, and returns a stable
status to the kernel; service configuration is defined here, while the target
adapter and long-running execution remain the next Roadmap 5.2 steps.

## Configuration

The source format is a strict TOML subset with `[init]`, `[logging]`, `[env]`,
`[shutdown]`, `[service.NAME]`, `[service.NAME.env]`, and
`[service.NAME.limits]` sections. Unknown keys, duplicate keys, malformed
arrays, unsafe paths, unsupported capabilities, missing commands or readiness
timeouts, unknown dependencies, and dependency cycles are rejected before any
service starts.

Each service declares an absolute `command` and `workdir`, optional `args`,
`mode` (`foreground` or `background`), `kind` (`service`, `daemon`, or
`oneshot`), and `autostart`. `stdin`/`stdout`/`stderr` support `inherit`,
`null`, `serial`, or `file:/absolute/path`. The remaining fields define an
explicit `order`, dependencies, restart policy, crash limit, runtime and
stop timeouts, stop/kill signals, timeout action, the six kernel capability
names (`mount`, `raw-io`, `net-admin`, `net-raw`, `memory-map`,
`device-admin`), and bounded CPU/memory/fd/child limits. Global `[env]` values
are inherited by every service; values in the service environment section
override them. A daemon must use `readiness = "notify"`; notification keeps a
service in `Starting` until the supervisor receives `notify_ready`. A oneshot
that exits successfully becomes `Completed` and unblocks dependents.

Manual `stop` enters `Stopping`, forwards the configured stop signal once,
escalates to the kill signal after `stop_timeout_ms`, and never applies the
restart policy to that intentional exit. Runtime/readiness timeout uses the
same bounded stop path and applies its declared timeout action. `start` queues
an explicit launch for a service with `autostart = false`; service/daemon
children remain owned by the supervisor regardless of foreground/background
mode.

`[shutdown]` selects the initial signal, bounded grace timeout, and `kill` or
`halt` timeout action. A controlled shutdown/reboot forwards the selected
signal, stops normal startup, and applies the configured timeout action on the
next supervisor tick.

Logging records use monotonic sequence IDs and supervisor timestamps. Both
memory records and optional serial lines are bounded by `retention`. When the
configured log storage is unavailable, `on_storage_unavailable` explicitly
selects `serial`, `continue`, or `halt`; `set_storage_available` lets the
target adapter report that condition. Stalled children use the service
`timeout_ms`, `stop_timeout_ms`, `stop_signal`, `kill_signal`, and
`timeout_action` policy, so termination and escalation are logged exactly once
per transition.

`[boot]` may name the normal `shell`, a `recovery_shell`, and service lists
for `mounts`, `devices`, and `logging`. The normal shell is not autostarted
until every named gate is `Ready`, `Running`, or `Completed` and log storage
is available. `start_recovery_shell` bypasses normal-shell gates for an
explicit recovery launch. `recovery_mode_for_source` selects the configured
mode for valid input and safely defaults to `RecoveryMode::Shell` when the
configuration cannot be parsed.

Getty uses this supervisor boundary rather than embedding service policy in the
kernel: quickinit owns the `/bin/getty` service lifecycle, restart/backoff,
shutdown signal, and recovery-console selection; getty owns fd-0 prompt state,
bounded retry/lockout, and returns a versioned `LoginHandoff` for `/bin/login`.
The current target adapter still exposes these as policy contracts, while the
Roadmap 5.2 service executor remains the component that will wire a long-lived
getty service to a real `ProcessBackend`.

## Control CLI

`parse_command` accepts `status`, `logs [service]`, `start SERVICE`,
`stop SERVICE`, `restart SERVICE`, `log-level LEVEL`, `shutdown`, and `reboot`.
`execute_line` dispatches those commands through the supervisor and returns
stable text: status lines contain service/state/exit fields, log lines use the
existing sequence/timestamp format, and control commands report their queued
state or return a typed backend error. The transport and target console loop
are intentionally left to the kernel/userspace ABI integration.

Run the host contract tests with:

~~~text
cargo test --locked
~~~

The host suite covers malformed configuration, spawn failures and dependency
blocking, rapid crashes/backoff, orphan reaping, signal races, shutdown and
reboot log hand-off, storage failure, readiness gates, and recovery launch.
The existing graphical QEMU smoke additionally verifies PID 1 hand-off,
child reaping, deterministic failure markers, and kernel log continuation on
both supported targets.

The freestanding bootstrap package is in [`bootstrap/`](bootstrap/). It is
built by `toolchain/scripts/build-quickinit.py` for both supported targets and
currently proves the bounded `write/getpid/wait/spawn/exit` PID 1 handoff. It
is not yet a full service supervisor; the remaining kernel-backed process and
filesystem contracts stay explicitly open in Roadmap 5.2.
