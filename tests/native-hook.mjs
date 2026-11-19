import { spawnSync } from 'node:child_process';

const TOOLS = { edit: 'Edit', read: 'Read', shell: 'Bash', other: 'Task' };

// A routing hint's lines as fields: `intent`, `next`, project `skills`, `answer` and `lookup`,
// the ids of its `standards`, `references` and whatever its `data` line carries; null when
// the text holds no routing hint.
export function routingHint(text) {
  const start = text.indexOf('Routing hints');
  if (start < 0) return null;
  const hint = {};
  for (const line of text.slice(start).split('\n').slice(1)) {
    const field = /^(intent|next|project skills|answer|lookup|read|data): (.*)$/.exec(line);
    if (field) {
      const [, key, value] = field;
      if (key === 'intent') hint.intent = value;
      else if (key === 'next') hint.next = value.replace(/^open /, '');
      else if (key === 'project skills') hint.skills = value.split(', ');
      else if (key === 'read') hint.references = value.split(', ');
      else if (key === 'data') Object.assign(hint, JSON.parse(value));
      else hint[key] = JSON.parse(value);
    } else if (line.startsWith('- ')) {
      (hint.standards ??= []).push({ id: line.slice(2).split(/[: ]/)[0] });
    } else if (line !== 'standards:' && !line.startsWith('  - ')) {
      break;
    }
  }
  return hint;
}

// One delivery hook event through `craft hook`, as a host runs it. `session` holds the session
// lines a started or restored session receives first. `context` is undefined when the host
// receives nothing else; otherwise its `text`, and as `data` the JSON after its leading sentence.
export function deliverHook(cwd, lifecycle, { tool = 'other', file = null, prompt = '', session = null, host = 'codex' } = {}) {
  const payload = { cwd, tool_name: TOOLS[tool], tool_input: file ? { file_path: file } : {}, prompt };
  if (session !== null) payload.session_id = session;
  const args = ['hook', host, lifecycle, '--package', 'craft-delivery', '--handler', 'deliver', '--scope', 'any'];
  const child = spawnSync(process.env.CRAFT_NATIVE_BINARY, args, { cwd, input: JSON.stringify(payload), encoding: 'utf8' });
  if (child.status !== 0) throw new Error(child.stderr);
  const output = child.stdout ? JSON.parse(child.stdout) : {};
  const received = output.hookSpecificOutput?.additionalContext ?? output.additional_context;
  const lines = received ? received.split('\n') : [];
  const isSession = line => line.startsWith('Craft session') || line.startsWith('Unfinished tasks of earlier sessions')
    || line.startsWith('Open work plans') || line.startsWith('Planning evidence of earlier sessions');
  const identity = lines.filter(isSession);
  const text = lines.filter(line => !isSession(line)).join('\n');
  if (!text) return { session: identity, context: undefined };
  const start = text.indexOf(': {');
  const data = routingHint(text) ?? (start < 0 ? null : JSON.parse(text.slice(start + 2)));
  return { session: identity, context: { text, data } };
}
