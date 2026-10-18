import fs from 'node:fs';
import path from 'node:path';
import { readProject } from './project.mjs';
import { route } from './router.mjs';
import { prepareServices } from './services.mjs';
import { readCheckpoint, recoverCheckpoint, saveCheckpoint } from './checkpoint.mjs';
import { invalidateRoutingHint } from './hint-cache.mjs';
export { reuseRoutingHint, commitRoutingHint } from './hint-cache.mjs';

function projectRoot(cwd) {
  let directory = path.resolve(cwd);
  for (let depth = 0; depth < 30; depth++) {
    if (fs.existsSync(path.join(directory, '.capabilities/project.json'))) return directory;
    if (fs.existsSync(path.join(directory, '.git'))) break;
    const parent = path.dirname(directory);
    if (directory === parent) break;
    directory = parent;
  }
  return path.resolve(cwd);
}
function intentOf(event, project) {
  if (event.tool === 'edit' && project.routing?.editHints === true) return 'implement';
  if (event.event !== 'before-prompt') return null;
  // Hints only. This does not authorize or execute the classified task.
  const text = event.prompt ?? '';
  if (/^\s*(?:review|\/review)\b/i.test(text)) return 'review';
  if (/^\s*(?:plan|\/plan)\b/i.test(text)) return 'plan';
  if (/^\s*(?:verify|\/verify)\b/i.test(text)) return 'verify';
  if (/^\s*(?:find|locate|where)\b/i.test(text)) return 'locate';
  return null;
}
export function prepareEvent(event) {
  const root = projectRoot(event.cwd);
  const project = readProject(root);
  const result = { ...event, project, routingRoot: root };
  if (event.session && project.routing?.dedupeHints === true && ['session-start', 'before-compact', 'after-compact'].includes(event.event)) {
    try { invalidateRoutingHint(root, event.session, event.host); }
    catch { /* Reuse is advisory; unavailable state never authorizes or blocks work. */ }
  }
  const intent = intentOf(event, project);
  if (event.event === 'session-start' && Object.keys(project.services ?? {}).length) result.services = prepareServices(project);
  if (intent) {
    let file = event.file;
    if (file && path.isAbsolute(file)) file = path.relative(root, file).split(path.sep).join('/');
    result.routing = route({ intent, files: file ? [file] : [], project, host: event.host });
  }
  const session = event.session;
  if (session && project.checkpoint?.enabled === true) {
    if (['after-tool', 'turn-end', 'before-compact'].includes(event.event)) {
      // Store only state/explicit metadata, not prompts, transcripts, tool output or credentials.
      saveCheckpoint(root, session);
    }
    if (['session-start', 'after-compact'].includes(event.event)) result.checkpoint = recoverCheckpoint(root, session);
    else if (event.event === 'before-compact') result.checkpoint = { saved: readCheckpoint(root, session) };
  }
  return result;
}
