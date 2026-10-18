import { commands } from './commands.mjs';
export { commands };
export function invocation(words) {
  let index = 0;
  const environment = new Map();
  while (/^[A-Za-z_][A-Za-z_0-9]*=/.test(words[index] ?? '')) {
    const split = words[index].indexOf('='); environment.set(words[index].slice(0, split), words[index].slice(split + 1)); index++;
  }
  const executable = (words[index++] ?? '').split(/[\\/]/).at(-1).replace(/\.exe$/i, '');
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
