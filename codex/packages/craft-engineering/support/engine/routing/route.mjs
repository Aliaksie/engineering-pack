import { delegation } from './execution.mjs';
import { extension } from '../contracts/paths.mjs';
import { capabilityAdvice } from '../policy/capability.mjs';
import { loadingPlan } from './guidance.mjs';
import { relativeFile, validateProject } from '../contracts/project.mjs';
import { planOperation } from './operations.mjs';

const legacyLanguages = { '.java': 'jvm', '.kt': 'jvm', '.kts': 'jvm' };
const member = (file, directory) => directory === '.' || file === directory || file.startsWith(`${directory}/`);

export function matchingModules(project, files) {
  const modules = (project.modules ?? []).map((module) => ({ ...module, path: relativeFile(module.path) }));
  const selected = new Set();
  for (const value of files) {
    const file = relativeFile(value);
    const matches = modules.filter((module) => member(file, module.path));
    const longest = Math.max(...matches.map((module) => module.path.length));
    for (const module of matches) if (module.path.length === longest) selected.add(module);
  }
  return [...selected];
}
export function lookup(project, { file, module } = {}) {
  validateProject(project);
  if (file !== undefined) file = relativeFile(file);
  const selected = module ? (project.modules ?? []).filter((entry) => entry.name === module)
    : file ? matchingModules(project, [file]) : project.modules ?? [];
  if (module && !selected.length) throw new Error(`unknown module: ${module}`);
  return { modules: selected.map((entry) => ({ ...entry, path: relativeFile(entry.path) })), availability: 'declarations-only; verify paths and checks in the actual project', execution: 'deterministic-lookup' };
}

export function route({ intent, files = [], project = { formatVersion: 1 }, host = 'agents', delegate = false, task, risks = [], availableSkills, verificationMode, catalog, bindings } = {}) {
  validateProject(project);
  if (!catalog.intents.includes(intent)) throw new Error(`unknown intent: ${intent}`);
  if (verificationMode !== undefined && (intent !== 'verify' || !['manual', 'automation', 'test-code'].includes(verificationMode))) throw new Error('verificationMode requires verify intent and manual, automation or test-code');
  const changed = [...new Set(files.map(relativeFile))];
  const modules = matchingModules(project, changed);
  // An explicit module is a useful fallback when a task targets its root but has no file list.
  const languages = new Set(changed.map((file) => (legacyLanguages[extension(file)] ?? catalog.extensions[extension(file)])).filter(Boolean));
  const layers = new Set(modules.flatMap((module) => module.layers ?? []));
  if (!modules.length) for (const layer of project.layers ?? []) layers.add(layer);
  if (!changed.length) {
    for (const language of project.languages ?? []) languages.add(language);
    for (const layer of project.layers ?? []) layers.add(layer);
  }
  for (const module of modules) for (const language of module.languages ?? []) languages.add(language);
  for (const file of changed) {
    const layer = catalog.layerExtensions[extension(file)];
    if (layer) layers.add(layer);
  }
  const loading = loadingPlan({ intent, files: changed, modules, project, risks, availableSkills, verificationMode, catalog });
  const skills = new Set(loading.selections.flatMap((entry) => entry.skills));
  const reasons = loading.selections.map((entry) => ({ id: entry.id, selectors: entry.reason, evidence: entry.evidence }));
  if (catalog.stackIntents.includes(intent) && intent !== 'verify') for (const module of modules) for (const skill of module.skills ?? []) skills.add(skill);
  const operations = [];
  const missing = [];
  if (intent === 'status') operations.push(planOperation(project, 'git.status', {}, bindings));
  if (task) {
    try { operations.push(planOperation(project, 'task.read', { id: task }, bindings)); }
    catch (error) { missing.push(error.message); }
  }
  const role = { implement: 'implementer', plan: 'planner', investigate: 'researcher', review: 'reviewer', locate: 'locator', research: 'researcher', verify: 'verifier', explore: 'verifier' }[intent];
  const checks = modules.flatMap((module) => (module.checks ?? []).map((check) => ({ module: module.name, cwd: module.path, ...check, execution: 'not-run' })));
  let execution = { mode: ['locate', 'status'].includes(intent) || (intent === 'run-check' && checks.length) ? 'tool' : 'main', role: role ?? 'coordinator' };
  if (intent === 'run-check' && !checks.length) missing.push('No checks declared for the affected modules; choose verification before planning execution.');
  if (intent === 'locate') reasons.push('Use bounded repository search before asking an LLM to find names.');
  const rule = project.execution?.roles?.[role];
  if (delegate) {
    const hostRule = project.execution?.hosts?.[host];
    if (rule?.mode === 'delegate' && hostRule?.delegation === true) {
      const model = rule.profile ? hostRule.models?.[rule.profile] : undefined;
      if (rule.profile && !model) missing.push(`host ${host} has no model mapping for profile ${rule.profile}`);
      else execution = delegation(project, role, host, rule.profile, execution);
    } else reasons.push('Delegation was not selected: the role and host must both explicitly support it.');
  }
  const selected = [...skills];
  if (availableSkills) for (const skill of selected) if (!availableSkills.includes(skill)) missing.push(`skill not installed: ${skill}`);
  const moduleIds = new Set(modules.map((module) => module.name));
  const impacted = new Set(moduleIds);
  let progress = true;
  while (progress) {
    progress = false;
    for (const module of project.modules ?? []) if (!impacted.has(module.name) && module.dependsOn?.some((id) => impacted.has(id))) { impacted.add(module.name); progress = true; }
  }
  const dependents = [...impacted].filter((id) => !moduleIds.has(id));
  return {
    intent, capability: capabilityAdvice(intent, risks), skills: selected, skillAvailability: availableSkills ? 'checked-against-host-list' : 'not-verified', execution,
    ...(intent === 'verify' ? { taskBoundary: { kind: 'testing-only', mode: verificationMode ?? 'choose-from-request',
      testEdits: verificationMode === 'test-code' ? 'requested-test-scope-only' : 'not-selected', productEdits: 'not-selected', enforcement: 'routing contract; host permissions and original scope remain authoritative' } } : {}),
    files: changed, languages: [...languages], layers: [...layers],
    references: [...new Set(loading.selections.flatMap((entry) => entry.references))],
    loading,
    modules: modules.map(({ name, path, owner }) => ({ name, path, ...(owner ? { owner } : {}) })), affectedDependents: dependents,
    checks,
    operations, missing, reasons,
    authorization: 'route-only; host permissions and original user scope still apply'
  };
}
