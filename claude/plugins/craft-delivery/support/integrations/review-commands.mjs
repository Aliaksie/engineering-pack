import { commands } from './commands.mjs';
// A narrow command guardrail for explicitly configured read-only review sessions.
// Tool-side authorization and an isolated runner remain the enforcement boundary.
// Recognize direct segments, literal shell -c scripts and command substitutions.
// Aliases, variable expansion and arbitrary interpreters still require tool-side restrictions.
function publishes(words, depth) {
  const [executable, ...args] = words;
  if (['sh', 'bash', 'dash', 'zsh', 'ksh', '/bin/sh', '/bin/bash', '/bin/dash', '/bin/zsh', '/bin/ksh'].includes(executable)) {
    const index = args.findIndex(word => /^-[^-]*c/.test(word));
    if (index >= 0) return inspect(args[index + 1] ?? '', depth + 1);
  }
  if (executable === 'git') {
    let index = 0;
    while (index < args.length && args[index].startsWith('-')) {
      const option = args[index++];
      if (['-C', '-c', '--git-dir', '--work-tree', '--namespace', '--config-env', '--super-prefix'].includes(option)) index++;
    }
    return args[index] === 'push';
  }
  if (!['gh', 'glab'].includes(executable)) return false;
  if (['pr', 'mr', 'issue'].includes(args[0]) && ['comment', 'note', 'review', 'approve', 'merge', 'close', 'reopen', 'edit'].includes(args[1])) return true;
  if (args[0] !== 'api') return false;
  return args.some((argument, index) => /\bmutation\b/.test(argument)
    || (/^(?:-X|--method)$/.test(argument) && /^(?:POST|PUT|PATCH|DELETE)$/i.test(args[index + 1] ?? ''))
    || /^(?:-X|--method)=(?:POST|PUT|PATCH|DELETE)$/i.test(argument));
}


function inspect(command, depth = 0) {
  // Bound recursive scripts and substitutions; an opaque deep script is unsafe
  // for this explicitly read-only guardrail.
  if (depth >= 16) return true;
  if (commands({ command }, 'review').some(words => publishes(words, depth))) return true;
  let quote = '';
  for (let index = 0; index < command.length; index++) {
    const character = command[index];
    if (character === '\\' && quote !== "'") { index++; continue; }
    if (character === "'" && quote !== '"') { quote = quote ? '' : "'"; continue; }
    if (quote === "'") continue;
    if (character === '"') { quote = quote ? '' : '"'; continue; }
    if (character !== '`' && !(character === '$' && command[index + 1] === '(')) continue;
    const backtick = character === '`';
    const start = index + (backtick ? 1 : 2);
    let end = start, nesting = 1, innerQuote = '';
    for (; end < command.length; end++) {
      const current = command[end];
      if (current === '\\' && innerQuote !== "'") { end++; continue; }
      if (backtick) { if (current === '`') break; continue; }
      if (innerQuote) { if (current === innerQuote) innerQuote = ''; continue; }
      if (current === "'" || current === '"') { innerQuote = current; continue; }
      if (current === '(') nesting++;
      if (current === ')' && --nesting === 0) break;
    }
    if (inspect(command.slice(start, end), depth + 1)) return true;
    index = end;
  }
  return false;
}

export const publishingCommand = command => typeof command === 'string' && inspect(command);
