import { observe, withTelemetry } from '../telemetry/observe.mjs';
import { projectData, preferenceData, preferenceFile, readSessionInput } from '../../platform/index.mjs';
import { decide } from '../engine-client.mjs';
export const validateProject = project => decide('effective-settings', { project }).settings;
// Effective settings for JS commands; the `settings` command itself runs natively.
export const readProject = (root, { preferencesFile = preferenceFile() } = {}) => withTelemetry(root, {}, () => observe('project.loaded',
  () => decide('effective-settings', { project: projectData(root), preferences: preferenceData(preferencesFile) }).settings, () => ({}), { failureCode:'CONFIG_INVALID' }));
export const readInput = file => observe('input.loaded', () => readSessionInput(file), () => ({}), { failureCode:'INVALID_INPUT' });
