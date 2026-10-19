# Optimize a measured execution path

Identify the scarce resource: context, latency, model cost, tool calls or remote quotas.
Record a representative baseline before claiming improvement. A cheaper model that creates
extra retries may cost more overall; a large cache can return stale or cross-tenant information.

Choose the simplest useful control. Batch independent searches, avoid duplicate reads and
return bounded evidence. Delegate extraction only when the host supports it and authorization
permits it. Consumer-defined profiles map logical roles to available models; shared instructions
must not hardcode a vendor alias or permission grant. Use the current sufficient model or a
configured standard profile for routine implementation
and focused debugging, with real acceptance checks. Keep architecture, ambiguous ownership
and sensitive trust judgments with the main reasoner or a configured strong profile when
warranted. Escalate for insufficient declared capability or observed current-target quality
failures within configured attempt budgets. Missing tools, credentials or permissions need
prerequisite resolution, not a stronger model. A logical economical class is not a measured
price ranking.

Distinguish switching the main model from assigning a bounded worker: a switch can inherit
large context; a worker bounds transferred context but adds coordination and verification.
Consumer mappings and host capabilities determine availability. Missing support produces a
recommendation or local fallback, never a claimed switch. Explain choices when they change
execution; do not force a tier ladder or split tiny coupled tasks. Fewer dispatches do not
necessarily mean fewer model requests, smaller context or cheaper accepted work.

Separate provider prompt caching, application/lookup evidence reuse, routing-text deduplication
and local checkpoints. Reuse does not certify semantic correctness or authority.
For each application cache define a key covering source revision, query, relevant configuration,
identity and authorization scope. Specify expiration/invalidation and whether failure permits a
stale response. Never reuse protected data across principals merely because prompts match.

Report comparable task outcomes, elapsed time and observable costs, including misses and retries.
Judge accepted-task quality first and include all worker/coordinator work, integration,
cache misses, retries and new-session rehydration where observed. Record uncached input,
cached input, cache writes and output when exposed. Monetary comparisons require actual model
identity and applicable dated billing rates or observed billing; otherwise cost is unknown.
High cache use neither proves low total cost nor rules out a justified handoff.
If the host exposes no cost or cache telemetry, say so. Optimization guidance does not establish
an implemented cache engine or measurable model benefit by itself.
