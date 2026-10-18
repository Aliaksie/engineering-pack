#!/usr/bin/env node
import { readSessionInput } from './platform/node/resources/session-input.mjs';
import { sessionStatus } from './application/session/status.mjs';
import { readProject } from './application/project/configuration.mjs';
import { runSession } from '../platform-runtime/session.mjs';
await runSession(sessionStatus, readSessionInput, readProject, "codex");
