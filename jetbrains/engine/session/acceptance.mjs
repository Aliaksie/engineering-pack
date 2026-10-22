import { digest } from '../cache/identity.mjs';
import { byteLength, normalizeScope } from '../contracts/paths.mjs';

// Payload versions are independent of the package and existing session formats.
const fail = reason => { throw new Error(`acceptance:${reason}`); };
function object(value, fields) {
  if (!value || typeof value !== 'object' || Array.isArray(value)
    || Object.keys(value).some(key => !fields.includes(key))) fail('invalid-fields');
}
function text(value, max = 512) {
  if (typeof value !== 'string' || !value.trim() || value.length > max || /[\x00-\x1f\x7f]/.test(value)) fail('invalid-text');
}
function choice(value, values) { if (!values.includes(value)) fail('invalid-value'); }
function list(value, max, visit) {
  if (!Array.isArray(value) || value.length > max) fail('capacity');
  value.forEach(visit);
}
function unique(values) { if (new Set(values).size !== values.length) fail('duplicate-id'); }
function ids(value, max = 32) { list(value, max, entry => text(entry, 128)); unique(value); }
function scopes(value) {
  list(value, 32, entry => { text(entry); if (normalizeScope(entry) !== entry) fail('invalid-scope'); });
  unique(value);
  if (!value.length) fail('missing-scope');
}
function timestamp(value) {
  if (typeof value !== 'string' || !/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{3})?Z$/.test(value)
    || !Number.isFinite(Date.parse(value))) fail('invalid-time');
}
function hash(value) { if (typeof value !== 'string' || !/^[a-f0-9]{64}$/.test(value)) fail('invalid-digest'); }
function version(value) { if (value.formatVersion !== 1) fail('unsupported-version'); }
function size(value) { if (byteLength(JSON.stringify(value)) > 32768) fail('capacity'); }
function references(values, known) {
  ids(values);
  if (values.some(id => !known.includes(id))) fail('unknown-reference');
}
function source(value) {
  object(value, ['id', 'kind', 'reference', 'identity', 'observedAt', 'scope', 'provenance', 'status']);
  text(value.id, 128); text(value.reference, 1024);
  choice(value.kind, ['request', 'instruction', 'configuration', 'code', 'check', 'adr', 'external']);
  if (value.identity !== null) text(value.identity);
  timestamp(value.observedAt); scopes(value.scope);
  choice(value.provenance, ['declared', 'observed', 'inferred']);
  choice(value.status, ['accepted', 'proposed', 'superseded', 'unknown']);
}
function constraint(value, sources, known) {
  object(value, ['id', 'description', 'sourceIds', 'authority', 'status', 'scope', 'required', 'conflicts']);
  text(value.id, 128); text(value.description); references(value.sourceIds, sources);
  if (!value.sourceIds.length) fail('missing-basis');
  choice(value.authority, ['request', 'project', 'observation']);
  choice(value.status, ['accepted', 'proposed', 'superseded', 'unknown']);
  scopes(value.scope);
  if (typeof value.required !== 'boolean') fail('invalid-value');
  references(value.conflicts, known);
  if (value.conflicts.includes(value.id)) fail('invalid-conflict');
}

export function validateTaskContext(value) {
  object(value, ['formatVersion', 'scope', 'sources', 'constraints', 'checks', 'questions', 'coverage']);
  version(value); size(value); scopes(value.scope);
  list(value.sources, 32, source); unique(value.sources.map(entry => entry.id));
  const sources = value.sources.map(entry => entry.id);
  list(value.constraints, 32, entry => constraint(entry, sources, value.constraints.map(item => item.id)));
  unique(value.constraints.map(entry => entry.id));
  list(value.checks, 32, entry => {
    object(entry, ['id', 'description', 'sourceIds', 'scope']);
    text(entry.id, 128); text(entry.description); references(entry.sourceIds, sources); scopes(entry.scope);
  });
  unique(value.checks.map(entry => entry.id));
  list(value.questions, 32, entry => text(entry));
  choice(value.coverage, ['complete', 'incomplete', 'unknown']);
  return value;
}

