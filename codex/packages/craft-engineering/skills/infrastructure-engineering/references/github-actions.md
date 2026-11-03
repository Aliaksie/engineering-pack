# Apply CI/CD principles to GitHub Actions

Use this page with [CI/CD](ci-cd.md) when the affected configuration actually uses GitHub
Actions. Inspect `.github/workflows`, referenced local actions/reusable workflows and
invoked scripts. Check the action revisions and runner/shell configuration in the target;
the language under test and the runtime of a JavaScript action are separate constraints.

## Validate workflows and scripts

Use [actionlint](https://github.com/rhysd/actionlint/blob/main/docs/checks.md) for workflow
structure, expressions, contexts and job dependencies. Its ShellCheck integration checks
supported embedded shell scripts when ShellCheck is available; verify that prerequisite
when shell linting is a required gate. Check PowerShell or other script languages with
appropriate tooling rather than assuming ShellCheck covers them. Follow the pinned
tool's documentation for supported checks; it cannot prove remote actions or jobs succeed.

Run validation against all affected workflows, including separately dispatched release
workflows. Wire the check into CI and into the dependency chain protecting publication.
For a new lint integration, test a valid workflow and a representative invalid expression,
dependency or script so a missing integration cannot look like a successful check.

## Inspect event and privilege boundaries

Trace `on`, branch/path filters, `if`, `needs`, matrix expansion and job outputs. Check
pull-request, push and manual paths separately, including fork permissions and skipped
dependencies. Lint can catch configuration mistakes; repository rules and event behavior
still require actual evidence. Do not dispatch a publishing workflow merely to test it.

Apply least-privilege `permissions`, scope write tokens to publishing jobs and avoid
persisted checkout credentials when they are unnecessary. Pass untrusted event values as
data through environment variables or structured arguments; do not interpolate them into
shell source. In particular, do not combine privileged `pull_request_target` execution
with untrusted checked-out code. Review privileged downstream workflows and downloaded
artifacts for the same trust boundary.

Use full commit SHA action pins and an explicit update policy where the consumer requires
immutable dependencies; a version comment is only a human-readable annotation. Verify
downloaded tool archives against a pinned digest. Set useful timeouts and concurrency
rules, especially for releases: cancelling an in-progress publisher can leave partial
external state. Check artifact names, retention, hidden-file needs and sensitive content.

Follow the target's policies and GitHub's [secure use reference](https://docs.github.com/en/actions/reference/security/secure-use).
Report the hosted run URL and tested revision when available; label local lint and script
tests separately. An older green run is not evidence for a newly changed workflow.
