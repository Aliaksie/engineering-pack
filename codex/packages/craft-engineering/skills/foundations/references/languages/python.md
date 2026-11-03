# Python decisions

Read the supported interpreter version, package layout and environment configuration. Imports
execute module bodies, so changes to import-time work can affect startup, tests and optional
dependencies. Preserve supported packaging and entrypoint behavior.

Type annotations document/check expectations but do not validate external values. Validate domain
constraints at boundaries and distinguish absent values from false or empty valid values.
Avoid mutable default arguments for per-call state; make shared caches explicit and bounded.

```python
def read_headers(paths):
    headers = []
    for source in paths:
        with open(source, encoding='utf-8') as stream:
            headers.append(stream.readline())
    return headers
```

Use context managers for resource ownership. For async code, distinguish coroutines from scheduled
tasks, await owned work and handle cancellation during cleanup. Blocking I/O in an event-loop task
can stall unrelated requests; thread/process choices depend on workload and the actual runtime.
Protect multi-step shared-state invariants rather than relying on incidental interpreter behavior.

Verify exceptions, resource closure and import behavior with the project's runner. Check filesystem,
locale and encoding assumptions when portability matters.

## Atomic reservation with SQLite

This complete standard-library example uses one conditional SQL update to prevent the
balance going below zero. The condition is evaluated by SQLite with the write; it is not a
Python read/check/write sequence. The two calls illustrate the result contract, not a load
or concurrency test.

```python
import sqlite3
from contextlib import closing

with closing(sqlite3.connect(':memory:')) as database:
    database.executescript('''
        CREATE TABLE account (
            id INTEGER PRIMARY KEY,
            available INTEGER NOT NULL CHECK (available >= 0)
        );
        INSERT INTO account VALUES (1, 10);
    ''')

    def reserve_units(account_id, units):
        if type(units) is not int or not 1 <= units <= 1_000_000:
            raise ValueError('Units must be a positive integer within the allowed range')
        with database:
            changed = database.execute(
                'UPDATE account SET available = available - ? '
                'WHERE id = ? AND available >= ?',
                (units, account_id, units),
            ).rowcount
        return changed == 1

    assert reserve_units(1, 7) is True
    assert reserve_units(1, 7) is False
    assert database.execute('SELECT available FROM account WHERE id = 1').fetchone()[0] == 3
```

A false result means no row was changed, including a missing account. Database errors propagate
instead of becoming insufficient-balance results. Each successful call reserves again: duplicate
operation identities need a separate durable uniqueness/transaction design. A Python lock alone
would not coordinate other processes writing the database. The sample amount ceiling is a domain
choice to replace with the consumer's real constraint.

API details: [sqlite3 connection transactions](https://docs.python.org/3/library/sqlite3.html#how-to-use-the-connection-context-manager).
The connection context handles commit/rollback; the outer `closing` closes the connection.
