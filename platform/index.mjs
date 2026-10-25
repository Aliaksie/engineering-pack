// Composition root for concrete runtime ports. Use cases depend on these
// bounded operations; Node implementation details stay in node/.
export { stateFile, stateLocation, readState, lockState, releaseState, writeState } from './node/state-files.mjs';
export { gitState as repositoryState, gitCheckout as repositoryCheckout } from './node/repository/snapshot.mjs';
export { repositoryRoots } from './node/repository/roots.mjs';
export { workspaceOperation } from './node/workspace.mjs';
export { contextFiles, groundingFiles } from './node/resources/context.mjs';
export { projectData, preferenceData, preferenceFile } from './node/resources/project.mjs';
export { readSessionInput } from './node/resources/session-input.mjs';
export { realRoot, bundleRoot, clock, uniqueId, projectRoot, catalogData, sourceIdentity } from './node/environment.mjs';
export { reviewSnapshot } from './node/repository/review.mjs';
export { runtimeVersion } from './node/environment.mjs';
export { rpcProcess } from './node/rpc-process.mjs';
export { projectEvidenceProcess } from './node/project-evidence.mjs';
export { nativeCommand, nativeLocate, nativeHook } from './node/native-command.mjs';
export { TELEMETRY_BATCH_EVENTS, telemetryConfiguration, telemetryIdentity, appendTelemetry, appendTelemetryBatch, readTelemetry, readTelemetrySnapshot, postTelemetry,
  telemetryContext, runTelemetryContext, telemetryRoot } from './node/telemetry.mjs';
