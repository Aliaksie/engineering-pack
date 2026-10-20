import { decide } from '../application/engine-client.mjs';
import { HOSTS } from './registry.mjs';

const native = new Set(['codex', 'claude', 'cursor', 'copilot', 'gemini', 'gemini-extension']);
export function executionSupport(host) {
  if (!Object.hasOwn(HOSTS, host)) throw new Error('unknown host adapter');
  const indirect = host === 'jetbrains' || host === 'agents';
  return { surface: indirect ? 'explicit underlying-agent binding; guidance-only without one' : host === 'codex' ? 'owned Codex App Server client'
    : host === 'claude' ? 'consumer-supplied official Claude Agent SDK query' : 'owned capability-negotiated ACP v1 client',
  entry: 'support/hosts/control.mjs', nativeHost: indirect ? null : host,
  workers: native.has(host) ? 'bounded read-only native sessions; exact model and supported mode required' : 'underlying-agent-dependent',
  review: native.has(host) ? 'scoped native review/task; no semantic verdict inferred' : 'underlying-agent-dependent',
  compaction: native.has(host) ? 'native completion plus source/checkpoint-bound resume acceptance; capability-dependent' : 'underlying-agent-dependent',
  activation: 'explicit owned client and authority verification; no automatic attachment, installation or trust',
  liveVerification: 'not-established-by-export-or-protocol-tests' };
}
export function featureSupport(host) {
  const execution = executionSupport(host), mappings = HOSTS[host].events;
  const contextHooks = Object.values(mappings).some(mapping => mapping.context);
  const features = Object.fromEntries(Object.keys(decide('feature-report', { state: {}, repository: {} }).features).map(feature => [feature,
    { support: 'portable-command-and-guidance', activation: 'explicit command or agent workflow; host execution required for CLI', evidence: 'runtime receipts or explicit declarations; export alone proves no use' }]));
  for (const key of ['routing', 'hint-deduplication']) features[key] = { support: contextHooks ? 'mapped-hooks-and-portable-command' : 'portable-command-and-guidance',
    activation: contextHooks ? 'native hook discovery/trust required; explicit CLI fallback' : 'no registered context-injection hooks; explicit invocation', evidence: 'actual delivered/reused context; selection alone is insufficient' };
  for (const key of ['delegation', 'review', 'compaction']) features[key] = { support: native.has(host) ? 'explicit-native-binding' : 'underlying-agent-dependent',
    activation: execution.activation, evidence: key === 'compaction' ? 'native completion, matching checkpoint/resume and acceptance reference' : 'scoped decision, native lifecycle/output and separate acceptance' };
  features['context-accounting'] = { support: 'portable-estimate-and-native-observation', activation: 'fresh matching native usage required for occupancy; selected-file estimates remain separate', evidence: 'unknown when current occupancy is unavailable' };
  features.evaluation = { support: 'explicit-frozen-pair-collector', activation: 'caller-owned isolated native runner and acceptance judgments', evidence: 'real outputs and measurements; protocol fixtures are not model evaluation' };
  return { formatVersion: 1, adapter: host, features, execution,
    limitations: host === 'jetbrains' ? ['Native AI Chat project rules do not provide lifecycle/tool control; bind the actual underlying agent when available.']
      : host === 'agents' ? ['AGENTS is a portable guidance layout, not a universal execution protocol; bind a real supported host explicitly.']
      : ['Native controls require an owning integration; installing hooks or copying runtime files does not connect it.', 'Missing native model/mode/command/resume support fails closed; write-capable workers are not enabled.'] };
}
