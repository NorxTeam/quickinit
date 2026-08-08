# Contributing

Keep the supervisor deterministic and dependency-free until the userspace
ABI exposes the process, filesystem, signal, and logging operations required by
PID 1. Every behavior change needs host tests; changes to boot or service
ordering also need a graphical QEMU smoke once the kernel integration gate is
available.
