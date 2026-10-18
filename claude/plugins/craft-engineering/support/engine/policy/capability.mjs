// Class suggestions are task heuristics, never vendor rankings, prices or model switches.
export function capabilityAdvice(operation, risks = []) {
  if (typeof operation !== 'string' || !Array.isArray(risks) || risks.some(risk => typeof risk !== 'string')) throw new Error('invalid capability operation/risks');
  if (operation === 'unknown') return { class: 'unknown', execution: 'recommendation-only', reason: 'Operation is unknown.', pricing: 'unknown' };
  const strong = operation === 'architecture' || risks.some(risk => ['architecture-ambiguity', 'ambiguous-debugging', 'distributed-consistency', 'security-sensitive', 'complex-cross-module'].includes(risk));
  const economical = ['locate', 'lookup', 'discover', 'context-report', 'status', 'metadata', 'format', 'extract', 'run-check', 'operation'].includes(operation);
  return { class: strong ? 'strong' : economical ? 'economical' : 'standard',
    execution: economical && !strong ? 'prefer-deterministic-tool' : 'recommendation-only',
    reason: strong ? 'The declared operation requires architecture or difficult cross-boundary reasoning.'
      : economical ? 'Bounded mechanical evidence gathering can use deterministic tools or an economical profile.'
      : 'Routine bounded engineering work normally starts with the configured standard profile.',
    pricing: 'unknown; classes are not measured cost or capability', authorization: 'no model switch or delegation executed' };
}
