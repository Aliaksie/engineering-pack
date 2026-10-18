// Host observations and controls live here; all decision policy stays in runtime/session.mjs.
export function sessionCapabilities(platform) {
  if (platform === 'claude') return { observation: 'statusline-current-usage', display: 'optional-native-statusline', commands: { compact: '/compact', 'start-new-session': '/clear' } };
  if (platform === 'codex') return { observation: 'native-ui-only; no stable usage field in mapped hooks', display: 'native /statusline; explicit CLI advice fallback', commands: { inspectUsage: '/statusline', compact: '/compact', 'start-new-session': '/new' } };
  return { observation: 'unavailable-in-current-adapter', display: 'explicit CLI advice fallback', commands: {} };
}
export function sessionObservation(payload, platform, now = Date.now()) {
  if (platform !== 'claude') return undefined;
  const window = payload?.context_window;
  const current = window?.current_usage;
  if (!current) return undefined;
  const counts = [current.input_tokens, current.cache_creation_input_tokens, current.cache_read_input_tokens];
  if (!counts.every((value) => Number.isSafeInteger(value) && value >= 0)) return undefined;
  return { kind: 'current-context', currentTokens: counts.reduce((sum, value) => sum + value, 0),
    capacityTokens: window.context_window_size, sessionId: payload.session_id,
    source: 'claude-statusline.current_usage (input and cache tokens; excludes output)', observedAt: new Date(now).toISOString() };
}
export function renderSession(advice, platform) {
  const { usage, suggestion } = advice;
  const status = usage.status === 'observed'
    ? `Context ${usage.currentTokens}/${usage.capacityTokens} tokens (${Math.round(usage.usedRatio * 100)}%)`
    : 'Context usage unknown';
  if (!suggestion) return status;
  const command = sessionCapabilities(platform).commands[suggestion.action];
  return `${status} | Next: ${command ?? suggestion.action}${suggestion.checks ? ` (${suggestion.checks.length} declared check(s); inspect JSON for argv)` : ''}`;
}
export async function runSession(advisor, readInput, readProject, platform) {
  try {
    let data, root = process.cwd(), json = false, nativeStdin = false;
    const args = process.argv.slice(2);
    const seen = new Set();
    for (let index = 0; index < args.length; index++) {
      const flag = args[index];
      if (!['--data', '--root', '--json', '--native-stdin'].includes(flag) || seen.has(flag)) throw new Error('usage: session.mjs [--data task.json] [--root project] [--json] [--native-stdin]');
      seen.add(flag);
      if (flag === '--json') json = true;
      else if (flag === '--native-stdin') nativeStdin = true;
      else {
        const value = args[++index];
        if (!value || value.startsWith('--')) throw new Error(`missing ${flag} value`);
        if (flag === '--data') data = readInput(value); else root = value;
      }
    }
    // Only Claude has a verified status-line payload contract here. Other adapters accept
    // explicit normalized data and never scrape private transcripts to invent a token count.
    let input = data ?? {};
    if (nativeStdin && platform !== 'claude') throw new Error('native usage input is unsupported by this adapter');
    if ((!data || nativeStdin) && platform === 'claude') {
      const chunks = []; let bytes = 0;
      for await (const chunk of process.stdin) {
        bytes += chunk.length;
        if (bytes > 65536) throw new Error('statusline payload exceeds size limit');
        chunks.push(chunk);
      }
      const payload = JSON.parse(Buffer.concat(chunks).toString('utf8'));
      if (data && data.sessionId !== payload.session_id) throw new Error('task record does not identify this native session');
      input = { ...input, sessionId: payload.session_id, usage: sessionObservation(payload, platform) };
    }
    const advice = advisor(input, readProject(root));
    process.stdout.write(`${json ? JSON.stringify({ ...advice, host: sessionCapabilities(platform) }, null, 2) : renderSession(advice, platform)}\n`);
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
}
