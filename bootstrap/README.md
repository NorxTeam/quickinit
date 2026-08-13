# quickinit bootstrap

This package is the smallest freestanding entry point for the quickinit
handoff. It verifies PID 1 identity, exercises the frozen `write`, `getpid`,
`wait`, `spawn`, and `exit` syscall slice, and exits deterministically after
proving that its child was reaped.

It is intentionally separate from the host `std` policy core in the parent
package. It is not yet the service supervisor: exec/signal, filesystem-backed
configuration, and blocking wait still belong to the next kernel ABI pass.
