import { normalizeScope, privatePath } from './contracts/paths.mjs';
import { digest } from './cache/identity.mjs';

export function repositoryScopes(value) {
  if (!Array.isArray(value) || value.length < 1 || value.length > 8
    || value.some(item => typeof item !== 'string' || item.length > 1024 || /[\x00-\x1f\x7f]/.test(item))) throw new Error('repositories require 1..8 workspace-relative directories');
  const scopes = value.map(normalizeScope).sort();
  if (scopes.some(privatePath)) throw new Error('private workspace cannot be a repository scope');
  if (new Set(scopes).size !== scopes.length) throw new Error('duplicate repository scope');
  return scopes;
}

export function repositoryCheckout(entries) {
  return `workspace:${digest(entries.map(({ path, checkout }) => [path, checkout]))}`;
}

export function repositorySnapshot(entries) {
  const paths = repositoryScopes(entries.map(entry => entry.path));
  const repositories = paths.map(path => entries.find(entry => entry.path === path));
  const fingerprint = field => digest(repositories.map(entry => [entry.path, entry.head, entry.checkout, entry.fingerprints[field]]));
  return {
    repositories,
    checkout: repositoryCheckout(repositories),
    // This identifies a set of revisions; it is deliberately not a Git commit SHA.
    head: `workspace:${digest(repositories.map(entry => [entry.path, entry.head]))}`,
    status: repositories.flatMap(entry => entry.status.map(status => `${entry.path}: ${status}`)),
    gitAvailable: repositories.every(entry => entry.gitAvailable),
    fingerprints: {
      index: fingerprint('index'), workingTree: fingerprint('workingTree'),
      complete: repositories.every(entry => entry.fingerprints.complete),
      issues: repositories.flatMap(entry => entry.fingerprints.issues.map(issue => `${entry.path}: ${issue}`)),
      files: repositories.reduce((sum, entry) => sum + entry.fingerprints.files, 0),
      bytes: repositories.reduce((sum, entry) => sum + entry.fingerprints.bytes, 0)
    }
  };
}
