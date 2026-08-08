# Nordix quickinit

quickinit is the planned first userspace process and service supervisor. The
current 0.1 implementation is a host-testable policy core: it parses a strict
init configuration, validates service dependencies, starts services in stable
order, handles exits, restarts, backoff, timeouts, forwards signals, retains
bounded sequence-numbered logs, and exposes a small control CLI grammar.

The policy core deliberately has no host process execution and no hidden
fallback to Linux paths. ProcessBackend is the narrow adapter that will bind
the model to Nordix spawn, exec, wait, and signal syscalls when those ABI
operations are available. Until then, the Roadmap 5.2 PID 1 and QEMU items
remain open.

## Configuration

The source format is a strict TOML subset with [init], [logging], [env], and
[service.NAME] sections. Unknown keys, duplicate keys, malformed arrays,
missing commands, unknown dependencies, and dependency cycles are rejected
before any service starts.

Run the host contract tests with:

~~~text
cargo test --locked
~~~

The freestanding bootstrap package is in [`bootstrap/`](bootstrap/). It is
built by `toolchain/scripts/build-quickinit.py` for both supported targets and
currently proves the bounded `write/getpid/wait/exit` handoff. It is not yet a
full PID1 supervisor; the remaining kernel-backed process and filesystem
contracts stay explicitly open in Roadmap 5.2.