export function validateEngineeringContract(value) {
  object(value, ['formatVersion', 'intent', 'exclusions', 'context', 'criteria', 'decisions', 'derivation']);
  version(value); size(value); text(value.intent); list(value.exclusions, 32, entry => text(entry));
  validateTaskContext(value.context);
  const sources = value.context.sources.map(entry => entry.id), checks = value.context.checks.map(entry => entry.id);
  list(value.criteria, 32, entry => {
    object(entry, ['id', 'description', 'required', 'scope', 'sourceIds', 'checkIds', 'evidenceKind']);
    text(entry.id, 128); text(entry.description); scopes(entry.scope);
    if (typeof entry.required !== 'boolean') fail('invalid-value');
    references(entry.sourceIds, sources); references(entry.checkIds, checks);
    if (!entry.sourceIds.length) fail('missing-basis');
    choice(entry.evidenceKind, ['check', 'manual']);
  });
  unique(value.criteria.map(entry => entry.id));
  if (!value.criteria.length) fail('missing-criteria');
  list(value.decisions, 32, entry => {
    object(entry, ['id', 'description', 'blocking', 'resolved']);
    text(entry.id, 128); text(entry.description);
    if (typeof entry.blocking !== 'boolean' || typeof entry.resolved !== 'boolean') fail('invalid-value');
  });
  unique(value.decisions.map(entry => entry.id));
  object(value.derivation, ['reporter', 'method', 'observedAt']);
  text(value.derivation.reporter); timestamp(value.derivation.observedAt);
  choice(value.derivation.method, ['declared', 'observed', 'inferred']);
  return value;
}

const canonical = value => Array.isArray(value) ? value.map(canonical)
  : value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])])) : value;
export function engineeringContractDigest(value) { return digest(canonical(validateEngineeringContract(value))); }

function binding(value) {
  object(value, ['kind', 'identity']);
  choice(value.kind, ['repository', 'environment']);
  if (value.identity !== null) text(value.identity);
}
export function validateAcceptanceEvidence(value) {
  object(value, ['formatVersion', 'records']); version(value); size(value);
  list(value.records, 64, entry => {
    object(entry, ['id', 'criterionIds', 'scope', 'contract', 'binding', 'checkId', 'kind', 'artifact', 'observedAt', 'reporter', 'basis', 'support', 'execution']);
    text(entry.id, 128); ids(entry.criterionIds); scopes(entry.scope); hash(entry.contract); binding(entry.binding);
    if (!entry.criterionIds.length) fail('missing-criteria');
    text(entry.checkId, 128); choice(entry.kind, ['check', 'manual']);
    text(entry.artifact, 1024); timestamp(entry.observedAt); text(entry.reporter); text(entry.basis);
    choice(entry.support, ['verified', 'observed', 'inferred', 'unknown']);
    choice(entry.execution, ['passed', 'failed', 'not-run', 'not-available']);
  });
  unique(value.records.map(entry => entry.id));
  return value;
}

// Pure applicability over supplied observations; this does not verify artifacts or prose.
export function acceptanceCoverage(contract, evidence, current, blockers = false) {
  if (!contract) return { assurance: 'declared', coverage: 'unknown', complete: false, criteria: [] };
  validateEngineeringContract(contract); validateAcceptanceEvidence(evidence); binding(current);
  if (typeof blockers !== 'boolean') fail('invalid-value');
  const fingerprint = engineeringContractDigest(contract);
  const records = evidence.records.map(entry => ({ ...entry, freshness: entry.contract !== fingerprint ? 'stale'
    : entry.binding.identity === null || current.identity === null ? 'unknown'
      : entry.binding.kind === current.kind && entry.binding.identity === current.identity ? 'current' : 'stale' }));
  const known = contract.criteria.map(entry => entry.id);
  if (records.some(entry => entry.criterionIds.some(id => !known.includes(id)))) fail('unknown-reference');
  const criteria = contract.criteria.map(criterion => {
    const applicable = records.filter(entry => entry.criterionIds.includes(criterion.id) && entry.freshness === 'current'
      && entry.kind === criterion.evidenceKind && criterion.checkIds.includes(entry.checkId)
      && criterion.scope.every(scope => entry.scope.includes(scope)));
    const failed = applicable.some(entry => entry.execution === 'failed');
    const verified = !failed && applicable.some(entry => entry.support === 'verified' && entry.execution === 'passed');
    return { id: criterion.id, required: criterion.required, status: failed ? 'failed' : verified ? 'verified' : 'gap',
      evidenceIds: applicable.map(entry => entry.id) };
  });
  const conflicts = contract.context.constraints.filter(entry => entry.required && entry.status !== 'superseded'
    && (entry.status !== 'accepted' || entry.conflicts.length)).map(entry => entry.id);
  const decisions = contract.decisions.filter(entry => entry.blocking && !entry.resolved).map(entry => entry.id);
  return { assurance: 'attributed', contract: fingerprint, criteria, records, conflicts, decisions,
    complete: !blockers && !conflicts.length && !decisions.length && criteria.every(entry => !entry.required || entry.status === 'verified') };
}
