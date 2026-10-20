import { executionSupport } from './capabilities.mjs';
// Native controls are recommendations for a specific surface, never activation evidence.
export function setupControls(platform) {
  if (platform === 'codex') return {
    surface: 'Codex terminal CLI only; verify desktop/IDE controls separately',
    commands: { reviewHooks: '/hooks', inspectUsage: '/status', configureUsage: '/statusline' },
    execution: executionSupport(platform)
  };
  return { surface: 'Use the actual host surface and version documentation', commands: {},
    execution: executionSupport(platform) };
}
