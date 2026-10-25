import { nativeControl } from '../platform/index.mjs';

// Host callbacks retain only opaque native tickets. The pid belongs to this calling
// transport process, rather than the short-lived craft process serving each phase.
export const assertNativeSource = (root, source, session, task) => nativeControl({ phase: 'source', root, source, session, task });
export async function withTaskAdmission(input, effect) {
  const base = { root: input.root, session: input.session, task: input.task, ...(input.source === undefined ? {} : { source: input.source }) };
  const ticket = nativeControl({ ...base, phase: 'admission-acquire', pid: process.pid });
  try { return await effect(() => nativeControl({ ...base, ticket, phase: 'admission-check' })); }
  finally { nativeControl({ ...base, ticket, phase: 'admission-release' }); }
}
export function nativeJournal({ root, session, task, host, coordinator, mode }) {
  const base = { root, session, task, host, coordinator, mode };
  const opened = nativeControl({ ...base, phase: 'open', pid: process.pid });
  const journal = opened.journal;
  let revision = opened.revision;
  const call = (phase, input = {}) => nativeControl({ ...base, ticket: opened.ticket, expected: revision, phase, ...input });
  return { context: opened.context, journal,
    persist(withdrawn) { const result = call('persist', { journal, withdrawn }); revision = result.revision; },
    begin(input, kind, nativeId = null) {
      const result = call('begin', { input, kind, nativeId }); revision = result.revision;
      const op = result.journal.operations.at(-1); journal.operations.push(op); return op;
    },
    close() { call('close'); }
  };
}
