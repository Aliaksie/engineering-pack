#!/usr/bin/env node
import * as handler from '../hooks/context.mjs';
import { runHook } from '../platform-runtime/hook.mjs';
import { prepareEvent, reuseRoutingHint, commitRoutingHint, hookFailure, hookDefault, flushTelemetry, observeHook, hookDiagnostic } from '../support/application/context-delivery/prepare.mjs';
await runHook(handler.evaluate, 'claude', {"name":"routing","event":"before-tool","tool":"any","handler":"hooks/context.mjs"}, handler.prepare ?? prepareEvent, { observational: handler.observational === true, reuse: reuseRoutingHint, commit: commitRoutingHint, failure: hookFailure, default: hookDefault, flush: flushTelemetry, run: (event, callback) => observeHook({ ...event, packageName: "craft-delivery" }, callback), diagnostic: hookDiagnostic });
