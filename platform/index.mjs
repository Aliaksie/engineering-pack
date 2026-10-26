// Composition root for concrete runtime ports. Use cases depend on these
// bounded operations; Node implementation details stay in node/.
export { stateFile, stateLocation, readState, lockState, releaseState, writeState } from './node/state-files.mjs';
export { gitState as repositoryState, gitCheckout as repositoryCheckout } from './node/repository/snapshot.mjs';
export { repositoryRoots } from './node/repository/roots.mjs';
export { projectData, preferenceData, preferenceFile } from './node/resources/project.mjs';
export { bundleRoot, clock, uniqueId, projectRoot, catalogData, sourceIdentity } from './node/environment.mjs';
export { runtimeVersion } from './node/environment.mjs';
export { TELEMETRY_BATCH_EVENTS, telemetryConfiguration, telemetryIdentity, appendTelemetry, appendTelemetryBatch, readTelemetry, postTelemetry,
  telemetryContext, runTelemetryContext, telemetryRoot } from './node/telemetry.mjs';
