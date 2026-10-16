# C# and .NET decisions

Identify the target framework, language version, nullable settings and application host. Keep
public nullability contracts consistent with runtime validation of external values. Records,
classes and structs have different equality, copying and mutation consequences; choose by meaning.

Own disposable resources through their actual sync/async lifetime. A service cannot safely retain
a request-scoped dependency merely because it was injected. Confirm serialization and ORM behavior
against configured options rather than assuming property names or lazy state are transparent.

This fragment requires a target supporting asynchronous disposal and an existing decoder
that accepts the cancellation token; it does not define a universal decoding API.

```csharp
await using var stream = File.OpenRead(path);
return await DecodeAsync(stream, cancellationToken);
```

Await tasks that belong to the operation and propagate cancellation where supported. Avoid blocking
on `.Result` or `.Wait()` in asynchronous paths. Cancellation is a request, not proof that an external
effect was undone; define the caller's result when work may have committed. Do not assume Task.WhenAll
limits concurrency or queue size.

Use appropriate synchronization for shared mutable state and transactions for durable invariants.
Check exception handling, cancellation and disposal with the consumer's test host. A successful build
cannot establish configured dependency lifetimes, remote permissions or database behavior.
