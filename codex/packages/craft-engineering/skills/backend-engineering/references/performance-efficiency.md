# Improve the workload that matters

Define a target such as tail latency, throughput, memory or remote-call budget and measure a
representative workload. Record data volume, concurrency, warmup and dependencies so a comparison
is meaningful. Optimize a demonstrated bottleneck rather than a stylistic suspicion.

Account for amplification: per-row queries, repeated serialization, unnecessary network round
trips and unbounded buffering. Batch where semantics permit it, but bound batch size, queueing
delay and memory. Streaming can reduce peak memory while extending resource lifetimes; verify
cleanup and cancellation.

Caching needs an owner, key and freshness rule. Include tenant, permission and relevant version
in the key; decide invalidation, failure behavior and stampede control. An optimization that serves
another user's data or silently changes consistency is a defect regardless of speed.

```text
Experiment: compare 200 lookups individually with bounded groups on the same dataset;
record total time, slowest-group time, peak memory and dependency request count.
```

Test overload and unfavorable inputs as well as the average case. Retain the simplest change
that meets the target and report tradeoffs, noisy measurements or unavailable production evidence.
Do not present a local microbenchmark as proof of end-to-end improvement.
