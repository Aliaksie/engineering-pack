// Effective settings through the JavaScript engine, for tests of the JS readers it still serves.
import { observe, withTelemetry } from '../application/telemetry/observe.mjs';
import { projectData, preferenceData, preferenceFile } from '../platform/index.mjs';
import { decide } from '../application/engine-client.mjs';
export const validateProject = project => decide('effective-settings', { project }).settings;
export const readProject = (root, { preferencesFile = preferenceFile() } = {}) => withTelemetry(root, {}, () => observe('project.loaded',
  () => decide('effective-settings', { project: projectData(root), preferences: preferenceData(preferencesFile) }).settings, () => ({}), { failureCode:'CONFIG_INVALID' }));
