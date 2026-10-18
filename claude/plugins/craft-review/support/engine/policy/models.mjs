import { delegation } from '../routing/execution.mjs';
import { validateProject } from '../contracts/project.mjs';

const ROLE_INTENTS = { locator: 'locate', researcher: 'research', implementer: 'implement', reviewer: 'review', planner: 'plan', verifier: 'verify' };
const text = (value) => typeof value === 'string' && value.trim().length > 0 && value.length <= 1024;
const object = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);

// Profile declarations and cited acceptance failures support a recommendation, not a model
// call or a claim of measured capability. The host mapping/delegation plan remains in route().
export function modelAdvice(input, project = { formatVersion: 1 }) {
  validateProject(project);
  if (!object(input) || Object.keys(input).some((key) => !['host', 'role', 'task', 'revision', 'requiredCapabilities', 'attempts', 'capabilityClass', 'currentProfile'].includes(key))) throw new Error('invalid model advice input');
  for (const field of ['host', 'task', 'revision']) if (!text(input[field])) throw new Error(`model advice ${field} required`);
  if (!Object.hasOwn(ROLE_INTENTS, input.role)) throw new Error('unknown model advice role');
  if (input.capabilityClass !== undefined && !['economical', 'standard', 'strong'].includes(input.capabilityClass)) throw new Error('invalid capability class');
  if (input.currentProfile !== undefined && !text(input.currentProfile)) throw new Error('invalid current profile');
  const required = input.requiredCapabilities ?? [];
  if (!Array.isArray(required) || required.length > 32 || required.some((item) => typeof item !== 'string' || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(item))) throw new Error('invalid requiredCapabilities');
  const attempts = input.attempts ?? [];
  if (!Array.isArray(attempts) || attempts.length > 80) throw new Error('attempts must be a bounded array');
  for (const attempt of attempts) {
    if (!object(attempt) || Object.keys(attempt).some((key) => !['id', 'profile', 'task', 'revision', 'outcome', 'evidence'].includes(key))
      || !['id', 'profile', 'task', 'revision', 'evidence'].every((key) => text(attempt[key]))
      || !['quality-failure', 'permission-denied', 'tool-unavailable', 'passed'].includes(attempt.outcome)) throw new Error('attempt requires identity, profile, task, revision, outcome and evidence');
  }
  if (new Set(attempts.map((attempt) => attempt.id)).size !== attempts.length) throw new Error('attempt IDs must be unique');
  const applicable = attempts.filter((attempt) => attempt.task === input.task && attempt.revision === input.revision);
  const base = { formatVersion: 1, host: input.host, role: input.role, task: input.task, revision: input.revision,
    requiredCapabilities: required, ignoredAttempts: attempts.length - applicable.length,
    evidencePolicy: 'Capabilities are consumer declarations; cited attempt outcomes are not independently judged. Stale/other-task attempts do not justify escalation.',
    authorization: 'recommendation-only; no model switch, worker, retry or tool permission was executed' };
  const blocker = applicable.find((attempt) => ['permission-denied', 'tool-unavailable'].includes(attempt.outcome));
  if (blocker) return { ...base, decision: 'stop', profile: blocker.profile, reasons: [`Resolve ${blocker.outcome}; changing models does not remove this boundary.`], evidence: [blocker.evidence] };
  const reasons = [];
  const profiles = project.execution?.profiles ?? {};
  let profile = project.execution?.roles?.[input.role]?.profile;
  if (!profile) return { ...base, decision: 'unavailable', reasons: ['Role has no configured logical profile.'] };
  for (let step = 0; step < 16; step++) {
    const definition = profiles[profile];
    if (!definition) return { ...base, decision: 'unavailable', profile, reasons: [...reasons, 'Profile capability metadata is undeclared.'] };
    const evidence = applicable.filter((attempt) => attempt.profile === profile);
    if (evidence.some((attempt) => attempt.outcome === 'passed')) return { ...base, decision: 'completed', profile, reasons: [...reasons, 'Current-target acceptance is recorded passed; no escalation needed.'], evidence: evidence.filter((attempt) => attempt.outcome === 'passed').map((attempt) => attempt.evidence) };
    if (input.capabilityClass !== undefined && definition.class === undefined) return { ...base, decision: 'unavailable', profile, reasons: [...reasons, 'Requested capability class has no declared profile class mapping.'] };
    const classes = ['economical', 'standard', 'strong'];
    const classMissing = input.capabilityClass !== undefined && definition.class !== undefined
      && classes.indexOf(definition.class) < classes.indexOf(input.capabilityClass);
    const missing = required.filter((capability) => !definition.capabilities.includes(capability));
    const exhausted = evidence.filter((attempt) => attempt.outcome === 'quality-failure').length >= (definition.maxAttempts ?? 1);
    if (missing.length || exhausted || classMissing) {
      reasons.push(classMissing ? `Profile ${profile} is below the requested ${input.capabilityClass} capability class.` : missing.length ? `Profile ${profile} lacks declared capabilities: ${missing.join(', ')}.` : `Profile ${profile} reached its configured quality-attempt limit; cited evidence: ${evidence.map((attempt) => attempt.evidence).join(', ')}.`);
      if (!definition.escalateTo) return { ...base, decision: 'stop', profile, reasons: [...reasons, 'No further escalation is configured.'] };
      profile = definition.escalateTo;
      continue;
    }
    const execution = delegation(project, input.role, input.host, profile);
    return { ...base, decision: execution.mode === 'delegate' ? 'recommend' : 'unavailable', profile,
      execution, capabilityClass: definition.class ?? 'unknown',
      currentProfile: input.currentProfile ?? null,
      comparison: input.currentProfile === undefined ? 'unknown' : input.currentProfile === profile ? 'already-selected' : 'configured-profile-recommended; prices and actual capability are not measured',
      attemptsRemaining: (definition.maxAttempts ?? 1) - evidence.length,
      reasons: [...reasons, ...(execution.mode !== 'delegate' ? ['Role and host must explicitly support delegation and an exact model mapping.'] : [])] };
  }
  return { ...base, decision: 'stop', reasons: [...reasons, 'Profile chain limit reached.'] };
}


export { capabilityAdvice } from './capability.mjs';
