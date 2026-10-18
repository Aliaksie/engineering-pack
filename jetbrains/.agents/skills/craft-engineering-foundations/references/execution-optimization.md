# Optimize a measured execution path

Identify the scarce resource: context, latency, model cost, tool calls or remote quotas.
Record a representative baseline before claiming improvement. A cheaper model that creates
extra retries may cost more overall; a large cache can return stale or cross-tenant information.

Choose the simplest useful control. Batch independent searches, avoid duplicate reads and
return bounded evidence. Delegate extraction only when the host supports it and authorization
permits it. Consumer-defined profiles map logical roles to available models; shared instructions
must not hardcode a vendor alias or permission grant. Escalate when ambiguity, missing evidence
or repeated failure makes the cheaper path inadequate, and keep the final judgment accountable.

Separate provider prompt caching from application result caching and local checkpoints.
For each application cache define a key covering source revision, query, relevant configuration,
identity and authorization scope. Specify expiration/invalidation and whether failure permits a
stale response. Never reuse protected data across principals merely because prompts match.

Report comparable task outcomes, elapsed time and observable costs, including misses and retries.
If the host exposes no cost or cache telemetry, say so. Optimization guidance does not establish
an implemented cache engine or measurable model benefit by itself.
