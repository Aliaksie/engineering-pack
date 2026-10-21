// Storage boundary shared by filesystem inventories, including nested workspaces.
export const privatePath = value => /(^|\/)\.capabilities\/state(?:\/|$)|(^|\/)\.git(?:\/|$)/.test(value);
