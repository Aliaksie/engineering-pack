# JVM languages decisions

Identify Java/Kotlin language level, JVM target, build tool and actual framework independently.
Keep nullability and checked/unchecked exception expectations visible across Java/Kotlin boundaries.
A platform type or annotation is not proof that a remote or Java-supplied value is non-null.

Choose collection mutability and value equality deliberately. Public records/data classes expose
a contract; adding fields can affect serialization, construction and equality. Validate untrusted
payloads before promoting them to domain objects.

This fragment uses Java 10+ local type inference; use an explicit resource type for an
older supported source level. `digest` represents the caller's existing stream-processing operation.

```java
try (var input = Files.newInputStream(source)) {
    return digest(input);
}
```

Manage streams, database sessions and executors by explicit ownership. Futures, reactive streams,
coroutines and virtual threads have different cancellation and scheduling behavior; use the installed
API contract rather than assuming they are interchangeable. A cheap thread does not remove database
connection limits or protect a shared mutable invariant.

Inspect transaction and proxy boundaries when a framework supplies them: self-invocation, async
handoffs and lazy state can change when work executes or commits. Use atomic operations, locks or
transactions that cover the whole decision. Verify lifecycle and failure paths at the relevant JVM
or framework boundary; compiling a helper does not establish integration behavior.
