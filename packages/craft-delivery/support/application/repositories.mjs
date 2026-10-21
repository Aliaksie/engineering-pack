import { decide } from './engine-client.mjs';
import { repositoryRoots, repositoryState, repositoryCheckout } from '../platform/index.mjs';

export function repositorySnapshot(root, repositories, { checkoutOnly = false } = {}) {
  if (repositories === undefined) return checkoutOnly ? { checkout: repositoryCheckout(root) } : repositoryState(root);
  const scopes = decide('repository-scopes', { repositories });
  const entries = repositoryRoots(root, scopes).map(entry => ({ path: entry.path,
    ...(checkoutOnly ? { checkout: repositoryCheckout(entry.root) } : repositoryState(entry.root)) }));
  return checkoutOnly ? { checkout: decide('repository-checkout', { repositories: entries }) }
    : decide('repository-snapshot', { repositories: entries });
}
