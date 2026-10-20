#!/usr/bin/env node
import { sessionStatus } from './application/session/status.mjs';
import { flushTelemetry } from './application/telemetry/observe.mjs';
import { readProject, readInput } from './application/project/configuration.mjs';
import { observeNativeSession, hookDiagnostic } from './application/context-delivery/prepare.mjs';
import { runSession } from '../platform-runtime/session.mjs';
await runSession(sessionStatus, readInput, readProject, "gemini-extension", { run:(root, host, callback) => observeNativeSession(root, host, callback, "craft-stories"), diagnostic:hookDiagnostic });
await flushTelemetry();
