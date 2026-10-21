// Storage boundary shared by filesystem inventories, including nested workspaces.
export const privatePath = value => /(^|\/)\.(?:craft|capabilities)\/state(?:\/|$)|(^|\/)\.git(?:\/|$)/.test(value);
