export const featureIds = ['routing', 'hint-deduplication', 'lookup', 'batching', 'services', 'attempts', 'loop-detection', 'model-advice', 'delegation',
  'checkpoints', 'continuation', 'context-advice', 'next-step', 'compaction', 'context-accounting', 'review', 'telemetry', 'evaluation'];
const statuses = ['used', 'eligible-but-skipped', 'disabled', 'unsupported', 'not-applicable', 'failed'];
const text = value => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
export function validateFeatures(records = {}) {
  if (!records || typeof records !== 'object' || Array.isArray(records) || Object.keys(records).length > featureIds.length) throw new Error('invalid task feature outcomes');
  for (const [feature, record] of Object.entries(records)) {
    if (!featureIds.includes(feature) || !record || Object.keys(record).some(key => !['status', 'reason', 'evidence', 'provenance', 'source', 'at'].includes(key))
      || !statuses.includes(record.status) || !text(record.reason) || !text(record.evidence)
      || !['agent-declared', 'runtime-observed'].includes(record.provenance) || !text(record.at) || !Number.isFinite(Date.parse(record.at))
      || (record.source !== null && !/^[a-f0-9]{64}$/.test(record.source ?? ''))) throw new Error('invalid feature outcome record');
  }
  return records;
}
export function recordFeature(state, input, source, at, provenance = 'agent-declared') {
  if (!state.taskRecord) throw new Error('task feature outcomes require task-start');
  if (!input || typeof input !== 'object' || Object.keys(input).some(key => !['feature', 'status', 'reason', 'evidence'].includes(key)) || !featureIds.includes(input.feature)) throw new Error('unknown feature or invalid feature outcome');
  const { feature, ...record } = input; if (provenance === 'agent-declared' && state.featureOutcomes?.[feature]?.provenance === 'runtime-observed') throw new Error('a runtime-observed feature outcome cannot be replaced by a declaration');
  const next = { ...record, source, at, provenance };
  validateFeatures({ [feature]: next });
  state.featureOutcomes = { ...validateFeatures(state.featureOutcomes), [feature]: next };
  return { state, recorded: { feature, ...next } };
}
export function featureReport(state, source) {
  const records = validateFeatures(state.featureOutcomes);
  const activity = state.activity ?? {};
  const inferred = {
    routing: (activity.routingDecisions ?? 0) > 0 ? ['Routing decisions recorded; resource loading remains unobserved.', 'activity:routingDecisions'] : null,
    lookup: (activity.freshLookups ?? 0) + (activity.reuseHits ?? 0) > 0 ? ['Task discovery receipts exist; inspect reuse/invalidation evidence separately.', 'activity:discoveryRequests'] : null,
    attempts: (activity.operationsAttempted ?? 0) > 0 ? ['Local attempt receipts exist; semantic acceptance is agent-declared.', 'activity:operationsAttempted'] : null
  };
  const features = Object.fromEntries(featureIds.map(feature => {
    const saved = records[feature];
    if (saved) return [feature, { ...saved, sourceStatus: saved.source === null || source === null ? 'unknown' : saved.source === source ? 'current' : 'stale' }];
    if (inferred[feature]) return [feature, { status: 'used', reason: inferred[feature][0], evidence: inferred[feature][1], provenance: 'runtime-receipt', sourceStatus: 'historical' }];
    return [feature, { status: 'unknown', reason: 'No outcome evidence retained for this feature.', provenance: 'unobserved', sourceStatus: 'unknown' }];
  }));
  return { features, allClassified: Object.values(features).every(record => record.status !== 'unknown'),
    limits: 'Task-local observations/declarations, not a semantic verdict or proof of natural workflow adoption. Stale or historical outcomes do not certify current acceptance.' };
}
