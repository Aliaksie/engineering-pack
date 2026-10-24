import fs from 'node:fs';
import assert from 'node:assert/strict';
import test from 'node:test';
import { evaluate } from '../engine/index.mjs';

const baseline = JSON.parse(fs.readFileSync(new URL('./fixtures/runtime/engine-contracts.json', import.meta.url), 'utf8'));

// Listed diagnostic prose may change across implementations; error codes and
// the remaining JSON (omitted fields, ordering and identities) stay contractual.
function semantic(response, diagnosticPaths = []) {
  if (response.ok) {
    const copy = structuredClone(response);
    for (const parts of diagnosticPaths) {
      const parent = parts.slice(0, -1).reduce((value, key) => value[key], copy.result);
      const key = parts.at(-1);
      assert.equal(typeof parent[key], 'string', 'diagnostic field must remain a string');
      delete parent[key];
    }
    return copy;
  }
  const { message, ...error } = response.error;
  return { ...response, error };
}

for (const fixture of baseline.cases) {
  test(`frozen engine contract: ${fixture.id}`, () => {
    const request = structuredClone(fixture.request);
    const response = evaluate(request);
    assert.deepEqual(semantic(response, fixture.diagnosticResultPaths), semantic(fixture.response, fixture.diagnosticResultPaths));
    assert.deepEqual(request, fixture.request, 'evaluation must leave caller data unchanged');
  });
}
