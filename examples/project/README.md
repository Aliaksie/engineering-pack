# Consumer configuration example

This is a configuration skeleton, not a working service. Module paths, owners, checks and
remote identities are examples. `team-task-cli` is a consumer-owned tool name and is not
provided or claimed to exist. Running the route/operation examples returns plans only.
Replace all bindings with actual tools and host model names before using them in a project.
Automatic checkpoints and delegated workers start disabled. No credentials belong here.

From the source checkout, inspect the example without installing any integration:

```text
npm run route -- --root examples/project --intent review --file services/orders/Order.java
npm run services:prepare -- --root examples/project
```

The route selects the `orders` module, its owner and relevant guidance. Service preparation
lists configured commands and parameter names; availability and authentication remain unverified.
The example application files and Gradle wrapper are not supplied, so its declared checks
cannot be run as an application test suite.

For your own project, start with `.craft/project.json` containing:

```json
{ "formatVersion": 1 }
```

Add only the modules, stack evidence and service bindings you actually use; the larger
example is a menu of configuration fields, not a required project structure. Without a
configuration file the runtime uses this same minimal default. Run commands from the source
checkout with `--root` pointing at your project, or invoke an installed package's
`support/cli/main.mjs` directly. See [routing](../../docs/ROUTING.md) and
[integrations](../../docs/INTEGRATIONS.md) for the field contracts.
