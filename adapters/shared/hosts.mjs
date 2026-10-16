// Native support is explicit: observing an event is not the same as injecting context.
const event = (name, context = false, deny = false, matcher) => ({ name, context, deny, ...(matcher ? { matcher } : {}) });
export const HOSTS = {
  claude: { skills: null, events: {
    'before-tool': event('PreToolUse', true, true), 'after-tool': event('PostToolUse', true),
    'before-prompt': event('UserPromptSubmit', true), 'session-start': event('SessionStart', true, false, 'startup|resume'),
    'before-compact': event('PreCompact'), 'after-compact': event('SessionStart', true, false, 'compact'), 'turn-end': event('Stop')
  } },
  codex: { skills: null, events: {
    'before-tool': event('PreToolUse', true, true), 'after-tool': event('PostToolUse', true),
    'before-prompt': event('UserPromptSubmit', true), 'session-start': event('SessionStart', true, false, 'startup|resume'),
    'before-compact': event('PreCompact'), 'after-compact': event('SessionStart', true, false, 'compact'), 'turn-end': event('Stop')
  } },
  cursor: { skills: '.cursor/skills', config: '.cursor/hooks.json', events: {
    'before-tool': event('preToolUse', false, true), 'after-tool': event('postToolUse', true),
    'before-prompt': event('beforeSubmitPrompt'), 'session-start': event('sessionStart', true),
    'before-compact': event('preCompact'), 'turn-end': event('stop')
  } },
  copilot: { skills: '.github/skills', config: '.github/hooks/craft-capabilities.json', events: {
    'before-tool': event('preToolUse', false, true), 'after-tool': event('postToolUse'),
    'before-prompt': event('userPromptSubmitted'), 'session-start': event('sessionStart', true),
    'before-compact': event('preCompact'), 'turn-end': event('agentStop')
  } },
  gemini: { skills: '.gemini/skills', config: '.gemini/settings.json', events: {
    'before-tool': event('BeforeTool', false, true), 'after-tool': event('AfterTool', true),
    'before-prompt': event('BeforeAgent', true), 'session-start': event('SessionStart', true),
    'before-compact': event('PreCompress'), 'turn-end': event('AfterAgent')
  } },
  agents: { skills: '.agents/skills', events: {} }
};
export function hookMapping(platform, definition) {
  const mapping = HOSTS[platform]?.events[definition.event];
  return mapping ? { ...mapping, event: definition.event, tool: definition.tool, handler: definition.handler, id: definition.name } : null;
}
