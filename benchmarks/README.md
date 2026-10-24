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

- [Runtime scaling](runtime-scaling/README.md): opt-in Windows/Linux/macOS socket,
  timer and suspended-task populations, pinned storage and resource cleanup.
