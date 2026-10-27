import { spawnSync } from 'node:child_process';

const TOOLS = { edit: 'Edit', read: 'Read', shell: 'Bash', other: 'Task' };

// One delivery hook event through `craft hook`, as a host runs it. `session` holds the session
// lines a started or restored session receives first. `context` is undefined when the host
// receives nothing else; otherwise its `text`, and as `data` the JSON after its leading sentence.
export function deliverHook(cwd, lifecycle, { tool = 'other', file = null, session = null, host = 'codex' } = {}) {
  const payload = { cwd, tool_name: TOOLS[tool], tool_input: file ? { file_path: file } : {}, prompt: '' };
  if (session !== null) payload.session_id = session;
  const args = ['hook', host, lifecycle, '--package', 'craft-delivery', '--handler', 'deliver', '--scope', 'any'];
  const child = spawnSync(process.env.CRAFT_NATIVE_BINARY, args, { cwd, input: JSON.stringify(payload), encoding: 'utf8' });
  if (child.status !== 0) throw new Error(child.stderr);
  const received = child.stdout ? JSON.parse(child.stdout).hookSpecificOutput?.additionalContext : undefined;
  const lines = received ? received.split('\n') : [];
  const isSession = line => line.startsWith('Craft session') || line.startsWith('Unfinished tasks of earlier sessions');
  const identity = lines.filter(isSession);
  const text = lines.filter(line => !isSession(line)).join('\n');
  if (!text) return { session: identity, context: undefined };
  const start = text.indexOf(': {');
  return { session: identity, context: { text, data: start < 0 ? null : JSON.parse(text.slice(start + 2)) } };
}
