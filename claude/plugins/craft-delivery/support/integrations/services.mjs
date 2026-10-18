// One registry supplies both invocation plans and availability metadata.
// These are external transport mappings, never host lifecycle controls.
function changeId(params) {
  if (!/^[1-9][0-9]*$/.test(String(params.id))) throw new Error('change id must be a positive integer');
  return String(params.id);
}
function gitDiff(_binding, params) {
  const revisions = [];
  if (params.base !== undefined || params.head !== undefined) {
    for (const name of ['base','head']) if (typeof params[name] !== 'string' || !/^(?!-)[A-Za-z0-9_./~^@{}+-]+$/.test(params[name])) throw new Error('invalid Git revision');
    revisions.push(`${params.base}...${params.head}`);
  }
  return { args: ['diff','--no-ext-diff','--no-textconv','--no-color',...revisions,'--'] };
}
function change(binding, params, operation) {
  const id = changeId(params);
  const github = binding.provider === 'github';
  const repository = binding.host ? github ? `${binding.host}/${binding.repository}` : `https://${binding.host}/${binding.repository}` : binding.repository;
  const args = [github ? 'pr' : 'mr', operation === 'change.read' ? 'view' : 'diff', id, '--repo', repository];
  if (operation === 'change.read') args.push(...(github ? ['--json','number,title,body,baseRefOid,headRefOid'] : ['--output','json']));
  return { args };
}
const definitions = [
  { providers: ['git'], operation: 'git.status', requiredParameters: [], resolve: () => ({ args: ['status','--short'] }) },
  { providers: ['git'], operation: 'git.diff', requiredParameters: [], optionalParameters: ['base','head'], resolve: gitDiff },
  ...['change.read','change.diff'].map(operation => ({ providers: ['github','gitlab'], operation, requiredParameters: ['id'], repository: true, resolve: change }))
];
export function serviceBindings(project, parameters = {}) {
  const result = {};
  for (const binding of [project.services?.git ?? { provider:'git', transport:'cli', command:'git' }, project.services?.codehost, project.services?.tasks]) {
    if (binding?.transport !== 'cli') continue;
    for (const definition of definitions) {
      if (!definition.providers.includes(binding.provider)) continue;
      const { operation, requiredParameters, optionalParameters } = definition;
      const item = { requiredParameters, ...(optionalParameters ? { optionalParameters } : {}) };
      if (definition.repository && !binding.repository) item.missing = item.error = 'codehost repository must be explicit';
      else {
        try { item.mapping = definition.resolve(binding, parameters[operation] ?? {}, operation); }
        catch (error) { item.error = error.message; }
      }
      result[operation] = item;
    }
  }
  return result;
}
