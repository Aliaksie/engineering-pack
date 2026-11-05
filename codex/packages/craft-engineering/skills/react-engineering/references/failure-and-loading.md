# Design the complete interaction state

List the states a user can actually encounter: initial loading, useful partial/stale data, empty
success, rejected access, failed operation and completed action. Distinguish them in the state
model; a missing value should not simultaneously mean loading, empty and failure.

Retain useful content during refresh when the product permits it, while indicating freshness.
Make recovery actionable and preserve user input after a recoverable failure. Bound retries and
avoid duplicate submissions when an external effect may already have occurred. Disable only the
controls whose operation is unavailable.

Example: a failed address update keeps the edited address visible, labels the unsaved state and
allows an intentional retry. A timeout after submission requires status reconciliation before
creating another update, according to the server's operation contract.

Communicate progress and errors accessibly without repeatedly stealing focus. Distinguish route
render failures from ordinary rejected API operations; an error boundary does not replace all
asynchronous error handling. Verify slow responses, out-of-order results and keyboard recovery.
