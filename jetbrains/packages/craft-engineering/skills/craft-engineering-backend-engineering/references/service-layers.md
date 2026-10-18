# Put policy beside its owner

Trace the public contract, domain decision and I/O needed to satisfy it. Keep a boundary when
it owns validation, translation, authorization, lifecycle or a meaningful substitution seam.
Avoid adding layers that only forward arguments and obscure the operation.

Transport objects describe a protocol; domain values express accepted meaning; persistence
records belong to storage policy. Reuse one type only when those contracts genuinely coincide.
Validate externally supplied shape and meaning before relying on them inside the domain.

Choose dependency direction so volatile transport/storage concerns do not define domain rules
without need. A transaction belongs around an invariant, not automatically around each helper.
Check whether repositories, clients and units of work can safely live for the containing service's
lifetime. A singleton retaining a tenant-scoped credential or request transaction is a correctness
and isolation problem even when dependency injection accepts it.

```text
Pseudocode — update an owned document in one store:
actor = trustedAuthenticationContext()
command = parseAndValidate(request.body)
within transaction:
    document = loadForUpdate(actor.tenantId, command.documentId)
    require mayEdit(actor, document)
    document.rename(command.title)
    save(document)
return publicDocument(document)
```

This example assumes tenant scoping and the relevant mutable permission/domain state are
checked within the same consistency boundary as the update. `loadForUpdate` denotes the
store's supported serialization mechanism, not a universal API. A local transaction does
not make an external authorization service or remote side effect atomic. Static request
validation need not hold a database transaction open.

Verify contracts at the boundary they promise: malformed input, denied access, domain rejection
and dependency failure. Use the existing architecture when it supports these responsibilities;
name the concrete problem before introducing another architectural pattern.
