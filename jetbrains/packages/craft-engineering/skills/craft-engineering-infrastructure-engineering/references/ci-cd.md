# Keep pipelines verifiable and recoverable

Start from the affected pipeline files, invoked scripts, dependency versions and consumer
policy. Trace the trigger through validation, build, artifact promotion and external
effects. Establish which source revision each stage consumes and what evidence permits
publication. A repository host does not establish its CI provider or deployment target.

## Validate the configuration and the work it runs

Use the provider's configuration validator or an established pipeline linter to check
syntax, expressions and job dependencies. Validate embedded scripts with their actual
interpreter's tooling. YAML parsing alone cannot establish pipeline semantics; pipeline
lint alone cannot establish that a release script behaves correctly.

Expose reproducible local check commands where practical and invoke the same checks in
CI. Make missing required validators fail visibly rather than silently skipping checks.
Keep dependencies for pipeline authoring separate from the product runtime. Select tools
and versions supported by the consumer; do not require a particular language or runner.

Test consequential behavior with isolated fixtures or dry-run facilities: failed checks
must prevent publication, untrusted contributions must not gain privileged effects, and
release artifacts must correspond to the validated revision. Cover changed trigger,
dependency, cancellation or recovery behavior when it affects those invariants. A small
negative fixture can verify a newly integrated validator detects its intended failure;
avoid tests that only assert workflow spelling or duplicate the implementation.

## Preserve the release boundary

Keep validation jobs at the permissions they need; grant write access only to stages
whose effects require it. Treat contribution metadata, checked-out code, caches and
downloaded artifacts according to their provenance. Do not execute untrusted source with
release credentials. Reference secrets through the consumer's supported mechanism and
avoid exposing them through logs, artifacts or interpreter input.

Pin action/plugin/tool dependencies to verifiable identities under the consumer's update
policy. Verify downloaded binaries and retain an update path for versions and checksums.
Identify artifact revision, platform and version; promote validated artifacts or rebuild
from the same verified inputs. Cache reuse is an optimization, not release evidence.

Ensure publication/deployment depends on the checks that protect it. Review filters,
skipped jobs, manual entry points and alternate release paths for bypasses. Coordinate
concurrent writers and cancellation around stateful effects. Bound waits and retries;
after an uncertain publish result, inspect actual state before repeating an effect.
Retain enough safe failure evidence to recover partial releases without exposing secrets.

## State what verification proves

Report validator versions, checks run, revision/scope and observed failures. Distinguish
static checks, isolated script tests and a real hosted run; a local pass cannot prove
runner availability, credentials, repository settings or external publication. Starting
a deployment, release or hosted workflow still needs the user's authorization.
