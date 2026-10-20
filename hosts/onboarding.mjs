// Native controls are recommendations for a specific surface, never activation evidence.
export function setupControls(platform) {
  if (platform === 'codex') return {
    surface: 'Codex terminal CLI only; verify desktop/IDE controls separately',
    commands: { reviewHooks: '/hooks', inspectUsage: '/status', configureUsage: '/statusline' },
    execution: { surface: 'explicit owned Codex App Server client; not automatic CLI/desktop attachment',
      entry: 'support/hosts/codex/control.mjs', workers: 'read-only threads with bounded briefs',
      review: 'inline review/start on idle coordinator', compaction: 'native lifecycle plus checkpoint/resume verification',
      activation: 'consumer-owned client, authorization and isolated process configuration required' }
  };
  return { surface: 'Use the actual host surface and version documentation', commands: {},
    execution: { surface: 'unavailable-in-current-adapter', activation: 'advice/manual host controls only' } };
}
