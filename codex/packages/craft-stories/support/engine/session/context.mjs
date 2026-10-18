const MAX_TARGETS = 256;
export function contextRequests(plan) {
  const requests = (plan.loading?.selections ?? []).flatMap((entry) => [...entry.skills, ...entry.references].map((target) => ({ target, selectedBy: entry.id })));
  for (const target of plan.skills ?? []) if (!requests.some((entry) => entry.target === target)) requests.push({ target, selectedBy: 'module:configured-skills' });
  if (requests.length > MAX_TARGETS) throw new Error(`context report supports at most ${MAX_TARGETS} selections`);
  return requests;
}
export function summarizeContext(entries, missing, budgetBytes) {
  if (budgetBytes !== undefined && (!Number.isSafeInteger(budgetBytes) || budgetBytes < 1)) throw new Error('budgetBytes must be a positive safe integer');
  const bytes = entries.reduce((sum, entry) => sum + entry.bytes, 0);
  const repeatedSelections = entries.reduce((sum, entry) => sum + entry.selections - 1, 0);
  const hashes = new Map();
  for (const entry of entries) hashes.set(entry.sha256, [...(hashes.get(entry.sha256) ?? []), entry.path]);
  return {
    formatVersion: 1, scope: 'selected-guidance-files; not live session usage',
    complete: missing.length === 0, files: entries, missing,
    totals: { files: entries.length, bytes, estimatedTokens: entries.reduce((sum, entry) => sum + entry.estimatedTokens, 0), repeatedSelections },
    identicalContent: [...hashes.values()].filter((paths) => paths.length > 1),
    estimator: 'ceil(UTF-8 bytes / 4) per unique path; heuristic, not a tokenizer',
    budget: { bytes: budgetBytes ?? null, status: budgetBytes === undefined ? 'not-configured' : bytes > budgetBytes ? 'exceeded' : missing.length ? 'unknown' : 'within', action: 'report-only; no guidance omitted' },
    sessionUsage: { currentTokens: null, capacityTokens: null, status: 'unknown; no host observation supplied' },
    limits: 'Measures package resources, not host-loaded overlay copies. Includes selected entry files, explicit references and required package instructions once per path. Excludes unloaded links, host skill-discovery metadata, tool schemas, conversation and tool output. Missing files make totals a lower bound.'
  };
}
