import { acceptanceCoverage } from './acceptance.mjs';
import { byteLength } from '../contracts/paths.mjs';

// Probe detail is a bounded view input, never a second acceptance ledger.
export function qaProjection({ contract, evidence, current, probes, blockers = false }) {
  const coverage = acceptanceCoverage(contract, evidence, current, blockers);
  const fail = () => { throw new Error('qa:invalid-probes'); };
  const text = value => typeof value === 'string' && value.trim() && value.length <= 1024
    && !/[\x00-\x1f\x7f]/.test(value);
  const strings = (values, allowEmpty = false) => Array.isArray(values) && values.length <= 32
    && (allowEmpty || values.length) && values.every(text) && new Set(values).size === values.length;
  if (!contract || !Array.isArray(probes) || probes.length > 32
    || byteLength(JSON.stringify(probes)) > 32768) fail();
  const ids = new Set();
  const projected = probes.map(probe => {
    const fields = ['id', 'criterionIds', 'risk', 'flows', 'preconditions', 'actions', 'expectedBasis', 'evidenceIds'];
    if (!probe || typeof probe !== 'object' || Array.isArray(probe)
      || Object.keys(probe).some(key => !fields.includes(key))
      || !text(probe.id) || ids.has(probe.id) || !text(probe.risk) || !text(probe.expectedBasis)
      || !strings(probe.criterionIds) || !strings(probe.flows) || !strings(probe.preconditions, true)
      || !strings(probe.actions) || !strings(probe.evidenceIds, true)
      || probe.criterionIds.some(id => !contract.criteria.some(criterion => criterion.id === id))) fail();
    ids.add(probe.id);
    const records = probe.evidenceIds.map(id => {
      const record = coverage.records.find(entry => entry.id === id);
      if (!record || record.checkId !== probe.id
        || record.criterionIds.some(criterion => !probe.criterionIds.includes(criterion))) fail();
      return record;
    });
    return { ...probe, records, execution: records.length ? 'recorded' : 'not-run' };
  });
  const criteria = contract.criteria.map(criterion => {
    const records = coverage.records.filter(record => record.criterionIds.includes(criterion.id));
    const currentRecords = records.filter(record => record.freshness === 'current');
    return { ...criterion, acceptance: coverage.criteria.find(entry => entry.id === criterion.id).status,
      probeIds: projected.filter(probe => probe.criterionIds.includes(criterion.id)).map(probe => probe.id),
      evidenceIds: records.map(record => record.id),
      conflict: currentRecords.some(record => record.execution === 'failed')
        && currentRecords.some(record => record.execution === 'passed') };
  });
  return { assurance: coverage.assurance, contract: coverage.contract, binding: current,
    intent: contract.intent, exclusions: contract.exclusions, questions: contract.context.questions,
    sources: contract.context.sources, constraints: contract.context.constraints,
    decisions: contract.decisions, checks: contract.context.checks, criteria, probes: projected,
    unexploredCriterionIds: criteria.filter(criterion => !projected.some(probe => probe.criterionIds.includes(criterion.id)
      && probe.records.some(record => record.freshness === 'current' && record.criterionIds.includes(criterion.id)
        && ['passed', 'failed'].includes(record.execution)))).map(criterion => criterion.id),
    coverage };
}
