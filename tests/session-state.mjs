// Session records `craft` writes, read and edited as test fixtures; never included in runtime exports.
import fs from 'node:fs';
import { digest, readJson, stateFile } from './state.mjs';
import { run } from './craft.mjs';

const empty = () => ({ formatVersion: 1, activity: {}, lookups: {}, attempts: [] });
export const sessionFile = (root, session, task, create = false) => stateFile(root, `activity-${digest([session, task]).slice(0, 24)}.json`, create, session);
export const bindingFile = (root, session, create = false) => stateFile(root, 'task.json', create, session);
export const readSessionState = (root, session, task) => readJson(sessionFile(root, session, task)) ?? empty();

// Another writer's edit, serialized as `craft` writes records.
export function updateSessionState(root, session, task, update) {
  const file = sessionFile(root, session, task, true), state = readJson(file) ?? empty();
  update(state);
  fs.writeFileSync(file, `${JSON.stringify(state)}\n`);
  return state;
}
export function addActivity(state, values) {
  for (const [name, value] of Object.entries(values)) state.activity[name] = (state.activity[name] ?? 0) + value;
  return state;
}

// A writer holding the record's lock until `release()`.
export function holdLock(file) {
  fs.writeFileSync(`${file}.lock`, '', { flag: 'wx' });
  return () => fs.rmSync(`${file}.lock`, { force: true });
}

// The task the session view reports as bound to the current checkout.
export const activeTask = (root, session) => run(['session', '--root', root, '--session', session]).task?.id;
