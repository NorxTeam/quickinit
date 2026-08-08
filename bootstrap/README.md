# quickinit bootstrap

This package is the smallest freestanding entry point for the quickinit
handoff. It exercises the currently frozen `write`, `getpid`, `wait`, and
`exit` syscall slice and exits deterministically after proving that there are
no children to reap.

It is intentionally separate from the host `std` policy core in the parent
package. The bootstrap is not yet the service supervisor or a complete PID1:
spawn/exec/signal, filesystem-backed configuration, and blocking wait still
belong to the next kernel ABI pass.
