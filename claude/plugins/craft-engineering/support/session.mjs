#!/usr/bin/env node
import { sessionAdvice, readSessionInput } from './runtime/session.mjs';
import { readProject } from './runtime/project.mjs';
import { runSession } from '../platform-runtime/session.mjs';
await runSession(sessionAdvice, readSessionInput, readProject, "claude");
