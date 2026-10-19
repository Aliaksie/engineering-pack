// Composition root for concrete runtime ports. Use cases depend on these
// bounded operations; Node implementation details stay in node/.
export { stateFile, readState, lockState, releaseState, writeState } from './node/state-files.mjs';
export { gitState as repositoryState } from './node/repository/snapshot.mjs';
export { scan } from './node/search.mjs';
export { inventoryIdentity } from './node/repository/inventory.mjs';
export { contextFiles } from './node/resources/context.mjs';
export { projectData } from './node/resources/project.mjs';
export { readSessionInput } from './node/resources/session-input.mjs';
export { realRoot, relativePath, bundleRoot, clock, uniqueId, projectRoot, catalogData, sourceIdentity } from './node/environment.mjs';
export { reviewSnapshot } from './node/repository/review.mjs';
export { runtimeVersion } from './node/environment.mjs';
export { telemetryConfiguration, telemetryIdentity, appendTelemetry, readTelemetry, postTelemetry,
  telemetryContext, runTelemetryContext, telemetryRoot } from './node/telemetry.mjs';
