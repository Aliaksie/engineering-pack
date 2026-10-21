// Inputs use project-relative POSIX paths on every host.
export const extension = file => {
  const name = file.slice(file.lastIndexOf('/') + 1);
  const dot = name.lastIndexOf('.');
  return dot <= 0 ? '' : name.slice(dot);
};
export const normalizeScope = value => {
  if (typeof value !== 'string' || !value || value.startsWith('/') || /^[A-Za-z]:|\\/.test(value) || value.split('/').includes('..')) throw new Error('paths must be relative scopes without traversal');
  return value.split('/').filter(part => part && part !== '.').join('/') || '.';
};
export const byteLength = value => new TextEncoder().encode(value).length;
export const privatePath = value => /(^|\/)\.capabilities\/state(?:\/|$)|(^|\/)\.git(?:\/|$)/.test(value);
