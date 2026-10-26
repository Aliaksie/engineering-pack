# Go decisions

Read the module's Go version, package boundaries and existing error conventions. Keep exported
APIs small and useful to their consumers. Interfaces are valuable at actual substitution seams;
do not generate one for every concrete type. Review nil interface behavior when a typed nil can
cross an API boundary.

Errors carry outcomes callers must handle. Wrap causes when callers need classification and avoid
logging the same failure at every layer. Defers run when the containing function returns; a defer
inside a long loop can retain many resources. Close owned resources at the intended lifetime.

```go
type Source interface {
    Load(context.Context) ([]byte, error)
}

func load(ctx context.Context, source Source) ([]byte, error) {
    if err := ctx.Err(); err != nil { return nil, err }
    return source.Load(ctx)
}
```

Propagate context for cancellation and deadlines without storing request context in long-lived
services. Every goroutine needs an exit path and an owner for errors. A channel coordinates work;
it does not automatically bound a producer backlog. Only the owner responsible for completion
should close it, with concurrent senders accounted for.

Use synchronization or ownership for shared maps and compound updates. Run relevant tests and race
detection when concurrency is affected, while recognizing that a passing race run covers only the
executed schedules.
