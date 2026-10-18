import fs from 'node:fs';
import path from 'node:path';
import { validatePlaybookCatalog } from './validate-catalog.mjs';

export const catalog = validatePlaybookCatalog(JSON.parse(fs.readFileSync(new URL('./catalog.json', import.meta.url), 'utf8')));

// Match each affected module independently so a provider/framework in one module
// cannot borrow a layer declared by a different module.
export function loadingPlan({ intent, files, modules, project, risks, availableSkills, verificationMode }) {
  const stack = catalog.stackIntents.includes(intent) && !(intent === 'verify' && verificationMode === 'manual');
  const missingContext = [];
  const member = (file, scope) => scope.path === '.' || file === scope.path || file.startsWith(`${scope.path}/`);
  const unmatched = files.filter((file) => !modules.some((scope) => member(file, scope)));
  const scopes = modules.length ? [...modules, ...(unmatched.length ? [project] : [])] : [project];
  const contexts = scopes.map((scope) => {
    const scopedFiles = scope === project ? (modules.length ? unmatched : files) : files.filter((file) =>
      member(file, scope) && !modules.some((other) => other.path.length > scope.path.length && member(file, other)));
    const evidence = [{ dimension: 'intents', values: [intent], source: 'request', inferred: false }, { dimension: 'risks', values: risks, source: 'request', inferred: false }];
    const values = {};
    for (const dimension of ['languages', 'runtimes', 'frameworks', 'providers', 'layers']) {
      values[dimension] = [...(scope[dimension] ?? project[dimension] ?? [])];
      if (values[dimension].length) evidence.push({ dimension, values: [...values[dimension]], source: scope[dimension] !== undefined && scope !== project ? `module:${scope.name}` : 'project', inferred: false });
    }
    for (const file of scopedFiles) {
      const language = catalog.extensions[path.extname(file)];
      if (language) {
        // Legacy jvm declarations remain accepted; precise language identity is
        // available in the loading plan even for those consumers.
        values.languages.push(language);
        evidence.push({ dimension: 'languages', values: [language], source: file, inferred: true });
      }
      const layer = catalog.layerExtensions[path.extname(file)];
      if (layer) {
        values.layers.push(layer);
        evidence.push({ dimension: 'layers', values: [layer], source: file, inferred: true });
      }
    }
    for (const language of values.languages) {
      const runtime = catalog.runtimeImplications[language];
      if (runtime && !values.runtimes.includes(runtime)) {
        values.runtimes.push(runtime);
        evidence.push({ dimension: 'runtimes', values: [runtime], source: `language:${language}`, inferred: true });
      }
    }
    if (stack && values.languages.some((language) => ['javascript', 'typescript'].includes(language)) && !values.runtimes.length) {
      missingContext.push(`Runtime is unspecified for ${scope.name ?? 'project'}; JavaScript/TypeScript does not establish Node or browser.`);
    }
    return { values, evidence };
  });
  const selections = [];
  for (const entry of catalog.entries) {
    if (!stack && !entry.when.intents) continue;
    const matched = contexts.filter(({ values }) => Object.entries(entry.when).every(([dimension, choices]) => choices.some((value) => (dimension === 'intents' ? [intent] : dimension === 'risks' ? risks : values[dimension] ?? []).includes(value))));
    if (!matched.length) continue;
    const { value, ...loadingEntry } = entry;
    const substitutions = [];
    const skills = entry.skills.map((primary) => {
      const fallback = entry.fallbackSkills?.[primary];
      if (availableSkills && !availableSkills.includes(primary) && fallback && availableSkills.includes(fallback)) {
        substitutions.push({ unavailable: primary, selected: fallback, reason: 'Declared fallback is installed; primary skill is unavailable.' });
        return fallback;
      }
      return primary;
    });
    selections.push({ ...loadingEntry, skills, ...(substitutions.length ? { substitutions } : {}), evidence: matched.flatMap(({ evidence }) => evidence.filter((item) => Object.hasOwn(entry.when, item.dimension))), reason: entry.when });
  }
  return {
    workflow: selections.find((entry) => entry.id.startsWith('workflow:'))?.id ?? null,
    practices: selections.filter((entry) => entry.id.startsWith('practice:')).map(({ id }) => id),
    specializations: selections.filter((entry) => /^(language|runtime|framework|provider|layer):/.test(entry.id)).map(({ id }) => id),
    selections,
    documentationQuestions: selections.filter((entry) => entry.documentationQuestion).map((entry) => ({ id: entry.id, question: entry.documentationQuestion, status: 'resolve-only-if-affected; pinned-version-required' })),
    unavailable: selections.filter((entry) => entry.unavailable).map(({ id, unavailable }) => ({ id, reason: unavailable })),
    evidence: stack ? contexts.flatMap(({ evidence }) => evidence) : [],
    missingContext: [...missingContext, ...(stack && !files.length ? ['Affected paths are unspecified; project declarations do not establish the affected scope.'] : [])],
    evidencePolicy: 'Declarations and extension inference require repository verification. Existing behavior is evidence, not accepted policy; surface conflicts with project instructions.'
  };
}
