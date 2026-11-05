# TypeScript decisions

Inspect compiler options, target, module resolution, emitted output and the runtime executing it.
TypeScript types disappear at runtime; validate external JSON and messages before assigning a
trusted domain type. A type assertion changes the checker's view and performs no conversion.

Use unions and narrowing to represent distinct outcomes. Prefer precise boundaries over broad
`any`; `unknown` requires evidence before use. Keep nullable and optional semantics consistent
with project compiler settings. Structural compatibility does not prove two identifiers have
the same domain meaning, so use suitable wrappers or branded types when that distinction matters.

```typescript
type Receipt = { kind: 'accepted'; reference: string } | { kind: 'declined'; reason: string };
function caption(value: Receipt): string {
  return value.kind === 'accepted' ? value.reference : value.reason;
}
```

Model asynchronous success and failure explicitly where callers need them. Verify promise
ownership, cancellation and runtime serialization separately from compile-time inference.
Avoid importing server-only modules into browser bundles, and check package exports against the
consumer's build mode. Use installed library declarations and actual runtime contracts together.

Run relevant type/build checks plus behavioral checks for parsing, module loading and edge
values. Compiler success does not establish authorization, correct network behavior or a working
production bundle.
