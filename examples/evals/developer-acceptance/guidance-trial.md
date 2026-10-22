# Deliver a scoped change

Use the accepted request and project instructions to establish scope, constraints and checks.
Inspect Git state and preserve unrelated work. Follow existing branching and authorization
policy before edits. Reuse relevant implementation and read the engineering detail needed
for the affected behavior. Resolve material ambiguity without inventing requirements.

Keep tiny changes direct: inspect, edit, run the relevant check and report the result.
No new setup, ADR, task contract, worker or tracker is required merely to fix a small bug.
For substantial work retain one private task record with cited constraints, criterion IDs,
source identity, evidence and gaps. The optional runtime uses task-start --contract and
conditional task-step --acceptance-evidence; no-hook or guidance-only work uses the same
facts manually. Missing packages or native controls do not block useful local work.

Verify each required criterion with an adequate check or explicit assessed observation.
Record actual checked source, artifact, basis and reporter. Distinguish passed, failed,
not-run, not-available and unknown; a green unrelated check proves no criterion. Recheck
after code, instructions or acceptance changes; never relabel old evidence as current.
Correct in-scope defects and repeat affected checks. Preserve disagreements and optional gaps.

Return changed behavior, criterion-to-check evidence and remaining limits. Completion
requires current support for every required criterion; unavailable checks remain gaps.
Local edits, commits and external actions have separate authorization. Worker/review results
cannot certify integrated acceptance; silence and timeout grant no permission. Follow the
consumer's commit strategy only when authorized. Keep internal records out of shipped files.
