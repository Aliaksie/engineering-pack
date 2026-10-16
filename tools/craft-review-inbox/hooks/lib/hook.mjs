import path from 'node:path';

/** Recognize ordinary direct commands only; this is not a shell interpreter. */
export function commands(event) {
  if (Array.isArray(event?.argv) && event.argv.every((word) => typeof word === 'string')) return [event.argv];
  if (typeof event?.command !== 'string' || event.command.length > 65536) return [];
  const result = []; let words = [], word = '', quote = '', escaped = false;
  const token = () => { if (word) words.push(word); word = ''; };
  const command = () => { token(); if (words.length) result.push(words); words = []; };
  for (const character of event.command) {
    if (escaped) { word += character; escaped = false; continue; }
    if (character === '\\' && quote !== "'") { escaped = true; continue; }
    if (quote) { if (character === quote) quote = ''; else word += character; continue; }
    if (character === "'" || character === '"') { quote = character; continue; }
    if (';|&\n'.includes(character)) { command(); continue; }
    if (/\s/.test(character)) token(); else word += character;
  }
  if (quote || escaped) return [];
  command(); return result;
}

export function invocation(words) {
  let index = 0;
  const environment = new Map();
  while (/^[A-Za-z_][A-Za-z_0-9]*=/.test(words[index] ?? '')) {
    const split = words[index].indexOf('='); environment.set(words[index].slice(0, split), words[index].slice(split + 1)); index++;
  }
  const executable = path.basename(words[index++] ?? '').replace(/\.exe$/i, '');
  const args = words.slice(index);
  if (executable !== 'gh') return { executable, args, environment };
  while (args[0]?.startsWith('-')) {
    const flag = args.shift();
    if (['--hostname', '--repo', '-R'].includes(flag)) args.shift();
    else if (!flag.includes('=') && !['--help', '--version'].includes(flag)) return { executable: 'unsupported', args, environment };
  }
  return { executable, args, environment };
}

export const allow = () => ({ decision: 'allow' });
export const reject = (reason) => ({ decision: 'deny', reason });
export function option(args, ...names) {
  for (let index = 0; index < args.length; index++) {
    for (const name of names) {
      if (args[index] === name) return args[index + 1] ?? '';
      if (args[index].startsWith(`${name}=`)) return args[index].slice(name.length + 1);
    }
  }
  return undefined;
}
