import { observe, withTelemetry } from '../telemetry/observe.mjs';
import { stateLocation, projectData, preferenceData, preferenceFile, readSessionInput } from '../../platform/index.mjs';
import { decide } from '../engine-client.mjs';
export const validateProject = project => decide('effective-settings', { project }).settings;
export const readProject = (root, options) => withTelemetry(root, {}, () => observe('project.loaded', () => readEffectiveSettings(root, options).settings, () => ({}), { failureCode:'CONFIG_INVALID' }));
export const readInput = file => observe('input.loaded', () => readSessionInput(file), () => ({}), { failureCode:'INVALID_INPUT' });

export function readEffectiveSettings(root, { preferencesFile = preferenceFile(), overrides = {} } = {}) {
  const result = decide('effective-settings', { project: projectData(root), preferences: preferenceData(preferencesFile), overrides });
  let state;
  try { state = stateLocation(root); }
  catch (error) { state = { status: 'unavailable', reason: error.message }; }
  return { ...result, state };
}
