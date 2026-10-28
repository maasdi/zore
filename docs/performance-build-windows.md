# Windows native build measurements

Issue [#205](https://github.com/maasdi/zore/issues/205) measures current-main
native build stages and the tradeoffs of overlapping runtime preparation with
clang object generation on Windows x64 MSVC. The
[historical Linux report](performance-build-scheduling.md) remains unchanged
as evidence for its recorded source and host.

The [capture harness](../benchmarks/performance/README.md#windows-build-capture)
builds an exact source revision and a matching serial control in isolated
worktrees. The control changes only the scheduling block; it is not a new
production compiler option. Release compiler/profiler identities, the control
patch, source manifests, cache evidence, LLVM/object hashes and all command
intervals are preserved. Separate linker timing is nested within rustc linking.

An initial profile covers hello, JSON, the REST fixture, the multi-file REST
sample, a local dependency and a suspended-payload control. A subsequent paired
comparison uses the same six workloads and three alternating repetitions, with
separate warm caches and fresh cold caches. Semantic/ownership checking and
code-generation optimization remain unchanged.

Hosted capture and correctness validation are pending. No Windows build-time
improvement, regression or performance-parity result is claimed before the
raw evidence is recorded and reviewed.
