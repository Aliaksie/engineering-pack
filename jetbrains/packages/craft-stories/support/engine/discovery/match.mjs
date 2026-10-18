import { normalizeScope as locateScope } from '../contracts/paths.mjs';
const SNIPPET = 240;
function bound(value, name, ceiling) {
  if (!Number.isSafeInteger(value) || value < 1 || value > ceiling) throw new Error(`${name} must be an integer from 1 to ${ceiling}`);
  return value;
}
export function locateRequest({ terms, paths = [], maxFiles = 500, maxMatches = 80, maxBytes = 2_097_152 } = {}) {
  if (!Array.isArray(terms) || !terms.length || terms.length > 32 || terms.some(term => typeof term !== 'string' || !term || term.length > 256 || /[\r\n\0]/.test(term))) throw new Error('terms must contain 1 to 32 nonempty single-line literals of at most 256 characters');
  if (!Array.isArray(paths)) throw new Error('paths must be an array');
  terms = [...new Set(terms)];
  const scopes = [...new Set(paths.map(locateScope))];
  bound(maxFiles, 'maxFiles', 10_000);
  bound(maxMatches, 'maxMatches', 1000);
  bound(maxBytes, 'maxBytes', 16_777_216);
  return { terms, scopes, maxFiles, maxMatches, maxBytes, maxFileBytes: 262144, maxSnippetCharacters: SNIPPET };
}
export function matchLines(content, file, terms, maxMatches) {
  const matches = [];
  const lines = content.split(/\r?\n/);
  for (let index = 0; index < lines.length; index++) {
    const matchedTerms = terms.filter(term => lines[index].includes(term));
    if (!matchedTerms.length) continue;
    const offset = Math.max(0, Math.min(...matchedTerms.map(term => lines[index].indexOf(term))) - 40);
    const text = lines[index].slice(offset, offset + SNIPPET);
    matches.push({ path: file, line: index + 1, text, termIndexes: matchedTerms.map(term => terms.indexOf(term)), snippetTruncated: offset > 0 || offset + text.length < lines[index].length });
    if (matches.length >= maxMatches) { break; }
  }
  return matches;
}
