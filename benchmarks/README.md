# Performance workloads

- [Compiler, REST/JSON, frames and existing workloads](performance/README.md):
  reproducible commands, raw measurements and interpretation.
- [Suspended tasks](suspended-tasks/README.md): settled Linux RSS and threads.
- [Frame storage](frame-storage/README.md): historical layout/memory comparisons
  and frame-reuse analysis scaling.
- [Cooperative budgets](cooperative-budget/README.md): compute, I/O and mixed
  workloads and prior scheduler measurements.

Measurements are opt-in. Correctness belongs in tests; host-sensitive values
are not CI thresholds.

Every Python module must import on Linux, macOS and Windows: import
platform-specific modules such as `fcntl` or `resource` inside the functions
that need them. CI imports each module on all three hosts and runs each
harness's `test_*.py` there; Unix-only harness tests skip themselves on Windows.

- [Logging](logging/README.md): `std/log` time per record into discarding,
  file, and buffered or unbuffered rolling sinks, with and without daily
  rolling.
- [Runtime scaling](runtime-scaling/README.md): opt-in Linux/macOS and optional Windows socket,
  timer and suspended-task populations, pinned storage and resource cleanup.
