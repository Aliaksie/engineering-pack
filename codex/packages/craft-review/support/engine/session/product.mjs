import { acceptanceCoverage } from './acceptance.mjs';

// Focus selects existing constraints; it cannot introduce requirements or observations.
export function productProjection({ contract, evidence, current, focus = {}, blockers = false }, view) {
  if (!contract) throw new Error('product:missing-contract');
  const coverage = acceptanceCoverage(contract, evidence, current, blockers);
  const fields = ['boundaries', 'dependencies', 'consequences'];
  if (!focus || typeof focus !== 'object' || Array.isArray(focus)
    || Object.keys(focus).some(key => !fields.includes(key))) throw new Error('product:invalid-focus');
  const selected = Object.fromEntries(fields.map(field => {
    const ids = Object.hasOwn(focus, field) ? focus[field] : [];
    if (!Array.isArray(ids) || ids.length > 32 || new Set(ids).size !== ids.length
      || ids.some(id => typeof id !== 'string' || !contract.context.constraints.some(entry => entry.id === id))) {
      throw new Error('product:invalid-reference');
    }
    return [field, ids.map(id => {
      const constraint = contract.context.constraints.find(entry => entry.id === id);
      return { ...constraint, sources: constraint.sourceIds.map(sourceId => contract.context.sources.find(source => source.id === sourceId)) };
    })];
  }));
  const shared = { assurance: coverage.assurance, contract: coverage.contract, binding: current,
    intent: contract.intent, exclusions: contract.exclusions, sources: contract.context.sources,
    constraints: contract.context.constraints, questions: contract.context.questions,
    decisions: contract.decisions, discoveryCoverage: contract.context.coverage,
    criteria: contract.criteria.map(criterion => ({ ...criterion,
      acceptance: coverage.criteria.find(entry => entry.id === criterion.id) })), coverage };
  if (view === 'product') return { ...shared, dependencies: selected.dependencies,
    unresolvedDecisions: contract.decisions.filter(decision => !decision.resolved),
    deliveryGaps: coverage.criteria.filter(criterion => criterion.status !== 'verified') };
  return { ...shared, ...selected, adrSources: contract.context.sources.filter(source => source.kind === 'adr'),
    evidenceGaps: coverage.criteria.filter(criterion => criterion.status !== 'verified') };
}
