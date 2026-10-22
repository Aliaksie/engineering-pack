import { decide, digest } from '../engine-client.mjs';
import { readProject } from '../project/configuration.mjs';
import { repositorySnapshot } from '../repositories.mjs';
import { groundingFiles, clock } from '../../platform/index.mjs';

// Read-only, opt-in composition; no session writes, service connections or checks.
export function groundedContext(root, input, { preferencesFile } = {}) {
  const request = decide('grounding-request', input);
  const before = repositorySnapshot(root);
  const observations = groundingFiles(root, request.files);
  const project = readProject(root, { preferencesFile });
  const after = groundingFiles(root, request.files);
  const current = repositorySnapshot(root);
  const questions = [...(input.questions ?? [])];
  const stable = digest(observations) === digest(after);
  if (!stable) questions.push('Sources changed during composition; repeat grounding.');
  const binding = { kind: 'repository', identity: decide('task-source', { repository: before }) };
  if (!stable || digest(before) !== digest(current)) { binding.identity = null; questions.push('Repository changed during composition; freshness is unknown.'); }
  const context = decide('task-context-compose', { ...input, scope: request.scope, project, observations: after, observedAt: clock().iso, binding, questions });
  return { context, binding, discovery: input.discovery ?? null,
    authorization: 'Citations and declared checks only; no source text or check command was executed.' };
}
